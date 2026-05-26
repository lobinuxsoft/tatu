//! Tier-3 additions across the misc opcode set the original modules
//! didn't cover: bitwise `and`/`or`, shifts (`shl`/`shr`/`sar`),
//! single-operand arithmetic (`inc`/`dec`), 0-operand sign-extends
//! (`cdqe`/`cqo`), flag stack (`pushf`/`popf`/`pushfq`/`popfq`), and the
//! zero/sign-extending moves (`movzx`/`movsxd`).
//!
//! Each of these has a unique operand shape that didn't fit the existing
//! `cmp`/`add`/`mov` templates, but together they cover the long tail
//! the FearLess audit flagged as Tier-3 (~7% of the corpus in aggregate).

use std::collections::HashMap;

use iced_x86::code_asm::{CodeAssembler, byte_ptr, dword_ptr, registers as r, word_ptr};

use super::AsmError;
use super::operands::{
    MemSize, TypedReg, apply_size, parse_immediate, parse_register, split_two_operands,
    try_parse_memory_operand,
};

/// Logical AND / OR — register dst (`reg, reg|imm`) only. Memory dst
/// already covered by `try_parse_memory_operand` callers elsewhere, but
/// these mnemonics are dominated by reg/reg in the audit so we keep the
/// initial cut minimal.
pub(super) fn emit_logical(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{mnem} {lhs_text:?}: lhs must be a register"))
    })?;
    let mut a = CodeAssembler::new(64)?;
    match lhs {
        TypedReg::R64(l) => {
            if let Some(TypedReg::R64(r)) = parse_register(rhs_text) {
                match mnem {
                    "and" => a.and(l, r)?,
                    "or" => a.or(l, r)?,
                    other => return Err(AsmError::Unsupported(format!("{other}: bad logical"))),
                };
            } else if let Some(imm) = parse_immediate(rhs_text, syms) {
                match mnem {
                    "and" => a.and(l, imm as i32)?,
                    "or" => a.or(l, imm as i32)?,
                    other => return Err(AsmError::Unsupported(format!("{other}: bad logical"))),
                };
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: rhs must be register or immediate"
                )));
            }
        }
        TypedReg::R32(l) => {
            if let Some(TypedReg::R32(r)) = parse_register(rhs_text) {
                match mnem {
                    "and" => a.and(l, r)?,
                    "or" => a.or(l, r)?,
                    other => return Err(AsmError::Unsupported(format!("{other}: bad logical"))),
                };
            } else if let Some(imm) = parse_immediate(rhs_text, syms) {
                match mnem {
                    "and" => a.and(l, imm as i32)?,
                    "or" => a.or(l, imm as i32)?,
                    other => return Err(AsmError::Unsupported(format!("{other}: bad logical"))),
                };
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: rhs must be register or immediate"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(format!(
                "{mnem} {lhs_text}: 8/16-bit not supported yet"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

/// Shifts — `mnem reg, imm8` or `mnem reg, cl`. We don't support memory
/// destinations or the implicit single-bit form (`shl reg`) until a
/// table needs them.
pub(super) fn emit_shift(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{mnem} {lhs_text:?}: lhs must be a register"))
    })?;
    let rhs_is_cl = rhs_text.eq_ignore_ascii_case("cl");
    let imm = if rhs_is_cl {
        None
    } else {
        Some(parse_immediate(rhs_text, syms).ok_or_else(|| {
            AsmError::Unsupported(format!(
                "{mnem} {lhs_text}, {rhs_text:?}: count must be imm8 or `cl`"
            ))
        })?)
    };
    let mut a = CodeAssembler::new(64)?;
    match lhs {
        TypedReg::R64(l) => match (mnem, rhs_is_cl, imm) {
            ("shl" | "sal", true, _) => a.shl(l, r::cl)?,
            ("shr", true, _) => a.shr(l, r::cl)?,
            ("sar", true, _) => a.sar(l, r::cl)?,
            ("shl" | "sal", false, Some(i)) => a.shl(l, i as u32)?,
            ("shr", false, Some(i)) => a.shr(l, i as u32)?,
            ("sar", false, Some(i)) => a.sar(l, i as u32)?,
            _ => return Err(AsmError::Unsupported(format!("{mnem}: bad operands"))),
        },
        TypedReg::R32(l) => match (mnem, rhs_is_cl, imm) {
            ("shl" | "sal", true, _) => a.shl(l, r::cl)?,
            ("shr", true, _) => a.shr(l, r::cl)?,
            ("sar", true, _) => a.sar(l, r::cl)?,
            ("shl" | "sal", false, Some(i)) => a.shl(l, i as u32)?,
            ("shr", false, Some(i)) => a.shr(l, i as u32)?,
            ("sar", false, Some(i)) => a.sar(l, i as u32)?,
            _ => return Err(AsmError::Unsupported(format!("{mnem}: bad operands"))),
        },
        _ => {
            return Err(AsmError::Unsupported(format!(
                "{mnem} {lhs_text}: 8/16-bit not supported"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

/// `inc/dec/neg/not <reg|mem>` — single-operand integer ops.
/// All four follow the same shape: `mnem reg` or `mnem dword/qword ptr [mem]`.
pub(super) fn emit_inc_dec(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
) -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    if let Some(reg) = parse_register(rest.trim()) {
        match (mnem, reg) {
            ("inc", TypedReg::R64(r)) => a.inc(r)?,
            ("inc", TypedReg::R32(r)) => a.inc(r)?,
            ("dec", TypedReg::R64(r)) => a.dec(r)?,
            ("dec", TypedReg::R32(r)) => a.dec(r)?,
            ("neg", TypedReg::R64(r)) => a.neg(r)?,
            ("neg", TypedReg::R32(r)) => a.neg(r)?,
            ("not", TypedReg::R64(r)) => a.not(r)?,
            ("not", TypedReg::R32(r)) => a.not(r)?,
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "{mnem}: 8/16-bit register not supported yet"
                )));
            }
        };
    } else if let Some((size_opt, mem)) = try_parse_memory_operand(rest, syms)? {
        let size = size_opt.ok_or_else(|| {
            AsmError::Unsupported(format!(
                "{mnem} {rest:?}: memory operand needs an explicit size prefix"
            ))
        })?;
        let sized = apply_size(size, mem);
        match mnem {
            "inc" => a.inc(sized)?,
            "dec" => a.dec(sized)?,
            "neg" => a.neg(sized)?,
            "not" => a.not(sized)?,
            other => return Err(AsmError::Unsupported(format!("{other}: unknown"))),
        };
        // size is read above to ensure the prefix exists, but iced sizes
        // off the `sized` operand directly so we don't switch on it here.
        let _ = size;
    } else {
        return Err(AsmError::Unsupported(format!(
            "{mnem} {rest:?}: operand must be a register or memory"
        )));
    }
    Ok(a.assemble(base)?)
}

/// 0-operand instructions: `cdqe`, `cqo`, `cwde`, `cwd`, `cdq`,
/// `pushf`/`popf`, `pushfq`/`popfq`, `leave`, `nop` (already in mod.rs).
pub(super) fn emit_zero_arg(mnem: &str, base: u64) -> Result<Option<Vec<u8>>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    match mnem {
        "cdqe" => a.cdqe()?,
        "cqo" => a.cqo()?,
        "cwde" => a.cwde()?,
        "cwd" => a.cwd()?,
        "cdq" => a.cdq()?,
        "pushfq" | "pushf" => a.pushfq()?,
        "popfq" | "popf" => a.popfq()?,
        "leave" => a.leave()?,
        "stc" => a.stc()?,
        "clc" => a.clc()?,
        "cld" => a.cld()?,
        "std" => a.std()?,
        "cmc" => a.cmc()?,
        _ => return Ok(None),
    };
    Ok(Some(a.assemble(base)?))
}

