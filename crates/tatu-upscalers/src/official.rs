//! Each upscaler's newest official runtime, from its vendor's own SDK
//! repository: NVIDIA's DLSS (github.com/NVIDIA/DLSS), AMD's FSR
//! (GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) and Intel's XeSS (intel/xess).
//! Pinned by tag, size and SHA-256 like the OptiScaler release; the user's
//! Tatu fetches each file from the vendor when asked and never hosts them.
//! AMD's and Intel's licenses allow redistributing their DLLs; NVIDIA's
//! doesn't on their own, and allows them on NVIDIA GPUs — the tracker only
//! offers DLSS there.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::detect::join_rel;
use crate::package::{self, Pinned};
use crate::{UpscalerError, io_err, patch, pe};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sdk {
    Dlss,
    Fsr,
    Xess,
}

impl Sdk {
    /// Short name, also the prefix of the patch that installs it.
    pub fn kind(self) -> &'static str {
        match self {
            Sdk::Dlss => "dlss",
            Sdk::Fsr => "fsr",
            Sdk::Xess => "xess",
        }
    }

    /// The release, as the vendor names it.
    pub fn release(self) -> &'static str {
        match self {
            Sdk::Dlss => "DLSS 310.9.1",
            Sdk::Fsr => "FSR SDK 2.3.0",
            Sdk::Xess => "XeSS SDK 3.0.2",
        }
    }

    fn base_url(self) -> &'static str {
        match self {
            Sdk::Dlss => {
                "https://raw.githubusercontent.com/NVIDIA/DLSS/v310.9.1/lib/Windows_x86_64/rel"
            }
            Sdk::Fsr => {
                "https://raw.githubusercontent.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/v2.3.0/Kits/FidelityFX/signedbin"
            }
            Sdk::Xess => "https://raw.githubusercontent.com/intel/xess/v3.0.2/bin",
        }
    }
}

struct File {
    sdk: Sdk,
    name: &'static str,
    /// As its version resource reads.
    version: &'static str,
    size: u64,
    sha256: &'static str,
}

/// ponytail: pinned per Tatu release, like OptiScaler; bumping a vendor means
/// its new tag in [`Sdk::release`] / [`Sdk::base_url`] and its files' sizes,
/// hashes and versions here (their git blob ids must match what GitHub's API
/// lists for the tag).
const FILES: [File; 10] = [
    File {
        sdk: Sdk::Dlss,
        name: "nvngx_dlss.dll",
        version: "310.9.1.0",
        size: 58_956_912,
        sha256: "3975567b8943c53acce397f2b72380092f84f162d00b0d2c7d08a1025c563983",
    },
    File {
        sdk: Sdk::Dlss,
        name: "nvngx_dlssd.dll",
        version: "310.9.1.0",
        size: 48_343_664,
        sha256: "4bc7ea5fcb2f32cf86bc2cb072e8d2860914a0be511cbdcb2fc206c1d9d83b80",
    },
    File {
        sdk: Sdk::Dlss,
        name: "nvngx_dlssg.dll",
        version: "310.9.1.0",
        size: 7_460_976,
        sha256: "ff6e90eb78b827927dff5b4ecc6b1c870c2e9bca29ed9f48c7d348cc9e170b82",
    },
    File {
        sdk: Sdk::Fsr,
        name: "amd_fidelityfx_loader_dx12.dll",
        version: "2.3.0.2740",
        size: 26_376,
        sha256: "e2d85aa05a9bd9ed8b38935fdf5199372cca6f74c12015143bb6f945ee1608aa",
    },
    File {
        sdk: Sdk::Fsr,
        name: "amd_fidelityfx_upscaler_dx12.dll",
        version: "4.1.1.2740",
        size: 28_761_864,
        sha256: "d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46",
    },
    File {
        sdk: Sdk::Fsr,
        name: "amd_fidelityfx_framegeneration_dx12.dll",
        version: "4.0.1.2740",
        size: 40_085_776,
        sha256: "02297beedd285e822d3a64f314cf00faf378dcec0edc47ff0c4dd71b3a8c2f18",
    },
    File {
        sdk: Sdk::Xess,
        name: "libxess.dll",
        version: "2.0.2.68",
        size: 77_795_704,
        sha256: "251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7",
    },
    File {
        sdk: Sdk::Xess,
        name: "libxess_dx11.dll",
        version: "2.0.2.68",
        size: 156_016,
        sha256: "c7cfe86f0c9d94e4fb3696d3cd5035e2bbb6a8b1b0572f8b7395a4cdfd0c625e",
    },
    File {
        sdk: Sdk::Xess,
        name: "libxess_fg.dll",
        version: "1.3.1.78",
        size: 22_957_432,
        sha256: "ec5e0c65e075570c6ede72618bb666d0be0c2e10b2ea9762c0fe8cb8e375ab27",
    },
    File {
        sdk: Sdk::Xess,
        name: "libxell.dll",
        version: "1.3.2.10",
        size: 415_368,
        sha256: "d2030dcd694fda8f2ec7e044b13e6db8f0b56d4ba9113a5efad334e3f3ded8c7",
    },
];

