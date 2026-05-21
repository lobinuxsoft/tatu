//! `cheat_runtime_enable` / `cheat_runtime_disable` — drive the executor
//! against a per-feature AA script, persist the undo log for crash
//! recovery, route through the per-game backend (Phase 7B of #106).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use cheat_runtime::bridge_client::BridgeClient;
use cheat_runtime::{
    BackendKind, Engine, FeatureKind, FreezeRegistry, PersistedAlloc, PersistedHook,
    PersistedWrite, find_pid_by_exe, load_manifests_for, parse_script,
};
use tatu_proto::{Request, Response, WireOutcome};
use tauri::State;

use super::backend::resolve_backend;
use super::{ActiveCheatEntry, ActiveCheats, purge_stale_cheats};
use crate::state::{AppState, GameBackend};

#[tauri::command]
pub fn cheat_runtime_enable(
    app_id: String,
    feature_uuid: String,
    state: State<'_, Mutex<AppState>>,
    active: State<'_, ActiveCheats>,
    freezes: State<'_, FreezeRegistry>,
) -> Result<(), String> {
    purge_stale_cheats(&active, Some(&freezes))?;
    {
        let guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        if guard.contains_key(&feature_uuid) {
            return Ok(());
        }
    }

    // Defence in depth: the frontend already dims toggle rows when a
    // prereq is missing, but a stale state.json or a script-driven
    // invocation could route here without the gate. Refusing early
    // also gives the user a more actionable error than "AOB scan
    // failed at 0x140000000" minutes later.
    crate::commands::prereqs_cmd::check_feature_prereqs(&app_id, &feature_uuid).map_err(|e| {
        eprintln!("[enable {feature_uuid}] prereq gate: {e}");
        e
    })?;

    let (exe, script_src) = locate_feature_script(&app_id, &feature_uuid).map_err(|e| {
        eprintln!("[enable {feature_uuid}] locate_feature_script: {e}");
        e
    })?;

    match resolve_backend(&state, &app_id) {
        GameBackend::Linux => enable_linux(&app_id, &feature_uuid, &exe, &script_src, &active),
        GameBackend::Bridge { wineprefix } => enable_bridge(
            &app_id,
            &feature_uuid,
            &exe,
            &script_src,
            &wineprefix,
            &active,
        ),
    }
}

fn enable_linux(
    app_id: &str,
    feature_uuid: &str,
    exe: &str,
    script_src: &str,
    active: &ActiveCheats,
) -> Result<(), String> {
    let pid = find_pid_by_exe(exe).ok_or_else(|| {
        let msg = format!("game process '{exe}' is not running; launch the game first");
        eprintln!("[enable {feature_uuid}] {msg}");
        msg
    })?;
    eprintln!(
        "[enable {feature_uuid}] linux backend, pid {}",
        pid.as_raw()
    );
    let script = parse_script(script_src).map_err(|e| format!("parse: {e}"))?;
    let mut engine = Engine::new(pid);
    let cheat = engine.enable(&script).map_err(|e| format!("enable: {e}"))?;
    eprintln!(
        "[enable {feature_uuid}] linux success, symbols={:?}",
        cheat.symbols().keys().collect::<Vec<_>>()
    );

    let record = cheat.to_persisted(
        app_id.to_string(),
        feature_uuid.to_string(),
        exe.to_string(),
        Some(chrono_now_iso8601()),
    );
    if let Err(e) = record.write() {
        eprintln!(
            "[enable {feature_uuid}] WARNING: failed to persist undo log: {e}. Recovery on next launch will not be available."
        );
    }

    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    guard.insert(feature_uuid.to_string(), ActiveCheatEntry::Linux(cheat));
    Ok(())
}

