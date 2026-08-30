//! GOG content-system v2 downloader (#243 follow-up) — actually
//! downloading/installing an owned GOG game, the piece deliberately left
//! out of #263. Reverse-engineered, no official GOG documentation: every
//! struct/URL shape below was verified live against a real owned game
//! (Alone in the Dark 1, id 1207660923) before being written, not copied
//! from a doc — `builds` → `repository` → `depot manifest` →
//! `secure_link` → chunk bytes, each step's real JSON/bytes inspected by
//! hand first. Cross-checked against Heroic-Games-Launcher/heroic-gogdl's
//! actual source (the real client this protocol serves) for anything the
//! live probe alone couldn't confirm (e.g. build selection order).

use std::collections::HashMap;
use std::io::Read;

use flate2::read::ZlibDecoder;
use md5::{Digest, Md5};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::gog_account::USER_AGENT;

const CONTENT_SYSTEM: &str = "https://content-system.gog.com";
const CDN_META: &str = "https://gog-cdn-fastly.gog.com/content-system/v2/meta";

#[derive(Debug, Deserialize)]
struct BuildsResponse {
    items: Vec<Build>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub product_id: String,
    pub os: String,
    pub branch: Option<String>,
    pub version_name: String,
    pub generation: u32,
    pub link: String,
}

#[derive(Debug, Deserialize)]
pub struct Repository {
    #[serde(rename = "installDirectory")]
    pub install_directory: String,
    pub depots: Vec<Depot>,
    /// Present on old (generation 1) DOS-era games — e.g. `["DOSBox074_2CS"]`
    /// for Alone in the Dark. Not resolved by this module: a dependency is
    /// itself a whole separate GOG product to download, out of scope for
    /// the first working version of this downloader.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Depot {
    pub manifest: String,
    pub size: u64,
    #[serde(rename = "compressedSize")]
    pub compressed_size: u64,
    pub languages: Vec<String>,
    #[serde(rename = "productId")]
    pub product_id: String,
    /// GOG's own small metadata depots (install scripts, `goggame-*.info`)
    /// carry this flag — real game content depots don't.
    #[serde(default, rename = "isGogDepot")]
    pub is_gog_depot: bool,
}

#[derive(Debug, Deserialize)]
struct DepotManifestWrapper {
    depot: DepotManifest,
}

