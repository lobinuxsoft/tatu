//! Conditional move (`cmov*`) mnemonics — Tier-3.
//!
//! Audit hits across FearLess corpus: `cmovl` 7, `cmovb` 4, `cmovg` 1,
//! `cmovs` 1, `cmovle` 1 — ~14 lines total. Same shape as `mov reg, reg`
//! or `mov reg, mem`, but the move only happens when the named flag
//! condition holds. We cover the full Jcc family for parity even though
//! the audit only flags a subset — keeping iced-x86 in sync with the
//! conditional-jump set in `control_flow.rs` so the parser surface stays
//! coherent.

use std::collections::HashMap;

use iced_x86::code_asm::{CodeAssembler, dword_ptr, qword_ptr};

use super::AsmError;
use super::operands::{
    MemSize, TypedReg, parse_register, split_two_operands, try_parse_memory_operand,
};

/// Return `true` when `name` is a recognised conditional-move mnemonic.
/// Mirrors the Jcc set used by [`super::control_flow::is_conditional_jump`].
pub(super) fn is_cmov(name: &str) -> bool {
    matches!(
        name,
        "cmove"
            | "cmovne"
            | "cmovz"
            | "cmovnz"
            | "cmova"
            | "cmovae"
            | "cmovb"
            | "cmovbe"
            | "cmovc"
            | "cmovnc"
            | "cmovg"
            | "cmovge"
            | "cmovl"
            | "cmovle"
            | "cmovs"
            | "cmovns"
            | "cmovo"
            | "cmovno"
            | "cmovp"
            | "cmovnp"
            | "cmovpe"
            | "cmovpo"
            | "cmovna"
            | "cmovnae"
            | "cmovnb"
            | "cmovnbe"
            | "cmovng"
            | "cmovnge"
            | "cmovnl"
            | "cmovnle"
    )
}

pub(super) fn emit_cmov(
    mnem: &str,
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{mnem} {lhs_text:?}: lhs must be a register"))
    })?;
    let mut a = CodeAssembler::new(64)?;
    match lhs {
        TypedReg::R64(l) => {
            if let Some(TypedReg::R64(r)) = parse_register(rhs_text) {
                dispatch_cmov_rr64(&mut a, mnem, l, r)?;
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                let size = size_opt.unwrap_or(MemSize::Qword);
                if size != MemSize::Qword {
                    return Err(AsmError::Unsupported(format!(
                        "{mnem} {lhs_text}, {rhs_text:?}: r64 needs qword ptr"
                    )));
                }
                dispatch_cmov_rm64(&mut a, mnem, l, qword_ptr(mem))?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: rhs must be a register or memory"
                )));
            }
        }
        TypedReg::R32(l) => {
            if let Some(TypedReg::R32(r)) = parse_register(rhs_text) {
                dispatch_cmov_rr32(&mut a, mnem, l, r)?;
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                let size = size_opt.unwrap_or(MemSize::Dword);
                if size != MemSize::Dword {
                    return Err(AsmError::Unsupported(format!(
                        "{mnem} {lhs_text}, {rhs_text:?}: r32 needs dword ptr"
                    )));
                }
                dispatch_cmov_rm32(&mut a, mnem, l, dword_ptr(mem))?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "{mnem} {lhs_text}, {rhs_text:?}: rhs must be a register or memory"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(format!(
                "{mnem} {lhs_text}: 8/16-bit cmov not supported"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

// Hand-rolled dispatch tables — Rust can't generate the trait-bound match
// inside a single function across both R64/R32 widths in a compact form,
// so we duplicate per width. iced-x86's overload set is huge but the cost
// here is one match arm per mnemonic per width = ~30 lines × 4 = ~120 lines.

fn dispatch_cmov_rr64(
    a: &mut CodeAssembler,
    mnem: &str,
    l: iced_x86::code_asm::AsmRegister64,
    r: iced_x86::code_asm::AsmRegister64,
) -> Result<(), AsmError> {
    match mnem {
        "cmove" | "cmovz" => a.cmove(l, r)?,
        "cmovne" | "cmovnz" => a.cmovne(l, r)?,
        "cmova" | "cmovnbe" => a.cmova(l, r)?,
        "cmovae" | "cmovnb" | "cmovnc" => a.cmovae(l, r)?,
        "cmovb" | "cmovnae" | "cmovc" => a.cmovb(l, r)?,
        "cmovbe" | "cmovna" => a.cmovbe(l, r)?,
        "cmovg" | "cmovnle" => a.cmovg(l, r)?,
        "cmovge" | "cmovnl" => a.cmovge(l, r)?,
        "cmovl" | "cmovnge" => a.cmovl(l, r)?,
        "cmovle" | "cmovng" => a.cmovle(l, r)?,
        "cmovs" => a.cmovs(l, r)?,
        "cmovns" => a.cmovns(l, r)?,
        "cmovo" => a.cmovo(l, r)?,
        "cmovno" => a.cmovno(l, r)?,
        "cmovp" | "cmovpe" => a.cmovp(l, r)?,
        "cmovnp" | "cmovpo" => a.cmovnp(l, r)?,
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown cmov mnemonic"
            )));
        }
    };
    Ok(())
}

