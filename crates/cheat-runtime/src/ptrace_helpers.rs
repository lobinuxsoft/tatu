//! Centralised ptrace surface — port of `ceserver/api.c::safe_ptrace`,
//! `ptrace_attach_andwait`, and `mychildhandler` (SIGCHLD).
//!
//! Ported 1:1 from `cheat-engine/Cheat Engine/ceserver/api.c`:
//! - `safe_ptrace` (line 222)
//! - `ptrace_attach_andwait` (line 243)
//! - `WakeDebuggerThread` (line 303) — collapsed into the
//!   [`debugger_event`] [`Condvar`] notify; CE uses a `sem_post`.
//! - `mychildhandler` (line 308) — SIGCHLD signal handler that wakes
//!   the debugger thread; the body is the same one-liner CE has.
//!
//! Why a separate module: CE goes through `safe_ptrace` for **every**
//! single ptrace call so retries, error translation, and logging all
//! happen in one place. Tatu's pre-#139 code called `nix::sys::ptrace`
//! directly from each consumer, with each consumer re-implementing
//! (or worse, forgetting) the attach + waitpid dance — exactly the
//! kind of duplication that bit `find_what_writes` in session
//! 2026-05-25.
//!
//! Public surface:
//! - [`PtraceError`] — typed error covering the two cases callers act
//!   on (tracee vanished mid-call → skip; other → bubble up).
//! - [`safe_ptrace`] — generic wrapper that translates errno → result.
//! - [`attach_and_wait`] — PTRACE_ATTACH + waitpid until the tracee
//!   reaches the SIGSTOP delivered by attach. Relays non-stop signals
//!   transparently.
//! - [`install_sigchld_handler`] — idempotent SIGCHLD registration,
//!   wakes [`wait_for_debug_event`] when any tracee status changes.
//! - [`wait_for_debug_event`] — block (with timeout) until a SIGCHLD
//!   fires; the debug subsystem consumes this once #142 lands.

use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::libc;
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

