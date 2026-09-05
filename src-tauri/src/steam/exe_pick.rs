//! Pick the main `.exe` out of an already-known install directory.
//!
//! Pure `std::fs` walking + a size/name heuristic — no unix-only API, unlike
//! the rest of `steam::exe` (which resolves cheat-table targets and is
//! gated `#[cfg(unix)]`, see its own doc comment). Cartridge Goldberg
//! injection (#206/#207) needs this on every platform Tatu itself runs on,
//! so it lives here instead.

use std::path::{Path, PathBuf};

use steam_vdf_parser::{Obj, parse_appinfo};

use super::install::steam_install_dir;

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

/// Resolve the main `.exe` under an already-known install directory,
/// returned as a path relative to it (e.g.
/// `End/Binaries/Win64/ff7remake_.exe`, not just `ff7remake_.exe`) — most
/// UE-style ports have the real shipping binary several folders deep behind
/// a small root-level stub, and the caller needs the full path to actually
/// launch it. Reused by cartridge Goldberg injection (#206/#207), which
/// already has the cartridge's own install dir, and by `steam::exe`'s
/// `detect_game_exe` for the cheat-table feature's own install dir.
///
/// Checks Steam's own `appcache/appinfo.vdf` first — the exact data behind
/// the "Play" button, so no guessing "is this Launcher.exe skippable" or
/// "which of two bundled games did the user mean" the way the size/name
/// heuristic below has to. `app_id` is 0 for callers with no meaningful
/// Steam appid (e.g. a GOG install), which this just never matches, falling
/// straight through to the heuristic. Falls through too whenever Steam
/// isn't installed on the preparing machine, this app was never cached
/// locally, or its recorded executable doesn't actually exist under
/// `install_dir` (a stale/relocated depot) — the heuristic is the fallback
/// of last resort, not `appinfo.vdf`.
pub(crate) fn pick_main_exe_in(install_dir: &Path, app_id: u64) -> Result<String, String> {
    if app_id != 0
        && let Some(relative) = exe_from_appinfo(app_id)
        && install_dir.join(&relative).is_file()
    {
        return Ok(relative);
    }

    // UE games bury the shipping exe at <Game>/Binaries/Win64/<Game>-Win64-Shipping.exe
    // (depth 3 from install root). Depth 5 leaves margin for engines that nest deeper.
    let exes = enumerate_exes(install_dir, 5);
    if exes.is_empty() {
        return Err(format!("no .exe files under {}", install_dir.display()));
    }
    pick_main_exe(install_dir, &exes).ok_or_else(|| {
        format!(
            "could not select a main .exe under {}",
            install_dir.display()
        )
    })
}

/// Reads `<steam_install_dir>/appcache/appinfo.vdf` and returns the
/// `executable` Steam itself would launch for `app_id`, Windows-eligible
/// launch entries only. Live-verified against several titles whose real
/// entry point is a branded launcher, not the biggest/deepest engine exe
/// (issue #273) — `appinfo.vdf`'s answer matched every one of them exactly.
fn exe_from_appinfo(app_id: u64) -> Option<String> {
    let steam_dir = steam_install_dir()?;
    let path = steam_dir.join("appcache").join("appinfo.vdf");
    let bytes = std::fs::read(path).ok()?;
    let vdf = parse_appinfo(&bytes).ok()?;
    let root = vdf.as_obj()?;
    let app = root.get(&app_id.to_string())?.as_obj()?;
    let launch = app
        .get("appinfo")?
        .as_obj()?
        .get("config")?
        .as_obj()?
        .get("launch")?
        .as_obj()?;

    launch
        .iter()
        .filter_map(|(_, entry)| entry.as_obj())
        .find(|entry| launch_entry_is_windows(entry))
        .and_then(|entry| entry.get("executable"))
        .and_then(|v| v.as_str())
        .map(|s| s.replace('\\', "/"))
}

