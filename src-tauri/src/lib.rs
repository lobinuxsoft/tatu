mod achievements;
mod inventory;
mod shortcuts;
mod state;
mod steam;

use std::sync::Mutex;

use shortcuts::NonSteamGame;
use state::AppState;
use steam::Game;
use tauri::{Emitter, Manager, State};

type SharedState = Mutex<AppState>;

#[tauri::command]
fn get_state(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    // Build achievement progress summary from cache.
    let ach_progress: std::collections::HashMap<u64, (usize, usize)> = s.achievement_cache.iter()
        .map(|(&app_id, cached)| {
            let unlocked = cached.achievements.iter().filter(|a| a.achieved).count();
            let total = cached.achievements.len();
            (app_id, (unlocked, total))
        })
        .collect();

    Ok(serde_json::json!({
        "games": s.games,
        "completed": s.completed,
        "completed_nonsteam": s.completed_nonsteam,
        "last_sync": s.last_sync,
        "non_steam": s.non_steam,
        "steam_api_key": s.steam_api_key,
        "steam_id": s.steam_id,
        "ach_progress": ach_progress,
    }))
}

#[tauri::command]
fn get_settings(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "steam_api_key": s.steam_api_key,
        "steam_id": s.steam_id,
    }))
}

#[tauri::command]
fn save_settings(
    steam_api_key: String,
    steam_id: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.steam_api_key = steam_api_key;
    s.steam_id = steam_id;
    s.save();
    Ok(())
}

#[tauri::command]
fn sync_steam(state: State<'_, SharedState>) -> Result<Vec<Game>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let key = s.steam_api_key.clone();
    let id = s.steam_id.clone();
    drop(s);

    let games = steam::fetch_games(&key, &id)?;
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.games = games.clone();
    s.last_sync = Some(now_epoch());
    s.save();
    Ok(games)
}

#[tauri::command]
fn sync_nonsteam(state: State<'_, SharedState>) -> Result<Vec<NonSteamGame>, String> {
    let games = shortcuts::parse_shortcuts()?;
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.non_steam = games.clone();
    s.save();
    Ok(games)
}

#[tauri::command]
fn fetch_details(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let mut games = s.games.clone();
    drop(s);

    let app_clone = app.clone();
    std::thread::spawn(move || {
        steam::fetch_details_for(&mut games, |current, total| {
            let _ = app_clone.emit(
                "detail_progress",
                serde_json::json!({ "current": current, "total": total }),
            );
        });

        let state: tauri::State<'_, SharedState> = app_clone.state();
        if let Ok(mut s) = state.lock() {
            s.games = games;
            s.save();
            let _ = app_clone.emit("details_done", serde_json::json!({ "games": &s.games }));
        }
    });

    Ok(())
}

#[tauri::command]
fn get_game_details(app_id: u64, state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let game = s.games.iter().find(|g| g.id == app_id).ok_or("Game not found")?;
    let needs_fetch = game.genres.is_empty();
    let mut game_clone = game.clone();
    drop(s);

    if needs_fetch {
        steam::fetch_single_detail(&mut game_clone);
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if let Some(g) = s.games.iter_mut().find(|g| g.id == app_id) {
            *g = game_clone.clone();
        }
        s.save();
    }

    serde_json::to_value(&game_clone).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_game_achievements(app_id: u64, state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let api_key = s.steam_api_key.clone();
    let steam_id = s.steam_id.clone();

    let game_ach_count = s.games.iter()
        .find(|g| g.id == app_id)
        .map(|g| g.achievements)
        .unwrap_or(0);

    // Check cache.
    if let Some(cached) = s.achievement_cache.get(&app_id) {
        if cached.achievements.len() as u32 == game_ach_count {
            let cached_clone = cached.clone();
            drop(s);

            // Lightweight freshness check.
            match achievements::fetch_max_unlock_time(&api_key, &steam_id, app_id) {
                Ok(max_time) if max_time == cached_clone.last_max_unlock_time => {
                    return serde_json::to_value(&cached_clone).map_err(|e| e.to_string());
                }
                _ => {} // Cache stale or check failed, full fetch below.
            }
        } else {
            drop(s);
        }
    } else {
        drop(s);
    }

    // Full fetch.
    let result = achievements::fetch_game_achievements(&api_key, &steam_id, app_id)?;
    let json = serde_json::to_value(&result).map_err(|e| e.to_string())?;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.achievement_cache.insert(app_id, result);
    s.save();

    Ok(json)
}

#[tauri::command]
async fn get_game_cards(app_id: u64, state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let steam_id = {
        let s = state.lock().map_err(|e| e.to_string())?;

        if let Some(cached) = s.cards_cache.get(&app_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now - cached.fetched_at < 1800 {
                return serde_json::to_value(cached).map_err(|e| e.to_string());
            }
        }
        s.steam_id.clone()
    };

    let result = inventory::fetch_game_cards(steam_id, app_id).await?;
    let json = serde_json::to_value(&result).map_err(|e| e.to_string())?;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.cards_cache.insert(app_id, result);
    s.save();

    Ok(json)
}

#[tauri::command]
fn detect_steam_id() -> Option<String> {
    steam::detect_steam_id()
}

#[tauri::command]
fn save_completed(completed: Vec<u64>, state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.completed = completed.into_iter().collect();
    s.save();
    Ok(())
}

#[tauri::command]
fn save_completed_nonsteam(
    completed: Vec<u64>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.completed_nonsteam = completed.into_iter().collect();
    s.save();
    Ok(())
}

fn now_epoch() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}

pub fn run() {
    let app_state = AppState::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(app_state))
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_settings,
            save_settings,
            sync_steam,
            sync_nonsteam,
            fetch_details,
            get_game_details,
            get_game_achievements,
            get_game_cards,
            detect_steam_id,
            save_completed,
            save_completed_nonsteam,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
