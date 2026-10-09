//! Resolve the main `.exe` of a Steam-installed game by appid.
//!
//! Strategy:
//! 1. If the user wrote an override at `<cheat-tables>/<app_id>/.detected-exe`, use it.
//! 2. Parse `libraryfolders.vdf` → enumerate every Steam library on the system.
//! 3. For each library, look for `steamapps/appmanifest_<app_id>.acf` and parse its
//!    `installdir` field. First match wins.
//! 4. Enumerate `.exe` files under `<library>/steamapps/common/<installdir>/`
//!    up to depth 2. Filter out obvious non-game executables (crash handlers,
//!    redistributables, uninstallers).
//! 5. If exactly one `.exe` remains, take it. Otherwise take the largest.
//! 6. Cache the chosen name so subsequent launches are instant.

use std::fs;
use std::path::PathBuf;

use crate::steam::exe_pick::pick_main_exe_in;
use crate::steam::install::find_install_path;

const OVERRIDE_FILENAME: &str = ".detected-exe";
const TABLES_SUBDIR: &str = "backlog-tracker/cheat-tables";

pub fn detect_game_exe(app_id: &str) -> Result<String, String> {
    if let Some(exe) = read_override(app_id) {
        return Ok(exe);
    }
    let install_path = find_install_path(app_id)?;
    let chosen = pick_main_exe_in(&install_path, app_id.parse().unwrap_or(0))?;
    let _ = cache_detection(app_id, &chosen);
    Ok(chosen)
}

fn override_path(app_id: &str) -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join(TABLES_SUBDIR)
            .join(app_id)
            .join(OVERRIDE_FILENAME),
    )
}

fn read_override(app_id: &str) -> Option<String> {
    let path = override_path(app_id)?;
    let raw = fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn cache_detection(app_id: &str, exe_name: &str) -> std::io::Result<()> {
    let Some(path) = override_path(app_id) else {
        return Ok(());
    };
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, exe_name)
}
