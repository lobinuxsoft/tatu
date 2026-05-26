//! Operand parsing: registers, memory expressions, immediates, numerics.

use std::collections::HashMap;

use iced_x86::code_asm::{
    AsmMemoryOperand, AsmRegister8, AsmRegister16, AsmRegister32, AsmRegister64, AsmRegisterXmm,
    byte_ptr, dword_ptr, qword_ptr, registers as r, word_ptr,
};

use super::AsmError;

pub(super) enum TypedReg {
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
pub(super) fn parse_register(name: &str) -> Option<TypedReg> {
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
pub(super) fn promote_to_r64(name: &str) -> Option<AsmRegister64> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemSize {
    Byte,
    Word,
    Dword,
    Qword,
}

/// Parse an SSE/SSE2 XMM register name (`xmm0`..`xmm15`). Returns the
/// iced-x86 typed register constant or `None` when the text isn't a
/// recognised XMM name. Companion to [`parse_register`] (general-purpose
/// integer regs); kept separate because SSE callers know they want XMM
/// specifically and the integer fallback would silently match `r0`-style
/// invalid text.
pub(super) fn parse_xmm_register(name: &str) -> Option<AsmRegisterXmm> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        "xmm0" => r::xmm0,
        "xmm1" => r::xmm1,
        "xmm2" => r::xmm2,
        "xmm3" => r::xmm3,
        "xmm4" => r::xmm4,
        "xmm5" => r::xmm5,
        "xmm6" => r::xmm6,
        "xmm7" => r::xmm7,
        "xmm8" => r::xmm8,
        "xmm9" => r::xmm9,
        "xmm10" => r::xmm10,
        "xmm11" => r::xmm11,
        "xmm12" => r::xmm12,
        "xmm13" => r::xmm13,
        "xmm14" => r::xmm14,
        "xmm15" => r::xmm15,
        _ => return None,
    })
}

/// Parse a CE-AA memory operand like `dword ptr [r13+13C]` or
/// `byte ptr [unlHarvestFlag]`. Returns the size prefix together with an
/// iced-x86 [`AsmMemoryOperand`] that still needs the size applied via
/// [`apply_size`] before it can be passed to `mov`/`cmp`.
///
/// Returns `Ok(None)` when the text is not a memory operand (no `[...]`
/// bracket pair); the caller should fall through to the register / immediate
/// parsers. Returns `Err` when the text *looks* like a memory operand but
/// the body is malformed — bubbling that up as `Unsupported` so the user
/// sees the offending text rather than a silent fall-through.
pub(super) fn try_parse_memory_operand(
    text: &str,
    syms: &HashMap<String, u64>,
) -> Result<Option<(Option<MemSize>, AsmMemoryOperand)>, AsmError> {
    let t = text.trim();
    let (size, body) = match split_size_prefix(t) {
        Some((s, b)) => (Some(s), b),
        None => {
            // Allow the `[expr]` shorthand without an explicit size prefix —
            // CE Auto-Assembler infers the width from the OTHER operand
            // (the register's size in `mov [foo], rax`, or the immediate's
            // explicit width otherwise). Callers receive `None` and must
            // resolve it before encoding.
            if t.starts_with('[') && t.ends_with(']') {
                (None, t)
            } else {
                return Ok(None);
            }
        }
    };
    let body = body.trim();
    if !body.starts_with('[') || !body.ends_with(']') {
        return Err(AsmError::Unsupported(format!(
            "memory operand {text:?}: expected `[...]` body"
        )));
    }
    let inner = &body[1..body.len() - 1];
    let mem = parse_addr_inside_brackets(inner.trim(), syms)?;
    Ok(Some((size, mem)))
}

fn split_size_prefix(text: &str) -> Option<(MemSize, &str)> {
    for (kw, size) in [
        ("byte ptr", MemSize::Byte),
        ("word ptr", MemSize::Word),
        ("dword ptr", MemSize::Dword),
        ("qword ptr", MemSize::Qword),
    ] {
        if let Some(rest) = text.strip_prefix(kw) {
            return Some((size, rest.trim_start()));
        }
    }
    None
}

