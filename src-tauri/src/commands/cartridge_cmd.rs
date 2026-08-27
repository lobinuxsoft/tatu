use std::path::{Path, PathBuf};

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::SharedState;
use crate::cartridge::{self, RemovableDrive};

#[tauri::command]
pub async fn list_removable_drives() -> Result<Vec<RemovableDrive>, String> {
    cartridge::list_removable_drives().await
}

#[tauri::command]
pub fn has_cartridge_structure(mount_point: String) -> bool {
    cartridge::has_cartridge_structure(Path::new(&mount_point))
}

/// Every app already recorded on this cartridge — feeds the "Gestionar
/// cartucho" screen, independent of any one game's install flow.
#[tauri::command]
pub fn list_cartridge_apps(mount_point: String) -> Result<Vec<cartridge::CartridgeApp>, String> {
    cartridge::list_apps(Path::new(&mount_point))
}

#[tauri::command]
pub fn is_registered_library(mount_point: String) -> bool {
    cartridge::is_registered_library(Path::new(&mount_point))
}

/// Opens `steam://install/<app_id>` — refused outright if the active
/// account doesn't own the app, so Steam never gets a chance to silently
/// swap the install for a purchase page instead.
#[tauri::command]
pub fn trigger_install(
    app: AppHandle,
    state: State<'_, SharedState>,
    app_id: u64,
) -> Result<(), String> {
    let owned = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.games.iter().any(|g| g.id == app_id)
    };
    if !owned {
        return Err(format!(
            "App {app_id} is not in the active Steam account's owned games — refusing to install"
        ));
    }

    app.opener()
        .open_url(cartridge::install_url(app_id), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Polls the cartridge's `appmanifest_<app_id>.acf` for Steam's own
/// "fully installed" flag, recording the app on the #193 marker (with its
/// already-classified DRM preservability) the moment it flips.
#[tauri::command]
pub fn poll_install_status(
    state: State<'_, SharedState>,
    app_id: u64,
    mount_point: String,
) -> Result<bool, String> {
    let (name, preservability) = {
        let s = state.lock().map_err(|e| e.to_string())?;
        let name = s
            .games
            .iter()
            .find(|g| g.id == app_id)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let preservability = s
            .drm_cache
            .get(&app_id)
            .map(|info| info.preservability.clone())
            .unwrap_or_default();
        (name, preservability)
    };

    cartridge::poll_install_status(app_id, Path::new(&mount_point), &name, preservability)
}

/// Swaps in the vendored Goldberg emulator for an `Easy`-classified app
/// already recorded on the cartridge by #195. Templates ship as Tauri
/// resources (`vendor/goldberg/`) rather than compiled into the binary —
/// see #199.
#[tauri::command]
pub fn inject_goldberg(
    app: AppHandle,
    state: State<'_, SharedState>,
    app_id: u64,
    mount_point: String,
) -> Result<(), String> {
    let preservability = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.drm_cache
            .get(&app_id)
            .map(|info| info.preservability.clone())
            .unwrap_or_default()
    };

    let template_x86 = app
        .path()
        .resolve("vendor/goldberg/x86/steam_api.dll", BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    let template_x64 = app
        .path()
        .resolve(
            "vendor/goldberg/x64/steam_api64.dll",
            BaseDirectory::Resource,
        )
        .map_err(|e| e.to_string())?;

    cartridge::inject_goldberg(
        Path::new(&mount_point),
        app_id,
        preservability,
        &template_x86,
        &template_x64,
    )
}

/// Caches this app's SteamGridDB cover art onto the cartridge (#205). Best
/// effort: the caller treats a failure here as a warning, never a reason to
/// undo an install that already succeeded.
#[tauri::command]
pub async fn fetch_cartridge_art(
    state: State<'_, SharedState>,
    app_id: u64,
    mount_point: String,
) -> Result<(), String> {
    let api_key = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.steamgriddb_api_key.clone()
    };
    cartridge::fetch_cartridge_art(api_key, PathBuf::from(mount_point), app_id).await
}

/// Caches this app's short Steam store description onto the cartridge
/// (#205), for the launcher's info panel (#204). Public endpoint, no API
/// key. Same best-effort treatment as `fetch_cartridge_art`.
#[tauri::command]
pub async fn fetch_cartridge_description(app_id: u64, mount_point: String) -> Result<(), String> {
    cartridge::fetch_cartridge_description(PathBuf::from(mount_point), app_id).await
}

/// Bundles the shared umu-run + Proton + Steam Linux Runtime files onto the
/// cartridge (#206) — only needed once an "Easy" (Goldberg-patched) game
/// exists on it, so this is called right alongside `inject_goldberg`. A
/// no-op copy after the first call for a given cartridge; the real fetch
/// happens at most once per Tatu install, cached outside the cartridge.
#[tauri::command]
pub async fn bundle_linux_runtime(mount_point: String) -> Result<(), String> {
    cartridge::bundle_linux_runtime(PathBuf::from(mount_point)).await
}

/// Copies both the Linux and Windows launcher binaries onto the cartridge
/// root (#204's remaining gap). `spawn_blocking`d: this is ~180MB of copy
/// onto whatever drive is plugged in, and #217 already found a slow/stuck
/// USB stick can stall a sync file op long enough to freeze all of Tatu's
/// IPC if it isn't pushed off the async runtime's worker thread.
#[tauri::command]
pub async fn install_launcher_binaries(app: AppHandle, mount_point: String) -> Result<(), String> {
    let linux_binary = app
        .path()
        .resolve(
            "vendor/launcher/linux/tatu-launcher",
            BaseDirectory::Resource,
        )
        .map_err(|e| e.to_string())?;
    let windows_binary = app
        .path()
        .resolve(
            "vendor/launcher/windows/tatu-launcher.exe",
            BaseDirectory::Resource,
        )
        .map_err(|e| e.to_string())?;

    tokio::task::spawn_blocking(move || {
        cartridge::install_launcher_binaries(
            Path::new(&mount_point),
            &linux_binary,
            &windows_binary,
        )
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
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
