//! Tauri commands over `~/.config/tatu/launcher.toml`.
//!
//! Three primitives:
//!
//! - `launcher_config_get_for_app` — pulls the per-game block (or
//!   the global `default_proton` fallback if no per-game entry
//!   exists) so the frontend can render the current selection.
//! - `launcher_config_set_for_app` — upserts the per-game block.
//!   Atomic write via temp + rename; creates the parent dir on
//!   first call.
//! - `launcher_config_unset_app` — drops the per-game block (post
//!   "Revert to Linux" so the launcher passes through transparently).
//!
//! `launcher_list_protons` enumerates installed Protons via
//! [`crate::steam::list_protons`] so the dropdown does not need the
//! user to type a directory name.

use serde::{Deserialize, Serialize};

use crate::steam::{ProtonInfo, list_protons};
use tatu_launcher::config::{Config, ConfigError, GameConfig};

/// What the frontend sees for a per-game launcher entry. Flattened
/// shape (no `Option<GameConfig>` wrapper) so the JS side does not
/// have to discriminate "no entry" from "entry with defaults".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LauncherGameView {
    /// Currently picked Proton — falls back to the global
    /// `default_proton` when the per-game block doesn't override.
    pub proton: String,
    /// Optional override for the in-Wine target exe name. Empty
    /// string when the launcher should infer from argv.
    #[serde(default)]
    pub target_exe: String,
    /// Whether the launcher actually swaps to the bridge for this
    /// game. The "Switch to Tatu" toggle drives this.
    pub tatu_enabled: bool,
}

#[tauri::command]
pub fn launcher_config_get_for_app(app_id: String) -> Result<LauncherGameView, String> {
    let cfg = Config::load_or_default().map_err(stringify_err)?;
    let game = cfg.game(&app_id).cloned().unwrap_or_default();
    Ok(LauncherGameView {
        proton: game
            .proton
            .clone()
            .unwrap_or_else(|| cfg.default_proton.clone()),
        target_exe: game.target_exe.unwrap_or_default(),
        tatu_enabled: game.tatu_enabled,
    })
}

#[tauri::command]
pub fn launcher_config_set_for_app(
    app_id: String,
    view: LauncherGameView,
) -> Result<(), String> {
    let mut cfg = Config::load_or_default().map_err(stringify_err)?;
    let proton = if view.proton == cfg.default_proton {
        None
    } else {
        Some(view.proton)
    };
    let target_exe = if view.target_exe.is_empty() {
        None
    } else {
        Some(view.target_exe)
    };
    cfg.upsert_game(
        app_id,
        GameConfig {
            proton,
            target_exe,
            tatu_enabled: view.tatu_enabled,
        },
    );
    cfg.save().map_err(stringify_err)
}

#[tauri::command]
pub fn launcher_config_unset_app(app_id: String) -> Result<(), String> {
    let mut cfg = Config::load_or_default().map_err(stringify_err)?;
    cfg.remove_game(&app_id);
    cfg.save().map_err(stringify_err)
}

#[tauri::command]
pub fn launcher_list_protons() -> Vec<ProtonInfo> {
    list_protons()
}

fn stringify_err(e: ConfigError) -> String {
    e.to_string()
}
