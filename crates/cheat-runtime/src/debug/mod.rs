//! Debug subsystem facade — `Debugger` struct owning per-process
//! debug state (attached threads, event queue, breakpoint slots) +
//! the public methods that map to CE's StartDebug / StopDebug /
//! SetBreakpoint / RemoveBreakpoint / WaitForDebugEvent /
//! ContinueFromDebugEvent (`ceserver/api.c`).
//!
//! # Design vs CE
//!
//! CE owns the debugger state inside `PProcessData` (one per
//! debugged process) and routes calls through a single dedicated
//! "debugger thread" via socket IPC when the caller is on another
//! thread. We don't replicate the socket dispatcher: the `Debugger`
//! struct is `Send + Sync` (state behind locks), so any thread can
//! call any method. This drops ~200 LOC of CE plumbing without
//! losing semantics — the Mutex around `threads` serves the same
//! "one operation at a time" guarantee CE's debuggerthread did.
//!
//! # Lifecycle
//!
//! ```ignore
//! let dbg = Debugger::start(Pid::from_raw(1234))?;
//! dbg.set_breakpoint(tid, 0, addr, BpType::Write, BpSize::Dword)?;
//! let ev = dbg.wait_for_event(Some(Duration::from_secs(1)))?;
//! dbg.continue_from_event(ev.tid, false)?;
//! drop(dbg); // stop_debug runs in Drop
//! ```

pub mod breakpoint;
pub mod event;
pub mod event_loop;
pub mod queue;

use std::collections::HashMap;
use std::mem::offset_of;
use std::os::raw::c_void;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nix::libc::{self, PTRACE_POKEUSER};
use nix::sys::ptrace;
use nix::unistd::Pid;

pub use breakpoint::{BpSize, BpType};
pub use event::{DebugEvent, DebugEventKind};

use crate::debug::event_loop::{
    attach_thread_for_debug, classify_status, detach_thread_from_debug, drain_pending_into_queue,
};
use crate::debug::queue::EventQueue;
use crate::ptrace_helpers::{PtraceError, safe_ptrace};

/// Per-thread state CE keeps inside `ThreadData`. We track only what
/// the public API needs.
#[derive(Debug, Clone, Copy)]
struct ThreadState {
    /// `True` while the thread is ptrace-stopped under us — set when
    /// we observe a stop event for it, cleared on `continue_from_event`.
    is_paused: bool,
}

/// The Debugger handle. `Send + Sync`: every method takes `&self`,
/// so the same handle can be used from multiple threads (CE
/// serializes through its debuggerthread; we serialize through the
/// internal `Mutex`).
#[derive(Debug)]
pub struct Debugger {
    pid: Pid,
    threads: Mutex<HashMap<Pid, ThreadState>>,
    queue: Arc<EventQueue>,
}

