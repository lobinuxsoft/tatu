use tauri::State;

use crate::SharedState;
use crate::{achievements, hltb, inventory, steam};

#[tauri::command]
pub fn get_game_details(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let game = s
        .games
        .iter()
        .find(|g| g.id == app_id)
        .ok_or("Game not found")?;
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
pub fn get_game_achievements(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<serde_json::Value, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let api_key = s.steam_api_key.clone();
    let steam_id = s.steam_id.clone();

    let game_ach_count = s
        .games
        .iter()
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
pub async fn get_game_cards(
    app_id: u64,
    state: State<'_, SharedState>,
) -> Result<serde_json::Value, String> {
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
pub async fn search_hltb(
    app_id: u64,
    game_name: String,
    state: State<'_, SharedState>,
) -> Result<Option<hltb::HltbResult>, String> {
    // Check cache first.
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = s.hltb_cache.get(&app_id) {
            return Ok(Some(cached.clone()));
        }
    }

    let name = game_name.clone();
    let result = tokio::task::spawn_blocking(move || hltb::search(&name))
        .await
        .map_err(|e| e.to_string())??;

    let best = result.into_iter().next();

    if let Some(ref entry) = best {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        s.hltb_cache.insert(app_id, entry.clone());
        s.save();
    }

    Ok(best)
}
