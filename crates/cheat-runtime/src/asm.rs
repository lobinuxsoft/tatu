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
//! Scope (Phase B v2.1):
//!
//! - `jmp <addr|symbol>` / `call <addr|symbol>` / `ret` (Phase B v1).
//! - Conditional jumps with a `<target>` operand: `je`/`jne`/`jg`/`jge`/
//!   `jl`/`jle`/`ja`/`jae`/`jb`/`jbe`/`jz`/`jnz`/`jc`/`jnc`/`js`/`jns`.
//! - `push <reg64|reg32|imm32>`, `pop <reg64|reg32>` — 32-bit register
//!   names alias to their 64-bit equivalent, matching CE's behaviour
//!   when porting Win32-era scripts to x86_64.
//! - `mov <reg>, <reg>` and `mov <reg>, <imm>` (same width, no memory
//!   operands yet).
//!
//! Out of scope for v2.1: memory operands like `dword ptr [r13+13C]`,
//! float-literal coercion `(float)100`, `cmp` / `add` / `sub` / `xor` /
//! `lea`. Those land in Phase B v2.2 alongside the memory-operand parser.

use std::collections::HashMap;

use iced_x86::code_asm::{
    AsmRegister8, AsmRegister16, AsmRegister32, AsmRegister64, CodeAssembler, registers as r,
};

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
        "jmp" => Ok(Some(emit_unary_target(
            rest,
            symbols,
            base_addr,
            Mnemonic::Jmp,
        )?)),
        "call" => Ok(Some(emit_unary_target(
            rest,
            symbols,
            base_addr,
            Mnemonic::Call,
        )?)),
        "ret" | "retn" => Ok(Some(emit_ret()?)),
        "push" => Ok(Some(emit_push(rest, symbols, base_addr)?)),
        "pop" => Ok(Some(emit_pop(rest, base_addr)?)),
        "mov" => Ok(Some(emit_mov(rest, symbols, base_addr)?)),
        m if is_conditional_jump(m) => Ok(Some(emit_jcc(m, rest, symbols, base_addr)?)),
        _ => Ok(None),
    }
}

// ---- mnemonic helpers --------------------------------------------------

enum Mnemonic {
    Jmp,
    Call,
}

fn is_conditional_jump(m: &str) -> bool {
    matches!(
        m,
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
    )
}

fn emit_unary_target(
    rest: &str,
    syms: &HashMap<String, u64>,
    base: u64,
    m: Mnemonic,
) -> Result<Vec<u8>, AsmError> {
    let target = resolve_target(rest, syms)?;
    let mut a = CodeAssembler::new(64)?;
    match m {
        Mnemonic::Jmp => a.jmp(target)?,
        Mnemonic::Call => a.call(target)?,
    };
    Ok(a.assemble(base)?)
}

fn emit_ret() -> Result<Vec<u8>, AsmError> {
    let mut a = CodeAssembler::new(64)?;
    a.ret()?;
    Ok(a.assemble(0)?)
}

fn emit_jcc(
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
        "jg" => a.jg(target)?,
        "jge" => a.jge(target)?,
        "jl" => a.jl(target)?,
        "jle" => a.jle(target)?,
        "ja" => a.ja(target)?,
        "jae" | "jnc" => a.jae(target)?,
        "jb" | "jc" => a.jb(target)?,
        "jbe" => a.jbe(target)?,
        "js" => a.js(target)?,
        "jns" => a.jns(target)?,
        _ => return Err(AsmError::Unsupported(mnemonic.into())),
    };
    Ok(a.assemble(base)?)
}

// ---- push / pop / mov --------------------------------------------------

fn emit_push(rest: &str, syms: &HashMap<String, u64>, base: u64) -> Result<Vec<u8>, AsmError> {
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

fn emit_pop(rest: &str, base: u64) -> Result<Vec<u8>, AsmError> {
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

fn emit_mov(rest: &str, syms: &HashMap<String, u64>, base: u64) -> Result<Vec<u8>, AsmError> {
    let (dst_text, src_text) = rest
        .split_once(',')
        .ok_or_else(|| AsmError::BadOperand(rest.into()))?;
    let dst_text = dst_text.trim();
    let src_text = src_text.trim();

    let dst = parse_register(dst_text)
        .ok_or_else(|| AsmError::Unsupported(format!("mov dest {dst_text:?}: not a register")))?;
    let mut a = CodeAssembler::new(64)?;
    match dst {
        TypedReg::R64(dst_r) => {
            if let Some(TypedReg::R64(src_r)) = parse_register(src_text) {
                a.mov(dst_r, src_r)?;
            } else if let Some(imm) = resolve_numeric_or_symbol(src_text, syms) {
                a.mov(dst_r, imm)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "mov {dst_text}, {src_text:?}: src must be a 64-bit register or immediate"
                )));
            }
        }
        TypedReg::R32(dst_r) => {
            if let Some(TypedReg::R32(src_r)) = parse_register(src_text) {
                a.mov(dst_r, src_r)?;
            } else if let Some(imm) = resolve_numeric_or_symbol(src_text, syms) {
                a.mov(dst_r, imm as u32)?;
            } else {
                return Err(AsmError::Unsupported(format!(
                    "mov {dst_text}, {src_text:?}: src must be a 32-bit register or immediate"
                )));
            }
        }
        _ => {
            return Err(AsmError::Unsupported(format!(
                "mov {dst_text}: 8/16-bit destination not supported in v2.1"
            )));
        }
    }
    Ok(a.assemble(base)?)
}

