//! GitHub release fetch for REFramework-nightly.

use serde::Deserialize;

use crate::reframework::ReframeworkError;

const RELEASES_API: &str =
    "https://api.github.com/repos/praydog/REFramework-nightly/releases/latest";
const ASSET_NAME: &str = "REFramework.zip";
const USER_AGENT: &str = concat!("tatu-tracker/", env!("CARGO_PKG_VERSION"));

/// Cap downloaded asset size at 100 MB to keep a hostile / mangled
/// release from filling the disk. Current REFramework.zip is ~13 MB
/// so the headroom is generous.
const DOWNLOAD_CAP_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) struct Release {
    pub tag: String,
    pub asset_url: String,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
}

/// Hit the GitHub releases API and pull the `REFramework.zip` asset
/// metadata out of the latest release. Returns the version tag (for
/// the UI) + the asset URL the caller will then GET directly.
pub(super) fn fetch_latest_release() -> Result<Release, ReframeworkError> {
    let body: ReleaseJson = ureq::get(RELEASES_API)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_json()?;

    let asset = body
        .assets
        .into_iter()
        .find(|a| a.name == ASSET_NAME)
        .ok_or(ReframeworkError::AssetMissing)?;

    Ok(Release {
        tag: body.tag_name,
        asset_url: asset.browser_download_url,
    })
}

/// Download the asset bytes — capped at [`DOWNLOAD_CAP_BYTES`] so a
/// runaway release size cannot exhaust memory or disk.
pub(super) fn download_asset(url: &str) -> Result<Vec<u8>, ReframeworkError> {
    let mut response = ureq::get(url).header("User-Agent", USER_AGENT).call()?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(DOWNLOAD_CAP_BYTES as u64)
        .read_to_vec()?;
    Ok(bytes)
}
