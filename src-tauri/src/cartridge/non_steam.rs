// Raw copy of a non-Steam game onto a cartridge (#236, sub-issue of #192's
// parent epic #203). Unlike Steam/GOG installs, there's no manifest and no
// content-system to drive this — most non-Steam entries are DRM-free
// already, so a plain recursive copy of the install root (`shortcuts.vdf`'s
// "Start In", or the user's own correction — see `NonSteamGame::
// install_root`) is the whole install step.

use std::fs;
use std::path::{Path, PathBuf};

use crate::drm::Preservability;
use crate::shortcuts::NonSteamGame;

#[cfg(test)]
use super::marker::write_marker;
use super::marker::{AppSource, CartridgeApp, add_app};

/// Top-level folder for non-Steam games on a cartridge, parallel to
/// `steamapps/` (Steam) and `GOG/` (GOG).
const NON_STEAM_DIR: &str = "NON-STEAM";

/// Total bytes `add_non_steam_app` is about to copy — the progress bar's
/// denominator, computed up front so the UI has a real percentage from the
/// first event instead of an indeterminate spinner (live feedback: a copy
/// that finished in well under a second for a small game looked exactly
/// like a hang/no-op for a large one with nothing on screen to tell them
/// apart).
pub fn install_size(game: &NonSteamGame) -> u64 {
    super::usage::size_of(Path::new(game.install_root()))
}

/// Copies `game`'s entire install folder (Steam's own "Start In" for the
/// shortcut, the only install-root Tatu has for a non-Steam entry) into
/// `NON-STEAM/<install_dir>/` on the cartridge and records it on the
/// marker. `install_dir` is whatever the folder is actually named on disk —
/// same "trust the real folder name, don't invent a slug" precedent as
/// Steam's `installdir` and GOG's `repo.install_directory`.
///
/// `on_progress(bytes_just_copied)` is called after each file — the caller
/// accumulates and throttles its own emit, same split `gog_download::
/// download_depot` already uses (`on_progress` there is per-file too, not
/// per-byte-of-that-file, but the reasoning — keep Tauri/event concerns out
/// of the plain-Rust copy — is the same).
pub fn add_non_steam_app(
    mount_point: &Path,
    game: &NonSteamGame,
    mut on_progress: impl FnMut(u64),
) -> Result<CartridgeApp, String> {
    if game.install_root().is_empty() {
        return Err(format!(
            "\"{}\" no tiene una carpeta de inicio (Start In) registrada en Steam — \
             editá el acceso directo en Steam, o corregí la carpeta a mano, y volvé a intentar",
            game.name
        ));
    }
    let src = PathBuf::from(game.install_root());
    if !src.is_dir() {
        return Err(format!(
            "La carpeta \"{}\" no existe o no es accesible desde esta máquina",
            src.display()
        ));
    }
    let install_dir = src
        .file_name()
        .ok_or_else(|| format!("Ruta inválida: {}", src.display()))?
        .to_string_lossy()
        .into_owned();

    let dest = mount_point.join(NON_STEAM_DIR).join(&install_dir);
    copy_dir_all(&src, &dest, &mut on_progress)?;

    // The exe Steam launches is usually relative to StartDir, but shortcuts
    // can also carry an absolute path pointing inside the same folder
    // (Steam accepts both) — strip whichever prefix actually matches so
    // the recorded path is always cartridge-relative either way.
    let exe_name = Path::new(&game.exe)
        .strip_prefix(&src)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from(Path::new(&game.exe).file_name().unwrap_or_default()));

    let exe_path = Path::new(NON_STEAM_DIR)
        .join(&install_dir)
        .join(exe_name)
        .to_string_lossy()
        .replace('\\', "/");

    let app = CartridgeApp {
        app_id: game.id,
        name: game.name.clone(),
        source: AppSource::NonSteam,
        // DRM-free by construction — the whole reason a raw copy is enough.
        preservability: Preservability::Trivial,
        standalone: true,
        exe_path,
    };
    add_app(mount_point, app.clone())?;

    // Art/description/screenshots are NOT fetched here — same division of
    // labor Steam/GOG installs already have: `poll_install_status`/
    // `run_gog_download` only ever copy bytes and touch the marker,
    // `cartridge_manage.js`'s "Preparar launcher" loop is what fetches art
    // afterward (and can be re-run later without re-copying the whole
    // game). Live feedback: an earlier version fetched art/description
    // inline in this function — animated art choices ended up baked into
    // the copy step with no way to re-run just that part.
    Ok(app)
}

