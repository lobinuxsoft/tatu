//! Tauri commands that expose the `cheat-runtime` crate to the frontend.
//!
//! The runtime operates on per-game **manifests** living under
//! `$XDG_CONFIG_HOME/backlog-tracker/trainers/<app_id>/`. Each manifest is a
//! self-describing JSON binding each user-facing feature to the CE Auto-
//! Assembler script that implements it (see `cheat_runtime::manifest`).
//!
//! Aurora's raw JSON exports are **not** consumed here yet — the feature ↔
//! script binding is still an open reverse-engineering problem (documented
//! in personal memory). Once solved, an Aurora → manifest converter lands
//! and these commands light up for the captured trainers.

use std::collections::HashMap;
use std::sync::Mutex;

use cheat_runtime::{
    ActiveCheat, AddrExpr, ChainError, Engine, FeatureKind, FreezeRegistry, ManifestFeature,
    PersistedHook, Pid, VType, Value, ValueSpec, find_pid_by_exe, load_all_persisted_hooks,
    load_manifests_for, parse_addr_expr, parse_script, read_chain, resolve_addr_expr, walk_chain,
    write_chain,
};
use serde::{Deserialize, Serialize};
use tauri::State;

/// Tauri-managed registry of currently enabled cheats, keyed by feature UUID.
pub type ActiveCheats = Mutex<HashMap<String, ActiveCheat>>;

#[derive(Debug, Serialize)]
pub struct FeatureView {
    pub manifest_title: String,
    pub manifest_exe: String,
    pub uuid: String,
    pub name: String,
    pub category: Option<String>,
    pub kind: FeatureKind,
    pub active: bool,
    pub game_running: bool,
    /// Present only for `kind == Value`. The frontend uses this to render
    /// a typed input + freeze toggle and to detect whether the value's
    /// symbol dependency is satisfied (see [`master_symbol_for_value`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_spec: Option<ValueSpec>,
    /// Name of the AA symbol this Value entry's `base_expr` dereferences,
    /// if any. Used by the UI to dim Value features whose scaffold cheat
    /// isn't enabled yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_symbol: Option<String>,
    /// `true` if the value's required symbol is currently bound by some
    /// active cheat (or the value has no symbol dep). The UI uses this to
    /// gate the read/write/freeze controls on a per-feature basis.
    pub symbol_ready: bool,
}

#[tauri::command]
pub fn cheat_runtime_list_features(
    app_id: String,
    active: State<'_, ActiveCheats>,
) -> Result<Vec<FeatureView>, String> {
    // Drop registry entries whose PID is gone — the UI must not paint
    // toggles as active when the game has been relaunched out-of-band.
    purge_stale_cheats(&active)?;
    let manifests = load_manifests_for(&app_id).map_err(|e| e.to_string())?;
    let (active_keys, registered_symbols) = {
        let guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        let keys: std::collections::HashSet<String> = guard.keys().cloned().collect();
        let mut syms: std::collections::HashSet<String> = std::collections::HashSet::new();
        for cheat in guard.values() {
            for k in cheat.symbols().keys() {
                syms.insert(k.clone());
            }
        }
        (keys, syms)
    };

    let mut out = Vec::new();
    for m in manifests {
        let game_running = find_pid_by_exe(&m.exe).is_some();
        for f in m.features {
            let required_symbol = master_symbol_for_value(&f);
            let symbol_ready = match &required_symbol {
                Some(name) => registered_symbols.contains(name),
                None => true,
            };
            let value_spec = f.value.clone();
            out.push(FeatureView {
                manifest_title: m.title.clone(),
                manifest_exe: m.exe.clone(),
                active: active_keys.contains(&f.uuid),
                uuid: f.uuid,
                name: f.name,
                category: f.category,
                kind: f.kind,
                value_spec,
                required_symbol,
                symbol_ready,
                game_running,
            });
        }
    }
    Ok(out)
}

