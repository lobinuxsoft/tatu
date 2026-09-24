use tauri::State;

use crate::SharedState;
use crate::steam;

#[tauri::command]
pub fn get_steam_favorites(state: State<'_, SharedState>) -> Result<Vec<u64>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let steam_id = s.steam_id.clone();
    drop(s);
    steam::get_steam_favorites(&steam_id)
}

#[tauri::command]
pub fn list_steam_collections(
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
pub fn import_completed_from_collection(
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
