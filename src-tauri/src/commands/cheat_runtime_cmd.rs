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

use cheat_runtime::{ActiveCheat, Engine, find_pid_by_exe, load_manifests_for, parse_script};
use serde::Serialize;
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
    pub active: bool,
    pub game_running: bool,
}

#[tauri::command]
pub fn cheat_runtime_list_features(
    app_id: String,
    active: State<'_, ActiveCheats>,
) -> Result<Vec<FeatureView>, String> {
    let manifests = load_manifests_for(&app_id).map_err(|e| e.to_string())?;
    let active_keys: std::collections::HashSet<String> = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?
        .keys()
        .cloned()
        .collect();

    let mut out = Vec::new();
    for m in manifests {
        let game_running = find_pid_by_exe(&m.exe).is_some();
        for f in m.features {
            out.push(FeatureView {
                manifest_title: m.title.clone(),
                manifest_exe: m.exe.clone(),
                active: active_keys.contains(&f.uuid),
                uuid: f.uuid,
                name: f.name,
                category: f.category,
                game_running,
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn cheat_runtime_enable(
    app_id: String,
    feature_uuid: String,
    active: State<'_, ActiveCheats>,
) -> Result<(), String> {
    {
        let guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        if guard.contains_key(&feature_uuid) {
            return Ok(());
        }
    }

    let (exe, script_src) = locate_feature_script(&app_id, &feature_uuid)?;
    let pid = find_pid_by_exe(&exe)
        .ok_or_else(|| format!("game process '{exe}' is not running; launch the game first"))?;
    let script = parse_script(&script_src).map_err(|e| format!("parse: {e}"))?;
    let mut engine = Engine::new(pid);
    let cheat = engine.enable(&script).map_err(|e| format!("enable: {e}"))?;

    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    guard.insert(feature_uuid, cheat);
    Ok(())
}

#[tauri::command]
pub fn cheat_runtime_disable(
    feature_uuid: String,
    active: State<'_, ActiveCheats>,
) -> Result<(), String> {
    let cheat = {
        let mut guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        guard.remove(&feature_uuid)
    };
    match cheat {
        Some(c) => c.disable().map_err(|e| format!("disable: {e}")),
        None => Ok(()),
    }
}

fn locate_feature_script(app_id: &str, uuid: &str) -> Result<(String, String), String> {
    let manifests = load_manifests_for(app_id).map_err(|e| e.to_string())?;
    for m in manifests {
        for f in m.features {
            if f.uuid == uuid {
                return Ok((m.exe, f.script));
            }
        }
    }
    Err(format!("feature {uuid} not found for app {app_id}"))
}
