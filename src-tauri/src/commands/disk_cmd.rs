use tauri::State;

use crate::SharedState;
use crate::{disk, steam};

/// Scan Steam's local caches for disk sizes and merge into size_cache.
///
/// Two sources are consulted in order:
///   1. `appcache/appinfo.vdf` — covers every owned app (including not
///      currently installed) via depot manifest sizes. Upper-bound estimate.
///   2. `steamapps/libraryfolders.vdf` — exact SizeOnDisk for apps installed
///      right now. Overwrites any appinfo estimate for those apps.
///
/// Returns every entry in size_cache that corresponds to an app in the
/// tracker's library, sorted by app_id.
#[tauri::command]
pub fn scan_sizes(state: State<'_, SharedState>) -> Result<Vec<disk::DiskSize>, String> {
    let steam_dir = steam::steam_install_dir().ok_or("Steam install directory not found")?;

    // Best-effort: appinfo may fail parsing on very new formats, but we still
    // want libraryfolders results if so.
    let appinfo_entries = disk::scan_appinfo_sizes(&steam_dir).unwrap_or_default();
    let installed_entries = disk::scan_installed_sizes(&steam_dir)?;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    let known_ids: std::collections::HashSet<u64> = s.games.iter().map(|g| g.id).collect();

    // First pass: appinfo (lower priority).
    for entry in appinfo_entries {
        s.size_cache.insert(entry.app_id, entry);
    }
    // Second pass: libraryfolders (higher priority, exact on-disk size).
    for entry in installed_entries {
        s.size_cache.insert(entry.app_id, entry);
    }

    let mut result: Vec<disk::DiskSize> = s
        .size_cache
        .values()
        .filter(|e| known_ids.contains(&e.app_id))
        .cloned()
        .collect();
    result.sort_by_key(|e| e.app_id);
    s.save();
    Ok(result)
}
