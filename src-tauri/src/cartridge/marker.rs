use std::fs;
use std::path::Path;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::drm::Preservability;

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
    /// From `drm::DrmInfo` at install time (#195) — `Easy` is what makes an
    /// app a #199 (Goldberg injection) candidate; the UI (#196) shows the
    /// rest as-is.
    pub preservability: Preservability,
    /// Set once #199's Goldberg injection has run for this app — lets the
    /// UI (#196) show "playable standalone" without re-probing the install.
    /// Deliberately left out of the checksum below: it's UI metadata, not
    /// part of what the checksum protects (the cartridge's identity and app
    /// list), so adding it can't invalidate a marker written before #199.
    #[serde(default)]
    pub standalone: bool,
    /// Cartridge-relative path (forward slashes, always — read by the
    /// cross-platform Godot launcher) to the main `.exe`, resolved once by
    /// [`crate::steam::exe::pick_main_exe_in`] at the same moment Goldberg
    /// injection runs (#206/#207): the launcher has no Steam client to ask,
    /// so it has to already know exactly which file to hand to Proton or
    /// run directly. Same out-of-checksum reasoning as `standalone` above.
    #[serde(default)]
    pub exe_path: String,
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
        input.push('/');
        input.push_str(&format!("{:?}", app.preservability));
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

/// Add (or replace, by app_id) one app entry on an existing cartridge's
/// marker and rewrite it — the checksum is always recomputed, never
/// preserved from the old file. Called once #195's `poll_install_status`
/// sees Steam report the install fully done.
pub fn add_app(mount_point: &Path, app: CartridgeApp) -> Result<(), String> {
    let mut marker = read_marker(mount_point)
        .ok_or_else(|| format!("{} has no valid cartridge marker", mount_point.display()))?;
    marker.apps.retain(|a| a.app_id != app.app_id);
    marker.apps.push(app);

    let rebuilt = CartridgeMarker::new(marker.apps, marker.created_at);
    let json = serde_json::to_string_pretty(&rebuilt).map_err(|e| e.to_string())?;
    fs::write(mount_point.join(MARKER_FILENAME), json)
        .map_err(|e| format!("Cannot write {MARKER_FILENAME}: {e}"))
}

/// Flip `standalone` on an already-recorded app, record its resolved main
/// `.exe` path, and rewrite the marker. Called once #199's Goldberg
/// injection finishes for that app; errors if the app was never recorded by
/// #195's `add_app` in the first place.
pub fn set_standalone(mount_point: &Path, app_id: u64, exe_path: String) -> Result<(), String> {
    let mut marker = read_marker(mount_point)
        .ok_or_else(|| format!("{} has no valid cartridge marker", mount_point.display()))?;
    let Some(app) = marker.apps.iter_mut().find(|a| a.app_id == app_id) else {
        return Err(format!(
            "App {app_id} is not recorded on this cartridge yet"
        ));
    };
    app.standalone = true;
    app.exe_path = exe_path;

    let rebuilt = CartridgeMarker::new(marker.apps, marker.created_at);
    let json = serde_json::to_string_pretty(&rebuilt).map_err(|e| e.to_string())?;
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

    fn app(app_id: u64, name: &str) -> CartridgeApp {
        CartridgeApp {
            app_id,
            name: name.to_string(),
            preservability: Preservability::Unknown,
            standalone: false,
            exe_path: String::new(),
        }
    }

    #[test]
    fn absent_marker_is_not_a_cartridge() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_cartridge_structure(dir.path()));
    }

    #[test]
    fn valid_marker_is_a_cartridge() {
        let dir = tempfile::tempdir().unwrap();
        let marker = CartridgeMarker::new(vec![app(379720, "DOOM")], 0);
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
        marker.apps.push(app(999, "sneaked in after the checksum"));
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
        let a = CartridgeMarker::new(vec![app(1, "A"), app(2, "B")], 0);
        let b = CartridgeMarker::new(vec![app(2, "B"), app(1, "A")], 0);
        assert_eq!(a.checksum, b.checksum);
    }

    #[test]
    fn add_app_appends_and_stays_trusted() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();

        add_app(dir.path(), app(379720, "DOOM")).unwrap();
        let marker = read_marker(dir.path()).expect("still trustworthy after add_app");
        assert_eq!(marker.apps, vec![app(379720, "DOOM")]);
    }

    #[test]
    fn add_app_replaces_same_app_id() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();

        add_app(dir.path(), app(1, "Old Name")).unwrap();
        add_app(
            dir.path(),
            CartridgeApp {
                app_id: 1,
                name: "New Name".to_string(),
                preservability: Preservability::Easy,
                standalone: false,
                exe_path: String::new(),
            },
        )
        .unwrap();

        let marker = read_marker(dir.path()).unwrap();
        assert_eq!(marker.apps.len(), 1);
        assert_eq!(marker.apps[0].name, "New Name");
        assert_eq!(marker.apps[0].preservability, Preservability::Easy);
    }

    #[test]
    fn set_standalone_flips_the_flag_and_stays_trusted() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        add_app(dir.path(), app(379720, "DOOM")).unwrap();

        set_standalone(
            dir.path(),
            379720,
            "steamapps/common/DOOM/DOOM.exe".to_string(),
        )
        .unwrap();

        let marker = read_marker(dir.path()).expect("still trustworthy after set_standalone");
        assert!(marker.apps[0].standalone);
        assert_eq!(marker.apps[0].exe_path, "steamapps/common/DOOM/DOOM.exe");
    }

    #[test]
    fn set_standalone_rejects_an_app_never_recorded() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        assert!(set_standalone(dir.path(), 1, String::new()).is_err());
    }
}
