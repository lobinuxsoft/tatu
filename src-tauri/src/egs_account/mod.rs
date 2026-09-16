//! Epic Games Store account integration (#343) — OAuth2 login against the
//! user's own EGS account and listing what they own. Native Rust, mirroring
//! `gog_account`'s shape (#243), verified against `legendary`
//! (github.com/legendary-gl/legendary, MIT) — the actual reverse-engineered
//! EGS client Heroic itself shells out to, not Heroic's own wrapper.
//!
//! `CLIENT_ID`/`CLIENT_SECRET` below are the ones baked into the official
//! Epic Games Launcher itself ("UELauncher") — reverse-engineered years ago
//! and reused by every community EGS tool (legendary, Heroic, Rare). Not a
//! Tatu secret: it identifies "an Epic Games Launcher-compatible client" to
//! Epic's auth server, same category as GOG's own baked-in client id.
//!
//! Scope here stops at login + owned-games list; actually downloading a
//! game is EGS's own manifest+chunk protocol (#344), and cartridge install
//! is its own step after that (#345) — not attempted here.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";
pub(crate) const USER_AGENT: &str =
    "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit";

const OAUTH_HOST: &str = "account-public-service-prod03.ol.epicgames.com";
const LIBRARY_HOST: &str = "library-service.live.use1a.on.epicgames.com";
const CATALOG_HOST: &str = "catalog-public-service-prod06.ol.epicgames.com";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EgsTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: String,
}

/// One entry from the library-items list — just enough to ask the catalog
/// for the rest (`resolve_details`). EGS has no numeric id anywhere in this
/// chain, unlike Steam/GOG.
#[derive(Debug, Clone)]
pub struct EgsLibraryEntry {
    pub namespace: String,
    pub catalog_item_id: String,
    pub app_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgsOwnedGame {
    /// Not a real Epic id — EGS identifies apps by string
    /// (namespace + catalogItemId + appName), no numeric id exists anywhere
    /// in its API. Derived from `catalog_item_id` (see `derive_app_id`) so
    /// this fits the same u64 app-id space Steam/GOG already use throughout
    /// Tatu — `DetailSource` (window_cmd.rs) exists specifically because
    /// those spaces already don't share a namespace with each other.
    pub id: u64,
    pub namespace: String,
    pub catalog_item_id: String,
    pub app_name: String,
    pub title: String,
    /// Square-ish box art — used for the list row thumbnail, same role
    /// GOG's `icon_url` plays.
    #[serde(default)]
    pub icon_url: Option<String>,
    /// Wide store-front art — used for the detail window's header banner,
    /// same role GOG's `background_url` plays.
    #[serde(default)]
    pub background_url: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Single-element (or empty) — kept as a `Vec` rather than
    /// `Option<String>` so this reuses `render/game_list.js`'s shared
    /// `matchesQuery`/search helper unchanged, same as GOG's own
    /// `developers` field. EGS's catalog exposes no genre/publisher field
    /// at all (checked live against the documented schema) — there is
    /// nothing to put there, unlike GOG's separate catalog-search lookup.
    #[serde(default)]
    pub developers: Vec<String>,
}

/// URL to open in the system browser to start the login flow.
pub fn login_url() -> String {
    let redirect =
        format!("https://www.epicgames.com/id/api/redirect?clientId={CLIENT_ID}&responseType=code");
    format!(
        "https://www.epicgames.com/id/login?redirectUrl={}",
        urlencoding::encode(&redirect)
    )
}

/// Unlike GOG, Epic's own redirect target renders raw JSON
/// (`{"authorizationCode": "...", ...}`) in the browser instead of a
/// further redirect carrying a `code=` query param — confirmed against
/// `legendary`'s own CLI login flow (`cli.py`'s `auth` command, which asks
/// the user to paste exactly this). Accepts either that whole JSON blob or
/// a bare code either way.
pub fn extract_code(pasted: &str) -> Option<String> {
    let trimmed = pasted.trim();
    if trimmed.starts_with('{') {
        let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        return value
            .get("authorizationCode")
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    let bare = trimmed.trim_matches('"');
    (!bare.is_empty()).then(|| bare.to_string())
}

pub fn exchange_code(code: &str) -> Result<EgsTokens, String> {
    request_token(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("token_type", "eg1"),
    ])
}

pub fn refresh(tokens: &EgsTokens) -> Result<EgsTokens, String> {
    request_token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &tokens.refresh_token),
        ("token_type", "eg1"),
    ])
}

