// Manual SteamGridDB art picker (#328, ported from `capydeploy`'s own
// `ArtworkSelector.svelte` + `crates/steamgriddb` — same author, same API,
// intentionally the same shape) for non-Steam entries: unlike
// `fetch_cartridge_art`'s "take the first grid result" auto-pick (fine for
// a real Steam appid, where the search space is one exact game), a
// non-Steam entry's title is free text — the user has to see the
// candidates and confirm, same reasoning #328's AppID field already
// applies to the Steam Store preview.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::shortcuts::ArtworkSelection;

const BASE_URL: &str = "https://www.steamgriddb.com/api/v2";

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtSearchResult {
    pub id: i32,
    pub name: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtImage {
    #[serde(default)]
    pub width: i32,
    #[serde(default)]
    pub height: i32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub thumb: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    )
}

/// Same agent, timed differently — SteamGridDB's "animated" picks (#328's
/// own Filters panel exposes them) aren't small: confirmed live, a chosen
/// capsule/hero pair for one game ran 33-46MB each. A single
/// `timeout_global` (what `agent()` above uses, fine for its small JSON
/// calls) has no idea a request is a 46MB body versus a dead server — it
/// counts DNS+connect+headers+body as one lump sum, so the only way to
/// make it survive a real big file is to also make it survive a genuinely
/// hung connection for just as long.
///
/// `ureq` (checked directly, no version has this) doesn't expose a real
/// idle/stall timeout — one that resets every time a byte arrives, only
/// firing on an actual stall. Every knob it has is a cumulative cap over
/// some phase. Splitting the phases is the closest fit available without
/// hand-rolling raw socket reads or pulling in `curl` for its
/// `low_speed_time`/`low_speed_limit` (a true stall detector, but a new
/// dependency + FFI for one code path): connect/response-headers get a
/// short, strict cap (if a server hasn't even replied by then, it never
/// will); the body — where a 46MB file legitimately takes a while — gets
/// its own long cap, uncontaminated by the fast phases before it.
fn download_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .timeout_recv_body(Some(Duration::from_secs(300)))
            .build(),
    )
}

