use tauri::{Emitter, Manager, State};

use crate::SharedState;
use crate::egs_account::{self, EgsTokens};

/// Delay between per-game catalog lookups during a library sync — same
/// politeness convention `gog_cmd`'s bulk sync already uses.
const EGS_TITLE_LOOKUP_DELAY_MS: u64 = 300;

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
