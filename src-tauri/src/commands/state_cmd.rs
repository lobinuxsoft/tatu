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
        "gog_connected": s.gog_tokens.is_some(),
        "gog_library": s.gog_library,
        "completed_gog": s.completed_gog,
    }))
}

/// Everything the detail window (#187, its own OS window/process) actually
/// needs to open on one game — not the whole library. It used to call
/// `get_state` just like the main window, which meant serializing and
/// shipping every game plus the full DRM/HLTB/achievement/size caches
/// (2.8MB+ for a 540-game library, live-measured) over IPC just to render
/// one game's header before its own panels start their own targeted
/// fetches. That's the entire reason opening a game felt like a hang: a
/// blank window for however long that transfer took, with no spinner.
#[tauri::command]
pub fn get_game_context(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let game = s.games.iter().find(|g| g.id == app_id);
    let non_steam_game = s.non_steam.iter().find(|g| g.id == app_id);
    let drm_info = s.drm_cache.get(&app_id);

    Ok(serde_json::json!({
        "game": game,
        "non_steam_game": non_steam_game,
        "drm_info": drm_info,
    }))
}

#[tauri::command]
pub fn get_settings(state: State<'_, SharedState>) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "steam_api_key": s.steam_api_key,
        "steam_id": s.steam_id,
        "steamgriddb_api_key": s.steamgriddb_api_key,
        // Never the raw tokens — the renderer only needs to know whether an
        // account is connected and what it last saw, every real GOG call
        // stays in Rust.
        "gog_connected": s.gog_tokens.is_some(),
        "gog_library": s.gog_library,
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

#[tauri::command]
pub fn save_completed_gog(
    completed: Vec<u64>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.completed_gog = completed.into_iter().collect();
    s.save();
    Ok(())
}
