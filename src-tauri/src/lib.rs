mod achievements;
mod commands;
mod disk;
mod drm;
mod hltb;
mod inventory;
mod shortcuts;
mod state;
mod steam;

use std::sync::Mutex;

use cheat_core::freeze::FreezeRegistry;
use state::AppState;

pub type SharedState = Mutex<AppState>;

pub fn run() {
    let app_state = AppState::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(app_state))
        .manage(FreezeRegistry::new())
        .invoke_handler(tauri::generate_handler![
            commands::state_cmd::get_state,
            commands::state_cmd::get_settings,
            commands::state_cmd::save_settings,
            commands::state_cmd::save_completed,
            commands::state_cmd::save_completed_nonsteam,
            commands::sync_cmd::sync_steam,
            commands::sync_cmd::sync_nonsteam,
            commands::sync_cmd::fetch_details,
            commands::detail_cmd::get_game_details,
            commands::detail_cmd::get_game_achievements,
            commands::detail_cmd::get_game_cards,
            commands::detail_cmd::search_hltb,
            commands::drm_cmd::get_game_drm,
            commands::drm_cmd::fetch_all_drm,
            commands::collection_cmd::get_steam_favorites,
            commands::collection_cmd::list_steam_collections,
            commands::collection_cmd::import_completed_from_collection,
            commands::disk_cmd::scan_sizes,
            commands::misc_cmd::detect_steam_id,
            commands::cheat_cmd::cheat_list,
            commands::cheat_cmd::cheat_trigger,
            commands::cheat_cmd::cheat_status,
            commands::cheat_cmd::cheat_freeze_toggle,
            commands::cheat_cmd::cheat_freeze_status,
            commands::ce_cmd::ce_install_status,
            commands::ce_cmd::ce_install_trigger,
            commands::ce_cmd::ce_list_tables_for_game,
            commands::ce_cmd::ce_open_for_game,
            commands::cheat_search_cmd::open_fearless_search,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
