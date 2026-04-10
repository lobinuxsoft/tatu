use std::sync::Mutex;

use regex::Regex;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://howlongtobeat.com";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
/// Auth token TTL: 10 minutes.
const AUTH_TTL_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HltbResult {
    pub game_id: u64,
    pub game_name: String,
    /// Main story duration in hours.
    pub main_hours: f64,
    /// Main + extras duration in hours.
    pub extra_hours: f64,
    /// Completionist duration in hours.
    pub completionist_hours: f64,
    /// Review score (0-100).
    pub review_score: u32,
}

struct AuthInfo {
    endpoint: String,
    token: String,
    key: String,
    value: String,
    fetched_at: u64,
}

static AUTH_CACHE: Mutex<Option<AuthInfo>> = Mutex::new(None);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Discover the current search endpoint by parsing HLTB's JS bundles.
fn discover_endpoint() -> Result<String, String> {
    let html = ureq::get(BASE_URL)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("HLTB homepage fetch failed: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("HLTB read error: {e}"))?;

    let script_re = Regex::new(r#"src="(/_next/static/chunks/[^"]+\.js)""#).unwrap();
    let fetch_re =
        Regex::new(r#"fetch\s*\(\s*["']/api/([a-zA-Z0-9_]+)[^"']*["']\s*,\s*\{[^}]*method:\s*["']POST"#)
            .unwrap();

    for cap in script_re.captures_iter(&html) {
        let script_path = &cap[1];
        let url = format!("{}{}", BASE_URL, script_path);
        let Ok(resp) = ureq::get(&url).header("User-Agent", USER_AGENT).call() else {
            continue;
        };
        let Ok(js) = resp.into_body().read_to_string() else {
            continue;
        };
        if let Some(m) = fetch_re.captures(&js) {
            return Ok(format!("/api/{}", &m[1]));
        }
    }

    Ok("/api/find".to_string())
}

/// Fetch the auth token from the init endpoint.
fn fetch_auth_fresh(endpoint: &str) -> Result<AuthInfo, String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let init_url = format!("{}{}/init?t={}", BASE_URL, endpoint, ts);
    let json: serde_json::Value = ureq::get(&init_url)
        .header("User-Agent", USER_AGENT)
        .header("Referer", BASE_URL)
        .call()
        .map_err(|e| format!("HLTB init failed: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("HLTB init parse error: {e}"))?;

    let token = json["token"].as_str().unwrap_or("").to_string();
    let mut key = String::new();
    let mut value = String::new();
    if let Some(obj) = json.as_object() {
        for (k, v) in obj {
            let lower = k.to_lowercase();
            if lower.contains("key") {
                key = v.as_str().unwrap_or("").to_string();
            } else if lower.contains("val") {
                value = v.as_str().unwrap_or("").to_string();
            }
        }
    }

    Ok(AuthInfo {
        endpoint: endpoint.to_string(),
        token,
        key,
        value,
        fetched_at: now_secs(),
    })
}

/// Get cached auth or fetch a fresh one.
fn get_auth() -> Result<(String, String, String, String, String), String> {
    {
        let cache = AUTH_CACHE.lock().unwrap();
        if let Some(ref auth) = *cache {
            if now_secs() - auth.fetched_at < AUTH_TTL_SECS {
                return Ok((
                    auth.endpoint.clone(),
                    auth.token.clone(),
                    auth.key.clone(),
                    auth.value.clone(),
                    format!("{}{}", BASE_URL, auth.endpoint),
                ));
            }
        }
    }

    let endpoint = discover_endpoint()?;
    let auth = fetch_auth_fresh(&endpoint)?;
    let result = (
        auth.endpoint.clone(),
        auth.token.clone(),
        auth.key.clone(),
        auth.value.clone(),
        format!("{}{}", BASE_URL, auth.endpoint),
    );

    let mut cache = AUTH_CACHE.lock().unwrap();
    *cache = Some(auth);

    Ok(result)
}

/// Search HowLongToBeat for a game by name.
pub fn search(game_name: &str) -> Result<Vec<HltbResult>, String> {
    let (_endpoint, token, key, value, search_url) = get_auth()?;
    let terms: Vec<&str> = game_name.split_whitespace().collect();

    let mut payload = serde_json::json!({
        "searchType": "games",
        "searchTerms": terms,
        "searchPage": 1,
        "size": 5,
        "searchOptions": {
            "games": {
                "userId": 0,
                "platform": "",
                "sortCategory": "popular",
                "rangeCategory": "main",
                "rangeTime": { "min": 0, "max": 0 },
                "gameplay": { "perspective": "", "flow": "", "genre": "", "difficulty": "" },
                "rangeYear": { "min": "", "max": "" },
                "modifier": ""
            },
            "users": { "sortCategory": "postcount" },
            "lists": { "sortCategory": "follows" },
            "filter": "",
            "sort": 0,
            "randomizer": 0
        },
        "useCache": true
    });

    if !key.is_empty() {
        payload[&key] = serde_json::Value::String(value.clone());
    }

    let resp: serde_json::Value = ureq::post(&search_url)
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .header("Referer", BASE_URL)
        .header("Origin", BASE_URL)
        .header("x-auth-token", &token)
        .header("x-hp-key", &key)
        .header("x-hp-val", &value)
        .send_json(&payload)
        .map_err(|e| format!("HLTB search failed: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("HLTB search parse error: {e}"))?;

    let data = resp["data"]
        .as_array()
        .ok_or("No data in HLTB response")?;

    let results: Vec<HltbResult> = data
        .iter()
        .filter_map(|g| {
            let game_id = g["game_id"].as_u64()?;
            let game_name = g["game_name"].as_str()?.to_string();
            let to_hours = |v: &serde_json::Value| {
                let secs = v.as_f64().unwrap_or(0.0);
                (secs / 3600.0 * 10.0).round() / 10.0
            };
            Some(HltbResult {
                game_id,
                game_name,
                main_hours: to_hours(&g["comp_main"]),
                extra_hours: to_hours(&g["comp_plus"]),
                completionist_hours: to_hours(&g["comp_100"]),
                review_score: g["review_score"].as_u64().unwrap_or(0) as u32,
            })
        })
        .collect();

    Ok(results)
}
