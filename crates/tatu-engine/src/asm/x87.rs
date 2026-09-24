//! x87 floating-point mnemonics: `fld`, `fstp`, `fild`, `fistp`, `fadd`,
//! `fsub`, `fmul`, `fdiv`, plus the 0-operand transcendentals
//! (`fsin`/`fcos`/`fsqrt`/`fchs`/`fabs`) and constant loaders
//! (`fld1`/`fldz`/`fldpi`).
//!
//! Tier-3 addition. Audit hits in the FearLess corpus:
//! `fstp` 22, `fld` 21, `fmul` 13, `fild` 10, `fistp` 10, `fsin` 9,
//! `fcos` 6 — ~85 lines combined (~2.8% of the corpus). Modern UE/Unity
//! tables prefer SSE2, but older or hand-rolled tables (Mass Effect:
//! Andromeda, Crimson Desert legacy patches) still use x87 for player
//! coords / stat scaling.
//!
//! Operand shapes recognised:
//!
//! - **0-arg** (`fsin`, `fcos`, `fsqrt`, `fchs`, `fabs`, `fld1`, `fldz`,
//!   `fldpi`): no operand.
//! - **1-arg memory** (`fld`, `fstp`, `fild`, `fistp`, `fadd`, `fsub`,
//!   `fmul`, `fdiv`): `mnem dword/qword/word ptr [addr]`. The explicit
//!   size prefix is required for x87 — `fld [rax]` is ambiguous between
//!   single, double, and extended precision; we reject it with a clear
//!   error to point the user at the missing prefix.

use std::collections::HashMap;

use iced_x86::code_asm::{CodeAssembler, dword_ptr, qword_ptr, word_ptr};

use super::AsmError;
use super::operands::{MemSize, try_parse_memory_operand};

/// Compile a 0-operand x87 instruction. Returns `Ok(None)` when the
/// mnemonic isn't in the recognised 0-arg set so the top-level dispatcher
/// can fall through.
fn emit_x87_zero_arg(mnem: &str, base: u64) -> Result<Option<Vec<u8>>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    match mnem {
        "fsin" => a.fsin()?,
        "fcos" => a.fcos()?,
        "fsqrt" => a.fsqrt()?,
        "fchs" => a.fchs()?,
        "fabs" => a.fabs()?,
        "fld1" => a.fld1()?,
        "fldz" => a.fldz()?,
        "fldpi" => a.fldpi()?,
        _ => return Ok(None),
    };
    Ok(Some(a.assemble(base)?))
}

/// Compile a 1-operand x87 memory instruction. The operand must have an
/// explicit size prefix (`dword ptr`, `qword ptr`, or for integer ops
/// `word ptr` / `dword ptr` / `qword ptr`).
fn emit_x87_mem_arg(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    mnem: &str,
) -> Result<Vec<u8>, AsmError> {
    let (size_opt, mem) = try_parse_memory_operand(rest, syms)?.ok_or_else(|| {
        AsmError::Unsupported(format!(
            "{mnem} {rest:?}: x87 requires a memory operand (st(n) register form not supported)"
        ))
    })?;
    let size = size_opt.ok_or_else(|| {
        AsmError::Unsupported(format!(
            "{mnem} {rest:?}: x87 needs an explicit `dword/qword/word ptr` size prefix"
        ))
    })?;
    let mut a = CodeAssembler::new(64)?;
    // Float ops (fld/fstp/fadd/fsub/fmul/fdiv) accept dword or qword;
    // integer ops (fild/fistp) accept word/dword/qword. iced encodes the
    // correct opcode based on the typed memory operand we hand it.
    match (mnem, size) {
        ("fld", MemSize::Dword) => a.fld(dword_ptr(mem))?,
        ("fld", MemSize::Qword) => a.fld(qword_ptr(mem))?,
        ("fstp", MemSize::Dword) => a.fstp(dword_ptr(mem))?,
        ("fstp", MemSize::Qword) => a.fstp(qword_ptr(mem))?,
        ("fadd", MemSize::Dword) => a.fadd(dword_ptr(mem))?,
        ("fadd", MemSize::Qword) => a.fadd(qword_ptr(mem))?,
        ("fsub", MemSize::Dword) => a.fsub(dword_ptr(mem))?,
        ("fsub", MemSize::Qword) => a.fsub(qword_ptr(mem))?,
        ("fmul", MemSize::Dword) => a.fmul(dword_ptr(mem))?,
        ("fmul", MemSize::Qword) => a.fmul(qword_ptr(mem))?,
        ("fdiv", MemSize::Dword) => a.fdiv(dword_ptr(mem))?,
        ("fdiv", MemSize::Qword) => a.fdiv(qword_ptr(mem))?,
        ("fild", MemSize::Word) => a.fild(word_ptr(mem))?,
        ("fild", MemSize::Dword) => a.fild(dword_ptr(mem))?,
        ("fild", MemSize::Qword) => a.fild(qword_ptr(mem))?,
        ("fistp", MemSize::Word) => a.fistp(word_ptr(mem))?,
        ("fistp", MemSize::Dword) => a.fistp(dword_ptr(mem))?,
        ("fistp", MemSize::Qword) => a.fistp(qword_ptr(mem))?,
        (other, sz) => {
            return Err(AsmError::Unsupported(format!(
                "{other} {sz:?}: unsupported x87 (size, mnemonic) pair"
            )));
        }
    };
    Ok(a.assemble(base)?)
}