#[derive(Debug, Deserialize)]
pub struct DepotManifest {
    pub items: Vec<DepotItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DepotItem {
    pub path: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub chunks: Vec<Chunk>,
}

impl DepotItem {
    /// `type` is also `"DepotLink"`/`"DepotDirectory"` for non-file
    /// entries — those carry no `chunks` and nothing to download.
    pub fn is_file(&self) -> bool {
        self.item_type == "DepotFile"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Chunk {
    #[serde(rename = "compressedMd5")]
    pub compressed_md5: String,
    #[serde(rename = "compressedSize")]
    pub compressed_size: u64,
    pub md5: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
struct SecureLinkResponse {
    urls: Vec<CdnEndpoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CdnEndpoint {
    pub endpoint_name: String,
    pub url_format: String,
    pub parameters: HashMap<String, serde_json::Value>,
    /// GOG's own priority hint for which mirrors to try last — trusted
    /// over the numeric `priority` field, which does NOT sort the way its
    /// name suggests (confirmed live: the real primary endpoint carried a
    /// higher `priority` number than the one explicitly flagged
    /// fallback-only).
    #[serde(default)]
    pub fallback_only: bool,
}

/// `os` is `"windows"` or `"osx"` — GOG products only, no native Linux
/// builds distributed through this endpoint (matches this project's own
/// Proton-first cartridge story for Windows-only Steam depots).
pub fn fetch_builds(access_token: &str, product_id: u64, os: &str) -> Result<Vec<Build>, String> {
    let url = format!("{CONTENT_SYSTEM}/products/{product_id}/os/{os}/builds?generation=2");
    let mut response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("GOG builds request failed: {e}"))?;
    let body: BuildsResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("GOG builds response parse failed: {e}"))?;
    Ok(body.items)
}

/// heroic-gogdl picks the first build with no branch set (i.e. the public
/// release, not a beta/private branch) — `items` is already newest-first
/// per the real API response, so this is also "latest public build".
pub fn pick_build(builds: &[Build]) -> Option<&Build> {
    builds
        .iter()
        .find(|b| b.branch.is_none())
        .or_else(|| builds.first())
}

pub fn fetch_repository(access_token: &str, build: &Build) -> Result<Repository, String> {
    fetch_zlib_json(access_token, &build.link)
}

pub fn fetch_depot_manifest(access_token: &str, depot: &Depot) -> Result<DepotManifest, String> {
    let url = format!("{CDN_META}/{}", galaxy_path(&depot.manifest));
    let wrapper: DepotManifestWrapper = fetch_zlib_json(access_token, &url)?;
    Ok(wrapper.depot)
}

/// GET a zlib-compressed (standard zlib framing, header+deflate+adler32 —
/// NOT raw deflate) JSON response and decompress+parse it. Every
/// content-system v2 response this module reads (`build.link`, depot
/// manifests) is shaped this way.
fn fetch_zlib_json<T: DeserializeOwned>(access_token: &str, url: &str) -> Result<T, String> {
    let mut response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("GOG request to {url} failed: {e}"))?;
    let compressed = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("GOG response read failed ({url}): {e}"))?;
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut json_bytes = Vec::new();
    decoder
        .read_to_end(&mut json_bytes)
        .map_err(|e| format!("zlib decompress failed ({url}): {e}"))?;
    serde_json::from_slice(&json_bytes)
        .map_err(|e| format!("GOG response JSON parse failed ({url}): {e}"))
}

/// `manifest`/chunk-md5 hashes are addressed on GOG's CDN git-object-style:
/// `abcdef1234...` → `ab/cd/abcdef1234...`.
fn galaxy_path(hash: &str) -> String {
    if hash.contains('/') {
        return hash.to_string();
    }
    format!("{}/{}/{hash}", &hash[0..2], &hash[2..4])
}

/// Resolves the signed CDN mirror list for `product_id`, sorted with real
/// (non-fallback) endpoints first. `path` is `"/"` for a fresh install of
/// the base depot — patch/delta downloads (not implemented here) use a
/// different `root` value, per heroic-gogdl.
pub fn fetch_secure_link(access_token: &str, product_id: u64) -> Result<Vec<CdnEndpoint>, String> {
    let url = format!(
        "{CONTENT_SYSTEM}/products/{product_id}/secure_link?_version=2&generation=2&path=/"
    );
    let mut response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("GOG secure_link request failed: {e}"))?;
    let body: SecureLinkResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("GOG secure_link response parse failed: {e}"))?;
    let mut endpoints = body.urls;
    endpoints.sort_by_key(|e| e.fallback_only);
    Ok(endpoints)
}

/// Substitutes `{key}` placeholders in `endpoint.url_format` from its own
/// `parameters` map, appending this chunk's galaxy-sharded path onto
/// whichever parameter is named `path` — confirmed live: every real
/// endpoint (gcore, fastly) uses `path` for this, both put the object key
/// as a `{path}`-interpolated URL segment rather than a query parameter.
fn build_chunk_url(endpoint: &CdnEndpoint, compressed_md5: &str) -> String {
    let mut params = endpoint.parameters.clone();
    let base_path = params
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    params.insert(
        "path".to_string(),
        serde_json::Value::String(format!("{base_path}/{}", galaxy_path(compressed_md5))),
    );

    let mut url = endpoint.url_format.clone();
    for (key, value) in &params {
        let value_str = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        url = url.replace(&format!("{{{key}}}"), &value_str);
    }
    url
}