// ---- operand parsing ---------------------------------------------------

enum TypedReg {
    // R8 / R16 are recognised by the parser but the v2.1 emitters reject
    // them with `Unsupported`. Reserved here so adding 8/16-bit support in
    // Phase B v2.2+ touches one match arm, not the table.
    #[allow(dead_code)]
    R8(AsmRegister8),
    #[allow(dead_code)]
    R16(AsmRegister16),
    R32(AsmRegister32),
    R64(AsmRegister64),
}

/// Best-effort name → typed iced-x86 register constant. Covers the integer
/// general-purpose set across all four widths; XMM / YMM / segment / control
/// registers stay None until a Phase B v2.2+ trainer needs them.
fn parse_register(name: &str) -> Option<TypedReg> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        // 64-bit
        "rax" => TypedReg::R64(r::rax),
        "rbx" => TypedReg::R64(r::rbx),
        "rcx" => TypedReg::R64(r::rcx),
        "rdx" => TypedReg::R64(r::rdx),
        "rsi" => TypedReg::R64(r::rsi),
        "rdi" => TypedReg::R64(r::rdi),
        "rbp" => TypedReg::R64(r::rbp),
        "rsp" => TypedReg::R64(r::rsp),
        "r8" => TypedReg::R64(r::r8),
        "r9" => TypedReg::R64(r::r9),
        "r10" => TypedReg::R64(r::r10),
        "r11" => TypedReg::R64(r::r11),
        "r12" => TypedReg::R64(r::r12),
        "r13" => TypedReg::R64(r::r13),
        "r14" => TypedReg::R64(r::r14),
        "r15" => TypedReg::R64(r::r15),
        // 32-bit
        "eax" => TypedReg::R32(r::eax),
        "ebx" => TypedReg::R32(r::ebx),
        "ecx" => TypedReg::R32(r::ecx),
        "edx" => TypedReg::R32(r::edx),
        "esi" => TypedReg::R32(r::esi),
        "edi" => TypedReg::R32(r::edi),
        "ebp" => TypedReg::R32(r::ebp),
        "esp" => TypedReg::R32(r::esp),
        "r8d" => TypedReg::R32(r::r8d),
        "r9d" => TypedReg::R32(r::r9d),
        "r10d" => TypedReg::R32(r::r10d),
        "r11d" => TypedReg::R32(r::r11d),
        "r12d" => TypedReg::R32(r::r12d),
        "r13d" => TypedReg::R32(r::r13d),
        "r14d" => TypedReg::R32(r::r14d),
        "r15d" => TypedReg::R32(r::r15d),
        // 16-bit
        "ax" => TypedReg::R16(r::ax),
        "bx" => TypedReg::R16(r::bx),
        "cx" => TypedReg::R16(r::cx),
        "dx" => TypedReg::R16(r::dx),
        // 8-bit
        "al" => TypedReg::R8(r::al),
        "bl" => TypedReg::R8(r::bl),
        "cl" => TypedReg::R8(r::cl),
        "dl" => TypedReg::R8(r::dl),
        _ => return None,
    })
}

/// `push ebx` in 64-bit mode encodes as `push rbx`. iced-x86 refuses the
/// 32-bit form in long mode, so promote on the fly. CE does the same when
/// the original .CT was authored against a Win32 game and the user opens
/// it in CE 64-bit mode.
fn promote_to_r64(name: &str) -> Option<AsmRegister64> {
    let n = name.trim().to_ascii_lowercase();
    let r64_name: &str = match n.as_str() {
        "eax" => "rax",
        "ebx" => "rbx",
        "ecx" => "rcx",
        "edx" => "rdx",
        "esi" => "rsi",
        "edi" => "rdi",
        "ebp" => "rbp",
        "esp" => "rsp",
        "r8d" => "r8",
        "r9d" => "r9",
        "r10d" => "r10",
        "r11d" => "r11",
        "r12d" => "r12",
        "r13d" => "r13",
        "r14d" => "r14",
        "r15d" => "r15",
        _ => return None,
    };
    match parse_register(r64_name)? {
        TypedReg::R64(r) => Some(r),
        _ => None,
    }
}

fn resolve_numeric_or_symbol(token: &str, syms: &HashMap<String, u64>) -> Option<u64> {
    if let Some(v) = parse_numeric(token) {
        return Some(v);
    }
    syms.get(token.trim()).copied()
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
    if let Some(idx) = t.rfind(|c| c == '+' || c == '-')
        && idx > 0
    {
        let (sym, op_and_off) = t.split_at(idx);
        let (op, off) = op_and_off.split_at(1);
        let base = symbols
            .get(sym.trim())
            .copied()
            .or_else(|| parse_numeric(sym.trim()))
            .ok_or_else(|| AsmError::UnknownSymbol(sym.trim().to_string()))?;
        let off = parse_numeric(off.trim()).ok_or_else(|| AsmError::BadOperand(operand.into()))?;
        return Ok(match op {
            "+" => base.wrapping_add(off),
            _ => base.wrapping_sub(off),
        });
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
    fn unknown_mnemonic_returns_none() {
        // `cmp` with memory operand falls through to None until Phase B v2.2.
        let bytes = compile_line("cmp byte ptr [foo], 1", &HashMap::new(), 0).unwrap();
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
}
