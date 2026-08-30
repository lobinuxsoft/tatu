mod achievements;
mod cartridge;
mod commands;
mod disk;
mod drm;
mod gog_account;
mod gog_download;
mod hltb;
mod inventory;
mod shortcuts;
mod state;
mod steam;

// Drops the Mono collector DLL into a Proton prefix — cheat path only.
#[cfg(unix)]
mod prereqs;

use std::sync::Mutex;

#[cfg(unix)]
use std::collections::HashMap;

#[cfg(unix)]
use cheat_runtime::FreezeRegistry;
#[cfg(unix)]
use commands::cheat_runtime_cmd::{ActiveCheats, FrameworkActor};

use state::AppState;
use tauri::WebviewWindowBuilder;
use tauri_plugin_opener::OpenerExt;

pub type SharedState = Mutex<AppState>;

/// The tracker commands every platform gets, plus whatever the caller
/// appends. Written as a macro because `tauri::generate_handler!` needs the
/// literal path list — a `#[cfg]` inside its arguments is never expanded, and
/// keeping two full copies of the list is how they drift apart.
macro_rules! tracker_handler {
    ($($extra:path),* $(,)?) => {
        tauri::generate_handler![
            commands::state_cmd::get_state,
            commands::state_cmd::get_game_context,
            commands::state_cmd::get_settings,
            commands::state_cmd::save_settings,
            commands::state_cmd::save_completed,
            commands::state_cmd::save_completed_nonsteam,
            commands::state_cmd::save_completed_gog,
            commands::sync_cmd::sync_steam,
            commands::sync_cmd::sync_nonsteam,
            commands::sync_cmd::fetch_details,
            commands::detail_cmd::get_game_details,
            commands::detail_cmd::get_game_achievements,
            commands::detail_cmd::get_game_cards,
            commands::detail_cmd::search_hltb,
            commands::drm_cmd::get_game_drm,
            commands::drm_cmd::fetch_all_drm,
            commands::gog_cmd::gog_login_url,
            commands::gog_cmd::gog_is_connected,
            commands::gog_cmd::gog_connect,
            commands::gog_cmd::gog_disconnect,
            commands::gog_cmd::fetch_gog_library,
            commands::gog_cmd::get_gog_game_context,
            commands::gog_cmd::fetch_gog_extra_details,
            commands::gog_cmd::gog_download_game,
            commands::collection_cmd::get_steam_favorites,
            commands::collection_cmd::list_steam_collections,
            commands::collection_cmd::import_completed_from_collection,
            commands::disk_cmd::scan_sizes,
            commands::cartridge_cmd::list_removable_drives,
            commands::cartridge_cmd::has_cartridge_structure,
            commands::cartridge_cmd::is_registered_library,
            commands::cartridge_cmd::get_cartridge_usage,
            commands::cartridge_cmd::trigger_install,
            commands::cartridge_cmd::find_pending_cartridge,
            commands::cartridge_cmd::poll_install_status,
            commands::cartridge_cmd::inject_goldberg,
            commands::cartridge_cmd::fetch_cartridge_art,
            commands::cartridge_cmd::fetch_cartridge_description,
            commands::cartridge_cmd::fetch_cartridge_screenshots,
            commands::cartridge_cmd::fetch_cartridge_trailer,
            commands::cartridge_cmd::bundle_linux_runtime,
            commands::cartridge_cmd::uninstall_from_cartridge,
            commands::cartridge_cmd::list_cartridge_apps,
            commands::cartridge_cmd::install_launcher_binaries,
            commands::cartridge_cmd::refresh_cartridge_drm,
            commands::misc_cmd::detect_steam_id,
            commands::misc_cmd::cheats_supported,
            commands::misc_cmd::state_path,
            commands::window_cmd::open_detail_window,
            commands::window_cmd::detail_target,
            $($extra),*
        ]
    };
}

