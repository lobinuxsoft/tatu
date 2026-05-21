//! Attach/detach + rollback path shared by `ActiveCheat::disable` and the
//! Drop guard. Kept separate from the `Engine` write path so the two
//! lifecycles (ENABLE vs DISABLE) don't tangle their ptrace state.

use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;
use tatu_mem::MemoryAccess;

use crate::alloc;
use crate::memory_access::ProcessVmMem;

use super::active::ActiveCheat;
use super::error::ExecError;

/// Attach to the main thread of `pid` and wait for it to stop on SIGSTOP.
/// Returns `true` on success — the caller must `ptrace::detach` later.
/// Returns `false` if the attach is impossible (self-pid, EPERM, ESRCH);
/// the caller falls back to a non-attached write path in that case.
pub(super) fn attach_main_thread(pid: Pid) -> bool {
    if pid == Pid::this() {
        return false;
    }
    if ptrace::attach(pid).is_err() {
        return false;
    }
    loop {
        match waitpid(pid, None) {
            Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => return true,
            Ok(WaitStatus::Stopped(_, sig)) => {
                // Spurious signal arrived first — forward and keep waiting
                // for our SIGSTOP.
                if ptrace::cont(pid, sig).is_err() {
                    return false;
                }
            }
            Ok(_) | Err(_) => {
                let _ = ptrace::detach(pid, None);
                return false;
            }
        }
    }
}

/// Restore every byte the active cheat overwrote and release every
/// codecave it allocated. Mirrors the ENABLE write-pass structure: one
/// ptrace attach for the whole undo batch, POKEDATA writes under that
/// pause, detach once at the end.
///
/// Errors no longer get swallowed — the first restore failure aborts and
/// returns, leaving the remaining undo entries on the stack so a caller
/// can decide whether to retry. The dealloc loop runs regardless because
/// dealloc has its own attach lifecycle (CE-style remote `munmap`).
pub(super) fn rollback(active: &mut ActiveCheat) -> Result<(), ExecError> {
    let attached = attach_main_thread(active.pid);
    let restore_result = restore_all_writes(active, attached);
    if attached {
        let _ = ptrace::detach(active.pid, None);
    }
    // Always drain allocs even if a restore failed — leaving codecaves
    // mapped in the game would compound the problem, and `dealloc_remote`
    // owns its own ptrace attach so we don't need ours.
    let mut dealloc_err: Option<ExecError> = None;
    for (_, (addr, size)) in active.allocs.drain() {
        if let Err(e) = alloc::dealloc_remote(active.pid, addr, size) {
            // Keep the first error; subsequent ones are likely the same
            // root cause (e.g. game already exited).
            if dealloc_err.is_none() {
                dealloc_err = Some(ExecError::Alloc(e));
            }
        }
    }
    restore_result?;
    match dealloc_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn restore_all_writes(active: &mut ActiveCheat, attached: bool) -> Result<(), ExecError> {
    let mut mem = ProcessVmMem::with_attached(active.pid, attached);
    while let Some((addr, bytes)) = active.undo.pop() {
        if let Err(e) = mem.write(addr, &bytes) {
            // Push the entry back so a retry can resume from here.
            active.undo.push((addr, bytes));
            return Err(ExecError::Memory(e));
        }
    }
    Ok(())
}
