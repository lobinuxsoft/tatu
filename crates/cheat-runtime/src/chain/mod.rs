//! Pointer-chain resolution and value read/write — the Rust port of CE's
//! `TMemoryRecord.GetRealAddress` (`MemoryRecordUnit.pas:3664`) plus the
//! subset of `symbolhandler.pas:getAddressFromName` we need to evaluate the
//! `<Address>` strings emitted by `.CT` files.
//!
//! ## CE's algorithm, restated
//!
//! Given a value entry with `<Address>"[base_address]+30"</Address>` and
//! `<Offsets>{ 13C, 8B8, 2D0 }</Offsets>`:
//!
//! ```text
//! sym_addr = symbol_table["base_address"]              ; 1. lookup
//! base     = read_u64(sym_addr) + 0x30                 ; 2. deref the [name]
//! ptr      = base
//! for o in offsets.iter().rev() {                       ; 3. walk in REVERSE —
//!     ptr = read_u64(ptr) + o                           ;    offsets[len-1]
//! }                                                     ;    first
//! value    = read_<vtype>(ptr)                          ; 4. final read
//! ```
//!
//! Offsets[0] gets ADDED to the last deref'd pointer but is NOT itself
//! followed by another read — that matches the `for i := offsetCount-1
//! downto 0` loop in CE that does one `read_u64` per iteration, plus the
//! final value read by [`crate::memory::read_bytes`].

use std::collections::HashMap;

use nix::unistd::Pid;

use crate::manifest::VType;
use crate::memory::{self, RuntimeError};

/// One node of an `<Address>` expression. CE's expression grammar is rich
/// (nested `[[...]]`, arithmetic on tokens, module-relative `"Game.exe"+1A`,
/// etc.), but every `.CT` we've seen in the wild — including the user's
/// EM v11 table — only uses two shapes:
///
/// - `[symbol_name]` (with optional `+ hex` / `- hex` literal offset)
/// - a bare numeric literal (hex with `0x` prefix or implicit, decimal
///   otherwise)
///
/// We accept those exactly. Anything else returns
/// [`ChainError::UnsupportedAddrExpr`] so users get a clear error instead
/// of a silently-wrong resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddrExpr {
    /// Raw address literal (no deref required).
    Literal(u64),
    /// `[symbol]` + `offset`. Resolving requires one `read_u64` from the
    /// symbol's address.
    SymbolDeref { symbol: String, offset: i64 },
}

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error(
        "address expression {expr:?} is unsupported (only [symbol]+hex and literals are recognised)"
    )]
    UnsupportedAddrExpr { expr: String },
    #[error(
        "symbol {symbol:?} is not registered — enable the scaffold cheat that registers it first"
    )]
    UnknownSymbol { symbol: String },
    #[error("invalid number {token:?} in address expression")]
    InvalidNumber { token: String },
    #[error("memory access: {0}")]
    Memory(#[from] RuntimeError),
}

/// Parse a CE `<Address>` string into an [`AddrExpr`].
///
/// Accepts:
/// - `"[symbol]"`, `"[symbol]+1A"`, `"[symbol] - 30"` (whitespace tolerant)
/// - `"0x12345678"`, `"12345678"` (treated as hex since CE addresses are
///   always hex on disk), `"1234"` followed by no other chars and no `0x`
///   prefix also parses as hex — matches CE behaviour.
pub fn parse_addr_expr(input: &str) -> Result<AddrExpr, ChainError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ChainError::UnsupportedAddrExpr {
            expr: input.to_string(),
        });
    }

    if let Some(rest) = s.strip_prefix('[') {
        let (symbol, tail) =
            rest.split_once(']')
                .ok_or_else(|| ChainError::UnsupportedAddrExpr {
                    expr: input.to_string(),
                })?;
        let symbol = symbol.trim().to_string();
        if symbol.is_empty() {
            return Err(ChainError::UnsupportedAddrExpr {
                expr: input.to_string(),
            });
        }
        let offset =
            parse_trailing_offset(tail).ok_or_else(|| ChainError::UnsupportedAddrExpr {
                expr: input.to_string(),
            })?;
        return Ok(AddrExpr::SymbolDeref { symbol, offset });
    }

    parse_hex_u64(s)
        .map(AddrExpr::Literal)
        .ok_or_else(|| ChainError::InvalidNumber {
            token: s.to_string(),
        })
}

/// Parse the optional `+hex` or `-hex` trailing the closing bracket.
/// Returns `Some(0)` when the trail is empty (bare `[sym]` deref).
fn parse_trailing_offset(tail: &str) -> Option<i64> {
    let tail = tail.trim();
    if tail.is_empty() {
        return Some(0);
    }
    let (sign, rest) = match tail.as_bytes()[0] {
        b'+' => (1i64, tail[1..].trim_start()),
        b'-' => (-1i64, tail[1..].trim_start()),
        _ => return None,
    };
    let magnitude = parse_hex_u64(rest)? as i64;
    Some(sign * magnitude)
}

/// Accept `"0x1A2B"`, `"1A2B"` (treated as hex — CE convention) or `"0X.."`.
/// Rejects empty / non-hex strings.
fn parse_hex_u64(s: &str) -> Option<u64> {
    let body = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if body.is_empty() {
        return None;
    }
    u64::from_str_radix(body, 16).ok()
}

