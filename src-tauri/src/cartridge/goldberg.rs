use std::fs;
use std::path::{Path, PathBuf};

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
    steam_id: &str,
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

    // Resolved early (before the DLL swap below), via appinfo.vdf when
    // available — the exact file Steam's own "Play" button launches.
    // Whether THIS exe (or anything else in the tree) carries a SteamStub
    // wrapper turned out not to matter at all: live-verified 2026-09-05
    // against KINGDOM HEARTS -HD 1.5+2.5 ReMIX- (2552430), whose
    // `appinfo.vdf` entry point is itself SteamStub-wrapped with no
    // separate unwrapped launcher to go through instead — it ran fine
    // standalone as the literal top-level process, no unpacking, no child
    // process, nothing else needed beyond the DLL swap below. Steamless
    // (#271/#273) and the SteamStub refusal this replaced were both built
    // on the same wrong assumption (a wrapped entry point needs
    // unwrapping, or at least a clean launcher to hide behind) — neither
    // is true. Goldberg's DLL sitting next to the exe is the only thing
    // that ever mattered.
    let exe_name = pick_main_exe_in(&install_dir, app_id)?;
    let exe_full_path = install_dir.join(&exe_name);

    let dll64 = named(&files, "steam_api64.dll");
    let dll32 = named(&files, "steam_api.dll");
    if dll64.is_empty() && dll32.is_empty() {
        return Err(format!(
            "No steam_api(64).dll found under {}",
            install_dir.display()
        ));
    }
    for original in &dll64 {
        swap_dll(original, template_x64)?;
    }
    for original in &dll32 {
        swap_dll(original, template_x86)?;
    }

    // gbe_fork checks `steam_settings/steam_appid.txt` NEXT TO THE DLL first,
    // falling back to the exe's own run path only if that's absent — without
    // this folder, some builds fail SteamAPI_Init silently (no crash, no
    // window, no error) instead of falling back cleanly. Root-level
    // steam_appid.txt below covers the run-path fallback for whichever
    // process actually ends up as CWD.
    for original in dll64.iter().chain(dll32.iter()) {
        let Some(dll_dir) = original.parent() else {
            continue;
        };
        let settings_dir = dll_dir.join("steam_settings");
        fs::create_dir_all(&settings_dir)
            .map_err(|e| format!("Cannot create {}: {e}", settings_dir.display()))?;
        fs::write(settings_dir.join("steam_appid.txt"), app_id.to_string()).map_err(|e| {
            format!(
                "Cannot write {}: {e}",
                settings_dir.join("steam_appid.txt").display()
            )
        })?;

        // Without Steam running, the game can't read the client's own
        // language setting — gbe_fork answers ISteamApps::
        // GetCurrentGameLanguage() from this file instead, so a standalone
        // launch defaults to whatever the real Steam client would show a
        // Spanish-speaking account (live-caught: FF7 Remake ran in English
        // standalone despite the source Steam library being set to
        // Spanish). "spanish" is Valve's own API string for it, same one
        // every Steamworks game already checks against.
        // A fixed, real account_steamid (rather than Goldberg's default
        // random-per-launch one) matters beyond identity: many games bake
        // the SteamID64 straight into their local save path (e.g. FF7
        // Remake's "Documents/My Games/.../Steam/<steamid>/"). Without this,
        // every standalone launch writes to a fresh, different folder that
        // never lines up with the save Steam Cloud already knows about.
        let mut config = "[user::general]\nlanguage=spanish\n".to_string();
        if !steam_id.is_empty() {
            config.push_str(&format!("account_steamid={steam_id}\n"));
        }
        fs::write(settings_dir.join("configs.user.ini"), config).map_err(|e| {
            format!(
                "Cannot write {}: {e}",
                settings_dir.join("configs.user.ini").display()
            )
        })?;
    }

    fs::write(install_dir.join("steam_appid.txt"), app_id.to_string())
        .map_err(|e| format!("Cannot write steam_appid.txt: {e}"))?;

    // Valve's own convention has SteamAPI_Init fall back to a
    // `steam_appid.txt` in the PROCESS'S OWN CURRENT DIRECTORY when no real
    // Steam client is present — distinct from gbe_fork's own DLL-relative
    // `steam_settings/` lookup already covered above. The launcher always
    // sets CWD to the exe's own folder (see main.gd's own comment on
    // this), which for an engine that buries its exe several folders deep
    // (UE's `<Game>/Binaries/Win64/`) is neither of the two locations
    // already written.
    if let Some(exe_dir) = exe_full_path.parent() {
        fs::write(exe_dir.join("steam_appid.txt"), app_id.to_string()).map_err(|e| {
            format!(
                "Cannot write {}: {e}",
                exe_dir.join("steam_appid.txt").display()
            )
        })?;
    }

    let exe_path = exe_full_path
        .strip_prefix(mount_point)
        .map_err(|_| "Resolved exe path escaped the cartridge root".to_string())?
        .to_string_lossy()
        .replace('\\', "/");

    set_standalone(mount_point, app_id, exe_path)
}

