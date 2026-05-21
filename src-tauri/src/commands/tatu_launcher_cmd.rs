//! Tauri commands exposing the [`crate::tatu_launcher`] module.
//!
//! Pure pass-throughs: the heavy lifting lives in the module; this
//! file only converts the typed errors to `String` so they land in
//! the frontend's `.catch()` cleanly.

use crate::tatu_launcher::{
    self, TatuLauncherStatus, get_compat_tool_for_app, install_compat_tool, set_compat_tool_for_app,
};

#[tauri::command]
pub fn tatu_launcher_status() -> TatuLauncherStatus {
    tatu_launcher::status()
}

#[tauri::command]
pub fn tatu_launcher_install() -> Result<String, String> {
    install_compat_tool()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tatu_launcher_set_for_app(app_id: String) -> Result<(), String> {
    set_compat_tool_for_app(&app_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tatu_launcher_get_for_app(app_id: String) -> Result<Option<String>, String> {
    get_compat_tool_for_app(&app_id).map_err(|e| e.to_string())
}
