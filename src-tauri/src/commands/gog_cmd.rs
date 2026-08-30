use std::path::PathBuf;

use tauri::{Emitter, Manager, State};

use crate::SharedState;
use crate::gog_account::{self, GogTokens};
use crate::gog_download;

/// Delay between per-game title lookups during a library sync — same
/// politeness convention `drm_cmd`'s bulk sync already uses for PCGamingWiki
/// / Steam Store, applied here since `api.gog.com` documents no rate limit
/// either way.
const GOG_TITLE_LOOKUP_DELAY_MS: u64 = 300;

#[tauri::command]
pub fn gog_login_url() -> String {
    gog_account::login_url()
}

#[tauri::command]
pub fn gog_is_connected(state: State<'_, SharedState>) -> Result<bool, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.gog_tokens.is_some())
}

#[tauri::command]
pub fn gog_disconnect(state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.gog_tokens = None;
    s.gog_library.clear();
    s.save();
    Ok(())
}

/// Exchanges the code the user pasted back from the browser login for a
/// real token pair, and stores it. `pasted` accepts either the bare code
/// or GOG's full redirect URL — see `gog_account::extract_code`.
#[tauri::command]
pub async fn gog_connect(pasted: String, state: State<'_, SharedState>) -> Result<(), String> {
    let code =
        gog_account::extract_code(&pasted).ok_or("No se encontró un código en lo que pegaste")?;
    let tokens = tokio::task::spawn_blocking(move || gog_account::exchange_code(&code))
        .await
        .map_err(|e| format!("Task error: {e}"))??;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.gog_tokens = Some(tokens);
    s.save();
    Ok(())
}

/// Refreshes the library in the background, emitting `gog_library_progress`
/// per game resolved and `gog_library_done` at the end — same pattern
/// `fetch_all_drm` already uses for its own bulk sync, needed here for the
/// same reason: a real library is dozens-plus of sequential HTTP requests
/// (one per owned id, to resolve its title), and silently blocking the
/// settings screen for that long is exactly the "looks frozen" mistake
/// this session already found and fixed once in the launcher (#254).
#[tauri::command]
pub fn fetch_gog_library(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let tokens = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.gog_tokens
            .clone()
            .ok_or("No hay una cuenta GOG conectada")?
    };

    std::thread::spawn(move || {
        let access_token = match refreshed_access_token(&app, &tokens) {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit("gog_library_error", e);
                return;
            }
        };

        let ids = match gog_account::fetch_owned_game_ids(&access_token) {
            Ok(ids) => ids,
            Err(e) => {
                let _ = app.emit("gog_library_error", e);
                return;
            }
        };

        let total = ids.len();
        let mut games = Vec::with_capacity(total);
        for (i, id) in ids.into_iter().enumerate() {
            let game = gog_account::resolve_details(id);
            let _ = app.emit(
                "gog_library_progress",
                serde_json::json!({ "current": i + 1, "total": total, "game": game }),
            );
            games.push(game);
            std::thread::sleep(std::time::Duration::from_millis(GOG_TITLE_LOOKUP_DELAY_MS));
        }

        let state: tauri::State<'_, SharedState> = app.state();
        if let Ok(mut s) = state.lock() {
            s.gog_library = games;
            s.save();
        }
        let _ = app.emit("gog_library_done", serde_json::json!({ "total": total }));
    });

    Ok(())
}

/// GOG access tokens are short-lived (1h, confirmed in the token
/// response's `expires_in`) — rather than track expiry separately, this
/// just refreshes unconditionally before a library sync (an infrequent,
/// user-initiated action) and persists the new pair if it rotated.
fn refreshed_access_token(app: &tauri::AppHandle, tokens: &GogTokens) -> Result<String, String> {
    let refreshed = gog_account::refresh(tokens)?;
    let access_token = refreshed.access_token.clone();
    let state: tauri::State<'_, SharedState> = app.state();
    if let Ok(mut s) = state.lock() {
        s.gog_tokens = Some(refreshed);
        s.save();
    }
    Ok(access_token)
}

