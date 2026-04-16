use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use steam_vdf_parser::{Obj, Value, parse_appinfo};

/// How a given size value was obtained.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SizeSource {
    /// Parsed from Steam's libraryfolders.vdf (exact, installed now).
    LocalManifest,
    /// Sum of depot manifest sizes from appcache/appinfo.vdf (covers every
    /// owned app including non-installed ones; may over/underestimate by
    /// including DLC and excluding platform-specific depots).
    Appinfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskSize {
    pub app_id: u64,
    pub bytes: u64,
    pub source: SizeSource,
    pub measured_at: u64,
}

/// Scan every Steam library declared in libraryfolders.vdf and return the
/// exact size on disk for each installed app ID. Trivial, offline.
pub fn scan_installed_sizes(steam_dir: &Path) -> Result<Vec<DiskSize>, String> {
    let vdf_path = steam_dir.join("steamapps").join("libraryfolders.vdf");
    let content = fs::read_to_string(&vdf_path)
        .map_err(|e| format!("Cannot read {}: {e}", vdf_path.display()))?;

    let apps_re = Regex::new(r#""apps"\s*\{([^}]+)\}"#).map_err(|e| e.to_string())?;
    let pair_re = Regex::new(r#""(\d+)"\s*"(\d+)""#).map_err(|e| e.to_string())?;

    let now = now_secs();
    let mut sizes: HashMap<u64, u64> = HashMap::new();

    for apps_match in apps_re.captures_iter(&content) {
        let block = &apps_match[1];
        for pair in pair_re.captures_iter(block) {
            let Ok(id) = pair[1].parse::<u64>() else {
                continue;
            };
            let Ok(size) = pair[2].parse::<u64>() else {
                continue;
            };
            if size == 0 {
                continue;
            }
            let entry = sizes.entry(id).or_insert(0);
            if size > *entry {
                *entry = size;
            }
        }
    }

    Ok(sizes
        .into_iter()
        .map(|(app_id, bytes)| DiskSize {
            app_id,
            bytes,
            source: SizeSource::LocalManifest,
            measured_at: now,
        })
        .collect())
}

/// Parse appcache/appinfo.vdf and estimate the download size of every owned
/// app by summing its depot manifest sizes. Depots are filtered to mimic what
/// a Linux user running Proton would actually download: the global depots
/// (no oslist), Windows and Linux depots, skipping Mac-only and shared
/// install redistributables.
///
/// The returned sizes are an **upper bound** — they include every language
/// depot and most DLC depots because we do not know which of those the user
/// owns. For NAS planning this is conservative (estimate a bit high rather
/// than a bit low).
pub fn scan_appinfo_sizes(steam_dir: &Path) -> Result<Vec<DiskSize>, String> {
    let path = steam_dir.join("appcache").join("appinfo.vdf");
    let bytes = fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

    let vdf = parse_appinfo(&bytes).map_err(|e| format!("appinfo.vdf parse failed: {e}"))?;
    let root = vdf.as_obj().ok_or("appinfo.vdf root is not an object")?;

    let now = now_secs();
    let mut out = Vec::new();

    for (appid_str, app_v) in root.iter() {
        let Ok(app_id) = appid_str.parse::<u64>() else {
            continue;
        };
        let Some(app) = app_v.as_obj() else { continue };
        let Some(appinfo) = app.get("appinfo").and_then(|v| v.as_obj()) else {
            continue;
        };
        let Some(depots) = appinfo.get("depots").and_then(|v| v.as_obj()) else {
            continue;
        };

        let mut total: u64 = 0;
        for (depot_id, depot_v) in depots.iter() {
            if depot_id.parse::<u64>().is_err() {
                continue; // skip meta keys: branches, privatebranches, baselanguages, ...
            }
            let Some(depot) = depot_v.as_obj() else {
                continue;
            };
            if !depot_matches_platform(depot) {
                continue;
            }
            total += public_manifest_size(depot);
        }

        if total > 0 {
            out.push(DiskSize {
                app_id,
                bytes: total,
                source: SizeSource::Appinfo,
                measured_at: now,
            });
        }
    }

    Ok(out)
}

/// Decide whether a depot would be downloaded on a Linux system (possibly
/// running Windows builds through Proton).
///
/// Include when:
/// - `config.oslist` is absent or empty (neutral content / docs / DLC).
/// - `config.oslist` mentions "windows" or "linux".
///
/// Exclude when:
/// - `sharedinstall = 1` (SteamOS/Proton runtimes shared across apps).
/// - `config.oslist` mentions only macos or other non-target platforms.
fn depot_matches_platform(depot: &Obj) -> bool {
    if let Some(v) = depot.get("sharedinstall")
        && let Some(n) = get_u64(v)
        && n == 1
    {
        return false;
    }
    let Some(cfg) = depot.get("config").and_then(|v| v.as_obj()) else {
        return true;
    };
    let Some(oslist) = cfg.get("oslist").and_then(|v| v.as_str()) else {
        return true;
    };
    if oslist.trim().is_empty() {
        return true;
    }
    let l = oslist.to_lowercase();
    l.contains("windows") || l.contains("linux")
}

/// Extract `manifests.public.size` from a depot object.
fn public_manifest_size(depot: &Obj) -> u64 {
    depot
        .get("manifests")
        .and_then(|v| v.as_obj())
        .and_then(|m| m.get("public"))
        .and_then(|v| v.as_obj())
        .and_then(|p| p.get("size"))
        .and_then(get_u64)
        .unwrap_or(0)
}

fn get_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Str(s) => s.parse::<u64>().ok(),
        Value::I32(n) => Some(*n as u64),
        Value::U64(n) => Some(*n),
        _ => None,
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
