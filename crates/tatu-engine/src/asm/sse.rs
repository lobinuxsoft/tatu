//! SSE / SSE2 scalar mnemonics: `movss/movsd`, `addss/addsd`, `subss/subsd`,
//! `mulss/mulsd`, `divss/divsd`, `xorps/xorpd`, `comiss/comisd`,
//! `ucomiss/ucomisd`, `cvtsi2ss/sd`, `cvtss2si/sd2si`, `cvtss2sd/sd2ss`.
//!
//! Tier-2 addition (companion to [`super::arith`]'s Tier-1 `nop`/`test`).
//! The FearLess audit (17 .CT / 287 scripts / 8805 raw asm lines) measured
//! SSE2 scalar mnemonics as the dominant gap after T1: ~380 lines (~13% of
//! the corpus) — UE5 / Unity float cheats are universally SSE2. Without
//! these, scripts like Enigma of Fear's `healthnodamage aob`
//! (`addsd xmm0,xmm1; cvtsd2ss xmm5,xmm0; movss [r15+68],xmm5`) error at
//! pass-1 length estimation.
//!
//! Operand shapes recognised:
//!
//! - **Arith** (`addss/sd`, `subss/sd`, `mulss/sd`, `divss/sd`,
//!   `xorps/pd`, `comiss/sd`, `ucomiss/sd`, `cvtsd2ss`, `cvtss2sd`):
//!   `mnem xmm, xmm` or `mnem xmm, mem`. Destination is always xmm.
//! - **Mov** (`movss/sd`): three shapes — `movss xmm, xmm`,
//!   `movss xmm, mem`, `movss mem, xmm`. Memory size matches the scalar
//!   width (4 bytes for SS, 8 bytes for SD).
//! - **Convert int↔float** (`cvtsi2ss/sd`, `cvtss2si`, `cvtsd2si`): mixes
//!   a general-purpose register with an xmm one.
//!
//! Memory operand parsing reuses [`super::operands::try_parse_memory_operand`]
//! — the SSE callers only re-cast the result with the appropriate `dword_ptr`
//! / `qword_ptr` size hint when the AA author omitted the explicit prefix
//! (very common with SSE: `movss [r15+68], xmm5` has no `dword ptr` because
//! the mnemonic itself encodes the width).

use std::collections::HashMap;

use iced_x86::code_asm::{CodeAssembler, dword_ptr, qword_ptr};

use super::AsmError;
use super::operands::{
    MemSize, TypedReg, parse_register, parse_xmm_register, split_two_operands,
    try_parse_memory_operand,
};

/// Scalar precision — selects the matching memory-size hint when the AA
/// source omits the explicit `dword ptr` / `qword ptr` prefix.
#[derive(Debug, Clone, Copy)]
enum Scalar {
    /// Single (32-bit float) — used by `*ss` mnemonics. Memory operand
    /// implicit size is `dword ptr`.
    Single,
    /// Double (64-bit float) — used by `*sd` mnemonics. Memory operand
    /// implicit size is `qword ptr`.
    Double,
}

impl Scalar {
    fn expected_size(self) -> MemSize {
        match self {
            Scalar::Single => MemSize::Dword,
            Scalar::Double => MemSize::Qword,
        }
    }
}

/// Pattern A: arithmetic `mnem xmm, xmm|mem`. Used by `addss/sd`,
/// `subss/sd`, `mulss/sd`, `divss/sd`, `xorps/pd`, `comiss/sd`,
/// `ucomiss/sd`, `cvtsd2ss`, `cvtss2sd`. Destination is always xmm; source
/// is xmm or memory.
fn emit_sse_arith(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
    scalar: Scalar,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_xmm_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{mnem} {lhs_text:?}: lhs must be an xmm register"))
    })?;
    let mut a = CodeAssembler::new(64)?;
    if let Some(rhs) = parse_xmm_register(rhs_text) {
        dispatch_sse_arith(&mut a, mnem, lhs, rhs)?;
    } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
        let size = size_opt.unwrap_or(scalar.expected_size());
        let mem = match size {
            MemSize::Dword => dword_ptr(mem),
            MemSize::Qword => qword_ptr(mem),
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: memory width must match scalar ({})",
                    match scalar {
                        Scalar::Single => "dword",
                        Scalar::Double => "qword",
                    }
                )));
            }
        };
        dispatch_sse_arith_mem(&mut a, mnem, lhs, mem)?;
    } else {
        return Err(AsmError::Unsupported(format!(
            "{mnem} {lhs_text}, {rhs_text:?}: rhs must be xmm or memory"
        )));
    }
    Ok(a.assemble(base)?)
}

