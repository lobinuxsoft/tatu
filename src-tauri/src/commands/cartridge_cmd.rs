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
    let path = Path::new(&mount_point);
    // Reconciles the marker against Steam's own manifests first (#244) — a
    // game installed directly through Steam, rather than this app's own
    // per-game flow, never touched the marker at all otherwise.
    cartridge::sync_marker_with_installed_apps(path)?;
    cartridge::list_apps(path)
}

#[tauri::command]
pub fn is_registered_library(mount_point: String) -> bool {
    cartridge::is_registered_library(Path::new(&mount_point))
}

/// Disk usage breakdown (#228) for the Cartucho tab's bar chart — total/free
/// space plus the launcher (combined) and each installed game's own total.
/// `spawn_blocking`d: summing every file under `steamapps/common/` walks a
/// real directory tree, same I/O-on-a-slow-drive concern `scan_sizes`
/// upstream already has for the main tracker.
#[tauri::command]
pub async fn get_cartridge_usage(mount_point: String) -> Result<cartridge::CartridgeUsage, String> {
    tokio::task::spawn_blocking(move || cartridge::usage(Path::new(&mount_point)))
        .await
        .map_err(|e| format!("Task error: {e}"))?
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

/// Finds a currently-connected cartridge already tracking `app_id`'s
/// install (in progress or finished) — lets the UI resume watching it
/// straight away instead of making the user click back through the drive
/// picker if the modal (or Tatu itself) was closed mid-install.
#[tauri::command]
pub async fn find_pending_cartridge(app_id: u64) -> Result<Option<String>, String> {
    cartridge::find_pending_cartridge(app_id).await
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

/// Re-classifies every app already on this cartridge and injects Goldberg
/// for any that newly resolve to Easy — part of "Preparar launcher" (#238),
/// covering the whole cartridge automatically instead of needing a manual
/// per-game trigger. Also refreshes the main library's own DRM cache for
/// each app touched, so "Desconocido" in the Steam tab gets fixed too, not
/// just the cartridge marker.
#[tauri::command]
pub fn refresh_cartridge_drm(
    app: AppHandle,
    state: State<'_, SharedState>,
    mount_point: String,
) -> Result<Vec<cartridge::PrepareDrmResult>, String> {
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

    let results =
        cartridge::refresh_drm_and_inject(Path::new(&mount_point), &template_x86, &template_x64)?;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    for result in &results {
        if let Some(info) = &result.drm_info {
            s.drm_cache.insert(result.app_id, info.clone());
        }
    }
    s.save();

    Ok(results)
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

/// Same as `fetch_cartridge_art`, for a GOG app — no Steam appid to key
/// SteamGridDB off, so `title` (the name already known from `gog_library`)
/// drives a title search instead. See `cartridge::fetch_gog_cartridge_art`.
#[tauri::command]
pub async fn fetch_gog_cartridge_art(
    state: State<'_, SharedState>,
    app_id: u64,
    title: String,
    mount_point: String,
) -> Result<(), String> {
    let api_key = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.steamgriddb_api_key.clone()
    };
    cartridge::fetch_gog_cartridge_art(api_key, PathBuf::from(mount_point), app_id, title).await
}

/// Caches this app's short Steam store description onto the cartridge
/// (#205), for the launcher's info panel (#204). Public endpoint, no API
/// key. Same best-effort treatment as `fetch_cartridge_art`.
#[tauri::command]
pub async fn fetch_cartridge_description(app_id: u64, mount_point: String) -> Result<(), String> {
    cartridge::fetch_cartridge_description(PathBuf::from(mount_point), app_id).await
}

/// Same as `fetch_cartridge_description`, for a GOG app matched to a Steam
/// listing by title. See `cartridge::fetch_gog_cartridge_description`.
#[tauri::command]
pub async fn fetch_gog_cartridge_description(
    app_id: u64,
    title: String,
    mount_point: String,
) -> Result<(), String> {
    cartridge::fetch_gog_cartridge_description(PathBuf::from(mount_point), app_id, title).await
}

/// Caches this app's Steam store screenshots onto the cartridge (#213's
/// Tatu-side half — the launcher's gallery UI already shipped in #211,
/// with nothing to actually show until this exists).
#[tauri::command]
pub async fn fetch_cartridge_screenshots(app_id: u64, mount_point: String) -> Result<(), String> {
    cartridge::fetch_cartridge_screenshots(PathBuf::from(mount_point), app_id).await
}

/// Same as `fetch_cartridge_screenshots`, for a GOG app matched to a Steam
/// listing by title. See `cartridge::fetch_gog_cartridge_screenshots`.
#[tauri::command]
pub async fn fetch_gog_cartridge_screenshots(
    app_id: u64,
    title: String,
    mount_point: String,
) -> Result<(), String> {
    cartridge::fetch_gog_cartridge_screenshots(PathBuf::from(mount_point), app_id, title).await
}

/// Caches this app's Steam trailer, transcoded to `.ogv`, onto the
/// cartridge (#212). Opt-in — only called when the Cartucho tab's
/// "Preparar launcher" step has the trailer toggle checked, unlike art and
/// description above which always run.
#[tauri::command]
pub async fn fetch_cartridge_trailer(app_id: u64, mount_point: String) -> Result<(), String> {
    cartridge::fetch_cartridge_trailer(PathBuf::from(mount_point), app_id).await
}

/// Same as `fetch_cartridge_trailer`, for a GOG app matched to a Steam
/// listing by title. See `cartridge::fetch_gog_cartridge_trailer`.
#[tauri::command]
pub async fn fetch_gog_cartridge_trailer(
    app_id: u64,
    title: String,
    mount_point: String,
) -> Result<(), String> {
    cartridge::fetch_gog_cartridge_trailer(PathBuf::from(mount_point), app_id, title).await
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

/// Mounts an already-formatted cartridge that shows up unmounted — a drive
/// reconnected after its first format, or one the desktop's own automounter
/// never picked up. Windows drives are always mounted with a letter, so
/// this has no meaning there (same gate as `format_as_cartridge`).
#[cfg(unix)]
#[tauri::command]
pub async fn mount_cartridge(device: String) -> Result<String, String> {
    cartridge::mount_cartridge(&device).await
}

/// Exempts this cartridge's NTFS device from udisks2's default
/// `windows_names` mount option, which otherwise blocks the `:` in
/// Proton's own `dosdevices/c:` symlink — every Proton launch (Steam-native
/// or the standalone launcher) fails to create its wineprefix without this.
/// Idempotent per device UUID; part of "Preparar launcher" so both new and
/// already-formatted cartridges get it.
#[cfg(unix)]
#[tauri::command]
pub async fn ensure_symlinks(mount_point: String) -> Result<cartridge::SymlinksOutcome, String> {
    cartridge::ensure_symlinks(Path::new(&mount_point)).await
}

/// Forces `app_id` onto Proton in Steam's own `config.vdf` — needed when
/// Steam installed the app's native Linux build onto the cartridge instead
/// of the Windows one it actually needs (#206). Requires Steam closed, same
/// constraint `set_winhttp_override` already has for the same reason
/// (Steam rewrites its own config files on exit).
#[cfg(unix)]
#[tauri::command]
pub fn force_proton_compat(app_id: u64) -> Result<(), String> {
    // Closes Steam itself rather than asking the user to do it by hand
    // (confusing live, 2026-08-29) — Tatu already knows exactly why this
    // needs Steam closed, so it just does it. Relaunches on its own the
    // moment `trigger_install` opens the next `steam://` URI.
    crate::steam::stop_steam_for_config_edit().map_err(|e| e.to_string())?;
    crate::steam::force_proton_compat(&app_id.to_string()).map_err(|e| e.to_string())
}

/// Deletes an app's install (files + manifest) from the cartridge so a
/// follow-up `trigger_install` starts completely fresh — paired with
/// `force_proton_compat` when the wrong depot landed.
#[tauri::command]
pub fn uninstall_from_cartridge(app_id: u64, mount_point: String) -> Result<(), String> {
    cartridge::uninstall_from_cartridge(Path::new(&mount_point), app_id)
}
