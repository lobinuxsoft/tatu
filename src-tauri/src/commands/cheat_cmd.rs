use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use cheat_core::db::load_cheat_table;
use cheat_core::freeze::{FreezeKey, FreezeRegistry};
use cheat_core::types::Cheat;
use cheat_core::{is_process_running, trigger_cheat};

#[derive(Debug, Serialize)]
pub struct CheatSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub value_type: &'static str,
    pub action_kind: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CheatStatus {
    pub has_cheats: bool,
    pub process_running: bool,
}

fn cheats_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("backlog-tracker")
        .join("cheats")
}

#[tauri::command]
pub fn cheat_list(app_id: u64) -> Result<Vec<CheatSummary>, String> {
    let table = load_cheat_table(&cheats_dir(), app_id).map_err(|e| e.to_string())?;
    Ok(table.cheats.iter().map(summarize).collect())
}

#[tauri::command]
pub fn cheat_trigger(app_id: u64, cheat_id: String) -> Result<(), String> {
    let table = load_cheat_table(&cheats_dir(), app_id).map_err(|e| e.to_string())?;
    trigger_cheat(&table, &cheat_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cheat_status(app_id: u64) -> CheatStatus {
    match load_cheat_table(&cheats_dir(), app_id) {
        Ok(table) => CheatStatus {
            has_cheats: !table.cheats.is_empty(),
            process_running: is_process_running(&table.exe_pattern),
        },
        Err(_) => CheatStatus {
            has_cheats: false,
            process_running: false,
        },
    }
}

#[tauri::command]
pub fn cheat_freeze_toggle(
    registry: State<'_, FreezeRegistry>,
    app_id: u64,
    cheat_id: String,
    enabled: bool,
) -> Result<bool, String> {
    let key = FreezeKey {
        app_id,
        cheat_id: cheat_id.clone(),
    };

    if !enabled {
        registry.stop(&key);
        return Ok(false);
    }

    let table = load_cheat_table(&cheats_dir(), app_id).map_err(|e| e.to_string())?;
    registry
        .start(&table, &cheat_id)
        .map_err(|e| e.to_string())?;
    Ok(registry.is_active(&key))
}

#[tauri::command]
pub fn cheat_freeze_status(
    registry: State<'_, FreezeRegistry>,
    app_id: u64,
    cheat_id: String,
) -> bool {
    let key = FreezeKey { app_id, cheat_id };

    // Auto-cleanup: if the target process is gone, a previously-active
    // worker has already exited silently but the registry entry lingers.
    // Reflect reality to the frontend instead of reporting a phantom on.
    if let Ok(table) = load_cheat_table(&cheats_dir(), key.app_id)
        && !is_process_running(&table.exe_pattern)
    {
        registry.stop(&key);
        return false;
    }

    registry.is_active(&key)
}

fn summarize(cheat: &Cheat) -> CheatSummary {
    CheatSummary {
        id: cheat.id.clone(),
        name: cheat.name.clone(),
        description: cheat.description.clone(),
        value_type: cheat.action.value().type_name(),
        action_kind: cheat.action.kind_name(),
    }
}
