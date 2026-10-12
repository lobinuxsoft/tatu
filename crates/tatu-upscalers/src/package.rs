//! The pinned OptiScaler release: downloaded from its GitHub release by the
//! user's own Tatu, checked against the SHA-256 GitHub publishes for it, and
//! extracted once into Tatu's cache for every game to share. Never bundled —
//! same reasoning as adb: the user fetches it from the source.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::{UpscalerError, io_err};

pub const VERSION: &str = "0.9.4";
const URL: &str = "https://github.com/optiscaler/OptiScaler/releases/download/v0.9.4/Optiscaler_0.9.4-final.20260718._MM.7z";
const SHA256: &str = "575cb4df866116093df75af607e37fd70e10f5163e0f23fd5c804142e80ef0ad";
const SIZE: u64 = 55_016_448;

/// Every file of the release as extracted: name, size, SHA-256. The archive's
/// own hash is checked on download; this catches a cache that changed since —
/// an antivirus quarantining a DLL, a stray file — and is the list installed,
/// so nothing else in the cache folder ever reaches a game.
pub(crate) const FILES: &[(&str, u64, &str)] = &[
    (
        "!! README_EXTRACT ALL FILES TO GAME FOLDER !!.txt",
        400,
        "09051e971a9a1bf7a1500a041b2a4652414a13d23266709cdb41e4ec12208ade",
    ),
    (
        "D3D12_Optiscaler/D3D12Core.dll",
        3351064,
        "07d286c306f8117321422affd9e6388c12d0fb4be1c7fc689d9e899324feeb24",
    ),
    (
        "Licenses/DirectX_LICENSE.txt",
        13147,
        "5239850894610071566f7ecee0b751fde43c862032d92b99d7d0f596b3433ebd",
    ),
    (
        "Licenses/FidelityFX_v1_LICENSE.md",
        1142,
        "a335c2bb2bd28184b9a30b6e35b0c7e4aa7826abd1c388fc2aa02405e9d04f03",
    ),
    (
        "Licenses/FidelityFX_v2_LICENSE.md",
        61784,
        "f0da09d71ad5c82759a179e774535d4a829e5c96c49294167c1152402b2cb400",
    ),
    (
        "Licenses/XeSS_LICENSE.txt",
        4194,
        "784be42c39a4a4d03cf85d88e15e59d75828fca773ce45b8dbfffbd7eaf208df",
    ),
    (
        "OptiScaler.dll",
        25379632,
        "fbfb6676b829dad7e020fb830586a16aa0ec6add78016db48ef12e2ae1803231",
    ),
    (
        "OptiScaler.ini",
        47477,
        "6d7ed07e2e9df3d611b6f59047716bcbc0b67834296b8e4414816189b1233aa7",
    ),
    (
        "amd_fidelityfx_dx12.dll",
        26376,
        "e2d85aa05a9bd9ed8b38935fdf5199372cca6f74c12015143bb6f945ee1608aa",
    ),
    (
        "amd_fidelityfx_framegeneration_dx12.dll",
        40085776,
        "02297beedd285e822d3a64f314cf00faf378dcec0edc47ff0c4dd71b3a8c2f18",
    ),
    (
        "amd_fidelityfx_upscaler_dx12.dll",
        28761864,
        "d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46",
    ),
    (
        "amd_fidelityfx_vk.dll",
        9332432,
        "a1624cc4238fef046f30c4d80ce3f47be63fc5f5373f49e3ee9edb9960f54c78",
    ),
    (
        "dlssg_to_fsr3_amd_is_better.dll",
        3038208,
        "806020c0444f7841a3a9f661295c16fc482e981e0666d0b9624bf6ccd19c4b5e",
    ),
    (
        "fakenvapi.dll",
        415232,
        "373e73dc06e2d66e90083fde3bc99ef5c1ed0bef98d1f7eda56c8b073269c3f1",
    ),
    (
        "fakenvapi.ini",
        466,
        "a4bfa3ec7b4badd93ae5e1ad74a582238082d871f54f38f68e7d9fb6384ce772",
    ),
    (
        "libxell.dll",
        411504,
        "61aeeab7fdcfb6d1742f86c14c68c663d759607d01a6132645c857291eaebb86",
    ),
    (
        "libxess.dll",
        77795704,
        "251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7",
    ),
    (
        "libxess_dx11.dll",
        156016,
        "c7cfe86f0c9d94e4fb3696d3cd5035e2bbb6a8b1b0572f8b7395a4cdfd0c625e",
    ),
    (
        "libxess_fg.dll",
        22957432,
        "ec5e0c65e075570c6ede72618bb666d0be0c2e10b2ea9762c0fe8cb8e375ab27",
    ),
    (
        "setup_linux.sh",
        12328,
        "0571b0ae077d985616acf9f4101e8338fc225b1b22aa7b339100c7ea3f3022be",
    ),
    (
        "setup_windows.bat",
        16947,
        "cf731a2d308df4b48db968b911b0ea1fc490fdf79d6c2ed168363312c95d2f4e",
    ),
];

