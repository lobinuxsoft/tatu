//! REFramework prerequisite detection + install.
//!
//! praydog's REFramework is a `dinput8.dll` proxy that loads at game
//! startup, neutralises Capcom Anti-Tamper's periodic page-integrity
//! scans and anti-debug syscalls, and exposes a Lua scripting API.
//! Without it, our AOB scans + trampolines crash any RE Engine title
//! within seconds — every PRAGMATA / RE2-8 / MHRise / MHWilds / DD2 /
//! DMC5 / SF6 cheat table calls out the same prerequisite.
//!
//! The release is monolithic since the 2025 refactor: one
//! `REFramework.zip` whose `dinput8.dll` detects the running game's
//! TDB layout at startup and dispatches accordingly, so we do not
//! map appid → asset (every game gets the same payload).
//!
//! [`status`] is cheap: a stat + size check on `<game_dir>/dinput8.dll`
//! plus a check for the `<game_dir>/reframework/` config directory
//! REFramework creates on first run. [`install`] fetches the latest
//! nightly via the GitHub releases JSON API + downloads the asset
//! over HTTPS + extracts the entire archive into the game directory.

mod download;
mod install;
mod status;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use install::install;
pub use status::status;

/// On-disk install state of REFramework for a given game directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReframeworkStatus {
    NotInstalled,
    /// `dinput8.dll` is present and big enough to plausibly be
    /// REFramework. `dll_size_bytes` is surfaced so the frontend can
    /// flag the user when the file looks suspiciously small (rare
    /// failure mode: partial extraction).
    Installed {
        dll_size_bytes: u64,
        has_reframework_dir: bool,
    },
}

/// Result of a successful [`install`] call. `version_tag` is the
/// GitHub release tag (e.g. `nightly-01373-c4b1314…`) so the UI can
/// show what landed.
#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub version_tag: String,
    pub installed_dir: PathBuf,
    pub bytes_extracted: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ReframeworkError {
    #[error("steam game install dir not resolvable: {0}")]
    NoGameDir(String),
    #[error("github api: {0}")]
    Network(Box<ureq::Error>),
    #[error("REFramework.zip asset missing from latest release")]
    AssetMissing,
    #[error("zip extract: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ureq::Error> for ReframeworkError {
    fn from(e: ureq::Error) -> Self {
        Self::Network(Box::new(e))
    }
}
