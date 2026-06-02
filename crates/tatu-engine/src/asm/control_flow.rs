//! Control-flow mnemonics: `jmp`, `call`, `ret`, conditional jumps.

use std::collections::HashMap;

use iced_x86::code_asm::{CodeAssembler, qword_ptr};

use super::AsmError;
use super::operands::{MemSize, resolve_target, try_parse_memory_operand};

#[derive(Debug)]
pub(super) enum Mnemonic {
    Jmp,
    Call,
}

pub(super) fn is_conditional_jump(m: &str) -> bool {
    matches!(
        m,
        // Canonical Intel mnemonics.
        "je" | "jne"
            | "jg"
            | "jge"
            | "jl"
            | "jle"
            | "ja"
            | "jae"
            | "jb"
            | "jbe"
            | "jz"
            | "jnz"
            | "jc"
            | "jnc"
            | "js"
            | "jns"
            | "jo"
            | "jno"
            | "jp"
            | "jnp"
            | "jpe"
            | "jpo"
            // CE-AA / NASM-style aliases (still appear in older / hand-rolled tables).
            | "jna"   // = jbe
            | "jnae"  // = jb
            | "jnb"   // = jae
            | "jnbe"  // = ja
            | "jng"   // = jle
            | "jnge"  // = jl
            | "jnl"   // = jge
            | "jnle" // = jg
    )
}

pub(super) fn emit_unary_target(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    m: Mnemonic,
) -> Result<Vec<u8>, AsmError> {
    // CE-AA `jmp far <target>` / `jmp near <target>` — distance hints the
    // x86 assembler historically needed to pick the operand size. In long
    // mode they are no-ops: iced already selects `rel32` (or an indirect
    // trampoline) based on the resolved target, so strip the hint and treat
    // the rest as the target. CE table authors emit `jmp far` to force the
    // 5-byte hook form (DD2 RE Engine codecaves).
    let rest = rest
        .strip_prefix("far ")
        .or_else(|| rest.strip_prefix("near "))
        .map(str::trim_start)
        .unwrap_or(rest);

    let mut a = CodeAssembler::new(64)?;

    // Indirect call/jmp through memory: `call qword ptr [rax+370]`. CE
    // tables use this for vtable dispatch hooks. iced-x86 distinguishes
    // `a.call(addr)` (rel32 to absolute) from `a.call(qword_ptr(mem))`
    // (FF /2 modrm) — pick the right one based on whether the operand
    // parses as a memory operand.
    if let Some((size_opt, mem)) = try_parse_memory_operand(rest.trim(), syms)? {
        let size = size_opt.unwrap_or(MemSize::Qword);
        if size != MemSize::Qword {
            return Err(AsmError::Unsupported(format!(
                "indirect {m:?} target {rest:?}: only qword ptr memory operands are supported"
            )));
        }
        match m {
            Mnemonic::Jmp => a.jmp(qword_ptr(mem))?,
            Mnemonic::Call => a.call(qword_ptr(mem))?,
        };
        return Ok(a.assemble(base)?);
    }

    let target = resolve_target(rest, syms)?;
    match m {
        Mnemonic::Jmp => a.jmp(target)?,
        Mnemonic::Call => a.call(target)?,
    };
    Ok(a.assemble(base)?)
}

pub(super) fn emit_ret() -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    a.ret()?;
    Ok(a.assemble(0)?)
}

