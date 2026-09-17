//! Epic Games Store manifest + chunked download protocol (#344). Reverse-
//! engineered, no official documentation — every byte offset below was
//! verified against a real manifest and a real chunk downloaded from a
//! real owned game (Enter the Gungeon, catalogItemId
//! `25b771db01554b1bb40c5617b663d17e`) before being written, same
//! live-probe-first discipline `gog_download` used for GOG's protocol.
//! Cross-checked against `legendary`'s actual binary reader
//! (`legendary/models/manifest.py`) for anything the live probe alone
//! couldn't confirm (chunk path formula, chunk-dir version thresholds).
//!
//! A published crate (`epic_manifest_parser_rs` on crates.io) parses this
//! same format, but it's missing the `secret_guid`/`window_size_compressed`/
//! `encryption_tag` fields manifests version 22+ add to each chunk entry —
//! without those a chunk path can't be built for any game on the newer
//! format at all. Studied its source for the byte layout (which matched
//! `legendary` exactly for everything it does cover) rather than adopting
//! it outright, per the project's "study source, don't adopt blindly when
//! it only covers part of the case" rule.
//!
//! Scope here stops at fetch manifest → download chunks → reassemble files
//! into `dest_root`. Wired into a Tauri command by `commands::egs_cmd`
//! (#345).

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};

use crate::egs_account::{USER_AGENT, agent};

const LAUNCHER_HOST: &str = "launcher-public-service-prod06.ol.epicgames.com";

const MANIFEST_MAGIC: u32 = 0x44BEC00C;
const CHUNK_MAGIC: u32 = 0xB1FE3AA2;
/// Manifest feature-level threshold where chunk entries grow the extra
/// `secret_guid`/`window_size_compressed`/`encryption_tag` fields, and chunk
/// paths switch from the plain hex scheme to the base64+secret-guid one —
/// confirmed against `legendary`'s `ChunkDataList.read`/`ChunkInfo.path`
/// (`manifest_version >= 22` is the exact condition both use).
const SECRET_GUID_FEATURE_LEVEL: i32 = 22;

// ---------------------------------------------------------------------
// Byte reader — UE4's own binary serialization primitives. Deliberately
// separate from `epic_manifest_parser_rs`'s `ByteReader`: this one never
// panics (`dbg!`/`.unwrap()` in that crate would crash the whole app on a
// single malformed chunk) and returns `Result` throughout.
// ---------------------------------------------------------------------

struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn tell(&self) -> usize {
        self.pos
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err(format!(
                "manifest read overflow: wanted {n} bytes at {}, only {} left",
                self.pos,
                self.remaining()
            ));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    fn guid(&mut self) -> Result<[u32; 4], String> {
        Ok([self.u32()?, self.u32()?, self.u32()?, self.u32()?])
    }

    fn sha1(&mut self) -> Result<[u8; 20], String> {
        Ok(self.bytes(20)?.try_into().unwrap())
    }

    /// UE4's `FString` wire format: an `i32` length prefix. Positive means
    /// UTF-8, `length` bytes including a trailing NUL. Negative means
    /// UTF-16LE, `-length` code units (`-length * 2` bytes), also
    /// NUL-terminated. Zero means an empty string with no body at all.
    fn fstring(&mut self) -> Result<String, String> {
        let length = self.i32()?;
        if length == 0 {
            return Ok(String::new());
        }
        if length > 0 {
            let raw = self.bytes(length as usize)?;
            let without_nul = raw.strip_suffix(&[0]).unwrap_or(raw);
            String::from_utf8(without_nul.to_vec())
                .map_err(|e| format!("manifest FString is not valid UTF-8: {e}"))
        } else {
            let units = (-length) as usize;
            let raw = self.bytes(units * 2)?;
            // `chunks_exact` over `as_chunks`: the latter is still nightly-only
            // (`slice_as_chunks`) on the stable toolchains this project builds
            // with — a newer clippy than this repo's pinned toolchain suggests
            // it anyway, so this is allowed rather than adopted. `unknown_lints`
            // is allowed alongside it: an older clippy (that predates this lint
            // existing at all) would otherwise error on the `allow` itself
            // under this project's `-D warnings` — confirmed live, CI's clippy
            // and this machine's local one disagreed on whether the lint name
            // is even valid.
            #[allow(unknown_lints)]
            #[allow(clippy::chunks_exact_to_as_chunks)]
            let code_units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let without_nul = code_units.strip_suffix(&[0]).unwrap_or(&code_units);
            Ok(String::from_utf16_lossy(without_nul))
        }
    }

    fn fstring_array(&mut self) -> Result<Vec<String>, String> {
        let count = self.u32()?;
        (0..count).map(|_| self.fstring()).collect()
    }
}

