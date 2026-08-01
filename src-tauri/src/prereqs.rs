//! User-side installer for [`cheat_runtime::Prereq`] variants (#98).
//!
//! Detection lives in `cheat_runtime::prereqs` (filesystem-only, deps-free).
//! The installer side is here because it pulls `ureq` for the GitHub
//! Releases JSON + zip blob download, plus the `zip` crate to extract
//! the dinput8.dll out of praydog's release archive.
//!
//! ### REFramework install algorithm
//!
//! 1. `GET https://api.github.com/repos/praydog/REFramework-nightly/releases/latest`
//!    — JSON describing the latest nightly. We pull `assets[].name` +
//!    `assets[].browser_download_url`.
//! 2. Pick the asset named `REFramework.zip` (the canonical proxy build —
//!    covers RE2/3/4, MHW, DD2, SF6, PRAGMATA, Kunitsu-Gami, …).
//! 3. Download into memory (~12 MB ceiling, well under any sensible RAM
//!    budget — the tracker is a desktop UI, not a daemon).
//! 4. Open as zip, extract `dinput8.dll` into a sibling `*.tmp` file next
//!    to the final target so the rename is atomic on the same filesystem.
//! 5. If `<game_dir>/dinput8.dll` already exists (corrupt / older mod),
//!    rename it to `dinput8.dll.tatu-backup` once, then `tatu-backup.N`
//!    if a backup already exists.
//! 6. Rename the `.tmp` over the final location.
//!
//! Errors at any step surface as [`PrereqError`] with enough context for
//! the UI's toast — the user must be able to read "GitHub API returned
//! 404" or "extracted dinput8.dll was 0 bytes" and act on it.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cheat_runtime::{UnityBackend, detect_unity_backend};
use serde::Deserialize;

const GITHUB_API: &str = "https://api.github.com/repos/praydog/REFramework-nightly/releases/latest";
const ASSET_NAME: &str = "REFramework.zip";
const DLL_NAME: &str = "dinput8.dll";
const USER_AGENT: &str = concat!("tatu-tracker/", env!("CARGO_PKG_VERSION"));

/// The Mono collector DLL, cross-built (`x86_64-pc-windows-gnu`, release,
/// stripped) and embedded at compile time. Unlike REFramework — which we
/// download from praydog's GitHub releases — tatu builds this collector
/// itself, so we vendor the pre-built artifact in `assets/` and ship it in
/// the binary. Regenerate with `assets/regen-mono-collector.sh` whenever
/// the `cheat-mono-collector` crate changes.
const MONO_COLLECTOR_DLL: &[u8] = include_bytes!("../assets/cheat-mono-collector.dll");

/// Proxy name the collector masquerades as under Proton/Wine. The DLL
/// exports the full winhttp surface (45 forwarders) so the game loads it
/// transparently; pairing this with `WINEDLLOVERRIDES=winhttp=n,b` makes
/// Wine prefer our sibling DLL over the prefix's builtin.
const MONO_PROXY_NAME: &str = "winhttp.dll";

#[derive(Debug, thiserror::Error)]
pub enum PrereqError {
    #[error("github releases API request failed: {0}")]
    Network(String),
    #[error("github releases JSON had no asset named {0}")]
    AssetMissing(&'static str),
    #[error("download body was {got} bytes; expected at least {min}")]
    DownloadTooSmall { got: u64, min: u64 },
    #[error("zip extraction failed: {0}")]
    Zip(String),
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("game directory not found for app_id {0}")]
    GameDirMissing(String),
    #[error("extracted dinput8.dll was empty")]
    EmptyDll,
    #[error("{0} is not a Unity Mono game; the Mono collector only loads into Mono runtimes")]
    NotMonoUnity(String),
}

