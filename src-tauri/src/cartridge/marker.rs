use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Filename of the marker Tatu writes at a cartridge's root once it has been
/// formatted (#194). No paths inside it — Linux and Windows resolve the
/// Steam library path for the same physical drive differently, so baking one
/// in would go stale the first time the drive moves between platforms.
pub const MARKER_FILENAME: &str = ".tatu-cartridge.json";

// Written by #194's format step, not yet — only read_marker (used in tests
// below) exists here in #193.
#[allow(dead_code)]
pub const MARKER_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartridgeApp {
    pub app_id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartridgeMarker {
    pub format_version: u32,
    pub apps: Vec<CartridgeApp>,
    pub created_at: u64,
}

/// Read and parse the marker at a drive's root, if present and valid.
pub fn read_marker(mount_point: &Path) -> Option<CartridgeMarker> {
    let content = fs::read_to_string(mount_point.join(MARKER_FILENAME)).ok()?;
    serde_json::from_str(&content).ok()
}

/// Whether `mount_point` is the root of an already-prepared cartridge.
/// A file that exists but fails to parse counts as absent — better to
/// re-offer the guided setup than to trust a corrupt marker.
pub fn has_cartridge_structure(mount_point: &Path) -> bool {
    read_marker(mount_point).is_some()
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
        let marker = CartridgeMarker {
            format_version: MARKER_FORMAT_VERSION,
            apps: vec![CartridgeApp {
                app_id: 379720,
                name: "DOOM".to_string(),
            }],
            created_at: 0,
        };
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
}