/// For a Value feature, return the name of the symbol its `base_expr`
/// dereferences. `None` for literals, Headers, Toggles, and any unparseable
/// expression (the runtime will reject the read at apply time with a more
/// specific error, but the UI can already dim the row).
fn master_symbol_for_value(f: &ManifestFeature) -> Option<String> {
    let spec = f.value.as_ref()?;
    match parse_addr_expr(&spec.base_expr).ok()? {
        AddrExpr::SymbolDeref { symbol, .. } => Some(symbol),
        AddrExpr::Literal(_) => None,
    }
}

/// True if `/proc/<pid>/` still exists — the lightest possible liveness
/// check. Used to detect when the user closed and re-launched the game
/// out-of-band; without this the enable shortcut returns the registry
/// entry from the dead PID and the new game never gets hooked.
fn pid_is_alive(pid: Pid) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid.as_raw())).exists()
}

/// Drop every active cheat whose PID is gone. Their `Drop` impl will try
/// to roll back writes against the dead process; those calls fail silently
/// (ESRCH) so the only effect is freeing the in-memory registry slot.
fn purge_stale_cheats(active: &ActiveCheats) -> Result<(), String> {
    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    let stale: Vec<String> = guard
        .iter()
        .filter(|(_, c)| !pid_is_alive(c.pid()))
        .map(|(k, _)| k.clone())
        .collect();
    for uuid in stale {
        guard.remove(&uuid);
    }
    Ok(())
}

#[tauri::command]
pub fn cheat_runtime_enable(
    app_id: String,
    feature_uuid: String,
    active: State<'_, ActiveCheats>,
) -> Result<(), String> {
    // The user may have closed and re-launched the game out-of-band. Any
    // ActiveCheat still in the registry is bound to a PID that no longer
    // exists, and re-applying would otherwise short-circuit on its
    // presence without ever hooking the new game process.
    purge_stale_cheats(&active)?;
    {
        let guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        if guard.contains_key(&feature_uuid) {
            return Ok(());
        }
    }

    let (exe, script_src) = locate_feature_script(&app_id, &feature_uuid).map_err(|e| {
        eprintln!("[enable {feature_uuid}] locate_feature_script: {e}");
        e
    })?;
    let pid = find_pid_by_exe(&exe).ok_or_else(|| {
        let msg = format!("game process '{exe}' is not running; launch the game first");
        eprintln!("[enable {feature_uuid}] {msg}");
        msg
    })?;
    eprintln!(
        "[enable {feature_uuid}] starting against pid {}",
        pid.as_raw()
    );
    let script = parse_script(&script_src).map_err(|e| {
        let msg = format!("parse: {e}");
        eprintln!("[enable {feature_uuid}] {msg}");
        msg
    })?;
    let mut engine = Engine::new(pid);
    let cheat = engine.enable(&script).map_err(|e| {
        let msg = format!("enable: {e}");
        eprintln!("[enable {feature_uuid}] {msg}");
        msg
    })?;
    eprintln!(
        "[enable {feature_uuid}] success, symbols={:?}",
        cheat.symbols().keys().collect::<Vec<_>>()
    );

    // Persist the undo log so a tracker restart can revert the hook even
    // if we never see the user's matching disable click.
    let record = cheat.to_persisted(
        app_id.clone(),
        feature_uuid.clone(),
        exe.clone(),
        Some(chrono_now_iso8601()),
    );
    if let Err(e) = record.write() {
        // Persistence failure is not fatal — the in-memory ActiveCheat
        // still works for THIS session. Log loudly so the user knows
        // recovery on next launch won't be available.
        eprintln!(
            "[enable {feature_uuid}] WARNING: failed to persist undo log: {e}. The hook still applies, but a tracker restart will leave it orphaned."
        );
    }

    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    guard.insert(feature_uuid, cheat);
    Ok(())
}

/// Minimal ISO-8601 timestamp using only std. Avoids pulling chrono just
/// for this one cosmetic field — recovery never parses it, only the user
/// reads it from the on-disk JSON if they go spelunking.
fn chrono_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