/// Same absent-oslist-means-neutral convention `disk.rs`'s
/// `depot_matches_platform` already uses for depots — a launch entry with
/// no `config.oslist` at all applies to every platform.
fn launch_entry_is_windows(entry: &Obj) -> bool {
    let Some(cfg) = entry.get("config").and_then(|v| v.as_obj()) else {
        return true;
    };
    let Some(oslist) = cfg.get("oslist").and_then(|v| v.as_str()) else {
        return true;
    };
    oslist.trim().is_empty() || oslist.to_lowercase().contains("windows")
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

fn pick_main_exe(root: &Path, exes: &[(PathBuf, u64)]) -> Option<String> {
    let filtered: Vec<&(PathBuf, u64)> = exes.iter().filter(|(p, _)| !is_non_game_exe(p)).collect();
    let pool = if filtered.is_empty() {
        exes.iter().collect::<Vec<_>>()
    } else {
        filtered
    };
    let chosen = if pool.len() == 1 {
        pool[0].0.as_path()
    } else {
        pool.iter().max_by_key(|(_, size)| *size)?.0.as_path()
    };
    // Relative to `root`, not just the file's own basename — a picked exe
    // several folders deep (the common UE shape) needs its full subpath to
    // actually be launchable; `file_name()` alone silently truncated this
    // to a name that doesn't exist at the install root (live-caught on
    // FINAL FANTASY VII REMAKE: chose `End/Binaries/Win64/ff7remake_.exe`
    // correctly by size, then recorded bare `ff7remake_.exe`).
    chosen.strip_prefix(root).ok()?.to_str().map(String::from)
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
        let root = Path::new("/g");
        let exes = vec![
            (PathBuf::from("/g/Game.exe"), 100_000_000),
            (PathBuf::from("/g/UnityCrashHandler64.exe"), 5_000_000),
        ];
        assert_eq!(pick_main_exe(root, &exes), Some("Game.exe".to_string()));
    }

    #[test]
    fn pick_main_exe_returns_largest_when_multiple_remain() {
        let root = Path::new("/g");
        let exes = vec![
            (PathBuf::from("/g/Launcher.exe"), 10_000_000),
            (PathBuf::from("/g/Game-Win64-Shipping.exe"), 500_000_000),
        ];
        assert_eq!(
            pick_main_exe(root, &exes),
            Some("Game-Win64-Shipping.exe".to_string())
        );
    }

    /// Live regression (FINAL FANTASY VII REMAKE): a root-level stub
    /// (`ff7remake.exe`, 325KB) alongside the real shipping binary three
    /// folders deep (`End/Binaries/Win64/ff7remake_.exe`, 96MB). The larger
    /// one wins by size, same as any other case here — the bug was
    /// returning just its basename instead of the subpath needed to find
    /// it again from the install root.
    #[test]
    fn pick_main_exe_keeps_subpath_for_nested_winner() {
        let root = Path::new("/g");
        let exes = vec![
            (PathBuf::from("/g/ff7remake.exe"), 325_448),
            (
                PathBuf::from("/g/End/Binaries/Win64/ff7remake_.exe"),
                96_927_560,
            ),
        ];
        assert_eq!(
            pick_main_exe(root, &exes),
            Some("End/Binaries/Win64/ff7remake_.exe".to_string())
        );
    }

    #[test]
    fn pick_main_exe_falls_back_when_all_filtered() {
        let root = Path::new("/g");
        let exes = vec![
            (PathBuf::from("/g/UnityCrashHandler64.exe"), 5_000_000),
            (PathBuf::from("/g/vc_redist.x64.exe"), 25_000_000),
        ];
        assert_eq!(
            pick_main_exe(root, &exes),
            Some("vc_redist.x64.exe".to_string())
        );
    }

    #[test]
    fn pick_main_exe_returns_none_for_empty_input() {
        assert_eq!(pick_main_exe(Path::new("/g"), &[]), None);
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
        let err = pick_main_exe_in(tmp.path(), 0).unwrap_err();
        assert!(err.contains("no .exe files"));
    }
}

#[cfg(test)]
mod appinfo_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn app_id_zero_never_consults_appinfo() {
        // A GOG install (app_id 0 by convention) must go straight to the
        // heuristic — asserting this without touching the filesystem or
        // Steam at all is what keeps this test hermetic.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Game.exe"), vec![0u8; 1024]).unwrap();
        assert_eq!(pick_main_exe_in(tmp.path(), 0).unwrap(), "Game.exe");
    }

    #[test]
    fn is_windows_treats_absent_oslist_as_universal() {
        use steam_vdf_parser::parse_text;
        // The root key here just plays the role of a launch entry's own
        // numeric index ("0", "1", ...) — its object body is what
        // `launch_entry_is_windows` actually inspects.
        let vdf = parse_text(r#""0" { "config" { } }"#).unwrap();
        let entry = vdf.as_obj().unwrap();
        assert!(launch_entry_is_windows(entry));
    }

    #[test]
    fn is_windows_rejects_macos_only_entries() {
        use steam_vdf_parser::parse_text;
        let vdf = parse_text(r#""0" { "config" { "oslist" "macos" } }"#).unwrap();
        let entry = vdf.as_obj().unwrap();
        assert!(!launch_entry_is_windows(entry));
    }

    // Real Steam + real appinfo.vdf on whichever machine runs this — not
    // part of the normal suite (no Steam install in CI). Run by hand with
    // `cargo test -- --ignored appinfo_matches_live_verified_games` after
    // touching this file's appinfo navigation logic. The expected values
    // are the same ones issue #273 confirmed by hand, one game at a time,
    // before this replaced that manual table entirely.
    #[test]
    #[ignore]
    fn appinfo_matches_live_verified_games() {
        assert_eq!(exe_from_appinfo(377840).as_deref(), Some("FF9_Launcher.exe"));
        assert_eq!(
            exe_from_appinfo(292120).as_deref(),
            Some("FFXiiiLauncher.exe")
        );
        assert_eq!(
            exe_from_appinfo(3837340).as_deref(),
            Some("FFVII_LAUNCHER.exe")
        );
    }
}
