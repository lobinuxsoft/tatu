use std::path::{Path, PathBuf};

/// Resolves a Proton install referenced by `name` to the `proton`
/// script's absolute path. Accepts:
///   - absolute path to the `proton` script itself
///   - absolute path to the install directory
///   - directory name under `<steam_root>/compatibilitytools.d/` (custom builds)
///   - directory name under `<steam_root>/steamapps/common/`      (official builds)
pub fn resolve(name: &str, steam_root: &Path) -> Result<PathBuf, ProtonError> {
    let candidates = candidates(name, steam_root);
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| ProtonError::NotFound(name.to_owned()))
}

fn candidates(name: &str, steam_root: &Path) -> Vec<PathBuf> {
    let raw = Path::new(name);
    if raw.is_absolute() {
        return match raw.file_name().and_then(|n| n.to_str()) {
            Some("proton") => vec![raw.to_path_buf()],
            _ => vec![raw.join("proton")],
        };
    }
    vec![
        steam_root
            .join("compatibilitytools.d")
            .join(name)
            .join("proton"),
        steam_root
            .join("steamapps/common")
            .join(name)
            .join("proton"),
    ]
}

/// Finds the active Steam root. `~/.steam/root` is the canonical
/// symlink Steam keeps current across reinstalls; falls back to the
/// usual install locations when it's missing.
pub fn steam_root() -> Result<PathBuf, ProtonError> {
    let home = dirs::home_dir().ok_or(ProtonError::NoHome)?;
    let candidates = [
        home.join(".steam/root"),
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
    ];
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or(ProtonError::NoSteamRoot)
}

#[derive(Debug, thiserror::Error)]
pub enum ProtonError {
    #[error("HOME not set, cannot locate Steam root")]
    NoHome,
    #[error("no Steam install found under ~/.steam/{{root,steam}} or ~/.local/share/Steam")]
    NoSteamRoot,
    #[error("proton install '{0}' not found in compatibilitytools.d/ or steamapps/common/")]
    NotFound(String),
}
