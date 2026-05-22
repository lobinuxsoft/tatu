//! Typed pointer-chain value commands: `cheat_runtime_value_read` /
//! `_write` / `_freeze`. Each command resolves a Value feature's
//! `AddrExpr`, walks its offset chain, then routes the read / write /
//! freeze through the in-prefix `tatu-bridge --connect` over its TCP
//! loopback port (Aurora-style — see [`tatu_proto::BRIDGE_HOST`]).
//! Freezes reach the bridge via a per-tick connect-write-disconnect
//! loop ([`FreezeTarget::Bridge`]).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use cheat_runtime::bridge_client::BridgeClient;
use cheat_runtime::{
    AddrExpr, FeatureKind, FreezeRegistry, FreezeTarget, VType, Value, ValueSpec,
    load_manifests_for, parse_addr_expr,
};
use serde::Deserialize;
use tatu_proto::{Request, Response, WireVType, WireValue};
use tauri::State;

use super::backend::resolve_backend;
use super::{ActiveCheats, merged_symbols};
use crate::state::{AppState, BridgeEntry};

/// Locate a Value feature: returns the owning manifest's `exe`, the
/// resolved [`ValueSpec`], and the parsed [`AddrExpr`] — saving callers
/// from re-parsing on every read/write/freeze invocation.
fn locate_value_feature(app_id: &str, uuid: &str) -> Result<(String, ValueSpec, AddrExpr), String> {
    let manifests = load_manifests_for(app_id).map_err(|e| e.to_string())?;
    for m in manifests {
        for f in m.features {
            if f.uuid != uuid {
                continue;
            }
            return match (f.kind, f.value) {
                (FeatureKind::Value, Some(spec)) => {
                    let expr = parse_addr_expr(&spec.base_expr).map_err(|e| e.to_string())?;
                    Ok((m.exe, spec, expr))
                }
                (FeatureKind::Value, None) => Err(format!(
                    "feature {uuid:?} is a Value but has no value-spec — the manifest is malformed"
                )),
                (other, _) => Err(format!(
                    "feature {uuid:?} is a {other:?}, not a Value — use cheat_runtime_enable / disable"
                )),
            };
        }
    }
    Err(format!("feature {uuid} not found for app {app_id}"))
}

/// Pre-flight: every `[symbol]` reference in the Value's `base_expr` must
/// resolve in the merged symbol table, otherwise the read would fail
/// deeper in `chain::resolve_addr_expr` with the same error.
fn ensure_symbol_registered(expr: &AddrExpr, symbols: &HashMap<String, u64>) -> Result<(), String> {
    if let AddrExpr::SymbolDeref { symbol, .. } = expr
        && !symbols.contains_key(symbol)
    {
        return Err(format!(
            "symbol {symbol:?} not registered — enable the scaffold toggle first"
        ));
    }
    Ok(())
}


#[tauri::command]
pub fn cheat_runtime_value_read(
    app_id: String,
    feature_uuid: String,
    state: State<'_, Mutex<AppState>>,
    active: State<'_, ActiveCheats>,
) -> Result<Value, String> {
    let (_exe, spec, expr) = locate_value_feature(&app_id, &feature_uuid)?;
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;
    let BridgeEntry { wineprefix } = require_bridge(&state, &app_id)?;
    let (base, full_offsets) = bridge_chain(&expr, &spec.offsets, &symbols)?;
    let mut client = bridge_connect(&wineprefix)?;
    let resp = client
        .request(Request::ReadChainValue {
            base,
            offsets: full_offsets,
            vtype: vtype_to_wire(spec.vtype),
        })
        .map_err(|e| format!("bridge ReadChainValue: {e}"))?;
    match resp {
        Response::ChainValue { value } => Ok(wire_to_value(value)),
        Response::Err { message } => Err(format!("bridge: {message}")),
        other => Err(format!("bridge: unexpected response {other:?}")),
    }
}

#[tauri::command]
pub fn cheat_runtime_value_write(
    app_id: String,
    feature_uuid: String,
    value: Value,
    state: State<'_, Mutex<AppState>>,
    active: State<'_, ActiveCheats>,
) -> Result<(), String> {
    let (_exe, spec, expr) = locate_value_feature(&app_id, &feature_uuid)?;
    if value.vtype() != spec.vtype {
        return Err(format!(
            "value vtype {:?} does not match feature vtype {:?}",
            value.vtype(),
            spec.vtype
        ));
    }
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;
    let BridgeEntry { wineprefix } = require_bridge(&state, &app_id)?;
    let (base, full_offsets) = bridge_chain(&expr, &spec.offsets, &symbols)?;
    let mut client = bridge_connect(&wineprefix)?;
    let resp = client
        .request(Request::WriteChainValue {
            base,
            offsets: full_offsets,
            value: value_to_wire(value),
        })
        .map_err(|e| format!("bridge WriteChainValue: {e}"))?;
    match resp {
        Response::ChainWritten => Ok(()),
        Response::Err { message } => Err(format!("bridge: {message}")),
        other => Err(format!("bridge: unexpected response {other:?}")),
    }
}

