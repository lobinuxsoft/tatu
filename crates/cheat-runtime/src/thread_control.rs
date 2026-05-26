//! Reference-counted per-thread suspend/resume — 1:1 port of CE's
//! `ceserver/api.c`:
//! - `SuspendThread` (line 1720)
//! - `ResumeThread` (line 1829)
//! - `FindPausedThread` (line 397)
//!
//! # Why this exists
//!
//! Atomic patches in a multi-threaded game require pausing every
//! thread that might execute the bytes being overwritten. CE has a
//! coordinated suspend/resume API with reference counting so nested
//! suspends from different callers don't unstop early; this module
//! ports that API to Rust.
//!
//! # Reference counting
//!
//! Mirrors Windows `SuspendThread` semantics: the first suspend
//! actually stops the thread; subsequent suspends only increment a
//! per-thread counter. Resume decrements; only the resume that takes
//! the count back to 0 actually restarts the thread.
//!
//! # Divergence from CE
//!
//! CE uses `syscall(__NR_tkill, tid, SIGSTOP)` + `WaitForDebugEventNative`
//! because the ceserver runs a single dedicated debugger thread that
//! already owns the trace via `PTRACE_ATTACH` of the whole process.
//! This module is a standalone primitive — the caller doesn't have a
//! preexisting trace — so the first suspend uses `PTRACE_ATTACH` +
//! `waitpid` (which the kernel implements as tkill SIGSTOP internally
//! plus the trace flag), and the last resume uses `PTRACE_DETACH`
//! (which atomically continues the thread + drops the trace).
//!
//! Semantically identical: the thread stops on first suspend, stays
//! stopped while count > 0, runs on last resume. The full event-queue
//! + multi-debugger coordination CE has lives in the debug subsystem
//!   (#142) and will decorate this primitive when it lands.
//!
//! # Relation to `threads::PausedTarget`
//!
//! `threads::PausedTarget` is an RAII whole-process pause (attach
//! every TID once, detach on drop). This module is the per-thread,
//! ref-counted, library-shared primitive CE uses. Both coexist: the
//! RAII path stays for the AOB-patch fast path; this module is the
//! foundation for the upcoming debug subsystem.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::ptrace_helpers::PtraceError;

#[derive(Debug, Clone, Copy)]
struct SuspendEntry {
    /// Owning process — used by [`find_paused_thread`] to filter.
    pid_owner: Pid,
    count: u32,
}