#[derive(Debug, thiserror::Error)]
pub enum DebugError {
    #[error("ptrace error: {0}")]
    Ptrace(#[from] PtraceError),

    #[error("io reading /proc/{pid}/task: {source}")]
    EnumerateThreads {
        pid: i32,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid debug register index {0} (must be 0..=3)")]
    InvalidDebugRegister(u8),

    #[error("unknown thread tid={0}")]
    UnknownThread(Pid),

    #[error("wait_for_event timed out")]
    Timeout,
}

impl Debugger {
    /// Start debugging `pid`. Enumerates `/proc/<pid>/task`, attaches
    /// to every thread, emits a virtual `CreateProcess` event + one
    /// `CreateThread` per attached TID into the queue.
    ///
    /// 1:1 port of `StartDebug` (api.c:479). The SIGCHLD handler CE
    /// installs is owned by [`crate::ptrace_helpers::install_sigchld_handler`];
    /// we install it lazily here so a process can be debugged without
    /// the caller having to know.
    pub fn start(pid: Pid) -> Result<Self, DebugError> {
        let _ = crate::ptrace_helpers::install_sigchld_handler();

        let task_path = format!("/proc/{}/task", pid.as_raw());
        let entries =
            std::fs::read_dir(&task_path).map_err(|source| DebugError::EnumerateThreads {
                pid: pid.as_raw(),
                source,
            })?;

        let queue = Arc::new(EventQueue::new());
        let mut threads = HashMap::new();

        // CE emits CreateProcess (-2) once + CreateThread (-1) per TID.
        queue.push(DebugEvent {
            tid: pid,
            kind: DebugEventKind::CreateProcess,
        });

        for entry in entries {
            let entry = entry.map_err(|source| DebugError::EnumerateThreads {
                pid: pid.as_raw(),
                source,
            })?;
            let Some(name) = entry.file_name().to_str().map(|s| s.to_owned()) else {
                continue;
            };
            let Ok(tid_raw) = name.parse::<i32>() else {
                continue;
            };
            let tid = Pid::from_raw(tid_raw);
            // ESRCH between enumeration and attach = thread vanished;
            // skip without aborting (CE does the same — see api.c:535
            // logs "Failed to attach" but keeps going).
            match attach_thread_for_debug(tid) {
                Ok(()) => {
                    threads.insert(tid, ThreadState { is_paused: true });
                    queue.push(DebugEvent {
                        tid,
                        kind: DebugEventKind::CreateThread,
                    });
                    // CE immediately continues each freshly-attached
                    // thread (api.c:568 PTRACE_CONT) after recording
                    // the virtual events. Mirror that — otherwise the
                    // first wait_for_event returns SIGSTOPs instead
                    // of the consumer's expected real events.
                    let _ = ptrace::cont(tid, None);
                    if let Some(state) = threads.get_mut(&tid) {
                        state.is_paused = false;
                    }
                }
                Err(PtraceError::Errno(nix::errno::Errno::ESRCH)) => continue,
                Err(PtraceError::TraceeExited(_)) => continue,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(Self {
            pid,
            threads: Mutex::new(threads),
            queue,
        })
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// TIDs currently registered as part of this debug session.
    pub fn thread_ids(&self) -> Vec<Pid> {
        self.threads
            .lock()
            .expect("threads poisoned")
            .keys()
            .copied()
            .collect()
    }

    /// Set a hardware breakpoint on `tid`. Validates `debugreg` in
    /// `0..=3` (the four DRs). `tid` must already be stopped under
    /// us, or have been registered at `start()` time — the caller is
    /// responsible for pausing it first (e.g. via
    /// [`crate::thread_control::suspend_thread`]).
    ///
    /// 1:1 with CE `SetBreakpoint` x86_64 path (api.c:1077-1116) for
    /// the per-thread case. CE's `tid == -1` "all threads" loop is
    /// expressed as [`set_breakpoint_all_threads`] below.
    pub fn set_breakpoint(
        &self,
        tid: Pid,
        debugreg: u8,
        address: u64,
        bptype: BpType,
        bpsize: BpSize,
    ) -> Result<(), DebugError> {
        if debugreg > 3 {
            return Err(DebugError::InvalidDebugRegister(debugreg));
        }
        if !self
            .threads
            .lock()
            .expect("threads poisoned")
            .contains_key(&tid)
        {
            return Err(DebugError::UnknownThread(tid));
        }
        breakpoint::set_hardware_breakpoint(tid, debugreg, address, bptype, bpsize)?;
        Ok(())
    }

    /// Set the same breakpoint on every registered thread. Mirrors
    /// `SetBreakpoint(tid=-1)` recursion (api.c:670-681).
    pub fn set_breakpoint_all_threads(
        &self,
        debugreg: u8,
        address: u64,
        bptype: BpType,
        bpsize: BpSize,
    ) -> Result<usize, DebugError> {
        if debugreg > 3 {
            return Err(DebugError::InvalidDebugRegister(debugreg));
        }
        let tids: Vec<Pid> = self.thread_ids();
        let mut ok = 0usize;
        for tid in tids {
            if breakpoint::set_hardware_breakpoint(tid, debugreg, address, bptype, bpsize).is_ok() {
                ok += 1;
            }
        }
        Ok(ok)
    }

    /// Remove the breakpoint in `debugreg` on `tid`.
    /// 1:1 with CE `RemoveBreakpoint` x86 path (api.c:1374-1395).
    pub fn remove_breakpoint(&self, tid: Pid, debugreg: u8) -> Result<(), DebugError> {
        if debugreg > 3 {
            return Err(DebugError::InvalidDebugRegister(debugreg));
        }
        if !self
            .threads
            .lock()
            .expect("threads poisoned")
            .contains_key(&tid)
        {
            return Err(DebugError::UnknownThread(tid));
        }
        breakpoint::remove_hardware_breakpoint(tid, debugreg)?;
        Ok(())
    }

    /// Remove the breakpoint in `debugreg` on every registered
    /// thread. Mirrors `RemoveBreakpoint(tid=-1)` (api.c:1225-1233).
    pub fn remove_breakpoint_all_threads(&self, debugreg: u8) -> Result<usize, DebugError> {
        if debugreg > 3 {
            return Err(DebugError::InvalidDebugRegister(debugreg));
        }
        let tids: Vec<Pid> = self.thread_ids();
        let mut ok = 0usize;
        for tid in tids {
            if breakpoint::remove_hardware_breakpoint(tid, debugreg).is_ok() {
                ok += 1;
            }
        }
        Ok(ok)
    }

    /// Wait for any debug event. Returns the first one in the queue,
    /// or drains pending kernel events into the queue and waits if
    /// the queue is empty. `None` timeout = wait forever.
    ///
    /// 1:1 with `WaitForDebugEvent` (api.c:2215) but consolidated:
    /// CE separates the "for a specific TID" overload, which lives
    /// here as [`wait_for_event_on_tid`].
    pub fn wait_for_event(&self, timeout: Option<Duration>) -> Result<DebugEvent, DebugError> {
        if let Some(ev) = self.queue.pop_any() {
            self.mark_paused_for_event(&ev);
            return Ok(ev);
        }
        if let Some(ev) = drain_pending_into_queue(&self.queue, None)? {
            self.mark_paused_for_event(&ev);
            return Ok(ev);
        }
        match self.queue.wait_any(timeout) {
            Some(ev) => {
                self.mark_paused_for_event(&ev);
                Ok(ev)
            }
            None => Err(DebugError::Timeout),
        }
    }

    /// Wait for an event from a specific TID. Other TIDs' events
    /// that arrive in the meantime stay queued.
    pub fn wait_for_event_on_tid(
        &self,
        tid: Pid,
        timeout: Option<Duration>,
    ) -> Result<DebugEvent, DebugError> {
        if let Some(ev) = self.queue.pop_first_for_tid(tid) {
            self.mark_paused_for_event(&ev);
            return Ok(ev);
        }
        if let Some(ev) = drain_pending_into_queue(&self.queue, Some(tid))? {
            self.mark_paused_for_event(&ev);
            return Ok(ev);
        }
        match self.queue.wait_for_tid(tid, timeout) {
            Some(ev) => {
                self.mark_paused_for_event(&ev);
                Ok(ev)
            }
            None => Err(DebugError::Timeout),
        }
    }

    fn mark_paused_for_event(&self, ev: &DebugEvent) {
        if ev.kind.is_virtual() {
            return;
        }
        let mut map = self.threads.lock().expect("threads poisoned");
        if let Some(state) = map.get_mut(&ev.tid) {
            state.is_paused = true;
        }
    }

    /// Resume the tracee after a debug event. `ignore_signal`
    /// matches CE semantics: when `false`, the signal that caused
    /// the stop is delivered to the tracee on continue; when `true`,
    /// it is swallowed. Mirror `ContinueFromDebugEvent` (api.c:2380).
    pub fn continue_from_event(&self, tid: Pid, ignore_signal: bool) -> Result<(), DebugError> {
        // Virtual events: nothing to continue.
        // Clear DR6 (CE api.c:2445) so the next trap reports cleanly.
        let _ = clear_dr6(tid);

        let signal_to_deliver = if ignore_signal {
            None
        } else {
            // CE peeks siginfo via PTRACE_GETSIGINFO and forwards
            // si.si_signo unless it's SIGSTOP / SIGTSTP (19/21 in
            // CE's table). We approximate via the recorded WaitStatus
            // in the event — but the WaitStatus is consumed by the
            // queue, so peek again via siginfo here.
            siginfo_to_signal(tid)
        };
        ptrace::cont(tid, signal_to_deliver)
            .map_err(|e| DebugError::Ptrace(PtraceError::Errno(e)))?;
        let mut map = self.threads.lock().expect("threads poisoned");
        if let Some(state) = map.get_mut(&tid) {
            state.is_paused = false;
        }
        Ok(())
    }

    /// Manually trigger a wake on the internal queue. Useful when an
    /// external SIGCHLD handler pushes an event via [`event_queue`].
    pub fn wake(&self) {
        self.queue.wake_one();
    }

    /// Test/debug aid: borrow the underlying queue (for an external
    /// SIGCHLD handler to call `push` directly).
    pub fn event_queue(&self) -> Arc<EventQueue> {
        Arc::clone(&self.queue)
    }
}

impl Drop for Debugger {
    fn drop(&mut self) {
        // StopDebug equivalent — best-effort detach of every TID,
        // mirror api.c:2585. Errors are silently ignored: a thread
        // that already exited cannot be detached, and the only thing
        // to do in Drop is to release any kernel ptrace state.
        let tids: Vec<Pid> = self
            .threads
            .lock()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default();
        for tid in tids {
            let _ = detach_thread_from_debug(tid);
        }
    }
}

fn clear_dr6(tid: Pid) -> Result<(), PtraceError> {
    let off = (offset_of!(libc::user, u_debugreg) + 6 * 8) as *mut c_void;
    safe_ptrace(PTRACE_POKEUSER, tid, off, std::ptr::null_mut())?;
    Ok(())
}

fn siginfo_to_signal(tid: Pid) -> Option<nix::sys::signal::Signal> {
    use nix::sys::signal::Signal;
    let si = ptrace::getsiginfo(tid).ok()?;
    let signo = si.si_signo;
    // CE swallows 19 (SIGSTOP) + 21 (SIGTTIN) on x86; the gist is
    // "don't forward stops we induced ourselves". SIGSTOP and SIGTRAP
    // are the typical ones to drop.
    if signo == Signal::SIGSTOP as i32 || signo == Signal::SIGTRAP as i32 {
        return None;
    }
    Signal::try_from(signo).ok()
}

/// Helper exposed for external SIGCHLD handlers: classify a TID's
/// current WaitStatus and push it into `queue` if it's a stop event.
/// Returns the event that was pushed (or `None` if nothing was
/// waiting).
pub fn ingest_pending(queue: &EventQueue) -> Result<Option<DebugEvent>, PtraceError> {
    use nix::errno::Errno;
    use nix::sys::wait::WaitPidFlag;
    let any = Pid::from_raw(-1);
    let status = nix::sys::wait::waitpid(any, Some(WaitPidFlag::WNOHANG | WaitPidFlag::__WALL));
    match status {
        Ok(nix::sys::wait::WaitStatus::StillAlive) => Ok(None),
        Ok(status) => {
            let Some(tid) = status.pid() else {
                return Ok(None);
            };
            let kind = classify_status(tid, status);
            let ev = DebugEvent { tid, kind };
            queue.push(ev);
            Ok(Some(ev))
        }
        Err(Errno::ECHILD) => Ok(None),
        Err(e) => Err(PtraceError::Errno(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};

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
    fn debugger_error_types_implement_display() {
        let err = DebugError::InvalidDebugRegister(7);
        assert!(err.to_string().contains("invalid debug register"));
        let err = DebugError::UnknownThread(Pid::from_raw(123));
        assert!(err.to_string().contains("unknown thread"));
        let err = DebugError::Timeout;
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn ingest_pending_with_no_children_returns_none() {
        let q = EventQueue::new();
        // No tracees attached → ECHILD or StillAlive → Ok(None).
        let r = ingest_pending(&q).expect("ingest");
        assert!(r.is_none());
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE"]
    fn start_then_drop_against_sleep_child() {
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        std::thread::sleep(Duration::from_millis(50));

        let dbg = Debugger::start(pid).expect("start");
        assert_eq!(dbg.pid(), pid);
        // sleep is single-threaded → exactly one TID registered.
        assert_eq!(dbg.thread_ids().len(), 1);
        // Virtual CreateProcess + CreateThread should be queued.
        let ev1 = dbg
            .wait_for_event(Some(Duration::from_millis(500)))
            .expect("first virtual event");
        assert!(ev1.kind.is_virtual());
        drop(dbg);

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE"]
    fn set_then_remove_breakpoint_against_sleep_child() {
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        std::thread::sleep(Duration::from_millis(50));

        let dbg = Debugger::start(pid).expect("start");
        let tid = dbg.thread_ids()[0];

        // Set + remove DR0 — addresses are arbitrary, we only
        // validate the ptrace round-trip succeeds.
        dbg.set_breakpoint(tid, 0, 0x1000, BpType::Execute, BpSize::Byte)
            .expect("set bp");
        dbg.remove_breakpoint(tid, 0).expect("remove bp");

        // Validation: DR0 should be 0 (cleared); DR7 enable bit for
        // DR0 should be off. We can't peek without exposing helpers,
        // so the test asserts no errors.
        drop(dbg);
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn set_breakpoint_rejects_dr_out_of_range() {
        // No need for a live process — the validation happens before
        // the ptrace call.
        let dbg = Debugger {
            pid: Pid::from_raw(1),
            threads: Mutex::new(HashMap::new()),
            queue: Arc::new(EventQueue::new()),
        };
        let err = dbg
            .set_breakpoint(Pid::from_raw(1), 4, 0, BpType::Execute, BpSize::Byte)
            .unwrap_err();
        assert!(matches!(err, DebugError::InvalidDebugRegister(4)));
    }

    #[test]
    fn set_breakpoint_rejects_unknown_thread() {
        let dbg = Debugger {
            pid: Pid::from_raw(1),
            threads: Mutex::new(HashMap::new()),
            queue: Arc::new(EventQueue::new()),
        };
        let err = dbg
            .set_breakpoint(Pid::from_raw(999), 0, 0, BpType::Execute, BpSize::Byte)
            .unwrap_err();
        assert!(matches!(err, DebugError::UnknownThread(_)));
    }
}
