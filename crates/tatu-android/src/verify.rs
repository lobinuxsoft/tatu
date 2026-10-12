//! Folder-level integrity check: every local file's SHA-256 against the
//! phone's copy, with the device side hashed by ONE shell command.
//! GameNativeTool ran one `adb shell sha256sum` per file — thousands of adb
//! round-trips for a real game.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::AndroidError;
use crate::remote::quote;

/// One regular file of the game folder. `rel` uses `/` separators — the
/// same shape the device-side `find .` prints, minus its leading `./`.
pub(crate) struct LocalFile {
    pub rel: String,
    pub path: PathBuf,
    pub size: u64,
}

/// Every regular file under `root`. Symlinks are skipped on both sides
/// (`find -type f` doesn't list them either): shared storage can't hold
/// one anyway.
pub(crate) fn walk(root: &Path) -> Result<Vec<LocalFile>, AndroidError> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| read_err(&dir, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| read_err(&dir, e))?;
            let path = entry.path();
            let rel = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let kind = entry.file_type().map_err(|e| read_err(&path, e))?;
            if kind.is_dir() {
                pending.push((path, format!("{rel}/")));
            } else if kind.is_file() {
                let size = entry.metadata().map_err(|e| read_err(&path, e))?.len();
                files.push(LocalFile { rel, path, size });
            }
        }
    }
    Ok(files)
}

/// `rel → hex digest` for every local file. `hashed` counts bytes as they're
/// read (the progress bar); `stop` is polled per 1 MiB chunk so a cancel
/// never waits out a multi-gigabyte pak file.
pub(crate) fn hash_all(
    files: &[LocalFile],
    hashed: &AtomicU64,
    stop: impl Fn() -> bool,
) -> Result<HashMap<String, String>, AndroidError> {
    let mut buf = vec![0u8; 1 << 20];
    let mut digests = HashMap::with_capacity(files.len());
    for file in files {
        let mut reader = fs::File::open(&file.path).map_err(|e| read_err(&file.path, e))?;
        let mut hasher = Sha256::new();
        loop {
            if stop() {
                return Err(AndroidError::Cancelled);
            }
            let n = reader.read(&mut buf).map_err(|e| read_err(&file.path, e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            hashed.fetch_add(n as u64, Ordering::Relaxed);
        }
        digests.insert(file.rel.clone(), format!("{:x}", hasher.finalize()));
    }
    Ok(digests)
}

/// Hashes every file under `dir` on the phone in one go. `-exec … {} +`
/// batches the file list under the shell's argument limit on its own.
pub(crate) fn device_hash_script(dir: &str) -> String {
    format!("cd {} && find . -type f -exec sha256sum {{}} +", quote(dir))
}

/// `rel → hex digest` from `sha256sum`'s `<hex>  ./<path>` lines (toybox),
/// or `<hex> *./<path>` (coreutils' binary-mode marker, on ROMs that ship it).
pub(crate) fn parse_listing(out: &str) -> HashMap<String, String> {
    out.lines()
        .filter_map(|line| {
            let (hash, rest) = line.split_once(' ')?;
            let path = rest.strip_prefix([' ', '*']).unwrap_or(rest);
            Some((
                path.strip_prefix("./").unwrap_or(path).to_string(),
                hash.to_ascii_lowercase(),
            ))
        })
        .collect()
}

pub(crate) fn compare(
    local: &HashMap<String, String>,
    device: &HashMap<String, String>,
) -> Result<(), AndroidError> {
    let mut missing = Vec::new();
    let mut different = Vec::new();
    for (rel, hash) in local {
        match device.get(rel) {
            None => missing.push(rel.as_str()),
            Some(other) if other != hash => different.push(rel.as_str()),
            Some(_) => {}
        }
    }
    if missing.is_empty() && different.is_empty() {
        return Ok(());
    }
    missing.sort_unstable();
    different.sort_unstable();
    let example = missing
        .first()
        .or(different.first())
        .copied()
        .unwrap_or_default();
    Err(AndroidError::VerifyMismatch(format!(
        "{} archivo(s) no llegaron y {} llegaron distintos (por ejemplo: {example})",
        missing.len(),
        different.len()
    )))
}

fn read_err(path: &Path, source: std::io::Error) -> AndroidError {
    AndroidError::LocalRead {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_tree_matches_a_device_listing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Content/Paks")).unwrap();
        fs::write(dir.path().join("game.exe"), b"abc").unwrap();
        fs::write(dir.path().join("Content/Paks/a b's.pak"), b"").unwrap();

        let files = walk(dir.path()).unwrap();
        let hashed = AtomicU64::new(0);
        let local = hash_all(&files, &hashed, || false).unwrap();
        assert_eq!(hashed.load(Ordering::Relaxed), 3);

        // What toybox `sha256sum` prints on the phone for the same two files.
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let listing = format!("{abc}  ./game.exe\n{empty} *./Content/Paks/a b's.pak\n");
        assert!(compare(&local, &parse_listing(&listing)).is_ok());

        let truncated = format!("{empty}  ./game.exe\n");
        let err = compare(&local, &parse_listing(&truncated)).unwrap_err();
        assert!(
            err.to_string()
                .contains("1 archivo(s) no llegaron y 1 llegaron distintos")
        );
    }

    #[test]
    fn hashing_stops_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("big.pak"), b"x").unwrap();
        let files = walk(dir.path()).unwrap();
        let result = hash_all(&files, &AtomicU64::new(0), || true);
        assert!(matches!(result, Err(AndroidError::Cancelled)));
    }
}
