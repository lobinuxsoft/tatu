use serde::{Deserialize, Serialize};

const PCGW_API: &str = "https://www.pcgamingwiki.com/w/api.php";
const STEAM_APPDETAILS: &str = "https://store.steampowered.com/api/appdetails";
const USER_AGENT: &str =
    "game-progress-tracker (+https://github.com/lobinuxsoft/game-progress-tracker)";

/// High-level DRM classification for a Steam title.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrmStatus {
    DrmFree,
    SteamOnly,
    ThirdParty { vendors: Vec<String> },
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrmInfo {
    pub status: DrmStatus,
    pub notes: String,
    pub source: String,
    pub fetched_at: u64,
}

/// Raw data fetched from a single upstream source before merging.
#[derive(Debug, Default, Clone)]
struct RawDrm {
    pcgw_uses: Vec<String>,
    pcgw_removed: Vec<String>,
    pcgw_retail: Vec<String>,
    steam_drm_notice: Option<String>,
    steam_account_notice: Option<String>,
    steam_store_ok: bool,
    pcgw_ok: bool,
    pcgw_has_entry: bool,
}

/// Fetch DRM information for a Steam app ID, querying Steam Store and
/// PCGamingWiki and merging the results.
pub fn fetch_drm_info(app_id: u64) -> Result<DrmInfo, String> {
    let mut raw = RawDrm::default();

    if let Some((notice, account)) = fetch_from_steam(app_id) {
        raw.steam_drm_notice = notice;
        raw.steam_account_notice = account;
        raw.steam_store_ok = true;
    }

    if let Some(pcgw) = fetch_from_pcgamingwiki(app_id) {
        raw.pcgw_uses = pcgw.uses;
        raw.pcgw_removed = pcgw.removed;
        raw.pcgw_retail = pcgw.retail;
        raw.pcgw_has_entry = pcgw.has_entry;
        raw.pcgw_ok = true;
    }

    if !raw.steam_store_ok && !raw.pcgw_ok {
        return Err("Both Steam Store and PCGamingWiki requests failed".into());
    }

    Ok(merge(raw))
}

/// Query Steam Store appdetails and extract DRM-related fields.
fn fetch_from_steam(app_id: u64) -> Option<(Option<String>, Option<String>)> {
    let url = format!("{STEAM_APPDETAILS}?appids={app_id}&l=english");
    let body: serde_json::Value = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;

    let data = body.get(app_id.to_string())?.get("data")?;
    let notice = data
        .get("drm_notice")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let account = data
        .get("ext_user_account_notice")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some((notice, account))
}

struct PcgwDrm {
    uses: Vec<String>,
    removed: Vec<String>,
    retail: Vec<String>,
    has_entry: bool,
}

/// Query PCGamingWiki's Cargo API joining Infobox_game with Availability to
/// pull Uses_DRM / Removed_DRM / Retail_DRM for the game matching the Steam app ID.
fn fetch_from_pcgamingwiki(app_id: u64) -> Option<PcgwDrm> {
    let url = format!(
        "{PCGW_API}?action=cargoquery\
         &tables=Infobox_game,Availability\
         &join_on=Infobox_game._pageName=Availability._pageName\
         &fields=Availability.Uses_DRM=UsesDRM,Availability.Removed_DRM=RemovedDRM,Availability.Retail_DRM=RetailDRM\
         &where=Infobox_game.Steam_AppID%20HOLDS%20%22{app_id}%22\
         &format=json"
    );

    let body: serde_json::Value = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;

    let rows = body.get("cargoquery")?.as_array()?;
    let has_entry = !rows.is_empty();
    let mut uses: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut retail: Vec<String> = Vec::new();

    for row in rows {
        let Some(title) = row.get("title") else {
            continue;
        };
        collect_csv(&mut uses, title.get("UsesDRM"));
        collect_csv(&mut removed, title.get("RemovedDRM"));
        collect_csv(&mut retail, title.get("RetailDRM"));
    }

    dedup_keep_order(&mut uses);
    dedup_keep_order(&mut removed);
    dedup_keep_order(&mut retail);

    Some(PcgwDrm {
        uses,
        removed,
        retail,
        has_entry,
    })
}

fn collect_csv(out: &mut Vec<String>, value: Option<&serde_json::Value>) {
    let Some(s) = value.and_then(|v| v.as_str()) else {
        return;
    };
    for part in s.split(',') {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
    }
}

fn dedup_keep_order(items: &mut Vec<String>) {
    let mut seen: Vec<String> = Vec::new();
    items.retain(|x| {
        let lower = x.to_lowercase();
        if seen.contains(&lower) {
            false
        } else {
            seen.push(lower);
            true
        }
    });
}