/// FSR's providers, loaded by the FSR 3.1+ loader from its own folder: a
/// game's `amd_fidelityfx_dx12.dll` becomes the SDK's loader, and these go
/// next to it — without them the new loader has no upscaler to hand out.
const FSR_PROVIDERS: [&str; 2] = [
    "amd_fidelityfx_upscaler_dx12.dll",
    "amd_fidelityfx_framegeneration_dx12.dll",
];

fn file(name: &str) -> Option<&'static File> {
    FILES.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// Which official file replaces a game's DLL of this name: the same name,
/// except the FSR 3.1 loader, which games ship as `amd_fidelityfx_dx12.dll`.
fn official_for(game_name: &str) -> Option<&'static File> {
    if game_name.eq_ignore_ascii_case("amd_fidelityfx_dx12.dll") {
        file("amd_fidelityfx_loader_dx12.dll")
    } else {
        file(game_name)
    }
}

fn parts(version: &str) -> Option<Vec<u32>> {
    version.split('.').map(|p| p.parse().ok()).collect()
}

fn older(version: Option<&str>, than: &str) -> bool {
    version
        .and_then(parts)
        .zip(parts(than))
        .is_some_and(|(v, official)| v < official)
}

/// A game DLL the official release is newer than.
#[derive(Debug, Clone, Serialize)]
pub struct Update {
    /// The game's file, under its root.
    pub rel: String,
    pub sdk: Sdk,
    pub release: &'static str,
    /// The official file's version.
    pub version: &'static str,
}

/// The update for the game's `rel` at `version`, if the official release is
/// newer. Unknown versions aren't offered one: nothing to compare against.
pub fn update_for(rel: &str, version: Option<&str>) -> Option<Update> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let official = official_for(name)?;
    // DLSS 1 (Metro Exodus, BFV, FFXV…) isn't compatible with DLSS 2 and
    // later: a newer DLL there breaks the game.
    let major = version.and_then(parts).and_then(|p| p.first().copied());
    if official.sdk == Sdk::Dlss && major.is_some_and(|m| m < 2) {
        return None;
    }
    older(version, official.version).then(|| Update {
        rel: rel.to_string(),
        sdk: official.sdk,
        release: official.sdk.release(),
        version: official.version,
    })
}

/// What installing `update` places, as (official file, destination under
/// `root`): the official file over the game's, and for the FSR loader each
/// provider its folder lacks or has in an older version.
pub fn placements(root: &Path, update: &Update) -> Vec<(&'static str, String)> {
    let name = update.rel.rsplit('/').next().unwrap_or(&update.rel);
    let Some(official) = official_for(name) else {
        return Vec::new();
    };
    let mut out = vec![(official.name, update.rel.clone())];
    if official.name == "amd_fidelityfx_loader_dx12.dll" {
        let dir = update.rel.rsplit_once('/').map_or("", |(dir, _)| dir);
        for provider in FSR_PROVIDERS {
            let rel = join_rel(dir, provider);
            let (existing, _) = patch::resolve_ci(root, &rel);
            let current = existing
                .is_file()
                .then(|| pe::file_version(&existing))
                .flatten();
            let newest = file(provider).map_or("", |f| f.version);
            if !existing.is_file() || older(current.as_deref(), newest) {
                out.push((provider_name(provider), rel));
            }
        }
    }
    out
}

