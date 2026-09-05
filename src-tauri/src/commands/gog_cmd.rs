use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager, State};

use crate::SharedState;
use crate::gog_account::{self, GogTokens};
use crate::gog_download;

/// Delay between per-game title lookups during a library sync — same
/// politeness convention `drm_cmd`'s bulk sync already uses for PCGamingWiki
/// / Steam Store, applied here since `api.gog.com` documents no rate limit
/// either way.
const GOG_TITLE_LOOKUP_DELAY_MS: u64 = 300;

/// A GOG download's own stop signal, separate from `SharedState` (that one
/// is persisted to `state.json` on every save — this is pure runtime
/// coordination, gone the moment the app closes). Only one GOG download can
/// run at a time from the UI (one modal), so a single slot is enough; a
/// second `gog_download_game` call replaces it rather than stacking.
#[derive(Default)]
pub struct GogDownloadCancel(pub Mutex<Option<Arc<AtomicBool>>>);

/// Requests that the in-flight GOG download stop after its current file.
/// A no-op if nothing is downloading — the UI only shows the button while
/// a download is visibly running, but a stale click racing the download's
/// own completion should never be treated as an error.
#[tauri::command]
pub fn gog_cancel_download(state: State<'_, GogDownloadCancel>) {
    if let Ok(guard) = state.0.lock()
        && let Some(flag) = guard.as_ref()
    {
        flag.store(true, Ordering::Relaxed);
    }
}

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

/// Downloads and installs `product_id` under `mount_point/GOG` in the
/// background, emitting `gog_download_progress` per file and
/// `gog_download_done`/`gog_download_error` at the end — same
/// background-thread-plus-events shape `fetch_gog_library` already uses.
///
/// `GOG/` is a sibling of Steam's own `steamapps/`, namespaced to avoid any
/// collision with it or with whatever non-Steam cartridge layout #236 ends
/// up designing — the actual game folder name under it still comes from
/// GOG's own `Repository.install_directory`, not invented here.
#[tauri::command]
pub fn gog_download_game(
    app: tauri::AppHandle,
    product_id: u64,
    game_name: String,
    mount_point: String,
    language: String,
    state: State<'_, SharedState>,
    cancel_state: State<'_, GogDownloadCancel>,
) -> Result<(), String> {
    let tokens = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.gog_tokens
            .clone()
            .ok_or("No hay una cuenta GOG conectada")?
    };

    let cancel = Arc::new(AtomicBool::new(false));
    *cancel_state.0.lock().map_err(|e| e.to_string())? = Some(cancel.clone());

    std::thread::spawn(move || {
        if let Err(e) = run_gog_download(
            &app,
            &tokens,
            product_id,
            &game_name,
            &mount_point,
            &language,
            &cancel,
        ) {
            // Cancellation isn't a failure — the button that requested it
            // already told the user what's happening, this just confirms
            // it actually stopped rather than leaving them guessing (the
            // whole reason this exists, #243: a modal that could only ever
            // be dismissed by clicking outside gave no way to tell whether
            // a download was still running in the background).
            if cancel.load(Ordering::Relaxed) {
                let _ = app.emit("gog_download_cancelled", ());
            } else {
                let _ = app.emit("gog_download_error", e);
            }
        }
    });

    Ok(())
}

/// What `gog_get_download_size`/`run_gog_download` share: build/repo/depot
/// resolution and the validation that only a supported, dependency-free
/// generation-2 build proceeds. Cheap — `builds` and `repository` are both
/// small JSON responses, no manifest or chunk fetched yet — so the size
/// check can run well before committing to an actual download.
fn resolve_depot(
    access_token: &str,
    product_id: u64,
    language: &str,
) -> Result<
    (
        gog_download::Build,
        gog_download::Repository,
        gog_download::Depot,
    ),
    String,
> {
    let builds = gog_download::fetch_builds(access_token, product_id, "windows")?;
    let build = gog_download::pick_build(&builds)
        .ok_or("GOG no devolvió ningún build")?
        .clone();
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
    let repo = gog_download::fetch_repository(access_token, &build)?;
    if !repo.dependencies.is_empty() {
        return Err(format!(
            "este juego necesita dependencias externas ({}) — todavía no soportado",
            repo.dependencies.join(", ")
        ));
    }
    let depot = gog_download::pick_depot(&repo, language)
        .ok_or("El repositorio no tiene depots")?
        .clone();
    Ok((build, repo, depot))
}

