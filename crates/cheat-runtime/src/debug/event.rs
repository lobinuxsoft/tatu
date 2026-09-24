//! Debug event types — the value the debug loop produces when a tracee
//! stops for any reason (breakpoint hit, single-step trap, signal,
//! thread/process exit, virtual create-process/create-thread events).
//!
//! Maps to CE's `DebugEvent` struct (declared in `ceserver/api.h`) +
//! the negative `debugevent` codes CE uses for virtual events (`-1` =
//! thread create, `-2` = process create — see `api.c:602` and `:595`).

use nix::unistd::Pid;

/// What kind of stop the tracee took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugEventKind {
    /// A hardware breakpoint fired. `dr_index` is which of DR0-DR3 it
    /// was (decoded from DR6); `address` is the linear address of the
    /// instruction that triggered it (RIP at trap time for execute
    /// breakpoints; the data address being read/written for
    /// watchpoints).
    BreakpointHit { dr_index: u8, address: u64 },

    /// Single-step trap (TF bit in EFLAGS, set by us via
    /// `ContinueFromDebugEvent` with `IgnoreSignal::SingleStep`).
    SingleStep { address: u64 },

    /// Generic stop on a signal that was not a SIGTRAP from our
    /// breakpoints — typically SIGSTOP (from our own attach), SIGSEGV
    /// in the tracee, etc. The caller decides whether to forward it
    /// to the tracee on continue.
    Signal { signo: i32 },

    /// The thread exited or was killed by a signal.
    ThreadExit { exit_code: i32 },

    /// The whole process exited (the main thread terminated).
    ProcessExit { exit_code: i32 },

    /// Virtual "we just attached" event the debug loop emits once
    /// when `Debugger::start` finishes. Mirrors CE's `-2` event so
    /// upper layers (UI hookup) can react to "debug session begin".
    CreateProcess,

    /// Virtual "new thread to debug" event the debug loop emits per
    /// thread enumerated at attach time. Mirrors CE's `-1` event.
    CreateThread,
}

impl DebugEventKind {
    /// True for events the upper layer should not forward back to the
    /// tracee on continue (the kernel never delivered a real signal).
    pub fn is_virtual(&self) -> bool {
        matches!(
            self,
            DebugEventKind::CreateProcess | DebugEventKind::CreateThread
        )
    }

    /// True for the events that terminate the tracee — the debug loop
    /// must clean up its thread entry after returning one of these.
    pub fn is_exit(&self) -> bool {
        matches!(
            self,
            DebugEventKind::ThreadExit { .. } | DebugEventKind::ProcessExit { .. }
        )
    }
}

/// One stop event from the debug loop. `tid` is the thread that
/// stopped (not the owning process), matching CE semantics where
/// every stop is per-TID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugEvent {
    pub tid: Pid,
    pub kind: DebugEventKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_events_classified() {
        assert!(DebugEventKind::CreateProcess.is_virtual());
        assert!(DebugEventKind::CreateThread.is_virtual());
        assert!(!DebugEventKind::Signal { signo: 19 }.is_virtual());
        assert!(
            !DebugEventKind::BreakpointHit {
                dr_index: 0,
                address: 0
            }
            .is_virtual()
        );
    }

    #[test]
    fn exit_events_classified() {
        assert!(DebugEventKind::ProcessExit { exit_code: 0 }.is_exit());
        assert!(DebugEventKind::ThreadExit { exit_code: -1 }.is_exit());
        assert!(!DebugEventKind::CreateProcess.is_exit());
        assert!(!DebugEventKind::SingleStep { address: 0xdead }.is_exit());
    }
}
