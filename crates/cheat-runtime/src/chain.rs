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
mod tests {
    use super::*;

    fn empty_syms() -> HashMap<String, u64> {
        HashMap::new()
    }

    #[test]
    fn parses_bare_symbol_deref() {
        let e = parse_addr_expr("[base_address]").unwrap();
        assert_eq!(
            e,
            AddrExpr::SymbolDeref {
                symbol: "base_address".into(),
                offset: 0,
            }
        );
    }

    #[test]
    fn parses_symbol_with_positive_offset() {
        let e = parse_addr_expr("[base_address]+30").unwrap();
        assert_eq!(
            e,
            AddrExpr::SymbolDeref {
                symbol: "base_address".into(),
                offset: 0x30,
            }
        );
    }

    #[test]
    fn parses_symbol_with_negative_offset_and_whitespace() {
        let e = parse_addr_expr("  [ shop ] -4B0 ").unwrap();
        assert_eq!(
            e,
            AddrExpr::SymbolDeref {
                symbol: "shop".into(),
                offset: -0x4B0,
            }
        );
    }

    #[test]
    fn parses_literal_with_and_without_prefix() {
        assert_eq!(
            parse_addr_expr("0x12345").unwrap(),
            AddrExpr::Literal(0x12345)
        );
        assert_eq!(
            parse_addr_expr("DEADBEEF").unwrap(),
            AddrExpr::Literal(0xDEADBEEF)
        );
    }

    #[test]
    fn rejects_unterminated_bracket_and_empty_input() {
        assert!(matches!(
            parse_addr_expr("[oops"),
            Err(ChainError::UnsupportedAddrExpr { .. })
        ));
        assert!(matches!(
            parse_addr_expr(""),
            Err(ChainError::UnsupportedAddrExpr { .. })
        ));
    }

    #[test]
    fn rejects_unsupported_expression_shapes() {
        // CE supports these; we don't yet. Should error, not misparse.
        assert!(matches!(
            parse_addr_expr("[base_address]*2"),
            Err(ChainError::UnsupportedAddrExpr { .. })
        ));
        assert!(matches!(
            parse_addr_expr("[[double_deref]]"),
            Err(ChainError::UnsupportedAddrExpr { .. })
        ));
    }

    #[test]
    fn literal_resolves_unchanged() {
        let addr =
            resolve_addr_expr(Pid::this(), &AddrExpr::Literal(0xCAFEF00D), &empty_syms()).unwrap();
        assert_eq!(addr, 0xCAFEF00D);
    }

    #[test]
    fn symbol_deref_against_unknown_errors_clearly() {
        let expr = AddrExpr::SymbolDeref {
            symbol: "ghost".into(),
            offset: 0,
        };
        let err = resolve_addr_expr(Pid::this(), &expr, &empty_syms()).unwrap_err();
        assert!(matches!(err, ChainError::UnknownSymbol { ref symbol } if symbol == "ghost"));
    }

    /// Build a fake "scaffold": a Vec<u64> in our own address space that
    /// holds the pointer to dereference. The symbol table points at the
    /// slot; resolve_addr_expr reads from it via process_vm_readv on
    /// `Pid::this()`. Mirrors what the master AA toggle does for real:
    /// `alloc(base_address, 8)` + `mov [base_address], rax` — except the
    /// alloc is just a Rust Vec instead of a remote mmap.
    #[test]
    fn symbol_deref_reads_through_pointer_then_applies_offset() {
        let target: Vec<u64> = vec![0xAAAA_BBBB_CCCC_DDDD; 4];
        let slot: Box<u64> = Box::new(target.as_ptr() as u64);
        let mut syms = HashMap::new();
        syms.insert("base_address".into(), &*slot as *const u64 as u64);

        let expr = AddrExpr::SymbolDeref {
            symbol: "base_address".into(),
            offset: 8, // skip one Vec entry
        };
        let resolved = resolve_addr_expr(Pid::this(), &expr, &syms).unwrap();
        assert_eq!(resolved, target.as_ptr() as u64 + 8);
    }

    #[test]
    fn walk_chain_empty_returns_base() {
        let addr = walk_chain(Pid::this(), 0x1000, &[]).unwrap();
        assert_eq!(addr, 0x1000);
    }