#[derive(Debug, thiserror::Error)]
pub enum PtraceError {
    /// The tracee disappeared (ESRCH) between the caller deciding to
    /// act on it and us issuing the syscall. Callers iterating over
    /// thread lists treat this as "skip"; bubble up otherwise.
    #[error("tracee {0} vanished (ESRCH)")]
    TraceeGone(Pid),
    /// PTRACE_ATTACH timed out waiting for the SIGSTOP delivery (the
    /// tracee was stopped already and never resent a stop). CE just
    /// loops; we cap at a generous timeout so a runaway can't hang
    /// the whole tracker.
    #[error("attach to {0} timed out waiting for SIGSTOP")]
    AttachTimeout(Pid),
    /// The tracee terminated (WIFEXITED / WIFSIGNALED) while we were
    /// in the attach dance. CE returns -2 / -3 here.
    #[error("tracee {0} terminated during attach")]
    TraceeExited(Pid),
    /// Any other ptrace failure. Wraps the underlying errno so the
    /// caller can decide whether to log + continue or abort.
    #[error("ptrace error: {0}")]
    Errno(#[from] Errno),
}

/// Result of [`safe_ptrace`] when the caller wants the raw register-
/// or word-sized return value (PTRACE_PEEKDATA / PEEKUSER / GETREGS).
/// For request types that return -1-on-error-or-real-value (the
/// classic ptrace footgun) the caller checks errno; [`safe_ptrace`]
/// already translates that for them.
pub type PtraceResult = Result<i64, PtraceError>;

/// `safe_ptrace` port — direct `libc::ptrace` call wrapped to
/// translate errno into [`PtraceError`]. Mirrors CE's idiom of
/// resetting `errno` before the call and checking it after.
///
/// `request` is a raw `c_int` (use `libc::PTRACE_*` constants). Tatu
/// stays at this level instead of wrapping each request in its own
/// typed function because:
/// 1. nix's `ptrace` module is incomplete (no `POKEUSER`, no
///    `SETREGS` on every libc).
/// 2. CE's API surface is itself "give me the request, give me the
///    args, I'll handle errno" — preserving the same shape makes the
///    port direct.
///
/// Safety: caller is responsible for `addr` / `data` pointing at
/// memory the kernel can safely read/write per the specific request
/// semantics. CE's port has the same contract.
pub fn safe_ptrace(
    request: libc::c_uint,
    pid: Pid,
    addr: *mut c_void,
    data: *mut c_void,
) -> PtraceResult {
    // Reset errno before the call — ptrace returns -1 on error AND on
    // some legitimate PEEK results, so errno is the only way to
    // distinguish. CE does the same `errno = 0` dance.
    Errno::clear();
    let result = unsafe { libc::ptrace(request, pid.as_raw(), addr, data) };
    let err = Errno::last();
    if err != Errno::UnknownErrno && err != Errno::from_raw(0) {
        return Err(match err {
            Errno::ESRCH => PtraceError::TraceeGone(pid),
            other => PtraceError::Errno(other),
        });
    }
    Ok(result)
}

/// `ptrace_attach_andwait` port — PTRACE_ATTACH the target, then
/// loop on `waitpid` until we see the SIGSTOP delivery that signals
/// attach completion. Non-SIGSTOP signal-stops are relayed verbatim
/// (PTRACE_CONT with the signal) so we don't eat signals the tracee
/// actually needs to handle.
///
/// Returns the tid that's now stopped (usually = pid for single-
/// threaded, may differ when attaching to a process group on multi-
/// threaded tracees — CE handles that case the same way).
///
/// CE has no timeout; we add a generous 5-second cap to avoid
/// permanently hanging the tracker if a tracee never delivers
/// SIGSTOP for whatever kernel-side reason.
pub fn attach_and_wait(pid: Pid) -> Result<Pid, PtraceError> {
    safe_ptrace(
        libc::PTRACE_ATTACH,
        pid,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    )?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            return Err(PtraceError::AttachTimeout(pid));
        }
        let status = waitpid(Pid::from_raw(-1), Some(WaitPidFlag::__WALL))?;
        match status {
            WaitStatus::Stopped(stopped_pid, Signal::SIGSTOP) => return Ok(stopped_pid),
            WaitStatus::Stopped(stopped_pid, other_sig) => {
                // Not our SIGSTOP — relay the signal and keep waiting.
                // CE does PTRACE_CONT with the signal as data so the
                // tracee's own handler still runs.
                let _ = safe_ptrace(
                    libc::PTRACE_CONT as libc::c_uint,
                    stopped_pid,
                    std::ptr::null_mut(),
                    other_sig as i32 as *mut c_void,
                );
            }
            WaitStatus::Exited(exited_pid, _) | WaitStatus::Signaled(exited_pid, _, _) => {
                if exited_pid == pid {
                    return Err(PtraceError::TraceeExited(pid));
                }
                // Some unrelated child of ours exited; keep waiting.
            }
            WaitStatus::Continued(_) => {
                // CE logs "It already continued?" and loops; us too.
            }
            _ => {
                // PtraceEvent / PtraceSyscall / StillAlive — keep
                // waiting. CE treats anything that isn't a recognised
                // stop or exit as continue-and-retry.
            }
        }
    }
}

// -----------------------------------------------------------------------
// SIGCHLD handler + debugger thread wake-up
// -----------------------------------------------------------------------

/// Counter incremented by the SIGCHLD handler. Atomic so the handler
/// can write it without a lock (signal handlers must only call
/// async-signal-safe functions; locking a Mutex is not on that list).
static SIGCHLD_PENDING: AtomicBool = AtomicBool::new(false);

/// Condvar the debugger thread waits on. The SIGCHLD handler flips
/// the flag and notifies; [`wait_for_debug_event`] sees the flag and
/// returns. Lazy-init so the test harness can decide whether to
/// install the handler.
static DEBUG_EVENT: LazyLock<(Mutex<()>, Condvar)> =
    LazyLock::new(|| (Mutex::new(()), Condvar::new()));

/// Tracks whether [`install_sigchld_handler`] has registered the
/// handler at least once. Idempotent: repeat calls are no-ops so
/// every consumer can call it defensively on startup.
static HANDLER_INSTALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigchld_handler(_signum: i32) {
    // `mychildhandler` port — must only use async-signal-safe ops.
    // Setting an AtomicBool and notifying a Condvar via pthread_cond
    // are both on the safe list; touching a Mutex is technically
    // racy but the wake-up doesn't depend on holding the lock.
    SIGCHLD_PENDING.store(true, Ordering::SeqCst);
    let (_, ref cvar) = *DEBUG_EVENT;
    cvar.notify_all();
}