fn state() -> &'static Mutex<HashMap<Pid, SuspendEntry>> {
    static STATE: OnceLock<Mutex<HashMap<Pid, SuspendEntry>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Suspend a thread by TID. Returns the prior suspend count (`0` if
/// this is the first suspend), matching Windows `SuspendThread` /
/// CE `api.c:1720`.
///
/// First-time suspend (count was 0): `PTRACE_ATTACH(tid)` + `waitpid`
/// for the kernel-injected SIGSTOP. Subsequent suspends: bump counter
/// only — no syscall, the thread is already stopped under our trace.
///
/// `pid` is the owning process; recorded so [`find_paused_thread`]
/// can filter by process.
pub fn suspend_thread(pid: Pid, tid: Pid) -> Result<u32, PtraceError> {
    let mut map = state().lock().expect("suspend state poisoned");
    let prior = map.get(&tid).map(|e| e.count).unwrap_or(0);

    if prior == 0 {
        ptrace::attach(tid)?;
        wait_for_sigstop(tid)?;
    }

    map.insert(
        tid,
        SuspendEntry {
            pid_owner: pid,
            count: prior + 1,
        },
    );
    Ok(prior)
}

/// Resume a thread. Returns the new suspend count after decrement
/// (`0` means the thread is now running), matching CE `api.c:1829`.
///
/// Last-resume (count goes 1→0): `PTRACE_DETACH(tid)`, which
/// atomically continues the thread + drops our trace. Returns 0
/// silently for unknown TIDs — CE logs "Invalid thread" and returns
/// -1 there but the higher layers already gate on a real lookup, so
/// the silent path is the useful behaviour for a library primitive.
pub fn resume_thread(_pid: Pid, tid: Pid) -> Result<u32, PtraceError> {
    let mut map = state().lock().expect("suspend state poisoned");
    let count_before = match map.get(&tid) {
        Some(e) => e.count,
        None => return Ok(0),
    };
    if count_before == 0 {
        return Ok(0);
    }
    let new_count = count_before - 1;
    if new_count == 0 {
        ptrace::detach(tid, None)?;
        map.remove(&tid);
    } else if let Some(entry) = map.get_mut(&tid) {
        entry.count = new_count;
    }
    Ok(new_count)
}

/// Return the TID of any thread currently suspended that belongs to
/// `pid`, or `None`. 1:1 port of CE `FindPausedThread` (api.c:397) —
/// the debug subsystem uses it to attach to an already-stopped thread
/// without spending a second SIGSTOP.
pub fn find_paused_thread(pid: Pid) -> Option<Pid> {
    let map = state().lock().expect("suspend state poisoned");
    map.iter()
        .find(|(_, e)| e.pid_owner == pid && e.count > 0)
        .map(|(tid, _)| *tid)
}

/// Current suspend count for `tid` (`0` if not held). Test + debug aid.
pub fn suspend_count(tid: Pid) -> u32 {
    state()
        .lock()
        .expect("suspend state poisoned")
        .get(&tid)
        .map(|e| e.count)
        .unwrap_or(0)
}

/// Drain the global suspend state — test-only escape hatch so a panic
/// in one integration test doesn't poison subsequent ones with stale
/// entries. NOT public.
#[cfg(test)]
fn reset_state_for_tests() {
    state().lock().expect("suspend state poisoned").clear();
}

fn wait_for_sigstop(tid: Pid) -> Result<(), PtraceError> {
    loop {
        match waitpid(tid, None)? {
            WaitStatus::Stopped(stopped, Signal::SIGSTOP) if stopped == tid => return Ok(()),
            WaitStatus::Stopped(other, sig) => {
                // Pending non-SIGSTOP signal — forward + keep waiting
                // (mirror api.c:1777 loop semantics).
                let _ = ptrace::cont(other, sig);
            }
            WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _) => {
                return Err(PtraceError::TraceeExited(tid));
            }
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    #[test]
    fn suspend_count_starts_at_zero_for_unknown_tid() {
        let tid = Pid::from_raw(999_999);
        assert_eq!(suspend_count(tid), 0);
    }

    #[test]
    fn find_paused_returns_none_when_no_thread_of_pid_held() {
        let pid = Pid::from_raw(999_998);
        assert!(find_paused_thread(pid).is_none());
    }

    #[test]
    fn resume_unknown_tid_is_zero() {
        let pid = Pid::from_raw(999_997);
        let tid = Pid::from_raw(999_996);
        // No ptrace calls happen because state map lookup short-circuits.
        let result = resume_thread(pid, tid).expect("ok for unknown tid");
        assert_eq!(result, 0);
    }

    fn spawn_sleep() -> Child {
        Command::new("sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep")
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE"]
    fn nested_suspend_increments_then_decrements_once_per_resume() {
        reset_state_for_tests();
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        let tid = pid; // sleep is single-threaded → tid == pid
        std::thread::sleep(Duration::from_millis(50));

        let p1 = suspend_thread(pid, tid).expect("first suspend");
        assert_eq!(p1, 0, "first suspend prior count is 0");
        assert_eq!(suspend_count(tid), 1);

        let p2 = suspend_thread(pid, tid).expect("second suspend");
        assert_eq!(p2, 1);
        let p3 = suspend_thread(pid, tid).expect("third suspend");
        assert_eq!(p3, 2);
        assert_eq!(suspend_count(tid), 3);

        let r1 = resume_thread(pid, tid).expect("resume 1");
        assert_eq!(r1, 2);
        assert!(
            find_paused_thread(pid).is_some(),
            "still held after 1 resume"
        );

        let r2 = resume_thread(pid, tid).expect("resume 2");
        assert_eq!(r2, 1);
        let r3 = resume_thread(pid, tid).expect("resume 3");
        assert_eq!(r3, 0);
        assert!(
            find_paused_thread(pid).is_none(),
            "released after last resume"
        );
        assert_eq!(suspend_count(tid), 0);

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE"]
    fn find_paused_returns_some_after_suspend_none_after_resume() {
        reset_state_for_tests();
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        let tid = pid;
        std::thread::sleep(Duration::from_millis(50));

        assert!(find_paused_thread(pid).is_none());
        suspend_thread(pid, tid).expect("suspend");
        assert_eq!(find_paused_thread(pid), Some(tid));
        resume_thread(pid, tid).expect("resume");
        assert!(find_paused_thread(pid).is_none());

        let _ = child.kill();
        let _ = child.wait();
    }
}