fn dispatch_sse_arith(
    a: &mut CodeAssembler,
    mnem: &str,
    lhs: iced_x86::code_asm::AsmRegisterXmm,
    rhs: iced_x86::code_asm::AsmRegisterXmm,
) -> Result<(), AsmError> {
    match mnem {
        "addss" => a.addss(lhs, rhs)?,
        "addsd" => a.addsd(lhs, rhs)?,
        "subss" => a.subss(lhs, rhs)?,
        "subsd" => a.subsd(lhs, rhs)?,
        "mulss" => a.mulss(lhs, rhs)?,
        "mulsd" => a.mulsd(lhs, rhs)?,
        "divss" => a.divss(lhs, rhs)?,
        "divsd" => a.divsd(lhs, rhs)?,
        "xorps" => a.xorps(lhs, rhs)?,
        "xorpd" => a.xorpd(lhs, rhs)?,
        "comiss" => a.comiss(lhs, rhs)?,
        "comisd" => a.comisd(lhs, rhs)?,
        "ucomiss" => a.ucomiss(lhs, rhs)?,
        "ucomisd" => a.ucomisd(lhs, rhs)?,
        "cvtsd2ss" => a.cvtsd2ss(lhs, rhs)?,
        "cvtss2sd" => a.cvtss2sd(lhs, rhs)?,
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown SSE arith mnemonic"
            )));
        }
    };
    Ok(())
}

fn dispatch_sse_arith_mem(
    a: &mut CodeAssembler,
    mnem: &str,
    lhs: iced_x86::code_asm::AsmRegisterXmm,
    mem: iced_x86::code_asm::AsmMemoryOperand,
) -> Result<(), AsmError> {
    match mnem {
        "addss" => a.addss(lhs, mem)?,
        "addsd" => a.addsd(lhs, mem)?,
        "subss" => a.subss(lhs, mem)?,
        "subsd" => a.subsd(lhs, mem)?,
        "mulss" => a.mulss(lhs, mem)?,
        "mulsd" => a.mulsd(lhs, mem)?,
        "divss" => a.divss(lhs, mem)?,
        "divsd" => a.divsd(lhs, mem)?,
        "xorps" => a.xorps(lhs, mem)?,
        "xorpd" => a.xorpd(lhs, mem)?,
        "comiss" => a.comiss(lhs, mem)?,
        "comisd" => a.comisd(lhs, mem)?,
        "ucomiss" => a.ucomiss(lhs, mem)?,
        "ucomisd" => a.ucomisd(lhs, mem)?,
        "cvtsd2ss" => a.cvtsd2ss(lhs, mem)?,
        "cvtss2sd" => a.cvtss2sd(lhs, mem)?,
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown SSE arith mnemonic"
            )));
        }
    };
    Ok(())
}