fn provider_name(name: &str) -> &'static str {
    file(name).map_or("", |f| f.name)
}

/// The update as one reversible patch: every official file in `fetched`
/// (downloaded path, destination — as [`placements`] listed them) in place,
/// whatever the game had there backed up. Named after the SDK and the
/// game's file, so a second update of the same DLL is refused until the
/// first is reverted.
pub fn plan(
    root: &Path,
    update: &Update,
    fetched: Vec<(PathBuf, String)>,
    running: Vec<String>,
) -> patch::Plan {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(update.rel.to_lowercase().as_bytes()));
    patch::Plan {
        id: format!("{}-{}", update.sdk.kind(), &digest[..8]),
        sources: fetched
            .into_iter()
            .map(|(from, rel)| patch::Source {
                from,
                rel,
                owned: false,
            })
            .collect(),
        runtime: Vec::new(),
        meta: serde_json::json!({
            "kind": update.sdk.kind(),
            "file": update.rel,
            // `apply` validates the paths before touching anything; reading
            // a version first is harmless.
            "from_version": pe::file_version(&root.join(&update.rel)),
            "to_version": update.version,
            "release": update.release,
        }),
        running,
    }
}

/// Bytes still to download for `names` under `cache`.
pub fn missing_bytes(cache: &Path, names: &[&str]) -> u64 {
    names
        .iter()
        .filter_map(|name| file(name))
        .filter(|f| cached(cache, f).is_none())
        .map(|f| f.size)
        .sum()
}