    /// Walker reproduces CE's reverse-iteration semantics: the *last* offset
    /// in the document-order list is the first one applied during the walk.
    ///
    /// Layout: two leaves, a slots array pointing at each, an outer Box
    /// pointing at the slots array. Two walks with the same address-base
    /// and the offsets swapped end-for-end produce DIFFERENT final values —
    /// proving that order matters and our direction matches CE's.
    #[test]
    fn walk_chain_dereferences_offsets_in_reverse() {
        let leaf_a: Box<u32> = Box::new(0xAAAA_AAAA);
        let leaf_b: Box<u32> = Box::new(0xBBBB_BBBB);
        let slots: Box<[u64; 2]> =
            Box::new([&*leaf_a as *const u32 as u64, &*leaf_b as *const u32 as u64]);
        // The "base address" stored in a separate pointer slot — equivalent
        // to CE's `alloc(base_address, 8)` + `mov [base_address], rax`.
        let outer: Box<u64> = Box::new(slots.as_ptr() as u64);
        let base = &*outer as *const u64 as u64;

        // Doc order: offsets[0]=0 (innermost), offsets[1]=8 (outermost).
        // CE applies offsets[1] first → +8 = slot[1] = &leaf_b.
        // Then offsets[0] = +0 → &leaf_b.
        let final_b = walk_chain(Pid::this(), base, &[0, 8]).unwrap();
        assert_eq!(final_b, &*leaf_b as *const u32 as u64);

        // Same chain with the offsets swapped — now offsets[1]=0 applies
        // first, landing at slot[0] = &leaf_a.
        let final_a = walk_chain(Pid::this(), base, &[8, 0]).unwrap();
        // After deref of slot[0] = &leaf_a (4 bytes), +8 lands well past
        // the leaf — what matters is that it differs from `final_b`,
        // proving the offset-order is observed.
        assert_ne!(final_a, final_b);
    }

    /// Sanity: same buffers, two-step chain reads the actual leaf bytes
    /// when offsets are wired up correctly (one layer of indirection plus
    /// an explicit pointer offset).
    #[test]
    fn walk_chain_two_step_reaches_real_leaf_bytes() {
        let leaf: Box<u32> = Box::new(0xCAFE_F00D);
        let mid: Box<u64> = Box::new(&*leaf as *const u32 as u64);
        let outer: Box<u64> = Box::new(&*mid as *const u64 as u64);
        let base = &*outer as *const u64 as u64;

        let addr = walk_chain(Pid::this(), base, &[0, 0]).unwrap();
        assert_eq!(addr, &*leaf as *const u32 as u64);
        let v = read_value(Pid::this(), addr, VType::U32).unwrap();
        assert_eq!(v, Value::U32(0xCAFE_F00D));
    }

    #[test]
    fn read_value_dispatches_on_vtype() {
        let buf: Vec<u8> = vec![0x39, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // u64 = 1337
        let addr = buf.as_ptr() as u64;
        assert_eq!(
            read_value(Pid::this(), addr, VType::U32).unwrap(),
            Value::U32(1337)
        );
        assert_eq!(
            read_value(Pid::this(), addr, VType::U64).unwrap(),
            Value::U64(1337)
        );
        assert_eq!(
            read_value(Pid::this(), addr, VType::I32).unwrap(),
            Value::I32(1337)
        );
    }

    #[test]
    fn write_value_round_trips() {
        let mut buf = [0u8; 8];
        let addr = buf.as_mut_ptr() as u64;
        write_value(Pid::this(), addr, Value::U32(0xCAFE_BABE)).unwrap();
        assert_eq!(&buf[..4], &0xCAFE_BABEu32.to_le_bytes());
    }

    #[test]
    fn read_write_float_round_trips() {
        let mut buf = [0u8; 4];
        let addr = buf.as_mut_ptr() as u64;
        write_value(Pid::this(), addr, Value::F32(3.5)).unwrap();
        let back = read_value(Pid::this(), addr, VType::F32).unwrap();
        assert_eq!(back, Value::F32(3.5));
    }

    #[test]
    fn value_wire_format_is_self_describing_json() {
        let v = Value::I32(-42);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#"{"vtype":"i32","value":-42}"#);
        let back: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }
}
