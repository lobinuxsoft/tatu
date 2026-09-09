use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NonSteamGame {
    pub id: u64,
    pub name: String,
    pub exe: String,
    pub icon: String,
    pub last_played: u64,
    /// Steam's own "Start In" folder for this shortcut — the closest thing
    /// to an install root a non-Steam entry has, since there's no manifest
    /// to read it from. Used by #236's cartridge copy to know what to copy
    /// (the whole folder, not just the exe's own directory, which may not
    /// hold the game's other files).
    #[serde(default)]
    pub start_dir: String,
    /// User-assigned, not read from `shortcuts.vdf` (Steam has no concept of
    /// this for a shortcut) — set once in the detail window (#328) so a
    /// DRM-free copy that also happens to be sold on Steam can reuse Steam's
    /// own store data (description, screenshots) and SteamGridDB art instead
    /// of shipping with nothing. `sync_nonsteam` re-parses the whole file on
    /// every "Leer shortcuts.vdf" click, so this has to be merged back in by
    /// `id` afterward or it would be wiped on the next sync.
    #[serde(default)]
    pub steam_app_id: Option<u64>,
    /// User-corrected install root (#236, live feedback: `start_dir` above
    /// is Steam's own "Start In", which for a lot of real games is just the
    /// exe's OWN folder — e.g. an Unreal Engine game's
    /// `<Root>/<Game>/Binaries/Win64/`, not `<Root>/<Game>/` — copying only
    /// that leaves every sibling folder (`Content/`, `Engine/`, ...) behind.
    /// `None` means trust `start_dir` as-is; same merge-by-id-across-resync
    /// treatment as `steam_app_id` above.
    #[serde(default)]
    pub install_root_override: Option<String>,
}

impl NonSteamGame {
    /// The folder #236's cartridge copy actually reads from — the user's
    /// correction if they set one, `start_dir` otherwise.
    pub fn install_root(&self) -> &str {
        self.install_root_override
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.start_dir)
    }
}

/// One artwork slot per shortcut Steam actually renders in its own library
/// grid — ported from `capydeploy`'s `ArtworkSelector.svelte` (same author,
/// same SteamGridDB API, intentionally the same 5-slot shape). Empty string
/// means "not picked", not "picked and blank" — a partial pick (say, just a
/// portrait capsule) is a completely normal, valid selection.
///
/// Lives in `AppState::artwork` (a `HashMap<u64, ArtworkSelection>`), keyed
/// by whatever id the source itself uses (Steam appid, GOG product id, or a
/// Non-Steam shortcut id — #328 applies to all three), NOT as a field on
/// `Game`/`GogOwnedGame`/`NonSteamGame` — those three get wholesale
/// replaced on every sync, which would mean re-deriving the same
/// preserve-by-id merge `sync_nonsteam` already needs for `steam_app_id`
/// three separate times over.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ArtworkSelection {
    #[serde(default)]
    pub griddb_game_id: i32,
    #[serde(default)]
    pub grid_portrait: String,
    #[serde(default)]
    pub grid_landscape: String,
    #[serde(default)]
    pub hero: String,
    #[serde(default)]
    pub logo: String,
    #[serde(default)]
    pub icon: String,
}

impl ArtworkSelection {
    pub fn is_empty(&self) -> bool {
        self.grid_portrait.is_empty()
            && self.grid_landscape.is_empty()
            && self.hero.is_empty()
            && self.logo.is_empty()
            && self.icon.is_empty()
    }
}

