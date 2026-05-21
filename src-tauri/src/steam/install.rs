use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use regex::Regex;

/// Locate the user's Steam client install directory.
/// Exposed as `pub(crate)` so sibling modules (disk, collections) can reuse
/// it without re-implementing the platform-specific discovery.
pub(crate) fn steam_install_dir() -> Option<PathBuf> {
    // Linux: ~/.local/share/Steam or ~/.steam/steam
    if let Some(home) = dirs::home_dir() {
        let primary = home.join(".local/share/Steam");
        if primary.exists() {
            return Some(primary);
        }
        let alt = home.join(".steam/steam");
        if alt.exists() {
            return Some(alt);
        }
    }
    // Windows: C:\Program Files (x86)\Steam
    #[cfg(target_os = "windows")]
    {
        let win_path = PathBuf::from(r"C:\Program Files (x86)\Steam");
        if win_path.exists() {
            return Some(win_path);
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
pub(crate) fn library_paths() -> Vec<PathBuf> {
    let Some(steam) = steam_install_dir() else {
        return Vec::new();
    };
    let vdf = steam.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = fs::read_to_string(&vdf) else {
        return Vec::new();
    };
    parse_library_paths(&content)
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Pure helper kept separate for tests. Extracts every `"path"` value
/// from a `libraryfolders.vdf` blob. Windows-style `\\` separators
/// are normalised to forward slashes so callers can join them with
/// `Path::join` portably.
pub(crate) fn parse_library_paths(content: &str) -> Vec<String> {
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
}
