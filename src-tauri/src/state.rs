use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::achievements::GameAchievements;
use crate::disk::DiskSize;
use crate::drm::DrmInfo;
use crate::hltb::HltbResult;
use crate::inventory::GameCards;
use crate::shortcuts::NonSteamGame;
use crate::steam::Game;

/// How many numbered backups of state.json to keep (state.json.1 .. state.json.N).
const STATE_BACKUP_COUNT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    pub games: Vec<Game>,
    pub completed: HashSet<u64>,
    pub completed_nonsteam: HashSet<u64>,
    pub last_sync: Option<String>,
    #[serde(default)]
    pub non_steam: Vec<NonSteamGame>,
    #[serde(default)]
    pub steam_api_key: String,
    #[serde(default)]
    pub steam_id: String,
    #[serde(default)]
    pub achievement_cache: HashMap<u64, GameAchievements>,
    #[serde(default)]
    pub cards_cache: HashMap<u64, GameCards>,
    /// HLTB duration cache, keyed by Steam app ID.
    #[serde(default)]
    pub hltb_cache: HashMap<u64, HltbResult>,
    /// DRM classification cache, keyed by Steam app ID.
    #[serde(default)]
    pub drm_cache: HashMap<u64, DrmInfo>,
    /// Disk size cache (installed + previously measured), keyed by Steam app ID.
    #[serde(default)]
    pub size_cache: HashMap<u64, DiskSize>,
}

impl AppState {
    /// Where `state.json` actually lives on this platform. The UI used to
    /// print a hardcoded `~/.config/backlog-tracker/`, which is a lie on
    /// Windows.
    pub fn display_path() -> String {
        Self::path().display().to_string()
    }

    fn path() -> PathBuf {
        let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = config.join("backlog-tracker");
        let _ = fs::create_dir_all(&dir);
        dir.join("state.json")
    }

    /// Path for the Nth numbered backup: state.json.1, state.json.2, ...
    fn backup_path(n: usize) -> PathBuf {
        let base = Self::path();
        let mut s = base.into_os_string();
        s.push(format!(".{n}"));
        PathBuf::from(s)
    }

    /// Load state from disk, falling back through the numbered backup chain
    /// if the primary file is missing or unparseable. Writes the recovered
    /// state back to the primary path (without rotating) so later saves do
    /// not immediately promote a corrupt primary into the backup slots.
    pub fn load() -> Self {
        let primary = Self::path();

        match Self::try_read(&primary) {
            Ok(state) => return state,
            Err(e) => {
                if primary.exists() {
                    eprintln!(
                        "state: primary {} unreadable/corrupt: {e} — trying backups",
                        primary.display()
                    );
                }
            }
        }

        for n in 1..=STATE_BACKUP_COUNT {
            let bp = Self::backup_path(n);
            if !bp.exists() {
                continue;
            }
            match Self::try_read(&bp) {
                Ok(state) => {
                    eprintln!("state: recovered from {}", bp.display());
                    state.write_atomic_to_primary();
                    return state;
                }
                Err(e) => {
                    eprintln!("state: backup {} unreadable: {e}", bp.display());
                }
            }
        }

        Self::default()
    }

    /// Persist state to disk. Rotates the existing (parseable) primary file
    /// through state.json.1 .. state.json.N before writing the new primary
    /// atomically via a temp file + rename. Skipped when the serialized
    /// content is identical to the current primary to avoid burning backup
    /// slots on no-op saves (e.g. settings screen writes that change nothing).
    pub fn save(&self) {
        let path = Self::path();

        let new_data = match serde_json::to_string(self) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("state: serialize failed: {e}");
                return;
            }
        };

        if let Ok(existing) = fs::read_to_string(&path)
            && existing == new_data
        {
            return;
        }

        if path.exists() {
            if Self::try_read(&path).is_ok() {
                rotate_backups();
                let _ = fs::rename(&path, Self::backup_path(1));
            } else {
                // Primary exists but is corrupt — do not promote it into the
                // backup chain; just delete so the new write can proceed.
                let _ = fs::remove_file(&path);
            }
        }

        write_atomic(&path, &new_data);
    }

    fn write_atomic_to_primary(&self) {
        let path = Self::path();
        if let Ok(data) = serde_json::to_string(self) {
            write_atomic(&path, &data);
        }
    }

    fn try_read(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }
}

/// Shift backups down by one slot:
/// state.json.N-1 → state.json.N, ..., state.json.1 → state.json.2.
/// The oldest slot (state.json.N) is discarded.
fn rotate_backups() {
    let _ = fs::remove_file(AppState::backup_path(STATE_BACKUP_COUNT));
    for n in (1..STATE_BACKUP_COUNT).rev() {
        let src = AppState::backup_path(n);
        let dst = AppState::backup_path(n + 1);
        if src.exists() {
            let _ = fs::rename(&src, &dst);
        }
    }
}

/// Write `data` to `path` atomically: serialize to a sibling temp file,
/// then rename over the destination. On POSIX and NTFS rename is atomic,
/// so a crash mid-write can never leave a half-written primary file.
fn write_atomic(path: &Path, data: &str) {
    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);

    if let Err(e) = fs::write(&tmp, data) {
        eprintln!("state: write to tmp {} failed: {e}", tmp.display());
        return;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        eprintln!(
            "state: rename {} -> {} failed: {e}",
            tmp.display(),
            path.display()
        );
        let _ = fs::remove_file(&tmp);
    }
}