#[derive(serde::Serialize)]
pub struct GogDownloadSize {
    version_name: String,
    size: u64,
    compressed_size: u64,
}

/// Lets the UI show how big a download actually is (and let the user bail)
/// before spending any time on the manifest fetch or the download itself.
#[tauri::command]
pub async fn gog_get_download_size(
    app: tauri::AppHandle,
    product_id: u64,
    language: String,
    state: State<'_, SharedState>,
) -> Result<GogDownloadSize, String> {
    let tokens = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.gog_tokens
            .clone()
            .ok_or("No hay una cuenta GOG conectada")?
    };
    tokio::task::spawn_blocking(move || {
        let access_token = refreshed_access_token(&app, &tokens)?;
        let (build, _repo, depot) = resolve_depot(&access_token, product_id, &language)?;
        Ok(GogDownloadSize {
            version_name: build.version_name,
            size: depot.size,
            compressed_size: depot.compressed_size,
        })
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

fn run_gog_download(
    app: &tauri::AppHandle,
    tokens: &GogTokens,
    product_id: u64,
    game_name: &str,
    mount_point: &str,
    language: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let access_token = refreshed_access_token(app, tokens)?;
    let (build, repo, depot) = resolve_depot(&access_token, product_id, language)?;

    let _ = app.emit(
        "gog_download_started",
        serde_json::json!({
            "product_id": product_id,
            "version_name": build.version_name,
            "os": build.os,
        }),
    );
    let manifest = gog_download::fetch_depot_manifest(&access_token, &depot)?;
    let endpoints = gog_download::fetch_secure_link(&access_token, product_id)?;

    let cartridge_base = PathBuf::from(mount_point).join("GOG");
    let dest_root = gog_download::install_root(&cartridge_base, &repo);
    let total = manifest.items.iter().filter(|i| i.is_file()).count();
    let mut done = 0usize;
    // A depot with thousands of small files (confirmed live: 2200 files
    // for a 443MB game) emitted one IPC event per file with no gap between
    // them, which correlated live with the webview's WebKit renderer
    // process crashing (SIGSEGV, 4.6GB memory peak) and freezing the whole
    // machine. Throttled to at most 5/s — always still emits the very
    // last file so the UI actually reaches "done" instead of stalling at
    // a stale percentage.
    let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
    gog_download::download_depot(
        product_id,
        &endpoints,
        &depot,
        &manifest,
        &dest_root,
        |item| {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            done += 1;
            let is_last = done == total;
            if is_last || last_emit.elapsed() >= std::time::Duration::from_millis(200) {
                last_emit = std::time::Instant::now();
                let _ = app.emit(
                    "gog_download_progress",
                    serde_json::json!({ "current": done, "total": total, "path": item.path }),
                );
            }
            true
        },
    )?;

    // GOG ships DRM-free — no Steamworks wrapper to strip, no Goldberg
    // step needed — so this is standalone the moment the bytes land,
    // unlike a Steam app's two-step install-then-classify dance.
    // `pick_main_exe_in` is the same install-dir-scanning heuristic
    // `inject_goldberg` uses for Steam games; it only returns a bare
    // filename, so joining it under `dest_root` (and not deeper, matching
    // that same existing limitation) assumes the exe sits directly in the
    // install root — true for most of the small/indie/visual-novel titles
    // GOG's catalog skews toward, but not guaranteed for every engine.
    // 0: GOG product ids aren't Steam appids, so `KNOWN_EXE_OVERRIDES`
    // (keyed by Steam appid) never matches here — falls straight through
    // to the heuristic, same as before this parameter existed.
    let exe_name = crate::steam::pick_main_exe_in(&dest_root, 0)?;
    let exe_path = dest_root
        .join(&exe_name)
        .strip_prefix(PathBuf::from(mount_point))
        .map_err(|_| "Resolved exe path escaped the cartridge root".to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    crate::cartridge::add_app(
        Path::new(mount_point),
        crate::cartridge::CartridgeApp {
            app_id: product_id,
            name: game_name.to_string(),
            source: crate::cartridge::AppSource::Gog,
            preservability: crate::drm::Preservability::Alternative,
            standalone: true,
            exe_path,
        },
    )?;

    let _ = app.emit(
        "gog_download_done",
        serde_json::json!({ "product_id": product_id }),
    );
    Ok(())
}