/// Epic's token endpoint authenticates the CLIENT via HTTP Basic (the
/// UELauncher credentials above), not a `client_id`/`client_secret` form
/// field like GOG's — confirmed against `legendary`'s
/// `EPCAPI.start_session`/`_oauth_basic`.
fn request_token(params: &[(&str, &str)]) -> Result<EgsTokens, String> {
    let auth =
        base64::engine::general_purpose::STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"));
    let url = format!("https://{OAUTH_HOST}/account/api/oauth/token");
    // `http_status_as_error(false)`: ureq's default turns a 4xx/5xx into an
    // `Err` that discards the response body — Epic's error responses are
    // themselves informative JSON (`errorMessage`, e.g. "the authorization
    // code you supplied was not found"), and losing that behind a bare
    // "http status: 400" is a real UX regression from what GOG's own
    // equivalent flow shows. Read the body ourselves regardless of status.
    let mut response = ureq::post(&url)
        .header("User-Agent", USER_AGENT)
        .header("Authorization", &format!("Basic {auth}"))
        .config()
        .http_status_as_error(false)
        .build()
        .send_form(params.iter().copied())
        .map_err(|e| format!("EGS token request failed: {e}"))?;
    let body: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("EGS token response parse failed: {e}"))?;

    if let Some(err) = body.get("errorMessage").and_then(|v| v.as_str()) {
        return Err(format!("EGS login falló: {err}"));
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("EGS token response missing access_token")?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or("EGS token response missing refresh_token")?
        .to_string();
    let account_id = body
        .get("account_id")
        .and_then(|v| v.as_str())
        .ok_or("EGS token response missing account_id")?
        .to_string();
    Ok(EgsTokens {
        access_token,
        refresh_token,
        account_id,
    })
}

/// The apps the account owns, paginated via `responseMetadata.nextCursor` —
/// same shape `legendary.core.py`'s own library sync loop uses.
///
/// This response mixes in every Unreal Engine/FAB marketplace asset the
/// account has ever claimed (content packs, code plugins, free monthly
/// giveaways) alongside real games — checked live against a real account
/// (2026-09-16): 640 of 667 records were non-game marketplace content, not
/// games, with titles like "Action RPG" or "Automotive Materials" polluting
/// the tab. `namespace == "ue"` alone (361 records) missed the newer FAB
/// listings entirely (279 more, `sandboxName: "fab-listing-live"`, a shared
/// `productId` across all of them) — both are Epic's own structural
/// sentinels for "this is marketplace content", not a heuristic on the
/// title text. Same "real installable game only" rule GOG's
/// `fetch_owned_ids` already applies to its own equivalent noise
/// (bundle-container ids, orphaned products).
const MARKETPLACE_PRODUCT_ID: &str = "prod-ue";
const FAB_SANDBOX_NAME: &str = "fab-listing-live";

