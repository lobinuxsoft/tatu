//! Cheap stat-based detection of an installed REFramework drop-in.

use std::path::Path;

use crate::reframework::ReframeworkStatus;

/// Minimum `dinput8.dll` size that plausibly belongs to REFramework.
/// A stub Microsoft `dinput8.dll` (the file REFramework proxies) is
/// roughly 200 KB; a fully-built REFramework `dinput8.dll` is several
/// megabytes. 1 MB is a comfortable middle ground that rejects the
/// stub without false-negativing nightlies that trimmed symbols.
const MIN_REFRAMEWORK_DLL_SIZE: u64 = 1_000_000;

pub fn status(game_dir: &Path) -> ReframeworkStatus {
    let dll = game_dir.join("dinput8.dll");
    let Ok(meta) = std::fs::metadata(&dll) else {
        return ReframeworkStatus::NotInstalled;
    };
    if !meta.is_file() {
        return ReframeworkStatus::NotInstalled;
    }
    if meta.len() < MIN_REFRAMEWORK_DLL_SIZE {
        return ReframeworkStatus::NotInstalled;
    }
    ReframeworkStatus::Installed {
        dll_size_bytes: meta.len(),
        has_reframework_dir: game_dir.join("reframework").is_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn status_missing_when_no_dll() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(status(tmp.path()), ReframeworkStatus::NotInstalled);
    }

    #[test]
    fn status_missing_when_dll_too_small() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("dinput8.dll"), vec![0u8; 1024]).unwrap();
        assert_eq!(status(tmp.path()), ReframeworkStatus::NotInstalled);
    }

    #[test]
    fn status_installed_when_dll_large_enough() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("dinput8.dll"),
            vec![0u8; (MIN_REFRAMEWORK_DLL_SIZE + 1) as usize],
        )
        .unwrap();
        let st = status(tmp.path());
        match st {
            ReframeworkStatus::Installed {
                dll_size_bytes,
                has_reframework_dir,
            } => {
                assert!(dll_size_bytes > MIN_REFRAMEWORK_DLL_SIZE);
                assert!(!has_reframework_dir);
            }
            other => panic!("expected Installed, got {other:?}"),
        }
    }

    #[test]
    fn status_flags_reframework_dir_when_present() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("dinput8.dll"),
            vec![0u8; (MIN_REFRAMEWORK_DLL_SIZE + 1) as usize],
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("reframework")).unwrap();
        if let ReframeworkStatus::Installed {
            has_reframework_dir,
            ..
        } = status(tmp.path())
        {
            assert!(has_reframework_dir);
        } else {
            panic!("expected Installed status");
        }
    }
}