/// What the installer wrote, for the UI's success toast.
#[derive(Debug, serde::Serialize)]
pub struct InstalledInfo {
    /// Resolved final path of the placed `dinput8.dll`.
    pub dll_path: String,
    /// Size in bytes of the placed `dinput8.dll`.
    pub size_bytes: u64,
    /// Release tag name from GitHub (e.g. `latest`). Useful for the user
    /// to verify they got the build they expected.
    pub release_tag: String,
    /// Path of any previous `dinput8.dll` that was renamed out of the
    /// way before the new one was placed. `None` for fresh installs.
    pub backed_up: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleasePayload {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Install praydog's REFramework `dinput8.dll` proxy into `game_dir`.
///
/// Atomic against partial writes: the new DLL lands in a `.tmp` sibling
/// first and is renamed over the final path only after the zip + write
/// succeed. Any existing `dinput8.dll` is moved to a numbered backup
/// (`dinput8.dll.tatu-backup` / `.tatu-backup.2` / …) so a botched
/// install doesn't shred an unrelated mod the user had in place.
pub fn install_reframework(game_dir: &Path) -> Result<InstalledInfo, PrereqError> {
    if !game_dir.is_dir() {
        return Err(PrereqError::GameDirMissing(game_dir.display().to_string()));
    }

    let release = fetch_latest_release()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == ASSET_NAME)
        .ok_or(PrereqError::AssetMissing(ASSET_NAME))?;

    let zip_bytes = download_blob(&asset.browser_download_url)?;
    if zip_bytes.len() < 256 * 1024 {
        return Err(PrereqError::DownloadTooSmall {
            got: zip_bytes.len() as u64,
            min: 256 * 1024,
        });
    }

    let dll_bytes = extract_dll_from_zip(&zip_bytes)?;
    if dll_bytes.is_empty() {
        return Err(PrereqError::EmptyDll);
    }

    let final_path = game_dir.join(DLL_NAME);
    let backup = move_existing_aside(&final_path)?;
    let size_bytes = write_atomic(&final_path, &dll_bytes)?;

    Ok(InstalledInfo {
        dll_path: final_path.display().to_string(),
        size_bytes,
        release_tag: release.tag_name,
        backed_up: backup.map(|p| p.display().to_string()),
    })
}

/// Drop the embedded Mono collector into `game_dir` as `winhttp.dll`, so a
/// Unity Mono game running under Proton loads it on launch (the collector's
/// `DllMain` then waits for Mono, attaches, and serves `Class:Method`→JIT
/// resolution to native tatu over TCP loopback).
///
/// Gated on [`detect_unity_backend`] returning [`UnityBackend::Mono`] — the
/// collector only knows how to talk to the `mono_*` C API; dropping it into
/// an IL2CPP or non-Unity game would do nothing useful and would shadow the
/// game's real winhttp. Mirrors [`install_reframework`]'s atomic write +
/// numbered backup, but the bytes come from the compiled-in artifact rather
/// than a download, so there is no network/zip step.
///
/// Note: this only places the DLL. The companion launch-option
/// (`WINEDLLOVERRIDES=winhttp=n,b`) is a separate, smoke-time step.
pub fn install_mono_collector(game_dir: &Path) -> Result<InstalledInfo, PrereqError> {
    if !game_dir.is_dir() {
        return Err(PrereqError::GameDirMissing(game_dir.display().to_string()));
    }
    if detect_unity_backend(game_dir) != UnityBackend::Mono {
        return Err(PrereqError::NotMonoUnity(game_dir.display().to_string()));
    }

    let final_path = game_dir.join(MONO_PROXY_NAME);
    let backup = move_existing_aside(&final_path)?;
    let size_bytes = write_atomic(&final_path, MONO_COLLECTOR_DLL)?;

    Ok(InstalledInfo {
        dll_path: final_path.display().to_string(),
        size_bytes,
        // Not a download — report the tatu version that bundled the artifact.
        release_tag: concat!("tatu ", env!("CARGO_PKG_VERSION")).to_string(),
        backed_up: backup.map(|p| p.display().to_string()),
    })
}

fn fetch_latest_release() -> Result<ReleasePayload, PrereqError> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent(USER_AGENT)
        .build()
        .new_agent();
    let body: ReleasePayload = agent
        .get(GITHUB_API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| PrereqError::Network(e.to_string()))?
        .body_mut()
        .read_json()
        .map_err(|e| PrereqError::Network(format!("JSON parse: {e}")))?;
    Ok(body)
}

fn download_blob(url: &str) -> Result<Vec<u8>, PrereqError> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(USER_AGENT)
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| PrereqError::Network(e.to_string()))?;
    // 32 MB ceiling — REFramework.zip is ~6-12 MB; anything past that is
    // either a wrong asset or a runaway download.
    let body = response
        .body_mut()
        .with_config()
        .limit(32 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| PrereqError::Network(format!("body read: {e}")))?;
    Ok(body)
}

