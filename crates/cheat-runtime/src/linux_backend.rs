//! [`tatu_engine::Backend`] implementation for the Linux ptrace
//! runtime. Wraps the existing [`ProcessVmMem`] adapter (which already
//! handles read/write via `process_vm_readv` / `process_vm_writev` +
//! ptrace POKEDATA) and adds the platform-specific extras the
//! executor needs: remote `mmap` for codecaves, region enumeration
//! via `/proc/<pid>/maps`, and ptrace attach/detach.
//!
//! Two complementary adapters live in this crate:
//! - [`ProcessVmMem`] — bare `tatu_mem::MemoryAccess` with the local
//!   [`RuntimeError`] surfaced; what the chain walker and value
//!   reads use because they don't need alloc / attach.
//! - [`LinuxBackend`] (this file) — full
//!   `tatu_engine::Backend`. Lifts `RuntimeError` /
//!   [`alloc::AllocError`] / etc. into the trait's boxed `BackendError`
//!   so the executor can stay free of `where E:` noise.

use std::io;

use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;
use tatu_engine::backend::{Backend, BackendError, ReadableRegion, RegionPerms};
use tatu_mem::MemoryAccess;

use crate::alloc as remote_alloc;
use crate::maps::read_maps;
use crate::memory::RuntimeError;
use crate::memory_access::ProcessVmMem;

/// [`Backend`] impl for the native Linux ptrace runtime.
pub struct LinuxBackend {
    inner: ProcessVmMem,
}

impl LinuxBackend {
    pub fn new(pid: Pid) -> Self {
        Self {
            inner: ProcessVmMem::new(pid),
        }
    }

    pub fn pid(&self) -> Pid {
        self.inner.pid()
    }
}

impl MemoryAccess for LinuxBackend {
    type Error = BackendError;

    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, BackendError> {
        self.inner.read(addr, len).map_err(BackendError::new)
    }

    fn read_partial(&mut self, addr: u64, len: usize) -> Vec<u8> {
        self.inner.read_partial(addr, len)
    }

    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), BackendError> {
        self.inner.write(addr, bytes).map_err(BackendError::new)
    }
}

impl Backend for LinuxBackend {
    fn alloc(&mut self, size: usize, near_hint: Option<u64>) -> Result<u64, BackendError> {
        // The legacy alloc_remote API takes (pid, size, mode, near).
        // Mode `None` defaults to MAP_32BIT-or-PROT_EXEC-RWX inside
        // alloc.rs.
        remote_alloc::alloc_remote(self.inner.pid(), size, None, near_hint)
            .map_err(BackendError::new)
    }

    fn dealloc(&mut self, addr: u64, size: usize) -> Result<(), BackendError> {
        remote_alloc::dealloc_remote(self.inner.pid(), addr, size).map_err(BackendError::new)
    }

    fn readable_regions(&mut self) -> Result<Vec<ReadableRegion>, BackendError> {
        let regions = read_maps(self.inner.pid())
            .map_err(|e: io::Error| BackendError::new(RuntimeError::Io(e)))?;
        Ok(regions
            .into_iter()
            .filter(|r| r.perms.read)
            .map(|r| ReadableRegion {
                start: r.start,
                end: r.end,
                perms: RegionPerms {
                    read: r.perms.read,
                    write: r.perms.write,
                    execute: r.perms.execute,
                },
                path: r.path,
            })
            .collect())
    }

    fn attach(&mut self) -> bool {
        let pid = self.inner.pid();
        if pid == Pid::this() {
            return false;
        }
        if ptrace::attach(pid).is_err() {
            return false;
        }
        loop {
            match waitpid(pid, None) {
                Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {
                    self.inner.set_attached(true);
                    return true;
                }
                Ok(WaitStatus::Stopped(_, sig)) => {
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

    fn detach(&mut self) {
        if self.inner.is_attached() {
            let _ = ptrace::detach(self.inner.pid(), None);
            self.inner.set_attached(false);
        }
    }

    fn flush_instruction_cache(&mut self, _addr: u64, _len: usize) -> Result<(), BackendError> {
        // Linux POKEDATA writes are coherent against the target's
        // i-cache (the kernel issues the necessary invalidation in
        // `ptrace_writedata`). `process_vm_writev` is fine too — for
        // .text patches we go through POKEDATA inside the attach
        // window. No explicit flush needed.
        Ok(())
    }
}
