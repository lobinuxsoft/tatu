use std::path::{Path, PathBuf};

/// The Flatpak's home (`~/.var/app/<id>`): mounted over `~` inside its
/// sandbox, so paths Steam records there are under this on the host.
const FLATPAK_HOME: &str = ".var/app/com.valvesoftware.Steam";

/// Where Steam lives on Linux, by how it was installed: the distro package
/// (`~/.steam/steam` on older setups), the Flatpak, the Snap.
#[cfg(unix)]
const LINUX_STEAM_DIRS: [&str; 4] = [
    ".local/share/Steam",
    ".steam/steam",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    "snap/steam/common/.local/share/Steam",
];

/// Locate the user's Steam client install directory.
/// Exposed as `pub(crate)` so sibling modules (disk, collections) can reuse
/// it without re-implementing the platform-specific discovery.
pub(crate) fn steam_install_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        if let Some(home) = dirs::home_dir() {
            return pick_steam_dir(LINUX_STEAM_DIRS.iter().map(|dir| home.join(dir)));
        }
    }
    #[cfg(windows)]
    {
        // The installer writes the real path to the registry, and it is often
        // not on C: — Steam is routinely moved to a second drive. Guessing
        // Program Files first would find a leftover empty dir on those setups.
        if let Some(from_registry) = windows_steam_path_from_registry()
            && from_registry.exists()
        {
            return Some(from_registry);
        }
        let default = PathBuf::from(r"C:\Program Files (x86)\Steam");
        if default.exists() {
            return Some(default);
        }
    }
    None
}

/// Of the Steam folders that exist, the one in use: whose account login
/// (`config/loginusers.vdf`) is the most recent. A folder left behind by a
/// Steam replaced with the Flatpak — or the other way around — loses; on a
/// tie (`~/.steam/steam` is usually a link to the first) the earlier wins.
#[cfg_attr(not(unix), allow(dead_code))]
fn pick_steam_dir(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let mut best: Option<(Option<std::time::SystemTime>, PathBuf)> = None;
    for dir in candidates.into_iter().filter(|dir| dir.exists()) {
        let login = dir
            .join("config/loginusers.vdf")
            .metadata()
            .and_then(|m| m.modified())
            .ok();
        if best.as_ref().is_none_or(|(newest, _)| login > *newest) {
            best = Some((login, dir));
        }
    }
    best.map(|(_, dir)| dir)
}

/// Flatpak Steam's paths as the host sees them. Whatever it recorded under
/// `~` meant its sandbox's home — unless the user gave the Flatpak that host
/// folder, which then exists as recorded. `homes`: the real home and the
/// Flatpak's, when `steam` is the Flatpak.
fn host_path(recorded: &str, homes: Option<&(PathBuf, PathBuf)>) -> PathBuf {
    let path = PathBuf::from(recorded);
    if let Some((home, sandbox)) = homes
        && !path.starts_with(sandbox)
        && let Ok(rest) = path.strip_prefix(home)
    {
        let mapped = sandbox.join(rest);
        if mapped.exists() {
            return mapped;
        }
    }
    path
}

/// The real home and the Flatpak's, when `steam` is Flatpak Steam — also
/// when reached through a link (`~/.steam/steam` pointing into it).
fn flatpak_homes(steam: &Path) -> Option<(PathBuf, PathBuf)> {
    let home = dirs::home_dir()?;
    let sandbox = home.join(FLATPAK_HOME);
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    real(steam)
        .starts_with(real(&sandbox))
        .then_some((home, sandbox))
}