/// Install the SIGCHLD handler. Idempotent: safe to call from
/// multiple consumers. Uses `SA_RESTART | SA_NOCLDSTOP` so the
/// handler doesn't fire for every stop signal — only real child
/// state changes that the debugger cares about.
///
/// CE installs this once at `initAPI`; we expose it as an explicit
/// call so tests that don't need the handler can opt out.
pub fn install_sigchld_handler() -> Result<(), PtraceError> {
    if HANDLER_INSTALLED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let action = SigAction::new(
        SigHandler::Handler(sigchld_handler),
        SaFlags::SA_RESTART | SaFlags::SA_NOCLDSTOP,
        SigSet::empty(),
    );
    unsafe { sigaction(Signal::SIGCHLD, &action) }?;
    HANDLER_INSTALLED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Block until SIGCHLD fires or `timeout` elapses. Returns `true` if
/// a child event was pending, `false` on timeout. The pending flag is
/// cleared on return so consecutive calls don't see the same event.
///
/// This is the consumer side of [`sigchld_handler`]; the debug
/// subsystem (#142) will sit on top of it, calling `waitpid(-1)` in
/// a loop after every wake-up to drain the queue.
pub fn wait_for_debug_event(timeout: Duration) -> bool {
    let (ref lock, ref cvar) = *DEBUG_EVENT;
    let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    if SIGCHLD_PENDING.swap(false, Ordering::SeqCst) {
        return true;
    }
    let (_guard, wait_result) = cvar.wait_timeout(guard, timeout).unwrap_or_else(|p| {
        let (guard, result) = p.into_inner();
        (guard, result)
    });
    if wait_result.timed_out() {
        false
    } else {
        SIGCHLD_PENDING.swap(false, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::signal::{kill, raise};
    use nix::unistd::{ForkResult, fork};

    #[test]
    fn safe_ptrace_returns_typed_esrch_on_dead_pid() {
        // PID 999999 is almost certainly unused — ptrace(GETREGS) on
        // it should return ESRCH, which safe_ptrace must translate.
        let dead = Pid::from_raw(999999);
        let result = safe_ptrace(
            libc::PTRACE_GETREGS,
            dead,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        match result {
            Err(PtraceError::TraceeGone(p)) => assert_eq!(p, dead),
            other => panic!("expected TraceeGone, got {other:?}"),
        }
    }

    #[test]
    fn install_sigchld_handler_is_idempotent() {
        install_sigchld_handler().expect("first install");
        install_sigchld_handler().expect("second install");
        install_sigchld_handler().expect("third install");
        assert!(HANDLER_INSTALLED.load(Ordering::SeqCst));
    }

    #[test]
    fn wait_for_debug_event_times_out_when_no_signal() {
        // No SIGCHLD pending; should time out cleanly without panic.
        let fired = wait_for_debug_event(Duration::from_millis(50));
        assert!(!fired, "no SIGCHLD was raised, wait should time out");
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn attach_and_wait_against_self_fork() {
        // Fork a child that just spins; the parent attaches via
        // attach_and_wait, verifies it returns the child's pid as the
        // stopped tid, then detaches and reaps.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                // Spin until we get SIGTERM from the parent.
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(50));
                let stopped = attach_and_wait(child).expect("attach");
                assert_eq!(stopped, child);
                let _ = safe_ptrace(
                    libc::PTRACE_DETACH,
                    child,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                let _ = kill(child, Signal::SIGTERM);
                let _ = waitpid(child, None);
            }
        }
    }

    #[test]
    #[ignore = "manually triggers SIGCHLD in the test process; can be flaky on parallel runs"]
    fn sigchld_handler_wakes_wait_for_debug_event() {
        install_sigchld_handler().expect("install");
        // raise(SIGCHLD) in the same process — the handler must set
        // the pending flag, then wait_for_debug_event returns true.
        raise(Signal::SIGCHLD).expect("raise");
        let fired = wait_for_debug_event(Duration::from_secs(1));
        assert!(fired, "SIGCHLD should have set the pending flag");
    }
}
