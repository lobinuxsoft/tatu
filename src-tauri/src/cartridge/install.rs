use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::drm::Preservability;
use crate::steam::library_paths;

use super::marker::{CartridgeApp, add_app};

/// Steam's own client owns everything from here: download, EULA, its own
/// disk-selection prompt if more than one library qualifies.
pub fn install_url(app_id: u64) -> String {
    format!("steam://install/{app_id}")
}

/// Whether `mount_point` is already registered as a Steam library —
/// `library_paths()` is what `Settings > Storage > Add Drive` writes to,
/// so this is the same check Steam itself would make.
pub fn is_registered_library(mount_point: &Path) -> bool {
    let target = fs::canonicalize(mount_point).unwrap_or_else(|_| mount_point.to_path_buf());
    library_paths().iter().any(|lib| {
        let lib = fs::canonicalize(lib).unwrap_or_else(|_| lib.clone());
        lib == target
    })
}

/// Steam owns the download; this only watches for it finishing by polling
/// `steamapps/appmanifest_<app_id>.acf` for `StateFlags == 4` (fully
/// installed, no pending update, no missing/corrupt files — Steam's own
/// definition of "done"). On completion, records the app on the #193
/// marker so the drive's own manifest reflects it without a full re-scan.
///
/// Returns `Ok(false)` while still installing (or not started yet — the
/// manifest may not exist until Steam begins the download), `Ok(true)`
/// once recorded.
pub fn poll_install_status(
    app_id: u64,
    mount_point: &Path,
    name: &str,
    preservability: Preservability,
) -> Result<bool, String> {
    let manifest = appmanifest_path(mount_point, app_id);
    let Ok(content) = fs::read_to_string(&manifest) else {
        return Ok(false);
    };
    let Some(flags) = acf_field(&content, "StateFlags").and_then(|f| f.parse::<u32>().ok()) else {
        return Ok(false);
    };
    if flags != 4 {
        return Ok(false);
    }

    add_app(
        mount_point,
        CartridgeApp {
            app_id,
            name: name.to_string(),
            preservability,
            standalone: false,
            exe_path: String::new(),
        },
    )?;
    Ok(true)
}

/// Path to Steam's own per-app manifest on a cartridge — shared with #199's
/// Goldberg injection, which also needs `installdir` off the same file.
pub(super) fn appmanifest_path(mount_point: &Path, app_id: u64) -> PathBuf {
    mount_point
        .join("steamapps")
        .join(format!("appmanifest_{app_id}.acf"))
}

/// Reads one `"field" "value"` pair out of a Steam ACF/VDF file. Compiles a
/// fresh regex per call — this runs at most a few times per install poll or
/// injection, never in a hot loop, so a per-field static cache would be
/// complexity with no measurable benefit.
pub(super) fn acf_field(content: &str, field: &str) -> Option<String> {
    let re = Regex::new(&format!(r#""{field}"\s*"([^"]*)""#)).ok()?;
    Some(re.captures(content)?[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::marker::{read_marker, write_marker};

    #[test]
    fn install_url_targets_the_right_app() {
        assert_eq!(install_url(379720), "steam://install/379720");
    }

    #[test]
    fn state_flags_four_is_fully_installed() {
        let acf = r#""AppState" { "appid" "1" "StateFlags" "4" }"#;
        assert_eq!(acf_field(acf, "StateFlags").as_deref(), Some("4"));
    }

    #[test]
    fn state_flags_mid_download_is_not_four() {
        // Update started + update required, per Steam's own bitmask.
        let acf = r#""AppState" { "appid" "1" "StateFlags" "1026" }"#;
        assert_eq!(acf_field(acf, "StateFlags").as_deref(), Some("1026"));
    }

    #[test]
    fn installdir_is_read_from_the_manifest() {
        let acf = r#""AppState" { "appid" "1" "installdir" "DOOM" "StateFlags" "4" }"#;
        assert_eq!(acf_field(acf, "installdir").as_deref(), Some("DOOM"));
    }

    #[test]
    fn missing_manifest_is_not_yet_installed() {
        let dir = tempfile::tempdir().unwrap();
        let done =
            poll_install_status(1, dir.path(), "Nothing Yet", Preservability::Unknown).unwrap();
        assert!(!done);
    }

    #[test]
    fn fully_installed_manifest_records_the_app() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        let steamapps = dir.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join("appmanifest_379720.acf"),
            r#""AppState" { "appid" "379720" "StateFlags" "4" }"#,
        )
        .unwrap();

        let done =
            poll_install_status(379720, dir.path(), "DOOM", Preservability::Trivial).unwrap();
        assert!(done);

        let marker = read_marker(dir.path()).unwrap();
        assert_eq!(marker.apps.len(), 1);
        assert_eq!(marker.apps[0].app_id, 379720);
        assert_eq!(marker.apps[0].preservability, Preservability::Trivial);
    }

    #[test]
    fn mid_download_manifest_does_not_record_yet() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        let steamapps = dir.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join("appmanifest_379720.acf"),
            r#""AppState" { "appid" "379720" "StateFlags" "1026" }"#,
        )
        .unwrap();

        let done =
            poll_install_status(379720, dir.path(), "DOOM", Preservability::Trivial).unwrap();
        assert!(!done);

        let marker = read_marker(dir.path()).unwrap();
        assert!(marker.apps.is_empty());
    }
}
