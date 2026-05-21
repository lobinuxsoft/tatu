//! CE `<Address>` string parser. Pure (no memory I/O). The I/O
//! variant — resolve `[symbol]` against a `read_u64` from the
//! symbol's address — lives in [`resolve`] and goes through
//! [`crate::MemoryAccess`].
//!
//! Accepted grammar:
//! - `"[symbol]"`, `"[symbol]+1A"`, `"[symbol] - 30"` (whitespace
//!   tolerant; trailing offset is hex, optional `0x` prefix).
//! - `"0x12345678"`, `"12345678"` (treated as hex — CE addresses are
//!   always hex on disk).
//!
//! Anything else returns [`ParseError::Unsupported`] so the caller
//! gets a clear failure mode rather than a silently-wrong resolution.

use std::collections::HashMap;

use crate::{MemoryAccess, read_u64};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddrExpr {
    Literal(u64),
    SymbolDeref { symbol: String, offset: i64 },
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error(
        "address expression {expr:?} is unsupported (only [symbol]+hex and literals are recognised)"
    )]
    Unsupported { expr: String },
    #[error("invalid number {token:?} in address expression")]
    InvalidNumber { token: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError<E: std::error::Error + Send + Sync + 'static> {
    #[error(
        "symbol {symbol:?} is not registered — enable the scaffold cheat that registers it first"
    )]
    UnknownSymbol { symbol: String },
    #[error("memory: {0}")]
    Memory(#[source] E),
}

pub fn parse(input: &str) -> Result<AddrExpr, ParseError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ParseError::Unsupported {
            expr: input.to_string(),
        });
    }
    if let Some(rest) = s.strip_prefix('[') {
        let (symbol, tail) = rest.split_once(']').ok_or_else(|| ParseError::Unsupported {
            expr: input.to_string(),
        })?;
        let symbol = symbol.trim().to_string();
        if symbol.is_empty() {
            return Err(ParseError::Unsupported {
                expr: input.to_string(),
            });
        }
        let offset = parse_trailing_offset(tail).ok_or_else(|| ParseError::Unsupported {
            expr: input.to_string(),
        })?;
        return Ok(AddrExpr::SymbolDeref { symbol, offset });
    }
    parse_hex_u64(s)
        .map(AddrExpr::Literal)
        .ok_or_else(|| ParseError::InvalidNumber {
            token: s.to_string(),
        })
}

/// Resolve [`AddrExpr`] to a concrete remote address. `SymbolDeref`
/// reads 8 bytes from the symbol's address and adds the offset —
/// matching CE's `[name]` token.
pub fn resolve<M: MemoryAccess>(
    mem: &mut M,
    expr: &AddrExpr,
    symbols: &HashMap<String, u64>,
) -> Result<u64, ResolveError<M::Error>> {
    match expr {
        AddrExpr::Literal(addr) => Ok(*addr),
        AddrExpr::SymbolDeref { symbol, offset } => {
            let sym_addr = *symbols
                .get(symbol)
                .ok_or_else(|| ResolveError::UnknownSymbol {
                    symbol: symbol.clone(),
                })?;
            let pointer = read_u64(mem, sym_addr).map_err(ResolveError::Memory)?;
            Ok(apply_offset(pointer, *offset))
        }
    }
}

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

fn apply_offset(base: u64, offset: i64) -> u64 {
    if offset >= 0 {
        base.wrapping_add(offset as u64)
    } else {
        base.wrapping_sub((-offset) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_symbol_deref() {
        let e = parse("[base_address]").unwrap();
        assert_eq!(
            e,
            AddrExpr::SymbolDeref {
                symbol: "base_address".into(),
                offset: 0
            }
        );
    }

    #[test]
    fn parses_symbol_with_positive_offset() {
        let e = parse("[sym] + 1A").unwrap();
        assert_eq!(
            e,
            AddrExpr::SymbolDeref {
                symbol: "sym".into(),
                offset: 0x1A
            }
        );
    }

    #[test]
    fn parses_symbol_with_negative_offset() {
        let e = parse("[sym]-30").unwrap();
        assert_eq!(
            e,
            AddrExpr::SymbolDeref {
                symbol: "sym".into(),
                offset: -0x30
            }
        );
    }

    #[test]
    fn parses_hex_literal_with_prefix() {
        assert_eq!(parse("0xDEADBEEF").unwrap(), AddrExpr::Literal(0xDEAD_BEEF));
    }

    #[test]
    fn parses_bare_hex_literal() {
        assert_eq!(parse("DEADBEEF").unwrap(), AddrExpr::Literal(0xDEAD_BEEF));
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(parse(""), Err(ParseError::Unsupported { .. })));
    }
}
