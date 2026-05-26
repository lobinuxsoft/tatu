//! Data-movement mnemonics: `push`, `pop`, `mov`, `lea`.

use std::collections::HashMap;

use iced_x86::code_asm::{AsmMemoryOperand, CodeAssembler, dword_ptr, qword_ptr};

use super::AsmError;
use super::operands::{
    MemSize, TypedReg, apply_size, parse_addr_inside_brackets, parse_immediate, parse_register,
    promote_to_r64, resolve_numeric_or_symbol, split_two_operands, try_parse_memory_operand,
};

/// CE-AA infers the memory width of an unprefixed `[mem]` operand from
/// the OTHER operand's register width. Returns `None` if the partner isn't
/// a sized register (e.g. an immediate-into-mem move).
fn infer_size_from_register(partner: &str) -> Option<MemSize> {
    parse_register(partner).map(|r| match r {
        TypedReg::R64(_) => MemSize::Qword,
        TypedReg::R32(_) => MemSize::Dword,
        TypedReg::R16(_) => MemSize::Word,
        TypedReg::R8(_) => MemSize::Byte,
    })
}

pub(super) fn emit_push(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    let op = rest.trim();
    if let Some(reg) = parse_register(op) {
        match reg {
            // CE convention: `push ebx` in long mode encodes as `push rbx`.
            // iced-x86 refuses 32-bit push in 64-bit mode, so promote first.
            TypedReg::R64(r) => a.push(r)?,
            TypedReg::R32(_) => a.push(promote_to_r64(op).ok_or_else(|| {
                AsmError::Unsupported(format!("cannot promote {op:?} to a 64-bit register"))
            })?)?,
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "push {op:?}: unsupported width"
                )));
            }
        }
    } else if let Some(target) = resolve_numeric_or_symbol(op, syms) {
        a.push(target as i32)?;
    } else {
        return Err(AsmError::BadOperand(op.into()));
    }
    Ok(a.assemble(base)?)
}