/// Downloads one chunk, verifying it twice — once against `compressedMd5`
/// (the bytes actually on the wire, catches CDN corruption before wasting
/// time decompressing garbage) and once against `md5` after decompression
/// (catches decompression itself going wrong). Tries every endpoint in
/// order, falling through on any failure (network or checksum) rather
/// than giving up on the first mirror.
pub fn download_chunk(endpoints: &[CdnEndpoint], chunk: &Chunk) -> Result<Vec<u8>, String> {
    let mut last_err = "no CDN endpoints available".to_string();
    for endpoint in endpoints {
        let url = build_chunk_url(endpoint, &chunk.compressed_md5);
        let compressed = match ureq::get(&url).call() {
            Ok(mut response) => match response.body_mut().read_to_vec() {
                Ok(bytes) => bytes,
                Err(e) => {
                    last_err = format!("{}: read failed: {e}", endpoint.endpoint_name);
                    continue;
                }
            },
            Err(e) => {
                last_err = format!("{}: request failed: {e}", endpoint.endpoint_name);
                continue;
            }
        };

        if compressed.len() as u64 != chunk.compressed_size
            || hex_md5(&compressed) != chunk.compressed_md5
        {
            last_err = format!(
                "{}: compressed chunk failed verification",
                endpoint.endpoint_name
            );
            continue;
        }

        let mut decoder = ZlibDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        if decoder.read_to_end(&mut decompressed).is_err() {
            last_err = format!("{}: chunk decompression failed", endpoint.endpoint_name);
            continue;
        }
        if decompressed.len() as u64 != chunk.size || hex_md5(&decompressed) != chunk.md5 {
            last_err = format!(
                "{}: decompressed chunk failed verification",
                endpoint.endpoint_name
            );
            continue;
        }

        return Ok(decompressed);
    }
    Err(format!("all CDN endpoints failed: {last_err}"))
}

fn hex_md5(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn galaxy_path_shards_a_bare_hash() {
        assert_eq!(
            galaxy_path("4e57ae9ecb59d88b1f51be0a89a3cfaa"),
            "4e/57/4e57ae9ecb59d88b1f51be0a89a3cfaa"
        );
    }

    #[test]
    fn galaxy_path_leaves_an_already_sharded_path_alone() {
        assert_eq!(
            galaxy_path("4e/57/4e57ae9ecb59d88b1f51be0a89a3cfaa"),
            "4e/57/4e57ae9ecb59d88b1f51be0a89a3cfaa"
        );
    }

    /// Full pipeline against a real owned game (Alone in the Dark 1, id
    /// 1207660923), reusing Tatu's own stored GOG session from
    /// `state.json` — same file `AppState::path()` resolves, read
    /// directly here rather than duplicating a whole app-state loader for
    /// one test. `#[ignore]` because it depends on a real, connected GOG
    /// account and live network access. Deliberately targets one of the
    /// game's tiny `isGogDepot` metadata depots (a `goggame-*.info` file,
    /// single chunk, ~1.5KB) rather than the ~377MB main depot — this
    /// verifies the whole chain end-to-end without downloading a real
    /// game's worth of data every test run.
    #[test]
    #[ignore]
    fn downloads_and_verifies_a_real_metadata_chunk() {
        let home = std::env::var("HOME").expect("HOME not set");
        let state_path = format!("{home}/.config/backlog-tracker/state.json");
        let state_json = std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("cannot read {state_path}: {e}"));
        let state: serde_json::Value =
            serde_json::from_str(&state_json).expect("state.json is not valid JSON");
        let access_token = state["gog_tokens"]["access_token"]
            .as_str()
            .expect(
                "no gog_tokens.access_token in state.json — connect a GOG account in Tatu first",
            )
            .to_string();

        const PRODUCT_ID: u64 = 1207660923; // Alone in the Dark 1

        let builds =
            fetch_builds(&access_token, PRODUCT_ID, "windows").expect("fetch_builds failed");
        let build = pick_build(&builds).expect("no build returned");
        let repo = fetch_repository(&access_token, build).expect("fetch_repository failed");

        let metadata_depot = repo
            .depots
            .iter()
            .find(|d| d.is_gog_depot)
            .expect("no isGogDepot metadata depot found on this build");
        let manifest = fetch_depot_manifest(&access_token, metadata_depot)
            .expect("fetch_depot_manifest failed");
        let file = manifest
            .items
            .iter()
            .find(|i| i.is_file() && !i.chunks.is_empty())
            .expect("no file with chunks in this depot's manifest");

        let endpoints =
            fetch_secure_link(&access_token, PRODUCT_ID).expect("fetch_secure_link failed");
        assert!(
            !endpoints.is_empty(),
            "secure_link returned no CDN endpoints"
        );

        let chunk = &file.chunks[0];
        let bytes = download_chunk(&endpoints, chunk)
            .expect("download_chunk failed verification on every endpoint");
        assert_eq!(bytes.len() as u64, chunk.size);
        assert_eq!(hex_md5(&bytes), chunk.md5);
    }
}
