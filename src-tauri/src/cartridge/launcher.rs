use std::fs;
use std::path::Path;

/// Copies both the Linux and Windows launcher binaries onto the cartridge
/// root, alongside the `.tatu-cartridge.json` marker and `runtime/` (#206).
/// Both always land together, never just one — a cartridge has to run on
/// whichever OS the destination machine is (yaguarete_os#314 on Linux, an
/// autorun equivalent on Windows), and it has no way to know that in
/// advance. Called from the "prepare cartridge" step, not per-game install:
/// the launcher is shared by the whole cartridge, not tied to any one app.
///
/// `linux_binary`/`windows_binary` are Tatu's own vendored, pre-exported
/// Godot builds (built by CI, not committed — see `vendor/launcher/` in
/// `tauri.conf.json`'s bundle resources), resolved by the caller.
pub fn install_launcher_binaries(
    mount_point: &Path,
    linux_binary: &Path,
    windows_binary: &Path,
) -> Result<(), String> {
    copy_binary(linux_binary, &mount_point.join("tatu-launcher"), true)?;
    copy_binary(
        windows_binary,
        &mount_point.join("tatu-launcher.exe"),
        false,
    )
}

fn copy_binary(src: &Path, dest: &Path, executable: bool) -> Result<(), String> {
    fs::copy(src, dest)
        .map_err(|e| format!("Cannot copy {} to {}: {e}", src.display(), dest.display()))?;

    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = executable;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_binaries_land_at_the_cartridge_root() {
        let cartridge = tempfile::tempdir().unwrap();
        let sources = tempfile::tempdir().unwrap();
        let linux_src = sources.path().join("tatu-launcher");
        let windows_src = sources.path().join("tatu-launcher.exe");
        fs::write(&linux_src, b"elf-stub").unwrap();
        fs::write(&windows_src, b"pe-stub").unwrap();

        install_launcher_binaries(cartridge.path(), &linux_src, &windows_src).unwrap();

        assert!(cartridge.path().join("tatu-launcher").is_file());
        assert!(cartridge.path().join("tatu-launcher.exe").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn the_linux_binary_comes_out_executable() {
        use std::os::unix::fs::PermissionsExt;

        let cartridge = tempfile::tempdir().unwrap();
        let sources = tempfile::tempdir().unwrap();
        let linux_src = sources.path().join("tatu-launcher");
        let windows_src = sources.path().join("tatu-launcher.exe");
        fs::write(&linux_src, b"elf-stub").unwrap();
        fs::write(&windows_src, b"pe-stub").unwrap();

        install_launcher_binaries(cartridge.path(), &linux_src, &windows_src).unwrap();

        let mode = fs::metadata(cartridge.path().join("tatu-launcher"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn a_missing_source_binary_is_reported() {
        let cartridge = tempfile::tempdir().unwrap();
        let err = install_launcher_binaries(
            cartridge.path(),
            Path::new("/nonexistent/tatu-launcher"),
            Path::new("/nonexistent/tatu-launcher.exe"),
        )
        .unwrap_err();
        assert!(err.contains("tatu-launcher"));
    }
}
