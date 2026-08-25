use std::fs;
use std::path::Path;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

/// Filename of the marker Tatu writes at a cartridge's root once it has been
/// formatted (#194). No paths inside it — Linux and Windows resolve the
/// Steam library path for the same physical drive differently, so baking one
/// in would go stale the first time the drive moves between platforms.
pub const MARKER_FILENAME: &str = ".tatu-cartridge.json";

pub const MARKER_FORMAT_VERSION: u32 = 1;

/// Volume label every cartridge gets, always this exact string (leetspeak
/// for "GAME CARTRIDGE"). yaguarete_os's udev rule matches on it to
/// auto-run the cartridge launcher — obscure on purpose, though not a real
/// security boundary once this source is read; combined with the marker's
/// own checksum below it's enough to keep a random USB stick from ever
/// matching by accident.
pub const CARTRIDGE_LABEL: &str = "64M3_C4R7R1D63";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartridgeApp {
    pub app_id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartridgeMarker {
    pub format_version: u32,
    pub label: String,
    pub apps: Vec<CartridgeApp>,
    pub created_at: u64,
    /// MD5 over every field above — label + app list + timestamps, never
    /// the installed games' own bytes. Recomputing this costs microseconds
    /// even on a full library, unlike hashing the actual game files: file
    /// integrity for those already belongs to Steam's own manifest
    /// verification. This only proves the marker was written by Tatu
    /// rather than hand-edited or corrupted.
    pub checksum: String,
}

impl CartridgeMarker {
    pub fn new(apps: Vec<CartridgeApp>, created_at: u64) -> Self {
        let format_version = MARKER_FORMAT_VERSION;
        let label = CARTRIDGE_LABEL.to_string();
        let checksum = compute_checksum(format_version, &label, &apps, created_at);
        Self {
            format_version,
            label,
            apps,
            created_at,
            checksum,
        }
    }

    /// Whether the stored checksum still matches the rest of the fields and
    /// the label is the exact one Tatu writes.
    fn is_trustworthy(&self) -> bool {
        self.label == CARTRIDGE_LABEL
            && self.checksum
                == compute_checksum(
                    self.format_version,
                    &self.label,
                    &self.apps,
                    self.created_at,
                )
    }
}

/// Deterministic string form of the marker's fields, hashed with MD5. Apps
/// are sorted by app_id first so insertion order never changes the result.
/// Keep this format simple and documented: yaguarete_os's own shell-side
/// verification has to reproduce it exactly, independent of this crate.
fn compute_checksum(
    format_version: u32,
    label: &str,
    apps: &[CartridgeApp],
    created_at: u64,
) -> String {
    let mut sorted: Vec<&CartridgeApp> = apps.iter().collect();
    sorted.sort_by_key(|a| a.app_id);

    let mut input = format!("{format_version}:{label}:{created_at}");
    for app in sorted {
        input.push(':');
        input.push_str(&app.app_id.to_string());
        input.push('=');
        input.push_str(&app.name);
    }

    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Read and parse the marker at a drive's root, if present, valid JSON, and
/// internally consistent (label + checksum check out).
pub fn read_marker(mount_point: &Path) -> Option<CartridgeMarker> {
    let content = fs::read_to_string(mount_point.join(MARKER_FILENAME)).ok()?;
    let marker: CartridgeMarker = serde_json::from_str(&content).ok()?;
    marker.is_trustworthy().then_some(marker)
}

/// Whether `mount_point` is the root of an already-prepared, trustworthy
/// cartridge. A file that exists but fails to parse or fails its own
/// checksum counts as absent — better to re-offer the guided setup than to
/// trust a corrupt or hand-edited marker.
pub fn has_cartridge_structure(mount_point: &Path) -> bool {
    read_marker(mount_point).is_some()
}

/// Write a fresh, empty marker right after formatting (#194). Later
/// installs (#195) rewrite this file with the growing app list.
#[allow(dead_code)]
pub fn write_marker(mount_point: &Path) -> Result<(), String> {
    let marker = CartridgeMarker::new(Vec::new(), now_secs());
    let json = serde_json::to_string_pretty(&marker).map_err(|e| e.to_string())?;
    fs::write(mount_point.join(MARKER_FILENAME), json)
        .map_err(|e| format!("Cannot write {MARKER_FILENAME}: {e}"))
}

#[allow(dead_code)]
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_marker_is_not_a_cartridge() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_cartridge_structure(dir.path()));
    }

    #[test]
    fn valid_marker_is_a_cartridge() {
        let dir = tempfile::tempdir().unwrap();
        let marker = CartridgeMarker::new(
            vec![CartridgeApp {
                app_id: 379720,
                name: "DOOM".to_string(),
            }],
            0,
        );
        fs::write(
            dir.path().join(MARKER_FILENAME),
            serde_json::to_string(&marker).unwrap(),
        )
        .unwrap();
        assert!(has_cartridge_structure(dir.path()));
    }

    #[test]
    fn garbage_marker_is_not_a_cartridge() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(MARKER_FILENAME), "not json").unwrap();
        assert!(!has_cartridge_structure(dir.path()));
    }

    #[test]
    fn tampered_marker_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let mut marker = CartridgeMarker::new(Vec::new(), 0);
        marker.apps.push(CartridgeApp {
            app_id: 999,
            name: "sneaked in after the checksum".to_string(),
        });
        fs::write(
            dir.path().join(MARKER_FILENAME),
            serde_json::to_string(&marker).unwrap(),
        )
        .unwrap();
        assert!(!has_cartridge_structure(dir.path()));
    }

    #[test]
    fn wrong_label_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let mut marker = CartridgeMarker::new(Vec::new(), 0);
        marker.label = "SOMETHING_ELSE".to_string();
        fs::write(
            dir.path().join(MARKER_FILENAME),
            serde_json::to_string(&marker).unwrap(),
        )
        .unwrap();
        assert!(!has_cartridge_structure(dir.path()));
    }

    #[test]
    fn checksum_ignores_app_order() {
        let a = CartridgeMarker::new(
            vec![
                CartridgeApp {
                    app_id: 1,
                    name: "A".to_string(),
                },
                CartridgeApp {
                    app_id: 2,
                    name: "B".to_string(),
                },
            ],
            0,
        );
        let b = CartridgeMarker::new(
            vec![
                CartridgeApp {
                    app_id: 2,
                    name: "B".to_string(),
                },
                CartridgeApp {
                    app_id: 1,
                    name: "A".to_string(),
                },
            ],
            0,
        );
        assert_eq!(a.checksum, b.checksum);
    }
}
