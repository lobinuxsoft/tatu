use crate::steam;

#[tauri::command]
pub fn detect_steam_id() -> Option<String> {
    steam::detect_steam_id()
}

/// Whether this build ships a working cheat runtime.
///
/// False on Windows until `tatu-win` provides the Win32 memory backend
/// (#181). The frontend hides the whole Cheats tab on false rather than
/// rendering toggles whose commands are not even registered.
#[tauri::command]
pub fn cheats_supported() -> bool {
    cfg!(unix)
}

/// Absolute path of `state.json`, so the UI can show where the user's data
/// (including their API key) is kept without guessing the platform.
#[tauri::command]
pub fn state_path() -> String {
    crate::state::AppState::display_path()
}
