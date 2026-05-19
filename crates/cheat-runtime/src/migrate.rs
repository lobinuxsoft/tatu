//! One-shot migrator from the deprecated `cheat-core` legacy format
//! (`~/.config/backlog-tracker/cheats/<appid>.json`) to the manifest format
//! consumed by [`crate::manifest`] (`~/.config/backlog-tracker/trainers/<appid>/legacy.json`).
//!
//! The legacy format only ever encoded `Absolute` + `Static` + `PointerChain`
//! addresses with a single typed value write per cheat. We model the same data
//! locally (without pulling `cheat-core` back as a dependency) and emit a
//! synthetic CE Auto-Assembler script per cheat using the numeric label site
//! form supported by [`crate::parser::Statement::AbsoluteSite`]. `Static` and
//! `PointerChain` entries are skipped with a warning — they require module
//! base resolution / pointer walking that the legacy crate did at apply time
//! and the new runtime resolves through `aobscanmodule` instead, so a
//! mechanical conversion is impossible without re-running against the live
//! process. In practice the user's on-disk corpus is `Absolute`-only.
//!
//! The migration is idempotent: if `trainers/<appid>/legacy.json` already
//! exists, the source file is left untouched and reported as `Skipped`. This
//! lets the Tauri startup call this entry point on every boot safely.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::manifest::{Manifest, ManifestFeature};

const LEGACY_SUBDIR: &str = "backlog-tracker/cheats";
const MANIFEST_SUBDIR: &str = "backlog-tracker/trainers";
const MIGRATED_FILENAME: &str = "legacy.json";

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid legacy json at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrateReport {
    pub migrated: Vec<u64>,
    pub skipped: Vec<u64>,
    pub unsupported: Vec<(u64, String)>,
}

#[derive(Debug, Deserialize)]
struct LegacyTable {
    app_id: u64,
    #[serde(default)]
    game_name: String,
    exe_pattern: String,
    cheats: Vec<LegacyCheat>,
}

