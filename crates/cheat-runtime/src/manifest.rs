//! Per-game cheat manifest format.
//!
//! A manifest is a **self-describing** JSON file that explicitly binds each
//! user-facing feature to the CE Auto-Assembler script(s) that implement it:
//!
//! ```json
//! {
//!   "exe": "EnderMagnoliaSteam-Win64-Shipping.exe",
//!   "title": "Ender Magnolia",
//!   "features": [
//!     {
//!       "uuid": "12710713-cf53-47f2-8a7a-c3139fda2677",
//!       "name": "God Mode",
//!       "category": "Player",
//!       "script": "[ENABLE]\naobscanmodule(...)\n...\n[DISABLE]\n..."
//!     }
//!   ]
//! }
//! ```
//!
//! Manifests live under `$XDG_CONFIG_HOME/backlog-tracker/trainers/<app_id>/`
//! and are loaded eagerly by [`load_manifests_for`]. Many manifests can
//! coexist per game (e.g. one per trainer source).
//!
//! Aurora's raw JSON (handled by the `aurora` module) is **not** a manifest
//! — it exposes features and scripts as parallel lists with no binding.
//! Converting Aurora payloads to manifests is gated on reverse-engineering
//! the feature→script binding, which is documented in personal memory as a
//! still-open problem.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MANIFEST_SUBDIR: &str = "backlog-tracker/trainers";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub exe: String,
    #[serde(default)]
    pub title: String,
    pub features: Vec<ManifestFeature>,
}

/// Visual / behavioural category of a [`ManifestFeature`].
///
/// Mirrors Cheat Engine's distinction between functional cheats and pure
/// grouping headers in `.CT` tables (CE uses `<GroupHeader>1</GroupHeader>`
/// in the XML element — `MemoryRecordUnit.pas:148` documents it as
/// *"set if it's a groupheader, only the description matters then"*).
///
/// `Toggle` is the default so older manifests (which omit `kind` entirely)
/// continue to deserialize as functional cheats without a migration step.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    /// Functional cheat — has an Auto-Assembler script and renders as a
    /// switch in the UI.
    #[default]
    Toggle,
    /// Visual section title. No script, no switch — the UI renders just
    /// the description as a header above the features that follow.
    Header,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestFeature {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub kind: FeatureKind,
    /// CE Auto-Assembler script for [`FeatureKind::Toggle`]. Always `None`
    /// for [`FeatureKind::Header`]. Omitted in older manifests (before this
    /// field existed) — those load as `Toggle` with `script: None` and the
    /// runtime command rejects them at enable time with a clear error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid manifest json at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
}

pub fn manifests_dir_for(app_id: &str) -> Result<PathBuf, ManifestError> {
    Ok(dirs::config_dir()
        .ok_or(ManifestError::NoConfigDir)?
        .join(MANIFEST_SUBDIR)
        .join(app_id))
}

/// Load every `*.json` file under `$XDG_CONFIG_HOME/backlog-tracker/trainers/<app_id>/`.
///
/// Missing directory is not an error — returns an empty vec, so a UI can call
/// this on a game with no manifests and render an empty state cleanly.
pub fn load_manifests_for(app_id: &str) -> Result<Vec<Manifest>, ManifestError> {
    let dir = manifests_dir_for(app_id)?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            out.push(load_one(&path)?);
        }
    }
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(out)
}

fn load_one(path: &Path) -> Result<Manifest, ManifestError> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| ManifestError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn load_dir(dir: &Path) -> Vec<Manifest> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(m) = load_one(&path)
            {
                out.push(m);
            }
        }
        out
    }

    #[test]
    fn parses_minimal_manifest() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"u","name":"God Mode","script":"[ENABLE]\n[DISABLE]\n"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(m.exe, "Game.exe");
        assert_eq!(m.features.len(), 1);
        assert_eq!(m.features[0].name, "God Mode");
    }

    #[test]
    fn defaults_apply_for_optional_fields() {
        let m: Manifest = serde_json::from_str(r#"{"exe":"X.exe","features":[]}"#).unwrap();
        assert_eq!(m.title, "");
    }

    #[test]
    fn round_trip_serialisation_stable() {
        let m = Manifest {
            exe: "Game.exe".into(),
            title: "My Game".into(),
            features: vec![
                ManifestFeature {
                    uuid: "u".into(),
                    name: "God Mode".into(),
                    category: Some("Player".into()),
                    kind: FeatureKind::Toggle,
                    script: Some("[ENABLE]\n[DISABLE]\n".into()),
                },
                ManifestFeature {
                    uuid: "h".into(),
                    name: "=== Player ===".into(),
                    category: None,
                    kind: FeatureKind::Header,
                    script: None,
                },
            ],
        };
        let text = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn deserialises_old_shape_without_kind_as_toggle() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"u","name":"God Mode","script":"[ENABLE]\n[DISABLE]\n"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(m.features[0].kind, FeatureKind::Toggle);
        assert_eq!(
            m.features[0].script.as_deref(),
            Some("[ENABLE]\n[DISABLE]\n")
        );
    }

    #[test]
    fn header_kind_round_trips_without_script() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"sep1","name":"== Combat ==","kind":"header"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(m.features[0].kind, FeatureKind::Header);
        assert!(m.features[0].script.is_none());
        // Round-trip omits the script field when None.
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("\"script\""));
    }

    #[test]
    fn load_dir_picks_up_json_only() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "a.json",
            r#"{"exe":"a.exe","title":"Alpha","features":[]}"#,
        );
        write(
            tmp.path(),
            "b.json",
            r#"{"exe":"b.exe","title":"Beta","features":[]}"#,
        );
        write(tmp.path(), "ignore.txt", "not json");

        let mut all = load_dir(tmp.path());
        all.sort_by(|a, b| a.title.cmp(&b.title));
        let titles: Vec<&str> = all.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn malformed_json_surfaces_path_in_error() {
        let tmp = TempDir::new().unwrap();
        let bad = tmp.path().join("bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        let err = load_one(&bad).unwrap_err();
        match err {
            ManifestError::Json { path, .. } => assert_eq!(path, bad),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
