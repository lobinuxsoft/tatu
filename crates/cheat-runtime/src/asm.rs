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

use std::collections::HashMap;

use iced_x86::code_asm::{
    AsmMemoryOperand, AsmRegister8, AsmRegister16, AsmRegister32, AsmRegister64, CodeAssembler,
    byte_ptr, dword_ptr, qword_ptr, registers as r, word_ptr,
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
        "cmp" => Ok(Some(emit_cmp(rest, symbols, base_addr)?)),
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
    let (dst_text, src_text) = split_two_operands(rest)?;
    let mut a = CodeAssembler::new(64)?;

    // `mov <mem>, <...>` — memory destination.
    if let Some((size, mem)) = try_parse_memory_operand(dst_text, syms)? {
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
            } else if let Some((size, mem)) = try_parse_memory_operand(src_text, syms)? {
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
            } else if let Some((size, mem)) = try_parse_memory_operand(src_text, syms)? {
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

fn emit_mov_into_mem(
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

fn emit_cmp(rest: &str, syms: &HashMap<String, u64>, base: u64) -> Result<Vec<u8>, AsmError> {
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

fn split_two_operands(rest: &str) -> Result<(&str, &str), AsmError> {
    let (lhs, rhs) = rest
        .split_once(',')
        .ok_or_else(|| AsmError::BadOperand(rest.into()))?;
    Ok((lhs.trim(), rhs.trim()))
}

// ---- memory operand parsing --------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemSize {
    Byte,
    Word,
    Dword,
    Qword,
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
fn try_parse_memory_operand(
    text: &str,
    syms: &HashMap<String, u64>,
) -> Result<Option<(MemSize, AsmMemoryOperand)>, AsmError> {
    let t = text.trim();
    let (size, body) = match split_size_prefix(t) {
        Some((s, b)) => (s, b),
        None => {
            // Allow the `[expr]` shorthand without an explicit size prefix.
            if t.starts_with('[') && t.ends_with(']') {
                (MemSize::Dword, t)
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

fn parse_addr_inside_brackets(
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
    if let Some(idx) = inner.rfind(|c| c == '+' || c == '-')
        && idx > 0
    {
        let (lhs, op_disp) = inner.split_at(idx);
        let (op, disp_text) = op_disp.split_at(1);
        let lhs = lhs.trim();
        let disp_text = disp_text.trim();
        if let Some(reg) = parse_register(lhs) {
            // Inside `[reg+disp]`, CE Auto-Assembler treats unprefixed
            // tokens as hex (`[r13+13C]` means displacement `0x13C`).
            // `parse_numeric` already accepts `0x` / `$` / decimal; we add
            // the hex-by-default fallback as the third try, after the
            // symbol table.
            let raw = parse_numeric(disp_text)
                .or_else(|| syms.get(disp_text).copied())
                .or_else(|| u64::from_str_radix(disp_text, 16).ok())
                .ok_or_else(|| AsmError::BadOperand(inner.into()))?;
            let disp = if op == "-" {
                -(raw as i64) as i32
            } else {
                raw as i32
            };
            return reg_disp_into_memop(reg, disp, inner);
        }
    }

    // Absolute numeric or symbol → bare displacement.
    if let Some(v) = parse_numeric(inner) {
        return Ok(v.into());
    }
    if let Some(&v) = syms.get(inner) {
        return Ok(v.into());
    }
    Err(AsmError::UnknownSymbol(inner.into()))
}

fn reg_into_memop(reg: TypedReg, source: &str) -> Result<AsmMemoryOperand, AsmError> {
    match reg {
        TypedReg::R64(r) => Ok(r.into()),
        TypedReg::R32(r) => Ok(r.into()),
        _ => Err(AsmError::Unsupported(format!(
            "address register {source:?}: only r64 / r32 supported in brackets"
        ))),
    }
}

fn reg_disp_into_memop(
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

fn apply_size(size: MemSize, mem: AsmMemoryOperand) -> AsmMemoryOperand {
    match size {
        MemSize::Byte => byte_ptr(mem),
        MemSize::Word => word_ptr(mem),
        MemSize::Dword => dword_ptr(mem),
        MemSize::Qword => qword_ptr(mem),
    }
}

// ---- immediate parsing with float coercion -----------------------------

/// Resolve an immediate operand to a 64-bit raw value. Accepts the CE-AA
/// `(float)N` and `(double)N` casts (IEEE 754 bit pattern), numeric
/// literals (`0x...`, `$...`, decimal), and symbol names.
fn parse_immediate(text: &str, syms: &HashMap<String, u64>) -> Option<i64> {
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
        // `lea` is still Phase B v2.3 territory — must fall through to None
        // so the executor's compile_raw can surface its own Unsupported.
        let bytes = compile_line("lea rax, [rbx+8]", &HashMap::new(), 0).unwrap();
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

    // -- Phase B v2.2: memory operands + float literals ----------------

    #[test]
    fn parse_float_literal_to_bit_pattern() {
        // 100.0f32 = 0x42C80000.
        let imm = parse_immediate("(float)100", &HashMap::new()).unwrap();
        assert_eq!(imm as u32, 0x42C8_0000);
        // 100.0f64 = 0x4059000000000000.
        let imm = parse_immediate("(double)100", &HashMap::new()).unwrap();
        assert_eq!(imm as u64, 0x4059_0000_0000_0000);
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
    fn memory_operand_minus_displacement() {
        // mov dword ptr [r13-8], 0
        let bytes = compile_line("mov dword ptr [r13-8], 0", &HashMap::new(), 0)
            .unwrap()
            .unwrap();
        // Last byte of disp8 = -8 = 0xF8.
        assert!(bytes.iter().any(|&b| b == 0xF8));
    }

    #[test]
    fn malformed_memory_operand_errors() {
        let err = compile_line("mov dword ptr r13+13C, 0", &HashMap::new(), 0).unwrap_err();
        // Without brackets, the parser sees a non-register dest and rejects
        // it as an unsupported source.
        assert!(matches!(err, AsmError::Unsupported(_)));
    }
}