#[derive(Debug, Deserialize)]
struct LegacyCheat {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    address: LegacyAddress,
    action: LegacyAction,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum LegacyAddress {
    Absolute {
        #[serde(deserialize_with = "de_hex_or_dec")]
        address: u64,
    },
    Static {
        #[allow(dead_code)]
        module: String,
        #[allow(dead_code)]
        #[serde(deserialize_with = "de_hex_or_dec")]
        offset: u64,
    },
    PointerChain {
        #[allow(dead_code)]
        base_module: String,
        #[allow(dead_code)]
        #[serde(deserialize_with = "de_hex_or_dec")]
        base_offset: u64,
        #[allow(dead_code)]
        #[serde(default)]
        offsets: Vec<serde_json::Value>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum LegacyAction {
    WriteOnce {
        value: LegacyValue,
    },
    Freeze {
        value: LegacyValue,
        #[allow(dead_code)]
        #[serde(default)]
        interval_ms: Option<u64>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
enum LegacyValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl LegacyValue {
    fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            Self::U8(v) => v.to_le_bytes().to_vec(),
            Self::U16(v) => v.to_le_bytes().to_vec(),
            Self::U32(v) => v.to_le_bytes().to_vec(),
            Self::U64(v) => v.to_le_bytes().to_vec(),
            Self::I8(v) => v.to_le_bytes().to_vec(),
            Self::I16(v) => v.to_le_bytes().to_vec(),
            Self::I32(v) => v.to_le_bytes().to_vec(),
            Self::I64(v) => v.to_le_bytes().to_vec(),
            Self::F32(v) => v.to_le_bytes().to_vec(),
            Self::F64(v) => v.to_le_bytes().to_vec(),
        }
    }
}

impl LegacyAction {
    fn value(&self) -> &LegacyValue {
        match self {
            Self::WriteOnce { value } | Self::Freeze { value, .. } => value,
        }
    }
}

/// Migrate every legacy `*.json` under `$XDG_CONFIG_HOME/backlog-tracker/cheats/`
/// to the manifest layout. See module docs for idempotence rules.
pub fn migrate_default_dirs() -> Result<MigrateReport, MigrateError> {
    let config = dirs::config_dir().ok_or(MigrateError::NoConfigDir)?;
    migrate_dirs(&config.join(LEGACY_SUBDIR), &config.join(MANIFEST_SUBDIR))
}

/// Lower-level entry that lets callers point at arbitrary directories
/// (tests, dry runs, sandboxed environments).
pub fn migrate_dirs(
    legacy_dir: &Path,
    trainers_root: &Path,
) -> Result<MigrateReport, MigrateError> {
    let mut report = MigrateReport::default();
    if !legacy_dir.is_dir() {
        return Ok(report);
    }
    for entry in std::fs::read_dir(legacy_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        match migrate_one(&path, trainers_root, &mut report) {
            Ok(()) => {}
            Err(MigrateError::Json { path, source }) => {
                return Err(MigrateError::Json { path, source });
            }
            Err(other) => return Err(other),
        }
    }
    Ok(report)
}

fn migrate_one(
    path: &Path,
    trainers_root: &Path,
    report: &mut MigrateReport,
) -> Result<(), MigrateError> {
    let text = std::fs::read_to_string(path)?;
    let table: LegacyTable = serde_json::from_str(&text).map_err(|source| MigrateError::Json {
        path: path.to_path_buf(),
        source,
    })?;

    let target_dir = trainers_root.join(table.app_id.to_string());
    let target_file = target_dir.join(MIGRATED_FILENAME);
    if target_file.exists() {
        report.skipped.push(table.app_id);
        return Ok(());
    }

    let mut features = Vec::with_capacity(table.cheats.len());
    for cheat in &table.cheats {
        let LegacyAddress::Absolute { address } = cheat.address else {
            report
                .unsupported
                .push((table.app_id, format!("{}: non-absolute address", cheat.id)));
            continue;
        };
        features.push(ManifestFeature {
            uuid: synthetic_uuid(table.app_id, &cheat.id),
            name: cheat.name.clone(),
            category: cheat.description.clone(),
            kind: crate::manifest::FeatureKind::Toggle,
            script: Some(synth_script(address, &cheat.action.value().to_le_bytes())),
            value: None,
        });
    }

    let manifest = Manifest {
        exe: table.exe_pattern,
        title: if table.game_name.is_empty() {
            format!("Legacy {}", table.app_id)
        } else {
            table.game_name
        },
        features,
    };

    std::fs::create_dir_all(&target_dir)?;
    let body = serde_json::to_string_pretty(&manifest).map_err(|source| MigrateError::Json {
        path: target_file.clone(),
        source,
    })?;
    std::fs::write(&target_file, body)?;
    report.migrated.push(table.app_id);
    Ok(())
}

fn synth_script(address: u64, bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "[ENABLE]\n0x{address:X}:\ndb {bytes}\n\n[DISABLE]\n",
        bytes = hex.join(" "),
    )
}

fn synthetic_uuid(app_id: u64, cheat_id: &str) -> String {
    format!("legacy-{app_id}-{cheat_id}")
}

fn de_hex_or_dec<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        {
            Some(hex) => u64::from_str_radix(hex, 16).map_err(serde::de::Error::custom),
            None => s.parse::<u64>().map_err(serde::de::Error::custom),
        },
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("offset must fit in u64")),
        _ => Err(serde::de::Error::custom(
            "offset must be a string or number",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Script, Statement, parse as parse_script};
    use tempfile::TempDir;

    const EM_LEGACY: &str = r#"{
        "app_id": 2725260,
        "game_name": "ENDER MAGNOLIA",
        "exe_pattern": "EnderMagnoliaSteam-Win64-Shipping.exe",
        "cheats": [{
            "id": "1",
            "name": "Materials",
            "description": null,
            "address": { "kind": "Absolute", "address": 708486264 },
            "action": { "kind": "WriteOnce", "value": { "type": "u32", "value": 0 } }
        }]
    }"#;