/// The official file `name` under `cache`, downloaded from its vendor first
/// if it isn't there — exactly the pinned file, or an error.
pub fn ensure(
    cache: &Path,
    name: &str,
    mut progress: impl FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<PathBuf, UpscalerError> {
    let f = file(name).ok_or_else(|| {
        UpscalerError::Invalid(format!("{name} no es un DLL oficial que Tatu conozca"))
    })?;
    if let Some(path) = cached(cache, f) {
        return Ok(path);
    }
    let dir = dir(cache, f.sdk);
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    let part = dir.join(format!("{}.part", f.name));
    let url = format!("{}/{}", f.sdk.base_url(), f.name);
    package::download(
        &Pinned {
            url: &url,
            size: f.size,
            sha256: f.sha256,
        },
        &part,
        &mut progress,
        cancel,
    )?;
    let path = dir.join(f.name);
    fs::rename(&part, &path).map_err(|e| io_err(&path, e))?;
    Ok(path)
}

fn dir(cache: &Path, sdk: Sdk) -> PathBuf {
    let release = sdk.release().to_lowercase().replace(' ', "-");
    cache.join("official").join(release)
}

/// A copy already downloaded, if it's still exactly the pinned file.
fn cached(cache: &Path, f: &File) -> Option<PathBuf> {
    let path = dir(cache, f.sdk).join(f.name);
    let intact = fs::metadata(&path).is_ok_and(|m| m.len() == f.size)
        && patch::sha256_file(&path).is_ok_and(|sha| sha == f.sha256);
    intact.then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_older_dlls_are_offered_the_official_update() {
        let up = |rel: &str, v: &str| update_for(rel, Some(v)).map(|u| (u.sdk, u.version));
        assert_eq!(
            up("bin/nvngx_dlss.dll", "3.7.10.0"),
            Some((Sdk::Dlss, "310.9.1.0"))
        );
        assert_eq!(up("NVNGX_DLSSG.DLL", "310.9.1.0"), None);
        assert_eq!(
            up("amd_fidelityfx_dx12.dll", "1.0.1.41314"),
            Some((Sdk::Fsr, "2.3.0.2740"))
        );
        assert_eq!(up("libxess.dll", "1.3.1.33"), Some((Sdk::Xess, "2.0.2.68")));
        assert_eq!(up("libxess.dll", "2.1.0.0"), None);
        assert_eq!(
            up("amd_fidelityfx_vk.dll", "1.0.0.0"),
            None,
            "no official Vulkan build"
        );
        assert!(update_for("libxess.dll", None).is_none());
        assert_eq!(
            up("nvngx_dlss.dll", "1.0.13.0"),
            None,
            "DLSS 1 isn't compatible"
        );
        assert_eq!(
            up("nvngx_dlss.dll", "2.1.66.0"),
            Some((Sdk::Dlss, "310.9.1.0"))
        );
    }

    #[test]
    fn the_fsr_loader_brings_the_providers_its_folder_lacks() {
        let tmp = tempfile::tempdir().unwrap();
        // The game already has a provider newer than the pinned one.
        let pe = crate::pe::tests::fake_pe([9, 0, 0, 0], "AMD");
        fs::create_dir_all(tmp.path().join("bin")).unwrap();
        fs::write(
            tmp.path()
                .join("bin/amd_fidelityfx_framegeneration_dx12.dll"),
            pe,
        )
        .unwrap();
        let update = update_for("bin/amd_fidelityfx_dx12.dll", Some("1.0.1.0")).unwrap();
        assert_eq!(
            placements(tmp.path(), &update),
            vec![
                (
                    "amd_fidelityfx_loader_dx12.dll",
                    "bin/amd_fidelityfx_dx12.dll".to_string()
                ),
                (
                    "amd_fidelityfx_upscaler_dx12.dll",
                    "bin/amd_fidelityfx_upscaler_dx12.dll".to_string()
                ),
            ]
        );
        let xess = update_for("libxess.dll", Some("1.0.0.0")).unwrap();
        assert_eq!(
            placements(tmp.path(), &xess),
            vec![("libxess.dll", "libxess.dll".to_string())]
        );
    }

    #[test]
    fn a_download_cancelled_before_it_starts_never_connects() {
        let tmp = tempfile::tempdir().unwrap();
        let cancelled = AtomicBool::new(true);
        let started = std::time::Instant::now();
        let stopped = ensure(
            tmp.path(),
            "libxell.dll",
            |_, _| panic!("no download"),
            &cancelled,
        );
        assert!(matches!(stopped, Err(UpscalerError::Cancelled)));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "no network was touched"
        );
        assert!(!dir(tmp.path(), Sdk::Xess).join("libxell.dll").exists());
    }

    #[test]
    fn a_cached_copy_must_be_exactly_the_pinned_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = file("libxell.dll").unwrap();
        let path = dir(tmp.path(), f.sdk).join(f.name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not the release").unwrap();
        assert!(cached(tmp.path(), f).is_none());
        assert_eq!(missing_bytes(tmp.path(), &["libxell.dll"]), f.size);
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Stand-ins for downloaded official files, named like the real ones.
    fn fetched(dir: &Path, placements: &[(&str, String)]) -> Vec<(PathBuf, String)> {
        placements
            .iter()
            .map(|(name, dest)| {
                let path = dir.join(name);
                write(&path, format!("official {name}").as_bytes());
                (path, dest.clone())
            })
            .collect()
    }

    fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, fs::read(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn an_fsr_update_installs_as_one_patch_and_reverts_byte_for_byte() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("game");
        let game_ffx = crate::pe::tests::fake_pe([1, 0, 1, 41314], "FidelityFX");
        write(&root.join("bin/Game.exe"), b"exe");
        write(&root.join("bin/amd_fidelityfx_dx12.dll"), &game_ffx);
        let before = tree(&root);

        let update = update_for("bin/amd_fidelityfx_dx12.dll", Some("1.0.1.41314")).unwrap();
        let places = placements(&root, &update);
        assert_eq!(places.len(), 3, "loader + both providers");
        let plan = plan(
            &root,
            &update,
            fetched(&tmp.path().join("cache"), &places),
            Vec::new(),
        );
        assert!(plan.id.starts_with("fsr-"));
        let manifest = patch::apply(&root, &plan).unwrap();

        // The loader under the game's name, the providers next to it, the
        // game's own DLL in the backup.
        assert_eq!(
            fs::read(root.join("bin/amd_fidelityfx_dx12.dll")).unwrap(),
            b"official amd_fidelityfx_loader_dx12.dll"
        );
        assert!(root.join("bin/amd_fidelityfx_upscaler_dx12.dll").is_file());
        assert!(
            root.join("bin/amd_fidelityfx_framegeneration_dx12.dll")
                .is_file()
        );
        assert_eq!(
            manifest.backed_up(),
            vec!["bin/amd_fidelityfx_dx12.dll".to_string()]
        );
        assert_eq!(manifest.meta["from_version"], "1.0.1.41314");
        assert_eq!(manifest.meta["to_version"], "2.3.0.2740");
        // The scan now sees them as Tatu's: no second update is offered.
        let scan = crate::detect::scan(&root);
        assert!(
            scan.upscalers
                .iter()
                .all(|u| u.patched_by.as_deref() == Some(plan.id.as_str()))
        );

        assert!(patch::revert(&root, &plan.id).unwrap().complete());
        assert_eq!(tree(&root), before);
    }

    #[test]
    fn an_update_over_a_file_another_patch_placed_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("game");
        write(&root.join("bin/Game.exe"), b"exe");
        write(
            &root.join("bin/amd_fidelityfx_dx12.dll"),
            &crate::pe::tests::fake_pe([1, 0, 1, 0], "FFX"),
        );
        // Another patch (e.g. OptiScaler) put an older upscaler provider there.
        let other_upscaler = tmp.path().join("other/amd_fidelityfx_upscaler_dx12.dll");
        write(
            &other_upscaler,
            &crate::pe::tests::fake_pe([3, 1, 0, 0], "FFX"),
        );
        let other = patch::Plan {
            id: "optiscaler".into(),
            sources: vec![patch::Source {
                from: other_upscaler,
                rel: "bin/amd_fidelityfx_upscaler_dx12.dll".into(),
                owned: false,
            }],
            runtime: Vec::new(),
            meta: serde_json::Value::Null,
            running: Vec::new(),
        };
        patch::apply(&root, &other).unwrap();
        let before = tree(&root);

        let update = update_for("bin/amd_fidelityfx_dx12.dll", Some("1.0.1.0")).unwrap();
        let places = placements(&root, &update);
        assert!(
            places
                .iter()
                .any(|(_, dest)| dest.ends_with("upscaler_dx12.dll"))
        );
        let plan = plan(
            &root,
            &update,
            fetched(&tmp.path().join("cache"), &places),
            Vec::new(),
        );
        assert!(matches!(
            patch::apply(&root, &plan),
            Err(UpscalerError::Overlap(..))
        ));
        assert_eq!(tree(&root), before);
    }

    #[test]
    fn a_dll_updated_once_is_refused_a_second_time_until_reverted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("game");
        write(
            &root.join("libxess.dll"),
            &crate::pe::tests::fake_pe([1, 3, 1, 0], "XeSS"),
        );
        let update = update_for("libxess.dll", Some("1.3.1.0")).unwrap();
        let places = placements(&root, &update);
        let first = plan(
            &root,
            &update,
            fetched(&tmp.path().join("a"), &places),
            Vec::new(),
        );
        patch::apply(&root, &first).unwrap();
        let again = plan(
            &root,
            &update,
            fetched(&tmp.path().join("b"), &places),
            Vec::new(),
        );
        assert!(matches!(
            patch::apply(&root, &again),
            Err(UpscalerError::AlreadyApplied(_))
        ));
    }

    #[test]
    fn every_pinned_file_is_offered_only_over_older_versions() {
        for f in &FILES {
            let game_name = if f.name == "amd_fidelityfx_loader_dx12.dll" {
                "amd_fidelityfx_dx12.dll"
            } else {
                f.name
            };
            let mut older_version = parts(f.version).unwrap();
            older_version[0] -= 1;
            let older_version: Vec<String> = older_version.iter().map(u32::to_string).collect();
            let up = update_for(game_name, Some(&older_version.join("."))).unwrap();
            assert_eq!((up.sdk, up.version), (f.sdk, f.version), "{}", f.name);
            assert!(
                update_for(game_name, Some(f.version)).is_none(),
                "{} same",
                f.name
            );
            assert!(
                update_for(game_name, Some("999.0.0.0")).is_none(),
                "{} newer",
                f.name
            );
            // Pins look like what they claim to be.
            assert_eq!(f.sha256.len(), 64);
            assert!(
                f.size > 0
                    && f.sdk
                        .base_url()
                        .contains(f.sdk.release().rsplit(' ').next().unwrap())
            );
        }
    }
}
