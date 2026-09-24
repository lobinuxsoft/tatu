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
mod cmov;
mod control_flow;
mod data_move;
mod misc;
mod operands;
mod reassemble;
mod sse;
mod x87;

pub use self::reassemble::reassemble_instruction;

use std::collections::HashMap;

use self::arith::{Arith, emit_arith, emit_cmp, emit_test};
use self::cmov::{emit_cmov, is_cmov};
use self::control_flow::{Mnemonic, emit_jcc, emit_ret, emit_unary_target, is_conditional_jump};
use self::data_move::{emit_lea, emit_mov, emit_pop, emit_push};
use self::misc::dispatch_misc_mnemonic;
use self::sse::dispatch_sse_mnemonic;
use self::x87::dispatch_x87_mnemonic;

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

/// Resolve a CE-AA address expression (`symbol`, `symbol+N`, `symbol-N`,
/// `0xADDR`, `$ADDR`, decimal) to an absolute address against `symbols`.
/// Public so the executor can resolve `reassemble()` operands without
/// duplicating the operand grammar.
pub fn resolve_address(operand: &str, symbols: &HashMap<String, u64>) -> Result<u64, AsmError> {
    self::operands::resolve_target(operand, symbols)
}

/// Compile a single CE-AA asm line to bytes. Returns `Ok(None)` if the line
/// is not asm (caller should keep falling through to its own `db`/`dq`/`nop`
/// handlers).
///
/// Wraps [`compile_dispatch`] with a RIP-relative fallback: in long mode a
/// bare absolute memory operand (`[symbol]` / `[0xADDR]`) above ±2 GiB can't
/// be encoded as `[disp32]`, so on that specific iced failure we retry the
/// line with the operand rewritten RIP-relative (see [`rip_relative_retry`]).
pub fn compile_line(
    line: &str,
    symbols: &HashMap<String, u64>,
    base_addr: u64,
) -> Result<Option<Vec<u8>>, AsmError> {
    match compile_dispatch(line, symbols, base_addr) {
        Err(e @ AsmError::IcedX86(_)) => match rip_relative_retry(line, symbols, base_addr)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Err(e),
        },
        other => other,
    }
}

/// Span of the first `[...]` bracket pair in `line`, as `(open, close)` byte
/// indices. `None` when the line has no bracketed memory operand.
fn bracket_span(line: &str) -> Option<(usize, usize)> {
    let open = line.find('[')?;
    let close = line[open..].find(']')? + open;
    Some((open, close))
}

/// Retry a line whose only obstacle is a far bare-absolute memory operand by
/// emitting it RIP-relative. Returns `None` (propagate the original error)
/// when the line has no such operand or the address fits a 32-bit
/// displacement after all.
fn rip_relative_retry(
    line: &str,
    symbols: &HashMap<String, u64>,
    base_addr: u64,
) -> Result<Option<Vec<u8>>, AsmError> {
    let Some((open, close)) = bracket_span(line) else {
        return Ok(None);
    };
    let Some(target) = operands::resolve_bare_absolute(&line[open + 1..close], symbols) else {
        return Ok(None);
    };
    if operands::fits_i32(target) {
        return Ok(None);
    }
    // Compile with a small placeholder so iced emits the absolute `[disp32]`
    // form, then rewrite that operand to `[rip+disp32]` aimed at `target`.
    let placeholder = format!("{}[0x1000]{}", &line[..open], &line[close + 1..]);
    let Some(bytes) = compile_dispatch(&placeholder, symbols, base_addr)? else {
        return Ok(None);
    };
    let rip = reassemble::retarget_abs_to_rip(&bytes, base_addr, target)?;
    Ok(Some(rip))
}