/// Archive entries that are instructions for installing by hand, not part of
/// the install.
pub(crate) const NOT_INSTALLED: &[&str] = &[
    "setup_linux.sh",
    "setup_windows.bat",
    "!! README_EXTRACT ALL FILES TO GAME FOLDER !!.txt",
];

/// The extracted release under `cache`, downloading it first if needed.
/// `progress(done, total)` fires per chunk while downloading; `cancel` stops it.
pub fn ensure(
    cache: &Path,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, UpscalerError> {
    if let Some(files) = cached(cache) {
        return Ok(files);
    }
    let base = cache.join(format!("optiscaler-{VERSION}"));
    let files = base.join("files");
    fs::create_dir_all(&base).map_err(|e| io_err(&base, e))?;

    let archive = base.join("package.7z");
    let part = base.join("package.7z.part");
    download(
        &Pinned {
            url: URL,
            size: SIZE,
            sha256: SHA256,
        },
        &part,
        &mut progress,
        cancel,
    )?;
    fs::rename(&part, &archive).map_err(|e| io_err(&archive, e))?;

    // Into a scratch folder, renamed into place only when complete: a
    // half-extracted release is never mistaken for a finished one.
    let scratch = base.join("files.tmp");
    let _ = fs::remove_dir_all(&scratch);
    sevenz_rust2::decompress_file(&archive, &scratch)
        .map_err(|e| UpscalerError::Extract(e.to_string()))?;
    let _ = fs::remove_dir_all(&files);
    fs::rename(&scratch, &files).map_err(|e| io_err(&files, e))?;
    let _ = fs::remove_file(&archive);
    verify(&files)?;
    Ok(files)
}

/// The release, if it's already in the cache — every pinned file there with
/// its size (hashes are checked by [`verify`] before installing).
pub fn cached(cache: &Path) -> Option<PathBuf> {
    let files = cache.join(format!("optiscaler-{VERSION}")).join("files");
    FILES
        .iter()
        .all(|(name, size, _)| fs::metadata(files.join(name)).is_ok_and(|m| m.len() == *size))
        .then_some(files)
}

/// Every pinned file hashed, and nothing else in the folder — what gets
/// installed is exactly the release. A mismatch means the cache can't be
/// trusted: it is deleted, so the next preparation downloads it again.
pub fn verify(files: &Path) -> Result<(), UpscalerError> {
    let tampered = |name: String| {
        let _ = fs::remove_dir_all(files);
        Err(UpscalerError::Tampered(name))
    };
    for (name, _, sha) in FILES {
        if crate::patch::sha256_file(&files.join(name)).ok().as_deref() != Some(*sha) {
            return tampered((*name).to_string());
        }
    }
    let mut pending = vec![files.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| io_err(&dir, e))?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(files)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !FILES.iter().any(|(name, _, _)| *name == rel) {
                return tampered(rel);
            }
        }
    }
    Ok(())
}

/// A download known in advance: where, how big, and its SHA-256.
pub(crate) struct Pinned<'a> {
    pub url: &'a str,
    pub size: u64,
    pub sha256: &'a str,
}

/// `pin` into `to`, which is removed on any failure: a half download — or one
/// that isn't exactly the pinned file — is never kept.
pub(crate) fn download(
    pin: &Pinned,
    to: &Path,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), UpscalerError> {
    // Cancelled before it even starts (the tab's button is live from the
    // first moment): nothing is opened.
    if cancel.load(Ordering::Relaxed) {
        return Err(UpscalerError::Cancelled);
    }
    let result = download_into(pin, to, progress, cancel);
    if result.is_err() {
        let _ = fs::remove_file(to);
    }
    result
}

fn download_into(
    pin: &Pinned,
    to: &Path,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), UpscalerError> {
    // No global timeout: 78 MB takes a while on a slow link. Connecting and
    // the first response byte get a bound; the body's only is a backstop —
    // [`receive`] gives up on a connection that stops sending.
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(Duration::from_secs(6 * 60 * 60)))
            .build(),
    );
    let url = pin.url.to_string();
    let (done, sha) = receive(
        move || {
            let response = agent.get(&url).call().map_err(|e| e.to_string())?;
            Ok(Box::new(response.into_body().into_reader()) as Box<dyn Read + Send>)
        },
        to,
        pin.size,
        progress,
        cancel,
        STALL,
    )?;
    if done != pin.size || sha != pin.sha256 {
        return Err(UpscalerError::Checksum);
    }
    Ok(())
}