/// `movzx <r32|r64>, <r8|r16|byte ptr [mem]|word ptr [mem]>` and
/// `movsxd <r64>, <r32|dword ptr [mem]>`. CE-AA scripts use these in
/// pointer / index unpacking idioms.
pub(super) fn emit_extending_mov(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{mnem} {lhs_text:?}: lhs must be a register"))
    })?;
    let mut a = CodeAssembler::new(64)?;
    match (mnem, lhs) {
        ("movsxd", TypedReg::R64(l)) => {
            if let Some(TypedReg::R32(r)) = parse_register(rhs_text) {
                a.movsxd(l, r)?;
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                let size = size_opt.unwrap_or(MemSize::Dword);
                if size != MemSize::Dword {
                    return Err(AsmError::Unsupported(format!(
                        "{mnem}: source memory must be dword"
                    )));
                }
                a.movsxd(l, dword_ptr(mem))?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: rhs must be r32 or dword memory"
                )));
            }
        }
        ("movzx", lhs @ (TypedReg::R64(_) | TypedReg::R32(_))) => {
            // movzx r64/r32, r8/r16 or memory of those widths.
            // For simplicity require explicit `byte/word ptr` for memory.
            if let Some(rhs_reg) = parse_register(rhs_text) {
                emit_movzx_from_reg(&mut a, mnem, lhs_text, rhs_text, lhs, rhs_reg)?;
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                let size = size_opt.ok_or_else(|| {
                    AsmError::Unsupported(format!(
                        "{mnem} {lhs_text}, {rhs_text:?}: memory needs an explicit `byte ptr` or `word ptr` prefix"
                    ))
                })?;
                emit_movzx_from_mem(&mut a, lhs, mem, size, lhs_text)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: rhs must be a smaller register or sized memory"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(format!(
                "{mnem} {lhs_text}: unsupported width pair"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

fn emit_movzx_from_reg(
    a: &mut CodeAssembler,
    mnem: &str,
    lhs_text: &str,
    rhs_text: &str,
    lhs: TypedReg,
    rhs: TypedReg,
) -> Result<(), AsmError> {
    match (lhs, rhs) {
        (TypedReg::R64(l), TypedReg::R8(r)) => a.movzx(l, r)?,
        (TypedReg::R64(l), TypedReg::R16(r)) => a.movzx(l, r)?,
        (TypedReg::R32(l), TypedReg::R8(r)) => a.movzx(l, r)?,
        (TypedReg::R32(l), TypedReg::R16(r)) => a.movzx(l, r)?,
        _ => {
            return Err(AsmError::Unsupported(format!(
                "{mnem} {lhs_text}, {rhs_text}: width pair not supported"
            )));
        }
    };
    Ok(())
}

fn emit_movzx_from_mem(
    a: &mut CodeAssembler,
    lhs: TypedReg,
    mem: iced_x86::code_asm::AsmMemoryOperand,
    size: MemSize,
    lhs_text: &str,
) -> Result<(), AsmError> {
    match (lhs, size) {
        (TypedReg::R64(l), MemSize::Byte) => a.movzx(l, byte_ptr(mem))?,
        (TypedReg::R64(l), MemSize::Word) => a.movzx(l, word_ptr(mem))?,
        (TypedReg::R32(l), MemSize::Byte) => a.movzx(l, byte_ptr(mem))?,
        (TypedReg::R32(l), MemSize::Word) => a.movzx(l, word_ptr(mem))?,
        _ => {
            return Err(AsmError::Unsupported(format!(
                "movzx: width pair not supported (dst {lhs_text}, src size {size:?})"
            )));
        }
    };
    Ok(())
}

/// `imul reg, reg` / `imul reg, reg, imm` — 2 and 3-arg signed multiply.
/// 1-arg form (`imul reg`) not implemented (rare in audit).
pub(super) fn emit_imul(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    // 3-arg case has TWO commas; sniff that first.
    let comma_count = rest.matches(',').count();
    if comma_count == 2 {
        let parts: Vec<&str> = rest.splitn(3, ',').map(str::trim).collect();
        let dst = parse_register(parts[0]).ok_or_else(|| {
            AsmError::Unsupported(format!("imul {parts:?}: dst must be a register"))
        })?;
        let src = parse_register(parts[1]).ok_or_else(|| {
            AsmError::Unsupported("imul: middle operand must be a register".to_string())
        })?;
        let imm = parse_immediate(parts[2], syms).ok_or_else(|| {
            AsmError::Unsupported("imul: 3rd operand must be an immediate".to_string())
        })?;
        match (dst, src) {
            (TypedReg::R64(d), TypedReg::R64(s)) => a.imul_3(d, s, imm as i32)?,
            (TypedReg::R32(d), TypedReg::R32(s)) => a.imul_3(d, s, imm as i32)?,
            _ => {
                return Err(AsmError::Unsupported(
                    "imul: 3-arg requires matching 32/64-bit registers".to_string(),
                ));
            }
        };
        return Ok(a.assemble(base)?);
    }

    // 2-arg case.
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("imul {lhs_text:?}: lhs must be a register"))
    })?;
    match lhs {
        TypedReg::R64(l) => {
            if let Some(TypedReg::R64(r)) = parse_register(rhs_text) {
                a.imul_2(l, r)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "imul {lhs_text}, {rhs_text:?}: rhs must be a matching register"
                )));
            }
        }
        TypedReg::R32(l) => {
            if let Some(TypedReg::R32(r)) = parse_register(rhs_text) {
                a.imul_2(l, r)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "imul {lhs_text}, {rhs_text:?}: rhs must be a matching register"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(
                "imul: 8/16-bit not supported".to_string(),
            ));
        }
    }
    Ok(a.assemble(base)?)
}

