//! One-click adb for users who don't have it: Google's own SDK
//! Platform-Tools zip, fetched by the user's machine straight from
//! `dl.google.com`. Tatu never ships Google's prebuilt binaries — the SDK
//! License Agreement (§3.4) forbids redistributing them. Pinned to one
//! release and SHA-256 checked (the "latest" link publishes no checksum),
//! and only the adb files are extracted.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::AndroidError;

/// To bump: change the version and BOTH SHA-256s together, after checking
/// each new archive's SHA-1 against the platform-tools entry in
/// <https://dl.google.com/android/repository/repository2-3.xml>.
pub const VERSION: &str = "37.0.1";

/// `(archive suffix, SHA-256)` for this OS. Tatu builds for Linux and
/// Windows only, hence no `darwin` entry.
#[cfg(windows)]
const ARCHIVE: (&str, &str) = (
    "win",
    "45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99",
);
#[cfg(not(windows))]
const ARCHIVE: (&str, &str) = (
    "linux",
    "d230f13842f60f782a8645f9c813f8f845bf36089ea7289f28c48f17979313f1",
);

/// The adb binary first — [`download`] returns its path. On Windows adb
/// won't start without its two USB helper DLLs next to it.
#[cfg(windows)]
const FILES: &[&str] = &["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"];
#[cfg(not(windows))]
const FILES: &[&str] = &["adb"];

/// Real archive is ~8–9 MB; anything past this isn't platform-tools.
const DOWNLOAD_LIMIT: u64 = 64 << 20;

/// Downloads platform-tools, verifies it, and extracts adb into `dir`
/// (overwriting an earlier download). Returns the adb binary's path.
pub fn download(dir: &Path) -> Result<PathBuf, AndroidError> {
    let (os, sha256) = ARCHIVE;
    let url =
        format!("https://dl.google.com/android/repository/platform-tools_r{VERSION}-{os}.zip");
    let bytes = ureq::get(&url)
        .call()
        .and_then(|mut response| {
            response
                .body_mut()
                .with_config()
                .limit(DOWNLOAD_LIMIT)
                .read_to_vec()
        })
        .map_err(|e| AndroidError::Download(format!("{url}: {e}")))?;
    if format!("{:x}", Sha256::digest(&bytes)) != sha256 {
        return Err(AndroidError::Download(format!(
            "{url} no coincide con la versión esperada (SHA-256)"
        )));
    }
    extract(&bytes, dir)
}

fn extract(zip: &[u8], dir: &Path) -> Result<PathBuf, AndroidError> {
    let bad_zip = |e: zip::result::ZipError| AndroidError::Download(e.to_string());
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(bad_zip)?;
    fs::create_dir_all(dir).map_err(|e| write_err(dir, e))?;
    for name in FILES {
        let mut data = Vec::new();
        archive
            .by_name(&format!("platform-tools/{name}"))
            .map_err(bad_zip)?
            .read_to_end(&mut data)
            .map_err(|e| AndroidError::Download(e.to_string()))?;
        let path = dir.join(name);
        fs::write(&path, data).map_err(|e| write_err(&path, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .map_err(|e| write_err(&path, e))?;
        }
    }
    Ok(dir.join(FILES[0]))
}

fn write_err(path: &Path, e: std::io::Error) -> AndroidError {
    AndroidError::Download(format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extracts_only_the_adb_files() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for name in FILES.iter().copied().chain(["fastboot", "NOTICE.txt"]) {
            zip.start_file(format!("platform-tools/{name}"), options)
                .unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();

        let dir = tempfile::tempdir().unwrap();
        let adb = extract(&bytes, dir.path()).unwrap();
        assert_eq!(adb, dir.path().join(FILES[0]));
        for name in FILES {
            assert_eq!(fs::read(dir.path().join(name)).unwrap(), name.as_bytes());
        }
        assert!(!dir.path().join("fastboot").exists());
    }
}
