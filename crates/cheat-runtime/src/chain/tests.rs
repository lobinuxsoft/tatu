//! Pointer-chain resolution tests. Each test builds a Rust-side scaffold
//! (Box / Vec literals) and exercises the runtime against `Pid::this()`,
//! reading through `process_vm_readv` — same code path the live tracker
//! takes against a foreign game PID.

use std::collections::HashMap;

use nix::unistd::Pid;

use crate::manifest::VType;

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