/// Resolve an [`AddrExpr`] to a concrete remote address.
///
/// `SymbolDeref` does one `read_u64` from the symbol's address — that
/// matches CE's `[name]` token in `symbolhandler.pas:5789`, which calls
/// `readprocessmemory` with `processhandler.pointersize` (8 on x86_64).
pub fn resolve_addr_expr(
    pid: Pid,
    expr: &AddrExpr,
    symbols: &HashMap<String, u64>,
) -> Result<u64, ChainError> {
    match expr {
        AddrExpr::Literal(addr) => Ok(*addr),
        AddrExpr::SymbolDeref { symbol, offset } => {
            let sym_addr = *symbols
                .get(symbol)
                .ok_or_else(|| ChainError::UnknownSymbol {
                    symbol: symbol.clone(),
                })?;
            let bytes = memory::read_bytes(pid, sym_addr, 8)?;
            let pointer = u64::from_le_bytes(bytes.as_slice().try_into().unwrap());
            Ok(apply_offset(pointer, *offset))
        }
    }
}

fn apply_offset(base: u64, offset: i64) -> u64 {
    if offset >= 0 {
        base.wrapping_add(offset as u64)
    } else {
        base.wrapping_sub((-offset) as u64)
    }
}

/// Walk an offset chain from `base`, returning the final address that
/// contains the value to read. Iterates `offsets` in reverse — see the
/// module-level doc for why.
///
/// For `offsets = []`, returns `base` unchanged (a value entry whose
/// `<Address>` already pinpoints the target — e.g. `[base_address]` on its
/// own).
pub fn walk_chain(pid: Pid, base: u64, offsets: &[u64]) -> Result<u64, ChainError> {
    let mut cur = base;
    for &offset in offsets.iter().rev() {
        let bytes = memory::read_bytes(pid, cur, 8)?;
        let pointer = u64::from_le_bytes(bytes.as_slice().try_into().unwrap());
        cur = pointer.wrapping_add(offset);
    }
    Ok(cur)
}

/// Type-tagged numeric value — wire format between the runtime and the UI.
///
/// One variant per [`VType`], with `serde` `tag = "vtype"` so the JSON form
/// stays self-describing for the Tauri command layer:
/// `{"vtype":"u32","value":42}`.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "vtype", content = "value", rename_all = "lowercase")]
pub enum Value {
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl Value {
    pub const fn vtype(&self) -> VType {
        match self {
            Value::U32(_) => VType::U32,
            Value::I32(_) => VType::I32,
            Value::U64(_) => VType::U64,
            Value::I64(_) => VType::I64,
            Value::F32(_) => VType::F32,
            Value::F64(_) => VType::F64,
        }
    }

    fn to_le_bytes(self) -> Vec<u8> {
        match self {
            Value::U32(v) => v.to_le_bytes().to_vec(),
            Value::I32(v) => v.to_le_bytes().to_vec(),
            Value::U64(v) => v.to_le_bytes().to_vec(),
            Value::I64(v) => v.to_le_bytes().to_vec(),
            Value::F32(v) => v.to_le_bytes().to_vec(),
            Value::F64(v) => v.to_le_bytes().to_vec(),
        }
    }
}

/// Read a typed value from `addr`, dispatching on [`VType::size_bytes`].
pub fn read_value(pid: Pid, addr: u64, vtype: VType) -> Result<Value, ChainError> {
    let raw = memory::read_bytes(pid, addr, vtype.size_bytes())?;
    Ok(match vtype {
        VType::U32 => Value::U32(u32::from_le_bytes(raw.as_slice().try_into().unwrap())),
        VType::I32 => Value::I32(i32::from_le_bytes(raw.as_slice().try_into().unwrap())),
        VType::U64 => Value::U64(u64::from_le_bytes(raw.as_slice().try_into().unwrap())),
        VType::I64 => Value::I64(i64::from_le_bytes(raw.as_slice().try_into().unwrap())),
        VType::F32 => Value::F32(f32::from_le_bytes(raw.as_slice().try_into().unwrap())),
        VType::F64 => Value::F64(f64::from_le_bytes(raw.as_slice().try_into().unwrap())),
    })
}

/// Write `value` to `addr`. The serialised width comes from `value.vtype()`.
pub fn write_value(pid: Pid, addr: u64, value: Value) -> Result<(), ChainError> {
    memory::write_bytes(pid, addr, &value.to_le_bytes())?;
    Ok(())
}

/// End-to-end helper: evaluate the address expression, walk the chain,
/// read the value. Mirrors CE's full read path for a [`crate::manifest::FeatureKind::Value`].
pub fn read_chain(
    pid: Pid,
    base_expr: &AddrExpr,
    offsets: &[u64],
    vtype: VType,
    symbols: &HashMap<String, u64>,
) -> Result<Value, ChainError> {
    let base = resolve_addr_expr(pid, base_expr, symbols)?;
    let target = walk_chain(pid, base, offsets)?;
    read_value(pid, target, vtype)
}

/// Counterpart to [`read_chain`] — resolve + walk, then write `value`.
pub fn write_chain(
    pid: Pid,
    base_expr: &AddrExpr,
    offsets: &[u64],
    value: Value,
    symbols: &HashMap<String, u64>,
) -> Result<(), ChainError> {
    let base = resolve_addr_expr(pid, base_expr, symbols)?;
    let target = walk_chain(pid, base, offsets)?;
    write_value(pid, target, value)
}

#[cfg(test)]
mod tests;
