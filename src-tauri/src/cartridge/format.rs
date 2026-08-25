use std::collections::HashMap;

use udisks2::block::BlockProxy;
use udisks2::zbus::zvariant::Value;

use super::drives::{bytes_to_string, list_removable_drives};
use super::marker::{CARTRIDGE_LABEL, write_marker};

/// Format `device` as a cartridge: NTFS filesystem, `steamapps/` created,
/// an empty #193 marker written. Every gate below is mandatory — this is
/// the one operation in the epic that destroys data on a wrong pick.
pub async fn format_as_cartridge(
    device: &str,
    expected_label: &str,
    expected_bytes: u64,
) -> Result<(), String> {
    // Gate 1: re-run the removable check on this exact device right now —
    // never trust a list read even a few seconds earlier.
    let drives = list_removable_drives().await?;
    let current = drives.iter().find(|d| d.id == device).ok_or_else(|| {
        format!("{device} is not currently a removable drive — refusing to format")
    })?;

    // Gate 2: reject if what udisks reports right now drifted from what the
    // caller listed. Catches the user unplugging the intended drive and
    // something else landing at the same device path in between.
    if current.label != expected_label || current.total_bytes != expected_bytes {
        return Err(format!(
            "{device} no longer matches what was listed (label {expected_label:?} -> {:?}, \
             size {expected_bytes} -> {}) — refusing, a different drive may now be here",
            current.label, current.total_bytes
        ));
    }

    // Gate 3: a read-only drive (write-protect switch, failed dirty-bit
    // check) would fail deep inside udisks2's Format call with a raw D-Bus
    // error — catch it here with a message that actually says why.
    if current.read_only {
        return Err(format!("{device} is read-only — refusing to format"));
    }

    let client = udisks2::Client::new()
        .await
        .map_err(|e| format!("Cannot connect to udisks2: {e}"))?;
    let (object, block) = find_block(&client, device).await?;

    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("label", Value::from(CARTRIDGE_LABEL));
    options.insert("update-partition-type", Value::from(true));
    options.insert("take-ownership", Value::from(true));
    block
        .format("ntfs", options)
        .await
        .map_err(|e| format!("Format failed: {e}"))?;

    // Format leaves the filesystem unmounted — nothing re-inserted the
    // device to trigger an auto-mount. Mount it ourselves so steamapps/
    // and the marker can be written.
    let filesystem = object
        .filesystem()
        .await
        .map_err(|e| format!("Formatted, but no filesystem interface appeared: {e}"))?;
    let mount_path = filesystem
        .mount(HashMap::new())
        .await
        .map_err(|e| format!("Formatted, but mount failed: {e}"))?;
    let mount_point = std::path::Path::new(&mount_path);

    std::fs::create_dir_all(mount_point.join("steamapps"))
        .map_err(|e| format!("Formatted and mounted, but cannot create steamapps/: {e}"))?;
    write_marker(mount_point)
}

/// Locate the `Object`/`BlockProxy` pair for a device path (e.g. `/dev/sdb1`)
/// by scanning udisks2's managed objects — safer than reconstructing its
/// `/org/freedesktop/UDisks2/block_devices/<name>` path convention by hand.
async fn find_block(
    client: &udisks2::Client,
    device: &str,
) -> Result<(udisks2::Object, BlockProxy<'static>), String> {
    let objects = client
        .object_manager()
        .get_managed_objects()
        .await
        .map_err(|e| format!("udisks2 GetManagedObjects failed: {e}"))?;

    for path in objects.keys() {
        let object = client
            .object(path.clone())
            .expect("OwnedObjectPath is already a valid object path");
        let Ok(block) = object.block().await else {
            continue;
        };
        let Ok(dev_bytes) = block.device().await else {
            continue;
        };
        if bytes_to_string(&dev_bytes) == device {
            return Ok((object, block));
        }
    }

    Err(format!("No udisks2 block object found for {device}"))
}
