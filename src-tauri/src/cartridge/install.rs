use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::drm::Preservability;
use crate::steam::library_paths;

use super::drives::list_removable_drives;
use super::marker::{CartridgeApp, add_app, has_cartridge_structure, list_apps};

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

/// Reconciles the marker against every appmanifest actually present on the
/// cartridge. `poll_install_status` above is the ONLY thing that ever
/// writes a new app into the marker — installing a second game directly
/// through Steam's own UI (rather than this app's per-game "Instalar en un
/// cartucho" flow), once the cartridge is already a registered Steam
/// library, never touches the marker at all. Live case (2026-08-29):
/// CrossCode installed and fully playable via Steam, invisible everywhere
/// in Tatu because nothing had ever recorded it.
///
/// Only ADDS apps the marker doesn't already know about — an already-
/// tracked app's own classification/standalone state is left untouched,
/// that refresh is `cartridge::refresh_drm_and_inject`'s job (#238/#239).
pub fn sync_marker_with_installed_apps(mount_point: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(mount_point.join("steamapps")) else {
        return Ok(());
    };

    let known: std::collections::HashSet<u64> = list_apps(mount_point)
        .map(|apps| apps.iter().map(|a| a.app_id).collect())
        .unwrap_or_default();

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(app_id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("appmanifest_"))
            .and_then(|n| n.strip_suffix(".acf"))
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        if known.contains(&app_id) {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        // Same "fully installed" definition poll_install_status already
        // uses above — a manifest existing mid-download shouldn't get
        // recorded as a real, playable app yet.
        if acf_field(&content, "StateFlags").and_then(|f| f.parse::<u32>().ok()) != Some(4) {
            continue;
        }
        let name = acf_field(&content, "name").unwrap_or_else(|| format!("App {app_id}"));

        add_app(
            mount_point,
            CartridgeApp {
                app_id,
                name,
                preservability: Preservability::default(),
                standalone: false,
                exe_path: String::new(),
            },
        )?;
    }

    Ok(())
}

/// Finds a currently-connected cartridge that already has `app_id`'s Steam
/// manifest on it — installing, stalled, or already finished — so the UI
/// can resume watching it without the user re-clicking through the drive
/// picker. There is nothing in memory to lose track of: a manifest existing
/// on disk IS the evidence an install started, on whichever physical
/// cartridge happens to have it, regardless of whether Tatu's own modal
/// (or Tatu itself) stayed open the whole time.
pub async fn find_pending_cartridge(app_id: u64) -> Result<Option<String>, String> {
    let drives = list_removable_drives().await?;
    // A drive stalled at the kernel level (a struggling USB stick under
    // heavy write load — seen firsthand smoke-testing #206) can make a
    // plain fs read block for a very long time. Running these on a
    // blocking-pool thread keeps that stall from stealing the async
    // runtime's own worker threads, which is what actually froze the rest
    // of the UI's IPC, not just this one check.
    tokio::task::spawn_blocking(move || {
        for drive in drives {
            let Some(mount) = drive.mount_point else {
                continue;
            };
            let mount_path = Path::new(&mount);
            if has_cartridge_structure(mount_path) && appmanifest_path(mount_path, app_id).is_file()
            {
                return Some(mount);
            }
        }
        None
    })
    .await
    .map_err(|e| format!("Task error: {e}"))
}

/// Removes a game's install (both the manifest and the actual files) from
/// the cartridge, so a follow-up `steam://install` starts completely fresh
/// instead of Steam treating it as an update to repair in place. Needed
/// when the wrong depot landed (e.g. a native Linux build on a cartridge
/// that needs the Windows one, see #206's `force_proton_compat`) — Steam's
/// own depot-swap-on-update path is unreliable (ValveSoftware/Proton#6635).
pub fn uninstall_from_cartridge(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let manifest = appmanifest_path(mount_point, app_id);
    let content = fs::read_to_string(&manifest)
        .map_err(|e| format!("Cannot read {}: {e}", manifest.display()))?;
    let installdir = acf_field(&content, "installdir")
        .ok_or_else(|| format!("{} has no installdir field", manifest.display()))?;

    let install_dir = mount_point
        .join("steamapps")
        .join("common")
        .join(&installdir);
    if install_dir.is_dir() {
        fs::remove_dir_all(&install_dir)
            .map_err(|e| format!("Cannot remove {}: {e}", install_dir.display()))?;
    }
    fs::remove_file(&manifest).map_err(|e| format!("Cannot remove {}: {e}", manifest.display()))
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

    #[test]
    fn sync_adds_an_app_installed_outside_tatu() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        let steamapps = dir.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        // No add_app call for this one at all — same as installing a second
        // game straight through Steam's own UI, live case (2026-08-29).
        fs::write(
            steamapps.join("appmanifest_368340.acf"),
            r#""AppState" { "appid" "368340" "name" "CrossCode" "installdir" "CrossCode" "StateFlags" "4" }"#,
        )
        .unwrap();

        sync_marker_with_installed_apps(dir.path()).unwrap();

        let marker = read_marker(dir.path()).unwrap();
        assert_eq!(marker.apps.len(), 1);
        assert_eq!(marker.apps[0].app_id, 368340);
        assert_eq!(marker.apps[0].name, "CrossCode");
        assert_eq!(marker.apps[0].preservability, Preservability::Unknown);
    }

    #[test]
    fn sync_leaves_an_already_tracked_app_untouched() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        let steamapps = dir.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join("appmanifest_1.acf"),
            r#""AppState" { "appid" "1" "name" "Old Name" "StateFlags" "4" }"#,
        )
        .unwrap();
        add_app(
            dir.path(),
            CartridgeApp {
                app_id: 1,
                name: "Already Tracked".to_string(),
                preservability: Preservability::Easy,
                standalone: true,
                exe_path: "steamapps/common/Game/game.exe".to_string(),
            },
        )
        .unwrap();

        sync_marker_with_installed_apps(dir.path()).unwrap();

        let marker = read_marker(dir.path()).unwrap();
        assert_eq!(marker.apps.len(), 1);
        assert_eq!(marker.apps[0].name, "Already Tracked");
        assert!(marker.apps[0].standalone);
    }

    #[test]
    fn sync_ignores_a_mid_download_manifest() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        let steamapps = dir.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join("appmanifest_2.acf"),
            r#""AppState" { "appid" "2" "name" "Still Downloading" "StateFlags" "1026" }"#,
        )
        .unwrap();

        sync_marker_with_installed_apps(dir.path()).unwrap();

        let marker = read_marker(dir.path()).unwrap();
        assert!(marker.apps.is_empty());
    }
}
