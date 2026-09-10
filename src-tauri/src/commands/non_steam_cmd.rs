// Non-Steam entries have no manifest, no store page of their own — #328
// lets the user hand-assign the matching Steam listing (many DRM-free
// copies have one) so the detail window can show real info/art instead of
// nothing, the same way #236's cartridge copy uses it for launcher-side art.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::SharedState;
use crate::cartridge;

/// Sets (or clears, with `None`) the Steam AppID the user is confirming
/// matches this non-Steam entry — `non_steam_id` is the shortcut id, never
/// a Steam appid itself. Persists immediately so it survives the next
/// `sync_nonsteam` re-parse of `shortcuts.vdf` (see that command's own
/// merge-by-id logic).
#[tauri::command]
pub fn set_non_steam_appid(
    state: State<'_, SharedState>,
    non_steam_id: u64,
    steam_app_id: Option<u64>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    let game = s
        .non_steam
        .iter_mut()
        .find(|g| g.id == non_steam_id)
        .ok_or_else(|| format!("Juego Non-Steam {non_steam_id} no encontrado"))?;
    game.steam_app_id = steam_app_id;
    s.save();
    Ok(())
}

/// Live look at what `steam_app_id` resolves to on Steam's own store +
/// SteamGridDB, for the detail window to render immediately — confirms a
/// wrong id before it ever gets baked into a cartridge (#236).
#[tauri::command]
pub async fn get_non_steam_preview(
    state: State<'_, SharedState>,
    steam_app_id: u64,
) -> Result<cartridge::LiveSteamPreview, String> {
    let api_key = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.steamgriddb_api_key.clone()
    };
    cartridge::fetch_live_steam_preview(api_key, steam_app_id).await
}

/// Sets (or clears, with `None`/empty) the user's correction for this
/// shortcut's install root (#236) — `shortcuts.vdf`'s own "Start In" is
/// frequently just the exe's own folder, not the whole game (live case:
/// an Unreal Engine game's `<Root>/<Game>/Binaries/Win64/`), so the literal
/// value can't always be trusted for what to copy onto a cartridge.
#[tauri::command]
pub fn set_non_steam_install_root(
    state: State<'_, SharedState>,
    non_steam_id: u64,
    path: Option<String>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    let game = s
        .non_steam
        .iter_mut()
        .find(|g| g.id == non_steam_id)
        .ok_or_else(|| format!("Juego Non-Steam {non_steam_id} no encontrado"))?;
    game.install_root_override = path.filter(|p| !p.is_empty());
    s.save();
    Ok(())
}

/// Native folder picker for the correction above — no reason to hand-roll
/// a browser in HTML when the OS already has one. Must run in an async
/// command: the blocking dialog call is meant to block only this command's
/// own task, not the whole app (`tauri-plugin-dialog`'s own docs warn
/// against calling it from a sync context for exactly that reason).
#[tauri::command]
pub async fn pick_non_steam_folder(app: AppHandle, start_in: Option<String>) -> Option<String> {
    let mut builder = app.dialog().file();
    if let Some(dir) = start_in.filter(|d| !d.is_empty()) {
        builder = builder.set_directory(dir);
    }
    let picked = builder.blocking_pick_folder()?;
    picked
        .into_path()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}