/// Read `SteamPath` from the registry. HKCU is written per user by the
/// client itself; HKLM `InstallPath` is the machine-wide installer value and
/// only differs when several accounts share one install.
#[cfg(windows)]
fn windows_steam_path_from_registry() -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let candidates = [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ];

    for (hive, subkey, value) in candidates {
        if let Ok(key) = RegKey::predef(hive).open_subkey(subkey)
            && let Ok(path) = key.get_value::<String, _>(value)
            && !path.is_empty()
        {
            // SteamPath uses forward slashes; PathBuf handles both on Windows.
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Detect the Steam ID of the most recently used account by parsing the
/// local Steam client's `config/loginusers.vdf`. Returns the 17-digit
/// SteamID64 string or None if the file is missing / no Timestamp found.
pub fn detect_steam_id() -> Option<String> {
    let vdf_path = steam_install_dir()?.join("config/loginusers.vdf");
    let content = std::fs::read_to_string(&vdf_path).ok()?;

    let mut best_id: Option<String> = None;
    let mut best_timestamp: u64 = 0;
    let mut current_id: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim().trim_matches('"');
        // Steam IDs are 17-digit numbers starting with 7656119.
        if trimmed.len() == 17 && trimmed.starts_with("7656119") && trimmed.parse::<u64>().is_ok() {
            current_id = Some(trimmed.to_string());
        }
        if let Some(ref id) = current_id
            && (line.contains("\"Timestamp\"") || line.contains("\"timestamp\""))
        {
            let ts: u64 = line
                .split('"')
                .filter(|s| s.parse::<u64>().is_ok())
                .filter_map(|s| s.parse().ok())
                .next()
                .unwrap_or(0);
            if ts > best_timestamp {
                best_timestamp = ts;
                best_id = Some(id.clone());
            }
        }
    }

    // If only one account and no timestamp matched, return it.
    best_id.or(current_id)
}

/// Enumerate every Steam library on the system by parsing
/// `libraryfolders.vdf` under the resolved Steam install directory.
/// Returns the libraries in declaration order; the first one is
/// always Steam's primary install dir (Valve always emits index 0
/// for the canonical location).
///
/// Failure modes folded into an empty vec: Steam not installed, the
/// VDF missing or unreadable, no `"path"` entries inside.
///
/// `steam::exe` (Linux-only, cheat panel) and `cartridge` (#195, both
/// platforms) are the consumers — no platform gate here.
pub(crate) fn library_paths() -> Vec<PathBuf> {
    use std::fs;

    let Some(steam) = steam_install_dir() else {
        return Vec::new();
    };
    let vdf = steam.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = fs::read_to_string(&vdf) else {
        return Vec::new();
    };
    let homes = flatpak_homes(&steam);
    parse_library_paths(&content)
        .iter()
        .map(|path| host_path(path, homes.as_ref()))
        .collect()
}

/// Resolve the Steam install directory for an appid:
/// `<library>/steamapps/common/<installdir>`, whichever library's
/// `appmanifest_<app_id>.acf` claims it. Used by `exe::detect_game_exe`
/// and the cheat prereqs (#98, `dinput8.dll` next to the game exe) — and
/// by "Enviar a Android", which is why it lives here, cross-platform, and
/// not in the Linux-only `exe` module anymore.
pub(crate) fn find_install_path(app_id: &str) -> Result<PathBuf, String> {
    for lib in library_paths() {
        let manifest = lib
            .join("steamapps")
            .join(format!("appmanifest_{app_id}.acf"));
        let Ok(text) = std::fs::read_to_string(&manifest) else {
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
    use regex::Regex;
    use std::sync::OnceLock;

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""installdir"\s*"([^"]+)""#).expect("static regex"));
    re.captures(acf).map(|c| c[1].to_string())
}

/// Pure helper kept separate for tests. Extracts every `"path"` value
/// from a `libraryfolders.vdf` blob. Windows-style `\\` separators
/// are normalised to forward slashes so callers can join them with
/// `Path::join` portably.
pub(crate) fn parse_library_paths(content: &str) -> Vec<String> {
    use regex::Regex;
    use std::sync::OnceLock;

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""path"\s*"([^"]+)""#).expect("static regex"));
    re.captures_iter(content)
        .map(|c| c[1].replace("\\\\", "/"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_library_paths_extracts_unix_paths() {
        let vdf = r#"
            "libraryfolders"
            {
                "0"
                {
                    "path"		"/home/user/.local/share/Steam"
                }
                "1"
                {
                    "path"		"/run/media/user/SSD/SteamLibrary"
                }
            }
        "#;
        assert_eq!(
            parse_library_paths(vdf),
            vec![
                "/home/user/.local/share/Steam".to_string(),
                "/run/media/user/SSD/SteamLibrary".to_string()
            ]
        );
    }

    #[test]
    fn parse_library_paths_normalises_windows_escapes() {
        let vdf = r#""path"		"C:\\Program Files (x86)\\Steam""#;
        assert_eq!(
            parse_library_paths(vdf),
            vec!["C:/Program Files (x86)/Steam".to_string()]
        );
    }

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
    fn the_steam_folder_with_the_latest_login_wins() {
        use std::time::{Duration, SystemTime};
        let tmp = tempfile::tempdir().unwrap();
        let login = |dir: &str, age: u64| {
            let config = tmp.path().join(dir).join("config");
            std::fs::create_dir_all(&config).unwrap();
            let file = std::fs::File::create(config.join("loginusers.vdf")).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        };
        login("native", 3600);
        login("flatpak", 10);
        let pick = |dirs: &[&str]| {
            pick_steam_dir(dirs.iter().map(|d| tmp.path().join(d)))
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        };
        assert_eq!(
            pick(&["native", "missing", "flatpak"]).as_deref(),
            Some("flatpak")
        );
        assert_eq!(pick(&["missing", "native"]).as_deref(), Some("native"));
        assert_eq!(pick(&["missing"]), None);
    }

    #[test]
    fn flatpak_paths_under_home_are_its_sandbox() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let sandbox = home.join(FLATPAK_HOME);
        std::fs::create_dir_all(sandbox.join(".local/share/Steam")).unwrap();
        std::fs::create_dir_all(home.join("Games/SteamLibrary")).unwrap();
        let homes = (home.clone(), sandbox.clone());
        let steam = home.join(".local/share/Steam");
        assert_eq!(
            host_path(&steam.to_string_lossy(), Some(&homes)),
            sandbox.join(".local/share/Steam")
        );
        // A host folder the user shared with the Flatpak stays as recorded.
        let shared = home.join("Games/SteamLibrary");
        assert_eq!(host_path(&shared.to_string_lossy(), Some(&homes)), shared);
        // Not the Flatpak: nothing is translated.
        assert_eq!(host_path(&steam.to_string_lossy(), None), steam);
    }
}
