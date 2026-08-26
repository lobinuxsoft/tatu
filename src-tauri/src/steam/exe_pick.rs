//! Pick the main `.exe` out of an already-known install directory.
//!
//! Pure `std::fs` walking + a size/name heuristic — no unix-only API, unlike
//! the rest of `steam::exe` (which resolves cheat-table targets and is
//! gated `#[cfg(unix)]`, see its own doc comment). Cartridge Goldberg
//! injection (#206/#207) needs this on every platform Tatu itself runs on,
//! so it lives here instead.

use std::path::{Path, PathBuf};

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

/// Resolve the main `.exe` filename under an already-known install
/// directory. Reused by cartridge Goldberg injection (#206/#207), which
/// already has the cartridge's own install dir, and by `steam::exe`'s
/// `detect_game_exe` for the cheat-table feature's own install dir.
pub(crate) fn pick_main_exe_in(install_dir: &Path) -> Result<String, String> {
    // UE games bury the shipping exe at <Game>/Binaries/Win64/<Game>-Win64-Shipping.exe
    // (depth 3 from install root). Depth 5 leaves margin for engines that nest deeper.
    let exes = enumerate_exes(install_dir, 5);
    if exes.is_empty() {
        return Err(format!("no .exe files under {}", install_dir.display()));
    }
    pick_main_exe(&exes).ok_or_else(|| {
        format!(
            "could not select a main .exe under {}",
            install_dir.display()
        )
    })
}

fn enumerate_exes(root: &Path, max_depth: usize) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    walk_collect(root, 0, max_depth, &mut out);
    out
}

fn walk_collect(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
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

    #[test]
    fn pick_main_exe_in_errors_when_no_exe_exists() {
        let tmp = TempDir::new().unwrap();
        let err = pick_main_exe_in(tmp.path()).unwrap_err();
        assert!(err.contains("no .exe files"));
    }
}
