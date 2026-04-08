use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NonSteamGame {
    pub id: u64,
    pub name: String,
    pub exe: String,
    pub icon: String,
    pub last_played: u64,
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
            if let Some((key_str, after_key)) = key {
                if key_str == "AppName" || key_str == "appname" {
                    if let Some((name, after_val)) = read_cstring(&data, after_key) {
                        // Found a game entry. Now scan nearby bytes for other fields.
                        let search_start = if i > 200 { i - 200 } else { 0 };
                        let search_end = (after_val + 500).min(len);
                        let region = &data[search_start..search_end];
                        let offset = i - search_start;

                        let appid = find_int_field(region, "appid").unwrap_or(0) as u64;
                        let exe = find_string_field(region, "Exe").unwrap_or_default();
                        let icon = find_string_field(region, "icon").unwrap_or_default();
                        let last_played = find_int_field(region, "LastPlayTime").unwrap_or(0) as u64;

                        if !name.is_empty() {
                            games.push(NonSteamGame {
                                id: appid,
                                name,
                                exe,
                                icon,
                                last_played,
                            });
                        }

                        i = after_val;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

fn shortcuts_path() -> Option<PathBuf> {
    let steam_dir = dirs::home_dir()?.join(".local/share/Steam/userdata");
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
