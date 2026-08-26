mod collections;
mod games;
mod install;

// Cross-platform: cartridge Goldberg injection (#206/#207) needs to pick a
// game's main .exe on whichever OS Tatu itself is running on.
pub(crate) mod exe_pick;

// Both only feed the cheat panel: `exe` resolves the game binary a cheat
// table attaches to, `launch_options` sets WINEDLLOVERRIDES so Proton loads
// the Mono collector. Neither has a meaning on native Windows.
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
pub(crate) use install::{library_paths, steam_install_dir};
#[cfg(unix)]
pub(crate) use launch_options::{LaunchOptOutcome, set_winhttp_override};
