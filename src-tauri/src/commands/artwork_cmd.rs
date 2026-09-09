// Manual SteamGridDB art override (#328) — one shared map on `AppState`
// (`state.artwork`, keyed by id), so the SAME picker tab works for Steam,
// GOG, and Non-Steam entries alike (id namespaces don't formally collide,
// see `CartridgeApp::source`'s own doc comment for the same reasoning).

use tauri::State;

use crate::SharedState;
use crate::cartridge;
use crate::shortcuts::ArtworkSelection;

#[tauri::command]
pub fn get_artwork(
    state: State<'_, SharedState>,
    id: u64,
) -> Result<Option<ArtworkSelection>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.artwork.get(&id).cloned())
}

/// Consumed later by #236's cartridge copy to cache the chosen files, same
/// "Tatu prepares, launcher consumes" split every other cartridge asset
/// already follows. An empty selection removes the entry entirely rather
/// than storing a useless all-blank one.
#[tauri::command]
pub fn set_artwork(
    state: State<'_, SharedState>,
    id: u64,
    artwork: ArtworkSelection,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if artwork.is_empty() {
        s.artwork.remove(&id);
    } else {
        s.artwork.insert(id, artwork);
    }
    s.save();
    Ok(())
}

fn api_key(state: &State<'_, SharedState>) -> Result<String, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.steamgriddb_api_key.clone())
}

#[tauri::command]
pub async fn steamgriddb_search(
    state: State<'_, SharedState>,
    term: String,
) -> Result<Vec<cartridge::ArtSearchResult>, String> {
    cartridge::search_games(api_key(&state)?, term).await
}

#[tauri::command]
pub async fn steamgriddb_grids(
    state: State<'_, SharedState>,
    game_id: i32,
    page: i32,
    filters: cartridge::ArtFilters,
) -> Result<Vec<cartridge::ArtImage>, String> {
    cartridge::fetch_grids(api_key(&state)?, game_id, page, filters).await
}

#[tauri::command]
pub async fn steamgriddb_heroes(
    state: State<'_, SharedState>,
    game_id: i32,
    page: i32,
    filters: cartridge::ArtFilters,
) -> Result<Vec<cartridge::ArtImage>, String> {
    cartridge::fetch_heroes(api_key(&state)?, game_id, page, filters).await
}

#[tauri::command]
pub async fn steamgriddb_logos(
    state: State<'_, SharedState>,
    game_id: i32,
    page: i32,
    filters: cartridge::ArtFilters,
) -> Result<Vec<cartridge::ArtImage>, String> {
    cartridge::fetch_logos(api_key(&state)?, game_id, page, filters).await
}

#[tauri::command]
pub async fn steamgriddb_icons(
    state: State<'_, SharedState>,
    game_id: i32,
    page: i32,
    filters: cartridge::ArtFilters,
) -> Result<Vec<cartridge::ArtImage>, String> {
    cartridge::fetch_icons(api_key(&state)?, game_id, page, filters).await
}