fn extract_dll_from_zip(zip_bytes: &[u8]) -> Result<Vec<u8>, PrereqError> {
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| PrereqError::Zip(e.to_string()))?;
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).map(|e| e.name().to_string()))
        .collect::<Result<_, _>>()
        .map_err(|e| PrereqError::Zip(e.to_string()))?;
    // praydog's zip has `dinput8.dll` at the root; do an
    // ends_with match anyway in case future builds nest it.
    let dll_name = names
        .iter()
        .find(|n| n.eq_ignore_ascii_case(DLL_NAME) || n.to_ascii_lowercase().ends_with(DLL_NAME))
        .ok_or_else(|| PrereqError::Zip(format!("zip has no {DLL_NAME} entry")))?
        .clone();
    let mut entry = archive
        .by_name(&dll_name)
        .map_err(|e| PrereqError::Zip(e.to_string()))?;
    let mut out = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut out)
        .map_err(|e| PrereqError::Zip(format!("read {dll_name}: {e}")))?;
    Ok(out)
}

/// If `target` already exists, rename it to `<target>.tatu-backup` (or
/// `.tatu-backup.N` to avoid clobbering an earlier backup). Returns the
/// backup path so the UI can surface it.
fn move_existing_aside(target: &Path) -> Result<Option<PathBuf>, PrereqError> {
    if !target.exists() {
        return Ok(None);
    }
    let stem = target.to_path_buf();
    let mut backup = stem.with_extension("dll.tatu-backup");
    let mut n = 2;
    while backup.exists() {
        backup = stem.with_extension(format!("dll.tatu-backup.{n}"));
        n += 1;
        if n > 99 {
            // Defensive cap — if 99 backups are stacked the user has
            // bigger problems than we can solve here.
            return Err(PrereqError::Io {
                path: stem.clone(),
                source: std::io::Error::other("too many existing backups"),
            });
        }
    }
    fs::rename(&stem, &backup).map_err(|source| PrereqError::Io { path: stem, source })?;
    Ok(Some(backup))
}