pub(super) fn dispatch_x87_mnemonic(
    mnem: &str,
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Option<Vec<u8>>, AsmError> {
    // 0-arg first (the rest argument must be empty for those).
    if rest.trim().is_empty()
        && let Some(bytes) = emit_x87_zero_arg(mnem, base)?
    {
        return Ok(Some(bytes));
    }
    match mnem {
        "fld" | "fstp" | "fild" | "fistp" | "fadd" | "fsub" | "fmul" | "fdiv" => {
            Ok(Some(emit_x87_mem_arg(rest, syms, base, mnem)?))
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

    // --- 0-arg ---

    #[test]
    fn fsin_zero_arg() {
        let bytes = compile_line("fsin", &empty(), 0).unwrap().unwrap();
        // D9 FE
        assert_eq!(bytes, vec![0xD9, 0xFE]);
    }

    #[test]
    fn fcos_zero_arg() {
        let bytes = compile_line("fcos", &empty(), 0).unwrap().unwrap();
        assert_eq!(bytes, vec![0xD9, 0xFF]);
    }

    #[test]
    fn fld1_constant_loader() {
        let bytes = compile_line("fld1", &empty(), 0).unwrap().unwrap();
        // D9 E8
        assert_eq!(bytes, vec![0xD9, 0xE8]);
    }

    // --- 1-arg memory ---

    #[test]
    fn fld_dword_ptr_mem() {
        let bytes = compile_line("fld dword ptr [rax]", &empty(), 0)
            .unwrap()
            .unwrap();
        // D9 00 — fld dword ptr [rax]
        assert_eq!(bytes, vec![0xD9, 0x00]);
    }

    #[test]
    fn fstp_qword_ptr_mem_with_disp() {
        let bytes = compile_line("fstp qword ptr [rbx+8]", &empty(), 0)
            .unwrap()
            .unwrap();
        // DD 5B 08 — fstp qword ptr [rbx+0x08]
        assert_eq!(bytes, vec![0xDD, 0x5B, 0x08]);
    }

    #[test]
    fn fmul_dword_ptr_mem() {
        let bytes = compile_line("fmul dword ptr [rcx]", &empty(), 0)
            .unwrap()
            .unwrap();
        // D8 09 — fmul dword ptr [rcx]
        assert_eq!(bytes, vec![0xD8, 0x09]);
    }

    #[test]
    fn fild_dword_ptr_mem() {
        let bytes = compile_line("fild dword ptr [rax]", &empty(), 0)
            .unwrap()
            .unwrap();
        // DB 00 — fild dword ptr [rax]
        assert_eq!(bytes, vec![0xDB, 0x00]);
    }

    #[test]
    fn fistp_word_ptr_mem() {
        let bytes = compile_line("fistp word ptr [rax]", &empty(), 0)
            .unwrap()
            .unwrap();
        // DF 18 — fistp word ptr [rax]
        assert_eq!(bytes, vec![0xDF, 0x18]);
    }

    #[test]
    fn fld_without_size_prefix_errors() {
        let err = compile_line("fld [rax]", &empty(), 0).unwrap_err();
        // The clear error points at the missing prefix.
        let msg = format!("{err}");
        assert!(
            msg.contains("size prefix"),
            "expected 'size prefix' in error, got: {msg}"
        );
    }
}