pub(super) fn emit_pop(rest: &str, base: u64) -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    let op = rest.trim();
    let reg = parse_register(op).ok_or_else(|| AsmError::BadOperand(op.into()))?;
    match reg {
        TypedReg::R64(r) => a.pop(r)?,
        TypedReg::R32(_) => a.pop(promote_to_r64(op).ok_or_else(|| {
            AsmError::Unsupported(format!("cannot promote {op:?} to a 64-bit register"))
        })?)?,
        _ => {
            return Err(AsmError::Unsupported(format!(
                "pop {op:?}: unsupported width"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

pub(super) fn emit_mov(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let (dst_text, src_text) = split_two_operands(rest)?;
    let mut a = CodeAssembler::new(64)?;

    // `mov <mem>, <...>` — memory destination.
    if let Some((size_opt, mem)) = try_parse_memory_operand(dst_text, syms)? {
        // CE-AA infers an unprefixed `[mem]` size from the source register.
        let size = match size_opt {
            Some(s) => s,
            None => infer_size_from_register(src_text).ok_or_else(|| {
                AsmError::Unsupported(format!(
                    "mov {dst_text}, {src_text:?}: cannot infer memory size — add a `qword/dword/word/byte ptr` prefix"
                ))
            })?,
        };
        let sized = apply_size(size, mem);
        return emit_mov_into_mem(&mut a, sized, size, src_text, syms, base, dst_text);
    }

    // `mov <reg>, <...>` — register destination.
    let dst = parse_register(dst_text)
        .ok_or_else(|| AsmError::Unsupported(format!("mov dest {dst_text:?}: not a register")))?;
    match dst {
        TypedReg::R64(dst_r) => {
            if let Some(TypedReg::R64(src_r)) = parse_register(src_text) {
                a.mov(dst_r, src_r)?;
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(src_text, syms)? {
                let size = size_opt.unwrap_or(MemSize::Qword);
                if size != MemSize::Qword {
                    return Err(AsmError::Unsupported(format!(
                        "mov {dst_text}, {src_text:?}: r64 destination needs qword ptr source"
                    )));
                }
                a.mov(dst_r, qword_ptr(mem))?;
            } else if let Some(imm) = parse_immediate(src_text, syms) {
                a.mov(dst_r, imm)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "mov {dst_text}, {src_text:?}: src must be a register, memory, or immediate"
                )));
            }
        }
        TypedReg::R32(dst_r) => {
            if let Some(TypedReg::R32(src_r)) = parse_register(src_text) {
                a.mov(dst_r, src_r)?;
            } else if let Some((size_opt, mem)) = try_parse_memory_operand(src_text, syms)? {
                let size = size_opt.unwrap_or(MemSize::Dword);
                if size != MemSize::Dword {
                    return Err(AsmError::Unsupported(format!(
                        "mov {dst_text}, {src_text:?}: r32 destination needs dword ptr source"
                    )));
                }
                a.mov(dst_r, dword_ptr(mem))?;
            } else if let Some(imm) = parse_immediate(src_text, syms) {
                a.mov(dst_r, imm as u32)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "mov {dst_text}, {src_text:?}: src must be a register, memory, or immediate"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(format!(
                "mov {dst_text}: 8/16-bit destination not supported yet"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

pub(super) fn emit_mov_into_mem(
    a: &mut CodeAssembler,
    sized: AsmMemoryOperand,
    size: MemSize,
    src_text: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    dst_text: &str,
) -> Result<Vec<u8>, AsmError> {
    if let Some(reg) = parse_register(src_text) {
        match (size, reg) {
            (MemSize::Qword, TypedReg::R64(r)) => a.mov(sized, r)?,
            (MemSize::Dword, TypedReg::R32(r)) => a.mov(sized, r)?,
            (MemSize::Word, TypedReg::R16(r)) => a.mov(sized, r)?,
            (MemSize::Byte, TypedReg::R8(r)) => a.mov(sized, r)?,
            _ => {
                return Err(AsmError::Unsupported(format!(
                    "mov {dst_text}, {src_text:?}: register width mismatches memory size"
                )));
            }
        }
    } else if let Some(imm) = parse_immediate(src_text, syms) {
        match size {
            MemSize::Byte => a.mov(sized, (imm as u32) & 0xff)?,
            MemSize::Word => a.mov(sized, (imm as u32) & 0xffff)?,
            MemSize::Dword => a.mov(sized, imm as u32)?,
            MemSize::Qword => a.mov(sized, imm as i32)?,
        }
    } else {
        return Err(AsmError::Unsupported(format!(
            "mov {dst_text}, {src_text:?}: src must be a register or immediate"
        )));
    }
    Ok(a.assemble(base)?)
}

pub(super) fn emit_lea(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let (dst_text, src_text) = split_two_operands(rest)?;
    let dst = parse_register(dst_text)
        .ok_or_else(|| AsmError::Unsupported(format!("lea dst {dst_text:?}: not a register")))?;
    // `lea` always takes a memory operand for the source, even without a
    // size prefix. The CE convention is to write `lea rax, [rbx+8]` plain.
    let body = src_text.trim();
    if !body.starts_with('[') || !body.ends_with(']') {
        return Err(AsmError::Unsupported(format!(
            "lea {dst_text}, {src_text:?}: source must be a `[...]` memory expression"
        )));
    }
    let inner = &body[1..body.len() - 1];
    let mem = parse_addr_inside_brackets(inner.trim(), syms)?;
    let mut a = CodeAssembler::new(64)?;
    match dst {
        TypedReg::R64(r) => a.lea(r, mem)?,
        TypedReg::R32(r) => a.lea(r, mem)?,
        _ => {
            return Err(AsmError::Unsupported(format!(
                "lea {dst_text}: 8/16-bit destination not supported"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

#[cfg(test)]
mod tests {
    use super::super::compile_line;
    use super::*;

    #[test]
    fn push_r64_single_byte_opcode() {
        // push rax = 0x50 ; push rbx = 0x53
        assert_eq!(
            compile_line("push rax", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x50]
        );
        assert_eq!(
            compile_line("push rbx", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x53]
        );
        // push r13 = 0x41 0x55 (REX.B + push)
        assert_eq!(
            compile_line("push r13", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x41, 0x55]
        );
    }

    #[test]
    fn push_r32_promotes_to_r64() {
        // push ebx in long mode → push rbx encoding (0x53).
        assert_eq!(
            compile_line("push ebx", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x53]
        );
        assert_eq!(
            compile_line("push r13d", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x41, 0x55]
        );
    }

    #[test]
    fn pop_r64_and_r32_promote() {
        assert_eq!(
            compile_line("pop rax", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x58]
        );
        assert_eq!(
            compile_line("pop r13d", &HashMap::new(), 0)
                .unwrap()
                .unwrap(),
            vec![0x41, 0x5D]
        );
    }

    #[test]
    fn mov_reg64_imm64() {
        // mov rax, 1 → 48 c7 c0 01 00 00 00 (mov r/m64, imm32 sign-extended).
        let bytes = compile_line("mov rax, 1", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes[0], 0x48);
        assert!(bytes.len() >= 5);
    }

    #[test]
    fn mov_reg64_reg64() {
        // mov rax, rbx → 48 89 d8
        let bytes = compile_line("mov rax, rbx", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x48, 0x89, 0xD8]);
    }

    #[test]
    fn mov_reg32_imm32() {
        // mov eax, 1 → b8 01 00 00 00
        let bytes = compile_line("mov eax, 1", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0xB8, 0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn push_immediate() {
        // push 0x1234 → 68 34 12 00 00
        let bytes = compile_line("push 0x1234", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x68, 0x34, 0x12, 0x00, 0x00]);
    }

    #[test]
    fn mov_with_dest_imm_no_register_errors() {
        let err = compile_line("mov 0x1234, rax", &HashMap::new(), 0).unwrap_err();
        assert!(matches!(err, AsmError::Unsupported(_)));
    }

    #[test]
    fn mov_missing_comma_errors() {
        let err = compile_line("mov rax rbx", &HashMap::new(), 0).unwrap_err();
        assert!(matches!(err, AsmError::BadOperand(_)));
    }

    #[test]
    fn mov_dword_ptr_reg_disp_with_float_literal() {
        // The canonical Aurora pattern:
        //   mov dword ptr [r13+13C], (float)100
        // 100.0f32 = 0x42C80000.
        let bytes = compile_line("mov dword ptr [r13+13C],(float)100", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        // Check the encoding ends with the float bits in little-endian:
        // 00 00 C8 42.
        assert_eq!(&bytes[bytes.len() - 4..], &[0x00, 0x00, 0xC8, 0x42]);
        // C7 is the mov r/m32, imm32 opcode; first byte is the REX prefix
        // for the r13 base (REX.B set: 0x41).
        assert_eq!(bytes[0], 0x41);
        assert_eq!(bytes[1], 0xC7);
    }

    #[test]
    fn mov_reg_from_mem_qword() {
        // mov rax, qword ptr [rbx]
        let bytes = compile_line("mov rax, qword ptr [rbx]", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        // REX.W + 8B + ModR/M; 3 bytes total for [rbx] (no displacement).
        assert_eq!(bytes, vec![0x48, 0x8B, 0x03]);
    }

    #[test]
    fn mov_mem_from_reg_qword() {
        // mov qword ptr [rbx], rax
        let bytes = compile_line("mov qword ptr [rbx], rax", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x48, 0x89, 0x03]);
    }

    #[test]
    fn memory_operand_minus_displacement() {
        // mov dword ptr [r13-8], 0
        let bytes = compile_line("mov dword ptr [r13-8], 0", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        // Last byte of disp8 = -8 = 0xF8.
        assert!(bytes.contains(&0xF8));
    }

    #[test]
    fn malformed_memory_operand_errors() {
        let err = compile_line("mov dword ptr r13+13C, 0", &HashMap::new(), 0).unwrap_err();
        // Without brackets, the parser sees a non-register dest and rejects
        // it as an unsupported source.
        assert!(matches!(err, AsmError::Unsupported(_)));
    }

    #[test]
    fn lea_reg64_with_reg_disp() {
        // lea rax, [rbx+8] → 48 8D 43 08
        let bytes = compile_line("lea rax, [rbx+8]", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0x48, 0x8D, 0x43, 0x08]);
    }
}
