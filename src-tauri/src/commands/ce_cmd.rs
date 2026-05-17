use ce_launcher::{CeInstall, CeStatus, ensure_installed, status};

#[tauri::command]
pub fn ce_install_status() -> CeStatus {
    status()
}

#[tauri::command]
pub fn ce_install_trigger() -> Result<CeInstall, String> {
    ensure_installed().map_err(|e| e.to_string())
}