/// No data for this long and the connection is dead, even if it never closes.
const STALL: Duration = Duration::from_secs(60);

/// The body `open` connects to, into `to`, answering how many bytes arrived
/// and their SHA-256. Connecting and reading happen on a thread of their own,
/// so Cancel and a stalled connection are noticed within a fraction of a
/// second — also while it's still connecting. On either, that thread is left
/// behind: it never touches the file, and ends on its next read (nobody is
/// receiving anymore) or when ureq's timeouts run out.
fn receive(
    open: impl FnOnce() -> Result<Box<dyn Read + Send>, String> + Send + 'static,
    to: &Path,
    size: u64,
    progress: &mut impl FnMut(u64, u64),
    cancel: &AtomicBool,
    stall: Duration,
) -> Result<(u64, String), UpscalerError> {
    let (tx, rx) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(4);
    std::thread::spawn(move || {
        let mut body = match open() {
            Ok(body) => body,
            Err(e) => {
                let _ = tx.send(Err(std::io::Error::other(e)));
                return;
            }
        };
        loop {
            let mut buf = vec![0u8; 256 * 1024];
            let read = body.read(&mut buf).map(|n| {
                buf.truncate(n);
                buf
            });
            let last = !matches!(&read, Ok(chunk) if !chunk.is_empty());
            if tx.send(read).is_err() || last {
                return;
            }
        }
    });

    let mut file = fs::File::create(to).map_err(|e| io_err(to, e))?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut last_data = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(UpscalerError::Cancelled);
        }
        let chunk = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(chunk)) => chunk,
            Ok(Err(e)) => return Err(UpscalerError::Download(e.to_string())),
            Err(RecvTimeoutError::Timeout) if last_data.elapsed() < stall => continue,
            Err(RecvTimeoutError::Timeout) => {
                return Err(UpscalerError::Download(format!(
                    "la conexión no manda datos hace {} s",
                    stall.as_secs()
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(UpscalerError::Download("la conexión se cortó".into()));
            }
        };
        if chunk.is_empty() {
            break;
        }
        last_data = Instant::now();
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| io_err(to, e))?;
        done += chunk.len() as u64;
        if done > size {
            break;
        }
        progress(done, size);
    }
    file.flush().map_err(|e| io_err(to, e))?;
    Ok((done, format!("{:x}", hasher.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_that_isnt_exactly_the_release_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("files");
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("OptiScaler.dll"), b"not the release").unwrap();
        assert!(cached(tmp.path()).is_none());
        assert!(matches!(verify(&files), Err(UpscalerError::Tampered(_))));
        assert!(!files.exists(), "an untrusted cache is removed");
    }

    /// A connection that never sends another byte.
    struct Stalled;
    impl Read for Stalled {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(0)
        }
    }

    #[test]
    fn a_stalled_download_gives_up_and_cancel_answers_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let to = tmp.path().join("package.7z.part");
        let started = Instant::now();
        let open = || Ok(Box::new(Stalled) as Box<dyn Read + Send>);
        let stalled = receive(
            open,
            &to,
            SIZE,
            &mut |_, _| {},
            &AtomicBool::new(false),
            Duration::from_millis(300),
        );
        assert!(matches!(stalled, Err(UpscalerError::Download(_))));
        let cancelled = receive(
            open,
            &to,
            SIZE,
            &mut |_, _| {},
            &AtomicBool::new(true),
            STALL,
        );
        assert!(matches!(cancelled, Err(UpscalerError::Cancelled)));
        // Cancel also answers while it's still connecting.
        let connecting = || -> Result<Box<dyn Read + Send>, String> {
            std::thread::sleep(Duration::from_secs(3600));
            Err("never".into())
        };
        let cancelled = receive(
            connecting,
            &to,
            SIZE,
            &mut |_, _| {},
            &AtomicBool::new(true),
            STALL,
        );
        assert!(matches!(cancelled, Err(UpscalerError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(5));
        let refused =
            || -> Result<Box<dyn Read + Send>, String> { Err("connection refused".into()) };
        let failed = receive(
            refused,
            &to,
            SIZE,
            &mut |_, _| {},
            &AtomicBool::new(false),
            STALL,
        );
        assert!(matches!(failed, Err(UpscalerError::Download(e)) if e.contains("refused")));

        let (done, sha) = receive(
            || Ok(Box::new(&b"abc"[..]) as Box<dyn Read + Send>),
            &to,
            SIZE,
            &mut |_, _| {},
            &AtomicBool::new(false),
            STALL,
        )
        .unwrap();
        assert_eq!(done, 3);
        assert_eq!(
            sha,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
