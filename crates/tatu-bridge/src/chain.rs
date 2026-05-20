//! Pointer-chain walker — the Win32 port of `cheat_runtime::chain`'s
//! address-resolution + typed read/write, operating against a remote
//! `HANDLE` instead of `process_vm_readv` on a `Pid`.
//!
//! Offsets are walked in REVERSE — CE convention. See the doc comment
//! on `cheat_runtime::chain` for the rationale; this module duplicates
//! the algorithm bit-for-bit so a `.CT` table's `<Offsets>{ 13C, 8B8,
//! 2D0 }` resolves identically on either backend.

use windows_sys::Win32::Foundation::HANDLE;

use tatu_proto::{WireVType, WireValue};

use super::remote_mem::{RemoteMemError, read_remote, read_u64, write_remote};

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("remote memory: {0}")]
    Memory(#[from] RemoteMemError),
    #[error("decoded {len} bytes at {addr:#x} are not a valid {vtype:?}")]
    DecodeValue {
        addr: u64,
        len: usize,
        vtype: WireVType,
    },
}

/// Walk `offsets` from `base`, returning the final address. Offsets
/// are applied in reverse — CE semantics; see `cheat_runtime::chain`
/// for the canonical doc.
pub fn walk_chain(process: HANDLE, base: u64, offsets: &[u64]) -> Result<u64, ChainError> {
    let mut cur = base;
    for &offset in offsets.iter().rev() {
        let pointer = read_u64(process, cur)?;
        cur = pointer.wrapping_add(offset);
    }
    Ok(cur)
}

pub fn read_value(process: HANDLE, addr: u64, vtype: WireVType) -> Result<WireValue, ChainError> {
    let raw = read_remote(process, addr, vtype.size_bytes())?;
    WireValue::from_le_bytes(vtype, &raw).ok_or(ChainError::DecodeValue {
        addr,
        len: raw.len(),
        vtype,
    })
}

pub fn write_value(process: HANDLE, addr: u64, value: WireValue) -> Result<(), ChainError> {
    write_remote(process, addr, &value.to_le_bytes())?;
    Ok(())
}
