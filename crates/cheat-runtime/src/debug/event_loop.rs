//! Debug event loop helpers — the procedural part of `WaitForDebugEvent`
//! / `ContinueFromDebugEvent` / `StartDebug` / `StopDebug` that doesn't
//! belong inside the `Debugger` facade because it's pure-ish enough to
//! test on its own.
//!
//! 1:1 ports:
//! - [`drain_pending_into_queue`] = `WaitForDebugEventNative` first
//!   loop (api.c:2032-2062): drain every TID stopped with WNOHANG into
//!   the queue.
//! - [`classify_status`] = `GetStopSignalFromThread` (api.c around
//!   line 1990) + the SIGTRAP/DR6 inspection inline in
//!   `ContinueFromDebugEvent`.
//! - [`attach_thread_for_debug`] = the inner loop of `StartDebug`
//!   (api.c:530-540) per-TID.

use std::mem::offset_of;
use std::os::raw::c_void;

use nix::errno::Errno;
use nix::libc::{self, PTRACE_PEEKUSER};
use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::debug::breakpoint::decode_dr6;
use crate::debug::event::{DebugEvent, DebugEventKind};
use crate::debug::queue::EventQueue;
use crate::ptrace_helpers::{PtraceError, safe_ptrace};

/// Translate a `WaitStatus` into a [`DebugEventKind`]. For SIGTRAP we
/// inspect DR6 to tell breakpoint-hit apart from single-step.
pub fn classify_status(tid: Pid, status: WaitStatus) -> DebugEventKind {
    match status {
        WaitStatus::Exited(_, code) => DebugEventKind::ProcessExit { exit_code: code },
        WaitStatus::Signaled(_, sig, _) => DebugEventKind::ProcessExit {
            exit_code: -(sig as i32),
        },
        WaitStatus::Stopped(_, Signal::SIGTRAP) => classify_sigtrap(tid),
        WaitStatus::Stopped(_, sig) => DebugEventKind::Signal { signo: sig as i32 },
        // PTRACE_EVENT_* stops, syscall stops, continued: surface as
        // generic signals the caller decides about.
        _ => DebugEventKind::Signal { signo: 0 },
    }
}

fn classify_sigtrap(tid: Pid) -> DebugEventKind {
    // Read DR6: if a breakpoint condition bit (B0-B3) is set, report
    // breakpoint-hit; if bit 14 (BS, single-step) is set, report a
    // single-step. Otherwise it's a foreign SIGTRAP (int3 from the
    // tracee itself or a stop from another tracer) → treat as signal.
    let dr6 = read_dr6(tid).unwrap_or(0);

    if let Some(idx) = decode_dr6(dr6) {
        // RIP at trap time is the address-of-instruction for execute
        // breakpoints; for watchpoints it's the instruction that did
        // the access — close enough for the upper layer.
        let address = read_rip(tid).unwrap_or(0);
        return DebugEventKind::BreakpointHit {
            dr_index: idx,
            address,
        };
    }
    if dr6 & (1u64 << 14) != 0 {
        return DebugEventKind::SingleStep {
            address: read_rip(tid).unwrap_or(0),
        };
    }
    DebugEventKind::Signal {
        signo: Signal::SIGTRAP as i32,
    }
}

fn read_dr6(tid: Pid) -> Result<u64, PtraceError> {
    let off = (offset_of!(libc::user, u_debugreg) + 6 * 8) as *mut c_void;
    let v = safe_ptrace(PTRACE_PEEKUSER, tid, off, std::ptr::null_mut())?;
    Ok(v as u64)
}

fn read_rip(tid: Pid) -> Result<u64, PtraceError> {
    use crate::thread_context::{ContextFlags, get_thread_context};
    let ctx = get_thread_context(tid, ContextFlags::INTEGER)?;
    Ok(ctx.regs.rip)
}

