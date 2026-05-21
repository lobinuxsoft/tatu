//! Pointer-chain resolution + typed value read/write — Linux ptrace
//! front-end on top of [`tatu_mem::chain`].
//!
//! The pure algorithm (walking offsets in REVERSE per CE convention,
//! decoding LE bytes by [`VType`]) lives in `tatu-mem`. This module
//! adapts that to `cheat-runtime`'s established surface: functions
//! that take a [`Pid`], a tagged-JSON [`Value`] enum that the tracker
//! has been wire-compatible with since day one, and the `Pid`-bound
//! [`AddrExpr`] resolver.
//!
//! ## CE's algorithm, restated
//!
//! Given a value entry with `<Address>"[base_address]+30"</Address>` and
//! `<Offsets>{ 13C, 8B8, 2D0 }</Offsets>`:
//!
//! ```text
//! sym_addr = symbol_table["base_address"]              ; 1. lookup
//! base     = read_u64(sym_addr) + 0x30                 ; 2. deref [name]
//! ptr      = base
//! for o in offsets.iter().rev() {                       ; 3. walk in REVERSE
//!     ptr = read_u64(ptr) + o
//! }
//! value    = read_<vtype>(ptr)                          ; 4. final read
//! ```

use std::collections::HashMap;

use nix::unistd::Pid;
use tatu_mem::addr_expr;
use tatu_mem::chain as mem_chain;
use tatu_proto::{WireVType, WireValue};

use crate::manifest::VType;
use crate::memory::RuntimeError;
use crate::memory_access::ProcessVmMem;

/// `<Address>` expression node — CE's `[symbol]+hex` / hex literal
/// grammar. Identical shape to [`tatu_mem::addr_expr::AddrExpr`], kept
/// as its own type so the public API surface here (and `ChainError`'s
/// variants) stay stable across Phase 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddrExpr {
    Literal(u64),
    SymbolDeref { symbol: String, offset: i64 },
}

impl From<addr_expr::AddrExpr> for AddrExpr {
    fn from(value: addr_expr::AddrExpr) -> Self {
        match value {
            addr_expr::AddrExpr::Literal(a) => AddrExpr::Literal(a),
            addr_expr::AddrExpr::SymbolDeref { symbol, offset } => {
                AddrExpr::SymbolDeref { symbol, offset }
            }
        }
    }
}

impl From<AddrExpr> for addr_expr::AddrExpr {
    fn from(value: AddrExpr) -> Self {
        match value {
            AddrExpr::Literal(a) => addr_expr::AddrExpr::Literal(a),
            AddrExpr::SymbolDeref { symbol, offset } => {
                addr_expr::AddrExpr::SymbolDeref { symbol, offset }
            }
        }
    }
}

/// Type-tagged numeric value — wire format between the runtime and
/// the UI. Tagged JSON (`{"vtype":"u32","value":42}`) so the Tauri
/// command layer stays self-describing. Convert to/from
/// [`tatu_proto::WireValue`] (bincode-friendly, untagged on the wire)
/// via [`From`] when crossing the bridge IPC.
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
}

impl From<VType> for WireVType {
    fn from(value: VType) -> Self {
        match value {
            VType::U32 => WireVType::U32,
            VType::I32 => WireVType::I32,
            VType::U64 => WireVType::U64,
            VType::I64 => WireVType::I64,
            VType::F32 => WireVType::F32,
            VType::F64 => WireVType::F64,
        }
    }
}

impl From<WireVType> for VType {
    fn from(value: WireVType) -> Self {
        match value {
            WireVType::U32 => VType::U32,
            WireVType::I32 => VType::I32,
            WireVType::U64 => VType::U64,
            WireVType::I64 => VType::I64,
            WireVType::F32 => VType::F32,
            WireVType::F64 => VType::F64,
        }
    }
}

