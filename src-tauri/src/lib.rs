mod achievements;
mod disk;
mod drm;
mod hltb;
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
        "ach_progress": ach_progress,
        "hltb_cache": s.hltb_cache,
        "drm_cache": s.drm_cache,
        "size_cache": s.size_cache,
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
fn get_game_details(
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
fn get_game_achievements(
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
async fn get_game_cards(
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
fn detect_steam_id() -> Option<String> {
    steam::detect_steam_id()
}

/// DRM info cache TTL: 30 days. DRM rarely changes after release.
const DRM_CACHE_TTL_SECS: u64 = 60 * 60 * 24 * 30;

/// Throttle between PCGamingWiki / Steam Store requests during bulk DRM sync.
const DRM_BULK_DELAY_MS: u64 = 1000;

/// A cached DrmInfo is considered stale if its TTL expired or if it is a
/// pre-feature record that predates the preservability classifier (detected
/// by the hint being empty while status was successfully classified).
fn drm_cache_is_stale(cached: &drm::DrmInfo, now: u64) -> bool {
    if now.saturating_sub(cached.fetched_at) >= DRM_CACHE_TTL_SECS {
        return true;
    }
    if cached.preservability_hint.is_empty() && !matches!(cached.status, drm::DrmStatus::Unknown) {
        return true;
    }
    false
}

#[tauri::command]
fn fetch_all_drm(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pending: Vec<u64> = s
        .games
        .iter()
        .filter(|g| {
            s.drm_cache
                .get(&g.id)
                .map(|cached| drm_cache_is_stale(cached, now))
                .unwrap_or(true)
        })
        .map(|g| g.id)
        .collect();
    drop(s);

    let app_clone = app.clone();
    std::thread::spawn(move || {
        let total = pending.len();
        for (i, app_id) in pending.iter().enumerate() {
            let info = match drm::fetch_drm_info(*app_id) {
                Ok(v) => v,
                Err(_) => {
                    let _ = app_clone.emit(
                        "drm_progress",
                        serde_json::json!({ "current": i + 1, "total": total }),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(DRM_BULK_DELAY_MS));
                    continue;
                }
            };
            let state: tauri::State<'_, SharedState> = app_clone.state();
            if let Ok(mut s) = state.lock() {
                s.drm_cache.insert(*app_id, info.clone());
                s.save();
            }
            let _ = app_clone.emit(
                "drm_progress",
                serde_json::json!({ "current": i + 1, "total": total, "app_id": app_id, "info": info }),
            );
            std::thread::sleep(std::time::Duration::from_millis(DRM_BULK_DELAY_MS));
        }
        let _ = app_clone.emit("drm_done", serde_json::json!({ "total": total }));
    });

    Ok(())
}

#[tauri::command]
async fn get_game_drm(app_id: u64, state: State<'_, SharedState>) -> Result<drm::DrmInfo, String> {
    {
        let s = state.lock().map_err(|e| e.to_string())?;
        if let Some(cached) = s.drm_cache.get(&app_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if !drm_cache_is_stale(cached, now) {
                return Ok(cached.clone());
            }
        }
    }

    let info = tokio::task::spawn_blocking(move || drm::fetch_drm_info(app_id))
        .await
        .map_err(|e| e.to_string())??;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.drm_cache.insert(app_id, info.clone());
    s.save();
    Ok(info)
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

#[tauri::command]
async fn search_hltb(
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

#[tauri::command]
fn get_steam_favorites(state: State<'_, SharedState>) -> Result<Vec<u64>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let steam_id = s.steam_id.clone();
    drop(s);
    steam::get_steam_favorites(&steam_id)
}

/// Scan Steam's local caches for disk sizes and merge into size_cache.
///
/// Two sources are consulted in order:
///   1. `appcache/appinfo.vdf` — covers every owned app (including not
///      currently installed) via depot manifest sizes. Upper-bound estimate.
///   2. `steamapps/libraryfolders.vdf` — exact SizeOnDisk for apps installed
///      right now. Overwrites any appinfo estimate for those apps.
///
/// Returns every entry in size_cache that corresponds to an app in the
/// tracker's library, sorted by app_id.
#[tauri::command]
fn scan_sizes(state: State<'_, SharedState>) -> Result<Vec<disk::DiskSize>, String> {
    let steam_dir = steam::steam_install_dir().ok_or("Steam install directory not found")?;

    // Best-effort: appinfo may fail parsing on very new formats, but we still
    // want libraryfolders results if so.
    let appinfo_entries = disk::scan_appinfo_sizes(&steam_dir).unwrap_or_default();
    let installed_entries = disk::scan_installed_sizes(&steam_dir)?;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    let known_ids: std::collections::HashSet<u64> = s.games.iter().map(|g| g.id).collect();

    // First pass: appinfo (lower priority).
    for entry in appinfo_entries {
        s.size_cache.insert(entry.app_id, entry);
    }
    // Second pass: libraryfolders (higher priority, exact on-disk size).
    for entry in installed_entries {
        s.size_cache.insert(entry.app_id, entry);
    }

    let mut result: Vec<disk::DiskSize> = s
        .size_cache
        .values()
        .filter(|e| known_ids.contains(&e.app_id))
        .cloned()
        .collect();
    result.sort_by_key(|e| e.app_id);
    s.save();
    Ok(result)
}

#[tauri::command]
fn list_steam_collections(
    state: State<'_, SharedState>,
) -> Result<Vec<steam::SteamCollection>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let steam_id = s.steam_id.clone();
    drop(s);
    steam::list_steam_collections(&steam_id)
}

/// Import a Steam collection as completed games. Merges with existing completed
/// set (does not overwrite). Only imports app IDs present in the user's library.
/// Returns (matched_count, unknown_count) — games in the collection that are not
/// in the tracker library (e.g. removed, non-Steam) are reported as unknown.
#[tauri::command]
fn import_completed_from_collection(
    collection_name: String,
    state: State<'_, SharedState>,
) -> Result<(usize, usize), String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let steam_id = s.steam_id.clone();
    let known_ids: std::collections::HashSet<u64> = s.games.iter().map(|g| g.id).collect();
    drop(s);

    let found = steam::find_steam_collection_by_name(&steam_id, &collection_name)?
        .ok_or_else(|| format!("Collection not found: {collection_name}"))?;

    let mut matched = 0usize;
    let mut unknown = 0usize;

    let mut s = state.lock().map_err(|e| e.to_string())?;
    for app_id in &found.added {
        if known_ids.contains(app_id) {
            if s.completed.insert(*app_id) {
                matched += 1;
            }
        } else {
            unknown += 1;
        }
    }
    s.save();
    Ok((matched, unknown))
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
            get_steam_favorites,
            search_hltb,
            get_game_drm,
            fetch_all_drm,
            list_steam_collections,
            import_completed_from_collection,
            scan_sizes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
