//! `cheat_runtime_enable` / `cheat_runtime_disable` — drive the executor
//! against a per-feature AA script, persist the undo log for crash
//! recovery. Native Linux backend only (post-pivot #128): the game is
//! a normal Linux process from the kernel's POV; ptrace +
//! `process_vm_readv/writev` are the engine. Wine-side bridge dropped.

use std::sync::Mutex;

use cheat_runtime::{
    Engine, FeatureKind, FreezeRegistry, PersistedHook, find_pid_by_exe, load_manifests_for,
    parse_script,
};
use tauri::State;

use super::{ActiveCheatEntry, ActiveCheats, purge_stale_cheats};
use crate::state::AppState;

#[tauri::command]
pub fn cheat_runtime_enable(
    app_id: String,
    feature_uuid: String,
    _state: State<'_, Mutex<AppState>>,
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
        "[enable {feature_uuid}] linux runtime, pid {}",
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

    let record = cheat.to_persisted(
        app_id.clone(),
        feature_uuid.clone(),
        exe.clone(),
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
    guard.insert(feature_uuid, ActiveCheatEntry::Linux(cheat));
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
        started_at: None,
        writes: Vec::new(),
        allocs: Vec::new(),
    };
    if let Err(e) = stub.delete() {
        eprintln!("[disable {feature_uuid}] WARNING: failed to delete persisted record: {e}");
    }
    result
}

pub(super) fn locate_feature_script(app_id: &str, uuid: &str) -> Result<(String, String), String> {
    let manifests = load_manifests_for(app_id).map_err(|e| e.to_string())?;
    for m in manifests {
        // features_recursive walks Header → child Toggle subtrees so a UUID
        // buried 5 levels deep (real-world CE table shape, see #133 audit)
        // still resolves to its owning manifest's exe + script.
        let Some(f) = m.features_recursive().find(|f| f.uuid == uuid) else {
            continue;
        };
        return match (f.kind, f.script.as_ref()) {
            (FeatureKind::Header, _) => Err(format!(
                "feature {uuid:?} is a Header (visual-only) — not toggleable"
            )),
            (FeatureKind::Value, _) => Err(format!(
                "feature {uuid:?} is a Value — use cheat_runtime_value_read / write / freeze"
            )),
            (FeatureKind::Toggle, Some(script)) => Ok((m.exe.clone(), script.clone())),
            (FeatureKind::Toggle, None) => Err(format!(
                "feature {uuid:?} is a Toggle but has no script — the manifest is malformed"
            )),
        };
    }
    Err(format!("feature {uuid} not found for app {app_id}"))
}
