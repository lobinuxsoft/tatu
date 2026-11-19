use std::fs;
use std::path::Path;

use super::install::{appmanifest_path, uninstall_from_cartridge};
use super::marker::{AppSource, CartridgeApp, list_apps, remove_apps};

/// Deletes one game's files from the cartridge and drops its marker entry.
/// Files go first: if the delete fails the marker still lists the game, so
/// the UI never shows a game whose bytes are half gone as already removed.
pub fn remove_from_cartridge(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let app = list_apps(mount_point)?
        .into_iter()
        .find(|a| a.app_id == app_id)
        .ok_or_else(|| format!("App {app_id} is not on this cartridge"))?;

    match app.source {
        AppSource::Steam => remove_steam(mount_point, app_id)?,
        AppSource::Gog => remove_folder(mount_point, "GOG", &app)?,
        AppSource::NonSteam => remove_folder(mount_point, "NON-STEAM", &app)?,
        AppSource::Egs => remove_folder(mount_point, "EGS", &app)?,
    }
    remove_assets(mount_point, app_id)?;
    remove_apps(mount_point, &[app_id])
}

/// Art, description, screenshots and trailer live in `assets/<app_id>/`.
fn remove_assets(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let dir = mount_point.join("assets").join(app_id.to_string());
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Cannot remove {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// A Steam install that never finished has no manifest yet; its entry still
/// has to be removable, so a missing manifest skips the file delete.
fn remove_steam(mount_point: &Path, app_id: u64) -> Result<(), String> {
    if appmanifest_path(mount_point, app_id).is_file() {
        uninstall_from_cartridge(mount_point, app_id)?;
    }
    let steamapps = mount_point.join("steamapps");
    for subdir in ["compatdata", "shadercache", "downloading"] {
        let dir = steamapps.join(subdir).join(app_id.to_string());
        if dir.is_dir() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("Cannot remove {}: {e}", dir.display()))?;
        }
    }
    Ok(())
}

/// `exe_path` is `<top>/<install_dir>/...`; the install dir is its second
/// component. It comes from a file on a removable drive, so anything that
/// could climb out of `<top>/` is refused instead of deleted.
fn remove_folder(mount_point: &Path, top: &str, app: &CartridgeApp) -> Result<(), String> {
    let install_dir = app
        .exe_path
        .split('/')
        .nth(1)
        .filter(|d| !d.is_empty() && *d != "." && *d != ".." && !d.contains('\\'))
        .ok_or_else(|| format!("\"{}\" has no valid install folder recorded", app.name))?;
    let dir = mount_point.join(top).join(install_dir);
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Cannot remove {}: {e}", dir.display()))?;
    }
    Ok(())
}
