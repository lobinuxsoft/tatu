//! Enumerate Proton installs the launcher can resolve.
//!
//! `tatu-launcher` accepts a Proton reference (the `default_proton`
//! and per-game `proton` fields of `~/.config/tatu/launcher.toml`)
//! as either an absolute path or a directory name under one of two
//! Steam-managed locations:
//!
//! - `<steam>/compatibilitytools.d/` — custom builds the user
//!   dropped in (GE-Proton, Proton-aurora, Tatu Launcher itself,
//!   etc.).
//! - `<steam>/steamapps/common/` — official Valve builds (Proton -
//!   Experimental, Proton 9.0, Proton 10.0).
//!
//! Walks both and returns the directory names so the frontend can
//! populate a per-game dropdown without the user typing.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::steam::steam_install_dir;

/// One Proton install discovered on disk. `name` is what goes into
/// `launcher.toml` — never the absolute path — so the launcher's
/// own resolver (which lives in `tatu-launcher::proton`) takes over.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProtonInfo {
    /// Directory name as it appears under `compatibilitytools.d/` or
    /// `steamapps/common/` — the string the launcher resolves.
    pub name: String,
    /// `"official"` for `steamapps/common/Proton*`, `"custom"` for
    /// `compatibilitytools.d/*`. The frontend uses this to group /
    /// label entries in the dropdown.
    pub kind: ProtonKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProtonKind {
    Official,
    Custom,
}

/// Directory name of the Tatu Launcher drop-in itself. Skipped from
/// the picker because selecting it would re-enter the launcher
/// recursively — no useful Proton lives there.
const TATU_LAUNCHER_DIRNAME: &str = "tatu-launcher";

/// Discover Proton installs in both Steam-managed locations.
/// Returns an empty vec when Steam is not installed (caller already
/// renders a no-Steam error elsewhere).
pub fn list_protons() -> Vec<ProtonInfo> {
    let Some(steam) = steam_install_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_protons(
        &steam.join("steamapps").join("common"),
        ProtonKind::Official,
        &mut out,
        |name| name.to_ascii_lowercase().starts_with("proton"),
    );
    collect_protons(
        &steam.join("compatibilitytools.d"),
        ProtonKind::Custom,
        &mut out,
        |name| name != TATU_LAUNCHER_DIRNAME,
    );
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    out
}

fn collect_protons(
    dir: &Path,
    kind: ProtonKind,
    out: &mut Vec<ProtonInfo>,
    name_filter: impl Fn(&str) -> bool,
) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name_filter(&name) {
            continue;
        }
        // The launcher's resolver looks for `<dir>/proton`; if that
        // script is missing the entry is non-functional and the
        // picker should hide it.
        if entry.path().join("proton").is_file() {
            out.push(ProtonInfo { name, kind });
        }
    }
}

// Enforce a stable order in the dropdown: Official before Custom,
// then alphabetical by name.
impl Ord for ProtonKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use ProtonKind::*;
        match (self, other) {
            (Official, Official) | (Custom, Custom) => std::cmp::Ordering::Equal,
            (Official, Custom) => std::cmp::Ordering::Less,
            (Custom, Official) => std::cmp::Ordering::Greater,
        }
    }
}

impl PartialOrd for ProtonKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn stub_proton(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        File::create(dir.join("proton")).unwrap();
    }

    #[test]
    fn collect_protons_filters_by_name_and_proton_script() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        stub_proton(&root.join("Proton - Experimental"));
        stub_proton(&root.join("Proton 9.0 (Beta)"));
        stub_proton(&root.join("SomeOtherGame")); // not Proton — filtered
        fs::create_dir_all(root.join("Proton 8.0")).unwrap(); // dir without proton script — filtered

        let mut out = Vec::new();
        collect_protons(root, ProtonKind::Official, &mut out, |name| {
            name.to_ascii_lowercase().starts_with("proton")
        });
        let names: Vec<&str> = out.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Proton - Experimental"));
        assert!(names.contains(&"Proton 9.0 (Beta)"));
        assert!(
            !names.contains(&"Proton 8.0"),
            "dirs without proton script should be filtered"
        );
        assert!(!names.contains(&"SomeOtherGame"));
    }

    #[test]
    fn collect_protons_skips_tatu_launcher_drop_in() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        stub_proton(&root.join("tatu-launcher"));
        stub_proton(&root.join("GE-Proton9-25"));

        let mut out = Vec::new();
        collect_protons(root, ProtonKind::Custom, &mut out, |name| {
            name != TATU_LAUNCHER_DIRNAME
        });
        let names: Vec<&str> = out.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"GE-Proton9-25"));
        assert!(
            !names.contains(&"tatu-launcher"),
            "selecting tatu-launcher would recurse"
        );
    }

    #[test]
    fn proton_kind_sort_official_first() {
        let mut kinds = vec![ProtonKind::Custom, ProtonKind::Official, ProtonKind::Custom];
        kinds.sort();
        assert_eq!(
            kinds,
            vec![ProtonKind::Official, ProtonKind::Custom, ProtonKind::Custom]
        );
    }
}
