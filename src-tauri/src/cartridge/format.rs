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
    let (object, _block) = find_block(&client, device).await?;

    // `device` may already be a partition rather than the whole disk — a
    // drive that already carries a filesystem (re-formatting an existing
    // cartridge) or one that shipped from the factory with extra partitions
    // (observed live: a Kingston DataTraveler with its own 100MB/300KB
    // vendor partitions alongside the main one) reports its filesystem-
    // carrying partition here, per `list_removable_drives`'s own doc
    // comment. Formatting just that partition in place would silently
    // leave any other partition, or unpartitioned space past it, never
    // reclaimed — exactly what happened live (a 28.9GB drive ending up
    // with only 9.1GB usable). Always operate on the whole disk instead.
    let (disk_object, disk_block) = whole_disk(&client, &object).await?;

    // `tear-down` below only drops fstab/crypttab tracking — it does NOT
    // unmount an actively mounted partition (confirmed live: wiping a disk
    // with its own data partition still mounted just hangs, no error, no
    // udisks2 Job ever appears). Any partition this disk already has needs
    // unmounting by hand first, same as re-formatting an existing cartridge
    // whose filesystem the caller (or Steam, scanning it as a library) still
    // has open.
    unmount_existing_partitions(&client, &disk_object).await;

    // Wipe whatever partition table (or none) is currently there and lay
    // down a fresh, empty one — `tear-down` also drops any stale fstab/
    // crypttab tracking left over from a previous life as something else.
    let mut wipe_options: HashMap<&str, Value> = HashMap::new();
    wipe_options.insert("tear-down", Value::from(true));
    disk_block
        .format("gpt", wipe_options)
        .await
        .map_err(|e| format!("Wiping the partition table failed: {e}"))?;

    let partition_table = disk_object
        .partition_table()
        .await
        .map_err(|e| format!("No partition table after wiping the disk: {e}"))?;

    let mut format_options: HashMap<&str, Value> = HashMap::new();
    format_options.insert("label", Value::from(CARTRIDGE_LABEL));
    format_options.insert("take-ownership", Value::from(true));
    // offset 0, size 0: per udisks2's own convention, size 0 means "use
    // every remaining byte on the disk" — the entire point of this rewrite.
    let partition_path = partition_table
        .create_partition_and_format(0, 0, "", "", HashMap::new(), "ntfs", format_options)
        .await
        .map_err(|e| format!("CreatePartitionAndFormat failed: {e}"))?;
    let object = client
        .object(partition_path)
        .expect("OwnedObjectPath is already a valid object path");

    // CreatePartitionAndFormat leaves the filesystem unmounted — nothing
    // re-inserted the device to trigger an auto-mount. Mount it ourselves
    // so steamapps/ and the marker can be written.
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

/// Mount an already-formatted, currently-unmounted removable drive.
/// `format_as_cartridge` mounts once right after formatting, but nothing
/// remounts it after the drive is unplugged and reconnected (or the
/// desktop's own automounter didn't run) — this is that missing step.
/// Not gated as heavily as formatting: mounting is non-destructive, so the
/// only check that matters is that `device` is still actually a removable
/// drive right now, not some other block device that reused the path.
pub async fn mount_cartridge(device: &str) -> Result<String, String> {
    let drives = list_removable_drives().await?;
    if !drives.iter().any(|d| d.id == device) {
        return Err(format!(
            "{device} is not currently a removable drive — refusing to mount"
        ));
    }

    let client = udisks2::Client::new()
        .await
        .map_err(|e| format!("Cannot connect to udisks2: {e}"))?;
    let (object, _block) = find_block(&client, device).await?;
    let filesystem = object
        .filesystem()
        .await
        .map_err(|e| format!("{device} has no filesystem interface: {e}"))?;
    filesystem
        .mount(HashMap::new())
        .await
        .map_err(|e| format!("Mount failed: {e}"))
}

/// Unmounts every currently-mounted partition already on `disk_object`,
/// best-effort — a blank drive has no partition table at all (nothing to
/// do), and a partition with no filesystem interface or that's already
/// unmounted just errors on the attempt, which is exactly the outcome
/// wanted either way. Never returns an error itself: the caller's own
/// `format()` call is what actually needs to succeed, not this cleanup.
async fn unmount_existing_partitions(client: &udisks2::Client, disk_object: &udisks2::Object) {
    let Ok(partition_table) = disk_object.partition_table().await else {
        return;
    };
    let Ok(partitions) = partition_table.partitions().await else {
        return;
    };
    for path in partitions {
        let partition_object = client
            .object(path)
            .expect("OwnedObjectPath is already a valid object path");
        let Ok(filesystem) = partition_object.filesystem().await else {
            continue;
        };
        let _ = filesystem.unmount(HashMap::new()).await;
    }
}

/// Resolves `object` to the whole-disk `Object`/`BlockProxy` pair, following
/// `Partition.Table` up to the parent block device if `object` turns out to
/// be a partition rather than the disk itself.
async fn whole_disk(
    client: &udisks2::Client,
    object: &udisks2::Object,
) -> Result<(udisks2::Object, BlockProxy<'static>), String> {
    let Ok(partition) = object.partition().await else {
        let block = object
            .block()
            .await
            .map_err(|e| format!("No block interface on the disk: {e}"))?;
        return Ok((object.clone(), block));
    };
    let table_path = partition
        .table()
        .await
        .map_err(|e| format!("Cannot read the partition's table property: {e}"))?;
    let disk_object = client
        .object(table_path)
        .expect("OwnedObjectPath is already a valid object path");
    let disk_block = disk_object
        .block()
        .await
        .map_err(|e| format!("No block interface on the whole disk: {e}"))?;
    Ok((disk_object, disk_block))
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