/// Merge Steam Store + PCGamingWiki raw data into a final DrmInfo.
/// Priority: PCGamingWiki is authoritative when present; Steam Store fills gaps.
fn merge(raw: RawDrm) -> DrmInfo {
    let mut vendors: Vec<String> = Vec::new();
    let mut notes_parts: Vec<String> = Vec::new();

    // Steam Store notices always contribute to vendor detection.
    if let Some(ref notice) = raw.steam_drm_notice {
        add_vendors(&mut vendors, &detect_vendors(notice));
        notes_parts.push(format!("Steam: {notice}"));
    }
    if let Some(ref notice) = raw.steam_account_notice {
        add_vendors(&mut vendors, &detect_vendors(notice));
        notes_parts.push(format!("Account required: {notice}"));
    }

    // PCGamingWiki Uses_DRM contributes when values are non-Steam and non-DRM-free.
    for token in &raw.pcgw_uses {
        let lower = token.to_lowercase();
        if lower == "drm-free" || lower == "drm free" {
            continue;
        }
        if lower == "steam" {
            continue;
        }
        add_vendors(&mut vendors, &detect_vendors(token));
    }

    if !raw.pcgw_uses.is_empty() {
        notes_parts.push(format!("PCGW Uses: {}", raw.pcgw_uses.join(", ")));
    }
    if !raw.pcgw_removed.is_empty() {
        notes_parts.push(format!("PCGW Removed: {}", raw.pcgw_removed.join(", ")));
    }

    // Classification.
    let pcgw_says_drm_free = pcgw_is_drm_free(&raw.pcgw_uses, &raw.pcgw_retail);
    let pcgw_has_steam = raw.pcgw_uses.iter().any(|t| t.to_lowercase() == "steam");

    let status = if !vendors.is_empty() {
        DrmStatus::ThirdParty { vendors }
    } else if pcgw_says_drm_free {
        DrmStatus::DrmFree
    } else if pcgw_has_steam || raw.steam_drm_notice.is_some() || raw.steam_account_notice.is_some()
    {
        // If PCGamingWiki lists Steam (or Steam Store reported any DRM-ish notice
        // that did not match a known vendor), treat as SteamOnly.
        DrmStatus::SteamOnly
    } else if raw.pcgw_has_entry && raw.pcgw_uses.is_empty() && raw.pcgw_retail.is_empty() {
        // Entry exists but Availability table is empty: insufficient data.
        DrmStatus::Unknown
    } else {
        DrmStatus::Unknown
    };

    let source = match (raw.pcgw_ok && raw.pcgw_has_entry, raw.steam_store_ok) {
        (true, true) => "merged",
        (true, false) => "pcgamingwiki",
        (false, true) => "steam",
        _ => "none",
    };

    DrmInfo {
        status,
        notes: notes_parts.join(" | "),
        source: source.into(),
        fetched_at: now_secs(),
    }
}

/// Determine if PCGamingWiki data indicates the game is DRM-free across all stores.
/// Requires at least one "DRM-free" token and no non-Steam DRM vendors.
fn pcgw_is_drm_free(uses: &[String], retail: &[String]) -> bool {
    let any_drm_free = uses
        .iter()
        .chain(retail.iter())
        .any(|t| matches!(t.to_lowercase().as_str(), "drm-free" | "drm free"));

    if !any_drm_free {
        return false;
    }

    // Any token that is not Steam, not DRM-free, and not just a store name blocks classification.
    for t in uses.iter() {
        let lower = t.to_lowercase();
        if lower == "drm-free" || lower == "drm free" || lower == "steam" {
            continue;
        }
        if !detect_vendors(t).is_empty() {
            return false;
        }
        // Unknown token: be conservative.
        return false;
    }
    true
}

fn add_vendors(dst: &mut Vec<String>, src: &[String]) {
    for v in src {
        if !dst.contains(v) {
            dst.push(v.clone());
        }
    }
}

/// Detect known third-party DRM vendors from a free-form text snippet.
fn detect_vendors(text: &str) -> Vec<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("denuvo", "Denuvo"),
        ("ubisoft connect", "Ubisoft Connect"),
        ("uplay", "Ubisoft Connect"),
        ("ea app", "EA App"),
        ("ea desktop", "EA App"),
        ("origin", "EA App"),
        ("rockstar games social club", "Rockstar Launcher"),
        ("rockstar games launcher", "Rockstar Launcher"),
        ("social club", "Rockstar Launcher"),
        ("epic games launcher", "Epic Games"),
        ("epic games", "Epic Games"),
        ("battle.net", "Battle.net"),
        ("microsoft store", "Microsoft Store"),
        ("games for windows live", "GFWL"),
        ("gfwl", "GFWL"),
        ("bethesda.net", "Bethesda.net"),
        ("securom", "SecuROM"),
        ("starforce", "StarForce"),
        ("tages", "TAGES"),
        ("vmprotect", "VMProtect"),
        ("safedisc", "SafeDisc"),
        ("arxan", "Arxan"),
    ];

    let t = text.to_lowercase();
    let mut out = Vec::new();
    for (needle, label) in PATTERNS {
        if t.contains(needle) {
            let label_s = label.to_string();
            if !out.contains(&label_s) {
                out.push(label_s);
            }
        }
    }
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
