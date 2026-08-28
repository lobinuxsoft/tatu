use std::fs;
use std::path::{Path, PathBuf};

use goblin::pe::PE;

use super::hints::preservability_hint;
use super::types::{DrmInfo, DrmStatus, Preservability};
use crate::steam::exe::find_install_path;

/// When PCGamingWiki and Steam Store both come back inconclusive, inspect
/// the game's own installed files for concrete signals instead of leaving
/// it `Unknown` — only possible once the game is actually installed
/// locally, so a no-op (returns `info` unchanged) whenever it isn't.
///
/// Live-tested case (2026-08-28, Hellpoint/628670): PCGamingWiki had no
/// entry for it, but the install already had both `steam_api64.dll` (no
/// SteamStub section) and GOG Galaxy's `Galaxy64.dll` sitting right there —
/// enough to know for certain it's Goldberg-compatible and separately sold
/// DRM-free on GOG, without asking any network source at all.
pub(super) fn upgrade_from_installed_files(app_id: u64, info: DrmInfo) -> DrmInfo {
    if info.preservability != Preservability::Unknown {
        return info;
    }
    let Ok(install_dir) = find_install_path(&app_id.to_string()) else {
        return info;
    };

    let mut files = Vec::new();
    walk(&install_dir, &mut files);

    // Steam-wrapper-only checked first: it confirms the copy the user
    // already owns is fully fixable in place, no separate purchase needed
    // — prefer that over pointing at a GOG alternative when both signals
    // happen to be present (as with Hellpoint, which ships both SDKs).
    let has_steam_api = files.iter().any(|p| is_steam_api_dll(p));
    if has_steam_api {
        let stubbed = files
            .iter()
            .filter(|p| has_extension(p, "exe"))
            .any(|exe| exe_has_steam_stub(exe));
        if !stubbed {
            return upgraded_easy(info);
        }
    }

    if files.iter().any(|p| is_gog_galaxy_dll(p)) {
        return upgraded_alternative(info);
    }

    info
}

fn upgraded_easy(mut info: DrmInfo) -> DrmInfo {
    info.status = DrmStatus::SteamOnly;
    info.affects_steam_copy = true;
    info.explanation = "Se encontró steam_api(64).dll sin protección SteamStub en los archivos \
        ya instalados — solo wrapper de Steam, sin DRM de terceros detectado."
        .to_string();
    info.preservability = Preservability::Easy;
    info.preservability_hint = preservability_hint(&Preservability::Easy);
    info.source = "archivos locales".to_string();
    info.notes = push_note(
        info.notes,
        "steam_api(64).dll sin SteamStub en la instalación",
    );
    info
}

fn upgraded_alternative(mut info: DrmInfo) -> DrmInfo {
    info.preservability = Preservability::Alternative;
    info.preservability_hint = preservability_hint(&Preservability::Alternative);
    info.notes = push_note(info.notes, "SDK de GOG Galaxy detectado en la instalación");
    info
}

fn push_note(existing: String, note: &str) -> String {
    if existing.is_empty() {
        note.to_string()
    } else {
        format!("{existing} | {note}")
    }
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

fn is_steam_api_dll(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("steam_api.dll") | Some("steam_api64.dll")
    )
}

/// GOG Galaxy SDK filenames — the exact set found live on Hellpoint's own
/// install (`Galaxy64.dll` + `GalaxyCSharpGlue.dll`), plus the 32-bit and
/// namespaced variants GOG's own SDK docs ship under.
fn is_gog_galaxy_dll(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("Galaxy.dll")
            | Some("Galaxy64.dll")
            | Some("GalaxyCSharpGlue.dll")
            | Some("GalaxyCSharpGlue64.dll")
    )
}

/// Same `.bind`-section check `cartridge::goldberg` uses to refuse patching
/// a SteamStub-wrapped exe — duplicated rather than imported since `drm` is
/// the lower-level module here (`cartridge` already depends on `drm::
/// Preservability`, not the other way around).
fn exe_has_steam_stub(exe: &Path) -> bool {
    let Ok(bytes) = fs::read(exe) else {
        return false;
    };
    let Ok(pe) = PE::parse(&bytes) else {
        return false;
    };
    pe.sections.iter().any(|s| {
        let name = String::from_utf8_lossy(&s.name);
        name.trim_end_matches('\0') == ".bind"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_info(preservability: Preservability) -> DrmInfo {
        DrmInfo {
            status: DrmStatus::Unknown,
            notes: String::new(),
            source: "none".to_string(),
            fetched_at: 0,
            affects_steam_copy: false,
            explanation: String::new(),
            preservability,
            preservability_hint: String::new(),
            stores: Vec::new(),
            removed_drm: Vec::new(),
        }
    }

    #[test]
    fn already_classified_is_left_untouched() {
        let info = base_info(Preservability::Hard);
        let result = upgrade_from_installed_files(1, info.clone());
        assert_eq!(result.preservability, info.preservability);
    }

    #[test]
    fn unknown_with_no_install_stays_unknown() {
        // No Steam library on this machine has appid 999999999 installed —
        // find_install_path fails, so this is a no-op, not a panic.
        let result = upgrade_from_installed_files(999999999, base_info(Preservability::Unknown));
        assert_eq!(result.preservability, Preservability::Unknown);
    }

    #[test]
    fn recognizes_gog_galaxy_filenames() {
        assert!(is_gog_galaxy_dll(&PathBuf::from("Galaxy64.dll")));
        assert!(is_gog_galaxy_dll(&PathBuf::from("GalaxyCSharpGlue.dll")));
        assert!(!is_gog_galaxy_dll(&PathBuf::from("steam_api64.dll")));
    }

    #[test]
    fn recognizes_steam_api_filenames() {
        assert!(is_steam_api_dll(&PathBuf::from("steam_api64.dll")));
        assert!(is_steam_api_dll(&PathBuf::from("steam_api.dll")));
        assert!(!is_steam_api_dll(&PathBuf::from("Galaxy64.dll")));
    }
}
