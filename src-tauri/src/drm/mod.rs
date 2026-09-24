mod classify;
mod gog;
mod hints;
mod sources;
mod types;
mod vendors;

// find_install_path (steam::exe) that this depends on isn't ported to
// Windows yet — same story as cartridge::format/symlinks. Windows just
// keeps relying on the network sources alone until that lands.
#[cfg(unix)]
mod probe;

pub use types::{DrmInfo, DrmStatus, Preservability};

use classify::{RawDrm, merge, merge_epic};
use gog::upgrade_if_on_gog;
#[cfg(unix)]
use probe::upgrade_from_installed_files;
pub use sources::login_pcgw;
use sources::{fetch_from_pcgamingwiki, fetch_from_pcgamingwiki_by_title, fetch_from_steam};

/// Fetch DRM information for a Steam app ID, querying Steam Store and
/// PCGamingWiki and merging the results with a Steam-copy-centric heuristic.
/// `pcgw_agent` is `None` when the user hasn't set up PCGamingWiki
/// credentials yet (see `login_pcgw`) — PCGW is skipped rather than
/// treated as an error, same as any other source that comes back empty.
pub fn fetch_drm_info(app_id: u64, pcgw_agent: Option<&ureq::Agent>) -> Result<DrmInfo, String> {
    let mut raw = RawDrm::default();
    let mut steam_name = None;

    if let Some(steam) = fetch_from_steam(app_id) {
        raw.steam_drm_notice = steam.drm_notice;
        raw.steam_account_notice = steam.account_notice;
        raw.steam_store_ok = true;
        steam_name = steam.name;
    }

    if let Some(agent) = pcgw_agent
        && let Some(pcgw) = fetch_from_pcgamingwiki(agent, app_id)
    {
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

    let info = merge(raw, now_secs());
    // A network Unknown isn't necessarily a dead end — if the game is
    // already installed locally, its own files can settle it (#238).
    #[cfg(unix)]
    let info = upgrade_from_installed_files(app_id, info);
    // Still Unknown? PCGamingWiki not having an entry says nothing about
    // whether the game is actually sold DRM-free on GOG — ask GOG's own
    // catalog directly (#237).
    let info = match steam_name {
        Some(name) => upgrade_if_on_gog(&name, info),
        None => info,
    };
    Ok(info)
}

/// Fetch DRM information for an EGS game's own release, by title (#345) —
/// EGS games have no Steam AppID to query Steam Store or PCGW's
/// `Steam_AppID`-keyed lookup with, so this only ever talks to PCGamingWiki,
/// keyed by the wiki page title. `pcgw_agent` is `None` under the exact same
/// circumstances `fetch_drm_info` treats the same way — PCGW is skipped,
/// not an error.
pub fn fetch_epic_drm_info(
    title: &str,
    pcgw_agent: Option<&ureq::Agent>,
) -> Result<DrmInfo, String> {
    let mut raw = RawDrm::default();

    if let Some(agent) = pcgw_agent
        && let Some(pcgw) = fetch_from_pcgamingwiki_by_title(agent, title)
    {
        raw.pcgw_stores = pcgw.stores;
        raw.pcgw_uses = pcgw.uses;
        raw.pcgw_removed = pcgw.removed;
        raw.pcgw_retail = pcgw.retail;
        raw.pcgw_has_entry = pcgw.has_entry;
        raw.pcgw_ok = true;
    }

    if !raw.pcgw_ok {
        return Err(
            "PCGamingWiki request failed (no bot credentials, or the request itself failed)".into(),
        );
    }

    Ok(merge_epic(raw, now_secs()))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real PCGamingWiki query against a real page title, reusing Tatu's
    /// own stored bot credentials from `state.json` — same file
    /// `AppState::path()` resolves. `#[ignore]` because it depends on real,
    /// live network access and a configured PCGW bot password (#345).
    ///
    /// Exists specifically to verify the by-title Cargo query works at all
    /// — `fetch_from_pcgamingwiki_by_title` is a new query shape untested
    /// against the real API before this (the existing Steam path only ever
    /// queried by AppID). A raw `curl` probe of the same login flow hit a
    /// Cloudflare JS challenge (curl's TLS/HTTP fingerprint gets flagged as
    /// a bot); going through the real `ureq`-based agent this module
    /// already uses successfully for Steam's own DRM classification is the
    /// actual code path that matters, not a hand-rolled curl script.
    #[test]
    #[ignore]
    fn fetches_epic_drm_for_a_real_title() {
        let home = std::env::var("HOME").expect("HOME not set");
        let state_path = format!("{home}/.config/backlog-tracker/state.json");
        let state_json = std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("cannot read {state_path}: {e}"));
        let state: serde_json::Value =
            serde_json::from_str(&state_json).expect("state.json is not valid JSON");
        let username = state["pcgw_username"].as_str().expect(
            "no pcgw_username in state.json — configure PCGamingWiki bot credentials first",
        );
        let password = state["pcgw_bot_password"]
            .as_str()
            .expect("no pcgw_bot_password in state.json");

        let agent = login_pcgw(username, password).expect("PCGW login failed");

        // Batman: Arkham Knight — a real owned EGS game (per #345's own
        // live session), DRM-free on every store per PCGamingWiki.
        let pcgw = fetch_from_pcgamingwiki_by_title(&agent, "Batman: Arkham Knight")
            .expect("fetch_from_pcgamingwiki_by_title returned nothing");
        assert!(
            pcgw.has_entry,
            "expected a PCGW entry for Batman: Arkham Knight"
        );
        assert!(
            pcgw.stores
                .iter()
                .any(|s| s.to_lowercase().contains("epic")),
            "expected an Epic Games row in Stores, got {:?}",
            pcgw.stores
        );

        let info = fetch_epic_drm_info("Batman: Arkham Knight", Some(&agent))
            .expect("fetch_epic_drm_info failed");
        assert_ne!(
            info.status,
            DrmStatus::Unknown,
            "expected a real classification, not Unknown"
        );
    }
}
