use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager, State};

use crate::SharedState;
use crate::egs_account::{self, EgsOwnedGame, EgsTokens};
use crate::egs_download;

/// Delay between per-game catalog lookups during a library sync — same
/// politeness convention `gog_cmd`'s bulk sync already uses.
const EGS_TITLE_LOOKUP_DELAY_MS: u64 = 300;

/// An EGS download's own stop signal — same runtime-only (never persisted)
/// single-slot design `gog_cmd::GogDownloadCancel` uses, for the same
/// reason: only one EGS download can run at a time from the UI.
#[derive(Default)]
pub struct EgsDownloadCancel(pub Mutex<Option<Arc<AtomicBool>>>);

#[tauri::command]
pub fn egs_cancel_download(state: State<'_, EgsDownloadCancel>) {
    if let Ok(guard) = state.0.lock()
        && let Some(flag) = guard.as_ref()
    {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn egs_login_url() -> String {
    egs_account::login_url()
}

#[tauri::command]
pub fn egs_is_connected(state: State<'_, SharedState>) -> Result<bool, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.egs_tokens.is_some())
}

#[tauri::command]
pub fn egs_disconnect(state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.egs_tokens = None;
    s.egs_library.clear();
    s.save();
    Ok(())
}

/// Exchanges the code the user pasted back from the browser login for a
/// real token pair, and stores it. `pasted` accepts either the bare code or
/// the JSON blob Epic's own redirect page renders — see
/// `egs_account::extract_code`.
#[tauri::command]
pub async fn egs_connect(pasted: String, state: State<'_, SharedState>) -> Result<(), String> {
    let code =
        egs_account::extract_code(&pasted).ok_or("No se encontró un código en lo que pegaste")?;
    let tokens = tokio::task::spawn_blocking(move || egs_account::exchange_code(&code))
        .await
        .map_err(|e| format!("Task error: {e}"))??;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.egs_tokens = Some(tokens);
    s.save();
    Ok(())
}

/// Refreshes the library in the background, emitting `egs_library_progress`
/// per game resolved and `egs_library_done` at the end — same pattern
/// `fetch_gog_library` already uses.
#[tauri::command]
pub fn fetch_egs_library(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let tokens = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.egs_tokens
            .clone()
            .ok_or("No hay una cuenta de Epic Games conectada")?
    };

    std::thread::spawn(move || {
        let access_token = match refreshed_access_token(&app, &tokens) {
            Ok(t) => t,
            Err(e) => {
                let _ = app.emit("egs_library_error", e);
                return;
            }
        };

        let entries = match egs_account::fetch_owned_apps(&access_token) {
            Ok(entries) => entries,
            Err(e) => {
                let _ = app.emit("egs_library_error", e);
                return;
            }
        };

        let total = entries.len();
        let mut games = Vec::with_capacity(total);
        for (i, entry) in entries.into_iter().enumerate() {
            let game = egs_account::resolve_details(&access_token, &entry);
            let _ = app.emit(
                "egs_library_progress",
                serde_json::json!({ "current": i + 1, "total": total, "game": game }),
            );
            games.push(game);
            std::thread::sleep(std::time::Duration::from_millis(EGS_TITLE_LOOKUP_DELAY_MS));
        }

        let state: tauri::State<'_, SharedState> = app.state();
        if let Ok(mut s) = state.lock() {
            s.egs_library = games;
            s.save();
        }
        let _ = app.emit("egs_library_done", serde_json::json!({ "total": total }));
    });

    Ok(())
}

/// EGS access tokens are short-lived — same unconditional-refresh-before-use
/// approach `gog_cmd::refreshed_access_token` takes, for the same reason:
/// this is an infrequent, user-initiated action, so tracking expiry
/// separately buys nothing over just refreshing every time.
fn refreshed_access_token(app: &tauri::AppHandle, tokens: &EgsTokens) -> Result<String, String> {
    let refreshed = egs_account::refresh(tokens)?;
    let access_token = refreshed.access_token.clone();
    let state: tauri::State<'_, SharedState> = app.state();
    if let Ok(mut s) = state.lock() {
        s.egs_tokens = Some(refreshed);
        s.save();
    }
    Ok(access_token)
}

/// What the detail window needs for an EGS game — same targeted-per-window
/// split `get_gog_game_context` already has.
#[tauri::command]
pub fn get_egs_game_context(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<Option<egs_account::EgsOwnedGame>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.egs_library.iter().find(|g| g.id == app_id).cloned())
}

fn game_from_state(app_id: u64, state: &State<'_, SharedState>) -> Result<EgsOwnedGame, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    s.egs_library
        .iter()
        .find(|g| g.id == app_id)
        .cloned()
        .ok_or_else(|| "Juego no encontrado en la biblioteca de Epic Games".to_string())
}

