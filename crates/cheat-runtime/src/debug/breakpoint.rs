//! Hardware breakpoint set/remove + DR7 encoding — 1:1 port of CE's
//! `ceserver/api.c` x86_64 path:
//! - `SetBreakpoint` x86 chunk (line 1077-1116)
//! - `RemoveBreakpoint` x86 chunk (line 1374-1395)
//!
//! # DR7 layout (x86_64, Intel SDM Vol. 3B §17.2.4)
//!
//! Bits 0-7: L0,G0,L1,G1,L2,G2,L3,G3 — per-breakpoint enable. Local
//! (L*) is what we set; Global (G*) requires the kernel allowing it
//! and is unused here.
//! Bits 16-31: 4-bit field per DR (R/W type + length): bits
//! `[16+i*4..16+i*4+2]` = type (0=execute, 1=write, 2=undef on x86 →
//! we collapse to read+write, 3=read+write), bits
//! `[16+i*4+2..16+i*4+4]` = len (0=1, 1=2, 3=4, 2=8 on x86_64).
//!
//! CE encodes only L# (not G#) — same here.

use std::mem::offset_of;
use std::os::raw::c_void;

use nix::libc::{self, PTRACE_PEEKUSER, PTRACE_POKEUSER};
use nix::unistd::Pid;

use crate::ptrace_helpers::{PtraceError, safe_ptrace};

/// What kind of memory access traps the breakpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BpType {
    /// Execution at `address` — DR7 type = 0.
    Execute = 0,
    /// Write to `address` — DR7 type = 1.
    Write = 1,
    /// Read from `address`. x86 has no read-only watchpoint, so the
    /// hardware setup falls back to `ReadWrite` (mirror CE
    /// `api.c:1087-1088`). We keep the variant for API parity.
    ReadOnly = 2,
    /// Read or write — DR7 type = 3.
    ReadWrite = 3,
}

impl BpType {
    /// The 2-bit value written into DR7 for this breakpoint. The
    /// x86-no-read-only collapse happens here, matching CE.
    pub fn to_dr7_type(self) -> u64 {
        match self {
            BpType::Execute => 0,
            BpType::Write => 1,
            BpType::ReadOnly | BpType::ReadWrite => 3,
        }
    }
}

/// Size of the breakpoint watch window. CE accepts 1/2/4/8 raw byte
/// counts and bucketizes; we expose the canonical four widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpSize {
    Byte = 1,
    Word = 2,
    Dword = 4,
    Qword = 8,
}

impl BpSize {
    /// The 2-bit DR7 length field for this size. CE's mapping
    /// (`api.c:1095-1101`): <=1 → 0, <=2 → 1, else → 3 (4 byte). We
    /// add the x86_64 8-byte length (= 2) explicitly.
    pub fn to_dr7_len(self) -> u64 {
        match self {
            BpSize::Byte => 0,
            BpSize::Word => 1,
            BpSize::Dword => 3,
            BpSize::Qword => 2,
        }
    }
}

/// Encode the type + length + enable bits for `debugreg` into an
/// existing DR7 value. Caller passes the current DR7 (so other
/// breakpoints are preserved) and gets back the value to write back.
/// 1:1 with `api.c:1082-1101`.
///
/// `debugreg` must be 0-3 (DR0..DR3). Higher values are nonsensical
/// (DR4/DR5 are aliased, DR6/DR7 are status/control); the function
/// will silently produce garbage rather than panic — the higher
/// layers validate.
pub fn encode_dr7_set(current_dr7: u64, debugreg: u8, bptype: BpType, bpsize: BpSize) -> u64 {
    let dr = debugreg as u64;
    let enable_bit = 1u64 << (dr * 2);
    let type_field = bptype.to_dr7_type() << (16 + dr * 4);
    let len_field = bpsize.to_dr7_len() << (18 + dr * 4);
    current_dr7 | enable_bit | type_field | len_field
}

/// Clear the type + length + enable bits for `debugreg` in DR7. 1:1
/// with `api.c:1381-1382`.
pub fn encode_dr7_clear(current_dr7: u64, debugreg: u8) -> u64 {
    let dr = debugreg as u64;
    // CE clears bits 2*dr and 2*dr+1 (local + global enable), and
    // the 4-bit type+len field at 16+dr*4.
    let enable_mask = !(3u64 << (dr * 2));
    let type_mask = !(15u64 << (16 + dr * 4));
    current_dr7 & enable_mask & type_mask
}

/// Decode DR6 into the DR index that fired, or `None` if no
/// breakpoint hit (e.g. SIGTRAP from a foreign source). DR6 bits 0-3
/// are B0-B3 — the breakpoint condition flags. We return the lowest
/// set bit; CE handles multiple simultaneous hits by reporting one.
pub fn decode_dr6(dr6: u64) -> Option<u8> {
    let lowest = dr6 & 0xF;
    if lowest == 0 {
        None
    } else {
        Some(lowest.trailing_zeros() as u8)
    }
}

/// Offset of `u_debugreg[idx]` in `struct user`. Both
/// `SetBreakpoint` (api.c:1104) and `RemoveBreakpoint` (api.c:1379)
/// reach the DRs through this same `offsetof(struct user,
/// u_debugreg[N])` arithmetic.
fn debugreg_offset(idx: u8) -> usize {
    offset_of!(libc::user, u_debugreg) + (idx as usize) * 8
}

