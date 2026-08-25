use tauri::State;

use crate::SharedState;

#[tauri::command]
pub fn get_state(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    // Build achievement progress summary from cache.
    let ach_progress: std::collections::HashMap<u64, (usize, usize)> = s
        .achievement_cache
        .iter()
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
        "steamgriddb_api_key": s.steamgriddb_api_key,
        "ach_progress": ach_progress,
        "hltb_cache": s.hltb_cache,
        "drm_cache": s.drm_cache,
        "size_cache": s.size_cache,
    }))
}

#[tauri::command]
pub fn get_settings(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "steam_api_key": s.steam_api_key,
        "steam_id": s.steam_id,
        "steamgriddb_api_key": s.steamgriddb_api_key,
    }))
}

#[tauri::command]
pub fn save_settings(
    steam_api_key: String,
    steam_id: String,
    steamgriddb_api_key: String,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.steam_api_key = steam_api_key;
    s.steam_id = steam_id;
    s.steamgriddb_api_key = steamgriddb_api_key;
    s.save();
    Ok(())
}

#[tauri::command]
pub fn save_completed(completed: Vec<u64>, state: State<'_, SharedState>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.completed = completed.into_iter().collect();
    s.save();
    Ok(())
}

#[tauri::command]
pub fn save_completed_nonsteam(
    completed: Vec<u64>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.completed_nonsteam = completed.into_iter().collect();
    s.save();
    Ok(())
}
