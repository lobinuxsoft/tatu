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
    #[serde(default)]
    screenshots: Vec<Screenshot>,
}

#[derive(Debug, Deserialize)]
struct Movie {
    /// HLS master playlist — Steam stopped serving flat mp4/webm files for
    /// trailers at some point; verified live against the real API (#212)
    /// before writing this. `ffmpeg` ingests the manifest URL directly, no
    /// separate segment-by-segment download needed.
    hls_h264: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Screenshot {
    /// The 1920x1080 original — same file the launcher's gallery (#213)
    /// both thumbnails and enlarges, so there's no separate smaller
    /// `path_thumbnail` variant to bother caching too.
    path_full: String,
}

/// Caches this app's top SteamGridDB cover art at `assets/<app_id>/grid.<ext>`
/// on the cartridge (#205). The launcher (#204) only ever reads this local
/// file, never SteamGridDB directly — the destination machine may be offline
/// when it runs.
///
/// No art found for this app is not an error: the install this is attached
/// to already succeeded, and an empty `assets/<app_id>/` just means the
/// launcher falls back to no cover art for that entry.
///
/// Always re-fetched, unlike the trailer below — a cheap request, worth
/// paying every time "Preparar launcher" re-runs so a cartridge picks up a
/// better cover art SteamGridDB gets later, rather than being stuck with
/// whatever was cached the first time forever.
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
    // A re-fetch can land a different extension than last time (SteamGridDB
    // switches formats between calls) — remove any stale grid.* first so
    // exactly one ever exists, never an orphaned old file sitting next to
    // the new one that the launcher might pick by extension order instead.
    remove_grid_art(&dir);
    fs::write(dir.join(format!("grid.{ext}")), bytes)
        .map_err(|e| format!("Cannot write grid art: {e}"))
}