/// Drain every TID currently stopped (WNOHANG) into the queue. The
/// `filter_tid` parameter mirrors CE's `tid` argument to
/// `WaitForDebugEventNative`: pass `None` to take any TID, or
/// `Some(tid)` to also short-circuit return that one if it appears
/// in the drain pass.
///
/// Returns `Ok(Some(event))` if a matching event was found and
/// removed from the kernel queue, `Ok(None)` if nothing matched (or
/// no events at all).
pub fn drain_pending_into_queue(
    queue: &EventQueue,
    filter_tid: Option<Pid>,
) -> Result<Option<DebugEvent>, PtraceError> {
    let any = Pid::from_raw(-1);
    let target = filter_tid.unwrap_or(any);
    loop {
        let result = waitpid(target, Some(WaitPidFlag::WNOHANG | WaitPidFlag::__WALL));
        match result {
            Ok(WaitStatus::StillAlive) => return Ok(None),
            Ok(status) => {
                let stopped_tid = match status.pid() {
                    Some(p) => p,
                    None => return Ok(None),
                };
                let kind = classify_status(stopped_tid, status);
                let event = DebugEvent {
                    tid: stopped_tid,
                    kind,
                };
                if filter_tid.is_none_or(|t| t == stopped_tid) {
                    return Ok(Some(event));
                }
                queue.push(event);
            }
            Err(Errno::ECHILD) => return Ok(None),
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

/// Attach to a single thread for debugging. CE calls this per-TID
/// inside `StartDebug` (api.c:530-540). The thread will be stopped on
/// SIGSTOP after the call.
pub fn attach_thread_for_debug(tid: Pid) -> Result<(), PtraceError> {
    ptrace::attach(tid)?;
    // The kernel injects SIGSTOP on attach; wait for it so the thread
    // is in the expected stopped-by-us state when we return.
    loop {
        let status = waitpid(tid, Some(WaitPidFlag::__WALL))?;
        match status {
            WaitStatus::Stopped(stopped, Signal::SIGSTOP) if stopped == tid => return Ok(()),
            WaitStatus::Stopped(other, sig) => {
                let _ = ptrace::cont(other, sig);
            }
            WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _) => {
                return Err(PtraceError::TraceeExited(tid));
            }
            _ => continue,
        }
    }
}

/// Detach from a thread for debugging — CE's `StopDebug` per-TID body
/// (api.c:2595-2638): SIGSTOP it, waitpid to confirm, then DETACH.
/// Best-effort: errors are returned but the caller (typically `Drop`)
/// usually ignores them.
pub fn detach_thread_from_debug(tid: Pid) -> Result<(), PtraceError> {
    // The thread may already be stopped under us. If not, ask it to
    // stop so DETACH succeeds.
    let _ = nix::sys::signal::kill(tid, Signal::SIGSTOP);
    loop {
        match waitpid(tid, Some(WaitPidFlag::__WALL)) {
            Ok(WaitStatus::Stopped(stopped, _)) if stopped == tid => break,
            Ok(WaitStatus::Stopped(_, _)) => continue,
            Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => {
                return Ok(()); // thread is gone, nothing to detach
            }
            Ok(_) => continue,
            Err(Errno::ECHILD) => return Ok(()),
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    ptrace::detach(tid, None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_exited_yields_process_exit() {
        let status = WaitStatus::Exited(Pid::from_raw(100), 7);
        let kind = classify_status(Pid::from_raw(100), status);
        assert_eq!(kind, DebugEventKind::ProcessExit { exit_code: 7 });
    }

    #[test]
    fn classify_signaled_yields_negated_signo() {
        let status = WaitStatus::Signaled(Pid::from_raw(100), Signal::SIGKILL, false);
        let kind = classify_status(Pid::from_raw(100), status);
        assert_eq!(
            kind,
            DebugEventKind::ProcessExit {
                exit_code: -(Signal::SIGKILL as i32),
            }
        );
    }

    #[test]
    fn classify_stopped_non_sigtrap_yields_signal() {
        let status = WaitStatus::Stopped(Pid::from_raw(100), Signal::SIGSTOP);
        let kind = classify_status(Pid::from_raw(100), status);
        assert_eq!(
            kind,
            DebugEventKind::Signal {
                signo: Signal::SIGSTOP as i32,
            }
        );
    }
}