/// Whether Goldberg's own copy is still the active DLL. Steam's own
/// "verify integrity of game files" (or a game update re-downloading the
/// depot's original DLL) can silently put the real DLL back in place —
/// the marker's `standalone` flag has no way to notice that by itself, so
/// `refresh_drm_and_inject` uses this to re-inject even when the marker
/// still says `standalone: true` (live-caught on FF7 Remake, 2026-09-04:
/// a verify-integrity run reverted the DLL and the button did nothing
/// until the marker was hand-edited).
pub(super) fn is_still_injected(
    mount_point: &Path,
    app_id: u64,
    template_x86: &Path,
    template_x64: &Path,
) -> bool {
    let Ok(install_dir) = install_dir(mount_point, app_id) else {
        return true;
    };
    let mut files = Vec::new();
    walk(&install_dir, &mut files);

    let matches_template = |actual: &[&PathBuf], template: &Path| -> bool {
        if actual.is_empty() {
            return true;
        }
        let Ok(template_bytes) = fs::read(template) else {
            return true;
        };
        actual
            .iter()
            .all(|p| fs::read(p).map(|b| b == template_bytes).unwrap_or(true))
    };

    matches_template(&named(&files, "steam_api64.dll"), template_x64)
        && matches_template(&named(&files, "steam_api.dll"), template_x86)
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

fn named<'a>(files: &'a [PathBuf], filename: &str) -> Vec<&'a PathBuf> {
    files
        .iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) == Some(filename))
        .collect()
}

