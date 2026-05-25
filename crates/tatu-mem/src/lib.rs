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

use serde::{Deserialize, Serialize};

/// Type tag for [`WireValue`] payloads. Mirror of
/// `cheat_runtime::manifest::VType`. Originally lived in `tatu-proto`
/// as the wire-format for the tracker ↔ bridge socket; post-pivot
/// #128 the bridge is gone, types stay here as the local shared
/// vocabulary between `cheat-runtime` and the tracker handlers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WireVType {
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl WireVType {
    pub const fn size_bytes(self) -> usize {
        match self {
            WireVType::U32 | WireVType::I32 | WireVType::F32 => 4,
            WireVType::U64 | WireVType::I64 | WireVType::F64 => 8,
        }
    }
}

/// Type-tagged numeric value — mirror of `cheat_runtime::chain::Value`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum WireValue {
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl WireValue {
    pub const fn vtype(&self) -> WireVType {
        match self {
            WireValue::U32(_) => WireVType::U32,
            WireValue::I32(_) => WireVType::I32,
            WireValue::U64(_) => WireVType::U64,
            WireValue::I64(_) => WireVType::I64,
            WireValue::F32(_) => WireVType::F32,
            WireValue::F64(_) => WireVType::F64,
        }
    }

    pub fn to_le_bytes(self) -> Vec<u8> {
        match self {
            WireValue::U32(v) => v.to_le_bytes().to_vec(),
            WireValue::I32(v) => v.to_le_bytes().to_vec(),
            WireValue::U64(v) => v.to_le_bytes().to_vec(),
            WireValue::I64(v) => v.to_le_bytes().to_vec(),
            WireValue::F32(v) => v.to_le_bytes().to_vec(),
            WireValue::F64(v) => v.to_le_bytes().to_vec(),
        }
    }

    pub fn from_le_bytes(vtype: WireVType, bytes: &[u8]) -> Option<Self> {
        if bytes.len() != vtype.size_bytes() {
            return None;
        }
        Some(match vtype {
            WireVType::U32 => WireValue::U32(u32::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::I32 => WireValue::I32(i32::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::U64 => WireValue::U64(u64::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::I64 => WireValue::I64(i64::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::F32 => WireValue::F32(f32::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::F64 => WireValue::F64(f64::from_le_bytes(bytes.try_into().ok()?)),
        })
    }
}

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
