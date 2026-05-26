//! Thread register context (general-purpose + FP + debug registers)
//! via ptrace.
//!
//! Ported 1:1 from `cheat-engine/Cheat Engine/ceserver/`:
//! - `context.c::getContext` (line 12) → [`get_thread_context`]
//! - `context.c::setContext` (line 145) → [`set_thread_context`]
//! - `api.c::GetThreadContext` (line 1468) — the higher-level wrapper
//!   that handles "thread isn't suspended yet, suspend it first" lives
//!   in the future debug subsystem (#142); this module is the raw
//!   register I/O CE delegates to via `getContext`.
//! - DR0-DR7 access via `PTRACE_PEEKUSER` / `PTRACE_POKEUSER` at
//!   `offsetof(struct user, u_debugreg[N])`. CE uses the same pattern
//!   inline at every debug-register call site (api.c:1104, :2320, etc.);
//!   we surface it as [`read_debug_register`] / [`write_debug_register`]
//!   so the debug subsystem (#142) and any value-freeze code-patch
//!   path can share one implementation.
//!
//! The tracee **must** be ptrace-attached and stopped before any of
//! these functions can succeed. The "auto-attach + auto-suspend"
//! wrapper CE has in `GetThreadContext` is intentionally NOT ported
//! here — it would require the full event queue + thread state map
//! that the debug subsystem owns. Callers in the meantime hold a
//! [`crate::ptrace_helpers::attach_and_wait`] handle, do their I/O,
//! detach.

use std::mem::MaybeUninit;
use std::os::raw::c_void;

use nix::libc;
use nix::unistd::Pid;

use crate::ptrace_helpers::{PtraceError, safe_ptrace};

/// Which subset of the context the caller wants. CE's `CONTEXT_FLAGS`
/// uses the Windows bit positions; we mirror them so on-disk records
/// or any future CE-format trace replay stays byte-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextFlags(pub u32);

impl ContextFlags {
    /// Integer registers (rip, rsp, rax, …) — the GETREGS payload.
    pub const INTEGER: ContextFlags = ContextFlags(0x0001_0002);
    /// Floating-point + SSE state — the GETFPREGS payload.
    pub const FLOATING_POINT: ContextFlags = ContextFlags(0x0001_0008);
    /// Debug registers DR0-DR7. Each PEEKUSER call is a syscall, so
    /// callers that only need one DR (the common case for breakpoint
    /// dispatch) can call [`read_debug_register`] directly.
    pub const DEBUG_REGISTERS: ContextFlags = ContextFlags(0x0001_0010);
    /// All of the above.
    pub const FULL: ContextFlags =
        ContextFlags(Self::INTEGER.0 | Self::FLOATING_POINT.0 | Self::DEBUG_REGISTERS.0);