/// Dispatch a CE-AA asm line to the per-mnemonic encoder. Returns `Ok(None)`
/// when the line is not recognised asm.
fn compile_dispatch(
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
        "test" => Ok(Some(emit_test(rest, symbols, base_addr)?)),
        "lea" => Ok(Some(emit_lea(rest, symbols, base_addr)?)),
        "add" => Ok(Some(emit_arith(rest, symbols, base_addr, Arith::Add)?)),
        "sub" => Ok(Some(emit_arith(rest, symbols, base_addr, Arith::Sub)?)),
        "xor" => Ok(Some(emit_arith(rest, symbols, base_addr, Arith::Xor)?)),
        // Bare `nop` — single 0x90 byte. `nop N` (multi-byte padding) is
        // handled by the length estimator before we reach asm, but the
        // compile path also needs the trivial single-byte form for raw
        // asm lines in label bodies. `length.rs` already returns 1 for
        // the same token.
        "nop" if rest.is_empty() => Ok(Some(vec![0x90])),
        m if is_conditional_jump(m) => Ok(Some(emit_jcc(m, rest, symbols, base_addr)?)),
        m if is_cmov(m) => Ok(Some(emit_cmov(m, rest, symbols, base_addr)?)),
        // Three Tier-2/3 dispatch helpers — each returns Ok(None) when the
        // mnemonic isn't in its set, so we chain them with `?` short-circuit
        // ordering: SSE first (highest frequency post-Tier-2), then x87,
        // then misc (logical, shifts, inc/dec, 0-arg, movzx/movsxd, imul).
        m => {
            if let Some(bytes) = dispatch_sse_mnemonic(m, rest, symbols, base_addr)? {
                return Ok(Some(bytes));
            }
            if let Some(bytes) = dispatch_x87_mnemonic(m, rest, symbols, base_addr)? {
                return Ok(Some(bytes));
            }
            dispatch_misc_mnemonic(m, rest, symbols, base_addr)
        }
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
    fn far_absolute_memory_operand_becomes_rip_relative() {
        // A codecave constant above the 4 GiB line can't be a `[disp32]`
        // absolute; it must encode RIP-relative (DD2 Fatal Fall Height's
        // `movss xmm0,[HeightDieForHumanWorkOverride]`).
        let base = 0x1_43cb_0000_u64;
        let target = 0x1_43cb_1000_u64;
        let syms = symtab(&[("Override", target)]);
        let bytes = compile_line("movss xmm0,[Override]", &syms, base)
            .unwrap()
            .unwrap();
        // F3 0F 10 05 <disp32> = movss xmm0,[rip+disp32], 8 bytes.
        assert_eq!(&bytes[..4], &[0xF3, 0x0F, 0x10, 0x05], "rip-relative movss");
        assert_eq!(bytes.len(), 8);
        let disp = i32::from_le_bytes(bytes[4..8].try_into().unwrap()) as i64;
        let resolved = (base as i64 + bytes.len() as i64 + disp) as u64;
        assert_eq!(resolved, target, "must resolve back to the symbol");
    }

    #[test]
    fn near_absolute_memory_operand_stays_disp32() {
        // An address that fits a 32-bit displacement keeps the compact
        // SIB-absolute form — the RIP fallback must not fire.
        let syms = symtab(&[("low", 0x1000)]);
        let bytes = compile_line("movss xmm0,[low]", &syms, 0x4000)
            .unwrap()
            .unwrap();
        // F3 0F 10 04 25 <disp32> = SIB absolute, 9 bytes.
        assert_eq!(&bytes[..5], &[0xF3, 0x0F, 0x10, 0x04, 0x25]);
    }

    #[test]
    fn unknown_mnemonic_returns_none() {
        // Truly fake mnemonic — must fall through to None so the
        // executor's compile_raw can surface its own Unsupported.
        // (Real mnemonics keep being added; pinning a specific
        // unsupported one here causes false breakage. `fizzbuzz` is
        // safely never going to be an Intel mnemonic.)
        let bytes = compile_line("fizzbuzz rax, rbx", &HashMap::new(), 0).unwrap();
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
