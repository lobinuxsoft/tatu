//! Typed pointer-chain value commands: `cheat_runtime_value_read` /
//! `_write` / `_freeze`. Resolve a Value feature's `AddrExpr`, walk
//! its offset chain via `process_vm_readv`, then drive [`cheat_runtime`]'s
//! typed read / write / freeze primitives against the resolved address.
//!
//! Native Linux backend only post-pivot #128 — the wine-side bridge
//! routing was dropped. From the kernel's POV a wine game is just a
//! Linux process; ptrace + process_vm_* hit it directly.

use std::collections::HashMap;
use std::sync::Mutex;

use cheat_runtime::{
    AddrExpr, ChainError, FeatureKind, FreezeRegistry, FreezeTarget, Pid, VType, Value, ValueSpec,
    find_pid_by_exe, load_manifests_for, parse_addr_expr, read_chain, resolve_addr_expr,
    walk_chain, write_chain,
};
use serde::Deserialize;
use tauri::State;

use super::{ActiveCheats, merged_symbols};
use crate::state::AppState;

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

fn find_value_target(
    exe: &str,
    spec: &ValueSpec,
    expr: &AddrExpr,
    active: &ActiveCheats,
) -> Result<(Pid, u64, VType), String> {
    let pid = find_pid_by_exe(exe)
        .ok_or_else(|| format!("game process '{exe}' is not running; launch the game first"))?;
    let symbols = merged_symbols(active)?;
    ensure_symbol_registered(expr, &symbols)?;
    let base = resolve_addr_expr(pid, expr, &symbols).map_err(map_chain_err)?;
    let final_addr = walk_chain(pid, base, &spec.offsets).map_err(map_chain_err)?;
    Ok((pid, final_addr, spec.vtype))
}

fn map_chain_err(e: ChainError) -> String {
    e.to_string()
}

#[tauri::command]
pub fn cheat_runtime_value_read(
    app_id: String,
    feature_uuid: String,
    _state: State<'_, Mutex<AppState>>,
    active: State<'_, ActiveCheats>,
) -> Result<Value, String> {
    let (exe, spec, expr) = locate_value_feature(&app_id, &feature_uuid)?;
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;
    let pid = find_pid_by_exe(&exe)
        .ok_or_else(|| format!("game process '{exe}' is not running; launch the game first"))?;
    read_chain(pid, &expr, &spec.offsets, spec.vtype, &symbols).map_err(map_chain_err)
}

#[tauri::command]
pub fn cheat_runtime_value_write(
    app_id: String,
    feature_uuid: String,
    value: Value,
    _state: State<'_, Mutex<AppState>>,
    active: State<'_, ActiveCheats>,
) -> Result<(), String> {
    let (exe, spec, expr) = locate_value_feature(&app_id, &feature_uuid)?;
    if value.vtype() != spec.vtype {
        return Err(format!(
            "value vtype {:?} does not match feature vtype {:?}",
            value.vtype(),
            spec.vtype
        ));
    }
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;
    let pid = find_pid_by_exe(&exe)
        .ok_or_else(|| format!("game process '{exe}' is not running; launch the game first"))?;
    write_chain(pid, &expr, &spec.offsets, value, &symbols).map_err(map_chain_err)
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
    _state: State<'_, Mutex<AppState>>,
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
    let (exe, spec, expr) = locate_value_feature(&req.app_id, &req.feature_uuid)?;
    if value.vtype() != spec.vtype {
        return Err(format!(
            "value vtype {:?} does not match feature vtype {:?}",
            value.vtype(),
            spec.vtype
        ));
    }
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;

    // Resolve once up front; the freeze loop writes to the resolved
    // leaf address — no need to walk the chain on every tick.
    let (pid, addr, _) = find_value_target(&exe, &spec, &expr, &active)?;
    let target = FreezeTarget::Linux {
        pid,
        address: addr,
        bytes: value_to_le_bytes(value),
    };
    freezes
        .start(freeze_key(&req.app_id, &req.feature_uuid), target, None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn freeze_key(app_id: &str, feature_uuid: &str) -> String {
    format!("value:{app_id}:{feature_uuid}")
}

fn value_to_le_bytes(value: Value) -> Vec<u8> {
    match value {
        Value::U32(v) => v.to_le_bytes().to_vec(),
        Value::I32(v) => v.to_le_bytes().to_vec(),
        Value::U64(v) => v.to_le_bytes().to_vec(),
        Value::I64(v) => v.to_le_bytes().to_vec(),
        Value::F32(v) => v.to_le_bytes().to_vec(),
        Value::F64(v) => v.to_le_bytes().to_vec(),
    }
}
