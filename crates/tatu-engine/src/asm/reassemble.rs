//! CE-AA `reassemble()` support: relocate a single x86-64 instruction.
//!
//! When a hook overwrites the target's code with a `jmp` into a codecave, it
//! clobbers whatever multi-byte instruction was sitting at the hook site. The
//! codecave has to replay that displaced instruction before jumping back —
//! but it now lives at a different address, so any rip-relative memory operand
//! or relative branch encoded in it points to the wrong place. CE's
//! `reassemble(addr)` reads the instruction at `addr`, decodes it, and
//! re-encodes it at the codecave cursor with those displacements recomputed
//! against the new RIP.
//!
//! We delegate the decode → re-encode to iced-x86: [`Decoder`] reads the
//! instruction (its absolute branch / memory targets are preserved in the
//! decoded [`iced_x86::Instruction`]), and [`BlockEncoder`] re-emits it at the
//! destination IP, fixing the encoding — including promoting a `jcc rel8` to
//! `jcc rel32` when the original short branch can no longer reach its target
//! from the codecave (the common case: the displaced instruction was a short
//! conditional jump whose target is now 2 GiB+ away).

use iced_x86::{
    BlockEncoder, BlockEncoderOptions, Decoder, DecoderOptions, InstructionBlock, Register,
};

use super::AsmError;

/// Decode the first instruction in `code` (as if it were located at
/// `src_addr`) and re-encode it to run correctly at `dest_addr`.
///
/// `code` should hold at least the bytes of one instruction; callers pass the
/// x86-64 maximum of 15 bytes read from the target. Returns the re-encoded
/// bytes, whose length may differ from the original instruction's (branch
/// promotion changes the encoding size).
pub fn reassemble_instruction(
    code: &[u8],
    src_addr: u64,
    dest_addr: u64,
) -> Result<Vec<u8>, AsmError> {
    let mut decoder = Decoder::with_ip(64, code, src_addr, DecoderOptions::NONE);
    if !decoder.can_decode() {
        return Err(AsmError::Unsupported(format!(
            "reassemble: no bytes to decode at {src_addr:#x}"
        )));
    }
    let instr = decoder.decode();
    if instr.is_invalid() {
        return Err(AsmError::Unsupported(format!(
            "reassemble: undecodable instruction at {src_addr:#x}"
        )));
    }

    let instrs = [instr];
    let block = InstructionBlock::new(&instrs, dest_addr);
    let encoded = BlockEncoder::encode(64, block, BlockEncoderOptions::NONE).map_err(|e| {
        AsmError::Unsupported(format!(
            "reassemble: re-encode at {dest_addr:#x} failed: {e}"
        ))
    })?;
    Ok(encoded.code_buffer)
}

/// Re-encode a single instruction (already assembled in `bytes`, located at
/// `base`) so its absolute memory operand becomes RIP-relative pointing at
/// `target`.
///
/// `bytes` must hold an instruction whose memory operand iced encoded as an
/// absolute `[disp32]` (SIB, no base/index) — the caller produces this by
/// compiling with a small placeholder address. In long mode an absolute
/// target above ±2 GiB can't be reached by `[disp32]`, so CE (and this) emit
/// `[rip+disp32]` instead, which iced computes from the absolute `target` and
/// the instruction's own next-IP. Required by codecaves that load constants
/// stored in the cave (`movss xmm0,[Override]`) when the module — and thus the
/// cave — lives above the 4 GiB line.
pub(super) fn retarget_abs_to_rip(
    bytes: &[u8],
    base: u64,
    target: u64,
) -> Result<Vec<u8>, AsmError> {
    let mut decoder = Decoder::with_ip(64, bytes, base, DecoderOptions::NONE);
    if !decoder.can_decode() {
        return Err(AsmError::Unsupported(
            "rip-relative retarget: no bytes to decode".into(),
        ));
    }
    let mut instr = decoder.decode();
    if instr.is_invalid() {
        return Err(AsmError::Unsupported(
            "rip-relative retarget: undecodable instruction".into(),
        ));
    }

    instr.set_memory_base(Register::RIP);
    instr.set_memory_displacement64(target);
    instr.set_memory_displ_size(4);

    let instrs = [instr];
    let block = InstructionBlock::new(&instrs, base);
    let encoded = BlockEncoder::encode(64, block, BlockEncoderOptions::NONE).map_err(|e| {
        AsmError::Unsupported(format!("rip-relative retarget: re-encode failed: {e}"))
    })?;
    Ok(encoded.code_buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A short conditional branch displaced into a codecave within rel32 range
    /// must promote to the rel32 form while keeping its absolute target. This
    /// is exactly the DD2 `reassemble(HeightDieForHumanWorkHook+4)` case:
    /// `75 06` (`jne $+8`) at the hook site, replayed from a codecave the
    /// allocator placed near the module (CE relies on the same ±2 GiB locality,
    /// as does every `jmp rel32` hook).
    #[test]
    fn short_jcc_promotes_to_rel32_preserving_target() {
        let src = 0x1_4000_0000_u64;
        // `75 06` = jne rel8 +6 → absolute target = src + 2 + 6.
        let target = src.wrapping_add(2 + 6);
        let dest = src.wrapping_add(0x10_0000); // 1 MiB away — rel8 can't reach, rel32 can.

        let bytes = reassemble_instruction(&[0x75, 0x06], src, dest).unwrap();
        // 0F 85 <rel32> = 6 bytes.
        assert_eq!(bytes.len(), 6, "expected jcc rel32 form, got {bytes:02X?}");
        assert_eq!(&bytes[..2], &[0x0F, 0x85], "jne rel32 opcode");
        let rel = i32::from_le_bytes(bytes[2..6].try_into().unwrap()) as i64;
        let recomputed = (dest as i64).wrapping_add(6).wrapping_add(rel) as u64;
        assert_eq!(
            recomputed, target,
            "absolute branch target must be preserved"
        );
    }

    /// A position-independent instruction (no rip-relative operand, no
    /// relative branch) re-encodes byte-for-byte regardless of destination.
    #[test]
    fn position_independent_instruction_unchanged() {
        // 48 8B 42 10 = mov rax,[rdx+10].
        let orig = [0x48, 0x8B, 0x42, 0x10];
        let bytes = reassemble_instruction(&orig, 0x1000, 0x1_4001_0000).unwrap();
        assert_eq!(bytes, orig);
    }

    /// A rip-relative load must keep pointing at the same absolute address
    /// after relocation: the disp32 is recomputed against the new RIP.
    #[test]
    fn rip_relative_displacement_recomputed() {
        let src = 0x1_4000_0000_u64;
        // 8B 05 00 00 00 00 = mov eax,[rip+0] → absolute target = src + 6.
        let target = src.wrapping_add(6);
        let dest = src.wrapping_add(0x10_0000); // within ±2 GiB of the target.

        let bytes = reassemble_instruction(&[0x8B, 0x05, 0, 0, 0, 0], src, dest).unwrap();
        assert_eq!(bytes.len(), 6, "rip-relative form stays 6 bytes");
        assert_eq!(&bytes[..2], &[0x8B, 0x05]);
        let disp = i32::from_le_bytes(bytes[2..6].try_into().unwrap()) as i64;
        let recomputed = (dest as i64).wrapping_add(6).wrapping_add(disp) as u64;
        assert_eq!(recomputed, target, "rip-relative target must be preserved");
    }
}
