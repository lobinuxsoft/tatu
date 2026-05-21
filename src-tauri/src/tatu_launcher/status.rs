//! Installed-version detection for the Tatu Launcher drop-in.

use std::fs;

use crate::tatu_launcher::TatuLauncherStatus;
use crate::tatu_launcher::paths::{REQUIRED_FILES, install_dir};

/// File the install routine drops next to the binaries so the
/// frontend can compare against the version shipped in the running
/// tracker (env var `CARGO_PKG_VERSION` at compile time).
pub const VERSION_FILENAME: &str = "version.txt";

/// Read the on-disk Tatu Launcher install state. Cheap (a handful of
/// stat + one read) so the frontend can poll it on every cheats panel
/// refresh without burning cycles.
pub fn status() -> TatuLauncherStatus {
    let Ok(dir) = install_dir() else {
        return TatuLauncherStatus::NotInstalled;
    };
    if !dir.is_dir() {
        return TatuLauncherStatus::NotInstalled;
    }
    for file in REQUIRED_FILES {
        if !dir.join(file).is_file() {
            return TatuLauncherStatus::Corrupt {
                reason: format!("missing {file}"),
            };
        }
    }
    let version = fs::read_to_string(dir.join(VERSION_FILENAME))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    TatuLauncherStatus::Installed {
        version,
        install_dir: dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_filename_matches_release_convention() {
        // `version.txt` is what release-please-tracked artefacts ship.
        // Hard-coded so the install routine and status agree.
        assert_eq!(VERSION_FILENAME, "version.txt");
    }
}