fn enable_bridge(
    app_id: &str,
    feature_uuid: &str,
    exe: &str,
    script_src: &str,
    wineprefix: &str,
    active: &ActiveCheats,
) -> Result<(), String> {
    eprintln!("[enable {feature_uuid}] bridge backend, wineprefix={wineprefix}");

    let mut client = BridgeClient::connect(Path::new(wineprefix))
        .map_err(|e| format!("dial bridge at {wineprefix}: {e}"))?;
    let resp = client
        .request(Request::EnableScript {
            script_text: script_src.to_string(),
        })
        .map_err(|e| format!("bridge EnableScript: {e}"))?;
    let outcome = match resp {
        Response::EnableScript { outcome } => outcome,
        Response::Err { message } => return Err(format!("bridge: {message}")),
        other => return Err(format!("bridge: unexpected response {other:?}")),
    };
    eprintln!(
        "[enable {feature_uuid}] bridge success: {} writes, {} allocs, {} symbols",
        outcome.undo.len(),
        outcome.allocs.len(),
        outcome.symbols.len()
    );

    let symbols: HashMap<String, u64> = outcome.symbols.iter().cloned().collect();

    // Persist as BackendKind::Bridge so orphan recovery routes through
    // the bridge on next launch (see orphans.rs::restore_via_bridge).
    let record = PersistedHook {
        app_id: app_id.to_string(),
        feature_uuid: feature_uuid.to_string(),
        pid: 0,
        exe: exe.to_string(),
        backend: BackendKind::Bridge,
        wineprefix: Some(wineprefix.to_string()),
        started_at: Some(chrono_now_iso8601()),
        writes: outcome
            .undo
            .iter()
            .map(|(addr, bytes)| PersistedWrite {
                addr: *addr,
                original: bytes.clone(),
            })
            .collect(),
        allocs: outcome
            .allocs
            .iter()
            .map(|(symbol, addr, size)| PersistedAlloc {
                symbol: symbol.clone(),
                addr: *addr,
                size: *size as usize,
            })
            .collect(),
    };
    if let Err(e) = record.write() {
        eprintln!(
            "[enable {feature_uuid}] WARNING: failed to persist bridge hook: {e}. Recovery on next launch will not be available."
        );
    }

    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    guard.insert(
        feature_uuid.to_string(),
        ActiveCheatEntry::Bridge {
            wineprefix: wineprefix.to_string(),
            outcome,
            symbols,
        },
    );
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
    let entry = {
        let mut guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        guard.remove(&feature_uuid)
    };
    let result: Result<(), String> = match entry {
        Some(ActiveCheatEntry::Linux(c)) => c.disable().map_err(|e| format!("disable: {e}")),
        Some(ActiveCheatEntry::Bridge {
            wineprefix,
            outcome,
            ..
        }) => disable_bridge(&wineprefix, outcome),
        None => Ok(()),
    };
    // Whether the disable succeeded or not, the on-disk record is
    // strictly tied to the in-memory entry we just removed. Delete it
    // so a tracker restart doesn't try to recover a hook that the
    // user already revoked.
    let stub = PersistedHook {
        app_id,
        feature_uuid: feature_uuid.clone(),
        pid: 0,
        exe: String::new(),
        backend: BackendKind::default(),
        wineprefix: None,
        started_at: None,
        writes: Vec::new(),
        allocs: Vec::new(),
    };
    if let Err(e) = stub.delete() {
        eprintln!("[disable {feature_uuid}] WARNING: failed to delete persisted record: {e}");
    }
    result
}

fn disable_bridge(wineprefix: &str, outcome: WireOutcome) -> Result<(), String> {
    let mut client = BridgeClient::connect(Path::new(wineprefix))
        .map_err(|e| format!("dial bridge at {wineprefix}: {e}"))?;
    let resp = client
        .request(Request::DisableScript { outcome })
        .map_err(|e| format!("bridge DisableScript: {e}"))?;
    match resp {
        Response::DisableScript => Ok(()),
        Response::Err { message } => Err(format!("bridge: {message}")),
        other => Err(format!("bridge: unexpected response {other:?}")),
    }
}

pub(super) fn locate_feature_script(app_id: &str, uuid: &str) -> Result<(String, String), String> {
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