/// Deletes any existing `grid.*` file in `dir`, if present.
fn remove_grid_art(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let is_grid = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("grid."));
        if is_grid {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Caches this app's short store description at
/// `assets/<app_id>/description.txt` on the cartridge — the carousel's info
/// panel (#204) reads this local file, same offline-first reasoning as the
/// cover art above. Steam's `appdetails` endpoint is public, no API key.
/// Always re-fetched, same "cheap enough to just redo it" reasoning as the
/// cover art.
pub async fn fetch_cartridge_description(mount_point: PathBuf, app_id: u64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || fetch_cartridge_description_sync(&mount_point, app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_cartridge_description_sync(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let dir = mount_point.join("assets").join(app_id.to_string());

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

/// Caches this app's Steam store screenshots at
/// `assets/<app_id>/screenshots/<n>.jpg` (#213) — the launcher's gallery
/// only ever reads whatever files already sit in that folder, same
/// "Tatu prepares, launcher consumes" split every asset here follows.
///
/// Idempotent, unlike art/description above: a released game's screenshot
/// set essentially never changes, and re-downloading every one of them
/// (often a dozen-plus, unlike a single grid image) on every "Preparar
/// launcher" click would add up as a cartridge's library grows.
pub async fn fetch_cartridge_screenshots(mount_point: PathBuf, app_id: u64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || fetch_cartridge_screenshots_sync(&mount_point, app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_cartridge_screenshots_sync(mount_point: &Path, app_id: u64) -> Result<(), String> {
    let dir = mount_point
        .join("assets")
        .join(app_id.to_string())
        .join("screenshots");
    if fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some()) {
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
    let screenshots = entry
        .success
        .then_some(entry.data)
        .flatten()
        .map(|d| d.screenshots)
        .unwrap_or_default();
    if screenshots.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    for (i, shot) in screenshots.iter().enumerate() {
        let bytes = agent
            .get(&shot.path_full)
            .call()
            .map_err(|e| format!("Screenshot {i} download failed: {e}"))?
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("Screenshot {i} read failed: {e}"))?;
        fs::write(dir.join(format!("{i:02}.jpg")), bytes)
            .map_err(|e| format!("Cannot write screenshot {i}: {e}"))?;
    }
    Ok(())
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
    let Some(master_url) = entry
        .success
        .then_some(entry.data)
        .flatten()
        .and_then(|d| d.movies.into_iter().find_map(|m| m.hls_h264))
    else {
        return Ok(());
    };

    // Feeding ffmpeg the master playlist directly — the natural first
    // attempt — actually produced a corrupt-looking trailer (glitchy in a
    // plain video player, nothing to do with Godot's own playback):
    // confirmed live (#212) as `ffmpeg` reading multiple video renditions
    // plus a separately-grouped audio track off the same ambiguous master
    // and interleaving them ("Packet corrupt", "Invalid NAL unit size").
    // Resolving one concrete ~720p rendition's own video+audio
    // sub-playlists first sidesteps that entirely, and is a real quality
    // win too — Steam's own 720p encode beats re-encoding-and-downscaling
    // its 1080p one ourselves. 720p rather than the full 1080p60 "max"
    // rendition: Godot's Theora decoder is software-only, and this is
    // playing behind the launcher's own shader-heavy UI on the same frame
    // budget — a lower resolution costs noticeably less to decode there.
    let playlist = agent
        .get(&master_url)
        .call()
        .map_err(|e| format!("HLS master playlist request failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("HLS master playlist read failed: {e}"))?;
    let Some((video_url, audio_url)) = pick_hls_rendition(&master_url, &playlist) else {
        return Ok(());
    };

    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return Err("ffmpeg not found on this system — install it to cache trailers".to_string());
    }

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    // ffmpeg picks its output muxer from the file extension unless told
    // otherwise — the atomic-write `.part` suffix (same pattern runtime.rs
    // already uses for its own downloads) hides the real `.ogv` extension
    // from that guess, so `-f ogg` says so explicitly instead. `fps=30`
    // caps decode cost further: the source runs ~60fps despite the
    // playlist's own FRAME-RATE=30 metadata tag (also confirmed live),
    // more than a background loop behind the UI needs.
    let part = dir.join("trailer.ogv.part");
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &video_url,
            "-i",
            &audio_url,
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-vf",
            "fps=30",
            "-c:v",
            "libtheora",
            "-qscale:v",
            "6",
            "-c:a",
            "libvorbis",
            "-qscale:a",
            "4",
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

/// Resolves a modest-quality (largest rendition at or under 720p, falling
/// back to the smallest available if none qualifies) rendition from an HLS
/// master playlist to direct video and (separately grouped) audio
/// sub-playlist URLs, both carrying the master's own query string — Steam's
/// CDN paths are structured so relative sub-playlist filenames sit right
/// next to the master's own.
fn pick_hls_rendition(master_url: &str, playlist: &str) -> Option<(String, String)> {
    let base = &master_url[..master_url.rfind('/')?];
    let query = master_url.find('?').map(|i| &master_url[i..]).unwrap_or("");

    let audio_file = playlist
        .lines()
        .find(|line| line.starts_with("#EXT-X-MEDIA:TYPE=AUDIO"))
        .and_then(|line| extract_quoted_attr(line, "URI"))?;

    let lines: Vec<&str> = playlist.lines().collect();
    let mut renditions: Vec<(u32, &str)> = Vec::new();
    for i in 0..lines.len() {
        let Some(height) = lines[i]
            .strip_prefix("#EXT-X-STREAM-INF:")
            .and_then(extract_resolution_height)
        else {
            continue;
        };
        let Some(&file) = lines.get(i + 1).filter(|f| !f.starts_with('#')) else {
            continue;
        };
        renditions.push((height, file));
    }
    let video_file = renditions
        .iter()
        .filter(|(height, _)| *height <= 720)
        .max_by_key(|(height, _)| *height)
        .or_else(|| renditions.iter().min_by_key(|(height, _)| *height))
        .map(|(_, file)| *file)?;

    let join = |file: &str| format!("{base}/{file}{query}");
    Some((join(video_file), join(audio_file)))
}

/// The value of a `KEY="value"` attribute inside an HLS tag line.
fn extract_quoted_attr<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    rest.get(..rest.find('"')?)
}

/// The height out of a `RESOLUTION=WxH` attribute inside an `#EXT-X-STREAM-INF` tag.
fn extract_resolution_height(attrs: &str) -> Option<u32> {
    let start = attrs.find("RESOLUTION=")? + "RESOLUTION=".len();
    let rest = &attrs[start..];
    let end = rest.find(',').unwrap_or(rest.len());
    rest[..end].split_once('x')?.1.parse().ok()
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

    #[test]
    fn an_already_populated_screenshots_dir_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let shots_dir = dir.path().join("assets").join("1").join("screenshots");
        fs::create_dir_all(&shots_dir).unwrap();
        fs::write(shots_dir.join("00.jpg"), b"already here").unwrap();

        fetch_cartridge_screenshots_sync(dir.path(), 1).unwrap();

        let entries: Vec<_> = fs::read_dir(&shots_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    // Real playlist pulled live from Steam's CDN for Alabaster Dawn
    // (appid 3110760) while investigating #212's corrupt-trailer bug —
    // not a hand-written fixture, so a future format change would show up
    // here instead of only in production.
    const REAL_MASTER_PLAYLIST: &str = "#EXTM3U\n\
#EXT-X-VERSION:7\n\
#EXT-X-INDEPENDENT-SEGMENTS\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"Default\",AUTOSELECT=YES,DEFAULT=YES,URI=\"hls_264_4_audio.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=5800000,CODECS=\"avc1.640029,mp4a.40.2\",RESOLUTION=1920x1080,FRAME-RATE=30,AUDIO=\"audio\"\n\
hls_264_0_video.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2600000,CODECS=\"avc1.640029,mp4a.40.2\",RESOLUTION=1280x720,FRAME-RATE=30,AUDIO=\"audio\"\n\
hls_264_1_video.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=1400000,CODECS=\"avc1.640029,mp4a.40.2\",RESOLUTION=854x480,FRAME-RATE=30,AUDIO=\"audio\"\n\
hls_264_2_video.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=1000000,CODECS=\"avc1.640029,mp4a.40.2\",RESOLUTION=640x360,FRAME-RATE=30,AUDIO=\"audio\"\n\
hls_264_3_video.m3u8\n";

    #[test]
    fn picks_the_720p_rendition_over_1080p() {
        let master = "https://video.example/store_trailers/3110760/x/hls_264_master.m3u8?t=123";
        let (video, audio) = pick_hls_rendition(master, REAL_MASTER_PLAYLIST).unwrap();
        assert_eq!(
            video,
            "https://video.example/store_trailers/3110760/x/hls_264_1_video.m3u8?t=123"
        );
        assert_eq!(
            audio,
            "https://video.example/store_trailers/3110760/x/hls_264_4_audio.m3u8?t=123"
        );
    }

    #[test]
    fn falls_back_to_the_smallest_rendition_when_none_is_720p_or_under() {
        let master = "https://video.example/x/master.m3u8";
        let playlist = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",URI=\"audio.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=2560x1440,AUDIO=\"audio\"\n\
v0.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2,RESOLUTION=1920x1080,AUDIO=\"audio\"\n\
v1.m3u8\n";
        let (video, _) = pick_hls_rendition(master, playlist).unwrap();
        assert_eq!(video, "https://video.example/x/v1.m3u8");
    }
}
