use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Adopted rather than reinvented (#206): [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher)
/// is the maintained, actively-used-by-Lutris/Heroic tool for running Proton
/// outside of Steam — replicating Steam's own runtime container so a game
/// behaves the same as it would through Steam's client, without needing that
/// client installed. Re-implementing that container/Proton-invocation dance
/// by hand would just be a worse, unmaintained copy of this.
const UMU_RUN_URL: &str = "https://github.com/Open-Wine-Components/umu-launcher/releases/download/1.4.4/umu-launcher-1.4.4-zipapp.tar";

const PROTON_VERSION: &str = "GE-Proton11-5";
const PROTON_URL: &str = "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/GE-Proton11-5/GE-Proton11-5-x86_64.tar.gz";
const PROTON_SHA512_URL: &str = "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/GE-Proton11-5/GE-Proton11-5-x86_64.sha512sum";

/// The Steam Linux Runtime container this pinned Proton build requires —
/// read out of its own `toolmanifest.vdf` (`require_tool_appid`), NOT
/// assumed. Verified empirically against `PROTON_VERSION` above: this maps
/// to appid 4183110 (variant "steamrt4"), not the older "sniper" (1628350)
/// most write-ups from before 2026 still reference. Re-check this mapping
/// by hand whenever `PROTON_VERSION` changes — umu's own
/// `RUNTIME_VERSIONS` table (`umu/umu_runtime.py`) is the source of truth.
const RUNTIME_VARIANT: &str = "steamrt4";
/// Archive name follows `SteamLinuxRuntime_<N>.tar.xz` for a numeric
/// codename (steamrt4 → "4") — see umu_runtime.py's own `_install_umu`.
const RUNTIME_ARCHIVE: &str = "SteamLinuxRuntime_4.tar.xz";

const RUNTIME_SUBDIR: &str = "runtime/linux";

/// Bundles the shared umu-run + Proton + Steam Linux Runtime files onto the
/// cartridge (#206) — the launcher (#204) later deploys these locally on
/// whatever destination machine it runs on and never touches the network
/// itself. Every Goldberg-patched ("Easy") app needs the same runtime, so
/// this only ever runs the actual fetch once per Tatu install (cached under
/// the OS cache dir) and is a no-op copy after the first "Easy" game on a
/// given cartridge.
pub async fn bundle_linux_runtime(mount_point: PathBuf) -> Result<(), String> {
    tokio::task::spawn_blocking(move || bundle_linux_runtime_sync(&mount_point))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn bundle_linux_runtime_sync(mount_point: &Path) -> Result<(), String> {
    let dest = mount_point.join(RUNTIME_SUBDIR);
    if dest.join("umu-run").is_file() {
        return Ok(()); // Already bundled on this cartridge.
    }

    let cache = ensure_cached()?;
    fs::create_dir_all(&dest).map_err(|e| format!("Cannot create {}: {e}", dest.display()))?;
    for name in ["umu-run", proton_filename(), RUNTIME_ARCHIVE] {
        fs::copy(cache.join(name), dest.join(name))
            .map_err(|e| format!("Cannot copy {name} onto cartridge: {e}"))?;
    }

    let manifest = format!(
        "{{\"proton_version\":\"{PROTON_VERSION}\",\"runtime_variant\":\"{RUNTIME_VARIANT}\"}}"
    );
    fs::write(dest.join("manifest.json"), manifest)
        .map_err(|e| format!("Cannot write runtime manifest: {e}"))
}

fn proton_filename() -> &'static str {
    "GE-Proton11-5-x86_64.tar.gz"
}

/// Downloads umu-run + the pinned Proton + its required Steam Linux Runtime
/// into a shared local cache (`<cache dir>/tatu/runtime/linux/`) if not
/// already there — this is the ONLY place that ever hits the network for
/// these files. Every cartridge's `bundle_linux_runtime_sync` call after the
/// first just copies from here.
fn ensure_cached() -> Result<PathBuf, String> {
    let cache = dirs::cache_dir()
        .ok_or("Cannot resolve the OS cache directory")?
        .join("tatu")
        .join(RUNTIME_SUBDIR);
    fs::create_dir_all(&cache).map_err(|e| format!("Cannot create {}: {e}", cache.display()))?;

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build(),
    );

    if !cache.join("umu-run").is_file() {
        download_umu_run(&agent, &cache)?;
    }
    if !cache.join(proton_filename()).is_file() {
        download_and_verify_sha512(
            &agent,
            PROTON_URL,
            PROTON_SHA512_URL,
            &cache,
            proton_filename(),
        )?;
    }
    if !cache.join(RUNTIME_ARCHIVE).is_file() {
        download_runtime(&agent, &cache)?;
    }

    Ok(cache)
}

/// The umu-run zipapp tarball has no companion checksum file published —
/// same trust level this codebase already extends to other GitHub-hosted
/// vendored assets (Kenney icons, Google Fonts) fetched over HTTPS.
fn download_umu_run(agent: &ureq::Agent, cache: &Path) -> Result<(), String> {
    let tmp = cache.join("umu-launcher.tar");
    download_to_file(agent, UMU_RUN_URL, &tmp)?;
    extract_tar_member(&tmp, "umu/umu-run", &cache.join("umu-run"))?;
    fs::remove_file(&tmp).ok();
    make_executable(&cache.join("umu-run"))
}

