use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sysinfo::Disks;

use super::install::{acf_field, appmanifest_path};
use super::marker::list_apps;

/// One installed game's total footprint on the cartridge — every file keyed
/// to its app_id, summed into a single number. The player doesn't care that
/// `common/`, `compatdata/`, `shadercache/`, and `assets/<app_id>/` are
/// separate directories; they only want "how much does this game cost me."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppUsage {
    pub app_id: u64,
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartridgeUsage {
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// `tatu-launcher` + `tatu-launcher.exe` + the whole bundled `runtime/`
    /// (#206) as ONE number — same reasoning as `AppUsage.bytes` above, the
    /// player sees "the launcher", never its individual pieces.
    pub launcher_bytes: u64,
    pub apps: Vec<AppUsage>,
    /// `used_bytes - launcher_bytes - sum(apps)` — the marker file, Steam's
    /// own `steamapps/` bookkeeping, anything not accounted for above.
    /// Keeps the breakdown always adding up to what the filesystem itself
    /// reports as used, rather than silently coming up short.
    pub other_bytes: u64,
}

/// Walks the whole tree summing real file sizes — `path` itself may be a
/// plain file (the launcher binaries at the cartridge root) or a directory
/// (`runtime/`, `steamapps/common/<installdir>`, ...). A missing path (a
/// game with no trailer downloaded, no compatdata yet) is just 0, not an
/// error — most of these categories are optional by design.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|entry| size_of(&entry.path())).sum()
}

/// Total/free space for the filesystem `mount_point` lives on — the most
/// specific (longest) matching mount in `sysinfo`'s own list, so a cartridge
/// mounted under another filesystem's tree still resolves to its own real
/// numbers rather than the parent's.
fn disk_space(mount_point: &Path) -> Result<(u64, u64), String> {
    let canonical = fs::canonicalize(mount_point).map_err(|e| e.to_string())?;
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space(), disk.available_space()))
        .ok_or_else(|| format!("No filesystem found for {}", mount_point.display()))
}

/// Computes the disk usage breakdown (#228) for a cartridge already known to
/// have the standard layout (`has_cartridge_structure` checked by the
/// caller, same as every other cartridge command).
pub fn usage(mount_point: &Path) -> Result<CartridgeUsage, String> {
    let (total_bytes, free_bytes) = disk_space(mount_point)?;
    let used_bytes = total_bytes.saturating_sub(free_bytes);

    let mut launcher_bytes = size_of(&mount_point.join("runtime"));
    for binary in ["tatu-launcher", "tatu-launcher.exe"] {
        launcher_bytes += size_of(&mount_point.join(binary));
    }

    let mut apps = Vec::new();
    let mut apps_bytes = 0u64;
    for app in list_apps(mount_point)? {
        let mut bytes = size_of(&mount_point.join("assets").join(app.app_id.to_string()));

        if let Ok(content) = fs::read_to_string(appmanifest_path(mount_point, app.app_id))
            && let Some(installdir) = acf_field(&content, "installdir")
        {
            bytes += size_of(
                &mount_point
                    .join("steamapps")
                    .join("common")
                    .join(&installdir),
            );
        }
        for subdir in ["compatdata", "shadercache", "downloading"] {
            bytes += size_of(
                &mount_point
                    .join("steamapps")
                    .join(subdir)
                    .join(app.app_id.to_string()),
            );
        }

        apps_bytes += bytes;
        apps.push(AppUsage {
            app_id: app.app_id,
            name: app.name,
            bytes,
        });
    }

    let other_bytes = used_bytes.saturating_sub(launcher_bytes + apps_bytes);

    Ok(CartridgeUsage {
        total_bytes,
        free_bytes,
        launcher_bytes,
        apps,
        other_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::marker::{CartridgeApp, add_app, write_marker};
    use crate::drm::Preservability;

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
    fn size_of_sums_files_recursively() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), vec![0u8; 10]).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub").join("b.txt"), vec![0u8; 20]).unwrap();
        assert_eq!(size_of(dir.path()), 30);
    }

    #[test]
    fn size_of_missing_path_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(size_of(&dir.path().join("nope")), 0);
    }

    #[test]
    fn usage_combines_the_launcher_and_keeps_apps_separate() {
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path()).unwrap();
        add_app(dir.path(), app(1, "Game One")).unwrap();

        fs::write(dir.path().join("tatu-launcher"), vec![0u8; 5]).unwrap();
        fs::create_dir_all(dir.path().join("runtime")).unwrap();
        fs::write(dir.path().join("runtime").join("umu-run"), vec![0u8; 7]).unwrap();

        let steamapps = dir.path().join("steamapps");
        fs::create_dir_all(steamapps.join("common").join("GameOne")).unwrap();
        fs::write(
            steamapps.join("common").join("GameOne").join("game.bin"),
            vec![0u8; 100],
        )
        .unwrap();
        fs::write(
            steamapps.join("appmanifest_1.acf"),
            r#""AppState" { "appid" "1" "installdir" "GameOne" "StateFlags" "4" }"#,
        )
        .unwrap();

        let result = usage(dir.path()).unwrap();
        assert_eq!(result.launcher_bytes, 12);
        assert_eq!(
            result.apps,
            vec![AppUsage {
                app_id: 1,
                name: "Game One".into(),
                bytes: 100,
            }]
        );
    }
}