    pub fn contains(self, other: ContextFlags) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Thread register snapshot. Mirrors CE's `CONTEXT` struct for the
/// x86_64 path. Other CE arches (ARM / aarch64) carry different
/// payloads — `reference_cheat_engine_source.md` keeps the port
/// scoped to x86_64, matching the Bazzite/Deck target.
///
/// `regs` and `fpregs` are populated only when the requested
/// `ContextFlags` included the matching bit; otherwise they hold the
/// zeroed default. `dr` is populated when `DEBUG_REGISTERS` was
/// requested.
#[derive(Debug, Clone)]
pub struct ThreadContext {
    pub regs: libc::user_regs_struct,
    pub fpregs: libc::user_fpregs_struct,
    pub dr: [u64; 8],
    /// Flags that were actually populated. Re-export so callers
    /// inspecting a context they didn't request themselves know what
    /// to trust.
    pub populated: ContextFlags,
}

impl Default for ThreadContext {
    fn default() -> Self {
        // SAFETY: both user_regs_struct and user_fpregs_struct are
        // plain-old-data C structs with no invalid bit patterns; a
        // zero-fill is a valid (if meaningless) instance.
        Self {
            regs: unsafe { std::mem::zeroed() },
            fpregs: unsafe { std::mem::zeroed() },
            dr: [0; 8],
            populated: ContextFlags(0),
        }
    }
}

/// `getContext` port — read the requested register groups from a
/// suspended tracee. Returns the populated [`ThreadContext`].
///
/// The tracee must already be ptrace-attached AND stopped. If it's
/// not, the underlying ptrace calls return ESRCH or EBUSY and the
/// returned [`PtraceError`] will say so — caller is expected to have
/// gone through [`crate::ptrace_helpers::attach_and_wait`] or
/// equivalent.
pub fn get_thread_context(tid: Pid, flags: ContextFlags) -> Result<ThreadContext, PtraceError> {
    let mut ctx = ThreadContext::default();

    if flags.contains(ContextFlags::INTEGER) {
        let mut regs = MaybeUninit::<libc::user_regs_struct>::zeroed();
        safe_ptrace(
            libc::PTRACE_GETREGS,
            tid,
            std::ptr::null_mut(),
            regs.as_mut_ptr() as *mut c_void,
        )?;
        // SAFETY: PTRACE_GETREGS, when it succeeds, fills the entire
        // user_regs_struct payload. The kernel guarantees the write;
        // we've checked for error.
        ctx.regs = unsafe { regs.assume_init() };
        ctx.populated.0 |= ContextFlags::INTEGER.0;
    }

    if flags.contains(ContextFlags::FLOATING_POINT) {
        let mut fpregs = MaybeUninit::<libc::user_fpregs_struct>::zeroed();
        safe_ptrace(
            libc::PTRACE_GETFPREGS,
            tid,
            std::ptr::null_mut(),
            fpregs.as_mut_ptr() as *mut c_void,
        )?;
        // SAFETY: same contract as above for the FP register block.
        ctx.fpregs = unsafe { fpregs.assume_init() };
        ctx.populated.0 |= ContextFlags::FLOATING_POINT.0;
    }

    if flags.contains(ContextFlags::DEBUG_REGISTERS) {
        for i in 0..8 {
            ctx.dr[i] = read_debug_register(tid, i)?;
        }
        ctx.populated.0 |= ContextFlags::DEBUG_REGISTERS.0;
    }

    Ok(ctx)
}

/// `setContext` port — write the populated register groups back to a
/// suspended tracee. Honours `ctx.populated` so a caller that only
/// fetched DEBUG_REGISTERS and wants to push DR0 back doesn't
/// accidentally clobber the integer registers with the zeroed default.
///
/// CE's `setContext` only writes integer registers ("todo FPU" in the
/// CE source); we do the same plus debug registers so the DR0 arming
/// path can share this function instead of calling
/// `write_debug_register` in a loop.
pub fn set_thread_context(tid: Pid, ctx: &ThreadContext) -> Result<(), PtraceError> {
    if ctx.populated.contains(ContextFlags::INTEGER) {
        safe_ptrace(
            libc::PTRACE_SETREGS,
            tid,
            std::ptr::null_mut(),
            &ctx.regs as *const _ as *mut c_void,
        )?;
    }

    if ctx.populated.contains(ContextFlags::FLOATING_POINT) {
        safe_ptrace(
            libc::PTRACE_SETFPREGS,
            tid,
            std::ptr::null_mut(),
            &ctx.fpregs as *const _ as *mut c_void,
        )?;
    }

    if ctx.populated.contains(ContextFlags::DEBUG_REGISTERS) {
        for (i, val) in ctx.dr.iter().enumerate() {
            write_debug_register(tid, i, *val)?;
        }
    }

    Ok(())
}

/// `PTRACE_PEEKUSER` at `offsetof(struct user, u_debugreg[reg])`.
/// Mirrors the inline pattern CE uses at every debug-register call
/// site (api.c:1082, :2320, ...).
///
/// `reg` is 0-7. The function clamps via `assert` to catch
/// out-of-range at test time; release builds wrap-around in the
/// offset arithmetic and ptrace returns EFAULT, which surfaces as
/// `PtraceError::Errno(EFAULT)` — still safe but harder to debug.
pub fn read_debug_register(tid: Pid, reg: usize) -> Result<u64, PtraceError> {
    debug_assert!(reg < 8, "u_debugreg has 8 slots (DR0..DR7)");
    let offset = debug_register_offset(reg);
    let val = safe_ptrace(
        libc::PTRACE_PEEKUSER,
        tid,
        offset as *mut c_void,
        std::ptr::null_mut(),
    )?;
    Ok(val as u64)
}

/// `PTRACE_POKEUSER` at `offsetof(struct user, u_debugreg[reg])`.
/// Companion of [`read_debug_register`].
pub fn write_debug_register(tid: Pid, reg: usize, val: u64) -> Result<(), PtraceError> {
    debug_assert!(reg < 8, "u_debugreg has 8 slots (DR0..DR7)");
    let offset = debug_register_offset(reg);
    safe_ptrace(
        libc::PTRACE_POKEUSER,
        tid,
        offset as *mut c_void,
        val as *mut c_void,
    )?;
    Ok(())
}

/// `offsetof(struct user, u_debugreg[reg])`. The 8-byte stride matches
/// the x86_64 `user_struct` layout in the kernel headers; ptrace
/// expects this exact byte offset.
fn debug_register_offset(reg: usize) -> usize {
    // u_debugreg is the last array in struct user; offsetof to its
    // base is the offset of any field after the regs/fpregs/u_tsize/
    // signal/holdmask block. Rather than hard-code that constant
    // (it differs subtly across glibc versions), let libc's `user`
    // struct give it to us via std::mem::offset_of on a const-
    // initialised default.
    //
    // The macro path `std::mem::offset_of!(libc::user, u_debugreg)`
    // works since Rust 1.77 and is stable. Each DR slot is 8 bytes
    // on x86_64.
    std::mem::offset_of!(libc::user, u_debugreg) + reg * 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::signal::{Signal, kill};
    use nix::sys::wait::waitpid;
    use nix::unistd::{ForkResult, fork};
    use std::time::Duration;

    use crate::ptrace_helpers::{attach_and_wait, safe_ptrace};

    #[test]
    fn context_flags_contains_matches_set_bits() {
        assert!(ContextFlags::FULL.contains(ContextFlags::INTEGER));
        assert!(ContextFlags::FULL.contains(ContextFlags::FLOATING_POINT));
        assert!(ContextFlags::FULL.contains(ContextFlags::DEBUG_REGISTERS));
        assert!(!ContextFlags::INTEGER.contains(ContextFlags::DEBUG_REGISTERS));
    }

    #[test]
    fn debug_register_offset_strides_by_8() {
        let dr0 = debug_register_offset(0);
        let dr1 = debug_register_offset(1);
        let dr7 = debug_register_offset(7);
        assert_eq!(dr1, dr0 + 8);
        assert_eq!(dr7, dr0 + 56);
    }

    #[test]
    fn default_context_is_empty_populated() {
        let ctx = ThreadContext::default();
        assert_eq!(ctx.populated.0, 0);
        assert_eq!(ctx.dr, [0; 8]);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn get_thread_context_round_trips_integer_regs_against_self_fork() {
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                // Spin forever; parent stops us, reads context, lets us go.
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(50));
                let _ = attach_and_wait(child).expect("attach");

                let ctx =
                    get_thread_context(child, ContextFlags::INTEGER).expect("get integer ctx");
                assert!(ctx.populated.contains(ContextFlags::INTEGER));
                assert!(!ctx.populated.contains(ContextFlags::DEBUG_REGISTERS));
                // RIP must be non-zero (the child is running real code).
                assert!(ctx.regs.rip != 0);

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
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn debug_register_round_trip_against_self_fork() {
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => loop {
                std::thread::sleep(Duration::from_millis(10));
            },
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(50));
                let _ = attach_and_wait(child).expect("attach");

                let dr0_before = read_debug_register(child, 0).expect("read DR0");
                write_debug_register(child, 0, 0xCAFE_BABE_DEAD_BEEF).expect("write DR0");
                let dr0_after = read_debug_register(child, 0).expect("read DR0 back");
                assert_eq!(dr0_after, 0xCAFE_BABE_DEAD_BEEF);
                let _ = dr0_before; // just confirm we can read it

                // Clear and detach.
                write_debug_register(child, 0, 0).expect("clear DR0");
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
}
