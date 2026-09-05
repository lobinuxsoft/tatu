//! Resolve the main `.exe` of a Steam-installed game by appid.
//!
//! Strategy:
//! 1. If the user wrote an override at `<cheat-tables>/<app_id>/.detected-exe`, use it.
//! 2. Parse `libraryfolders.vdf` → enumerate every Steam library on the system.
//! 3. For each library, look for `steamapps/appmanifest_<app_id>.acf` and parse its
//!    `installdir` field. First match wins.
//! 4. Enumerate `.exe` files under `<library>/steamapps/common/<installdir>/`
//!    up to depth 2. Filter out obvious non-game executables (crash handlers,
//!    redistributables, uninstallers).
//! 5. If exactly one `.exe` remains, take it. Otherwise take the largest.
//! 6. Cache the chosen name so subsequent launches are instant.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use regex::Regex;

use crate::steam::exe_pick::pick_main_exe_in;
use crate::steam::install::library_paths;

const OVERRIDE_FILENAME: &str = ".detected-exe";
const TABLES_SUBDIR: &str = "backlog-tracker/cheat-tables";

pub fn detect_game_exe(app_id: &str) -> Result<String, String> {
    if let Some(exe) = read_override(app_id) {
        return Ok(exe);
    }
    let install_path = find_install_path(app_id)?;
    let chosen = pick_main_exe_in(&install_path, app_id.parse().unwrap_or(0))?;
    let _ = cache_detection(app_id, &chosen);
    Ok(chosen)
}

fn override_path(app_id: &str) -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join(TABLES_SUBDIR)
            .join(app_id)
            .join(OVERRIDE_FILENAME),
    )
}

fn read_override(app_id: &str) -> Option<String> {
    let path = override_path(app_id)?;
    let raw = fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn cache_detection(app_id: &str, exe_name: &str) -> std::io::Result<()> {
    let Some(path) = override_path(app_id) else {
        return Ok(());
    };
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, exe_name)
}

/// Resolve the Steam install directory for an appid. Used by
/// [`detect_game_exe`] to enumerate `.exe` candidates and by
/// `tatu-tracker::prereqs` (#98) to place / inspect REFramework's
/// `dinput8.dll` next to the game exe.
pub fn find_install_path(app_id: &str) -> Result<PathBuf, String> {
    for lib in library_paths() {
        let manifest = lib
            .join("steamapps")
            .join(format!("appmanifest_{app_id}.acf"));
        let Ok(text) = fs::read_to_string(&manifest) else {
            continue;
        };
        let Some(installdir) = parse_installdir(&text) else {
            continue;
        };
        let game_dir = lib.join("steamapps").join("common").join(&installdir);
        if game_dir.is_dir() {
            return Ok(game_dir);
        }
    }
    Err(format!(
        "appmanifest_{app_id}.acf not found in any Steam library"
    ))
}

fn parse_installdir(acf: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""installdir"\s*"([^"]+)""#).expect("static regex"));
    re.captures(acf).map(|c| c[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_installdir_extracts_value() {
        let acf = r#"
            "AppState"
            {
                "appid"		"2725260"
                "installdir"	"EnderMagnolia"
            }
        "#;
        assert_eq!(parse_installdir(acf), Some("EnderMagnolia".to_string()));
    }

    #[test]
    fn parse_installdir_returns_none_when_missing() {
        assert_eq!(parse_installdir("\"AppState\"{}"), None);
    }
}