pub(super) fn emit_jcc(
    mnemonic: &str,
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
) -> Result<Vec<u8>, AsmError> {
    let target = resolve_target(rest, syms)?;
    let mut a = CodeAssembler::new(64)?;
    // CE-AA aliases: jz=je, jnz=jne, jc=jb, jnc=jae.
    match mnemonic {
        "je" | "jz" => a.je(target)?,
        "jne" | "jnz" => a.jne(target)?,
        "jg" | "jnle" => a.jg(target)?,
        "jge" | "jnl" => a.jge(target)?,
        "jl" | "jnge" => a.jl(target)?,
        "jle" | "jng" => a.jle(target)?,
        "ja" | "jnbe" => a.ja(target)?,
        "jae" | "jnc" | "jnb" => a.jae(target)?,
        "jb" | "jc" | "jnae" => a.jb(target)?,
        "jbe" | "jna" => a.jbe(target)?,
        "js" => a.js(target)?,
        "jns" => a.jns(target)?,
        "jo" => a.jo(target)?,
        "jno" => a.jno(target)?,
        "jp" | "jpe" => a.jp(target)?,
        "jnp" | "jpo" => a.jnp(target)?,
        _ => return Err(AsmError::Unsupported(mnemonic.into())),
    };
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
    fn jmp_to_numeric_emits_e9_relative() {
        let bytes = compile_line("jmp 0x1000", &HashMap::new(), 0x500)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0xE9, 0xFB, 0x0A, 0x00, 0x00]);
    }

    #[test]
    fn jmp_to_symbol_resolves_via_table() {
        let syms = symtab(&[("codecave", 0x2000)]);
        let bytes = compile_line("jmp codecave", &syms, 0x1000)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0xE9, 0xFB, 0x0F, 0x00, 0x00]);
    }

    #[test]
    fn jmp_to_symbol_plus_offset() {
        let syms = symtab(&[("orig", 0x10000)]);
        let bytes = compile_line("jmp orig+5", &syms, 0x20000).unwrap().unwrap();
        assert_eq!(bytes[0], 0xE9);
        let delta = i32::from_le_bytes(bytes[1..5].try_into().unwrap()) as i64;
        assert_eq!(delta, 0x10005 - 0x20005);
    }

    #[test]
    fn jmp_far_and_near_hints_are_stripped() {
        let syms = symtab(&[("codecave", 0x2000)]);
        // `jmp far codecave` must encode identically to `jmp codecave` — the
        // distance hint is a long-mode no-op (DD2 hook form).
        let plain = compile_line("jmp codecave", &syms, 0x1000)
            .unwrap()
            .unwrap();
        let far = compile_line("jmp far codecave", &syms, 0x1000)
            .unwrap()
            .unwrap();
        let near = compile_line("jmp near codecave", &syms, 0x1000)
            .unwrap()
            .unwrap();
        assert_eq!(far, plain);
        assert_eq!(near, plain);
    }

    #[test]
    fn call_to_numeric_emits_e8_relative() {
        let bytes = compile_line("call 0x2000", &HashMap::new(), 0x1000)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, vec![0xE8, 0xFB, 0x0F, 0x00, 0x00]);
    }

    #[test]
    fn ret_emits_c3() {
        assert_eq!(
            compile_line("ret", &HashMap::new(), 0).unwrap().unwrap(),
            vec![0xC3]
        );
        assert_eq!(
            compile_line("retn", &HashMap::new(), 0).unwrap().unwrap(),
            vec![0xC3]
        );
    }

    #[test]
    fn conditional_jumps_all_variants() {
        let cases = [
            ("je", 0x84_u8),
            ("jne", 0x85),
            ("jg", 0x8F),
            ("jge", 0x8D),
            ("jl", 0x8C),
            ("jle", 0x8E),
            ("ja", 0x87),
            ("jae", 0x83),
            ("jb", 0x82),
            ("jbe", 0x86),
            ("js", 0x88),
            ("jns", 0x89),
        ];
        for (mnemonic, opcode) in cases {
            let line = format!("{mnemonic} 0x1000");
            let bytes = compile_line(&line, &HashMap::new(), 0x500)
                .unwrap()
                .unwrap();
            // jcc rel32 = 0F <opcode> + 4-byte rel32; 6 bytes total.
            assert_eq!(bytes.len(), 6, "{mnemonic} should encode in 6 bytes");
            assert_eq!(bytes[0], 0x0F, "{mnemonic} prefix");
            assert_eq!(bytes[1], opcode, "{mnemonic} opcode");
        }
    }

    #[test]
    fn jcc_aliases_jz_jnz_jc_jnc() {
        let bytes_jz = compile_line("jz 0x1000", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        let bytes_je = compile_line("je 0x1000", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes_jz, bytes_je);
        let bytes_jnz = compile_line("jnz 0x1000", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        let bytes_jne = compile_line("jne 0x1000", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes_jnz, bytes_jne);
        let bytes_jc = compile_line("jc 0x1000", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        let bytes_jb = compile_line("jb 0x1000", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(bytes_jc, bytes_jb);
    }

    #[test]
    fn jne_to_symbol() {
        let syms = symtab(&[("@f", 0x1000)]);
        // Conditional jump rel32 = 6 bytes
        let bytes = compile_line("jne @f", &syms, 0x500).unwrap().unwrap();
        assert_eq!(bytes.len(), 6);
    }

    #[test]
    fn unknown_symbol_errors() {
        let err = compile_line("jmp missing", &HashMap::new(), 0).unwrap_err();
        assert!(matches!(err, AsmError::UnknownSymbol(s) if s == "missing"));
    }

    #[test]
    fn bad_operand_errors() {
        let err = compile_line("jmp ", &HashMap::new(), 0).unwrap_err();
        assert!(matches!(err, AsmError::BadOperand(_)));
    }

    #[test]
    fn symbol_offset_minus() {
        let syms = symtab(&[("orig", 0x10000)]);
        let bytes = compile_line("jmp orig-0x10", &syms, 0x5000)
            .unwrap()
            .unwrap();
        let delta = i32::from_le_bytes(bytes[1..5].try_into().unwrap()) as i64;
        assert_eq!(delta, (0x10000 - 0x10) - (0x5000 + 5));
    }
}
