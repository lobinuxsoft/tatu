mod collections;
mod games;
mod install;

// Cross-platform: cartridge Goldberg injection (#206/#207) needs to pick a
// game's main .exe on whichever OS Tatu itself is running on.
pub(crate) mod exe_pick;

// `exe` resolves the game binary a cheat table attaches to; `launch_options`
// edits a game's Steam launch options — WINEDLLOVERRIDES for the Mono
// collector and OptiScaler, GE-Proton's upgrade switches for the PC tab.
// Neither has a meaning on native Windows.
#[cfg(unix)]
pub(crate) mod exe;
#[cfg(unix)]
mod launch_options;

pub use collections::{
    SteamCollection, find_steam_collection_by_name, get_steam_favorites, list_steam_collections,
};
#[cfg(unix)]
pub(crate) use exe::detect_game_exe;
pub(crate) use exe_pick::pick_main_exe_in;
pub use games::{Game, fetch_details_for, fetch_games, fetch_single_detail};
pub use install::detect_steam_id;
pub(crate) use install::{find_install_path, library_paths, steam_install_dir};
#[cfg(unix)]
pub(crate) use launch_options::{
    AddOverride, LaunchOptError, LaunchOptOutcome, add_override, compat_tool, current_account,
    force_proton_compat, get_env, is_steam_running, override_mode, read_app_launch_options,
    remove_override, set_env, set_winhttp_override, stop_steam_for_config_edit,
    update_app_launch_options,
};
