use std::path::Path;

use crate::cartridge::{self, RemovableDrive};

#[tauri::command]
pub async fn list_removable_drives() -> Result<Vec<RemovableDrive>, String> {
    cartridge::list_removable_drives().await
}

#[tauri::command]
pub fn has_cartridge_structure(mount_point: String) -> bool {
    cartridge::has_cartridge_structure(Path::new(&mount_point))
}

// No verified non-elevated, silent format API on Windows yet (#194) —
// gated off there rather than shipped on a guess.
#[cfg(unix)]
#[tauri::command]
pub async fn format_as_cartridge(
    device: String,
    expected_label: String,
    expected_bytes: u64,
) -> Result<(), String> {
    cartridge::format_as_cartridge(&device, &expected_label, expected_bytes).await
}
