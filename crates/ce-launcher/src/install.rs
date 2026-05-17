use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CE_VERSION: &str = "7.6.6";
pub const CE_ZIP_URL: &str = "https://cheatengine.org/download/CheatEngineLinux766-6.zip";
pub const CE_ZIP_SHA256: &str = "d390da973c90f553d966ed3dc792a9f09adb55646a6b106acb16286ec4eabf64";
pub const CE_ZIP_SIZE: u64 = 24_123_457;

const EXTRACT_DIRNAME: &str = "CheatEngineLinux766-6";
const BINARY_NAME: &str = "cheatengine-x86_64";
const DOWNLOAD_LIMIT: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeInstall {
    pub binary: PathBuf,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CeStatus {
    NotInstalled,
    Installed { version: String, binary: PathBuf },
    Corrupt { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CeError {
    #[error("could not resolve user data dir (XDG_DATA_HOME / HOME unset?)")]
    NoDataDir,
    #[error("download failed: {0}")]
    Download(Box<ureq::Error>),
    #[error("downloaded zip size {got} != expected {expected}")]
    SizeMismatch { got: u64, expected: u64 },
    #[error("downloaded zip SHA256 mismatch (expected {expected}, got {got})")]
    ChecksumMismatch { expected: String, got: String },
    #[error("zip extraction failed: {0}")]
    Extract(#[from] zip::result::ZipError),
    #[error("binary not found after extraction: {0}")]
    BinaryMissing(PathBuf),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ureq::Error> for CeError {
    fn from(e: ureq::Error) -> Self {
        Self::Download(Box::new(e))
    }
}

pub fn install_dir() -> Result<PathBuf, CeError> {
    let base = dirs::data_local_dir().ok_or(CeError::NoDataDir)?;
    Ok(base.join("backlog-tracker").join("cheatengine-linux"))
}

pub fn binary_path() -> Result<PathBuf, CeError> {
    Ok(install_dir()?.join(EXTRACT_DIRNAME).join(BINARY_NAME))
}

pub fn status() -> CeStatus {
    let Ok(binary) = binary_path() else {
        return CeStatus::NotInstalled;
    };
    if !binary.is_file() {
        return CeStatus::NotInstalled;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(&binary) {
            Ok(md) if md.permissions().mode() & 0o111 == 0 => {
                return CeStatus::Corrupt {
                    reason: format!("binary not executable: {}", binary.display()),
                };
            }
            Err(e) => {
                return CeStatus::Corrupt {
                    reason: format!("metadata error: {e}"),
                };
            }
            _ => {}
        }
    }
    CeStatus::Installed {
        version: CE_VERSION.to_string(),
        binary,
    }
}

pub fn ensure_installed() -> Result<CeInstall, CeError> {
    if let CeStatus::Installed { version, binary } = status() {
        return Ok(CeInstall { binary, version });
    }

    let root = install_dir()?;
    std::fs::create_dir_all(&root)?;
    let zip_bytes = download(CE_ZIP_URL)?;
    verify_size(&zip_bytes)?;
    verify_sha256(&zip_bytes, CE_ZIP_SHA256)?;
    extract(&zip_bytes, &root)?;

    let binary = binary_path()?;
    if !binary.is_file() {
        return Err(CeError::BinaryMissing(binary));
    }
    mark_executable(&binary)?;

    Ok(CeInstall {
        binary,
        version: CE_VERSION.to_string(),
    })
}

fn download(url: &str) -> Result<Vec<u8>, CeError> {
    let bytes = ureq::get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(DOWNLOAD_LIMIT as u64)
        .read_to_vec()?;
    Ok(bytes)
}

fn verify_size(bytes: &[u8]) -> Result<(), CeError> {
    let got = bytes.len() as u64;
    if got != CE_ZIP_SIZE {
        return Err(CeError::SizeMismatch {
            got,
            expected: CE_ZIP_SIZE,
        });
    }
    Ok(())
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), CeError> {
    let got = sha256_hex(bytes);
    if got != expected {
        return Err(CeError::ChecksumMismatch {
            expected: expected.to_string(),
            got,
        });
    }
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

fn extract(bytes: &[u8], dest: &Path) -> Result<(), CeError> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)?;
    archive.extract(dest)?;
    Ok(())
}

#[cfg(unix)]
fn mark_executable(path: &Path) -> Result<(), CeError> {
    use std::os::unix::fs::PermissionsExt;
    let md = std::fs::metadata(path)?;
    let mut perms = md.permissions();
    perms.set_mode(perms.mode() | 0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn mark_executable(_path: &Path) -> Result<(), CeError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_of_empty_string_is_known() {
        let hex = sha256_hex(b"");
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_of_abc_is_known() {
        let hex = sha256_hex(b"abc");
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn install_dir_lives_under_backlog_tracker() {
        let dir = install_dir().expect("data dir should resolve");
        assert!(
            dir.ends_with("backlog-tracker/cheatengine-linux"),
            "unexpected install dir: {}",
            dir.display()
        );
    }

    #[test]
    fn binary_path_points_to_extracted_layout() {
        let bin = binary_path().expect("data dir should resolve");
        assert!(bin.ends_with("CheatEngineLinux766-6/cheatengine-x86_64"));
    }

    #[test]
    fn verify_size_rejects_wrong_size() {
        let result = verify_size(b"too small");
        assert!(matches!(result, Err(CeError::SizeMismatch { .. })));
    }

    #[test]
    fn verify_sha256_rejects_mismatch() {
        let result = verify_sha256(b"abc", "deadbeef");
        assert!(matches!(result, Err(CeError::ChecksumMismatch { .. })));
    }

    #[test]
    fn verify_sha256_accepts_match() {
        let abc_sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        verify_sha256(b"abc", abc_sha).expect("matching sha should pass");
    }

    #[test]
    fn status_when_binary_missing_returns_not_installed() {
        let dir = install_dir().expect("data dir should resolve");
        let bin = binary_path().expect("binary path should resolve");
        if !bin.is_file() {
            assert!(matches!(status(), CeStatus::NotInstalled));
        } else {
            let s = status();
            assert!(
                matches!(s, CeStatus::Installed { .. } | CeStatus::Corrupt { .. }),
                "unexpected status {s:?} (install dir: {})",
                dir.display()
            );
        }
    }

    /// Integration smoke: downloads CE Linux from upstream, verifies sha256, extracts,
    /// and marks the binary executable. Network + filesystem write to user data dir.
    /// Run with: cargo test -p ce-launcher --ignored ensure_installed_smoke
    #[test]
    #[ignore]
    fn ensure_installed_smoke() {
        let install = ensure_installed().expect("install should succeed");
        assert_eq!(install.version, CE_VERSION);
        assert!(
            install.binary.is_file(),
            "binary not at {}",
            install.binary.display()
        );
        let again = ensure_installed().expect("second call should be idempotent");
        assert_eq!(again.binary, install.binary);
    }
}
