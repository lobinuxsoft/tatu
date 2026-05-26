//! Pointer-chain walker + typed value read/write, generic over
//! [`MemoryAccess`].
//!
//! [`walk_chain`] iterates offsets in REVERSE — CE's
//! `TMemoryRecord.GetRealAddress` convention. The Linux backend
//! (`cheat-runtime`) and the Win32 bridge backend (`tatu-bridge`)
//! both go through this function so a `.CT` table's `<Offsets>`
//! resolves to the same leaf address on either side.
//!
//! [`read_value`] / [`write_value`] dispatch on [`WireVType`] and
//! decode the read bytes via [`WireValue::from_le_bytes`]. The
//! "decode failed" branch is only reachable if the backend reports a
//! mismatched read length (e.g. permissive reads on the boundary of
//! an unmapped region).

use crate::{MemoryAccess, WireVType, WireValue, read_u64};

#[derive(Debug, thiserror::Error)]
pub enum ChainError<E: std::error::Error + Send + Sync + 'static> {
    #[error("memory: {0}")]
    Memory(#[source] E),
    #[error("decoded {len} bytes at {addr:#x} are not a valid {vtype:?}")]
    Decode {
        addr: u64,
        len: usize,
        vtype: WireVType,
    },
}

impl<E: std::error::Error + Send + Sync + 'static> ChainError<E> {
    fn mem(e: E) -> Self {
        ChainError::Memory(e)
    }
}

/// Walk `offsets` from `base`, returning the final pointer address.
/// Offsets are applied in reverse — `offsets.last()` is dereferenced
/// first, `offsets.first()` is the last hop. Empty `offsets` returns
/// `base` unchanged.
pub fn walk_chain<M: MemoryAccess>(
    mem: &mut M,
    base: u64,
    offsets: &[u64],
) -> Result<u64, ChainError<M::Error>> {
    let mut cur = base;
    for &offset in offsets.iter().rev() {
        let pointer = read_u64(mem, cur).map_err(ChainError::mem)?;
        cur = pointer.wrapping_add(offset);
    }
    Ok(cur)
}

pub fn read_value<M: MemoryAccess>(
    mem: &mut M,
    addr: u64,
    vtype: WireVType,
) -> Result<WireValue, ChainError<M::Error>> {
    let raw = mem
        .read(addr, vtype.size_bytes())
        .map_err(ChainError::mem)?;
    WireValue::from_le_bytes(vtype, &raw).ok_or(ChainError::Decode {
        addr,
        len: raw.len(),
        vtype,
    })
}

pub fn write_value<M: MemoryAccess>(
    mem: &mut M,
    addr: u64,
    value: WireValue,
) -> Result<(), ChainError<M::Error>> {
    mem.write(addr, &value.to_le_bytes())
        .map_err(ChainError::mem)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal in-memory backend used by both chain and addr_expr tests.
    pub(super) struct InMemBackend {
        pub base: u64,
        pub data: Vec<u8>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("oob @ {0:#x}")]
    pub(super) struct Oob(pub u64);

    impl MemoryAccess for InMemBackend {
        type Error = Oob;
        fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, Oob> {
            let start = addr.checked_sub(self.base).ok_or(Oob(addr))? as usize;
            self.data
                .get(start..start + len)
                .map(<[u8]>::to_vec)
                .ok_or(Oob(addr))
        }
        fn read_partial(&mut self, _: u64, _: usize) -> Vec<u8> {
            Vec::new()
        }
        fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), Oob> {
            let start = addr.checked_sub(self.base).ok_or(Oob(addr))? as usize;
            let end = start + bytes.len();
            if end > self.data.len() {
                return Err(Oob(addr));
            }
            self.data[start..end].copy_from_slice(bytes);
            Ok(())
        }
    }

    #[test]
    fn walk_chain_empty_offsets_returns_base() {
        let mut mem = InMemBackend {
            base: 0x1000,
            data: vec![0; 64],
        };
        assert_eq!(walk_chain(&mut mem, 0x1234, &[]).unwrap(), 0x1234);
    }

    #[test]
    fn walk_chain_two_hops_iterates_in_reverse() {
        // Layout:
        //   base=0x1000
        //   [0x1000] holds 0x1020 (first deref target).
        //   [0x1020 + 0x10] holds 0x1040 (second deref target).
        //   We walk with offsets = [0x4, 0x10]. Reverse iteration:
        //     hop 1: read u64 at base(0x1000) + offsets[1](0x10)?
        //   ... Actually walk_chain does:
        //     cur = base = 0x1000
        //     loop offsets.rev() => [0x10, 0x4]:
        //       hop 1: ptr = read_u64(cur=0x1000) = 0x1020;
        //              cur = 0x1020 + 0x10 = 0x1030
        //       hop 2: ptr = read_u64(cur=0x1030) = 0x1040;
        //              cur = 0x1040 + 0x4 = 0x1044
        let mut data = vec![0u8; 256];
        data[0..8].copy_from_slice(&0x1020u64.to_le_bytes());
        data[0x30..0x38].copy_from_slice(&0x1040u64.to_le_bytes());
        let mut mem = InMemBackend { base: 0x1000, data };

        let result = walk_chain(&mut mem, 0x1000, &[0x4, 0x10]).unwrap();
        assert_eq!(result, 0x1044);
    }

    #[test]
    fn read_value_dispatches_on_vtype() {
        let mut data = vec![0u8; 16];
        data[0..4].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        data[8..16].copy_from_slice(&(-7i64).to_le_bytes());
        let mut mem = InMemBackend { base: 0, data };

        let v32 = read_value(&mut mem, 0, WireVType::U32).unwrap();
        assert_eq!(v32, WireValue::U32(0x1122_3344));

        let v64 = read_value(&mut mem, 8, WireVType::I64).unwrap();
        assert_eq!(v64, WireValue::I64(-7));
    }

    #[test]
    fn write_value_round_trips() {
        let mut mem = InMemBackend {
            base: 0,
            data: vec![0u8; 16],
        };
        write_value(&mut mem, 0, WireValue::F32(2.5)).unwrap();
        let v = read_value(&mut mem, 0, WireVType::F32).unwrap();
        assert_eq!(v, WireValue::F32(2.5));
    }
}
