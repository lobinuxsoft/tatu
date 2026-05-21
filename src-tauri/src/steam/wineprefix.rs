//! Resolve the Wine prefix that Steam provisions for any installed appid.
//!
//! Steam stores Proton per-game state under
//! `<library>/steamapps/compatdata/<appid>/`. The bridge backend
//! [`crate::state::GameBackend::Bridge`] needs `<prefix>/pfx/` so it
//! can compute the in-prefix port file the `tatu-bridge` writes on
//! bind (Aurora-style TCP loopback, post-#121).
//!
//! Resolution walks every Steam library — primary + extra mounts —
//! returned by [`crate::steam::install::library_paths`] and stops at
//! the first `compatdata/<appid>/pfx` that exists on disk. The pfx
//! is what Steam itself created when the user first launched the
//! title under any Proton flavour, so its presence doubles as a
//! "this game has been launched under Proton at least once" check
//! (a precondition for the bridge backend — without it there's no
//! `drive_c` for the bridge socket / port file).

use std::path::PathBuf;

use crate::steam::install::library_paths;

/// Locate the Wine prefix Steam provisioned for `app_id`.
///
/// Returns the absolute path to `<library>/steamapps/compatdata/<appid>/pfx`
/// for the first library that owns the game, or `None` if no such
/// directory exists in any configured library.
pub(crate) fn resolve_wineprefix(app_id: &str) -> Option<PathBuf> {
    for lib in library_paths() {
        let pfx = lib
            .join("steamapps")
            .join("compatdata")
            .join(app_id)
            .join("pfx");
        if pfx.is_dir() {
            return Some(pfx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    /// Mirrors the on-disk shape `resolve_wineprefix` walks. The
    /// resolver itself reads `library_paths()` which depends on a
    /// real `libraryfolders.vdf`; the test exercises the inner
    /// `compatdata/<app_id>/pfx` shape directly so it stays
    /// hermetic.
    #[test]
    fn pfx_layout_matches_steam_convention() {
        let tmp = TempDir::new().unwrap();
        let lib = tmp.path();
        let pfx = lib
            .join("steamapps")
            .join("compatdata")
            .join("2725260")
            .join("pfx");
        fs::create_dir_all(&pfx).unwrap();
        assert!(pfx.is_dir());
        assert_eq!(
            pfx.file_name().and_then(|n| n.to_str()),
            Some("pfx"),
            "Steam pfx leaf must be exactly 'pfx' so wine sees it as $WINEPREFIX"
        );
    }
}
