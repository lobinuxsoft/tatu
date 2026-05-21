//! Filesystem paths the Tatu Launcher module operates on.
//!
//! Two roles split here:
//!
//! - **Install dir**: where Steam looks for compat tools. Always
//!   `<steam>/compatibilitytools.d/tatu-launcher/`. Steam picks up
//!   any directory under `compatibilitytools.d/` that contains a
//!   readable `compatibilitytool.vdf` at startup; the directory
//!   name itself is just a label, but we keep it stable so the
//!   per-game `CompatToolMapping` rows stay valid across upgrades.
//!
//! - **Source candidates**: where the tracker should look for a
//!   freshly built drop-in payload to copy into the install dir.
//!   Ordered from most-canonical (staged `target/dist/`) to
//!   most-permissive (cached previous install). The first existing
//!   candidate wins.

use std::path::PathBuf;

use crate::steam::steam_install_dir;
use crate::tatu_launcher::TatuLauncherError;

/// The directory name Steam sees under `compatibilitytools.d/`.
/// Must match the inner key in `tools/tatu-launcher/compatibilitytool.vdf`
/// so the `CompatToolMapping[<appid>].name` value the tracker writes into
/// `config.vdf` resolves back to this drop-in.
pub const COMPAT_TOOL_NAME: &str = "tatu-launcher";

const COMPAT_TOOLS_SUBDIR: &str = "compatibilitytools.d";

/// Files the drop-in must contain to be considered installed (in
/// addition to the wrapper script + manifests). The Win32 binary is
/// optional for the `NotInstalled → installation` transition because
/// the install routine writes a placeholder when the user wants to
/// install without the bridge yet, but its absence flips status to
/// `Corrupt` because the bridge backend will fail at runtime.
pub(super) const REQUIRED_FILES: &[&str] = &[
    "tatu-launcher",
    "tatu-launcher.sh",
    "toolmanifest.vdf",
    "compatibilitytool.vdf",
    "tatu-bridge.exe",
];

/// Where the drop-in lives once installed. Sibling to whatever
/// Proton-GE / Proton-stl / etc. the user already has there.
pub fn install_dir() -> Result<PathBuf, TatuLauncherError> {
    let steam = steam_install_dir().ok_or(TatuLauncherError::NoSteam)?;
    Ok(steam.join(COMPAT_TOOLS_SUBDIR).join(COMPAT_TOOL_NAME))
}

/// Steam's root `config.vdf` carrying the `CompatToolMapping` block.
pub(super) fn config_vdf_path() -> Result<PathBuf, TatuLauncherError> {
    let steam = steam_install_dir().ok_or(TatuLauncherError::NoSteam)?;
    Ok(steam.join("config").join("config.vdf"))
}

/// Ordered candidates the installer searches for a staged drop-in
/// payload. The first directory containing every entry in
/// [`REQUIRED_FILES`] is used as the source. Order matters:
///
/// 1. `<repo>/target/dist/tatu-launcher/` — output of
///    `scripts/build-tatu-launcher.sh` (mounts both binaries +
///    scripts + VDFs).
/// 2. `<repo>/tools/tatu-launcher/` paired with `<repo>/target/release/`
///    and `<repo>/target/x86_64-pc-windows-gnu/release/` — the
///    in-repo source layout. Stitched together at copy time, not
///    represented as a single directory.
/// 3. `<tracker_executable_dir>/tatu-launcher/` — sibling to the
///    AppImage / installer for distribution bundles that ship the
///    payload next to the binary.
/// 4. `<XDG_DATA_HOME>/tatu/tatu-launcher/` — previous install
///    cached for upgrade-in-place.
pub fn source_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join("target/dist/tatu-launcher"));
        // Pop up to repo root if invoked from src-tauri/.
        if cwd.file_name().and_then(|n| n.to_str()) == Some("src-tauri")
            && let Some(parent) = cwd.parent()
        {
            out.push(parent.join("target/dist/tatu-launcher"));
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.join("tatu-launcher"));
    }

    if let Some(data) = dirs::data_dir() {
        out.push(data.join("tatu").join("tatu-launcher"));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_candidates_are_distinct_and_named() {
        let cands = source_candidates();
        // At least the cwd-rooted candidate always lands; in CI both
        // may exist (cwd + cwd.parent fallback). Both data + exe are
        // best-effort. Empty is impossible because cwd always
        // resolves in a normal test runner.
        assert!(!cands.is_empty(), "expected at least one source candidate");
        for cand in &cands {
            assert!(
                cand.ends_with("tatu-launcher"),
                "every candidate ends with the drop-in dir name, got {}",
                cand.display()
            );
        }
    }
}