#[tauri::command]
pub fn cheat_runtime_disable(
    app_id: String,
    feature_uuid: String,
    active: State<'_, ActiveCheats>,
) -> Result<(), String> {
    let cheat = {
        let mut guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        guard.remove(&feature_uuid)
    };
    let result = match cheat {
        Some(c) => c.disable().map_err(|e| format!("disable: {e}")),
        None => Ok(()),
    };
    // Whether the disable succeeded or not, the on-disk record is
    // strictly tied to the in-memory ActiveCheat we just removed. Delete
    // it so a tracker restart doesn't try to recover a hook that the
    // user already revoked.
    let stub = PersistedHook {
        app_id,
        feature_uuid: feature_uuid.clone(),
        pid: 0,
        exe: String::new(),
        started_at: None,
        writes: Vec::new(),
        allocs: Vec::new(),
    };
    if let Err(e) = stub.delete() {
        eprintln!("[disable {feature_uuid}] WARNING: failed to delete persisted record: {e}");
    }
    result
}

/// A persisted hook whose recorded PID is still alive — the user can
/// either restore the original bytes or dismiss the record (e.g. they
/// already re-launched the game and the bytes are gone anyway). Records
/// whose PID is dead are deleted silently and don't appear here.
#[derive(Debug, Serialize)]
pub struct OrphanHook {
    pub app_id: String,
    pub feature_uuid: String,
    pub exe: String,
    pub pid: i32,
    /// Best-effort feature display name resolved from the manifest for
    /// the same app_id + uuid. Empty when the manifest is gone (manifest
    /// was deleted while the hook was active).
    pub name: String,
    pub writes: usize,
    pub allocs: usize,
}

/// Scan the persist directory; delete records for dead PIDs; return the
/// rest tagged with their feature name. Called by the frontend at app
/// startup to surface a "restore orphan hooks" banner.
#[tauri::command]
pub fn cheat_runtime_orphans_list() -> Result<Vec<OrphanHook>, String> {
    let mut report = load_all_persisted_hooks().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut name_cache: HashMap<String, HashMap<String, String>> = HashMap::new();
    for record in report.records.drain(..) {
        if !record.pid_alive() {
            // Game already exited — kernel reclaimed memory, nothing to
            // restore. Drop the file silently.
            if let Err(e) = record.delete() {
                eprintln!(
                    "[orphans] couldn't delete record for dead pid {}: {e}",
                    record.pid
                );
            }
            continue;
        }
        let name = name_cache
            .entry(record.app_id.clone())
            .or_insert_with(|| build_name_index(&record.app_id))
            .get(&record.feature_uuid)
            .cloned()
            .unwrap_or_default();
        out.push(OrphanHook {
            app_id: record.app_id.clone(),
            feature_uuid: record.feature_uuid.clone(),
            exe: record.exe.clone(),
            pid: record.pid,
            name,
            writes: record.writes.len(),
            allocs: record.allocs.len(),
        });
    }
    for (path, err) in report.failed {
        eprintln!("[orphans] skip malformed record {}: {err}", path.display());
    }
    Ok(out)
}

/// Restore an orphan hook: load the record, run the rollback via the
/// same atomic POKEDATA path a live disable would have used, delete the
/// on-disk file. Idempotent — calling twice is a no-op for the second
/// call (the file is gone).
#[tauri::command]
pub fn cheat_runtime_orphans_restore(app_id: String, feature_uuid: String) -> Result<(), String> {
    let report = load_all_persisted_hooks().map_err(|e| e.to_string())?;
    let record = report
        .records
        .into_iter()
        .find(|r| r.app_id == app_id && r.feature_uuid == feature_uuid)
        .ok_or_else(|| {
            format!("no persisted hook record for app {app_id} feature {feature_uuid}")
        })?;
    if !record.pid_alive() {
        // Game exited between list and restore — nothing to do, just
        // clean up.
        record.delete().map_err(|e| e.to_string())?;
        return Ok(());
    }
    eprintln!(
        "[orphans] restoring {} writes for pid {} ({})",
        record.writes.len(),
        record.pid,
        record.exe
    );
    let cheat = ActiveCheat::from_persisted(&record);
    cheat
        .disable()
        .map_err(|e| format!("rollback failed: {e}"))?;
    record.delete().map_err(|e| e.to_string())?;
    Ok(())
}