/// Parse Steam's binary shortcuts.vdf to extract non-Steam game entries.
pub fn parse_shortcuts() -> Result<Vec<NonSteamGame>, String> {
    let path = shortcuts_path().ok_or("Could not find shortcuts.vdf")?;
    let data = std::fs::read(&path).map_err(|e| format!("Failed to read shortcuts.vdf: {e}"))?;

    let mut games = Vec::new();
    let mut i = 0;
    let len = data.len();

    // Each entry starts after a section opener (0x00 + index + 0x00).
    // We scan for AppName fields and collect surrounding fields.
    while i < len {
        // Look for AppName key (0x01 "AppName" 0x00 value 0x00).
        if i + 9 < len && data[i] == 0x01 {
            let key = read_cstring(&data, i + 1);
            if let Some((key_str, after_key)) = key
                && (key_str == "AppName" || key_str == "appname")
                && let Some((name, after_val)) = read_cstring(&data, after_key)
            {
                // Found a game entry. Now scan nearby bytes for other fields.
                let search_start = i.saturating_sub(200);
                let search_end = (after_val + 500).min(len);
                let region = &data[search_start..search_end];
                let _offset = i - search_start;

                let appid = find_int_field(region, "appid").unwrap_or(0) as u64;
                let exe = unquote(find_string_field(region, "Exe").unwrap_or_default());
                let icon = find_string_field(region, "icon").unwrap_or_default();
                let last_played = find_int_field(region, "LastPlayTime").unwrap_or(0) as u64;
                let start_dir = unquote(find_string_field(region, "StartDir").unwrap_or_default());

                if !name.is_empty() {
                    games.push(NonSteamGame {
                        id: appid,
                        name,
                        exe,
                        icon,
                        last_played,
                        start_dir,
                        steam_app_id: None,
                        install_root_override: None,
                    });
                }

                i = after_val;
                continue;
            }
        }
        i += 1;
    }

    games.sort_by_key(|g| g.name.to_lowercase());
    Ok(games)
}

/// Steam's own "Add a Non-Steam Game" dialog wraps `Exe` (and sometimes
/// `StartDir`) in literal double-quote characters INSIDE the VDF string
/// value itself — confirmed live against a real `shortcuts.vdf` (Janken.exe:
/// the stored string is `"/home/.../Janken.exe"`, quotes and all). Entries
/// written by other tools (EmuDeck, manual edits) don't quote at all, so
/// this only strips a matching leading+trailing pair, never partial quoting.
fn unquote(s: String) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(str::to_string)
        .unwrap_or(s)
}

fn shortcuts_path() -> Option<PathBuf> {
    // Was hardcoded to ~/.local/share/Steam; going through the shared
    // resolver is what makes non-Steam shortcuts work on Windows and on the
    // ~/.steam/steam layout alike.
    let steam_dir = crate::steam::steam_install_dir()?.join("userdata");
    if !steam_dir.exists() {
        return None;
    }

    // Find first user directory containing shortcuts.vdf.
    if let Ok(entries) = std::fs::read_dir(&steam_dir) {
        for entry in entries.flatten() {
            let path = entry.path().join("config/shortcuts.vdf");
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

/// Read a null-terminated C string starting at `pos`.
/// Returns (string, position_after_null).
fn read_cstring(data: &[u8], pos: usize) -> Option<(String, usize)> {
    let end = data[pos..].iter().position(|&b| b == 0)?;
    let s = String::from_utf8_lossy(&data[pos..pos + end]).to_string();
    Some((s, pos + end + 1))
}

/// Find a string field (type 0x01) with the given key name in a region.
fn find_string_field(region: &[u8], key: &str) -> Option<String> {
    let key_bytes = key.as_bytes();
    for i in 0..region.len().saturating_sub(key_bytes.len() + 3) {
        if region[i] == 0x01 && region[i + 1..].starts_with(key_bytes) {
            let after_key = i + 1 + key_bytes.len();
            if after_key < region.len() && region[after_key] == 0x00 {
                return read_cstring(region, after_key + 1).map(|(s, _)| s);
            }
        }
    }
    None
}

/// Find an int32 field (type 0x02) with the given key name in a region.
fn find_int_field(region: &[u8], key: &str) -> Option<u32> {
    let key_bytes = key.as_bytes();
    for i in 0..region.len().saturating_sub(key_bytes.len() + 6) {
        if region[i] == 0x02 && region[i + 1..].starts_with(key_bytes) {
            let after_key = i + 1 + key_bytes.len();
            if after_key < region.len() && region[after_key] == 0x00 {
                let val_start = after_key + 1;
                if val_start + 4 <= region.len() {
                    return Some(u32::from_le_bytes([
                        region[val_start],
                        region[val_start + 1],
                        region[val_start + 2],
                        region[val_start + 3],
                    ]));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_strips_a_matching_pair() {
        assert_eq!(
            unquote("\"/home/x/game.exe\"".to_string()),
            "/home/x/game.exe"
        );
    }

    #[test]
    fn unquote_leaves_an_unquoted_path_alone() {
        assert_eq!(unquote("/home/x/game.exe".to_string()), "/home/x/game.exe");
    }

    #[test]
    fn unquote_leaves_a_lone_leading_quote_alone() {
        // No matching trailing quote — not a quoted string, don't touch it.
        assert_eq!(
            unquote("\"/home/x/game.exe".to_string()),
            "\"/home/x/game.exe"
        );
    }
}
