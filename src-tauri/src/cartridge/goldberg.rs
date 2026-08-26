use std::fs;
use std::path::{Path, PathBuf};

use goblin::pe::PE;

use crate::drm::Preservability;
use crate::steam::pick_main_exe_in;

use super::install::{acf_field, appmanifest_path};
use super::marker::set_standalone;

/// Swaps a Steam-wrapper-only (`Preservability::Easy`) game's
/// `steam_api(64).dll` for the vendored Goldberg emulator so it launches
/// through Proton without the Steam client. Run once #195's
/// `poll_install_status` reports the install complete.
///
/// `Hard`/`Unknown` games are refused outright — there is no clean path for
/// them and this never touches their files.
pub fn inject_goldberg(
    mount_point: &Path,
    app_id: u64,
    preservability: Preservability,
    template_x86: &Path,
    template_x64: &Path,
) -> Result<(), String> {
    if preservability != Preservability::Easy {
        return Err(format!(
            "App {app_id} is not classified Easy — Goldberg injection only applies \
             to Steam-wrapper-only games"
        ));
    }

    let install_dir = install_dir(mount_point, app_id)?;
    let mut files = Vec::new();
    walk(&install_dir, &mut files);

    if files
        .iter()
        .filter(|p| has_extension(p, "exe"))
        .any(|exe| exe_has_steam_stub(exe))
    {
        return Err(format!(
            "App {app_id} is wrapped in a SteamStub packer — Goldberg injection alone \
             cannot make it standalone"
        ));
    }

    let dll64 = named(&files, "steam_api64.dll");
    let dll32 = named(&files, "steam_api.dll");
    if dll64.is_empty() && dll32.is_empty() {
        return Err(format!(
            "No steam_api(64).dll found under {}",
            install_dir.display()
        ));
    }
    for original in dll64 {
        swap_dll(original, template_x64)?;
    }
    for original in dll32 {
        swap_dll(original, template_x86)?;
    }

    fs::write(install_dir.join("steam_appid.txt"), app_id.to_string())
        .map_err(|e| format!("Cannot write steam_appid.txt: {e}"))?;

    // The launcher (#204/#206/#207) has no Steam client to ask which .exe to
    // run — resolve it once here, the same heuristic already used for the
    // cheat-runtime feature, and record it on the marker.
    let exe_name = pick_main_exe_in(&install_dir)?;
    let exe_path = install_dir
        .join(&exe_name)
        .strip_prefix(mount_point)
        .map_err(|_| "Resolved exe path escaped the cartridge root".to_string())?
        .to_string_lossy()
        .replace('\\', "/");

    set_standalone(mount_point, app_id, exe_path)
}

/// `steamapps/common/<installdir>`, where `installdir` comes from the same
/// manifest #195 already polls for `StateFlags`.
fn install_dir(mount_point: &Path, app_id: u64) -> Result<PathBuf, String> {
    let manifest = appmanifest_path(mount_point, app_id);
    let content = fs::read_to_string(&manifest)
        .map_err(|e| format!("Cannot read {}: {e}", manifest.display()))?;
    let installdir = acf_field(&content, "installdir")
        .ok_or_else(|| format!("{} has no installdir field", manifest.display()))?;
    Ok(mount_point
        .join("steamapps")
        .join("common")
        .join(installdir))
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

fn named<'a>(files: &'a [PathBuf], filename: &str) -> Vec<&'a PathBuf> {
    files
        .iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) == Some(filename))
        .collect()
}

/// Renames the original DLL to `<stem>_o.<ext>` (never deleted — that's
/// what lets the install still launch through Steam afterward) and drops
/// the vendored Goldberg template in its place.
fn swap_dll(original: &Path, template: &Path) -> Result<(), String> {
    let stem = original.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = original.extension().and_then(|s| s.to_str()).unwrap_or("");
    let backup = original.with_file_name(format!("{stem}_o.{ext}"));

    fs::rename(original, &backup)
        .map_err(|e| format!("Cannot rename {}: {e}", original.display()))?;
    fs::copy(template, original).map_err(|e| {
        format!(
            "Cannot copy Goldberg template to {}: {e}",
            original.display()
        )
    })?;
    Ok(())
}

/// Whether `exe` carries a SteamStub v1/v2 wrapper — detected by its
/// telltale `.bind` section. SteamStub v3.x rewrites the entry point
/// instead of adding a named section and is NOT caught by this: an
/// undetected v3.x stub just fails to launch standalone, same as if this
/// check didn't exist — a known gap, not a silent regression, given
/// injection still failed loud when the pattern IS one we recognize.
fn exe_has_steam_stub(exe: &Path) -> bool {
    let Ok(bytes) = fs::read(exe) else {
        return false;
    };
    let Ok(pe) = PE::parse(&bytes) else {
        return false;
    };
    pe.sections.iter().any(|s| is_bind_section(&s.name))
}