/// Recursive directory copy. Skips anything that's neither a regular file
/// nor a directory (sockets, device nodes) — a game install folder has no
/// legitimate use for those, same "ignore weird entries" stance
/// `usage::size_of` already takes for the read side of this problem.
fn copy_dir_all(src: &Path, dst: &Path, on_progress: &mut impl FnMut(u64)) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("No se pudo crear {}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("No se pudo leer {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry
            .file_type()
            .map_err(|e| format!("No se pudo leer {}: {e}", entry.path().display()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to, on_progress)?;
        } else if ty.is_file() {
            copy_file_chunked(&from, &to, on_progress)
                .map_err(|e| format!("No se pudo copiar {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// `fs::copy` hands the whole file to a single OS call (`copy_file_range`/
/// `sendfile`) with no hook in between — fine for the file itself, but a
/// modern game's few-huge-.pak-files layout (confirmed live: an Unreal
/// Engine game reporting its progress only once per multi-GB file) made
/// the bar sit still for minutes at a time, indistinguishable from a hang.
/// This reads/writes in fixed chunks instead, calling `on_progress` after
/// each one — same "call it a few MB at a time" idea `gog_download`'s own
/// chunked downloader already applies for the same reason, just for a
/// local copy instead of a network transfer.
fn copy_file_chunked(
    from: &Path,
    to: &Path,
    on_progress: &mut impl FnMut(u64),
) -> std::io::Result<()> {
    use std::io::{Read, Write};

    const CHUNK: usize = 4 * 1024 * 1024;
    let mut reader = fs::File::open(from)?;
    let mut writer = fs::File::create(to)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        on_progress(n as u64);
    }
    // `fs::copy` also preserves the source's permission bits (its own docs
    // guarantee this on Unix) — replicated here so this manual replacement
    // doesn't silently drop an executable bit some tool in the folder
    // (a bundled launcher script, a native Linux binary sitting next to
    // the Windows one) actually depends on.
    let perms = fs::metadata(from)?.permissions();
    fs::set_permissions(to, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: u64, start_dir: &Path, exe: &Path) -> NonSteamGame {
        NonSteamGame {
            id,
            name: "Some Game".to_string(),
            exe: exe.to_string_lossy().into_owned(),
            icon: String::new(),
            last_played: 0,
            start_dir: start_dir.to_string_lossy().into_owned(),
            steam_app_id: None,
            install_root_override: None,
        }
    }

    #[test]
    fn copies_the_whole_folder_and_records_the_app() {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("game.exe"), b"exe").unwrap();
        fs::create_dir(src.path().join("data")).unwrap();
        fs::write(src.path().join("data").join("assets.pak"), b"pak").unwrap();

        let cartridge = tempfile::tempdir().unwrap();
        write_marker(cartridge.path()).unwrap();

        let install_dir = src
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let g = game(1, src.path(), &src.path().join("game.exe"));
        let app = add_non_steam_app(cartridge.path(), &g, |_| {}).unwrap();

        assert_eq!(app.source, AppSource::NonSteam);
        assert_eq!(app.preservability, Preservability::Trivial);
        assert_eq!(app.exe_path, format!("NON-STEAM/{install_dir}/game.exe"));
        assert!(
            cartridge
                .path()
                .join("NON-STEAM")
                .join(&install_dir)
                .join("data")
                .join("assets.pak")
                .exists()
        );
    }

    #[test]
    fn rejects_a_missing_start_dir() {
        let g = game(
            1,
            Path::new("/nonexistent/path/xyz"),
            Path::new("/nonexistent/path/xyz/g.exe"),
        );
        let cartridge = tempfile::tempdir().unwrap();
        write_marker(cartridge.path()).unwrap();
        assert!(add_non_steam_app(cartridge.path(), &g, |_| {}).is_err());
    }

    #[test]
    fn rejects_an_empty_start_dir_field() {
        let mut g = game(1, Path::new("/tmp"), Path::new("/tmp/g.exe"));
        g.start_dir.clear();
        let cartridge = tempfile::tempdir().unwrap();
        write_marker(cartridge.path()).unwrap();
        assert!(add_non_steam_app(cartridge.path(), &g, |_| {}).is_err());
    }
}
