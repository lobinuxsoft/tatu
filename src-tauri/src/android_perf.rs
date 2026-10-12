// Saved performance sessions ("Medir rendimiento", GameNative tab): one JSON
// file per session under `<config>/backlog-tracker/performance/<source>_<id>/`,
// NOT in state.json — that file is rewritten whole (plus 5 backups) on every
// save, and a session carries a sample per second. Named by epoch ms (an
// RFC 3339 name has ':', which Windows rejects). Paths are only ever built
// here from (source, id, session id), never taken from the frontend.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tatu_android::perf::{Sample, Summary};

use crate::android::GameNativeConfig;

const SOURCES: &[&str] = &["steam", "non_steam"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfGame {
    /// "steam" or "non_steam", same as the rest of the GameNative tab.
    pub source: String,
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfDevice {
    pub serial: String,
    pub model: String,
}

/// Everything but the samples — what the sessions list reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfSessionHeader {
    /// Epoch ms when recording started; also the file name.
    pub id: u64,
    pub game: PerfGame,
    #[serde(default)]
    pub label: String,
    /// The handheld's performance mode / TDP setting, typed by the user —
    /// Android exposes no TDP to read.
    #[serde(default)]
    pub tdp_note: String,
    pub device: PerfDevice,
    /// The GameNative config this run used: prefilled from the game's saved
    /// best config, editable per session — comparing sessions is comparing
    /// these.
    #[serde(default)]
    pub config: Option<GameNativeConfig>,
    pub summary: Summary,
    /// Why recording ended early, if it did.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfSession {
    #[serde(flatten)]
    pub header: PerfSessionHeader,
    #[serde(default)]
    pub samples: Vec<Sample>,
}

fn game_dir(source: &str, id: u64) -> Result<PathBuf, String> {
    if !SOURCES.contains(&source) {
        return Err(format!("Origen desconocido: {source}"));
    }
    Ok(dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("backlog-tracker")
        .join("performance")
        .join(format!("{source}_{id}")))
}

fn session_path(source: &str, id: u64, session: u64) -> Result<PathBuf, String> {
    Ok(game_dir(source, id)?.join(format!("{session}.json")))
}

/// Temp file + rename, same as `state.json`: a crash mid-write never leaves
/// a half-written session behind.
pub fn save(session: &PerfSession) -> Result<(), String> {
    let h = &session.header;
    let path = session_path(&h.game.source, h.game.id, h.id)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let data = serde_json::to_string(session).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load(source: &str, id: u64, session: u64) -> Result<PerfSession, String> {
    let path = session_path(source, id, session)?;
    let data = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&data).map_err(|e| format!("{}: {e}", path.display()))
}

/// Newest first. An unreadable file is skipped, not fatal — one corrupt
/// session must not hide the others.
pub fn list(source: &str, id: u64) -> Result<Vec<PerfSessionHeader>, String> {
    let dir = game_dir(source, id)?;
    let mut headers: Vec<PerfSessionHeader> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    headers.sort_by_key(|h| std::cmp::Reverse(h.id));
    Ok(headers)
}

pub fn delete(source: &str, id: u64, session: u64) -> Result<(), String> {
    let path = session_path(source, id, session)?;
    fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tatu_android::perf::Sample;

    #[test]
    fn the_list_reads_headers_and_paths_stay_inside() {
        let session = PerfSession {
            header: PerfSessionHeader {
                id: 1_758_000_000_000,
                game: PerfGame {
                    source: "steam".into(),
                    id: 10,
                    name: "Hades".into(),
                },
                label: "DXVK async".into(),
                tdp_note: String::new(),
                device: PerfDevice {
                    serial: "192.168.1.20:5555".into(),
                    model: "Odin 2".into(),
                },
                config: None,
                summary: Summary::default(),
                error: None,
            },
            samples: vec![Sample::default(); 3],
        };
        let json = serde_json::to_string(&session).unwrap();
        let header: PerfSessionHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(header.label, "DXVK async");
        let back: PerfSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.samples.len(), 3);

        assert!(game_dir("../../etc", 1).is_err());
        assert!(
            session_path("steam", 10, 1)
                .unwrap()
                .ends_with("steam_10/1.json")
        );
    }
}