pub fn run() {
    let app_state = AppState::load();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(app_state))
        .manage(commands::window_cmd::DetailTarget::default());

    #[cfg(unix)]
    let builder = {
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

        // Post-#134: `.ct` files in `cheat-tables/<appid>/` are parsed on
        // demand by `load_manifests_for` (no more JSON sidecars), so the
        // startup auto-import pass was removed. Per-file parse failures now
        // surface at list time via the loader's stderr log + the import
        // command's validation toast.

        builder
            .manage(FreezeRegistry::new())
            .manage(active_cheats)
            .manage(FrameworkActor::spawn())
            .invoke_handler(tracker_handler![
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
                commands::cheat_runtime_cmd::prereqs::cheat_runtime_prereqs_check,
                commands::cheat_runtime_cmd::prereqs::cheat_runtime_prereqs_install,
                commands::cheat_runtime_cmd::prereqs::cheat_runtime_install_mono_collector,
                commands::cheat_runtime_cmd::prereqs::cheat_runtime_set_winhttp_override,
                commands::cheat_search_cmd::open_fearless_search,
                commands::cartridge_cmd::format_as_cartridge,
                commands::cartridge_cmd::mount_cartridge,
                commands::cartridge_cmd::ensure_symlinks,
                commands::cartridge_cmd::force_proton_compat,
            ])
    };

    // No cheat commands are registered here at all, so a stale frontend that
    // still asks for one gets a hard invoke error instead of a toggle that
    // pretends to work.
    #[cfg(not(unix))]
    let builder = builder.invoke_handler(tracker_handler![]);

    builder
        .setup(|app| {
            // The window is declared with `"create": false` in tauri.conf.json so
            // it can be built here with a navigation guard attached. Without it a
            // single external link strands the user: the webview navigates away
            // from the app and there is no back button and no address bar, so the
            // only way out is killing the process.
            let config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .expect("tauri.conf.json declares no window");

            let handle = app.handle().clone();
            WebviewWindowBuilder::from_config(app.handle(), &config)?
                .on_navigation(move |url| {
                    if is_app_url(url) {
                        return true;
                    }
                    // Anything pointing outside the app belongs in the user's
                    // browser. Returning false keeps the webview where it is.
                    if let Err(e) = handle.opener().open_url(url.as_str(), None::<&str>) {
                        eprintln!("[opener] failed to open {url}: {e}");
                    }
                    false
                })
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Whether `url` is the app's own frontend rather than somewhere on the web.
///
/// Tauri serves the bundled assets over a custom protocol whose exact shape
/// differs per platform (`tauri://localhost`, `http://tauri.localhost` on
/// Windows), and `cargo tauri dev` serves over plain localhost — so the check
/// is on scheme and host, never on a single hardcoded origin.
fn is_app_url(url: &tauri::Url) -> bool {
    match url.scheme() {
        "http" | "https" => matches!(
            url.host_str(),
            Some("localhost" | "tauri.localhost" | "127.0.0.1" | "[::1]")
        ),
        // Internal schemes the webview uses for its own bookkeeping. `about:blank`
        // in particular is what a fresh webview starts on.
        "tauri" | "asset" | "ipc" | "about" | "blob" | "data" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_app_url;

    fn url(s: &str) -> tauri::Url {
        s.parse().expect("test url")
    }

    #[test]
    fn app_origins_are_allowed() {
        for s in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html",
            "http://localhost:1420/",
            "http://127.0.0.1:1420/",
            "about:blank",
        ] {
            assert!(is_app_url(&url(s)), "{s} should be treated as the app");
        }
    }

    #[test]
    fn the_web_is_refused() {
        for s in [
            "https://steamcommunity.com/dev/apikey",
            "http://steamcommunity.com/dev/apikey",
            "https://www.gog.com/en/games",
            "https://localhost.evil.com/",
        ] {
            assert!(!is_app_url(&url(s)), "{s} should not navigate in-app");
        }
    }

    /// The A-Z jump list renders `<a href="#g-A">`, which resolves against the
    /// app origin. Refusing it would break in-page navigation.
    #[test]
    fn in_page_anchors_stay() {
        assert!(is_app_url(&url("http://tauri.localhost/#g-A")));
    }
}
