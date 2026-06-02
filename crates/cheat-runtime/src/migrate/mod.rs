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
            lua: false,
            children: Vec::new(),
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
        // Legacy cheat-core tables predate the prereqs concept (#98) and
        // never targeted RE Engine games in practice (cheat-core only
        // handled Absolute/Static/PointerChain, RE Engine cheats need
        // aobscanmodule). Empty vec is correct here.
        prereqs: Vec::new(),
        // Legacy cheat-core tables are plain AA, never Lua-framework.
        framework: false,
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
mod tests;