pub(super) fn dispatch_misc_mnemonic(
    mnem: &str,
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Option<Vec<u8>>, AsmError> {
    // 0-arg first (only when no operand).
    if rest.trim().is_empty()
        && let Some(bytes) = emit_zero_arg(mnem, base)?
    {
        return Ok(Some(bytes));
    }
    match mnem {
        "and" | "or" => Ok(Some(emit_logical(rest, syms, base, mnem)?)),
        "shl" | "shr" | "sar" | "sal" => Ok(Some(emit_shift(rest, syms, base, mnem)?)),
        "inc" | "dec" | "neg" | "not" => Ok(Some(emit_inc_dec(rest, syms, base, mnem)?)),
        "movzx" | "movsxd" => Ok(Some(emit_extending_mov(rest, syms, base, mnem)?)),
        "imul" => Ok(Some(emit_imul(rest, syms, base)?)),
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

    #[test]
    fn and_reg_imm() {
        let bytes = compile_line("and rax, 0x7F", &empty(), 0).unwrap().unwrap();
        // iced picks either the 4-byte imm8-sign-extended form
        // (48 83 E0 7F) or the rax-special 6-byte imm32 (48 25 7F 00 00 00).
        // Both compute the same AND.
        assert!(
            matches!(
                bytes.as_slice(),
                [0x48, 0x83, 0xE0, 0x7F] | [0x48, 0x25, 0x7F, 0, 0, 0]
            ),
            "unexpected `and rax,0x7F` encoding: {bytes:02X?}"
        );
    }

    #[test]
    fn or_reg_reg() {
        let bytes = compile_line("or rax, rbx", &empty(), 0).unwrap().unwrap();
        // 48 09 D8
        assert_eq!(bytes, vec![0x48, 0x09, 0xD8]);
    }

    #[test]
    fn shl_reg_imm() {
        let bytes = compile_line("shl rax, 4", &empty(), 0).unwrap().unwrap();
        // 48 C1 E0 04
        assert_eq!(bytes, vec![0x48, 0xC1, 0xE0, 0x04]);
    }

    #[test]
    fn shr_reg_cl() {
        let bytes = compile_line("shr rax, cl", &empty(), 0).unwrap().unwrap();
        // 48 D3 E8
        assert_eq!(bytes, vec![0x48, 0xD3, 0xE8]);
    }

    #[test]
    fn inc_reg64() {
        let bytes = compile_line("inc rax", &empty(), 0).unwrap().unwrap();
        // 48 FF C0
        assert_eq!(bytes, vec![0x48, 0xFF, 0xC0]);
    }

    #[test]
    fn dec_reg32() {
        let bytes = compile_line("dec eax", &empty(), 0).unwrap().unwrap();
        // FF C8
        assert_eq!(bytes, vec![0xFF, 0xC8]);
    }

    #[test]
    fn cdqe_zero_arg() {
        let bytes = compile_line("cdqe", &empty(), 0).unwrap().unwrap();
        // 48 98
        assert_eq!(bytes, vec![0x48, 0x98]);
    }

    #[test]
    fn popf_zero_arg() {
        // CE-AA `popf` in 64-bit mode is `popfq` — iced emits the
        // 64-bit form (just `9D`, the REX.W is implicit in long mode).
        let bytes = compile_line("popf", &empty(), 0).unwrap().unwrap();
        assert_eq!(bytes, vec![0x9D]);
    }

    #[test]
    fn movsxd_r64_r32() {
        let bytes = compile_line("movsxd rax, ebx", &empty(), 0)
            .unwrap()
            .unwrap();
        // 48 63 C3
        assert_eq!(bytes, vec![0x48, 0x63, 0xC3]);
    }

    #[test]
    fn movzx_r32_byte_mem() {
        let bytes = compile_line("movzx eax, byte ptr [rbx]", &empty(), 0)
            .unwrap()
            .unwrap();
        // 0F B6 03
        assert_eq!(bytes, vec![0x0F, 0xB6, 0x03]);
    }

    #[test]
    fn imul_two_arg() {
        let bytes = compile_line("imul rax, rbx", &empty(), 0).unwrap().unwrap();
        // 48 0F AF C3
        assert_eq!(bytes, vec![0x48, 0x0F, 0xAF, 0xC3]);
    }

    #[test]
    fn imul_three_arg_with_imm() {
        let bytes = compile_line("imul rax, rbx, 4", &empty(), 0)
            .unwrap()
            .unwrap();
        // 48 6B C3 04 — imul rax, rbx, 4 (imm8 form)
        assert_eq!(bytes, vec![0x48, 0x6B, 0xC3, 0x04]);
    }
}