// ---------------------------------------------------------------------
// Manifest model
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub guid: [u32; 4],
    pub hash: u64,
    pub sha1: [u8; 20],
    pub group_num: u8,
    /// Only set for feature level >= 22 — see `SECRET_GUID_FEATURE_LEVEL`.
    /// `None`/all-zero means the older, plain hex chunk path applies.
    pub secret_guid: Option<[u32; 4]>,
}

#[derive(Debug, Clone)]
pub struct ChunkPart {
    pub guid: [u32; 4],
    /// Byte offset *within that chunk's decompressed payload* this part starts at.
    pub offset: u32,
    pub size: u32,
    /// Byte offset in the reassembled output file this part lands at. Not
    /// read anywhere yet — `download_game` writes parts sequentially in
    /// manifest order instead of seeking (same "concatenating in order IS
    /// reassembly" reasoning `gog_download` uses), which happens to make
    /// this redundant for now. Kept on the model because it's real,
    /// verified wire data, not derived — a future out-of-order/resumable
    /// downloader would need it.
    #[allow(dead_code)]
    pub file_offset: u64,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub filename: String,
    pub flags: u8,
    pub chunk_parts: Vec<ChunkPart>,
}

impl FileEntry {
    pub fn executable(&self) -> bool {
        self.flags & 0x04 != 0
    }

    pub fn size(&self) -> u64 {
        self.chunk_parts.iter().map(|p| p.size as u64).sum()
    }
}

pub struct Manifest {
    pub feature_level: i32,
    /// The manifest's own internal codename (e.g. "Garlic"), not the
    /// display title — callers already have the real title from
    /// `EgsOwnedGame`, so nothing reads this back off the parsed manifest.
    /// Kept on the model since it's real parsed data, not derived.
    #[allow(dead_code)]
    pub app_name: String,
    pub build_version: String,
    pub launch_exe: String,
    pub chunks: Vec<ChunkInfo>,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    fn chunk_by_guid(&self, guid: &[u32; 4]) -> Option<&ChunkInfo> {
        self.chunks.iter().find(|c| &c.guid == guid)
    }
}

/// Parses a manifest as downloaded from the CDN (still header-wrapped,
/// possibly zlib-compressed) into a `Manifest`. Verifies the decompressed
/// body's SHA1 against the header's own hash before trusting any of it —
/// the same integrity check `legendary` and the official launcher both do.
pub fn parse_manifest(raw: &[u8]) -> Result<Manifest, String> {
    let mut header_reader = ByteReader::new(raw);
    let magic = header_reader.u32()?;
    if magic != MANIFEST_MAGIC {
        return Err(format!("not an EGS manifest (magic {magic:#x})"));
    }
    let header_size = header_reader.u32()?;
    let data_size_uncompressed = header_reader.u32()?;
    let _data_size_compressed = header_reader.u32()?;
    let header_sha1 = header_reader.sha1()?;
    let stored_as = header_reader.u8()?;
    let feature_level = header_reader.i32()?;
    if header_reader.tell() != header_size as usize {
        return Err(format!(
            "manifest header size mismatch: declared {header_size}, actually read {}",
            header_reader.tell()
        ));
    }

    let body_raw = &raw[header_size as usize..];
    let body = if stored_as & 0x01 != 0 {
        let mut decoder = ZlibDecoder::new(body_raw);
        let mut buf = Vec::with_capacity(data_size_uncompressed as usize);
        decoder
            .read_to_end(&mut buf)
            .map_err(|e| format!("manifest zlib decompress failed: {e}"))?;
        buf
    } else {
        body_raw.to_vec()
    };
    if body.len() as u32 != data_size_uncompressed {
        return Err(format!(
            "manifest body size mismatch: expected {data_size_uncompressed}, got {}",
            body.len()
        ));
    }
    let mut hasher = Sha1::new();
    hasher.update(&body);
    let actual_sha1: [u8; 20] = hasher.finalize().into();
    if actual_sha1 != header_sha1 {
        return Err("manifest body failed SHA1 verification".to_string());
    }

    let mut r = ByteReader::new(&body);
    let meta = parse_meta(&mut r)?;
    let chunks = parse_chunk_list(&mut r, feature_level)?;
    let files = parse_file_list(&mut r)?;
    // Custom fields section follows but nothing here needs it — not parsed.

    Ok(Manifest {
        feature_level,
        app_name: meta.app_name,
        build_version: meta.build_version,
        launch_exe: meta.launch_exe,
        chunks,
        files,
    })
}

struct ManifestMeta {
    app_name: String,
    build_version: String,
    launch_exe: String,
}