impl From<WireValue> for Value {
    fn from(value: WireValue) -> Self {
        match value {
            WireValue::U32(v) => Value::U32(v),
            WireValue::I32(v) => Value::I32(v),
            WireValue::U64(v) => Value::U64(v),
            WireValue::I64(v) => Value::I64(v),
            WireValue::F32(v) => Value::F32(v),
            WireValue::F64(v) => Value::F64(v),
        }
    }
}

impl From<Value> for WireValue {
    fn from(value: Value) -> Self {
        match value {
            Value::U32(v) => WireValue::U32(v),
            Value::I32(v) => WireValue::I32(v),
            Value::U64(v) => WireValue::U64(v),
            Value::I64(v) => WireValue::I64(v),
            Value::F32(v) => WireValue::F32(v),
            Value::F64(v) => WireValue::F64(v),
        }
    }
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
    #[error("decoded {len} bytes at {addr:#x} are not a valid {vtype:?}")]
    DecodeValue {
        addr: u64,
        len: usize,
        vtype: VType,
    },
}

/// Parse a CE `<Address>` string into an [`AddrExpr`]. Wraps
/// [`tatu_mem::addr_expr::parse`]; errors are remapped to
/// [`ChainError`]'s variants so the existing public surface stays
/// stable.
pub fn parse_addr_expr(input: &str) -> Result<AddrExpr, ChainError> {
    match addr_expr::parse(input) {
        Ok(e) => Ok(e.into()),
        Err(addr_expr::ParseError::Unsupported { expr }) => {
            Err(ChainError::UnsupportedAddrExpr { expr })
        }
        Err(addr_expr::ParseError::InvalidNumber { token }) => {
            Err(ChainError::InvalidNumber { token })
        }
    }
}

/// Resolve an [`AddrExpr`] to a concrete remote address. `SymbolDeref`
/// reads 8 bytes from the symbol's address — that matches CE's
/// `[name]` token (`symbolhandler.pas:5789`).
pub fn resolve_addr_expr(
    pid: Pid,
    expr: &AddrExpr,
    symbols: &HashMap<String, u64>,
) -> Result<u64, ChainError> {
    let mut mem = ProcessVmMem::new(pid);
    addr_expr::resolve(&mut mem, &expr.clone().into(), symbols).map_err(|e| match e {
        addr_expr::ResolveError::UnknownSymbol { symbol } => ChainError::UnknownSymbol { symbol },
        addr_expr::ResolveError::Memory(rt) => ChainError::Memory(rt),
    })
}

/// Walk an offset chain from `base`. Iterates `offsets` in reverse —
/// see the module-level doc. For `offsets = []`, returns `base`
/// unchanged.
pub fn walk_chain(pid: Pid, base: u64, offsets: &[u64]) -> Result<u64, ChainError> {
    let mut mem = ProcessVmMem::new(pid);
    mem_chain::walk_chain(&mut mem, base, offsets).map_err(map_chain_err)
}

pub fn read_value(pid: Pid, addr: u64, vtype: VType) -> Result<Value, ChainError> {
    let mut mem = ProcessVmMem::new(pid);
    mem_chain::read_value(&mut mem, addr, vtype.into())
        .map(Value::from)
        .map_err(map_chain_err)
}

pub fn write_value(pid: Pid, addr: u64, value: Value) -> Result<(), ChainError> {
    let mut mem = ProcessVmMem::new(pid);
    mem_chain::write_value(&mut mem, addr, value.into()).map_err(map_chain_err)
}

/// Evaluate the address expression, walk the chain, read the value.
/// Mirrors CE's full read path for a [`crate::manifest::FeatureKind::Value`].
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

/// Counterpart to [`read_chain`].
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

fn map_chain_err(e: mem_chain::ChainError<RuntimeError>) -> ChainError {
    match e {
        mem_chain::ChainError::Memory(rt) => ChainError::Memory(rt),
        mem_chain::ChainError::Decode { addr, len, vtype } => ChainError::DecodeValue {
            addr,
            len,
            vtype: vtype.into(),
        },
    }
}

#[cfg(test)]
mod tests;
