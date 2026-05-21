//! [`Backend`] — extension of [`tatu_mem::MemoryAccess`] with the
//! handful of platform-specific operations the autoassembler executor
//! needs beyond bulk read/write: remote allocation for codecaves,
//! readable-region enumeration for AOB scans, optional thread
//! suspension around a write batch, and an instruction-cache flush
//! hook for cross-process patches.
//!
//! Implemented by:
//! - `cheat-runtime`'s `LinuxBackend` (ptrace runtime), where alloc
//!   goes through a remote `mmap`, regions come from `/proc/<pid>/maps`,
//!   attach/detach is `PTRACE_ATTACH` + `PTRACE_DETACH`, and i-cache
//!   flush is a no-op (Linux's `process_vm_writev` already handles
//!   coherence + Linux ptrace `POKEDATA` writes are instruction-cache
//!   aware on x86_64).
//! - `tatu-bridge`'s `Win32Backend` (PR 7B), where alloc is
//!   `VirtualAllocEx`, regions come from `VirtualQueryEx`, attach is
//!   a no-op (`OpenProcess` already gives R/W on the target), and
//!   i-cache flush is `FlushInstructionCache` (required cross-process
//!   on x86_64).

use std::path::PathBuf;

use tatu_mem::MemoryAccess;

/// Permission bits on a readable region. The autoassembler scanner
/// uses [`Self::executable`] to bias toward code regions when an
/// `aobscanmodule` pattern is biased toward `.text`-style bytes; the
/// shared scanner sweeps anything readable today, but the field is
/// here so a more aggressive scanner can opt in later.
#[derive(Debug, Clone, Copy)]
pub struct RegionPerms {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// One contiguous readable region of the target's address space. The
/// `path` field is best-effort (file-backed mappings have one,
/// anonymous mappings don't).
#[derive(Debug, Clone)]
pub struct ReadableRegion {
    pub start: u64,
    pub end: u64,
    pub perms: RegionPerms,
    pub path: PathBuf,
}

impl ReadableRegion {
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

/// Errors a backend operation can surface. The trait wraps a boxed
/// `dyn Error` in a concrete newtype (instead of using
/// `Box<dyn Error + Send + Sync>` directly) so that:
///
/// - the type satisfies `Sized + std::error::Error + Send + Sync +
///   'static` cleanly — exactly what
///   `tatu_mem::MemoryAccess::Error`'s bound asks for;
/// - the executor stays free of `ExecError<B::Error>` propagation
///   noise across every `?`;
/// - both backends can wrap their own native error types (Linux
///   `nix::Errno` + `RuntimeError` on one side, Win32 `i32`
///   GetLastError on the other) without one having to lossy-stringify
///   into the other's shape.
#[derive(Debug)]
pub struct BackendError(pub Box<dyn std::error::Error + Send + Sync + 'static>);

impl BackendError {
    pub fn new<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        BackendError(Box::new(e))
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// The platform-specific operations the autoassembler executor needs
/// on top of [`MemoryAccess`]. Implementations are stateful — they
/// own a `Pid` / `HANDLE` / equivalent — but the trait itself only
/// constrains behaviour.
pub trait Backend: MemoryAccess<Error = BackendError> {
    /// Allocate `size` bytes of remote memory, optionally near
    /// `near_hint`. Returns the address of the new region.
    ///
    /// The Linux impl performs a remote `mmap` (executable RWX since
    /// codecaves carry code). The Win32 impl calls `VirtualAllocEx`
    /// with `MEM_COMMIT | MEM_RESERVE` + `PAGE_EXECUTE_READWRITE`.
    fn alloc(&mut self, size: usize, near_hint: Option<u64>) -> Result<u64, BackendError>;

    /// Release a region previously returned by [`Self::alloc`].
    fn dealloc(&mut self, addr: u64, size: usize) -> Result<(), BackendError>;

    /// Enumerate the readable regions of the target. The shared AOB
    /// scanner iterates this list and runs the masked-byte scan over
    /// every entry — see `tatu_mem::pattern::scan_range`.
    fn readable_regions(&mut self) -> Result<Vec<ReadableRegion>, BackendError>;

    /// Attach to / pause the target for an upcoming batch of writes.
    /// Returns `true` if the attach succeeded — Linux self-pid /
    /// `EPERM` / `ESRCH` falls back to `false` and the executor uses
    /// the unattached write path. Win32 backends `OpenProcess`
    /// already gives the right access; their impl returns `false`
    /// and skips the matching [`Self::detach`].
    fn attach(&mut self) -> bool;

    /// Counterpart to [`Self::attach`]. Called even when `attach`
    /// returned `false` so impls can keep their attach/detach
    /// bookkeeping symmetrical.
    fn detach(&mut self);

    /// Flush the target's instruction cache over `[addr, addr+len)`.
    /// Cross-process patches on x86_64 leave the target's pipeline
    /// holding pre-patch bytes until an explicit invalidation; the
    /// Win32 impl uses `FlushInstructionCache`. Linux's `POKEDATA`
    /// already handles coherence, so the Linux impl is a no-op.
    fn flush_instruction_cache(&mut self, _addr: u64, _len: usize) -> Result<(), BackendError> {
        Ok(())
    }
}
