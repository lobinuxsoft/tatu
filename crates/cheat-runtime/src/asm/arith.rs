//! Arithmetic / comparison mnemonics: `cmp`, `add`, `sub`, `xor`.

use std::collections::HashMap;

use iced_x86::code_asm::{CodeAssembler, dword_ptr, qword_ptr};

use super::AsmError;
use super::operands::{
    MemSize, TypedReg, apply_size, parse_immediate, parse_register, split_two_operands,
    try_parse_memory_operand,
};

pub(super) fn emit_cmp(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let mut a = CodeAssembler::new(64)?;

    // `cmp <mem>, <reg|imm>` — memory left.
    if let Some((size, mem)) = try_parse_memory_operand(lhs_text, syms)? {
        let sized = apply_size(size, mem);
        if let Some(reg) = parse_register(rhs_text) {
            match (size, reg) {
                (MemSize::Qword, TypedReg::R64(r)) => a.cmp(sized, r)?,
                (MemSize::Dword, TypedReg::R32(r)) => a.cmp(sized, r)?,
                (MemSize::Word, TypedReg::R16(r)) => a.cmp(sized, r)?,
                (MemSize::Byte, TypedReg::R8(r)) => a.cmp(sized, r)?,
                _ => {
                    return Err(AsmError::Unsupported(format!(
                        "cmp {lhs_text}, {rhs_text:?}: register width mismatches memory size"
                    )));
                }
            }
        } else if let Some(imm) = parse_immediate(rhs_text, syms) {
            match size {
                MemSize::Byte => a.cmp(sized, (imm as i32) & 0xff)?,
                MemSize::Word => a.cmp(sized, (imm as i32) & 0xffff)?,
                MemSize::Dword => a.cmp(sized, imm as i32)?,
                MemSize::Qword => a.cmp(sized, imm as i32)?,
            }
        } else {
            return Err(AsmError::Unsupported(format!(
                "cmp {lhs_text}, {rhs_text:?}: rhs must be a register or immediate"
            )));
        }
        return Ok(a.assemble(base)?);
    }

    // `cmp <reg>, <reg|imm|mem>` — register left.
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("cmp lhs {lhs_text:?}: not a register or memory"))
    })?;
    match lhs {
        TypedReg::R64(l) => {
            if let Some(TypedReg::R64(r)) = parse_register(rhs_text) {
                a.cmp(l, r)?;
            } else if let Some((size, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                if size != MemSize::Qword {
                    return Err(AsmError::Unsupported(format!(
                        "cmp {lhs_text}, {rhs_text:?}: r64 needs qword ptr"
                    )));
                }
                a.cmp(l, qword_ptr(mem))?;
            } else if let Some(imm) = parse_immediate(rhs_text, syms) {
                a.cmp(l, imm as i32)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "cmp {lhs_text}, {rhs_text:?}: rhs must be a register, memory, or immediate"
                )));
            }
        }
        TypedReg::R32(l) => {
            if let Some(TypedReg::R32(r)) = parse_register(rhs_text) {
                a.cmp(l, r)?;
            } else if let Some((size, mem)) = try_parse_memory_operand(rhs_text, syms)? {
                if size != MemSize::Dword {
                    return Err(AsmError::Unsupported(format!(
                        "cmp {lhs_text}, {rhs_text:?}: r32 needs dword ptr"
                    )));
                }
                a.cmp(l, dword_ptr(mem))?;
            } else if let Some(imm) = parse_immediate(rhs_text, syms) {
                a.cmp(l, imm as i32)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "cmp {lhs_text}, {rhs_text:?}: rhs must be a register, memory, or immediate"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(format!(
                "cmp {lhs_text}: 8/16-bit lhs register not supported yet"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Arith {
    Add,
    Sub,
    Xor,
}

pub(super) fn emit_arith(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    op: Arith,
) -> Result<Vec<u8>, AsmError> {
    let (lhs_text, rhs_text) = split_two_operands(rest)?;
    let lhs = parse_register(lhs_text).ok_or_else(|| {
        AsmError::Unsupported(format!("{op:?} lhs {lhs_text:?}: needs to be a register"))
    })?;
    let mut a = CodeAssembler::new(64)?;
    match lhs {
        TypedReg::R64(l) => match (op, parse_register(rhs_text)) {
            (Arith::Add, Some(TypedReg::R64(r))) => a.add(l, r)?,
            (Arith::Sub, Some(TypedReg::R64(r))) => a.sub(l, r)?,
            (Arith::Xor, Some(TypedReg::R64(r))) => a.xor(l, r)?,
            (op, None) => {
                let imm = parse_immediate(rhs_text, syms).ok_or_else(|| {
                    AsmError::Unsupported(format!(
                        "{op:?} {lhs_text}, {rhs_text:?}: rhs must be a register or immediate"
                    ))
                })?;
                match op {
                    Arith::Add => a.add(l, imm as i32)?,
                    Arith::Sub => a.sub(l, imm as i32)?,
                    Arith::Xor => a.xor(l, imm as i32)?,
                }
            }
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "{op:?} {lhs_text}, {rhs_text:?}: width mismatch"
                )));
            }
        },
        TypedReg::R32(l) => match (op, parse_register(rhs_text)) {
            (Arith::Add, Some(TypedReg::R32(r))) => a.add(l, r)?,
            (Arith::Sub, Some(TypedReg::R32(r))) => a.sub(l, r)?,
            (Arith::Xor, Some(TypedReg::R32(r))) => a.xor(l, r)?,
            (op, None) => {
                let imm = parse_immediate(rhs_text, syms).ok_or_else(|| {
                    AsmError::Unsupported(format!(
                        "{op:?} {lhs_text}, {rhs_text:?}: rhs must be a register or immediate"
                    ))
                })?;
                match op {
                    Arith::Add => a.add(l, imm as i32)?,
                    Arith::Sub => a.sub(l, imm as i32)?,
                    Arith::Xor => a.xor(l, imm as i32)?,
                }
            }
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "{op:?} {lhs_text}, {rhs_text:?}: width mismatch"
                )));
            }
        },
        _ => {
            return Err(AsmError::Unsupported(format!(
                "{op:?} {lhs_text}: 8/16-bit lhs not supported yet"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

#[cfg(test)]
mod tests {
    use super::super::compile_line;
    use super::*;

    fn symtab(entries: &[(&str, u64)]) -> HashMap<String, u64> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), *v))
            .collect()
    }

    #[test]
    fn cmp_byte_ptr_symbol_with_immediate() {
        // cmp byte ptr [unlHarvestFlag], 1
        // unlHarvestFlag is a symbol → absolute address.
        let syms = symtab(&[("unlHarvestFlag", 0x12345678)]);
        let bytes = compile_line("cmp byte ptr [unlHarvestFlag], 1", &syms, 0)
            .unwrap()
            .unwrap();
        // Should encode as cmp byte [disp32], imm8. The disp32 contains
        // 0x12345678 in little-endian.
        let disp = u32::from_le_bytes(bytes[bytes.len() - 5..bytes.len() - 1].try_into().unwrap());
        assert_eq!(disp, 0x12345678);
        // Last byte is the immediate.
        assert_eq!(*bytes.last().unwrap(), 0x01);
    }

    #[test]
    fn cmp_reg_imm() {
        // cmp eax, 1 — iced may pick either the `cmp r/m32, imm8 sext`
        // (3 bytes: 83 F8 01) or the eax-special `cmp eax, imm32`
        // (5 bytes: 3D 01 00 00 00). Accept either, both decode to the
        // same comparison.
        let bytes = compile_line("cmp eax, 1", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert!(
            matches!(bytes.as_slice(), [0x83, 0xF8, 0x01] | [0x3D, 0x01, 0, 0, 0]),
            "unexpected cmp encoding: {bytes:02X?}"
        );
    }

    #[test]
    fn cmp_reg_reg() {
        let bytes = compile_line("cmp rax, rbx", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        // 48 39 D8 — cmp r/m64, r64.
        assert_eq!(bytes, vec![0x48, 0x39, 0xD8]);
    }

    #[test]
    fn add_reg64_imm() {
        // add rax, 1 — iced picks the imm32 form for the rax special-case
        // (48 05 01 00 00 00, 6 bytes) over the imm8 sext form
        // (48 83 C0 01, 4 bytes). Both decode to the same operation.
        let bytes = compile_line("add rax, 1", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                bytes.as_slice(),
                [0x48, 0x83, 0xC0, 0x01] | [0x48, 0x05, 0x01, 0, 0, 0]
            ),
            "unexpected add encoding: {bytes:02X?}"
        );
    }

    #[test]
    fn sub_reg64_reg64() {
        // sub rax, rbx → 48 29 D8
        let bytes = compile_line("sub rax, rbx", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x48, 0x29, 0xD8]);
    }

    #[test]
    fn xor_zeros_register() {
        // xor eax, eax → 31 C0 (the canonical 2-byte zeroing idiom).
        let bytes = compile_line("xor eax, eax", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x31, 0xC0]);
    }
}