fn write_atomic(target: &Path, bytes: &[u8]) -> Result<u64, PrereqError> {
    let tmp = target.with_extension("dll.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|source| PrereqError::Io {
            path: tmp.clone(),
            source,
        })?;
        f.write_all(bytes).map_err(|source| PrereqError::Io {
            path: tmp.clone(),
            source,
        })?;
        f.sync_all().map_err(|source| PrereqError::Io {
            path: tmp.clone(),
            source,
        })?;
    }
    fs::rename(&tmp, target).map_err(|source| PrereqError::Io {
        path: tmp.clone(),
        source,
    })?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn move_existing_aside_is_noop_when_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("dinput8.dll");
        let result = move_existing_aside(&target).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn move_existing_aside_renames_to_tatu_backup() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("dinput8.dll");
        fs::write(&target, b"OLD_BYTES").unwrap();

        let backup = move_existing_aside(&target).unwrap().unwrap();
        assert!(!target.exists());
        assert!(backup.exists());
        assert_eq!(fs::read(&backup).unwrap(), b"OLD_BYTES");
        assert!(backup.to_string_lossy().ends_with(".tatu-backup"));
    }

    #[test]
    fn move_existing_aside_increments_when_backup_already_present() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("dinput8.dll");
        fs::write(&target, b"V1").unwrap();
        let first = move_existing_aside(&target).unwrap().unwrap();
        assert!(first.to_string_lossy().ends_with(".tatu-backup"));

        // Re-create the target, second backup must avoid clobbering the first.
        fs::write(&target, b"V2").unwrap();
        let second = move_existing_aside(&target).unwrap().unwrap();
        assert!(second.to_string_lossy().ends_with(".tatu-backup.2"));
        // V1 preserved.
        assert_eq!(fs::read(&first).unwrap(), b"V1");
        assert_eq!(fs::read(&second).unwrap(), b"V2");
    }

    #[test]
    fn write_atomic_lands_full_bytes_at_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("dinput8.dll");
        let bytes: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let size = write_atomic(&target, &bytes).unwrap();
        assert_eq!(size, 1024);
        assert_eq!(fs::read(&target).unwrap(), bytes);
        // No leftover .tmp.
        assert!(!target.with_extension("dll.tmp").exists());
    }

    #[test]
    fn install_reports_game_dir_missing() {
        let bogus = std::path::Path::new("/does/not/exist/at/all/please");
        let err = install_reframework(bogus).unwrap_err();
        match err {
            PrereqError::GameDirMissing(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn extract_dll_handles_root_entry() {
        // Build a tiny in-memory zip with `dinput8.dll` at the root,
        // mirroring praydog's release layout.
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(cursor);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zw.start_file("dinput8.dll", opts).unwrap();
        zw.write_all(b"FAKE_DLL").unwrap();
        let buf = zw.finish().unwrap().into_inner();

        let extracted = extract_dll_from_zip(&buf).unwrap();
        assert_eq!(extracted, b"FAKE_DLL");
    }

    /// Build a minimal Unity Mono game layout `detect_unity_backend` accepts.
    fn make_mono_game(root: &Path) {
        let data = root.join("Game_Data");
        fs::create_dir_all(data.join("Managed")).unwrap();
        fs::write(data.join("globalgamemanagers"), b"\0").unwrap();
        fs::write(data.join("Managed/Assembly-CSharp.dll"), b"MZ").unwrap();
        let runtime = data.join("MonoBleedingEdge/EmbedRuntime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("mono-2.0-bdwgc.dll"), b"MZ").unwrap();
    }

    #[test]
    fn embedded_collector_dll_has_a_pe_header() {
        // Guards against the vendored artifact going missing/empty at build
        // time — include_bytes! would still compile an empty slice.
        assert!(
            MONO_COLLECTOR_DLL.len() > 64 * 1024,
            "collector DLL too small"
        );
        assert_eq!(&MONO_COLLECTOR_DLL[..2], b"MZ", "not a PE image");
    }

    #[test]
    fn install_mono_reports_game_dir_missing() {
        let bogus = std::path::Path::new("/does/not/exist/at/all/please");
        match install_mono_collector(bogus).unwrap_err() {
            PrereqError::GameDirMissing(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn install_mono_rejects_non_mono_game() {
        let tmp = TempDir::new().unwrap();
        // A directory that exists but isn't a Unity Mono game.
        match install_mono_collector(tmp.path()).unwrap_err() {
            PrereqError::NotMonoUnity(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn install_mono_places_winhttp_into_mono_game() {
        let tmp = TempDir::new().unwrap();
        make_mono_game(tmp.path());

        let info = install_mono_collector(tmp.path()).unwrap();
        let placed = tmp.path().join("winhttp.dll");
        assert!(placed.is_file());
        assert_eq!(fs::read(&placed).unwrap(), MONO_COLLECTOR_DLL);
        assert_eq!(info.size_bytes, MONO_COLLECTOR_DLL.len() as u64);
        assert!(info.backed_up.is_none());
        assert!(!placed.with_extension("dll.tmp").exists());
    }

    #[test]
    fn install_mono_backs_up_existing_winhttp() {
        let tmp = TempDir::new().unwrap();
        make_mono_game(tmp.path());
        let placed = tmp.path().join("winhttp.dll");
        fs::write(&placed, b"PREEXISTING").unwrap();

        let info = install_mono_collector(tmp.path()).unwrap();
        let backup = info.backed_up.expect("should back up existing winhttp");
        assert!(backup.ends_with(".tatu-backup"));
        assert_eq!(fs::read(&backup).unwrap(), b"PREEXISTING");
        assert_eq!(fs::read(&placed).unwrap(), MONO_COLLECTOR_DLL);
    }

    #[test]
    fn extract_dll_errors_when_zip_has_no_dll() {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(cursor);
        let opts: zip::write::SimpleFileOptions = Default::default();
        zw.start_file("readme.txt", opts).unwrap();
        zw.write_all(b"hi").unwrap();
        let buf = zw.finish().unwrap().into_inner();

        let err = extract_dll_from_zip(&buf).unwrap_err();
        match err {
            PrereqError::Zip(msg) => assert!(msg.contains("dinput8.dll")),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