pub async fn search_games(api_key: String, term: String) -> Result<Vec<ArtSearchResult>, String> {
    tokio::task::spawn_blocking(move || {
        let url = format!(
            "{BASE_URL}/search/autocomplete/{}",
            urlencoding::encode(&term)
        );
        let resp: ApiResponse<Vec<ArtSearchResult>> = agent()
            .get(&url)
            .header("Authorization", &format!("Bearer {api_key}"))
            .call()
            .map_err(|e| format!("SteamGridDB search failed: {e}"))?
            .into_body()
            .read_json()
            .map_err(|e| format!("SteamGridDB search response parse failed: {e}"))?;
        Ok(resp.data)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

/// SteamGridDB's own `types`/`nsfw`/`humor` query filters, bundled so the 4
/// image-type fetchers below don't grow a 5th/6th positional `bool` each —
/// matches capydeploy's own `ImageFilters` in shape, just the subset #328's
/// Filters panel actually exposes (Types + Tags, not Styles/Dimensions/
/// Formats — those need per-asset-type option lists this port doesn't have
/// a UI for yet).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtFilters {
    /// `"static"` / `"animated"` / empty (both).
    #[serde(default)]
    pub image_type: String,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub humor: bool,
}

/// `kind` is the URL segment (`grids`/`heroes`/`logos`/`icons`) — one
/// generic fetcher instead of four near-identical copies, same DRY
/// reasoning `fetch_appdetails` already applies on the Steam-store side.
/// `page` is 0-indexed, same as SteamGridDB's own API and capydeploy's own
/// `client.rs` — a game's default grid set alone can run past 300 results
/// (confirmed live: DOOM returns 380), so a single unpaginated call was
/// silently only ever showing the first 50.
async fn fetch_images(
    api_key: String,
    game_id: i32,
    kind: &'static str,
    page: i32,
    filters: ArtFilters,
) -> Result<Vec<ArtImage>, String> {
    tokio::task::spawn_blocking(move || fetch_images_sync(&api_key, game_id, kind, page, &filters))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_images_sync(
    api_key: &str,
    game_id: i32,
    kind: &'static str,
    page: i32,
    filters: &ArtFilters,
) -> Result<Vec<ArtImage>, String> {
    let mut url = format!(
        "{BASE_URL}/{kind}/game/{game_id}?page={page}&nsfw={}&humor={}",
        if filters.nsfw { "any" } else { "false" },
        if filters.humor { "any" } else { "false" },
    );
    if !filters.image_type.is_empty() {
        url.push_str("&types=");
        url.push_str(&filters.image_type);
    }
    let resp: ApiResponse<Vec<ArtImage>> = agent()
        .get(&url)
        .header("Authorization", &format!("Bearer {api_key}"))
        .call()
        .map_err(|e| format!("SteamGridDB {kind} request failed: {e}"))?
        .into_body()
        .read_json()
        .map_err(|e| format!("SteamGridDB {kind} response parse failed: {e}"))?;
    if !resp.success {
        return Err(format!(
            "SteamGridDB reported an unsuccessful {kind} request"
        ));
    }
    Ok(resp.data)
}

/// Grids cover BOTH capsule (portrait, height > width) and wide (landscape,
/// width > height) — same single `/grids/game/{id}` endpoint capydeploy's
/// own `capsule`/`wide` tabs split client-side, not two different calls.
pub async fn fetch_grids(
    api_key: String,
    game_id: i32,
    page: i32,
    filters: ArtFilters,
) -> Result<Vec<ArtImage>, String> {
    fetch_images(api_key, game_id, "grids", page, filters).await
}

pub async fn fetch_heroes(
    api_key: String,
    game_id: i32,
    page: i32,
    filters: ArtFilters,
) -> Result<Vec<ArtImage>, String> {
    fetch_images(api_key, game_id, "heroes", page, filters).await
}

pub async fn fetch_logos(
    api_key: String,
    game_id: i32,
    page: i32,
    filters: ArtFilters,
) -> Result<Vec<ArtImage>, String> {
    fetch_images(api_key, game_id, "logos", page, filters).await
}

pub async fn fetch_icons(
    api_key: String,
    game_id: i32,
    page: i32,
    filters: ArtFilters,
) -> Result<Vec<ArtImage>, String> {
    fetch_images(api_key, game_id, "icons", page, filters).await
}

/// Async wrapper for `fetch_cartridge_art`/`fetch_gog_cartridge_art`'s
/// callers (#328): when the user manually picked art for a Steam or GOG
/// app, this — not the single-grid auto-pick those two use — is what
/// actually lands on the cartridge. `app_id` here is `assets/<app_id>/`,
/// same folder either path writes into.
pub async fn save_selected_artwork(
    api_key: String,
    mount_point: PathBuf,
    app_id: u64,
    selection: ArtworkSelection,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let dir = mount_point.join("assets").join(app_id.to_string());
        let errors = save_selected_artwork_sync(&dir, &api_key, &selection);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

/// Downloads whichever slots of `selection` are set to `dir/<slot>.<ext>` —
/// called from "Preparar launcher" (#236/#328), same as the Steam/GOG art
/// refresh loop it sits next to in `cartridge_manage.js`, so the launcher
/// (#329) only ever reads local files, never SteamGridDB directly. A
/// per-slot download failure (a stale URL, a network blip) is logged into
/// the returned list rather than aborting the rest — the copy this is
/// attached to already succeeded, and partial art beats none at all.
///
/// The portrait capsule slot is saved as plain `grid.<ext>`, not
/// `grid_portrait.<ext>` — `main.gd::_grid_art_path` (the carousel card's
/// cover art, already shipped, unrelated to this feature) only ever looks
/// for that exact name, and only ever displays a static `Image` — an
/// animated pick there (live-reported: a chosen capsule was a looping
/// .webm) shows up as a blank card, not a moving one, because Godot's
/// `Image.load()` can't decode video. `resolve_grid_url` below substitutes
/// a static alternative for THIS file specifically when that happens; the
/// user's actual pick stays exactly as chosen in `state.json`'s `artwork`
/// map, for whenever #329 registers a real Steam shortcut (which — like
/// Steam's own library grid — can render the animated version natively).
/// `grid`/`grid_landscape`/`hero`/`logo`/`icon` are Steam's own local grid
/// art slot names — untouched copies of the user's pick, kept for #329's
/// still-unbuilt Steam shortcut registration, which (like Steam's real
/// library) can render an animated pick natively. `card.<ext>` is a second,
/// separate file this function also writes: the launcher's own art, which
/// can NOT render animated media (see `write_launcher_card` below) and so
/// must never share a filename with the Steam-bound copy — writing a
/// static substitute into `grid.<ext>` itself, an earlier version of this
/// fix, corrupted the very file #329 needs to stay untouched.
pub fn save_selected_artwork_sync(
    dir: &Path,
    api_key: &str,
    selection: &ArtworkSelection,
) -> Vec<String> {
    let agent = download_agent();
    let mut errors = Vec::new();

    let slots: [(&str, &str); 5] = [
        ("grid", &selection.grid_portrait),
        ("grid_landscape", &selection.grid_landscape),
        ("hero", &selection.hero),
        ("logo", &selection.logo),
        ("icon", &selection.icon),
    ];
    for (slot, url) in slots {
        if url.is_empty() {
            continue;
        }
        if let Err(e) = download_one(&agent, dir, slot, url) {
            errors.push(format!("{slot}: {e}"));
        }
    }

    if !selection.grid_portrait.is_empty()
        && let Err(e) = write_launcher_card(&agent, dir, api_key, selection)
    {
        errors.push(format!("card: {e}"));
    }
    errors
}

/// The launcher's own card art (`card.<ext>`, read by `main.gd::
/// _grid_art_path`) must be something Godot's `Image.load()` can actually
/// decode — unlike the `grid.<ext>` copy above, which stays whatever the
/// user picked. The first fix here checked the picked URL for a `.webm`
/// suffix — wrong: confirmed live, SteamGridDB also serves animated
/// portraits as an animated WebP, same `RIFF/WEBP` container a static one
/// uses, just carrying an `ANIM` chunk, so the suffix check let it straight
/// through and the card stayed blank anyway. This sniffs the real bytes
/// (`looks_animated`) instead of guessing from the URL, only falling back
/// to a fresh `types=static` query when the bytes themselves prove
/// undecodable. A missing api key, no `griddb_game_id`, or no static result
/// just means the launcher gets no card art at all for this app — same
/// "not an error" precedent `fetch_cartridge_art`'s own doc comment already
/// sets for "no match found".
fn write_launcher_card(
    agent: &ureq::Agent,
    dir: &Path,
    api_key: &str,
    selection: &ArtworkSelection,
) -> Result<(), String> {
    let mut url = selection.grid_portrait.clone();
    let mut bytes = fetch_bytes(agent, &url)?;
    if looks_animated(&bytes) {
        let Some(replacement) = resolve_static_grid(api_key, selection) else {
            return Ok(());
        };
        url = replacement;
        bytes = fetch_bytes(agent, &url)?;
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("card.{}", url_ext(&url))), bytes).map_err(|e| e.to_string())
}

/// Animated WebP always carries an `ANIM` chunk right after the mandatory
/// `VP8X` header — within the file's first bytes, no need to parse the
/// whole thing. `.webm`/Matroska's own EBML magic number catches the video
/// case SteamGridDB also serves for some picks.
fn looks_animated(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(128)];
    (bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
        && head.windows(4).any(|w| w == b"ANIM"))
        || bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
}

fn resolve_static_grid(api_key: &str, selection: &ArtworkSelection) -> Option<String> {
    if api_key.is_empty() || selection.griddb_game_id == 0 {
        return None;
    }
    let filters = ArtFilters {
        image_type: "static".to_string(),
        nsfw: false,
        humor: true,
    };
    fetch_images_sync(api_key, selection.griddb_game_id, "grids", 0, &filters)
        .ok()?
        .into_iter()
        .find(|i| i.height > i.width)
        .map(|i| i.url)
}

/// `ureq` caps a response body at 10MB by default (a safety limit, not a
/// timeout — confirmed live: this, not the timeouts above, was the real
/// reason every animated pick failed, with the honest error
/// "response body is larger than request limit: 10485760" once errors
/// stopped being swallowed). SteamGridDB's animated hero images ran up to
/// 46MB in the same live case, so the override needs real headroom, not
/// just past the default.
const MAX_ART_BYTES: u64 = 100 * 1024 * 1024;

fn url_ext(url: &str) -> &str {
    url.rsplit('.')
        .next()
        .filter(|e| e.len() <= 4 && !e.contains('/'))
        .unwrap_or("png")
}

fn fetch_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .into_body()
        .with_config()
        .limit(MAX_ART_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

fn download_one(agent: &ureq::Agent, dir: &Path, slot: &str, url: &str) -> Result<(), String> {
    let bytes = fetch_bytes(agent, url)?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(format!("{slot}.{}", url_ext(url))), bytes).map_err(|e| e.to_string())
}