/// Every field is read in order regardless of whether Tatu needs it —
/// `FString`s aren't fixed-size, so skipping one without decoding it would
/// mean not knowing where it ends.
fn parse_meta(r: &mut ByteReader) -> Result<ManifestMeta, String> {
    let start = r.tell();
    let meta_size = r.u32()?;
    let data_version = r.u8()?;
    let _feature_level = r.i32()?;
    let _is_file_data = r.u8()?;
    let _app_id = r.u32()?;
    let app_name = r.fstring()?;
    let build_version = r.fstring()?;
    let launch_exe = r.fstring()?;
    let _launch_command = r.fstring()?;
    let _prereq_ids = r.fstring_array()?;
    let _prereq_name = r.fstring()?;
    let _prereq_path = r.fstring()?;
    let _prereq_args = r.fstring()?;
    if data_version >= 1 {
        let _build_id = r.fstring()?;
    }
    if data_version >= 2 {
        let _uninstall_action_path = r.fstring()?;
        let _uninstall_action_args = r.fstring()?;
    }
    if r.tell() - start != meta_size as usize {
        return Err(format!(
            "manifest meta size mismatch: declared {meta_size}, actually read {}",
            r.tell() - start
        ));
    }
    Ok(ManifestMeta {
        app_name,
        build_version,
        launch_exe,
    })
}

fn parse_chunk_list(r: &mut ByteReader, feature_level: i32) -> Result<Vec<ChunkInfo>, String> {
    let start = r.tell();
    let size = r.u32()?;
    let _version = r.u8()?;
    let count = r.u32()? as usize;

    let mut guids = Vec::with_capacity(count);
    for _ in 0..count {
        guids.push(r.guid()?);
    }
    let mut hashes = Vec::with_capacity(count);
    for _ in 0..count {
        hashes.push(r.u64()?);
    }
    let mut sha1s = Vec::with_capacity(count);
    for _ in 0..count {
        sha1s.push(r.sha1()?);
    }
    let mut group_nums = Vec::with_capacity(count);
    for _ in 0..count {
        group_nums.push(r.u8()?);
    }
    for _ in 0..count {
        r.u32()?; // window_size (uncompressed size) — not needed for download
    }
    for _ in 0..count {
        r.i64()?; // file_size (compressed size) — not needed for download
    }

    let mut secret_guids: Vec<Option<[u32; 4]>> = vec![None; count];
    if feature_level >= SECRET_GUID_FEATURE_LEVEL {
        for slot in secret_guids.iter_mut() {
            *slot = Some(r.guid()?);
        }
        for _ in 0..count {
            r.u32()?; // window_size_compressed — not needed for download
        }
        for _ in 0..count {
            r.bytes(16)?; // encryption_tag — encrypted chunks unsupported, see download_chunk
        }
    }

    if r.tell() - start != size as usize {
        return Err(format!(
            "chunk list size mismatch: declared {size}, actually read {}",
            r.tell() - start
        ));
    }

    Ok((0..count)
        .map(|i| ChunkInfo {
            guid: guids[i],
            hash: hashes[i],
            sha1: sha1s[i],
            group_num: group_nums[i],
            secret_guid: secret_guids[i].filter(|g| *g != [0, 0, 0, 0]),
        })
        .collect())
}

fn parse_file_list(r: &mut ByteReader) -> Result<Vec<FileEntry>, String> {
    let start = r.tell();
    let size = r.u32()?;
    let version = r.u8()?;
    let count = r.u32()? as usize;

    let mut filenames = Vec::with_capacity(count);
    for _ in 0..count {
        filenames.push(r.fstring()?);
    }
    for _ in 0..count {
        r.fstring()?; // symlink_target — Tatu targets Windows-via-Proton games only, never used
    }
    for _ in 0..count {
        r.sha1()?; // whole-file SHA1 — not verified here, per-chunk SHA1 already covers integrity
    }
    let mut flags = Vec::with_capacity(count);
    for _ in 0..count {
        flags.push(r.u8()?);
    }
    for _ in 0..count {
        r.fstring_array()?; // install_tags — not used, Tatu always installs everything
    }

    let mut chunk_parts_per_file = Vec::with_capacity(count);
    for _ in 0..count {
        let part_count = r.u32()?;
        let mut parts = Vec::with_capacity(part_count as usize);
        let mut file_offset = 0u64;
        for _ in 0..part_count {
            let part_start = r.tell();
            let struct_size = r.u32()?;
            let guid = r.guid()?;
            let offset = r.u32()?;
            let part_size = r.u32()?;
            if r.tell() - part_start != struct_size as usize {
                return Err(format!(
                    "chunk part size mismatch: declared {struct_size}, actually read {}",
                    r.tell() - part_start
                ));
            }
            parts.push(ChunkPart {
                guid,
                offset,
                size: part_size,
                file_offset,
            });
            file_offset += part_size as u64;
        }
        chunk_parts_per_file.push(parts);
    }

    if version >= 1 {
        for _ in 0..count {
            let has_md5 = r.u32()?;
            if has_md5 != 0 {
                r.bytes(16)?;
            }
        }
        for _ in 0..count {
            r.fstring()?; // mime_type — not needed for download
        }
    }
    if version >= 2 {
        for _ in 0..count {
            r.bytes(32)?; // sha256 — not verified, per-chunk SHA1 already covers integrity
        }
    }

    if r.tell() - start != size as usize {
        return Err(format!(
            "file list size mismatch: declared {size}, actually read {}",
            r.tell() - start
        ));
    }

    Ok(filenames
        .into_iter()
        .zip(flags)
        .zip(chunk_parts_per_file)
        .map(|((filename, flags), chunk_parts)| FileEntry {
            filename,
            flags,
            chunk_parts,
        })
        .collect())
}