/// Set a hardware breakpoint on `tid`. The thread must already be
/// ptrace-stopped (the caller — the debug subsystem — owns the
/// suspend/resume coordination).
///
/// 1:1 with `api.c:1082-1107`: PEEKUSER DR7, OR in new bits, POKEUSER
/// the address into the DR register, POKEUSER the updated DR7.
pub fn set_hardware_breakpoint(
    tid: Pid,
    debugreg: u8,
    address: u64,
    bptype: BpType,
    bpsize: BpSize,
) -> Result<(), PtraceError> {
    let current_dr7 = read_debug_reg(tid, 7)?;
    let new_dr7 = encode_dr7_set(current_dr7, debugreg, bptype, bpsize);
    write_debug_reg(tid, debugreg, address)?;
    write_debug_reg(tid, 7, new_dr7)?;
    Ok(())
}

/// Remove the breakpoint at `debugreg`. 1:1 with `api.c:1379-1388`:
/// PEEKUSER DR7, mask out the bits, POKEUSER zero into the DR, POKE
/// back the masked DR7.
pub fn remove_hardware_breakpoint(tid: Pid, debugreg: u8) -> Result<(), PtraceError> {
    let current_dr7 = read_debug_reg(tid, 7)?;
    let new_dr7 = encode_dr7_clear(current_dr7, debugreg);
    write_debug_reg(tid, debugreg, 0)?;
    write_debug_reg(tid, 7, new_dr7)?;
    Ok(())
}

fn read_debug_reg(tid: Pid, idx: u8) -> Result<u64, PtraceError> {
    let off = debugreg_offset(idx) as *mut c_void;
    let v = safe_ptrace(PTRACE_PEEKUSER, tid, off, std::ptr::null_mut())?;
    Ok(v as u64)
}

fn write_debug_reg(tid: Pid, idx: u8, val: u64) -> Result<(), PtraceError> {
    let off = debugreg_offset(idx) as *mut c_void;
    safe_ptrace(PTRACE_POKEUSER, tid, off, val as *mut c_void)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // CE reference: api.c:1082-1101 + 1381-1382. The tables below
    // hand-derive the expected DR7 values from those formulas so a
    // change in the encode helpers fails loudly.

    #[test]
    fn encode_dr7_set_dr0_execute_byte() {
        // dr=0, type=0 (execute), len=0 (byte).
        // expected: L0=1 → bit 0. Type+len field at bits 16-19 = 0.
        assert_eq!(encode_dr7_set(0, 0, BpType::Execute, BpSize::Byte), 0x1);
    }

    #[test]
    fn encode_dr7_set_dr0_write_dword() {
        // dr=0, type=1 (write), len=3 (dword=4).
        // L0=bit0; type field bits 16-17 = 1 (0x10000); len field
        // bits 18-19 = 3 (0xC0000).
        let expected = 0x1 | (1 << 16) | (3 << 18);
        assert_eq!(encode_dr7_set(0, 0, BpType::Write, BpSize::Dword), expected);
    }

    #[test]
    fn encode_dr7_set_dr3_readwrite_qword() {
        // dr=3 → enable bit 6; type field bits 28-29 = 3; len field
        // bits 30-31 = 2 (qword=8).
        let expected = (1u64 << 6) | (3u64 << 28) | (2u64 << 30);
        assert_eq!(
            encode_dr7_set(0, 3, BpType::ReadWrite, BpSize::Qword),
            expected
        );
    }

    #[test]
    fn encode_dr7_set_readonly_collapses_to_readwrite() {
        // x86 has no read-only watchpoint; CE collapses it to type=3.
        let a = encode_dr7_set(0, 1, BpType::ReadOnly, BpSize::Word);
        let b = encode_dr7_set(0, 1, BpType::ReadWrite, BpSize::Word);
        assert_eq!(a, b);
    }

    #[test]
    fn encode_dr7_set_preserves_other_breakpoints() {
        // Start with DR1 set (L1=bit2, plus some type bits).
        let dr1_existing = (1u64 << 2) | (1u64 << 20);
        let combined = encode_dr7_set(dr1_existing, 0, BpType::Execute, BpSize::Byte);
        // Must still have L1 + DR1 type bit set.
        assert_eq!(combined & (1u64 << 2), 1u64 << 2);
        assert_eq!(combined & (1u64 << 20), 1u64 << 20);
        // And L0 should be on.
        assert_eq!(combined & 1, 1);
    }

    #[test]
    fn encode_dr7_clear_drops_only_target() {
        // Set DR0 + DR1 first, then clear DR0 — DR1 should survive.
        let with_dr0 = encode_dr7_set(0, 0, BpType::Write, BpSize::Word);
        let with_both = encode_dr7_set(with_dr0, 1, BpType::Execute, BpSize::Byte);
        let cleared = encode_dr7_clear(with_both, 0);
        // DR0 L bit off.
        assert_eq!(cleared & 1, 0);
        // DR0 type+len field cleared.
        assert_eq!(cleared & (15u64 << 16), 0);
        // DR1 L bit still on.
        assert_eq!(cleared & (1u64 << 2), 1u64 << 2);
    }

    #[test]
    fn decode_dr6_no_hit() {
        assert_eq!(decode_dr6(0), None);
        // Bit 13 (BD) set but no B0-B3.
        assert_eq!(decode_dr6(0x2000), None);
    }

    #[test]
    fn decode_dr6_each_dr() {
        assert_eq!(decode_dr6(0b0001), Some(0));
        assert_eq!(decode_dr6(0b0010), Some(1));
        assert_eq!(decode_dr6(0b0100), Some(2));
        assert_eq!(decode_dr6(0b1000), Some(3));
    }

    #[test]
    fn decode_dr6_lowest_wins_on_multiple_hits() {
        // DR1 + DR3 fired simultaneously → return DR1 (lowest).
        assert_eq!(decode_dr6(0b1010), Some(1));
    }

    #[test]
    fn debugreg_offset_strides_by_8() {
        let dr0 = debugreg_offset(0);
        for i in 0..8 {
            assert_eq!(debugreg_offset(i), dr0 + (i as usize) * 8);
        }
    }
}