/// Pattern B: scalar move `mnem xmm/mem, xmm/mem` (one side must be xmm,
/// the other can be xmm or memory of matching width). Used by `movss`
/// and `movsd`. Encoders differ slightly between mem-dest and reg-dest,
/// so we branch on which side is the memory operand.
fn emit_sse_mov(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
    scalar: Scalar,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let mut a = CodeAssembler::new(64)?;

    // Memory destination: `movss [mem], xmm`.
    if let Some((size_opt, mem)) = try_parse_memory_operand(lhs_text, syms)? {
        let size = size_opt.unwrap_or(scalar.expected_size());
        let rhs = parse_xmm_register(rhs_text).ok_or_else(|| {
            AsmError::Unsupported(format!(
                "{mnem} [mem], {rhs_text:?}: rhs must be an xmm register"
            ))
        })?;
        let mem = match size {
            MemSize::Dword => dword_ptr(mem),
            MemSize::Qword => qword_ptr(mem),
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: memory width must match scalar"
                )));
            }
        };
        match mnem {
            "movss" => a.movss(mem, rhs)?,
            "movsd" => a.movsd_2(mem, rhs)?,
            other => {
                return Err(AsmError::Unsupported(format!(
                    "{other}: unknown SSE mov mnemonic"
                )));
            }
        };
        return Ok(a.assemble(base)?);
    }

    // Register destination: `movss xmm, xmm|mem`.
    let lhs = parse_xmm_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{mnem} {lhs_text:?}: lhs must be an xmm register"))
    })?;
    if let Some(rhs) = parse_xmm_register(rhs_text) {
        match mnem {
            "movss" => a.movss(lhs, rhs)?,
            "movsd" => a.movsd_2(lhs, rhs)?,
            other => {
                return Err(AsmError::Unsupported(format!(
                    "{other}: unknown SSE mov mnemonic"
                )));
            }
        };
    } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
        let size = size_opt.unwrap_or(scalar.expected_size());
        let mem = match size {
            MemSize::Dword => dword_ptr(mem),
            MemSize::Qword => qword_ptr(mem),
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: memory width must match scalar"
                )));
            }
        };
        match mnem {
            "movss" => a.movss(lhs, mem)?,
            "movsd" => a.movsd_2(lhs, mem)?,
            other => {
                return Err(AsmError::Unsupported(format!(
                    "{other}: unknown SSE mov mnemonic"
                )));
            }
        };
    } else {
        return Err(AsmError::Unsupported(format!(
            "{mnem} {lhs_text}, {rhs_text:?}: rhs must be xmm or memory"
        )));
    }
    Ok(a.assemble(base)?)
}

/// Pattern C: int→float convert (`cvtsi2ss/cvtsi2sd`) and float→int
/// convert (`cvtss2si/cvtsd2si`). Mixes a general-purpose register with
/// an xmm one.
fn emit_sse_cvt_int(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let mut a = CodeAssembler::new(64)?;
    match mnem {
        // cvtsi2ss/cvtsi2sd: dst = xmm, src = r32|r64|mem
        "cvtsi2ss" | "cvtsi2sd" => {
            let lhs = parse_xmm_register(lhs_text).ok_or_else(|| {
                AsmError::Unsupported(format!("{mnem} {lhs_text:?}: dst must be an xmm register"))
            })?;
            if let Some(rhs) = parse_register(rhs_text) {
                match (mnem, rhs) {
                    ("cvtsi2ss", TypedReg::R64(r)) => a.cvtsi2ss(lhs, r)?,
                    ("cvtsi2ss", TypedReg::R32(r)) => a.cvtsi2ss(lhs, r)?,
                    ("cvtsi2sd", TypedReg::R64(r)) => a.cvtsi2sd(lhs, r)?,
                    ("cvtsi2sd", TypedReg::R32(r)) => a.cvtsi2sd(lhs, r)?,
                    _ => {
                        return Err(AsmError::Unsupported(format!(
                            "{mnem} {lhs_text}, {rhs_text:?}: source must be a 32/64-bit register"
                        )));
                    }
                };
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                let size = size_opt.unwrap_or(MemSize::Dword);
                let mem = match size {
                    MemSize::Dword => dword_ptr(mem),
                    MemSize::Qword => qword_ptr(mem),
                    _ => {
                        return Err(AsmError::Unsupported(format!(
                            "{mnem}: source memory must be dword or qword"
                        )));
                    }
                };
                match mnem {
                    "cvtsi2ss" => a.cvtsi2ss(lhs, mem)?,
                    "cvtsi2sd" => a.cvtsi2sd(lhs, mem)?,
                    _ => unreachable!(),
                };
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: source must be a register or memory"
                )));
            }
        }
        // cvtss2si/cvtsd2si: dst = r32|r64, src = xmm|mem
        "cvtss2si" | "cvtsd2si" => {
            let lhs = parse_register(lhs_text).ok_or_else(|| {
                AsmError::Unsupported(format!(
                    "{mnem} {lhs_text:?}: dst must be a 32/64-bit register"
                ))
            })?;
            if let Some(rhs) = parse_xmm_register(rhs_text) {
                match (mnem, lhs) {
                    ("cvtss2si", TypedReg::R64(d)) => a.cvtss2si(d, rhs)?,
                    ("cvtss2si", TypedReg::R32(d)) => a.cvtss2si(d, rhs)?,
                    ("cvtsd2si", TypedReg::R64(d)) => a.cvtsd2si(d, rhs)?,
                    ("cvtsd2si", TypedReg::R32(d)) => a.cvtsd2si(d, rhs)?,
                    _ => {
                        return Err(AsmError::Unsupported(format!(
                            "{mnem} {lhs_text}, {rhs_text}: dst must be a 32/64-bit register"
                        )));
                    }
                };
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                let size = size_opt.unwrap_or(match mnem {
                    "cvtss2si" => MemSize::Dword,
                    "cvtsd2si" => MemSize::Qword,
                    _ => unreachable!(),
                });
                let mem = match size {
                    MemSize::Dword => dword_ptr(mem),
                    MemSize::Qword => qword_ptr(mem),
                    _ => {
                        return Err(AsmError::Unsupported(format!(
                            "{mnem}: source memory must be dword or qword"
                        )));
                    }
                };
                match (mnem, lhs) {
                    ("cvtss2si", TypedReg::R64(d)) => a.cvtss2si(d, mem)?,
                    ("cvtss2si", TypedReg::R32(d)) => a.cvtss2si(d, mem)?,
                    ("cvtsd2si", TypedReg::R64(d)) => a.cvtsd2si(d, mem)?,
                    ("cvtsd2si", TypedReg::R32(d)) => a.cvtsd2si(d, mem)?,
                    _ => {
                        return Err(AsmError::Unsupported(format!(
                            "{mnem} {lhs_text}, [mem]: dst must be a 32/64-bit register"
                        )));
                    }
                };
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: source must be xmm or memory"
                )));
            }
        }
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown SSE cvt mnemonic"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

