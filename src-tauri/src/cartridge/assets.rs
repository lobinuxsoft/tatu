use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GridsResponse {
    success: bool,
    #[serde(default)]
    data: Vec<GridImage>,
}

#[derive(Debug, Deserialize)]
struct GridImage {
    url: String,
}

#[derive(Debug, Deserialize)]
struct AppDetailsEntry {
    success: bool,
    data: Option<AppDetailsData>,
}

#[derive(Debug, Deserialize)]
struct AppDetailsData {
    #[serde(default)]
    short_description: String,
    #[serde(default)]
    movies: Vec<Movie>,
}

#[derive(Debug, Deserialize)]
struct Movie {
    /// HLS master playlist — Steam stopped serving flat mp4/webm files for
    /// trailers at some point; verified live against the real API (#212)
    /// before writing this. `ffmpeg` ingests the manifest URL directly, no
    /// separate segment-by-segment download needed.
    hls_h264: Option<String>,
}

/// Caches this app's top SteamGridDB cover art at `assets/<app_id>/grid.<ext>`
/// on the cartridge (#205). The launcher (#204) only ever reads this local
/// file, never SteamGridDB directly — the destination machine may be offline
/// when it runs.
///
/// No art found for this app is not an error: the install this is attached
/// to already succeeded, and an empty `assets/<app_id>/` just means the
/// launcher falls back to no cover art for that entry.
pub async fn fetch_cartridge_art(
    api_key: String,
    mount_point: PathBuf,
    app_id: u64,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || fetch_cartridge_art_sync(&api_key, &mount_point, app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_cartridge_art_sync(api_key: &str, mount_point: &Path, app_id: u64) -> Result<(), String> {
    let dir = mount_point.join("assets").join(app_id.to_string());
    if has_grid_art(&dir) {
        return Ok(());
    }
    if api_key.is_empty() {
        return Err("No SteamGridDB API key configured".to_string());
    }

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );

    let grids: GridsResponse = agent
        .get(format!(
            "https://www.steamgriddb.com/api/v2/grids/steam/{app_id}"
        ))
        .header("Authorization", &format!("Bearer {api_key}"))
        .call()
        .map_err(|e| format!("SteamGridDB request failed: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("SteamGridDB response parse failed: {e}"))?;

    if !grids.success {
        return Err("SteamGridDB reported an unsuccessful request".to_string());
    }
    let Some(image) = grids.data.first() else {
        return Ok(());
    };

    let ext = extension_of(&image.url);
    let bytes = agent
        .get(&image.url)
        .call()
        .map_err(|e| format!("Cover art download failed: {e}"))?
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("Cover art read failed: {e}"))?;

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    fs::write(dir.join(format!("grid.{ext}")), bytes)
        .map_err(|e| format!("Cannot write grid art: {e}"))
}

/// Whether `dir` already has a `grid.*` file from a previous call — skips
/// hitting SteamGridDB again every time #204's batch "prepare cartridge"
/// step re-runs for a cartridge that already has some apps set up.
fn has_grid_art(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("grid."))
    })
}