/// Resolves the current sniper/steamrt4 build the same way umu-run itself
/// does (`umu/umu_runtime.py::_install_umu`) — a `latest-public-beta.txt`
/// version pointer, then the archive + its `SHA256SUMS` entry, both served
/// straight from Valve's own repo.
fn download_runtime(agent: &ureq::Agent, cache: &Path) -> Result<(), String> {
    let base = format!("https://repo.steampowered.com/{RUNTIME_VARIANT}/images");
    let version = agent
        .get(format!("{base}/latest-public-beta.txt"))
        .call()
        .map_err(|e| format!("Runtime version lookup failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Runtime version read failed: {e}"))?
        .trim()
        .to_string();

    let sums = agent
        .get(format!("{base}/{version}/SHA256SUMS"))
        .call()
        .map_err(|e| format!("Runtime checksum request failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Runtime checksum read failed: {e}"))?;
    let expected = sums
        .lines()
        .find_map(|line| line.strip_suffix(&format!(" *{RUNTIME_ARCHIVE}")))
        .ok_or(format!("No checksum entry for {RUNTIME_ARCHIVE}"))?
        .to_string();

    let dest = cache.join(RUNTIME_ARCHIVE);
    download_to_file(agent, &format!("{base}/{version}/{RUNTIME_ARCHIVE}"), &dest)?;
    verify_sha256(&dest, &expected)
}

fn download_and_verify_sha512(
    agent: &ureq::Agent,
    url: &str,
    sha512_url: &str,
    cache: &Path,
    filename: &str,
) -> Result<(), String> {
    let sums = agent
        .get(sha512_url)
        .call()
        .map_err(|e| format!("Checksum request failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Checksum read failed: {e}"))?;
    let expected = sums
        .split_whitespace()
        .next()
        .ok_or("Empty checksum file")?
        .to_string();

    let dest = cache.join(filename);
    download_to_file(agent, url, &dest)?;
    verify_sha512(&dest, &expected)
}

fn download_to_file(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("part");
    let mut reader = agent
        .get(url)
        .call()
        .map_err(|e| format!("Download failed for {url}: {e}"))?
        .into_body()
        .into_reader();
    let mut writer = BufWriter::new(
        File::create(&tmp).map_err(|e| format!("Cannot create {}: {e}", tmp.display()))?,
    );
    io::copy(&mut reader, &mut writer).map_err(|e| format!("Download write failed: {e}"))?;
    drop(writer);
    fs::rename(&tmp, dest).map_err(|e| format!("Cannot finalize {}: {e}", dest.display()))
}

fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    (actual == expected).then_some(()).ok_or_else(|| {
        format!(
            "Checksum mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )
    })
}

fn verify_sha512(path: &Path, expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha512};
    let bytes = fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let actual = format!("{:x}", Sha512::digest(&bytes));
    (actual == expected).then_some(()).ok_or_else(|| {
        format!(
            "Checksum mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )
    })
}

/// Extracts a single named member out of a plain (uncompressed) tar file —
/// the umu-launcher zipapp release ships as a bare `.tar`, not `.tar.gz`.
fn extract_tar_member(tar_path: &Path, member: &str, dest: &Path) -> Result<(), String> {
    let file =
        File::open(tar_path).map_err(|e| format!("Cannot open {}: {e}", tar_path.display()))?;
    let mut archive = tar::Archive::new(file);
    let mut entries = archive
        .entries()
        .map_err(|e| format!("Cannot read tar entries: {e}"))?;
    let mut entry = entries
        .find_map(|e| {
            e.ok()
                .filter(|e| e.path().ok().is_some_and(|p| p == Path::new(member)))
        })
        .ok_or(format!("{member} not found in {}", tar_path.display()))?;
    let mut out =
        File::create(dest).map_err(|e| format!("Cannot create {}: {e}", dest.display()))?;
    io::copy(&mut entry, &mut out).map_err(|e| format!("Cannot extract {member}: {e}"))?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .map_err(|e| format!("Cannot stat {}: {e}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).map_err(|e| format!("Cannot chmod {}: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod manual_network_tests {
    use super::*;

    // Real network, ~660MB download — not part of the normal suite. Run by
    // hand with `cargo test -- --ignored manual_full_bundle` after touching
    // this file's URLs/checksums logic.
    #[test]
    #[ignore]
    fn manual_full_bundle() {
        let cartridge = tempfile::tempdir().unwrap();
        bundle_linux_runtime_sync(cartridge.path()).unwrap();

        let dest = cartridge.path().join(RUNTIME_SUBDIR);
        assert!(dest.join("umu-run").is_file());
        assert!(dest.join(proton_filename()).is_file());
        assert!(dest.join(RUNTIME_ARCHIVE).is_file());
        assert!(dest.join("manifest.json").is_file());

        // Idempotent: a second call must not re-download or re-copy.
        bundle_linux_runtime_sync(cartridge.path()).unwrap();
    }
}
