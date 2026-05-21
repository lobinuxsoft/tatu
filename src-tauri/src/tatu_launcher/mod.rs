//! Tatu Launcher — Steam compat tool drop-in management.
//!
//! Surfaces the wiring the frontend "Enable Tatu backend" toggle
//! depends on: detecting whether the drop-in is installed under
//! `~/.steam/steam/compatibilitytools.d/tatu-launcher/`, copying a
//! fresh payload into place from the staged build artefacts, and
//! patching `config.vdf` so Steam picks "Tatu Launcher" as the
//! compat tool for a given appid.
//!
//! The module is decoupled from Tauri — every entry point takes
//! plain types and returns `Result<_, TatuLauncherError>`. Tauri
//! commands in [`crate::commands::tatu_launcher_cmd`] convert the
//! errors to `String` for the frontend.

mod compat_map;
mod install;
mod paths;
mod status;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use compat_map::{get_compat_tool_for_app, set_compat_tool_for_app};
pub use install::install_compat_tool;
pub use status::status;

/// Snapshot of the Tatu Launcher install on disk. The frontend uses
/// this both to drive the UI banner and to decide whether to offer
/// a one-click install or just a per-game toggle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TatuLauncherStatus {
    /// Drop-in directory missing or essential files absent.
    NotInstalled,
    /// Drop-in present and parseable. `version` is read from the
    /// shipped `version.txt`; `install_dir` is the directory under
    /// `compatibilitytools.d/` Steam picks up.
    Installed {
        version: String,
        install_dir: PathBuf,
    },
    /// Drop-in present but one of the required files is missing or
    /// unreadable (typical when an older version installed without
    /// `tatu-bridge.exe` got partially upgraded).
    Corrupt { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum TatuLauncherError {
    #[error("Steam install not found under ~/.steam/{{root,steam}} or ~/.local/share/Steam")]
    NoSteam,
    #[error("source drop-in not staged; build via scripts/build-tatu-launcher.sh + scripts/build-tatu-bridge.sh first")]
    NoSource,
    #[error("Steam is running — close it before editing config.vdf (it gets rewritten on exit)")]
    SteamRunning,
    #[error("config.vdf missing CompatToolMapping section: {0}")]
    ConfigVdfShape(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
