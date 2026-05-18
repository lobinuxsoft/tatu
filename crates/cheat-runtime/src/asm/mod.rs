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
//! Scope (Phase B v2.2):
//!
//! - `jmp <addr|symbol>` / `call <addr|symbol>` / `ret` (Phase B v1).
//! - Conditional jumps with a `<target>` operand: `je`/`jne`/`jg`/`jge`/
//!   `jl`/`jle`/`ja`/`jae`/`jb`/`jbe`/`jz`/`jnz`/`jc`/`jnc`/`js`/`jns`.
//! - `push <reg64|reg32|imm32>`, `pop <reg64|reg32>` — 32-bit register
//!   names alias to their 64-bit equivalent, matching CE's behaviour
//!   when porting Win32-era scripts to x86_64.
//! - `mov` / `cmp` with the full operand matrix:
//!     - `<reg>, <reg>` / `<reg>, <imm>` (Phase B v2.1)
//!     - `<reg>, <mem>` / `<mem>, <reg>` / `<mem>, <imm>` (new in v2.2)
//! - Memory operand syntax: `byte|word|dword|qword ptr [<addr>]` where
//!   `<addr>` is one of: a bare register, `register+disp`, `register-disp`,
//!   a numeric absolute, or a symbol (looked up in the table).
//! - Float-literal immediates: `(float)100` → IEEE 754 single-precision
//!   bit pattern; `(double)100` → 64-bit. Required for cheats that overwrite
//!   game state with float constants (CE's canonical
//!   `mov dword ptr [r13+13C], (float)100` pattern).
//!
//! Out of scope for v2.2: anonymous labels (`@@:` / `@f` / `@b`),
//! scale-index SIB addressing (`[reg+reg*4+disp]`), `lea`, `add`/`sub`/`xor`
//! with full operand matrix. Surfaces as `Unsupported` until a real trainer
//! needs them.

mod arith;
mod control_flow;
mod data_move;
mod operands;

use std::collections::HashMap;

use self::arith::{Arith, emit_arith, emit_cmp};
use self::control_flow::{Mnemonic, emit_jcc, emit_ret, emit_unary_target, is_conditional_jump};
use self::data_move::{emit_lea, emit_mov, emit_pop, emit_push};

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
        "cmp" => Ok(Some(emit_cmp(rest, symbols, base_addr)?)),
        "lea" => Ok(Some(emit_lea(rest, symbols, base_addr)?)),
        "add" => Ok(Some(emit_arith(rest, symbols, base_addr, Arith::Add)?)),
        "sub" => Ok(Some(emit_arith(rest, symbols, base_addr, Arith::Sub)?)),
        "xor" => Ok(Some(emit_arith(rest, symbols, base_addr, Arith::Xor)?)),
        m if is_conditional_jump(m) => Ok(Some(emit_jcc(m, rest, symbols, base_addr)?)),
        _ => Ok(None),
    }
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
    fn unknown_mnemonic_returns_none() {
        // `imul` isn't in the Phase B subset — must fall through to None
        // so the executor's compile_raw can surface its own Unsupported.
        let bytes = compile_line("imul rax, rbx, 4", &HashMap::new(), 0).unwrap();
        assert!(bytes.is_none());
    }

    /// Real-world smoke: every asm Raw line of the Ender Magnolia
    /// `unlHarvestFlag` Aurora trainer compiles to bytes with the asm
    /// module — including the `cmp byte ptr [sym], 1`, the float-coerced
    /// store, and the `jne @f` after the parser rewrites `@f` to a real
    /// label. This is the line-level half of "the dialect is good enough
    /// for a real Aurora codecave"; the full Engine::enable round-trip
    /// against a live process is in the executor's ptrace-gated tests.
    #[test]
    fn em_fixture_codecave_body_all_lines_compile() {
        // Parser-resolved equivalent of the fixture's codecave block.
        // `@f` would have been rewritten by `resolve_anonymous_refs` to
        // `code` (the next label in the script). The asm module sees only
        // the post-resolution form.
        let lines = [
            "push ebx",
            "cmp byte ptr [unlHarvestFlag],1",
            "jne code",
            "mov dword ptr [r13+13C],(float)100",
            "pop r13d",
            "jmp return",
            "jmp codecave",
        ];
        let syms = symtab(&[
            ("unlHarvestFlag", 0x1000_0000),
            ("code", 0x2000_0000),
            ("return", 0x3000_0000),
            ("codecave", 0x4000_0000),
        ]);
        for line in lines {
            let bytes = compile_line(line, &syms, 0)
                .unwrap_or_else(|e| panic!("{line:?} should compile: {e}"))
                .unwrap_or_else(|| panic!("{line:?} should be recognised as asm"));
            assert!(
                !bytes.is_empty(),
                "{line:?} produced zero bytes — encoder dropped something"
            );
        }
    }
}
