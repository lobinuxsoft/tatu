use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::drives::bytes_to_string;

/// udisks2 forces `windows_names` onto every NTFS drive it automounts by
/// default (storaged-project/udisks#620) — it forbids the `:` in Wine's own
/// `dosdevices/c:` symlink, so Proton's prefix creation fails with EINVAL on
/// every cartridge plugged in and auto-mounted. A cartridge mounted through
/// `/etc/fstab` instead never hits this, since fstab bypasses udisks2
/// entirely — confirmed live: identical `ln -s ../drive_c` fails only on the
/// udisks2-automounted path.
const CONF_PATH: &str = "/etc/udisks2/mount_options.conf";

/// Exempts `mount_point`'s NTFS device from `windows_names`, scoped to that
/// device's UUID only (other NTFS drives on the machine are untouched), and
/// remounts it so the change takes effect immediately. Returns `false` if
/// the exemption was already present — safe to call every time "Preparar
/// launcher" runs. Requires one interactive `pkexec` authorization the
/// first time per device, since `/etc/udisks2/mount_options.conf` is
/// root-owned and udisks2 exposes no D-Bus API for editing it.
pub async fn ensure_symlinks(mount_point: &Path) -> Result<bool, String> {
    let client = udisks2::Client::new()
        .await
        .map_err(|e| format!("Cannot connect to udisks2: {e}"))?;
    let (object, uuid) = find_uuid(&client, mount_point).await?;

    let marker = format!("[/dev/disk/by-uuid/{uuid}]");
    let existing = fs::read_to_string(CONF_PATH).unwrap_or_default();
    if existing.contains(&marker) {
        return Ok(false);
    }

    let updated = format!("{existing}\n{marker}\nntfs_defaults=uid=$UID,gid=$GID\n");
    let tmp = std::env::temp_dir().join(format!("tatu-mount-options-{uuid}.conf"));
    fs::write(&tmp, &updated).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;

    let tmp_arg = tmp.to_string_lossy().into_owned();
    // `pkexec` blocks on an interactive graphical prompt the user may not
    // answer for a while — spawn_blocking keeps that off the async
    // executor, same reasoning `install_launcher_binaries` already applies
    // to its own slow blocking copy.
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new("pkexec")
            .args(["install", "-m", "0644", &tmp_arg, CONF_PATH])
            .status()
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
    .map_err(|e| format!("Cannot run pkexec: {e}"))?;
    let _ = fs::remove_file(&tmp);

    if !status.success() {
        return Err("Autorización de administrador cancelada o fallida".to_string());
    }

    let filesystem = object
        .filesystem()
        .await
        .map_err(|e| format!("No filesystem interface to remount: {e}"))?;
    filesystem
        .unmount(HashMap::new())
        .await
        .map_err(|e| format!("Unmount failed: {e}"))?;
    filesystem
        .mount(HashMap::new())
        .await
        .map_err(|e| format!("Remount failed: {e}"))?;

    Ok(true)
}

/// Finds the udisks2 object mounted at `mount_point` and reads its device
/// UUID — the caller only has the mount path, not the device.
async fn find_uuid(
    client: &udisks2::Client,
    mount_point: &Path,
) -> Result<(udisks2::Object, String), String> {
    let objects = client
        .object_manager()
        .get_managed_objects()
        .await
        .map_err(|e| format!("udisks2 GetManagedObjects failed: {e}"))?;
    let target = mount_point.to_string_lossy();

    for path in objects.keys() {
        let object = client
            .object(path.clone())
            .expect("OwnedObjectPath is already a valid object path");
        let Ok(filesystem) = object.filesystem().await else {
            continue;
        };
        let Ok(points) = filesystem.mount_points().await else {
            continue;
        };
        if !points.iter().any(|p| bytes_to_string(p) == target) {
            continue;
        }
        let Ok(block) = object.block().await else {
            continue;
        };
        let uuid = block
            .id_uuid()
            .await
            .map_err(|e| format!("Cannot read IdUUID: {e}"))?;
        return Ok((object, uuid));
    }

    Err(format!(
        "No udisks2 object is mounted at {}",
        mount_point.display()
    ))
}
