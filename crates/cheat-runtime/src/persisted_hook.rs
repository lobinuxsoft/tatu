//! On-disk snapshot of an active cheat so the runtime can recover hooks
//! that survived a tracker crash.
//!
//! ## Why
//!
//! When the user enables an AA toggle, `Engine::enable` writes a trampoline
//! into the game's `.text` and stores the bytes it overwrote in the
//! returned `ActiveCheat.undo`. If the tracker exits cleanly, `disable()`
//! replays that undo log and the game's memory ends up byte-identical to
//! before. But if the tracker process dies between those two events
//! (crash, force-quit, OOM, OS reboot), the in-memory undo log is lost
//! while the trampoline still lives in the game. The next time the
//! tracker comes back up, the user is forced to relaunch the game
//! manually — only a fresh PE load restores the original bytes.
//!
//! ## What this module does
//!
//! After every successful `enable`, the caller serialises the active
//! cheat into a JSON file at
//! `~/.config/backlog-tracker/active-hooks/<app_id>__<feature_uuid>.json`.
//! After every successful `disable`, the caller deletes that file.
//!
//! At tracker startup, the Tauri layer walks the directory and offers to
//! re-open each surviving record as an `ActiveCheat` whose `.disable()`
//! reverts the trampoline using the same atomic POKEDATA path the live
//! disable would have used. Records whose PID is no longer alive — the
//! common case when the user already closed the game — are deleted
//! silently, since the kernel reclaimed those pages on process exit.
//!
//! The file payload is intentionally minimal: addresses + bytes are all
//! the rollback needs. Symbols aren't persisted because by the time we
//! restore we want to forget them entirely (a fresh enable scan picks
//! them up again).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nix::unistd::Pid;
use serde::{Deserialize, Serialize};

const PERSIST_SUBDIR: &str = "backlog-tracker/active-hooks";

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid persisted-hook JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
}

/// One persisted hook record. Stored as JSON on disk; never accessed
/// directly by the engine — round-trip via [`PersistedHook::write`] /
/// [`PersistedHook::load_all`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedHook {
    pub app_id: String,
    pub feature_uuid: String,
    /// Best-effort identification of the game process. We only use the PID
    /// as a liveness probe (`/proc/<pid>/`); when the binary at the same
    /// PID changed, recovery is skipped — the user must re-enable from
    /// scratch.
    pub pid: i32,
    pub exe: String,
    /// ISO-8601 timestamp the enable completed at. Surface-only; the
    /// recovery code doesn't act on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Every byte block the cheat overwrote, in apply order. Reverting
    /// walks this list in reverse.
    pub writes: Vec<PersistedWrite>,
    /// Codecaves allocated by the enable. Reverting calls
    /// [`crate::alloc::dealloc_remote`] for each one.
    pub allocs: Vec<PersistedAlloc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedWrite {
    /// Address in the target process where the bytes were written.
    pub addr: u64,
    /// Original bytes the cheat overwrote — what the disable pass
    /// restores.
    #[serde(with = "hex_string")]
    pub original: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedAlloc {
    pub symbol: String,
    pub addr: u64,
    pub size: usize,
}

impl PersistedHook {
    /// Resolve the on-disk path for this record.
    pub fn path(&self) -> Result<PathBuf, PersistError> {
        let dir = persist_dir()?;
        Ok(dir.join(format!("{}__{}.json", self.app_id, self.feature_uuid)))
    }

    /// Serialise + atomically write to disk. Creates the directory if it
    /// doesn't exist. Caller is the Tauri command layer right after a
    /// successful `Engine::enable`.
    pub fn write(&self) -> Result<PathBuf, PersistError> {
        let target = self.path()?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| PersistError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self).map_err(|source| PersistError::Json {
            path: target.clone(),
            source,
        })?;
        fs::write(&target, body).map_err(|source| PersistError::Io {
            path: target.clone(),
            source,
        })?;
        Ok(target)
    }

    /// Delete the record at the canonical path. Idempotent — missing
    /// files are not an error, since `disable` runs after the file may
    /// have already been removed by a startup cleanup.
    pub fn delete(&self) -> Result<(), PersistError> {
        let target = self.path()?;
        match fs::remove_file(&target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(PersistError::Io {
                path: target,
                source,
            }),
        }
    }

    /// True if the recorded PID still exists in `/proc`. Caller treats a
    /// `false` here as "game already exited, kernel reclaimed memory,
    /// nothing to restore" and deletes the record.
    pub fn pid_alive(&self) -> bool {
        Path::new(&format!("/proc/{}", self.pid)).exists()
    }

    /// PID typed for use with the rest of `cheat-runtime`.
    pub fn pid_typed(&self) -> Pid {
        Pid::from_raw(self.pid)
    }
}

