//! GOG account integration (#243) — OAuth2 login against the user's own
//! GOG account and listing what they own. Native Rust, no external CLI
//! (`gogdl`/`lgogdownloader`) — investigated both, neither covers
//! login+library+cross-platform in one tool, and this endpoint set is
//! small enough that shelling out would add more integration cost than it
//! saves. Scope here stops at login + owned-games list; actually
//! downloading a game is GOG's separate content-system v2 protocol
//! (chunked, zlib-compressed manifests) — real scope of its own, not
//! attempted here.
//!
//! `CLIENT_ID`/`CLIENT_SECRET` below are the ones baked into the official
//! GOG Galaxy client itself — reverse-engineered and published years ago
//! (Yepoleb/gogapidocs, widely reused by gogdl/lgogdownloader/gogcli).
//! Not a Tatu secret: it identifies "a GOG Galaxy-compatible client" to
//! GOG's auth server, same category as a public OAuth client id.

use serde::{Deserialize, Serialize};

const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
const REDIRECT_URI: &str = "https://embed.gog.com/on_login_success?origin=client";
const USER_AGENT: &str =
    "game-progress-tracker (+https://github.com/lobinuxsoft/game-progress-tracker)";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GogTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GogOwnedGame {
    pub id: u64,
    pub title: String,
    /// Small square icon — used for the list row thumbnail.
    #[serde(default)]
    pub icon_url: Option<String>,
    /// Wide background art — used for the detail window's full-width
    /// header banner, matching Steam's `header_img` there. `icon_url`
    /// stretched to that width looked exactly as bad as it sounds
    /// (a 64px square upscaled full-width, confirmed live) — this is the
    /// field actually meant for that treatment, from the same response.
    #[serde(default)]
    pub background_url: Option<String>,
    /// Empty when no confident title match is found on `catalog.gog.com`
    /// (see `fetch_genres_and_developers`) — GOG's by-id product endpoint
    /// this module otherwise relies on has neither field at all, no matter
    /// which `expand` values are requested (checked live against every
    /// documented one). The catalog's own search is relevance-ranked, not
    /// substring matching — confirmed live: searching "Alone in the Dark 2"
    /// returns ten unrelated titles, none of them a match — so this stays
    /// empty for a real chunk of any library, not a bug to chase further.
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub release_date: Option<String>,
}

/// URL to open in the system browser to start the login flow. No embedded
/// webview, no custom URI scheme registration: the user logs in there and
/// pastes the resulting redirect URL (or just its `code=`) back into
/// Tatu — same UX `gh auth login --web` uses for the same reason (a
/// Tauri webview can't intercept a real browser's redirect).
pub fn login_url() -> String {
    format!(
        "https://auth.gog.com/auth?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&layout=client2",
        urlencoding::encode(REDIRECT_URI)
    )
}

/// Accepts either a bare authorization code or the full URL GOG redirected
/// to (`.../on_login_success?origin=client&code=<code>`) — whichever the
/// user pastes back.
pub fn extract_code(pasted: &str) -> Option<String> {
    let trimmed = pasted.trim();
    if let Some(idx) = trimmed.find("code=") {
        let rest = &trimmed[idx + "code=".len()..];
        let code = rest.split('&').next().unwrap_or(rest);
        return (!code.is_empty()).then(|| code.to_string());
    }
    (!trimmed.is_empty() && !trimmed.contains("://")).then(|| trimmed.to_string())
}

pub fn exchange_code(code: &str) -> Result<GogTokens, String> {
    request_token(&[
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
    ])
}

pub fn refresh(tokens: &GogTokens) -> Result<GogTokens, String> {
    request_token(&[
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("grant_type", "refresh_token"),
        ("refresh_token", &tokens.refresh_token),
    ])
}

/// GOG's token endpoint takes its params as a query string on a GET, not a
/// POST body — confirmed against community docs (Yepoleb/gogapidocs
/// auth.rst), unusual for an OAuth2 token exchange but that's what the
/// real endpoint expects.
fn request_token(params: &[(&str, &str)]) -> Result<GogTokens, String> {
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("https://auth.gog.com/token?{query}");
    let mut response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("GOG token request failed: {e}"))?;
    let body: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("GOG token response parse failed: {e}"))?;
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("GOG token response missing access_token")?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or("GOG token response missing refresh_token")?
        .to_string();
    Ok(GogTokens {
        access_token,
        refresh_token,
    })
}

