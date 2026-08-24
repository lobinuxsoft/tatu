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