fn is_bind_section(raw: &[u8; 8]) -> bool {
    let name = String::from_utf8_lossy(raw);
    name.trim_end_matches('\0') == ".bind"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::marker::{CartridgeApp, add_app, read_marker, write_marker};

    fn section_name(bytes: &[u8]) -> [u8; 8] {
        let mut name = [0u8; 8];
        name[..bytes.len()].copy_from_slice(bytes);
        name
    }

    #[test]
    fn bind_section_is_recognized() {
        assert!(is_bind_section(&section_name(b".bind")));
    }

    #[test]
    fn unrelated_section_is_not_flagged() {
        assert!(!is_bind_section(&section_name(b".text")));
    }

    #[test]
    fn non_easy_preservability_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = inject_goldberg(
            dir.path(),
            1,
            Preservability::Hard,
            Path::new("x86.dll"),
            Path::new("x64.dll"),
        )
        .unwrap_err();
        assert!(err.contains("not classified Easy"));
    }

    fn setup_cartridge_with_app(dir: &Path, app_id: u64, installdir: &str) -> (PathBuf, PathBuf) {
        write_marker(dir).unwrap();
        add_app(
            dir,
            CartridgeApp {
                app_id,
                name: "Test Game".to_string(),
                preservability: Preservability::Easy,
                standalone: false,
                exe_path: String::new(),
            },
        )
        .unwrap();

        let steamapps = dir.join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join(format!("appmanifest_{app_id}.acf")),
            format!(r#""AppState" {{ "appid" "{app_id}" "installdir" "{installdir}" "StateFlags" "4" }}"#),
        )
        .unwrap();

        let install_dir = steamapps.join("common").join(installdir);
        fs::create_dir_all(&install_dir).unwrap();

        let template_x64 = dir.join("template64.dll");
        fs::write(&template_x64, b"goldberg x64 template").unwrap();
        let template_x86 = dir.join("template86.dll");
        fs::write(&template_x86, b"goldberg x86 template").unwrap();

        (template_x86, template_x64)
    }

    #[test]
    fn missing_dll_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (x86, x64) = setup_cartridge_with_app(dir.path(), 1, "Empty Game");

        let err = inject_goldberg(dir.path(), 1, Preservability::Easy, &x86, &x64).unwrap_err();
        assert!(err.contains("No steam_api"));
    }

    #[test]
    fn injection_swaps_the_dll_and_marks_standalone() {
        let dir = tempfile::tempdir().unwrap();
        let (x86, x64) = setup_cartridge_with_app(dir.path(), 379720, "DOOM");
        let install_dir = dir.path().join("steamapps").join("common").join("DOOM");
        let original = install_dir.join("steam_api64.dll");
        fs::write(&original, b"real steam api").unwrap();
        fs::write(install_dir.join("DOOM.exe"), vec![0u8; 1024]).unwrap();

        inject_goldberg(dir.path(), 379720, Preservability::Easy, &x86, &x64).unwrap();

        assert_eq!(
            fs::read(install_dir.join("steam_api64_o.dll")).unwrap(),
            b"real steam api"
        );
        assert_eq!(fs::read(&original).unwrap(), fs::read(&x64).unwrap(),);
        assert_eq!(
            fs::read_to_string(install_dir.join("steam_appid.txt")).unwrap(),
            "379720"
        );

        let marker = read_marker(dir.path()).unwrap();
        assert!(marker.apps[0].standalone);
        assert_eq!(marker.apps[0].exe_path, "steamapps/common/DOOM/DOOM.exe");
    }

    #[test]
    fn injection_finds_the_dll_in_a_nested_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (x86, x64) = setup_cartridge_with_app(dir.path(), 1, "Nested Game");
        let install_dir = dir
            .path()
            .join("steamapps")
            .join("common")
            .join("Nested Game");
        let nested = install_dir.join("Binaries").join("Win64");
        fs::create_dir_all(&nested).unwrap();
        let original = nested.join("steam_api64.dll");
        fs::write(&original, b"real steam api").unwrap();
        fs::write(
            nested.join("NestedGame-Win64-Shipping.exe"),
            vec![0u8; 1024],
        )
        .unwrap();

        inject_goldberg(dir.path(), 1, Preservability::Easy, &x86, &x64).unwrap();

        assert!(nested.join("steam_api64_o.dll").exists());
        assert_eq!(fs::read(&original).unwrap(), fs::read(&x64).unwrap());
    }
}