// ---------------------------------------------------------------------
// Fetching — manifest metadata, manifest bytes, chunk bytes
// ---------------------------------------------------------------------

pub struct ManifestMirror {
    /// Full manifest URL including its auth query string.
    pub manifest_url: String,
    /// Everything up to (not including) the manifest filename — chunk
    /// paths are resolved relative to this, same mirror, same auth query
    /// string reused verbatim (confirmed live: a different mirror's token
    /// 403s, the manifest's own mirror's token works unmodified for
    /// sibling chunk paths under it).
    pub base_url: String,
    pub query_string: String,
}

/// Resolves the CDN mirror list for `namespace`/`catalog_item_id`/`app_name`
/// — the same endpoint `legendary`'s `get_game_manifest` calls.
pub fn fetch_manifest_mirrors(
    access_token: &str,
    namespace: &str,
    catalog_item_id: &str,
    app_name: &str,
) -> Result<Vec<ManifestMirror>, String> {
    let url = format!(
        "https://{LAUNCHER_HOST}/launcher/api/public/assets/v2/platform/Windows/namespace/{}/catalogItem/{}/app/{}/label/Live",
        urlencoding::encode(namespace),
        urlencoding::encode(catalog_item_id),
        urlencoding::encode(app_name),
    );
    let mut response = agent()
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("EGS manifest-list request failed: {e}"))?;
    let body: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("EGS manifest-list response parse failed: {e}"))?;

    let elements = body
        .get("elements")
        .and_then(|v| v.as_array())
        .ok_or("EGS manifest-list response missing 'elements'")?;
    let element = elements.first().ok_or(
        "EGS manifest-list response has no elements — game may not be available on Windows",
    )?;
    let manifests = element
        .get("manifests")
        .and_then(|v| v.as_array())
        .ok_or("EGS manifest-list element missing 'manifests'")?;

    let mirrors = manifests
        .iter()
        .filter_map(|m| {
            let uri = m.get("uri")?.as_str()?;
            let base_url = uri.rsplit_once('/')?.0.to_string();
            let query_string = m
                .get("queryParams")
                .and_then(|v| v.as_array())
                .map(|params| {
                    params
                        .iter()
                        .filter_map(|p| {
                            let name = p.get("name")?.as_str()?;
                            let value = p.get("value")?.as_str()?;
                            Some(format!("{name}={value}"))
                        })
                        .collect::<Vec<_>>()
                        .join("&")
                })
                .unwrap_or_default();
            let manifest_url = if query_string.is_empty() {
                uri.to_string()
            } else {
                format!("{uri}?{query_string}")
            };
            Some(ManifestMirror {
                manifest_url,
                base_url,
                query_string,
            })
        })
        .collect::<Vec<_>>();

    if mirrors.is_empty() {
        return Err("EGS manifest-list returned no usable mirrors".to_string());
    }
    Ok(mirrors)
}

/// Downloads and parses the manifest, trying every mirror in order until
/// one succeeds — same fallback-through-mirrors approach `gog_download`
/// uses for its own CDN endpoint list.
pub fn fetch_manifest(mirrors: &[ManifestMirror]) -> Result<Manifest, String> {
    let mut last_err = "no mirrors available".to_string();
    for mirror in mirrors {
        let raw = match agent()
            .get(&mirror.manifest_url)
            .header("User-Agent", USER_AGENT)
            .call()
        {
            Ok(mut response) => match response.body_mut().read_to_vec() {
                Ok(bytes) => bytes,
                Err(e) => {
                    last_err = format!("manifest read failed: {e}");
                    continue;
                }
            },
            Err(e) => {
                last_err = format!("manifest request failed: {e}");
                continue;
            }
        };
        match parse_manifest(&raw) {
            Ok(manifest) => return Ok(manifest),
            Err(e) => {
                last_err = format!("manifest parse failed: {e}");
                continue;
            }
        }
    }
    Err(format!("all manifest mirrors failed: {last_err}"))
}