fn dispatch_cmov_rm64(
    a: &mut CodeAssembler,
    mnem: &str,
    l: iced_x86::code_asm::AsmRegister64,
    mem: iced_x86::code_asm::AsmMemoryOperand,
) -> Result<(), AsmError> {
    match mnem {
        "cmove" | "cmovz" => a.cmove(l, mem)?,
        "cmovne" | "cmovnz" => a.cmovne(l, mem)?,
        "cmova" | "cmovnbe" => a.cmova(l, mem)?,
        "cmovae" | "cmovnb" | "cmovnc" => a.cmovae(l, mem)?,
        "cmovb" | "cmovnae" | "cmovc" => a.cmovb(l, mem)?,
        "cmovbe" | "cmovna" => a.cmovbe(l, mem)?,
        "cmovg" | "cmovnle" => a.cmovg(l, mem)?,
        "cmovge" | "cmovnl" => a.cmovge(l, mem)?,
        "cmovl" | "cmovnge" => a.cmovl(l, mem)?,
        "cmovle" | "cmovng" => a.cmovle(l, mem)?,
        "cmovs" => a.cmovs(l, mem)?,
        "cmovns" => a.cmovns(l, mem)?,
        "cmovo" => a.cmovo(l, mem)?,
        "cmovno" => a.cmovno(l, mem)?,
        "cmovp" | "cmovpe" => a.cmovp(l, mem)?,
        "cmovnp" | "cmovpo" => a.cmovnp(l, mem)?,
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown cmov mnemonic"
            )));
        }
    };
    Ok(())
}

fn dispatch_cmov_rr32(
    a: &mut CodeAssembler,
    mnem: &str,
    l: iced_x86::code_asm::AsmRegister32,
    r: iced_x86::code_asm::AsmRegister32,
) -> Result<(), AsmError> {
    match mnem {
        "cmove" | "cmovz" => a.cmove(l, r)?,
        "cmovne" | "cmovnz" => a.cmovne(l, r)?,
        "cmova" | "cmovnbe" => a.cmova(l, r)?,
        "cmovae" | "cmovnb" | "cmovnc" => a.cmovae(l, r)?,
        "cmovb" | "cmovnae" | "cmovc" => a.cmovb(l, r)?,
        "cmovbe" | "cmovna" => a.cmovbe(l, r)?,
        "cmovg" | "cmovnle" => a.cmovg(l, r)?,
        "cmovge" | "cmovnl" => a.cmovge(l, r)?,
        "cmovl" | "cmovnge" => a.cmovl(l, r)?,
        "cmovle" | "cmovng" => a.cmovle(l, r)?,
        "cmovs" => a.cmovs(l, r)?,
        "cmovns" => a.cmovns(l, r)?,
        "cmovo" => a.cmovo(l, r)?,
        "cmovno" => a.cmovno(l, r)?,
        "cmovp" | "cmovpe" => a.cmovp(l, r)?,
        "cmovnp" | "cmovpo" => a.cmovnp(l, r)?,
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown cmov mnemonic"
            )));
        }
    };
    Ok(())
}

fn dispatch_cmov_rm32(
    a: &mut CodeAssembler,
    mnem: &str,
    l: iced_x86::code_asm::AsmRegister32,
    mem: iced_x86::code_asm::AsmMemoryOperand,
) -> Result<(), AsmError> {
    match mnem {
        "cmove" | "cmovz" => a.cmove(l, mem)?,
        "cmovne" | "cmovnz" => a.cmovne(l, mem)?,
        "cmova" | "cmovnbe" => a.cmova(l, mem)?,
        "cmovae" | "cmovnb" | "cmovnc" => a.cmovae(l, mem)?,
        "cmovb" | "cmovnae" | "cmovc" => a.cmovb(l, mem)?,
        "cmovbe" | "cmovna" => a.cmovbe(l, mem)?,
        "cmovg" | "cmovnle" => a.cmovg(l, mem)?,
        "cmovge" | "cmovnl" => a.cmovge(l, mem)?,
        "cmovl" | "cmovnge" => a.cmovl(l, mem)?,
        "cmovle" | "cmovng" => a.cmovle(l, mem)?,
        "cmovs" => a.cmovs(l, mem)?,
        "cmovns" => a.cmovns(l, mem)?,
        "cmovo" => a.cmovo(l, mem)?,
        "cmovno" => a.cmovno(l, mem)?,
        "cmovp" | "cmovpe" => a.cmovp(l, mem)?,
        "cmovnp" | "cmovpo" => a.cmovnp(l, mem)?,
        other => {
            return Err(AsmError::Unsupported(format!(
                "{other}: unknown cmov mnemonic"
            )));
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::compile_line;
    use super::*;

    fn empty() -> HashMap<String, u64> {
        HashMap::new()
    }

    #[test]
    fn cmovl_r64_r64() {
        let bytes = compile_line("cmovl rax, rbx", &empty(), 0)
            .unwrap()
            .unwrap();
        // 48 0F 4C C3 — cmovl rax, rbx
        assert_eq!(bytes, vec![0x48, 0x0F, 0x4C, 0xC3]);
    }

    #[test]
    fn cmove_r32_r32() {
        let bytes = compile_line("cmove eax, ebx", &empty(), 0)
            .unwrap()
            .unwrap();
        // 0F 44 C3 — cmove eax, ebx
        assert_eq!(bytes, vec![0x0F, 0x44, 0xC3]);
    }

    #[test]
    fn cmovb_r64_mem() {
        let bytes = compile_line("cmovb rax, qword ptr [rbx]", &empty(), 0)
            .unwrap()
            .unwrap();
        // 48 0F 42 03 — cmovb rax, qword ptr [rbx]
        assert_eq!(bytes, vec![0x48, 0x0F, 0x42, 0x03]);
    }

    #[test]
    fn cmovne_alias_for_cmovnz() {
        // Both names should encode identically — Intel says they are
        // aliases (ZF=0 case).
        let cmovne = compile_line("cmovne rax, rbx", &empty(), 0)
            .unwrap()
            .unwrap();
        let cmovnz = compile_line("cmovnz rax, rbx", &empty(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(cmovne, cmovnz);
    }
}
