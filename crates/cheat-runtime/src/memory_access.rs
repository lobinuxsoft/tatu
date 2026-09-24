//! [`tatu_mem::MemoryAccess`] adapter for the Linux ptrace runtime.
//!
//! Wraps a target [`Pid`] and dispatches reads + writes to
//! [`crate::memory`]'s syscall-level helpers. The shared algorithms in
//! `tatu-mem` (AOB pattern scan, pointer-chain walk, typed read/write)
//! go through this adapter so the same logic compiles bit-for-bit
//! against this backend and against `tatu-bridge`'s Win32 backend.

use nix::unistd::Pid;

use crate::memory::{self, RuntimeError};
use tatu_mem::MemoryAccess;

/// Stateful adapter — the `attached` flag tells [`MemoryAccess::write`]
/// to use [`memory::write_bytes_attached`] (ptrace `PTRACE_POKEDATA`,
/// safe inside an existing attach session) instead of the default
/// `process_vm_writev`-with-EFAULT-fallback path.
///
/// The executor flips `attached` to `true` around a batch of patch
/// writes so it pays the ptrace attach cost once for the whole batch,
/// matching CE's `autoassembler.pas:4116` behaviour.
pub struct ProcessVmMem {
    pid: Pid,
    attached: bool,
}

impl ProcessVmMem {
    pub fn new(pid: Pid) -> Self {
        Self {
            pid,
            attached: false,
        }
    }

    pub fn with_attached(pid: Pid, attached: bool) -> Self {
        Self { pid, attached }
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn set_attached(&mut self, attached: bool) {
        self.attached = attached;
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }
}

impl MemoryAccess for ProcessVmMem {
    type Error = RuntimeError;

    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, Self::Error> {
        memory::read_bytes(self.pid, addr, len)
    }

    fn read_partial(&mut self, addr: u64, len: usize) -> Vec<u8> {
        // tatu_mem::read_partial returns empty on any failure; that
        // matches CE's `ceserver/api.c::ReadProcessMemory` callers
        // which ignore short-read returns.
        memory::read_bytes_partial(self.pid, addr, len).unwrap_or_default()
    }

    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), Self::Error> {
        if self.attached {
            memory::write_bytes_attached(self.pid, addr, bytes)
        } else {
            memory::write_bytes(self.pid, addr, bytes)
        }
    }
}
