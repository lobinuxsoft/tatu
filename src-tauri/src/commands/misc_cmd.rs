use crate::steam;

#[tauri::command]
pub fn detect_steam_id() -> Option<String> {
    steam::detect_steam_id()
}
