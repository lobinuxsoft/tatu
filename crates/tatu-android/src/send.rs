//! The whole transfer: preflight → push into staging → verify → rename.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::AndroidError;
use crate::adb::{Adb, POLL, wait_cancellable};
use crate::remote::{join, parse_df_available, parse_du_bytes, quote, validate_remote_path};
use crate::verify::{LocalFile, compare, device_hash_script, hash_all, parse_listing, walk};

/// How often the push polls the staging folder's size on the phone — each
/// poll is its own `adb shell du`, so not every [`POLL`].
const DU_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Pushing,
    Verifying,
}

/// `current`/`total` are bytes: copied so far while pushing, hashed on this
/// machine while verifying.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub phase: Phase,
    pub current: u64,
    pub total: u64,
}

/// Copies `local_dir` to `<remote_root>/<its own folder name>` on `serial`
/// and returns that final path.
///
/// The copy lands in a hidden `.tatu-staging-*` folder and is only renamed
/// into place once every file's SHA-256 matches the original, so GameNative
/// can never pick up a half-copied or corrupted game. An existing final
/// folder is never overwritten — that's a hard error. On any failure,
/// cancel included, the staging folder is deleted: no gigabytes of an
/// unverified copy left behind on the phone.
///
/// The folder name is kept verbatim on purpose: GameNative turns a custom
/// folder into a real Steam library entry when its name equals the game's
/// Steam `installdir` (`CustomGameScanner.createLibraryItemFromFolder`) —
/// exactly what a Steam install's own folder is called.
pub fn send_folder(
    adb: &Adb,
    serial: &str,
    local_dir: &Path,
    remote_root: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<String, AndroidError> {
    validate_remote_path(remote_root)?;
    let name = local_dir
        .file_name()
        .ok_or_else(|| AndroidError::BadSource(local_dir.display().to_string()))?
        .to_string_lossy();
    let final_dir = join(remote_root, &name);
    let staging = join(remote_root, &format!(".tatu-staging-{name}"));
    // The folder name is part of both paths and comes from the local disk.
    validate_remote_path(&final_dir)?;

    let files = walk(local_dir)?;
    let total: u64 = files.iter().map(|f| f.size).sum();

    // `rm -rf` of the staging name only ever hits a leftover of Tatu's own
    // from an interrupted run — nobody else names a folder `.tatu-staging-*`.
    let existing = adb.shell(
        serial,
        &format!(
            "mkdir -p {root} && rm -rf {staging} && if [ -e {final_dir} ]; then echo exists; fi",
            root = quote(remote_root),
            staging = quote(&staging),
            final_dir = quote(&final_dir),
        ),
    )?;
    if existing.trim() == "exists" {
        return Err(AndroidError::AlreadyExists(final_dir));
    }
    // Advisory: an unparseable `df` (odd OEM builds) just skips the check —
    // the push itself still fails loudly on a full disk.
    let df = adb.shell(serial, &format!("df -k {}", quote(remote_root)))?;
    if let Some(available) = parse_df_available(&df)
        && available < total
    {
        return Err(AndroidError::NoSpace {
            needed_gb: gb(total),
            available_gb: gb(available),
        });
    }

    let result = push(
        adb,
        serial,
        local_dir,
        &staging,
        total,
        cancel,
        &mut on_progress,
    )
    .and_then(|()| {
        verify_copy(
            adb,
            serial,
            &files,
            &staging,
            total,
            cancel,
            &mut on_progress,
        )
    })
    .and_then(|()| commit(adb, serial, &staging, &final_dir));
    if result.is_err() {
        let _ = adb.shell(serial, &format!("rm -rf {}", quote(&staging)));
    }
    result.map(|()| final_dir)
}

fn push(
    adb: &Adb,
    serial: &str,
    local_dir: &Path,
    staging: &str,
    total: u64,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(Progress),
) -> Result<(), AndroidError> {
    // `staging` doesn't exist yet (just removed), so adb creates it with the
    // folder's CONTENTS — not `staging/<name>/`. adb's sync protocol takes
    // the remote path raw: no shell, no quoting.
    let local = local_dir.to_string_lossy();
    let child = adb.spawn(&["-s", serial, "push", &local, staging])?;
    let mut last_poll = Instant::now();
    let done = wait_cancellable(child, cancel, || {
        if last_poll.elapsed() < DU_EVERY {
            return;
        }
        last_poll = Instant::now();
        if let Ok(out) = adb.shell(serial, &format!("du -sk {}", quote(staging)))
            && let Some(bytes) = parse_du_bytes(&out)
        {
            // `du` counts whole blocks, so it can overshoot the real size.
            on_progress(Progress {
                phase: Phase::Pushing,
                current: bytes.min(total),
                total,
            });
        }
    })?;
    if !done.status.success() {
        return Err(AndroidError::Adb {
            command: "push".into(),
            stderr: done.stderr.trim().to_string(),
        });
    }
    on_progress(Progress {
        phase: Phase::Pushing,
        current: total,
        total,
    });
    Ok(())
}

/// The phone hashes its copy while this machine hashes the original — two
/// different disks and CPUs, so running them side by side is free time.
fn verify_copy(
    adb: &Adb,
    serial: &str,
    files: &[LocalFile],
    staging: &str,
    total: u64,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(Progress),
) -> Result<(), AndroidError> {
    let hashed = AtomicU64::new(0);
    let device_failed = AtomicBool::new(false);
    let child = adb.spawn(&["-s", serial, "shell", &device_hash_script(staging)])?;

    thread::scope(|s| {
        let local = s.spawn(|| {
            hash_all(files, &hashed, || {
                cancel.load(Ordering::Relaxed) || device_failed.load(Ordering::Relaxed)
            })
        });
        let mut tick = || {
            on_progress(Progress {
                phase: Phase::Verifying,
                current: hashed.load(Ordering::Relaxed),
                total,
            })
        };
        let device = wait_cancellable(child, cancel, &mut tick).and_then(|done| {
            if done.status.success() {
                Ok(done.stdout)
            } else {
                Err(AndroidError::Adb {
                    command: "sha256sum".into(),
                    stderr: done.stderr.trim().to_string(),
                })
            }
        });
        if device.is_err() {
            // No point hashing the rest locally for a check that can't happen.
            device_failed.store(true, Ordering::Relaxed);
        }
        // The phone may finish first; keep the bar moving until this side does.
        while !local.is_finished() {
            tick();
            thread::sleep(POLL);
        }
        tick();
        let listing = device?;
        let local = local.join().expect("local hashing thread panicked")?;
        compare(&local, &parse_listing(&listing))
    })
}

/// Re-checks the final name at the last moment: something on the phone may
/// have created it while the copy ran, and it must never be overwritten.
fn commit(adb: &Adb, serial: &str, staging: &str, final_dir: &str) -> Result<(), AndroidError> {
    let out = adb.shell(
        serial,
        &format!(
            "if [ -e {f} ]; then echo exists; else mv {s} {f}; fi",
            f = quote(final_dir),
            s = quote(staging),
        ),
    )?;
    if out.trim() == "exists" {
        return Err(AndroidError::AlreadyExists(final_dir.to_string()));
    }
    Ok(())
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}
