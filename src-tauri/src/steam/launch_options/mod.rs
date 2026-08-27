//! Set a Steam game's launch options by editing `localconfig.vdf`.
//!
//! Used to apply `WINEDLLOVERRIDES=winhttp=n,b` so Proton/Wine loads the Mono
//! collector we drop next to the game exe (see [`crate::prereqs`]). Steam
//! rewrites `localconfig.vdf` on shutdown, so edits only stick while Steam is
//! closed — [`set_winhttp_override`] refuses to run otherwise rather than
//! silently losing the change.

mod override_merge;
mod vdf;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::install::{detect_steam_id, steam_install_dir};

/// SteamID64 of the first account; subtract to get the 32-bit account id that
/// names the `userdata/<id>` folder.
const STEAMID64_BASE: u64 = 76561197960265728;

#[derive(Debug, thiserror::Error)]
pub enum LaunchOptError {
    #[error("Steam is running; close it first (it rewrites localconfig.vdf on exit)")]
    SteamRunning,
    #[error("could not determine the active Steam account")]
    SteamIdNotFound,
    #[error("localconfig.vdf not found at {0}")]
    ConfigNotFound(PathBuf),
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("editing localconfig.vdf: {0}")]
    Vdf(#[from] vdf::VdfError),
}

/// What the edit did, for the UI's toast.
#[derive(Debug, serde::Serialize)]
pub struct LaunchOptOutcome {
    /// `false` when the override was already present (no write performed).
    pub changed: bool,
    /// The launch options before the edit (empty if the app had none).
    pub old_value: String,
    /// The launch options after the edit.
    pub new_value: String,
    /// The localconfig.vdf path that was (or would be) edited.
    pub config_path: String,
}

/// Apply `WINEDLLOVERRIDES=winhttp=n,b` to `app_id`'s launch options, merging
/// into whatever the user already had. No-op (with `changed: false`) if the
/// override is already present. Fails if Steam is running.
pub fn set_winhttp_override(app_id: &str) -> Result<LaunchOptOutcome, LaunchOptError> {
    if is_steam_running() {
        return Err(LaunchOptError::SteamRunning);
    }

    let path = localconfig_path()?;
    let src = fs::read_to_string(&path).map_err(|source| LaunchOptError::Io {
        path: path.clone(),
        source,
    })?;

    let old_value = vdf::read_launch_options(&src, app_id)?.unwrap_or_default();
    let path_str = path.display().to_string();

    let Some(new_value) = override_merge::merge_winhttp(&old_value) else {
        return Ok(LaunchOptOutcome {
            changed: false,
            new_value: old_value.clone(),
            old_value,
            config_path: path_str,
        });
    };

    let edited = vdf::set_launch_options(&src, app_id, &new_value)?;
    write_atomic(&path, edited.as_bytes())?;

    Ok(LaunchOptOutcome {
        changed: true,
        old_value,
        new_value,
        config_path: path_str,
    })
}

/// Steam's built-in experimental Proton branch — present on essentially
/// every Linux Steam install with Steam Play enabled at all, since Steam
/// itself manages/updates it, unlike a specific pinned Proton version the
/// user may or may not have installed.
const FORCE_TOOL_NAME: &str = "proton_experimental";

/// Force `app_id` to install/run under Proton (`config.vdf`'s
/// `CompatToolMapping`) instead of whatever native Linux build Steam would
/// otherwise prefer — the cartridge (#192/#206) always needs the Windows
/// depot, since it has to also work when plugged into a Windows machine
/// (#207) or run through the bundled Proton (#206), neither of which a
/// native Linux build satisfies. Known Valve limitation
/// (ValveSoftware/Proton#6635): forcing this alone does not guarantee the
/// Windows depot downloads — the caller still needs to wipe any existing
/// native install and trigger a fresh one after this returns.
pub fn force_proton_compat(app_id: &str) -> Result<(), LaunchOptError> {
    if is_steam_running() {
        return Err(LaunchOptError::SteamRunning);
    }

    let path = config_vdf_path()?;
    let src = fs::read_to_string(&path).map_err(|source| LaunchOptError::Io {
        path: path.clone(),
        source,
    })?;
    let edited = vdf::set_compat_tool(&src, app_id, FORCE_TOOL_NAME)?;
    write_atomic(&path, edited.as_bytes())
}

/// Resolve `<steam>/config/config.vdf` — the top-level install config, NOT
/// per-account like `localconfig.vdf` below.
fn config_vdf_path() -> Result<PathBuf, LaunchOptError> {
    let steam = steam_install_dir().ok_or(LaunchOptError::SteamIdNotFound)?;
    let path = steam.join("config/config.vdf");
    if !path.is_file() {
        return Err(LaunchOptError::ConfigNotFound(path));
    }
    Ok(path)
}

/// Resolve `<steam>/userdata/<account_id>/config/localconfig.vdf` for the most
/// recently used account.
fn localconfig_path() -> Result<PathBuf, LaunchOptError> {
    let steam = steam_install_dir().ok_or(LaunchOptError::SteamIdNotFound)?;
    let id64: u64 = detect_steam_id()
        .and_then(|s| s.parse().ok())
        .ok_or(LaunchOptError::SteamIdNotFound)?;
    let account_id = id64 - STEAMID64_BASE;
    let path = steam
        .join("userdata")
        .join(account_id.to_string())
        .join("config/localconfig.vdf");
    if !path.is_file() {
        return Err(LaunchOptError::ConfigNotFound(path));
    }
    Ok(path)
}

/// True if the Steam client appears to be running. Scans `/proc/*/comm` for a
/// process named exactly `steam` (the main client reaper). Conservative: any
/// read error is treated as "not running" so the check never wrongly blocks.
fn is_steam_running() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        // Only numeric PID directories.
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let comm = entry.path().join("comm");
        if let Ok(c) = fs::read_to_string(&comm)
            && c.trim() == "steam"
        {
            return true;
        }
    }
    false
}

/// Write `bytes` to `target` atomically (temp sibling + rename).
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), LaunchOptError> {
    let tmp = target.with_extension("vdf.tatu-tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|source| LaunchOptError::Io {
            path: tmp.clone(),
            source,
        })?;
        f.write_all(bytes).map_err(|source| LaunchOptError::Io {
            path: tmp.clone(),
            source,
        })?;
        f.sync_all().map_err(|source| LaunchOptError::Io {
            path: tmp.clone(),
            source,
        })?;
    }
    fs::rename(&tmp, target).map_err(|source| LaunchOptError::Io {
        path: tmp.clone(),
        source,
    })
}