pub(super) fn parse_addr_inside_brackets(
    inner: &str,
    syms: &HashMap<String, u64>,
) -> Result<AsmMemoryOperand, AsmError> {
    let inner = inner.trim();
    if inner.is_empty() {
        return Err(AsmError::BadOperand(inner.into()));
    }

    // Just a register: `[rax]`.
    if let Some(reg) = parse_register(inner) {
        return reg_into_memop(reg, inner);
    }

    // `register +/- displacement`. Pick the rightmost `+` or `-` so that
    // hex displacements containing letters don't trip the split.
    if let Some(idx) = inner.rfind(['+', '-'])
        && idx > 0
    {
        let (lhs, op_disp) = inner.split_at(idx);
        let (op, disp_text) = op_disp.split_at(1);
        let lhs = lhs.trim();
        let disp_text = disp_text.trim();
        if let Some(reg) = parse_register(lhs) {
            // Inside `[reg+disp]`, CE Auto-Assembler treats **every**
            // unprefixed token as hex — that's the canonical convention
            // shared with MASM/IDA/CE-AA, and the only way `[rax+00000370]`
            // (a vtable offset) decodes to `0x370` instead of decimal 370
            // (= `0x172`, a different vtable slot that crashes the game).
            // `parse_numeric` keeps its decimal fallback for explicit
            // `(int)N` callers; here we prefer hex first.
            let raw = u64::from_str_radix(disp_text, 16)
                .ok()
                .or_else(|| parse_numeric(disp_text))
                .or_else(|| syms.get(disp_text).copied())
                .ok_or_else(|| AsmError::BadOperand(inner.into()))?;
            let disp = if op == "-" {
                -(raw as i64) as i32
            } else {
                raw as i32
            };
            return reg_disp_into_memop(reg, disp, inner);
        }
    }

    // Absolute numeric or symbol → bare displacement. Bare `[N]` is hex by
    // CE-AA convention, matching the `[reg+N]` rule above.
    if let Some(v) = u64::from_str_radix(inner, 16)
        .ok()
        .or_else(|| parse_numeric(inner))
    {
        return Ok(v.into());
    }
    if let Some(&v) = syms.get(inner) {
        return Ok(v.into());
    }
    Err(AsmError::UnknownSymbol(inner.into()))
}

pub(super) fn reg_into_memop(reg: TypedReg, source: &str) -> Result<AsmMemoryOperand, AsmError> {
    match reg {
        TypedReg::R64(r) => Ok(r.into()),
        TypedReg::R32(r) => Ok(r.into()),
        _ => Err(AsmError::Unsupported(format!(
            "address register {source:?}: only r64 / r32 supported in brackets"
        ))),
    }
}

pub(super) fn reg_disp_into_memop(
    reg: TypedReg,
    disp: i32,
    source: &str,
) -> Result<AsmMemoryOperand, AsmError> {
    match reg {
        TypedReg::R64(r) => Ok(r + disp),
        TypedReg::R32(r) => Ok(r + disp),
        _ => Err(AsmError::Unsupported(format!(
            "address register {source:?}: only r64 / r32 supported in brackets"
        ))),
    }
}

pub(super) fn apply_size(size: MemSize, mem: AsmMemoryOperand) -> AsmMemoryOperand {
    match size {
        MemSize::Byte => byte_ptr(mem),
        MemSize::Word => word_ptr(mem),
        MemSize::Dword => dword_ptr(mem),
        MemSize::Qword => qword_ptr(mem),
    }
}

/// Resolve an immediate operand to a 64-bit raw value. Accepts the CE-AA
/// `(float)N` and `(double)N` casts (IEEE 754 bit pattern), numeric
/// literals (`0x...`, `$...`, decimal), and symbol names.
pub(super) fn parse_immediate(text: &str, syms: &HashMap<String, u64>) -> Option<i64> {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("(float)") {
        let f: f32 = rest.trim().parse().ok()?;
        return Some(f.to_bits() as i64);
    }
    if let Some(rest) = t.strip_prefix("(double)") {
        let f: f64 = rest.trim().parse().ok()?;
        return Some(f.to_bits() as i64);
    }
    if let Some(v) = parse_numeric(t) {
        return Some(v as i64);
    }
    syms.get(t).map(|&v| v as i64)
}

pub(super) fn parse_numeric(token: &str) -> Option<u64> {
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

pub(super) fn resolve_numeric_or_symbol(token: &str, syms: &HashMap<String, u64>) -> Option<u64> {
    if let Some(v) = parse_numeric(token) {
        return Some(v);
    }
    syms.get(token.trim()).copied()
}

/// Resolve an operand to an absolute address. Accepted forms:
/// - `0xDEADBEEF`, `$DEADBEEF`, decimal — numeric literal
/// - `symbol` — looks up in the symbol table
/// - `symbol+N` / `symbol-N` — symbol with byte offset (N hex or decimal)
pub(super) fn resolve_target(
    operand: &str,
    symbols: &HashMap<String, u64>,
) -> Result<u64, AsmError> {
    let t = operand.trim();
    if t.is_empty() {
        return Err(AsmError::BadOperand(operand.into()));
    }
    if let Some(addr) = parse_numeric(t) {
        return Ok(addr);
    }
    if let Some(idx) = t.rfind(['+', '-'])
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

pub(super) fn split_two_operands(rest: &str) -> Result<(&str, &str), AsmError> {
    let (lhs, rhs) = rest
        .split_once(',')
        .ok_or_else(|| AsmError::BadOperand(rest.into()))?;
    Ok((lhs.trim(), rhs.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_float_literal_to_bit_pattern() {
        // 100.0f32 = 0x42C80000.
        let imm = parse_immediate("(float)100", &HashMap::new()).unwrap();
        assert_eq!(imm as u32, 0x42C8_0000);
        // 100.0f64 = 0x4059000000000000.
        let imm = parse_immediate("(double)100", &HashMap::new()).unwrap();
        assert_eq!(imm as u64, 0x4059_0000_0000_0000);
    }
}