/// Build the `(base, offsets)` pair the bridge needs to resolve a
/// Value feature's address. The bridge's WalkChain implementation
/// iterates offsets in REVERSE (matching CE's
/// `cheat_runtime::chain::walk_chain`), so we can fold the
/// `SymbolDeref` step into the offsets list:
///
/// Linux flow (`read_chain`):
/// ```text
/// base = read_u64(sym_addr) + sym_offset   // resolve_addr_expr
/// target = walk_chain(base, spec.offsets)  // iterates reverse
/// value = read_at(target, vtype)
/// ```
///
/// Bridge equivalent in one round trip:
/// ```text
/// full_offsets = [...spec.offsets, sym_offset]
/// walk_chain(sym_addr, full_offsets) iterates reverse
///   = [sym_offset, then spec.offsets[N-1], ..., spec.offsets[0]]
///   hop 1: deref sym_addr → ptr; cur = ptr + sym_offset    (= Linux base)
///   hop 2..: walk spec.offsets in reverse                  (= Linux walk_chain)
/// ```
///
/// For `Literal` base, no symbol is involved: `base = addr`,
/// `offsets = spec.offsets`.
fn bridge_chain(
    expr: &AddrExpr,
    spec_offsets: &[u64],
    symbols: &HashMap<String, u64>,
) -> Result<(u64, Vec<u64>), String> {
    match expr {
        AddrExpr::Literal(addr) => Ok((*addr, spec_offsets.to_vec())),
        AddrExpr::SymbolDeref { symbol, offset } => {
            let sym_addr = symbols.get(symbol).copied().ok_or_else(|| {
                format!("symbol {symbol:?} not registered — enable the scaffold first")
            })?;
            let mut full = Vec::with_capacity(spec_offsets.len() + 1);
            full.extend_from_slice(spec_offsets);
            // sym_offset is i64 (apply_offset semantics) but the
            // wire's offsets are u64. Linux's resolve_addr_expr uses
            // wrapping_add/sub; we match by casting through u64
            // (negative offsets wrap, the executor adds them with
            // wrapping_add and gets the same result).
            full.push(*offset as u64);
            Ok((sym_addr, full))
        }
    }
}

fn bridge_connect(wineprefix: &str) -> Result<BridgeClient, String> {
    BridgeClient::connect(Path::new(wineprefix))
        .map_err(|e| format!("dial bridge at {wineprefix}: {e}"))
}

fn vtype_to_wire(v: VType) -> WireVType {
    v.into()
}

fn wire_to_value(w: WireValue) -> Value {
    w.into()
}

fn value_to_wire(v: Value) -> WireValue {
    v.into()
}

#[derive(Debug, Deserialize)]
pub struct ValueFreezeReq {
    pub app_id: String,
    pub feature_uuid: String,
    pub enabled: bool,
    /// Required when `enabled == true`. Ignored otherwise.
    #[serde(default)]
    pub value: Option<Value>,
}

#[tauri::command]
pub fn cheat_runtime_value_freeze(
    req: ValueFreezeReq,
    state: State<'_, Mutex<AppState>>,
    active: State<'_, ActiveCheats>,
    freezes: State<'_, FreezeRegistry>,
) -> Result<(), String> {
    if !req.enabled {
        freezes
            .stop(&freeze_key(&req.app_id, &req.feature_uuid))
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let value = req
        .value
        .ok_or_else(|| "value freeze requires the target value when enabling".to_string())?;
    let (_exe, spec, expr) = locate_value_feature(&req.app_id, &req.feature_uuid)?;
    if value.vtype() != spec.vtype {
        return Err(format!(
            "value vtype {:?} does not match feature vtype {:?}",
            value.vtype(),
            spec.vtype
        ));
    }
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;

    let BridgeEntry { wineprefix } = require_bridge(&state, &req.app_id)?;
    let (base, full_offsets) = bridge_chain(&expr, &spec.offsets, &symbols)?;
    let target = FreezeTarget::Bridge {
        wineprefix: std::path::PathBuf::from(wineprefix),
        base,
        offsets: full_offsets,
        value: value_to_wire(value),
    };
    freezes
        .start(freeze_key(&req.app_id, &req.feature_uuid), target, None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn require_bridge(
    state: &State<'_, Mutex<AppState>>,
    app_id: &str,
) -> Result<BridgeEntry, String> {
    resolve_backend(state, app_id).ok_or_else(|| {
        format!("Tatu is not enabled for appid {app_id} — click 'Enable Tatu' in the cheats panel banner")
    })
}

fn freeze_key(app_id: &str, feature_uuid: &str) -> String {
    format!("value:{app_id}:{feature_uuid}")
}