/// Path to `~/.config/backlog-tracker/active-hooks/`. The Tauri startup
/// pass calls this directly to enumerate records.
pub fn persist_dir() -> Result<PathBuf, PersistError> {
    Ok(dirs::config_dir()
        .ok_or(PersistError::NoConfigDir)?
        .join(PERSIST_SUBDIR))
}

/// Load every `*.json` file under the persist directory. Used by the
/// Tauri startup task to surface orphan hooks to the user. Malformed
/// files are surfaced as `failed` so a single corrupted record doesn't
/// hide the rest.
#[derive(Debug, Default)]
pub struct LoadAllReport {
    pub records: Vec<PersistedHook>,
    pub failed: Vec<(PathBuf, PersistError)>,
}

pub fn load_all() -> Result<LoadAllReport, PersistError> {
    let dir = persist_dir()?;
    let mut report = LoadAllReport::default();
    if !dir.is_dir() {
        return Ok(report);
    }
    let read = fs::read_dir(&dir).map_err(|source| PersistError::Io {
        path: dir.clone(),
        source,
    })?;
    for entry in read {
        let entry = entry.map_err(|source| PersistError::Io {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        match load_one(&path) {
            Ok(record) => report.records.push(record),
            Err(e) => report.failed.push((path, e)),
        }
    }
    Ok(report)
}

fn load_one(path: &Path) -> Result<PersistedHook, PersistError> {
    let text = fs::read_to_string(path).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| PersistError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Hex-encoded serde adapter — keeps byte arrays human-readable in the
/// JSON file (otherwise serde would emit a number array).
mod hex_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() % 2 != 0 {
            return Err(serde::de::Error::custom("odd-length hex string"));
        }
        let mut out = Vec::with_capacity(s.len() / 2);
        for chunk in s.as_bytes().chunks(2) {
            let pair = std::str::from_utf8(chunk).map_err(serde::de::Error::custom)?;
            let b = u8::from_str_radix(pair, 16).map_err(serde::de::Error::custom)?;
            out.push(b);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serialisation_keeps_bytes_as_hex() {
        let record = PersistedHook {
            app_id: "2725260".into(),
            feature_uuid: "abc".into(),
            pid: 1234,
            exe: "Game.exe".into(),
            started_at: Some("2026-05-19T01:23:45Z".into()),
            writes: vec![PersistedWrite {
                addr: 0x143006e96,
                original: vec![0x48, 0x8b, 0x08, 0x4c, 0x8d],
            }],
            allocs: vec![PersistedAlloc {
                symbol: "newmem".into(),
                addr: 0x13ffff000,
                size: 0x1000,
            }],
        };
        let json = serde_json::to_string_pretty(&record).unwrap();
        // Bytes round-trip through their hex form, not a number array.
        assert!(
            json.contains("\"488b084c8d\""),
            "expected hex-string encoding for original bytes, got: {json}"
        );
        let back: PersistedHook = serde_json::from_str(&json).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn odd_length_hex_string_is_rejected() {
        let bad = r#"{
            "app_id": "x",
            "feature_uuid": "y",
            "pid": 1,
            "exe": "g",
            "writes": [{ "addr": 0, "original": "abc" }],
            "allocs": []
        }"#;
        let err = serde_json::from_str::<PersistedHook>(bad).unwrap_err();
        assert!(err.to_string().contains("odd-length"));
    }

    #[test]
    fn delete_is_idempotent_for_missing_files() {
        let record = PersistedHook {
            app_id: "doesnt".into(),
            feature_uuid: "exist".into(),
            pid: 1,
            exe: "x".into(),
            started_at: None,
            writes: vec![],
            allocs: vec![],
        };
        // No file was ever written — delete should still be Ok.
        record.delete().expect("delete missing is idempotent");
    }
}
