mod classify;
mod hints;
mod sources;
mod types;
mod vendors;

pub use types::{DrmInfo, DrmStatus};

use classify::{RawDrm, merge};
use sources::{fetch_from_pcgamingwiki, fetch_from_steam};

/// Fetch DRM information for a Steam app ID, querying Steam Store and
/// PCGamingWiki and merging the results with a Steam-copy-centric heuristic.
pub fn fetch_drm_info(app_id: u64) -> Result<DrmInfo, String> {
    let mut raw = RawDrm::default();

    if let Some((notice, account)) = fetch_from_steam(app_id) {
        raw.steam_drm_notice = notice;
        raw.steam_account_notice = account;
        raw.steam_store_ok = true;
    }

    if let Some(pcgw) = fetch_from_pcgamingwiki(app_id) {
        raw.pcgw_stores = pcgw.stores;
        raw.pcgw_uses = pcgw.uses;
        raw.pcgw_removed = pcgw.removed;
        raw.pcgw_retail = pcgw.retail;
        raw.pcgw_has_entry = pcgw.has_entry;
        raw.pcgw_ok = true;
    }

    if !raw.steam_store_ok && !raw.pcgw_ok {
        return Err("Both Steam Store and PCGamingWiki requests failed".into());
    }

    Ok(merge(raw, now_secs()))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