    const PRAGMATA_LEGACY: &str = r#"{
        "app_id": 3357650,
        "game_name": "PRAGMATA",
        "exe_pattern": "PRAGMATA.exe",
        "cheats": [{
            "id": "full_heal",
            "name": "Full Heal",
            "description": "Smoke test heal",
            "address": { "kind": "Absolute", "address": "0xb056ec28" },
            "action": { "kind": "WriteOnce", "value": { "type": "f32", "value": 1600.0 } }
        }]
    }"#;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn migrates_em_to_manifest_with_absolute_site() {
        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path().join("legacy");
        let trainers = tmp.path().join("trainers");
        std::fs::create_dir_all(&legacy).unwrap();
        write(&legacy, "2725260.json", EM_LEGACY);

        let report = migrate_dirs(&legacy, &trainers).unwrap();
        assert_eq!(report.migrated, vec![2_725_260]);

        let target = trainers.join("2725260").join("legacy.json");
        let m: Manifest = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(m.exe, "EnderMagnoliaSteam-Win64-Shipping.exe");
        assert_eq!(m.title, "ENDER MAGNOLIA");
        assert_eq!(m.features.len(), 1);
        assert_eq!(m.features[0].name, "Materials");

        // The synthesised script must parse into AbsoluteSite + a db raw line.
        let script_src = m.features[0]
            .script
            .as_deref()
            .expect("migrated cheats are Toggle with a script");
        let Script { enable, .. } = parse_script(script_src).unwrap();
        assert!(matches!(enable[0], Statement::AbsoluteSite(708_486_264)));
        assert!(matches!(&enable[1], Statement::Raw(line) if line.starts_with("db ")));
    }

    #[test]
    fn migrates_pragmata_with_hex_address_and_f32_bytes() {
        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path().join("legacy");
        let trainers = tmp.path().join("trainers");
        std::fs::create_dir_all(&legacy).unwrap();
        write(&legacy, "3357650.json", PRAGMATA_LEGACY);

        migrate_dirs(&legacy, &trainers).unwrap();
        let target = trainers.join("3357650").join("legacy.json");
        let m: Manifest = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        let script = m.features[0]
            .script
            .as_deref()
            .expect("migrated cheats are Toggle with a script");
        // 1600.0f32 little-endian = 00 00 C8 44
        assert!(script.contains("0xB056EC28:"));
        assert!(script.contains("db 00 00 C8 44"));
    }

    #[test]
    fn second_run_is_idempotent_and_reports_skipped() {
        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path().join("legacy");
        let trainers = tmp.path().join("trainers");
        std::fs::create_dir_all(&legacy).unwrap();
        write(&legacy, "2725260.json", EM_LEGACY);

        migrate_dirs(&legacy, &trainers).unwrap();
        // Mutate the produced manifest, then re-run: skip must preserve it.
        let target = trainers.join("2725260").join("legacy.json");
        std::fs::write(&target, r#"{"exe":"x","title":"x","features":[]}"#).unwrap();

        let report = migrate_dirs(&legacy, &trainers).unwrap();
        assert_eq!(report.skipped, vec![2_725_260]);
        assert!(report.migrated.is_empty());
        let after = std::fs::read_to_string(&target).unwrap();
        assert!(after.contains(r#""features":[]"#));
    }

    #[test]
    fn missing_legacy_dir_is_not_an_error() {
        let tmp = TempDir::new().unwrap();
        let report = migrate_dirs(
            &tmp.path().join("does-not-exist"),
            &tmp.path().join("trainers"),
        )
        .unwrap();
        assert!(report.migrated.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn static_address_is_reported_as_unsupported() {
        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path().join("legacy");
        let trainers = tmp.path().join("trainers");
        std::fs::create_dir_all(&legacy).unwrap();
        let static_only = r#"{
            "app_id": 999,
            "game_name": "test",
            "exe_pattern": "x.exe",
            "cheats": [{
                "id": "s1",
                "name": "Static",
                "address": { "kind": "Static", "module": "x.exe", "offset": "0x100" },
                "action": { "kind": "WriteOnce", "value": { "type": "u32", "value": 1 } }
            }]
        }"#;
        write(&legacy, "999.json", static_only);

        let report = migrate_dirs(&legacy, &trainers).unwrap();
        assert_eq!(report.unsupported.len(), 1);
        assert_eq!(report.unsupported[0].0, 999);
        // A manifest is still written, just with zero features (the file
        // existed in legacy form so something was processed).
        let m: Manifest = serde_json::from_str(
            &std::fs::read_to_string(trainers.join("999").join("legacy.json")).unwrap(),
        )
        .unwrap();
        assert!(m.features.is_empty());
    }

    #[test]
    fn invalid_json_surfaces_path_in_error() {
        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path().join("legacy");
        let trainers = tmp.path().join("trainers");
        std::fs::create_dir_all(&legacy).unwrap();
        write(&legacy, "broken.json", "{not json");

        let err = migrate_dirs(&legacy, &trainers).unwrap_err();
        match err {
            MigrateError::Json { path, .. } => {
                assert!(path.ends_with("broken.json"));
            }
            other => panic!("expected Json error, got {other:?}"),
        }
    }
}
