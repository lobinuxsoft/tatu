//! Filesystem-only prereq detection (#98).
//!
//! Each [`Prereq`](crate::manifest::Prereq) variant gets a `detect_<x>`
//! function that takes a game install directory and reports whether the
//! dependency is in place. Network / download / install logic lives in
//! `tatu-tracker::prereqs` — keeping it out of this crate avoids dragging
//! `ureq` / `zip` deps into anything that just wants to read manifests.
//!
//! ### REFramework
//!
//! praydog's `dinput8.dll` proxy ships inside `REFramework.zip`. Once
//! extracted into the game dir, the file:
//!
//! - sits at `<game_dir>/dinput8.dll`,
//! - is ~6-12 MB depending on nightly,
//! - is unsigned (no Authenticode catalogue),
//! - contains the PE 32+ banner expected by a 64-bit Windows process.
//!
//! Steam's runtime never ships a `dinput8.dll` inside `<game_dir>/`, so
//! the simple file-existence check is enough to disambiguate "user
//! installed REFramework" from "untouched Steam install". We additionally
//! confirm the file starts with the `MZ` DOS signature to rule out stray
//! 0-byte placeholders.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Result of a single prereq check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrereqStatus {
    /// The dependency is in place and looks healthy.
    Installed,
    /// The dependency is not present in the game dir.
    Missing,
    /// A file with the expected name exists but didn't pass the sniff
    /// test (truncated, wrong format, …). Treated as missing by the UI
    /// but reported separately so the user knows a manual cleanup may be
    /// needed before re-installing.
    Corrupt,
}

/// Detect whether praydog's REFramework `dinput8.dll` proxy is installed
/// in `game_dir`. The check is intentionally minimal — anything more
/// strict (Authenticode chain, PE imports, REFramework's own banner)
/// would either fail on legitimate installs or require parsing the PE
/// header byte-for-byte, neither of which buys us much: REFramework is
/// the only mod that ever ships a `dinput8.dll` next to a Capcom RE
/// Engine exe, so file presence + `MZ` magic + non-trivial size is the
/// canonical signal.
pub fn detect_reframework(game_dir: &Path) -> PrereqStatus {
    let dll = game_dir.join("dinput8.dll");
    if !dll.is_file() {
        return PrereqStatus::Missing;
    }
    let Ok(meta) = std::fs::metadata(&dll) else {
        return PrereqStatus::Corrupt;
    };
    // REFramework nightlies are 6-12 MB. Steam itself never ships
    // dinput8.dll next to a game exe, so a tiny stub would be a
    // half-finished install.
    if meta.len() < 256 * 1024 {
        return PrereqStatus::Corrupt;
    }
    let Ok(mut f) = File::open(&dll) else {
        return PrereqStatus::Corrupt;
    };
    let mut magic = [0u8; 2];
    if f.read_exact(&mut magic).is_err() || magic != *b"MZ" {
        return PrereqStatus::Corrupt;
    }
    PrereqStatus::Installed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, bytes: &[u8]) {
        fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn missing_when_no_dll() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(detect_reframework(tmp.path()), PrereqStatus::Missing);
    }

    #[test]
    fn corrupt_when_tiny_stub() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "dinput8.dll", b"MZ");
        assert_eq!(detect_reframework(tmp.path()), PrereqStatus::Corrupt);
    }

    #[test]
    fn corrupt_when_wrong_magic() {
        let tmp = TempDir::new().unwrap();
        // 1 MB of zeros — passes the size gate, fails MZ.
        write(tmp.path(), "dinput8.dll", &vec![0u8; 1024 * 1024]);
        assert_eq!(detect_reframework(tmp.path()), PrereqStatus::Corrupt);
    }

    #[test]
    fn installed_when_mz_and_large() {
        let tmp = TempDir::new().unwrap();
        let mut bytes = vec![0u8; 1024 * 1024];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        write(tmp.path(), "dinput8.dll", &bytes);
        assert_eq!(detect_reframework(tmp.path()), PrereqStatus::Installed);
    }
}