/// `chunk_dir` name depends on the manifest's own feature level — confirmed
/// against `legendary`'s `get_chunk_dir`. Getting this wrong silently 404s
/// every chunk (caught live: `17` maps to `ChunksV4`, not `ChunksV3` as a
/// naive `>= 6` check alone would suggest — the thresholds must be checked
/// highest-first).
fn chunk_dir(feature_level: i32) -> &'static str {
    if feature_level >= 22 {
        "ChunksV5"
    } else if feature_level >= 15 {
        "ChunksV4"
    } else if feature_level >= 6 {
        "ChunksV3"
    } else if feature_level >= 3 {
        "ChunksV2"
    } else {
        "Chunks"
    }
}

/// Base64 (URL-safe, no padding) of 4 little-endian `u32`s — the encoding
/// the v22+ chunk path scheme uses for both the hash and the guid/secret
/// components. Confirmed against `legendary`'s `ChunkInfo.path`.
fn b64_u32s(parts: &[u32]) -> String {
    use base64::Engine;
    let mut bytes = Vec::with_capacity(parts.len() * 4);
    for p in parts {
        bytes.extend_from_slice(&p.to_le_bytes());
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn chunk_path(feature_level: i32, chunk: &ChunkInfo) -> String {
    let dir = chunk_dir(feature_level);
    if feature_level >= SECRET_GUID_FEATURE_LEVEL {
        let secret_part = match chunk.secret_guid {
            Some(g) => b64_u32s(&g),
            None => "plain".to_string(),
        };
        // hash is a u64 but the v22+ scheme base64-encodes it as if it were
        // two little-endian u32 halves back to back — same byte layout as
        // `struct.pack('<Q', hash)`, so splitting it that way here matches.
        let hash_lo = (chunk.hash & 0xFFFF_FFFF) as u32;
        let hash_hi = (chunk.hash >> 32) as u32;
        let hash_b64 = b64_u32s(&[hash_lo, hash_hi]);
        let guid_b64 = b64_u32s(&chunk.guid);
        format!(
            "{dir}/{secret_part}/{:02}/{hash_b64}_{guid_b64}.chunk",
            chunk.group_num
        )
    } else {
        let guid_hex: String = chunk.guid.iter().map(|g| format!("{g:08X}")).collect();
        format!(
            "{dir}/{:02}/{:016X}_{guid_hex}.chunk",
            chunk.group_num, chunk.hash
        )
    }
}

/// Downloads one chunk object, verifies its own header framing, decompresses
/// it if needed, and verifies the result against the chunk list's SHA1 for
/// that guid. Returns the chunk's full decompressed payload — callers slice
/// out whatever `ChunkPart::offset`/`size` range they need from it.
///
/// Tries every mirror in order, falling through on any failure (network,
/// framing, or checksum) rather than giving up on the first one — a single
/// mirror going briefly unreachable mid-download (confirmed live,
/// 2026-09-16: "No route to host" on one chunk request, dozens of prior
/// chunks from the same mirror had just succeeded) shouldn't fail the whole
/// download when two other working mirrors were already resolved and sitting
/// unused. Same fallback-through-endpoints shape `gog_download::download_chunk`
/// already uses for GOG's own CDN mirror list.
///
/// The whole mirror list is retried up to 3 times with a short backoff if
/// every mirror fails on a given pass — confirmed live (2026-09-16): a
/// second real download hit "No route to host" on *all three* mirrors for
/// the same chunk back to back, which a single blip on one mirror doesn't
/// explain. A brief, blanket network hiccup (VPN reroute, local link flap)
/// does, and it clears within a couple seconds — same retry-then-give-up
/// shape `gog_account::get_json_retrying` already uses for exactly this.
fn download_chunk(
    mirrors: &[ManifestMirror],
    manifest: &Manifest,
    chunk: &ChunkInfo,
) -> Result<Vec<u8>, String> {
    const ATTEMPTS: u32 = 3;
    let path = chunk_path(manifest.feature_level, chunk);
    let mut last_err = "no CDN mirrors available".to_string();
    for attempt in 1..=ATTEMPTS {
        for mirror in mirrors {
            match download_chunk_from(mirror, chunk, &path) {
                Ok(payload) => return Ok(payload),
                Err(e) => last_err = e,
            }
        }
        if attempt < ATTEMPTS {
            std::thread::sleep(std::time::Duration::from_millis(500 * attempt as u64));
        }
    }
    Err(format!(
        "all CDN mirrors failed for chunk {path} after {ATTEMPTS} attempts: {last_err}"
    ))
}

fn download_chunk_from(
    mirror: &ManifestMirror,
    chunk: &ChunkInfo,
    path: &str,
) -> Result<Vec<u8>, String> {
    let mut url = format!("{}/{path}", mirror.base_url);
    if !mirror.query_string.is_empty() {
        url.push('?');
        url.push_str(&mirror.query_string);
    }

    let mut response = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("chunk request failed ({path}): {e}"))?;
    let raw = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("chunk read failed ({path}): {e}"))?;

    let mut r = ByteReader::new(&raw);
    let magic = r.u32()?;
    if magic != CHUNK_MAGIC {
        return Err(format!("chunk {path} has wrong magic {magic:#x}"));
    }
    let version = r.i32()?;
    let header_size = r.u32()?;
    let _data_size_compressed = r.u32()?;
    let _guid = r.guid()?;
    let _rolling_hash = r.u64()?;
    let stored_as = r.u8()?;
    // version >= 2 ("StoresShaAndHashType"): sha1 + hash-type byte follow.
    if version >= 2 {
        r.sha1()?;
        r.u8()?;
    }
    // version >= 3 ("StoresDataSizeUncompressed"): uncompressed size follows.
    let data_size_uncompressed = if version >= 3 { Some(r.u32()?) } else { None };
    if r.tell() != header_size as usize {
        return Err(format!(
            "chunk header size mismatch ({path}): declared {header_size}, actually read {}",
            r.tell()
        ));
    }

    if stored_as & 0x02 != 0 {
        return Err(format!(
            "chunk {path} is encrypted — unsupported (no known key source)"
        ));
    }
    let payload_raw = &raw[header_size as usize..];
    let payload = if stored_as & 0x01 != 0 {
        let mut decoder = ZlibDecoder::new(payload_raw);
        let mut buf = Vec::with_capacity(data_size_uncompressed.unwrap_or(0) as usize);
        decoder
            .read_to_end(&mut buf)
            .map_err(|e| format!("chunk {path} decompress failed: {e}"))?;
        buf
    } else {
        payload_raw.to_vec()
    };

    let mut hasher = Sha1::new();
    hasher.update(&payload);
    let actual_sha1: [u8; 20] = hasher.finalize().into();
    if actual_sha1 != chunk.sha1 {
        return Err(format!("chunk {path} failed SHA1 verification"));
    }

    Ok(payload)
}

