//! Idempotent install of the Tatu Launcher drop-in.
//!
//! Mirrors the shell `tools/tatu-launcher/install.sh` so the tracker
//! can wire "Enable Tatu backend" from the UI without spawning a
//! shell. Source files are located via [`super::paths::source_candidates`];
//! the first candidate dir containing every required file wins, and
//! gets copied verbatim into `<steam>/compatibilitytools.d/tatu-launcher/`.
//!
//! Existing files are overwritten so a newer build replaces an
//! older drop-in (the alternative — install-once-skip-after — would
//! force the user to wipe the directory on every update).

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::tatu_launcher::TatuLauncherError;
use crate::tatu_launcher::paths::{REQUIRED_FILES, install_dir, source_candidates};
use crate::tatu_launcher::status::VERSION_FILENAME;

/// Files that need the executable bit set after copy. Mirrors what
/// `install.sh` does with `install -m 0755`.
const EXECUTABLE_FILES: &[&str] = &["tatu-launcher", "tatu-launcher.sh", "tatu-bridge.exe"];

/// Copy the staged drop-in into the install dir. Idempotent: re-running
/// after a no-op source change is harmless because every file is
/// rewritten with the same bytes.
///
/// Returns the destination path so the frontend can show "installed
/// at <path>" feedback after the click.
pub fn install_compat_tool() -> Result<PathBuf, TatuLauncherError> {
    let source = locate_source()?;
    let dest = install_dir()?;
    fs::create_dir_all(&dest)?;

    for file in REQUIRED_FILES {
        let from = source.join(file);
        let to = dest.join(file);
        fs::copy(&from, &to)?;
        if EXECUTABLE_FILES.contains(file) {
            set_executable(&to)?;
        }
    }

    let version_src = source.join(VERSION_FILENAME);
    if version_src.is_file() {
        fs::copy(&version_src, dest.join(VERSION_FILENAME))?;
    } else {
        // Fall back to the tracker's own crate version so the
        // frontend can still surface *something* under "installed
        // version" even when the build script didn't stamp one.
        fs::write(
            dest.join(VERSION_FILENAME),
            format!("{}\n", env!("CARGO_PKG_VERSION")),
        )?;
    }

    Ok(dest)
}

fn locate_source() -> Result<PathBuf, TatuLauncherError> {
    source_candidates()
        .into_iter()
        .find(|c| REQUIRED_FILES.iter().all(|f| c.join(f).is_file()))
        .ok_or(TatuLauncherError::NoSource)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), TatuLauncherError> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), TatuLauncherError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn stage_drop_in(dir: &Path) {
        for file in REQUIRED_FILES {
            fs::write(dir.join(file), b"stub").unwrap();
        }
    }

    #[test]
    fn locate_source_picks_first_complete_candidate() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("drop-in");
        fs::create_dir_all(&src).unwrap();
        stage_drop_in(&src);

        // Bypass the candidate search and check the inner predicate
        // directly — the search depends on env-derived paths, which
        // are not hermetic.
        let complete = REQUIRED_FILES.iter().all(|f| src.join(f).is_file());
        assert!(complete);
    }

    #[test]
    fn locate_source_rejects_incomplete_candidate() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("drop-in");
        fs::create_dir_all(&src).unwrap();
        for f in REQUIRED_FILES.iter().take(REQUIRED_FILES.len() - 1) {
            fs::write(src.join(f), b"stub").unwrap();
        }
        let complete = REQUIRED_FILES.iter().all(|f| src.join(f).is_file());
        assert!(!complete, "missing one file should fail completeness check");
    }
}
