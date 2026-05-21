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
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::steam::install::library_paths;

const OVERRIDE_FILENAME: &str = ".detected-exe";
const TABLES_SUBDIR: &str = "backlog-tracker/cheat-tables";
const NON_GAME_NEEDLES: &[&str] = &[
    "unitycrash",
    "easyanti",
    "easyanticheat",
    "vc_redist",
    "vcredist",
    "directx",
    "dxsetup",
    "redist",
    "crashhandler",
    "crashpad",
    "uninstall",
    "installshield",
    "setup.exe",
];

pub fn detect_game_exe(app_id: &str) -> Result<String, String> {
    if let Some(exe) = read_override(app_id) {
        return Ok(exe);
    }
    let install_path = find_install_path(app_id)?;
    // UE games bury the shipping exe at <Game>/Binaries/Win64/<Game>-Win64-Shipping.exe
    // (depth 3 from install root). Depth 5 leaves margin for engines that nest deeper.
    let exes = enumerate_exes(&install_path, 5);
    if exes.is_empty() {
        return Err(format!("no .exe files under {}", install_path.display()));
    }
    let chosen = pick_main_exe(&exes).ok_or_else(|| {
        format!(
            "could not select a main .exe under {}",
            install_path.display()
        )
    })?;
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

/// Resolve `<library>/steamapps/common/<installdir>/` for `app_id`
/// by walking every Steam library + parsing its `appmanifest_*.acf`.
/// Returns the first existing directory; errors with a human-readable
/// message when no library owns the appid (game uninstalled, wrong
/// id, …).
pub(crate) fn find_install_path(app_id: &str) -> Result<PathBuf, String> {
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

fn enumerate_exes(root: &Path, max_depth: usize) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    walk_collect(root, 0, max_depth, &mut out);
    out
}

fn walk_collect(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() && depth < max_depth {
            walk_collect(&path, depth + 1, max_depth, out);
        } else if ft.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((path, size));
        }
    }
}

fn pick_main_exe(exes: &[(PathBuf, u64)]) -> Option<String> {
    let filtered: Vec<&(PathBuf, u64)> = exes.iter().filter(|(p, _)| !is_non_game_exe(p)).collect();
    let pool = if filtered.is_empty() {
        exes.iter().collect::<Vec<_>>()
    } else {
        filtered
    };
    if pool.len() == 1 {
        return pool[0].0.file_name()?.to_str().map(String::from);
    }
    pool.iter()
        .max_by_key(|(_, size)| *size)
        .and_then(|(p, _)| p.file_name())
        .and_then(|n| n.to_str())
        .map(String::from)
}

fn is_non_game_exe(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_lowercase();
    NON_GAME_NEEDLES.iter().any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

    #[test]
    fn is_non_game_exe_flags_common_redistributables() {
        assert!(is_non_game_exe(Path::new("UnityCrashHandler64.exe")));
        assert!(is_non_game_exe(Path::new("foo/VC_redist.x64.exe")));
        assert!(is_non_game_exe(Path::new("dxsetup.exe")));
        assert!(is_non_game_exe(Path::new("EasyAntiCheat_Setup.exe")));
        assert!(!is_non_game_exe(Path::new("Game-Win64-Shipping.exe")));
        assert!(!is_non_game_exe(Path::new("Pragmata.exe")));
    }

    #[test]
    fn pick_main_exe_returns_single_when_only_one_remains() {
        let exes = vec![
            (PathBuf::from("/g/Game.exe"), 100_000_000),
            (PathBuf::from("/g/UnityCrashHandler64.exe"), 5_000_000),
        ];
        assert_eq!(pick_main_exe(&exes), Some("Game.exe".to_string()));
    }

    #[test]
    fn pick_main_exe_returns_largest_when_multiple_remain() {
        let exes = vec![
            (PathBuf::from("/g/Launcher.exe"), 10_000_000),
            (PathBuf::from("/g/Game-Win64-Shipping.exe"), 500_000_000),
        ];
        assert_eq!(
            pick_main_exe(&exes),
            Some("Game-Win64-Shipping.exe".to_string())
        );
    }

    #[test]
    fn pick_main_exe_falls_back_when_all_filtered() {
        let exes = vec![
            (PathBuf::from("/g/UnityCrashHandler64.exe"), 5_000_000),
            (PathBuf::from("/g/vc_redist.x64.exe"), 25_000_000),
        ];
        assert_eq!(pick_main_exe(&exes), Some("vc_redist.x64.exe".to_string()));
    }

    #[test]
    fn pick_main_exe_returns_none_for_empty_input() {
        assert_eq!(pick_main_exe(&[]), None);
    }

    #[test]
    fn enumerate_exes_walks_depth_limited() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(root.join("Game.exe"), vec![0u8; 1024]).unwrap();
        fs::create_dir_all(root.join("Engine/Binaries/Win64")).unwrap();
        fs::write(root.join("Engine/Binaries/Win64/Deep.exe"), vec![0u8; 128]).unwrap();
        fs::create_dir_all(root.join("Tools")).unwrap();
        fs::write(root.join("Tools/Helper.exe"), vec![0u8; 256]).unwrap();

        let mut found: Vec<String> = enumerate_exes(root, 2)
            .into_iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, vec!["Game.exe", "Helper.exe"]);
    }
}