/// Caches this app's short store description at
/// `assets/<app_id>/description.txt` on the cartridge — the carousel's info
/// panel (#204) reads this local file, same offline-first reasoning as the
/// cover art above. Steam's `appdetails` endpoint is public, no API key.
pub async fn fetch_cartridge_description(mount_point: PathBuf, app_id: u64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || fetch_cartridge_description_sync(&mount_point, app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_cartridge_description_sync(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let dir = mount_point.join("assets").join(app_id.to_string());
    if dir.join("description.txt").is_file() {
        return Ok(());
    }

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );

    let mut response: HashMap<String, AppDetailsEntry> = agent
        .get(format!(
            "https://store.steampowered.com/api/appdetails?appids={app_id}"
        ))
        .call()
        .map_err(|e| format!("Steam appdetails request failed: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("Steam appdetails response parse failed: {e}"))?;

    let Some(entry) = response.remove(&app_id.to_string()) else {
        return Ok(());
    };
    let Some(description) = entry
        .success
        .then_some(entry.data)
        .flatten()
        .map(|d| d.short_description)
        .filter(|d| !d.is_empty())
    else {
        return Ok(());
    };

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    fs::write(dir.join("description.txt"), description)
        .map_err(|e| format!("Cannot write description: {e}"))
}

/// Caches this app's Steam store trailer, transcoded to Ogg Theora
/// (`assets/<app_id>/trailer.ogv`) — the only video format Godot 4 ships
/// native playback for (WebM support was dropped in the 4.0 rewrite, MP4
/// was never supported). **Opt-in**, unlike the two fetches above: a
/// trailer runs 10-100+ MB, real cartridge storage cost cover art and
/// descriptions never had — the caller (the Cartucho tab's "Preparar
/// launcher" step) only calls this when the user asks for trailers too.
///
/// No trailer listed, or `ffmpeg` missing from the machine running Tatu,
/// is not fatal to the batch this runs in: the launcher (#204) already
/// falls back to a cached screenshot or the blurred cover art when this
/// file is absent.
pub async fn fetch_cartridge_trailer(mount_point: PathBuf, app_id: u64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || fetch_cartridge_trailer_sync(&mount_point, app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_cartridge_trailer_sync(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let dir = mount_point.join("assets").join(app_id.to_string());
    let dest = dir.join("trailer.ogv");
    if dest.is_file() {
        return Ok(());
    }

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );

    let mut response: HashMap<String, AppDetailsEntry> = agent
        .get(format!(
            "https://store.steampowered.com/api/appdetails?appids={app_id}"
        ))
        .call()
        .map_err(|e| format!("Steam appdetails request failed: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("Steam appdetails response parse failed: {e}"))?;

    let Some(entry) = response.remove(&app_id.to_string()) else {
        return Ok(());
    };
    let Some(movie_url) = entry
        .success
        .then_some(entry.data)
        .flatten()
        .and_then(|d| d.movies.into_iter().find_map(|m| m.hls_h264))
    else {
        return Ok(());
    };

    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return Err("ffmpeg not found on this system — install it to cache trailers".to_string());
    }

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    // ffmpeg picks its output muxer from the file extension unless told
    // otherwise — the atomic-write `.part` suffix (same pattern runtime.rs
    // already uses for its own downloads) hides the real `.ogv` extension
    // from that guess, so `-f ogg` says so explicitly instead.
    let part = dir.join("trailer.ogv.part");
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &movie_url,
            "-c:v",
            "libtheora",
            "-c:a",
            "libvorbis",
            "-f",
            "ogg",
        ])
        .arg(&part)
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;
    if !output.status.success() {
        let _ = fs::remove_file(&part);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.lines().rev().take(5).collect::<Vec<_>>().join(" | ");
        return Err(format!("ffmpeg failed: {tail}"));
    }
    fs::rename(&part, &dest).map_err(|e| format!("Cannot finalize trailer file: {e}"))
}

/// The file extension off a SteamGridDB image URL, falling back to `png` —
/// every format the API actually serves (png/jpg/webp) round-trips through
/// this fine, and a wrong guess here only costs a mislabeled-but-still-valid
/// image file, never a crash.
fn extension_of(url: &str) -> String {
    url.rsplit('.')
        .next()
        .filter(|ext| ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("png")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_is_read_from_the_url() {
        assert_eq!(
            extension_of("https://cdn2.steamgriddb.com/grid/abc.png"),
            "png"
        );
        assert_eq!(
            extension_of("https://cdn2.steamgriddb.com/grid/abc.jpeg"),
            "jpeg"
        );
    }

    #[test]
    fn a_url_with_no_recognizable_extension_falls_back_to_png() {
        assert_eq!(extension_of("https://cdn2.steamgriddb.com/grid/abc"), "png");
        assert_eq!(
            extension_of("https://cdn2.steamgriddb.com/grid/abc.verylongstuff"),
            "png"
        );
    }

    #[test]
    fn missing_api_key_is_refused_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let err = fetch_cartridge_art_sync("", dir.path(), 1).unwrap_err();
        assert!(err.contains("No SteamGridDB API key"));
    }

    #[test]
    fn an_already_cached_trailer_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("assets").join("1");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(app_dir.join("trailer.ogv"), b"already here").unwrap();

        fetch_cartridge_trailer_sync(dir.path(), 1).unwrap();

        assert_eq!(
            fs::read(app_dir.join("trailer.ogv")).unwrap(),
            b"already here"
        );
    }
}