/// Map a Tier-2 mnemonic to its dispatch helper. Used by [`super::compile_line`]
/// so the top-level mnemonic match doesn't have to enumerate every SSE2
/// op individually.
pub(super) fn dispatch_sse_mnemonic(
    mnem: &str,
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Option<Vec<u8>>, AsmError> {
    match mnem {
        // Pattern A — arith / compare / scalar-scalar convert
        "addss" | "subss" | "mulss" | "divss" | "xorps" | "comiss" | "ucomiss" | "cvtss2sd" => Ok(
            Some(emit_sse_arith(rest, syms, base, mnem, Scalar::Single)?),
        ),
        "addsd" | "subsd" | "mulsd" | "divsd" | "xorpd" | "comisd" | "ucomisd" | "cvtsd2ss" => Ok(
            Some(emit_sse_arith(rest, syms, base, mnem, Scalar::Double)?),
        ),
        // Pattern B — scalar move
        "movss" => Ok(Some(emit_sse_mov(rest, syms, base, mnem, Scalar::Single)?)),
        "movsd" => Ok(Some(emit_sse_mov(rest, syms, base, mnem, Scalar::Double)?)),
        // Pattern C — int↔float convert
        "cvtsi2ss" | "cvtsi2sd" | "cvtss2si" | "cvtsd2si" => {
            Ok(Some(emit_sse_cvt_int(rest, syms, base, mnem)?))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::super::compile_line;
    use super::*;

    fn empty() -> HashMap<String, u64> {
        HashMap::new()
    }

    // --- Pattern A — arith / compare ---

    #[test]
    fn addsd_reg_reg() {
        // The exact bug that surfaced in Enigma of Fear's `healthnodamage aob`
        // script. Before Tier-2 this errored with "cannot estimate length
        // for pass 1: 'addsd xmm0,xmm1'".
        let bytes = compile_line("addsd xmm0,xmm1", &empty(), 0)
            .unwrap()
            .unwrap();
        // F2 0F 58 C1 — addsd xmm0, xmm1
        assert_eq!(bytes, vec![0xF2, 0x0F, 0x58, 0xC1]);
    }

    #[test]
    fn addss_reg_reg() {
        let bytes = compile_line("addss xmm0,xmm1", &empty(), 0)
            .unwrap()
            .unwrap();
        // F3 0F 58 C1
        assert_eq!(bytes, vec![0xF3, 0x0F, 0x58, 0xC1]);
    }

    #[test]
    fn mulss_reg_mem_with_displacement() {
        // mulss xmm6, [rcx] — common float-multiplier idiom (walk-key
        // pattern). Memory size implicit from the SS scalar (dword ptr).
        let bytes = compile_line("mulss xmm6, [rcx]", &empty(), 0)
            .unwrap()
            .unwrap();
        // F3 0F 59 31 — mulss xmm6, dword ptr [rcx]
        assert_eq!(bytes, vec![0xF3, 0x0F, 0x59, 0x31]);
    }

    #[test]
    fn xorps_zeros_xmm() {
        // `xorps xmm0, xmm0` is the canonical zero idiom for xmm regs.
        let bytes = compile_line("xorps xmm0, xmm0", &empty(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x0F, 0x57, 0xC0]);
    }

    #[test]
    fn comiss_reg_reg() {
        // comiss sets EFLAGS based on ordered comparison of the low SS lane.
        let bytes = compile_line("comiss xmm0, xmm1", &empty(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x0F, 0x2F, 0xC1]);
    }

    // --- Pattern B — scalar move ---

    #[test]
    fn movss_mem_reg_with_displacement() {
        // The second arrow of the Enigma chain — `movss [r15+68], xmm5`.
        let bytes = compile_line("movss [r15+68], xmm5", &empty(), 0)
            .unwrap()
            .unwrap();
        // F3 41 0F 11 6F 68 — movss dword ptr [r15+0x68], xmm5
        // (REX.B = 0x41 because r15 is in the extended register set.)
        assert_eq!(bytes, vec![0xF3, 0x41, 0x0F, 0x11, 0x6F, 0x68]);
    }

    #[test]
    fn movss_reg_mem() {
        // Inverse direction: load.
        let bytes = compile_line("movss xmm0, [rax]", &empty(), 0)
            .unwrap()
            .unwrap();
        // F3 0F 10 00 — movss xmm0, dword ptr [rax]
        assert_eq!(bytes, vec![0xF3, 0x0F, 0x10, 0x00]);
    }

    #[test]
    fn movsd_mem_reg() {
        let bytes = compile_line("movsd [rax+8], xmm1", &empty(), 0)
            .unwrap()
            .unwrap();
        // F2 0F 11 48 08 — movsd qword ptr [rax+0x08], xmm1
        assert_eq!(bytes, vec![0xF2, 0x0F, 0x11, 0x48, 0x08]);
    }

    // --- Pattern C — int ↔ float convert ---

    #[test]
    fn cvtsd2ss_reg_reg() {
        // Third arrow of the Enigma chain — `cvtsd2ss xmm5, xmm0`.
        let bytes = compile_line("cvtsd2ss xmm5, xmm0", &empty(), 0)
            .unwrap()
            .unwrap();
        // F2 0F 5A E8 — cvtsd2ss xmm5, xmm0
        assert_eq!(bytes, vec![0xF2, 0x0F, 0x5A, 0xE8]);
    }

    #[test]
    fn cvtsi2ss_xmm_r64() {
        // Convert signed integer in rax to single-precision float in xmm0.
        let bytes = compile_line("cvtsi2ss xmm0, rax", &empty(), 0)
            .unwrap()
            .unwrap();
        // F3 48 0F 2A C0 — REX.W = 1 because the source is r64.
        assert_eq!(bytes, vec![0xF3, 0x48, 0x0F, 0x2A, 0xC0]);
    }

    #[test]
    fn cvtss2si_r32_xmm() {
        let bytes = compile_line("cvtss2si eax, xmm0", &empty(), 0)
            .unwrap()
            .unwrap();
        // F3 0F 2D C0 — cvtss2si eax, xmm0
        assert_eq!(bytes, vec![0xF3, 0x0F, 0x2D, 0xC0]);
    }

    /// End-to-end of Enigma of Fear's `healthnodamage aob` injection
    /// body — three SSE2 lines that pre-Tier-2 errored at pass 1.
    /// Re-emits each independently and asserts the byte counts match the
    /// known encodings.
    #[test]
    fn enigma_health_no_damage_lines() {
        let lines = [
            ("addsd xmm0,xmm1", 4usize), // F2 0F 58 C1
            ("cvtsd2ss xmm5,xmm0", 4),   // F2 0F 5A E8
            ("movss [r15+68],xmm5", 6),  // F3 41 0F 11 6F 68
        ];
        for (line, expected_len) in lines {
            let bytes = compile_line(line, &empty(), 0)
                .unwrap_or_else(|e| panic!("{line:?}: {e:?}"))
                .unwrap_or_else(|| panic!("{line:?}: compile_line returned None"));
            assert_eq!(
                bytes.len(),
                expected_len,
                "{line:?} encoded to {bytes:02X?} ({} bytes), expected {expected_len}",
                bytes.len()
            );
        }
    }
}
