//! Translate Cheat Engine Auto-Assembler asm lines to x86_64 machine code.
//!
//! Ported in spirit from CE's `autoassemblercode.pas` line dispatcher and
//! `Assemblerunit.pas` x86_64 encoder. The Pascal encoder is ~8700 LOC and
//! does its own ModR/M / SIB / REX layout; rather than re-port it byte for
//! byte, this module parses the CE-AA line just far enough to identify the
//! mnemonic + operands and dispatches into [`iced_x86::code_asm::CodeAssembler`]
//! for the actual encoding. iced-x86 is the canonical Rust x86 encoder
//! (MIT, maintained, exhaustive opcode coverage), so we pay for "no native
//! deps" with a Rust crate instead of porting Pascal opcode tables.
//!
//! Scope (Phase B v1): the minimum subset that lets a CE-AA code-injection
//! script compose against `aobscanmodule + alloc + codecave + jmp`:
//!
//! - `jmp <addr|symbol>` (+ optional `+N` offset suffix on the symbol)
//! - `call <addr|symbol>`
//! - `ret`
//!
//! Anything else still returns [`AsmError::Unsupported`] so the executor
//! surface stays the same. Phase B v2 will broaden coverage as smoke tests
//! against real trainers reveal which mnemonics are actually used.
//!
//! `base_addr` is the absolute address where the bytes will be written in
//! the target — needed because `jmp` / `call` emit rip-relative offsets, so
//! the encoding depends on the cursor's position.

use std::collections::HashMap;

use iced_x86::code_asm::CodeAssembler;

#[derive(Debug, thiserror::Error)]
pub enum AsmError {
    #[error("unknown symbol {0:?}")]
    UnknownSymbol(String),
    #[error("bad operand {0:?}")]
    BadOperand(String),
    #[error("iced-x86 encoding failure: {0}")]
    IcedX86(#[from] iced_x86::IcedError),
    #[error("unsupported asm line: {0:?}")]
    Unsupported(String),
}

/// Compile a single CE-AA asm line to bytes. Returns `Ok(None)` if the line
/// is not asm (caller should keep falling through to its own `db`/`dq`/`nop`
/// handlers).
pub fn compile_line(
    line: &str,
    symbols: &HashMap<String, u64>,
    base_addr: u64,
) -> Result<Option<Vec<u8>>, AsmError> {
    let trimmed = line.trim();
    let (mnemonic, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((m, r)) => (m.to_ascii_lowercase(), r.trim()),
        None => (trimmed.to_ascii_lowercase(), ""),
    };

    match mnemonic.as_str() {
        "jmp" => Ok(Some(emit_jmp(rest, symbols, base_addr)?)),
        "call" => Ok(Some(emit_call(rest, symbols, base_addr)?)),
        "ret" | "retn" => Ok(Some(emit_ret()?)),
        _ => Ok(None),
    }
}

fn emit_jmp(rest: &str, syms: &HashMap<String, u64>, base: u64) -> Result<Vec<u8>, AsmError> {
    let target = resolve_target(rest, syms)?;
    let mut a = CodeAssembler::new(64)?;
    a.jmp(target)?;
    Ok(a.assemble(base)?)
}

fn emit_call(rest: &str, syms: &HashMap<String, u64>, base: u64) -> Result<Vec<u8>, AsmError> {
    let target = resolve_target(rest, syms)?;
    let mut a = CodeAssembler::new(64)?;
    a.call(target)?;
    Ok(a.assemble(base)?)
}

fn emit_ret() -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    a.ret()?;
    Ok(a.assemble(0)?)
}

/// Resolve an operand to an absolute address. Accepted forms:
/// - `0xDEADBEEF`, `$DEADBEEF`, decimal — numeric literal
/// - `symbol` — looks up in the symbol table
/// - `symbol+N` / `symbol-N` — symbol with byte offset (N hex or decimal)
fn resolve_target(operand: &str, symbols: &HashMap<String, u64>) -> Result<u64, AsmError> {
    let t = operand.trim();
    if t.is_empty() {
        return Err(AsmError::BadOperand(operand.into()));
    }
    if let Some(addr) = parse_numeric(t) {
        return Ok(addr);
    }
    // `symbol+N` or `symbol-N`
    if let Some(idx) = t.rfind(|c| c == '+' || c == '-') {
        if idx > 0 {
            let (sym, op_and_off) = t.split_at(idx);
            let (op, off) = op_and_off.split_at(1);
            let base = symbols
                .get(sym.trim())
                .copied()
                .or_else(|| parse_numeric(sym.trim()))
                .ok_or_else(|| AsmError::UnknownSymbol(sym.trim().to_string()))?;
            let off =
                parse_numeric(off.trim()).ok_or_else(|| AsmError::BadOperand(operand.into()))?;
            return Ok(match op {
                "+" => base.wrapping_add(off),
                _ => base.wrapping_sub(off),
            });
        }
    }
    symbols
        .get(t)
        .copied()
        .ok_or_else(|| AsmError::UnknownSymbol(t.to_string()))
}

fn parse_numeric(token: &str) -> Option<u64> {
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = token.strip_prefix('$') {
        return u64::from_str_radix(hex, 16).ok();
    }
    token.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
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
        // 5 bytes: E9 + rel32 with delta = 0x1000 - (0x500 + 5) = 0xAFB
        assert_eq!(bytes, vec![0xE9, 0xFB, 0x0A, 0x00, 0x00]);
    }

    #[test]
    fn jmp_to_symbol_resolves_via_table() {
        let syms = symtab(&[("codecave", 0x2000)]);
        let bytes = compile_line("jmp codecave", &syms, 0x1000)
            .unwrap()
            .unwrap();
        // rel32 = 0x2000 - (0x1000 + 5) = 0xFFB
        assert_eq!(bytes, vec![0xE9, 0xFB, 0x0F, 0x00, 0x00]);
    }

    #[test]
    fn jmp_to_symbol_plus_offset() {
        let syms = symtab(&[("orig", 0x10000)]);
        let bytes = compile_line("jmp orig+5", &syms, 0x20000).unwrap().unwrap();
        // target = 0x10005, rel32 = 0x10005 - (0x20000 + 5) = -0xFFFFC = 0xFFF00004 (i32)
        assert_eq!(bytes[0], 0xE9);
        let delta = i32::from_le_bytes(bytes[1..5].try_into().unwrap()) as i64;
        assert_eq!(delta, 0x10005 - 0x20005);
    }

    #[test]
    fn call_to_numeric_emits_e8_relative() {
        let bytes = compile_line("call 0x2000", &HashMap::new(), 0x1000)
            .unwrap()
            .unwrap();
        // E8 + rel32 with delta = 0x2000 - 0x1005 = 0xFFB
        assert_eq!(bytes, vec![0xE8, 0xFB, 0x0F, 0x00, 0x00]);
    }

    #[test]
    fn ret_emits_c3() {
        let bytes = compile_line("ret", &HashMap::new(), 0).unwrap().unwrap();
        assert_eq!(bytes, vec![0xC3]);
        let bytes_n = compile_line("retn", &HashMap::new(), 0).unwrap().unwrap();
        assert_eq!(bytes_n, vec![0xC3]);
    }

    #[test]
    fn unknown_mnemonic_returns_none() {
        let bytes = compile_line("mov rax, 1", &HashMap::new(), 0).unwrap();
        assert!(bytes.is_none());
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
