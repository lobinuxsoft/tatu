use std::path::PathBuf;

use serde::Serialize;

use cheat_core::db::load_cheat_table;
use cheat_core::types::{Cheat, CheatAction, CheatValue};
use cheat_core::{is_process_running, trigger_cheat};

#[derive(Debug, Serialize)]
pub struct CheatSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub value_type: &'static str,
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

fn summarize(cheat: &Cheat) -> CheatSummary {
    CheatSummary {
        id: cheat.id.clone(),
        name: cheat.name.clone(),
        description: cheat.description.clone(),
        value_type: action_value_type(&cheat.action),
    }
}

fn action_value_type(action: &CheatAction) -> &'static str {
    match action {
        CheatAction::WriteOnce { value } => value_type_name(value),
    }
}

fn value_type_name(value: &CheatValue) -> &'static str {
    match value {
        CheatValue::U8(_) => "u8",
        CheatValue::U16(_) => "u16",
        CheatValue::U32(_) => "u32",
        CheatValue::U64(_) => "u64",
        CheatValue::I8(_) => "i8",
        CheatValue::I16(_) => "i16",
        CheatValue::I32(_) => "i32",
        CheatValue::I64(_) => "i64",
        CheatValue::F32(_) => "f32",
        CheatValue::F64(_) => "f64",
    }
}