/// Drop an orphan record without rolling it back. The user picks this
/// when they've already re-launched the game (so the bytes are fresh
/// anyway) and just wants the banner to go away.
#[tauri::command]
pub fn cheat_runtime_orphans_dismiss(app_id: String, feature_uuid: String) -> Result<(), String> {
    let stub = PersistedHook {
        app_id,
        feature_uuid,
        pid: 0,
        exe: String::new(),
        started_at: None,
        writes: Vec::new(),
        allocs: Vec::new(),
    };
    stub.delete().map_err(|e| e.to_string())
}

/// Resolve uuid → feature.name for every feature in every manifest of
/// `app_id`. Used by [`cheat_runtime_orphans_list`] to render readable
/// names in the recovery banner.
fn build_name_index(app_id: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(manifests) = load_manifests_for(app_id) {
        for m in manifests {
            for f in m.features {
                out.insert(f.uuid, f.name);
            }
        }
    }
    out
}

fn locate_feature_script(app_id: &str, uuid: &str) -> Result<(String, String), String> {
    let manifests = load_manifests_for(app_id).map_err(|e| e.to_string())?;
    for m in manifests {
        for f in m.features {
            if f.uuid != uuid {
                continue;
            }
            return match (f.kind, f.script) {
                (FeatureKind::Header, _) => Err(format!(
                    "feature {uuid:?} is a Header (visual-only) — not toggleable"
                )),
                (FeatureKind::Value, _) => Err(format!(
                    "feature {uuid:?} is a Value — use cheat_runtime_value_read / write / freeze"
                )),
                (FeatureKind::Toggle, Some(script)) => Ok((m.exe, script)),
                (FeatureKind::Toggle, None) => Err(format!(
                    "feature {uuid:?} is a Toggle but has no script — the manifest is malformed"
                )),
            };
        }
    }
    Err(format!("feature {uuid} not found for app {app_id}"))
}

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

/// Merge the symbol tables of every currently-active cheat into one map.
/// Same-name collisions are resolved last-write-wins — in practice they
/// don't happen because each AA toggle owns its alloc/scan symbols.
fn merged_symbols(active: &ActiveCheats) -> Result<HashMap<String, u64>, String> {
    let guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    let mut merged = HashMap::new();
    for cheat in guard.values() {
        for (k, v) in cheat.symbols() {
            merged.insert(k.clone(), *v);
        }
    }
    Ok(merged)
}

/// Pre-flight: every `[symbol]` reference in the Value's `base_expr` must
/// resolve in the merged symbol table, otherwise the read would fail
/// deeper in `chain::resolve_addr_expr` with the same error.
fn ensure_symbol_registered(expr: &AddrExpr, symbols: &HashMap<String, u64>) -> Result<(), String> {
    if let AddrExpr::SymbolDeref { symbol, .. } = expr {
        if !symbols.contains_key(symbol) {
            return Err(format!(
                "symbol {symbol:?} not registered — enable the scaffold toggle first"
            ));
        }
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
    active: State<'_, ActiveCheats>,
) -> Result<Value, String> {
    let (exe, spec, expr) = locate_value_feature(&app_id, &feature_uuid)?;
    let pid = find_pid_by_exe(&exe)
        .ok_or_else(|| format!("game process '{exe}' is not running; launch the game first"))?;
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;
    read_chain(pid, &expr, &spec.offsets, spec.vtype, &symbols).map_err(map_chain_err)
}

#[tauri::command]
pub fn cheat_runtime_value_write(
    app_id: String,
    feature_uuid: String,
    value: Value,
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
    let pid = find_pid_by_exe(&exe)
        .ok_or_else(|| format!("game process '{exe}' is not running; launch the game first"))?;
    let symbols = merged_symbols(&active)?;
    ensure_symbol_registered(&expr, &symbols)?;
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
    let (pid, addr, _) = find_value_target(&exe, &spec, &expr, &active)?;
    let bytes = value_to_le_bytes(value);
    freezes
        .start(
            freeze_key(&req.app_id, &req.feature_uuid),
            pid,
            addr,
            bytes,
            None,
        )
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