/// Renames the original DLL to `<stem>_o.<ext>` (never deleted — that's
/// what lets the install still launch through Steam afterward) and drops
/// the vendored Goldberg template in its place.
///
/// Skips the rename when `_o` already exists: re-running injection (a
/// newer Goldberg template, or Steam's own "verify integrity" having put
/// the real DLL back as active) used to blindly rename whatever was
/// currently active over the backup — the second time this ran, that
/// "original" was already Goldberg's own copy from the first run,
/// permanently destroying the one genuine backup with a fake one
/// (live-caught on FF7 Remake, 2026-09-03/04). The backup, once made, is
/// the real DLL for good; only the active copy ever gets replaced again.
fn swap_dll(original: &Path, template: &Path) -> Result<(), String> {
    let stem = original.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = original.extension().and_then(|s| s.to_str()).unwrap_or("");
    let backup = original.with_file_name(format!("{stem}_o.{ext}"));

    if !backup.exists() {
        fs::rename(original, &backup)
            .map_err(|e| format!("Cannot rename {}: {e}", original.display()))?;
    }
    fs::copy(template, original).map_err(|e| {
        format!(
            "Cannot copy Goldberg template to {}: {e}",
            original.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::marker::{AppSource, CartridgeApp, add_app, read_marker, write_marker};

    #[test]
    fn non_easy_preservability_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = inject_goldberg(
            dir.path(),
            1,
            Preservability::Hard,
            Path::new("x86.dll"),
            Path::new("x64.dll"),
            "",
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
                source: AppSource::Steam,
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
        // pick_main_exe_in now runs before the DLL check (#273 fix: it has
        // to know the real entry point before deciding what needs a
        // SteamStub unwrap) — needs an exe to resolve first, or this would
        // fail on "no .exe files" instead of exercising the DLL check this
        // test is actually about.
        let install_dir = dir
            .path()
            .join("steamapps")
            .join("common")
            .join("Empty Game");
        fs::write(install_dir.join("Game.exe"), vec![0u8; 1024]).unwrap();

        let err = inject_goldberg(dir.path(), 1, Preservability::Easy, &x86, &x64, "").unwrap_err();
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

        inject_goldberg(dir.path(), 379720, Preservability::Easy, &x86, &x64, "").unwrap();

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
    fn reinjection_keeps_the_real_backup() {
        let dir = tempfile::tempdir().unwrap();
        let (x86, x64) = setup_cartridge_with_app(dir.path(), 379720, "DOOM");
        let install_dir = dir.path().join("steamapps").join("common").join("DOOM");
        fs::write(install_dir.join("steam_api64.dll"), b"real steam api").unwrap();
        fs::write(install_dir.join("DOOM.exe"), vec![0u8; 1024]).unwrap();

        inject_goldberg(dir.path(), 379720, Preservability::Easy, &x86, &x64, "").unwrap();
        // A second run (a newer Goldberg template, or Steam's own "verify
        // integrity" never having restored the real DLL) must not treat
        // the now-active Goldberg copy as "the original" and rename it
        // over the real backup.
        fs::write(&x64, b"newer goldberg x64 template").unwrap();
        inject_goldberg(dir.path(), 379720, Preservability::Easy, &x86, &x64, "").unwrap();

        assert_eq!(
            fs::read(install_dir.join("steam_api64_o.dll")).unwrap(),
            b"real steam api"
        );
        assert_eq!(
            fs::read(install_dir.join("steam_api64.dll")).unwrap(),
            b"newer goldberg x64 template"
        );
    }

    #[test]
    fn drift_is_detected_when_the_real_dll_is_restored() {
        let dir = tempfile::tempdir().unwrap();
        let (x86, x64) = setup_cartridge_with_app(dir.path(), 379720, "DOOM");
        let install_dir = dir.path().join("steamapps").join("common").join("DOOM");
        fs::write(install_dir.join("steam_api64.dll"), b"real steam api").unwrap();
        fs::write(install_dir.join("DOOM.exe"), vec![0u8; 1024]).unwrap();
        inject_goldberg(dir.path(), 379720, Preservability::Easy, &x86, &x64, "").unwrap();

        assert!(is_still_injected(dir.path(), 379720, &x86, &x64));

        // Steam's own "verify integrity" restoring the real DLL as active,
        // completely outside Tatu's own control.
        fs::write(install_dir.join("steam_api64.dll"), b"real steam api").unwrap();
        assert!(!is_still_injected(dir.path(), 379720, &x86, &x64));
    }

    #[test]
    fn injection_writes_steam_settings_next_to_the_dll() {
        let dir = tempfile::tempdir().unwrap();
        let (x86, x64) = setup_cartridge_with_app(dir.path(), 379720, "DOOM");
        let install_dir = dir.path().join("steamapps").join("common").join("DOOM");
        fs::write(install_dir.join("steam_api64.dll"), b"real steam api").unwrap();
        fs::write(install_dir.join("DOOM.exe"), vec![0u8; 1024]).unwrap();

        inject_goldberg(dir.path(), 379720, Preservability::Easy, &x86, &x64, "").unwrap();

        // gbe_fork reads this location before its exe-run-path fallback —
        // without it, SteamAPI_Init can fail silently (#228 live incident:
        // no window, no crash, no error).
        assert_eq!(
            fs::read_to_string(install_dir.join("steam_settings").join("steam_appid.txt")).unwrap(),
            "379720"
        );
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

        inject_goldberg(dir.path(), 1, Preservability::Easy, &x86, &x64, "").unwrap();

        assert!(nested.join("steam_api64_o.dll").exists());
        assert_eq!(fs::read(&original).unwrap(), fs::read(&x64).unwrap());
    }
}
