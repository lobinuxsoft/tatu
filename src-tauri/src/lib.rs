mod state;
mod steam;

use std::sync::Mutex;

use state::AppState;
use steam::Game;
use tauri::{Emitter, Manager, State};

type SharedState = Mutex<AppState>;

#[tauri::command]
fn get_state(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "games": s.games,
        "completed": s.completed,
        "last_sync": s.last_sync,
    }))
}

#[tauri::command]
fn sync_steam(state: State<'_, SharedState>) -> Result<Vec<Game>, String> {
    let games = steam::fetch_games()?;
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.games = games.clone();
    s.last_sync = Some(now_epoch());
    s.save();
    Ok(games)
}

#[tauri::command]
fn fetch_details(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let mut games = s.games.clone();
    drop(s); // Release lock before long operation.

    let app_clone = app.clone();
    std::thread::spawn(move || {
        steam::fetch_details_for(&mut games, |current, total| {
            let _ = app_clone.emit("detail_progress", serde_json::json!({
                "current": current,
                "total": total,
            }));
        });

        // Save updated games back to state.
        let state: tauri::State<'_, SharedState> = app_clone.state();
        if let Ok(mut s) = state.lock() {
            s.games = games;
            s.save();
            let _ = app_clone.emit("details_done", serde_json::json!({
                "games": &s.games,
            }));
        }
    });

    Ok(())
}

#[tauri::command]
fn save_completed(completed: Vec<u64>, state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.completed = completed.into_iter().collect();
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
        .manage(Mutex::new(app_state))
        .invoke_handler(tauri::generate_handler![
            get_state,
            sync_steam,
            fetch_details,
            save_completed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
