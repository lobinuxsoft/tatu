use serde::{Deserialize, Serialize};

/// A removable, non-fixed drive as reported by the OS. `id` is the handle
/// later calls (format, register, install) act on: a device path on Linux
/// (`/dev/sdb1`), a drive letter on Windows (`E:\`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovableDrive {
    pub id: String,
    pub label: String,
    pub total_bytes: u64,
    pub mount_point: Option<String>,
}

/// Enumerate removable drives only — never a fixed/internal disk. This is
/// the safety gate every destructive cartridge operation (#194) re-checks
/// immediately before acting, so a false positive here is the one bug in
/// this epic that actually costs the user data.
#[cfg(unix)]
pub async fn list_removable_drives() -> Result<Vec<RemovableDrive>, String> {
    use std::collections::{HashMap, HashSet};

    let client = udisks2::Client::new()
        .await
        .map_err(|e| format!("Cannot connect to udisks2: {e}"))?;
    let objects = client
        .object_manager()
        .get_managed_objects()
        .await
        .map_err(|e| format!("udisks2 GetManagedObjects failed: {e}"))?;

    // Pass 1: which drive objects are actually removable media.
    let mut removable_drives: HashSet<String> = HashSet::new();
    for path in objects.keys() {
        // An OwnedObjectPath is already a valid object path, so this
        // conversion cannot fail — only the interface lookups below can.
        let object = client
            .object(path.clone())
            .expect("OwnedObjectPath is already a valid object path");
        let Ok(drive) = object.drive().await else {
            continue;
        };
        let removable = drive.removable().await.unwrap_or(false)
            || drive.media_removable().await.unwrap_or(false);
        if removable {
            removable_drives.insert(path.to_string());
        }
    }

    // Pass 2: every block belonging to one of those drives. A drive can own
    // several blocks (the whole disk plus each partition) — keep the one
    // that actually carries a filesystem (what the user mounts) over the
    // bare whole-disk block, so a blank drive still surfaces for #194 to
    // format while a prepared cartridge reports its real mount point.
    struct Candidate {
        drive: String,
        info: RemovableDrive,
        has_fs: bool,
    }
    let mut candidates = Vec::new();
    for path in objects.keys() {
        // An OwnedObjectPath is already a valid object path, so this
        // conversion cannot fail — only the interface lookups below can.
        let object = client
            .object(path.clone())
            .expect("OwnedObjectPath is already a valid object path");
        let Ok(block) = object.block().await else {
            continue;
        };
        let Ok(drive_path) = block.drive().await else {
            continue;
        };
        let drive_key = drive_path.to_string();
        if !removable_drives.contains(&drive_key) {
            continue;
        }
        if block.hint_ignore().await.unwrap_or(false) {
            continue;
        }

        let filesystem = object.filesystem().await.ok();
        let mount_point = match &filesystem {
            Some(fs) => fs
                .mount_points()
                .await
                .unwrap_or_default()
                .into_iter()
                .next()
                .map(|bytes| bytes_to_string(&bytes)),
            None => None,
        };

        candidates.push(Candidate {
            drive: drive_key,
            info: RemovableDrive {
                id: bytes_to_string(&block.device().await.unwrap_or_default()),
                label: block.id_label().await.unwrap_or_default(),
                total_bytes: block.size().await.unwrap_or(0),
                mount_point,
            },
            has_fs: filesystem.is_some(),
        });
    }

    let mut best: HashMap<String, Candidate> = HashMap::new();
    for candidate in candidates {
        match best.get(&candidate.drive) {
            None => {
                best.insert(candidate.drive.clone(), candidate);
            }
            Some(existing) => {
                let existing_rank = (existing.info.mount_point.is_some(), existing.has_fs);
                let candidate_rank = (candidate.info.mount_point.is_some(), candidate.has_fs);
                if candidate_rank > existing_rank {
                    best.insert(candidate.drive.clone(), candidate);
                }
            }
        }
    }

    Ok(best.into_values().map(|c| c.info).collect())
}

/// D-Bus byte-array properties (device path, mount point) are NUL-terminated
/// C strings; strip the terminator before decoding.
#[cfg(unix)]
fn bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(windows)]
pub async fn list_removable_drives() -> Result<Vec<RemovableDrive>, String> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOVABLE;

    let mut drives = Vec::new();
    let mask = unsafe { GetLogicalDrives() };

    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root: Vec<u16> = format!("{letter}:\\")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: `root` is a valid NUL-terminated wide string for the
        // lifetime of each call below.
        let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
        if drive_type != DRIVE_REMOVABLE {
            continue;
        }

        let mut label_buf = [0u16; 261]; // MAX_PATH + 1
        let got_label = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        let label = if got_label != 0 {
            let end = label_buf.iter().position(|&c| c == 0).unwrap_or(0);
            String::from_utf16_lossy(&label_buf[..end])
        } else {
            // No filesystem yet (unformatted media) — still a valid,
            // listable removable drive for #194 to format.
            String::new()
        };

        let mut total_bytes: u64 = 0;
        unsafe {
            GetDiskFreeSpaceExW(
                root.as_ptr(),
                std::ptr::null_mut(),
                &mut total_bytes,
                std::ptr::null_mut(),
            );
        }

        let root_str: String = format!("{letter}:\\");
        drives.push(RemovableDrive {
            id: root_str.clone(),
            label,
            total_bytes,
            mount_point: Some(root_str),
        });
    }

    Ok(drives)
}