fn tokens_from_state(state: &State<'_, SharedState>) -> Result<EgsTokens, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    s.egs_tokens
        .clone()
        .ok_or("No hay una cuenta de Epic Games conectada")
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct EgsDownloadSize {
    build_version: String,
    total_size: u64,
}

/// Fetches and parses the real manifest to report its actual size before
/// spending any bandwidth on a chunk — same "just JSON/manifest metadata,
/// no bytes yet" cost `gog_get_download_size` already keeps cheap enough to
/// run every time the detail window opens.
#[tauri::command]
pub async fn egs_get_download_size(
    app: tauri::AppHandle,
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<EgsDownloadSize, String> {
    let tokens = tokens_from_state(&state)?;
    let game = game_from_state(app_id, &state)?;
    tokio::task::spawn_blocking(move || {
        let access_token = refreshed_access_token(&app, &tokens)?;
        let mirrors = egs_download::fetch_manifest_mirrors(
            &access_token,
            &game.namespace,
            &game.catalog_item_id,
            &game.app_name,
        )?;
        let manifest = egs_download::fetch_manifest(&mirrors)?;
        let total_size = manifest.files.iter().map(|f| f.size()).sum();
        Ok(EgsDownloadSize {
            build_version: manifest.build_version,
            total_size,
        })
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

/// Strips characters that aren't safe as a path component on either
/// platform Tatu targets, so a game's own display title (which may contain
/// `:`, `™`, `®`, etc.) can be used directly as its install folder name —
/// EGS's manifest carries no separate "install directory" field the way
/// GOG's `Repository.install_directory` does, so unlike `gog_download`
/// there's nothing cleaner to reuse here.
fn sanitize_folder_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            _ => c,
        })
        .collect();
    cleaned.trim().to_string()
}

/// Downloads and installs `app_id` under `mount_point/EGS` in the
/// background, emitting `egs_download_progress` per file and
/// `egs_download_done`/`egs_download_error` at the end — same
/// background-thread-plus-events shape `gog_download_game` already uses.
#[tauri::command]
pub fn egs_download_game(
    app: tauri::AppHandle,
    app_id: u64,
    mount_point: String,
    state: State<'_, SharedState>,
    cancel_state: State<'_, EgsDownloadCancel>,
) -> Result<(), String> {
    let tokens = tokens_from_state(&state)?;
    let game = game_from_state(app_id, &state)?;

    let cancel = Arc::new(AtomicBool::new(false));
    *cancel_state.0.lock().map_err(|e| e.to_string())? = Some(cancel.clone());

    std::thread::spawn(move || {
        if let Err(e) = run_egs_download(&app, &tokens, &game, &mount_point, &cancel) {
            // Same "cancellation isn't a failure" distinction
            // `gog_cmd::gog_download_game` already draws — the button that
            // requested it already told the user what's happening, this
            // just confirms it actually stopped.
            if cancel.load(Ordering::Relaxed) {
                let _ = app.emit("egs_download_cancelled", ());
            } else {
                let _ = app.emit("egs_download_error", e);
            }
        }
    });

    Ok(())
}

fn run_egs_download(
    app: &tauri::AppHandle,
    tokens: &EgsTokens,
    game: &EgsOwnedGame,
    mount_point: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let access_token = refreshed_access_token(app, tokens)?;
    let mirrors = egs_download::fetch_manifest_mirrors(
        &access_token,
        &game.namespace,
        &game.catalog_item_id,
        &game.app_name,
    )?;
    let manifest = egs_download::fetch_manifest(&mirrors)?;

    let _ = app.emit(
        "egs_download_started",
        serde_json::json!({ "app_id": game.id, "build_version": manifest.build_version }),
    );

    let install_dir = sanitize_folder_name(&game.title);
    let dest_root = PathBuf::from(mount_point).join("EGS").join(&install_dir);
    let total = manifest.files.len();
    let mut done = 0usize;
    // Same throttling `gog_cmd::run_gog_download` already applies — a
    // depot with thousands of small files emitting one IPC event per file
    // with no gap correlated live with the webview's renderer crashing
    // under memory pressure.
    let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
    egs_download::download_game(&mirrors, &manifest, &dest_root, |file| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        done += 1;
        let is_last = done == total;
        if is_last || last_emit.elapsed() >= std::time::Duration::from_millis(200) {
            last_emit = std::time::Instant::now();
            let _ = app.emit(
                "egs_download_progress",
                serde_json::json!({ "current": done, "total": total, "path": file.filename }),
            );
        }
        true
    })?;

    // EGS ships DRM-free once downloaded through a valid entitlement — no
    // Steamworks wrapper to strip, no Goldberg step needed, same as GOG's
    // own "standalone the moment the bytes land" case.
    let exe_relative = manifest.launch_exe.replace('\\', "/");
    let exe_path = dest_root
        .join(&exe_relative)
        .strip_prefix(Path::new(mount_point))
        .map_err(|_| "Resolved exe path escaped the cartridge root".to_string())?
        .to_string_lossy()
        .replace('\\', "/");

    crate::cartridge::add_app(
        Path::new(mount_point),
        crate::cartridge::CartridgeApp {
            app_id: game.id,
            name: game.title.clone(),
            source: crate::cartridge::AppSource::Egs,
            preservability: crate::drm::Preservability::Alternative,
            standalone: true,
            exe_path,
        },
    )?;

    let _ = app.emit(
        "egs_download_done",
        serde_json::json!({ "app_id": game.id }),
    );
    Ok(())
}
