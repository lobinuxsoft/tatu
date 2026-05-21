//! Backend-agnostic memory primitives shared between the Linux ptrace
//! runtime (`cheat-runtime`) and the Win32 in-prefix bridge
//! (`tatu-bridge`).
//!
//! The crate is split into:
//!
//! - [`MemoryAccess`] — the read/write trait every backend implements.
//!   `cheat-runtime` ships an impl over `process_vm_readv` /
//!   `process_vm_writev`; `tatu-bridge` ships one over
//!   `ReadProcessMemory` / `WriteProcessMemory`.
//! - [`pattern`] — the AOB scanner (`Pattern` + `parse` + pure
//!   `scan(haystack)` + generic [`pattern::scan_range`] over any
//!   `MemoryAccess`). Cheat Engine `??`-wildcards, memchr fast-path on
//!   the first literal byte.
//! - [`chain`] — pointer-chain walking + typed value read/write.
//!   `walk_chain` iterates offsets in REVERSE (CE convention). The
//!   bridge side carried a `chain.rs` copy through Phase 4; that
//!   duplication ends here.
//! - [`addr_expr`] — pure parser for CE `<Address>` strings
//!   (`"[symbol]"`, `"[symbol]+1A"`, `"0xDEADBEEF"`). No memory I/O;
//!   the I/O variant lives in [`chain`].
//!
//! Wire-format types ([`tatu_proto::WireValue`] /
//! [`tatu_proto::WireVType`]) come from `tatu-proto` directly — that
//! crate stays the single source of truth for what crosses the
//! tracker ↔ bridge socket.

pub mod addr_expr;
pub mod chain;
pub mod pattern;

pub use tatu_proto::{WireVType, WireValue};

/// Backend-agnostic remote-memory I/O. Every method targets a single
/// remote address space; the concrete impl owns whatever handle /
/// PID / file descriptor it needs to reach that address space.
///
/// `Error` is an associated type so backend-specific errors
/// (`nix::Errno` from `process_vm_readv` vs `windows::core::Error`
/// from `ReadProcessMemory`) round-trip without lossy stringification.
pub trait MemoryAccess {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Read exactly `len` bytes at `addr`. Short reads are an error —
    /// callers that tolerate truncation use [`Self::read_partial`].
    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, Self::Error>;

    /// Permissive read: returns whatever the kernel transferred, up to
    /// `len`. Empty on full failure. Used by the AOB scanner to glide
    /// across unmapped holes inside a region.
    fn read_partial(&mut self, addr: u64, len: usize) -> Vec<u8>;

    /// Write `bytes.len()` bytes starting at `addr`.
    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Convenience: read 8 LE bytes at `addr` and decode as `u64`.
/// Pointer-chain walking is the hot path for this.
pub fn read_u64<M: MemoryAccess>(mem: &mut M, addr: u64) -> Result<u64, M::Error> {
    let bytes = mem.read(addr, 8)?;
    Ok(u64::from_le_bytes(bytes.as_slice().try_into().unwrap()))
}
