use tauri::{Emitter, Manager, State};

use crate::SharedState;
use crate::shortcuts::{self, NonSteamGame};
use crate::steam::{self, Game};

#[tauri::command]
pub fn sync_steam(state: State<'_, SharedState>) -> Result<Vec<Game>, String> {
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
pub fn sync_nonsteam(state: State<'_, SharedState>) -> Result<Vec<NonSteamGame>, String> {
    let games = shortcuts::parse_shortcuts()?;
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.non_steam = games.clone();
    s.save();
    Ok(games)
}

#[tauri::command]
pub fn fetch_details(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
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

fn now_epoch() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}