// ---------------------------------------------------------------------
// File reassembly
// ---------------------------------------------------------------------

/// Resolves `filename` (as it appears in a manifest, using `\` like every
/// real sample seen so far) against `dest_root`, rejecting anything that
/// would climb out of it — manifest paths come from Epic's CDN, not the
/// user, same zip-slip class of check `gog_download::resolve_install_path`
/// already applies to GOG's own manifest paths.
fn resolve_install_path(dest_root: &Path, filename: &str) -> Result<PathBuf, String> {
    let normalized = filename.replace('\\', "/");
    let mut resolved = dest_root.to_path_buf();
    for component in Path::new(&normalized).components() {
        match component {
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::CurDir => {}
            other => {
                return Err(format!("unsafe path component {other:?} in {filename}"));
            }
        }
    }
    if !resolved.starts_with(dest_root) {
        return Err(format!(
            "manifest entry escapes install directory: {filename}"
        ));
    }
    Ok(resolved)
}

/// Downloads every file in `manifest` into `dest_root`. Each unique chunk
/// is downloaded and decompressed once (many files/parts commonly share a
/// chunk — most manifests reference far fewer chunk *bytes on disk* than
/// chunk *parts*, e.g. shared engine content) and cached only for the
/// duration of this call, not persisted — chunk bytes aren't reusable
/// across a different game's manifest.
///
/// `on_progress` returning `false` stops the download after the current
/// file — same cooperative-cancellation shape `gog_download::download_depot`
/// uses, checked once per file rather than per chunk.
pub fn download_game(
    mirrors: &[ManifestMirror],
    manifest: &Manifest,
    dest_root: &Path,
    mut on_progress: impl FnMut(&FileEntry) -> bool,
) -> Result<(), String> {
    if mirrors.is_empty() {
        return Err("no CDN mirror available for chunk downloads".to_string());
    }

    let mut chunk_cache: HashMap<[u32; 4], Vec<u8>> = HashMap::new();

    for file in &manifest.files {
        if !on_progress(file) {
            return Err("download cancelled".to_string());
        }

        let dest_path = resolve_install_path(dest_root, &file.filename)?;
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?} failed: {e}"))?;
        }
        let mut out = fs::File::create(&dest_path)
            .map_err(|e| format!("create {dest_path:?} failed: {e}"))?;

        for part in &file.chunk_parts {
            // Not `entry()`: filling a vacant entry here means a fallible
            // network download, which the entry API has no clean way to
            // express (`or_insert_with` requires an infallible closure).
            #[allow(clippy::map_entry)]
            if !chunk_cache.contains_key(&part.guid) {
                let chunk_info = manifest.chunk_by_guid(&part.guid).ok_or_else(|| {
                    format!("file {} references unknown chunk guid", file.filename)
                })?;
                let payload = download_chunk(mirrors, manifest, chunk_info)?;
                chunk_cache.insert(part.guid, payload);
            }
            let payload = &chunk_cache[&part.guid];
            let start = part.offset as usize;
            let end = start + part.size as usize;
            if end > payload.len() {
                return Err(format!(
                    "chunk part for {} reads past its chunk's decompressed size",
                    file.filename
                ));
            }
            out.write_all(&payload[start..end])
                .map_err(|e| format!("write {dest_path:?} failed: {e}"))?;
        }

        #[cfg(unix)]
        if file.executable() {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&dest_path) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o111);
                let _ = fs::set_permissions(&dest_path, perms);
            }
        }

        // Chunks are large (typically ~1MB uncompressed) and mostly
        // consumed sequentially file-by-file — dropping the cache once a
        // chunk's last-referencing file is written would need a reference
        // count per chunk to do precisely; simpler and still bounded: cap
        // the cache at a size where re-downloading an evicted chunk stays
        // rare rather than tracking exact reuse.
        if chunk_cache.len() > 64 {
            chunk_cache.clear();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_dir_thresholds_check_highest_first() {
        // Live bug this test exists to pin: a naive `>= 6` check alone
        // would wrongly return ChunksV3 for a real feature level 17
        // manifest (confirmed live, 2026-09-16) — every level below the
        // next threshold up must fail before falling through.
        assert_eq!(chunk_dir(17), "ChunksV4");
        assert_eq!(chunk_dir(21), "ChunksV4");
        assert_eq!(chunk_dir(22), "ChunksV5");
        assert_eq!(chunk_dir(15), "ChunksV4");
        assert_eq!(chunk_dir(14), "ChunksV3");
        assert_eq!(chunk_dir(6), "ChunksV3");
        assert_eq!(chunk_dir(5), "ChunksV2");
        assert_eq!(chunk_dir(3), "ChunksV2");
        assert_eq!(chunk_dir(2), "Chunks");
    }

    #[test]
    fn chunk_path_pre_v22_matches_a_real_verified_url() {
        // Exact values from a real chunk on a real owned game (Enter the
        // Gungeon) — this path was fetched live and returned HTTP 200 with
        // the correct chunk magic (2026-09-16).
        let chunk = ChunkInfo {
            guid: [3142351150, 1241232442, 3744784272, 572789316],
            hash: 0xb8538f067e8af70a,
            sha1: [0; 20],
            group_num: 37,
            secret_guid: None,
        };
        assert_eq!(
            chunk_path(17, &chunk),
            "ChunksV4/37/B8538F067E8AF70A_BB4C792E49FBB43ADF34DF9022241244.chunk"
        );
    }

    #[test]
    fn fstring_reads_a_short_utf8_string() {
        // "Hi" as UTF-8: length=3 (2 chars + NUL), bytes 'H','i',0.
        let data = [3, 0, 0, 0, b'H', b'i', 0];
        let mut r = ByteReader::new(&data);
        assert_eq!(r.fstring().unwrap(), "Hi");
    }

    #[test]
    fn fstring_reads_an_empty_string() {
        let data = [0, 0, 0, 0];
        let mut r = ByteReader::new(&data);
        assert_eq!(r.fstring().unwrap(), "");
    }

    #[test]
    fn fstring_reads_a_utf16_string() {
        // "Hi" as UTF-16LE: length=-3 (2 units + NUL unit), units H,i,0.
        let data: [u8; 4 + 6] = {
            let mut d = [0u8; 10];
            d[0..4].copy_from_slice(&(-3i32).to_le_bytes());
            d[4..6].copy_from_slice(&0x0048u16.to_le_bytes());
            d[6..8].copy_from_slice(&0x0069u16.to_le_bytes());
            d[8..10].copy_from_slice(&0x0000u16.to_le_bytes());
            d
        };
        let mut r = ByteReader::new(&data);
        assert_eq!(r.fstring().unwrap(), "Hi");
    }

    #[test]
    fn resolve_install_path_joins_a_backslash_path() {
        let root = Path::new("/tmp/tatu-egs/MyGame");
        let resolved = resolve_install_path(root, "Data\\Textures\\a.pak").unwrap();
        assert_eq!(resolved, root.join("Data/Textures/a.pak"));
    }

    #[test]
    fn resolve_install_path_rejects_parent_dir_climb() {
        let root = Path::new("/tmp/tatu-egs/MyGame");
        assert!(resolve_install_path(root, "..\\..\\etc\\passwd").is_err());
    }

    /// Full pipeline against a real owned game (Enter the Gungeon), reusing
    /// Tatu's own stored EGS session from `state.json` — same file
    /// `AppState::path()` resolves. `#[ignore]` because it depends on a
    /// real, connected EGS account and live network access.
    #[test]
    #[ignore]
    fn fetches_and_downloads_a_real_chunk() {
        let home = std::env::var("HOME").expect("HOME not set");
        let state_path = format!("{home}/.config/backlog-tracker/state.json");
        let state_json = std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("cannot read {state_path}: {e}"));
        let state: serde_json::Value =
            serde_json::from_str(&state_json).expect("state.json is not valid JSON");
        let access_token = state["egs_tokens"]["access_token"]
            .as_str()
            .expect("no egs_tokens.access_token in state.json — connect an EGS account first")
            .to_string();

        const NAMESPACE: &str = "be13f6c4fb11427fbf3313ce93b97cc0";
        const CATALOG_ITEM_ID: &str = "25b771db01554b1bb40c5617b663d17e";
        const APP_NAME: &str = "Garlic"; // Enter the Gungeon

        let mirrors = fetch_manifest_mirrors(&access_token, NAMESPACE, CATALOG_ITEM_ID, APP_NAME)
            .expect("fetch_manifest_mirrors failed");
        assert!(!mirrors.is_empty());

        let manifest = fetch_manifest(&mirrors).expect("fetch_manifest failed");
        assert!(!manifest.chunks.is_empty());
        assert!(!manifest.files.is_empty());

        let chunk = &manifest.chunks[0];
        let payload = download_chunk(&mirrors, &manifest, chunk).expect("download_chunk failed");
        assert!(!payload.is_empty());
    }

    /// Same account/game as `fetches_and_downloads_a_real_chunk`, but
    /// exercises the full `download_game` orchestration into a scratch
    /// temp dir. `#[ignore]`: real account, live network, and downloads
    /// the entire game (~tens of MB for this title).
    #[test]
    #[ignore]
    fn download_game_installs_a_real_title() {
        let home = std::env::var("HOME").expect("HOME not set");
        let state_path = format!("{home}/.config/backlog-tracker/state.json");
        let state_json = std::fs::read_to_string(&state_path)
            .unwrap_or_else(|e| panic!("cannot read {state_path}: {e}"));
        let state: serde_json::Value =
            serde_json::from_str(&state_json).expect("state.json is not valid JSON");
        let access_token = state["egs_tokens"]["access_token"]
            .as_str()
            .expect("no egs_tokens.access_token in state.json")
            .to_string();

        const NAMESPACE: &str = "be13f6c4fb11427fbf3313ce93b97cc0";
        const CATALOG_ITEM_ID: &str = "25b771db01554b1bb40c5617b663d17e";
        const APP_NAME: &str = "Garlic";

        let mirrors = fetch_manifest_mirrors(&access_token, NAMESPACE, CATALOG_ITEM_ID, APP_NAME)
            .expect("fetch_manifest_mirrors failed");
        let manifest = fetch_manifest(&mirrors).expect("fetch_manifest failed");

        let dest_root = std::env::temp_dir().join("tatu-egs-download-test");
        let _ = std::fs::remove_dir_all(&dest_root);

        download_game(&mirrors, &manifest, &dest_root, |file| {
            eprintln!("downloading {}", file.filename);
            true
        })
        .expect("download_game failed");

        let mut files_checked = 0;
        for file in &manifest.files {
            let on_disk = dest_root.join(file.filename.replace('\\', "/"));
            let actual_size = std::fs::metadata(&on_disk)
                .unwrap_or_else(|e| panic!("{on_disk:?} missing after download_game: {e}"))
                .len();
            assert_eq!(actual_size, file.size(), "{on_disk:?} size mismatch");
            files_checked += 1;
        }
        assert!(files_checked > 0, "manifest had no file entries to check");

        std::fs::remove_dir_all(&dest_root).ok();
    }
}
