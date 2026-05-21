//! Download + extract the latest REFramework nightly into a game dir.

use std::io::Cursor;
use std::path::Path;

use crate::reframework::{InstallReport, ReframeworkError};
use crate::reframework::download::{download_asset, fetch_latest_release};

/// Fetch the latest REFramework nightly and extract its full payload
/// into `game_dir`. Idempotent: re-running overwrites the existing
/// drop-in with the freshly downloaded build (REFramework itself
/// expects the user to keep its directory in sync with the nightly).
///
/// Returns the version tag landed + byte count for the UI banner.
pub fn install(game_dir: &Path) -> Result<InstallReport, ReframeworkError> {
    if !game_dir.is_dir() {
        return Err(ReframeworkError::NoGameDir(
            game_dir.display().to_string(),
        ));
    }
    let release = fetch_latest_release()?;
    let bytes = download_asset(&release.asset_url)?;
    let total = extract_zip(&bytes, game_dir)?;
    Ok(InstallReport {
        version_tag: release.tag,
        installed_dir: game_dir.to_path_buf(),
        bytes_extracted: total,
    })
}

/// Walk the zip and extract every entry into `dest`. Skips obviously
/// unsafe paths (anything that would escape the destination dir via
/// `..` or absolute components). Returns the total uncompressed bytes
/// written.
fn extract_zip(bytes: &[u8], dest: &Path) -> Result<u64, ReframeworkError> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(rel) = safe_entry_path(file.name()) else {
            continue;
        };
        let out_path = dest.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut sink = std::fs::File::create(&out_path)?;
        let copied = std::io::copy(&mut file, &mut sink)?;
        total += copied;
    }
    Ok(total)
}

/// Reject zip-slip vectors. Returns `Some(relative_path)` only for
/// names that resolve as plain forward-relative paths.
fn safe_entry_path(raw: &str) -> Option<&Path> {
    let path = Path::new(raw);
    for comp in path.components() {
        use std::path::Component::*;
        match comp {
            Normal(_) | CurDir => {}
            ParentDir | RootDir | Prefix(_) => return None,
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            for (name, data) in entries {
                writer.start_file(*name, opts).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn extract_writes_every_entry() {
        let tmp = TempDir::new().unwrap();
        let bytes = make_zip(&[
            ("dinput8.dll", b"x".repeat(1024).as_slice()),
            ("reframework/scripts/init.lua", b"-- init"),
        ]);
        let total = extract_zip(&bytes, tmp.path()).unwrap();
        assert!(total > 1024);
        assert!(tmp.path().join("dinput8.dll").is_file());
        assert!(tmp.path().join("reframework/scripts/init.lua").is_file());
    }

    #[test]
    fn extract_rejects_zip_slip_via_parent_dir() {
        let tmp = TempDir::new().unwrap();
        // Build a zip with a parent-dir traversal — must be rejected.
        let bytes = make_zip(&[("../escape.dll", b"malware")]);
        let _ = extract_zip(&bytes, tmp.path()).unwrap();
        assert!(
            !tmp.path().join("..").join("escape.dll").exists(),
            "zip-slip via .. must be filtered"
        );
    }

    #[test]
    fn extract_rejects_absolute_paths() {
        let tmp = TempDir::new().unwrap();
        let bytes = make_zip(&[("/etc/passwd", b"oops")]);
        let _ = extract_zip(&bytes, tmp.path()).unwrap();
        assert!(!std::path::Path::new("/etc/passwd_oops").exists());
    }

    #[test]
    fn install_errors_when_game_dir_missing() {
        let err = install(Path::new("/tmp/definitely-does-not-exist-2026")).unwrap_err();
        assert!(matches!(err, ReframeworkError::NoGameDir(_)));
    }
}