/// What the detail window needs for a GOG game — the cached library entry
/// plus its own OS window, same split `get_game_context` already has for
/// Steam/Non-Steam (targeted per-window data, not the whole app state).
#[tauri::command]
pub fn get_gog_game_context(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<Option<gog_account::GogOwnedGame>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.gog_library.iter().find(|g| g.id == app_id).cloned())
}

/// Description and screenshots aren't cached in `gog_library` (#243) —
/// fetched live, only when the detail window actually opens on this game.
/// Genre/developer are not part of this: those come from `gog_library`
/// itself (`get_gog_game_context`), resolved once during the bulk sync.
#[tauri::command]
pub async fn fetch_gog_extra_details(app_id: u64) -> Result<gog_account::GogExtraDetails, String> {
    tokio::task::spawn_blocking(move || gog_account::fetch_extra_details(app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))
}

/// Downloads and installs `product_id` under `cartridge_base` in the
/// background, emitting `gog_download_progress` per file and
/// `gog_download_done`/`gog_download_error` at the end — same
/// background-thread-plus-events shape `fetch_gog_library` already uses.
///
/// `cartridge_base` is taken as-is from the caller rather than any
/// GOG-specific cartridge layout decided here (that's a separate,
/// not-yet-designed piece, #243) — the actual game folder name under it
/// comes from GOG's own `Repository.install_directory`, not invented by
/// this command.
#[tauri::command]
pub fn gog_download_game(
    app: tauri::AppHandle,
    product_id: u64,
    cartridge_base: String,
    language: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let tokens = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.gog_tokens
            .clone()
            .ok_or("No hay una cuenta GOG conectada")?
    };

    std::thread::spawn(move || {
        if let Err(e) = run_gog_download(&app, &tokens, product_id, &cartridge_base, &language) {
            let _ = app.emit("gog_download_error", e);
        }
    });

    Ok(())
}

fn run_gog_download(
    app: &tauri::AppHandle,
    tokens: &GogTokens,
    product_id: u64,
    cartridge_base: &str,
    language: &str,
) -> Result<(), String> {
    let access_token = refreshed_access_token(app, tokens)?;

    let builds = gog_download::fetch_builds(&access_token, product_id, "windows")?;
    let build = gog_download::pick_build(&builds).ok_or("GOG no devolvió ningún build")?;
    if build.generation != 2 {
        return Err(format!(
            "build generation {} no soportada (solo content-system v2)",
            build.generation
        ));
    }
    if build.product_id != product_id.to_string() {
        return Err(format!(
            "el build pertenece al producto {}, no a {product_id}",
            build.product_id
        ));
    }
    let _ = app.emit(
        "gog_download_started",
        serde_json::json!({
            "product_id": product_id,
            "version_name": build.version_name,
            "os": build.os,
        }),
    );
    let repo = gog_download::fetch_repository(&access_token, build)?;
    if !repo.dependencies.is_empty() {
        return Err(format!(
            "este juego necesita dependencias externas ({}) — todavía no soportado",
            repo.dependencies.join(", ")
        ));
    }
    let depot =
        gog_download::pick_depot(&repo, language).ok_or("El repositorio no tiene depots")?;
    let manifest = gog_download::fetch_depot_manifest(&access_token, depot)?;
    let endpoints = gog_download::fetch_secure_link(&access_token, product_id)?;

    let dest_root = gog_download::install_root(&PathBuf::from(cartridge_base), &repo);
    let total = manifest.items.iter().filter(|i| i.is_file()).count();
    let mut done = 0usize;
    gog_download::download_depot(
        product_id,
        &endpoints,
        depot,
        &manifest,
        &dest_root,
        |item| {
            done += 1;
            let _ = app.emit(
                "gog_download_progress",
                serde_json::json!({ "current": done, "total": total, "path": item.path }),
            );
        },
    )?;

    let _ = app.emit(
        "gog_download_done",
        serde_json::json!({ "product_id": product_id }),
    );
    Ok(())
}
