mod achievements;
mod commands;
mod disk;
mod drm;
mod hltb;
mod inventory;
mod shortcuts;
mod state;
mod steam;

use std::collections::HashMap;
use std::sync::Mutex;

use cheat_runtime::FreezeRegistry;
use commands::cheat_runtime_cmd::ActiveCheats;
use state::AppState;

pub type SharedState = Mutex<AppState>;

pub fn run() {
    let app_state = AppState::load();
    let active_cheats: ActiveCheats = Mutex::new(HashMap::new());

    // One-shot migration of any legacy cheat-core JSON to the manifest format
    // consumed by `cheat-runtime`. Idempotent: existing manifests are skipped.
    match cheat_runtime::migrate_default_dirs() {
        Ok(report) if !report.migrated.is_empty() || !report.unsupported.is_empty() => {
            eprintln!(
                "[cheat-runtime migrate] migrated={:?} skipped={:?} unsupported={:?}",
                report.migrated, report.skipped, report.unsupported
            );
        }
        Ok(_) => {}
        Err(e) => eprintln!("[cheat-runtime migrate] failed: {e}"),
    }

    // Auto-import every `.ct` table under `cheat-tables/<appid>/` into a
    // manifest at `trainers/<appid>/`. Same idempotency guarantee: only
    // tables without a matching manifest are written. Per-file failures are
    // logged but don't abort startup — one malformed `.ct` mustn't keep the
    // rest of the user's library from showing up.
    match cheat_runtime::auto_import_default_dirs() {
        Ok(report) if !report.created.is_empty() || !report.failed.is_empty() => {
            eprintln!(
                "[cheat-runtime ct-import] created={} skipped={} failed={}",
                report.created.len(),
                report.skipped.len(),
                report.failed.len()
            );
            for (path, err) in &report.failed {
                eprintln!("[cheat-runtime ct-import]   {} -> {err}", path.display());
            }
        }
        Ok(_) => {}
        Err(e) => eprintln!("[cheat-runtime ct-import] failed: {e}"),
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(app_state))
        .manage(FreezeRegistry::new())
        .manage(active_cheats)
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
            commands::ce_cmd::ce_install_status,
            commands::ce_cmd::ce_install_trigger,
            commands::ce_cmd::ce_list_tables_for_game,
            commands::ce_cmd::ce_open_for_game,
            commands::cheat_runtime_cmd::features::cheat_runtime_list_features,
            commands::cheat_runtime_cmd::toggles::cheat_runtime_enable,
            commands::cheat_runtime_cmd::toggles::cheat_runtime_disable,
            commands::cheat_runtime_cmd::orphans::cheat_runtime_orphans_list,
            commands::cheat_runtime_cmd::orphans::cheat_runtime_orphans_restore,
            commands::cheat_runtime_cmd::orphans::cheat_runtime_orphans_dismiss,
            commands::cheat_runtime_cmd::values::cheat_runtime_value_read,
            commands::cheat_runtime_cmd::values::cheat_runtime_value_write,
            commands::cheat_runtime_cmd::values::cheat_runtime_value_freeze,
            commands::cheat_runtime_cmd::import::cheat_runtime_import_ct,
            commands::cheat_runtime_cmd::import::cheat_runtime_remove_ct,
            commands::cheat_search_cmd::open_fearless_search,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
