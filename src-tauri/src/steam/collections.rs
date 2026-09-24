use serde::{Deserialize, Serialize};

use super::install::steam_install_dir;

/// A Steam user-defined collection (favorites, hidden, custom group).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamCollection {
    pub key: String,
    pub name: String,
    pub added: Vec<u64>,
}

/// Read all user-defined Steam collections from the local cloud storage file.
/// Converts the 64-bit Steam ID to a 32-bit account ID to locate the
/// correct userdata folder, then parses the cloud storage JSON.
pub fn list_steam_collections(steam_id: &str) -> Result<Vec<SteamCollection>, String> {
    let steam_dir = steam_install_dir().ok_or("Steam install directory not found")?;
    let id64: u64 = steam_id.parse().map_err(|_| "Invalid Steam ID format")?;
    // Convert SteamID64 to account ID (userdata folder name).
    let account_id = id64 - 76561197960265728;

    let cloud_path = steam_dir
        .join("userdata")
        .join(account_id.to_string())
        .join("config/cloudstorage/cloud-storage-namespace-1.json");

    let content = std::fs::read_to_string(&cloud_path)
        .map_err(|e| format!("Cannot read cloud storage: {e}"))?;

    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&content).map_err(|e| format!("Cannot parse cloud storage: {e}"))?;

    let mut out: Vec<SteamCollection> = Vec::new();

    for entry in &entries {
        // Each entry is a 2-element array: [key_string, object].
        let arr = match entry.as_array() {
            Some(a) if a.len() == 2 => a,
            _ => continue,
        };
        let obj = match arr[1].as_object() {
            Some(o) => o,
            None => continue,
        };
        let key = obj.get("key").and_then(|v| v.as_str()).unwrap_or("");
        if !key.starts_with("user-collections.") {
            continue;
        }
        let value_str = obj.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let value: serde_json::Value = match serde_json::from_str(value_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let added: Vec<u64> = value
            .get("added")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();

        out.push(SteamCollection {
            key: key.to_string(),
            name,
            added,
        });
    }

    Ok(out)
}

/// Read the user's Steam favorites via the generic collection reader.
/// Favorites are stored under the fixed key `user-collections.favorite`.
pub fn get_steam_favorites(steam_id: &str) -> Result<Vec<u64>, String> {
    let collections = list_steam_collections(steam_id)?;
    Ok(collections
        .into_iter()
        .find(|c| c.key == "user-collections.favorite")
        .map(|c| c.added)
        .unwrap_or_default())
}

/// Find a Steam collection by its display name (case-insensitive match).
pub fn find_steam_collection_by_name(
    steam_id: &str,
    name: &str,
) -> Result<Option<SteamCollection>, String> {
    let target = name.trim().to_lowercase();
    let collections = list_steam_collections(steam_id)?;
    Ok(collections
        .into_iter()
        .find(|c| c.name.trim().to_lowercase() == target))
}
