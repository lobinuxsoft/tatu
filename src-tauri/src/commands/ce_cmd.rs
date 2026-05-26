use ce_launcher::{
    CeInstall, CeStatus, CtTableEntry, ensure_installed, list_tables, open_for_game, status,
};

use crate::steam::detect_game_exe;

#[tauri::command]
pub fn ce_install_status() -> CeStatus {
    status()
}

#[tauri::command]
pub fn ce_install_trigger() -> Result<CeInstall, String> {
    ensure_installed().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ce_list_tables_for_game(app_id: String) -> Result<Vec<CtTableEntry>, String> {
    list_tables(&app_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ce_open_for_game(app_id: String, table_name: String) -> Result<(), String> {
    let exe_name = detect_game_exe(&app_id)?;
    open_for_game(&app_id, &exe_name, &table_name)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