/// The IDs the account owns — `embed.gog.com/user/data/games` returns just
/// numeric product IDs, no titles (confirmed against community docs).
fn fetch_owned_ids(access_token: &str) -> Result<Vec<u64>, String> {
    let mut response = ureq::get("https://embed.gog.com/user/data/games")
        .header("User-Agent", USER_AGENT)
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("GOG library request failed: {e}"))?;
    let body: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("GOG library response parse failed: {e}"))?;
    let ids = body
        .get("owned")
        .and_then(|v| v.as_array())
        .ok_or("GOG library response missing 'owned'")?
        .iter()
        .filter_map(|v| v.as_u64())
        .collect();
    Ok(ids)
}

/// Resolves a product id to its title via GOG's public, unauthenticated
/// product endpoint — the same one gogdb.org itself uses to build its
/// database. A failed lookup falls back to the raw numeric id as the title
/// instead of dropping the entry: the user still owns it either way.
///
/// Description and screenshots are the only fields left out of this bulk
/// fetch (real per-game cost with no list-row payoff — see
/// `fetch_extra_details`, called lazily only when the detail view opens).
/// Genre/developer come from a second request against `catalog.gog.com`
/// (title search, exact-match-only) so the list rows can show the same
/// tag pills Steam's do wherever a confident match exists — real cost
/// (doubles the per-game request count during a library sync), accepted
/// because showing nothing where Steam shows genre tags was the bigger
/// problem live-reported (#243).
fn fetch_details(id: u64) -> GogOwnedGame {
    let fallback = || GogOwnedGame {
        id,
        title: id.to_string(),
        icon_url: None,
        background_url: None,
        release_date: None,
        genres: Vec::new(),
        developers: Vec::new(),
    };
    let url = format!("https://api.gog.com/products/{id}");
    let Ok(mut response) = ureq::get(&url).header("User-Agent", USER_AGENT).call() else {
        return fallback();
    };
    let Ok(body) = response.body_mut().read_json::<serde_json::Value>() else {
        return fallback();
    };

    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string());
    // GOG serves every image URL protocol-relative ("//host/path.jpg") —
    // prefixing "https:" is required for the frontend to load it directly.
    let images = body.get("images");
    let image_url = |key: &str| {
        images
            .and_then(|images| images.get(key))
            .and_then(|v| v.as_str())
            .map(|s| format!("https:{s}"))
    };
    let icon_url = image_url("icon");
    let background_url = image_url("background");
    let release_date = body
        .get("release_date")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let (genres, developers) = fetch_genres_and_developers(&title);

    GogOwnedGame {
        id,
        title,
        icon_url,
        background_url,
        release_date,
        genres,
        developers,
    }
}

/// Owned ids only — the caller resolves details itself (one request per
/// id, worth reporting progress on for a real library size), see
/// `commands::gog_cmd::fetch_gog_library`.
pub fn fetch_owned_game_ids(access_token: &str) -> Result<Vec<u64>, String> {
    fetch_owned_ids(access_token)
}

pub fn resolve_details(id: u64) -> GogOwnedGame {
    fetch_details(id)
}

/// Everything the detail view shows beyond the library row (#243) — fetched
/// lazily, only when the user actually opens a game, not baked into
/// `resolve_details` for every game in the library on every sync. Genre and
/// developer are NOT here: those live on `GogOwnedGame` itself (resolved
/// once during the bulk sync, off `catalog.gog.com`), so the list rows and
/// this detail view read the same values instead of fetching them twice.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GogExtraDetails {
    /// GOG's own field is raw HTML straight from the store page; tags are
    /// stripped rather than rendered, matching this frontend's rule of
    /// never putting fetched markup into `innerHTML` unescaped.
    pub description: Option<String>,
    pub screenshot_urls: Vec<String>,
}

pub fn fetch_extra_details(id: u64) -> GogExtraDetails {
    fetch_description_and_screenshots(id)
}

