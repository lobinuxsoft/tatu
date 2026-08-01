mod collections;
pub(crate) mod exe;
mod games;
mod install;
mod launch_options;

pub use collections::{
    SteamCollection, find_steam_collection_by_name, get_steam_favorites, list_steam_collections,
};
pub(crate) use exe::detect_game_exe;
pub use games::{Game, fetch_details_for, fetch_games, fetch_single_detail};
pub use install::detect_steam_id;
pub(crate) use install::steam_install_dir;
pub(crate) use launch_options::{LaunchOptOutcome, set_winhttp_override};