pub fn fetch_owned_apps(access_token: &str) -> Result<Vec<EgsLibraryEntry>, String> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut url =
            format!("https://{LIBRARY_HOST}/library/api/public/items?includeMetadata=false");
        if let Some(c) = &cursor {
            url.push_str(&format!("&cursor={}", urlencoding::encode(c)));
        }
        let mut response = ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Authorization", &format!("Bearer {access_token}"))
            .call()
            .map_err(|e| format!("EGS library request failed: {e}"))?;
        let body: serde_json::Value = response
            .body_mut()
            .read_json()
            .map_err(|e| format!("EGS library response parse failed: {e}"))?;

        let records = body
            .get("records")
            .and_then(|v| v.as_array())
            .ok_or("EGS library response missing 'records'")?;
        for r in records {
            let product_id = r.get("productId").and_then(|v| v.as_str());
            let sandbox_name = r.get("sandboxName").and_then(|v| v.as_str());
            if product_id == Some(MARKETPLACE_PRODUCT_ID) || sandbox_name == Some(FAB_SANDBOX_NAME)
            {
                continue;
            }
            if let (Some(namespace), Some(app_name), Some(catalog_item_id)) = (
                r.get("namespace").and_then(|v| v.as_str()),
                r.get("appName").and_then(|v| v.as_str()),
                r.get("catalogItemId").and_then(|v| v.as_str()),
            ) {
                out.push(EgsLibraryEntry {
                    namespace: namespace.to_string(),
                    catalog_item_id: catalog_item_id.to_string(),
                    app_name: app_name.to_string(),
                });
            }
        }

        cursor = body
            .get("responseMetadata")
            .and_then(|m| m.get("nextCursor"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    Ok(out)
}

/// One id per catalog `bulk/items` call — matches GOG's own per-id
/// `fetch_details`, kept simple rather than batching several
/// `catalogItemId`s into one request (the endpoint supports it, but the
/// caller reports per-game progress the same way GOG's library sync does,
/// see `commands::egs_cmd::fetch_egs_library`).
pub fn resolve_details(access_token: &str, entry: &EgsLibraryEntry) -> EgsOwnedGame {
    let id = derive_app_id(&entry.catalog_item_id);
    let fallback = || EgsOwnedGame {
        id,
        namespace: entry.namespace.clone(),
        catalog_item_id: entry.catalog_item_id.clone(),
        app_name: entry.app_name.clone(),
        title: entry.app_name.clone(),
        icon_url: None,
        background_url: None,
        description: None,
        developers: Vec::new(),
    };

    let url = format!(
        "https://{CATALOG_HOST}/catalog/api/shared/namespace/{}/bulk/items?id={}&includeDLCDetails=false&includeMainGameDetails=true&country=US&locale=en-US",
        urlencoding::encode(&entry.namespace),
        urlencoding::encode(&entry.catalog_item_id),
    );
    let Some(body) = get_json_retrying(&url, access_token) else {
        return fallback();
    };
    let Some(item) = body.get(&entry.catalog_item_id) else {
        return fallback();
    };

    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| entry.app_name.clone());
    let images = item.get("keyImages").and_then(|v| v.as_array());
    // A single fixed type per slot (originally "Thumbnail"/"DieselStoreFrontWide")
    // returned nothing for most real games — checked live against a real
    // account (2026-09-16): "Thumbnail" and "DieselStoreFrontWide" are rare,
    // legacy-looking types (only present on titles with a bundled code-
    // redemption item); the two types every game in the sample actually
    // had were "DieselGameBoxTall" (portrait box art) and "DieselGameBox"
    // (wide promo art). Tried in priority order, first match wins.
    let image_url = |priority: &[&str]| -> Option<String> {
        let imgs = images?;
        priority.iter().find_map(|&wanted_type| {
            imgs.iter()
                .find(|img| img.get("type").and_then(|v| v.as_str()) == Some(wanted_type))
                .and_then(|img| img.get("url"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
    };
    let icon_url = image_url(&[
        "DieselGameBoxTall",
        "Thumbnail",
        "DieselGameBoxLogo",
        "DieselGameBox",
    ]);
    let background_url = image_url(&["DieselStoreFrontWide", "DieselGameBox", "DieselGameBoxTall"]);
    let description = item
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty());
    let developers = item
        .get("developer")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .into_iter()
        .collect();

    EgsOwnedGame {
        id,
        namespace: entry.namespace.clone(),
        catalog_item_id: entry.catalog_item_id.clone(),
        app_name: entry.app_name.clone(),
        title,
        icon_url,
        background_url,
        description,
        developers,
    }
}

/// Retries a GET+JSON-parse up to 3 times before giving up — same
/// transient-network-blip protection `gog_account::get_json_retrying` uses
/// for the exact same reason (a bulk library sync is dozens of sequential
/// requests; one silent failure among them shouldn't downgrade a real game
/// to its raw id as the title until the next full re-sync).
fn get_json_retrying(url: &str, access_token: &str) -> Option<serde_json::Value> {
    const ATTEMPTS: u32 = 3;
    for attempt in 1..=ATTEMPTS {
        let body = ureq::get(url)
            .header("User-Agent", USER_AGENT)
            .header("Authorization", &format!("Bearer {access_token}"))
            .call()
            .ok()
            .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok());
        if body.is_some() {
            return body;
        }
        if attempt < ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    }
    None
}

/// SHA-256 over the (stable, Epic-assigned) `catalogItemId` string,
/// truncated to its first 8 bytes as a big-endian `u64`, then masked to 52
/// bits. Deterministic across runs/platforms — this value gets persisted
/// (`state.json`, and eventually a cartridge marker), so it can never
/// depend on Rust's own `DefaultHasher` (explicitly not guaranteed stable
/// across compiler versions).
///
/// The 52-bit mask is required, not cosmetic: this id round-trips through
/// the frontend as a plain JS `Number` (an HTML `data-id` attribute parsed
/// back with `parseInt`, and Tauri's own IPC, which is JSON) — JS doubles
/// only represent integers exactly up to 2^53-1. A full 64-bit hash would
/// silently corrupt roughly half of all derived ids the moment they left
/// Rust. 2^52 distinct values is still astronomically more than any
/// personal EGS library will ever hold.
fn derive_app_id(catalog_item_id: &str) -> u64 {
    let digest = Sha256::digest(catalog_item_id.as_bytes());
    let full = u64::from_be_bytes(digest[..8].try_into().expect("sha256 digest is >= 8 bytes"));
    full & 0x000F_FFFF_FFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_app_id_is_deterministic() {
        assert_eq!(derive_app_id("abc123def456"), derive_app_id("abc123def456"));
    }

    #[test]
    fn derive_app_id_differs_for_different_ids() {
        assert_ne!(derive_app_id("game-one"), derive_app_id("game-two"));
    }

    #[test]
    fn extracts_code_from_a_json_blob() {
        let pasted = r#"{"authorizationCode":"abc123","expiresInSeconds":600}"#;
        assert_eq!(extract_code(pasted), Some("abc123".to_string()));
    }

    #[test]
    fn accepts_a_bare_code_with_no_json_around_it() {
        assert_eq!(extract_code("abc123"), Some("abc123".to_string()));
    }

    #[test]
    fn accepts_a_quoted_bare_code() {
        assert_eq!(extract_code("\"abc123\""), Some("abc123".to_string()));
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
    fn rejects_malformed_json() {
        assert_eq!(extract_code("{not json"), None);
    }

    #[test]
    fn rejects_json_with_no_authorization_code_field() {
        assert_eq!(extract_code(r#"{"foo":"bar"}"#), None);
    }
}