fn fetch_description_and_screenshots(id: u64) -> GogExtraDetails {
    let mut out = GogExtraDetails::default();
    let url = format!("https://api.gog.com/products/{id}?expand=description,screenshots");
    let Ok(mut response) = ureq::get(&url).header("User-Agent", USER_AGENT).call() else {
        return out;
    };
    let Ok(body) = response.body_mut().read_json::<serde_json::Value>() else {
        return out;
    };

    out.description = body
        .get("description")
        .and_then(|d| d.get("lead"))
        .and_then(|v| v.as_str())
        .map(strip_html_tags)
        .filter(|s| !s.is_empty());

    // Each screenshot ships a handful of pre-cropped sizes under
    // `formatted_images` rather than one URL — "ggvgl" is GOG's own large
    // gallery-view crop, the same one the store page itself displays.
    out.screenshot_urls = body
        .get("screenshots")
        .and_then(|v| v.as_array())
        .map(|shots| {
            shots
                .iter()
                .filter_map(|shot| {
                    shot.get("formatted_images")?
                        .as_array()?
                        .iter()
                        .find(|img| {
                            img.get("formatter_name").and_then(|v| v.as_str()) == Some("ggvgl")
                        })?
                        .get("image_url")?
                        .as_str()
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();

    out
}

/// GOG's by-id product endpoint (used everywhere else in this module) has
/// no genre or developer field at all — those only exist on
/// `catalog.gog.com`'s title-search endpoint, already used the same way by
/// `drm::gog::confirmed_on_gog` for the same reason. Only trusted on an
/// exact title match after normalizing (not just the first "like:" hit,
/// which can easily be an unrelated game sharing a word) — an unmatched
/// title returns empty rather than guessing.
fn fetch_genres_and_developers(title: &str) -> (Vec<String>, Vec<String>) {
    let empty = (Vec::new(), Vec::new());
    let url = format!(
        "https://catalog.gog.com/v1/catalog?query=like:{}&limit=10",
        urlencoding::encode(title)
    );
    let Ok(mut response) = ureq::get(&url).header("User-Agent", USER_AGENT).call() else {
        return empty;
    };
    let Ok(body) = response.body_mut().read_json::<serde_json::Value>() else {
        return empty;
    };
    let Some(products) = body.get("products").and_then(|v| v.as_array()) else {
        return empty;
    };

    let target = normalize_title(title);
    let Some(matched) = products.iter().find(|p| {
        p.get("productType").and_then(|v| v.as_str()) == Some("game")
            && p.get("title")
                .and_then(|v| v.as_str())
                .is_some_and(|t| normalize_title(t) == target)
    }) else {
        return empty;
    };

    let names_of = |field: &str| {
        matched
            .get(field)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .or_else(|| v.get("name")?.as_str().map(str::to_string))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    (names_of("genres"), names_of("developers"))
}

/// Same normalization `drm::gog::normalize_title` uses — lowercase,
/// alphanumeric only, no attempt at stripping edition suffixes. Kept as
/// its own copy rather than shared across modules: three lines, and the
/// two call sites belong to different concerns (DRM classification vs.
/// account/library data).
fn normalize_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Drops every `<...>` tag regardless of its content — safe against
/// malformed/nested markup by construction, since nothing between angle
/// brackets ever survives into the output. Blank lines `<br>`/`<p>`
/// boundaries leave behind are collapsed, and the handful of entities
/// GOG's own store markup actually uses are decoded.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_tags_drops_tags_and_keeps_text() {
        let html = "<p class=\"module\"><b>Hello</b><br>\nworld.<br></p>";
        assert_eq!(strip_html_tags(html), "Hello\n\nworld.");
    }

    #[test]
    fn strip_html_tags_decodes_common_entities() {
        assert_eq!(strip_html_tags("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(strip_html_tags("&quot;quoted&quot;"), "\"quoted\"");
    }

    #[test]
    fn strip_html_tags_never_lets_a_tag_survive_even_if_malformed() {
        let html = "<script>alert(1)</script>text<b unterminated";
        let out = strip_html_tags(html);
        assert!(!out.contains('<'));
        assert!(!out.contains('>'));
    }

    #[test]
    fn extracts_code_from_a_full_redirect_url() {
        let url = "https://embed.gog.com/on_login_success?origin=client&code=abc123";
        assert_eq!(extract_code(url), Some("abc123".to_string()));
    }

    #[test]
    fn extracts_code_when_it_is_not_the_last_query_param() {
        let url = "https://embed.gog.com/on_login_success?code=abc123&origin=client";
        assert_eq!(extract_code(url), Some("abc123".to_string()));
    }

    #[test]
    fn accepts_a_bare_code_with_no_url_around_it() {
        assert_eq!(extract_code("abc123"), Some("abc123".to_string()));
    }

    #[test]
    fn trims_whitespace_around_a_pasted_value() {
        assert_eq!(extract_code("  abc123  "), Some("abc123".to_string()));
    }

    #[test]
    fn rejects_an_empty_paste() {
        assert_eq!(extract_code(""), None);
        assert_eq!(extract_code("   "), None);
    }

    #[test]
    fn rejects_a_url_with_no_code_param() {
        assert_eq!(extract_code("https://embed.gog.com/on_login_success"), None);
    }
}
