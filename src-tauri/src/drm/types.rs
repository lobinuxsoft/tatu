use serde::{Deserialize, Serialize};

/// High-level DRM classification for a Steam title, from the perspective of
/// a copy purchased on Steam.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrmStatus {
    DrmFree,
    SteamOnly,
    ThirdParty { vendors: Vec<String> },
    Unknown,
}

/// Preservability level: how feasible is it to keep a playable copy of the
/// game independent of Steam?
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preservability {
    /// No DRM: copying the install folder is enough.
    Trivial,
    /// Only Steam wrapper DRM: Goldberg Steam Emu + Steamless cover it.
    Easy,
    /// Game is sold DRM-free on GOG (official legal alternative).
    Alternative,
    /// Publisher removed the DRM post-launch: the current Steam release is
    /// already preservable without extra tools.
    Removed { removed_vendors: Vec<String> },
    /// Third-party DRM active without a documented clean path.
    Hard,
    /// Insufficient data.
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrmInfo {
    pub status: DrmStatus,
    pub notes: String,
    pub source: String,
    pub fetched_at: u64,
    /// Whether the detected DRM affects the user's Steam-purchased copy.
    /// False for `DrmFree`; true for `SteamOnly` and `ThirdParty`.
    /// `Unknown` is conservatively reported as false.
    #[serde(default)]
    pub affects_steam_copy: bool,
    /// Human-readable explanation (Spanish) about Steam copy impact.
    #[serde(default)]
    pub explanation: String,
    /// Preservability classification (Goldberg compatibility, GOG alt, DRM removal).
    #[serde(default)]
    pub preservability: Preservability,
    /// Human-readable hint (Spanish) for the preservability level.
    #[serde(default)]
    pub preservability_hint: String,
    /// Raw PCGamingWiki Available_from tokens (stores), retained so the
    /// classifier can be re-run offline without a fresh API call.
    #[serde(default)]
    pub stores: Vec<String>,
    /// Raw PCGamingWiki Removed_DRM tokens (DRMs the publisher removed
    /// post-launch), retained for re-classification and user visibility.
    #[serde(default)]
    pub removed_drm: Vec<String>,
}
