//! What a game folder already has: the upscaler DLLs it ships (and their
//! versions), anti-cheat, where OptiScaler would go, which proxy names are
//! free. Read-only.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::optiscaler::PROXIES;
use crate::patch::{self, PATCHES_DIR};
use crate::pe;

/// Deep enough for Unreal's `Engine/Plugins/Runtime/Nvidia/DLSS/Binaries/ThirdParty/Win64/`.
const MAX_DEPTH: usize = 9;
/// A game with more entries than this is mostly assets; stop looking.
const MAX_ENTRIES: usize = 60_000;

/// Upscaler DLLs by file name (lowercase): kind and what to show.
const UPSCALERS: &[(&str, &str, &str)] = &[
    ("nvngx_dlss.dll", "dlss", "DLSS"),
    ("nvngx_dlssd.dll", "dlssd", "DLSS Ray Reconstruction"),
    ("nvngx_dlssg.dll", "dlssg", "DLSS Frame Generation"),
    ("sl.interposer.dll", "streamline", "NVIDIA Streamline"),
    ("amd_fidelityfx_dx12.dll", "fsr3", "FSR 3.1+ (DX12)"),
    (
        "amd_fidelityfx_upscaler_dx12.dll",
        "fsr3",
        "FSR 3.1+ (DX12)",
    ),
    ("amd_fidelityfx_vk.dll", "fsr3", "FSR 3.1+ (Vulkan)"),
    (
        "amd_fidelityfx_loader_dx12.dll",
        "fsr3",
        "FSR 3.1+ loader (DX12)",
    ),
    // Split out of amd_fidelityfx_dx12.dll since FSR 3.1.4: a game shipping
    // it uses FSR frame generation. The combined DLL alone says nothing —
    // every FSR 3.1 game carries its frame generation, used or not.
    (
        "amd_fidelityfx_framegeneration_dx12.dll",
        "fsrfg",
        "FSR Frame Generation (DX12)",
    ),
    ("ffx_fsr2_api_x64.dll", "fsr2", "FSR 2"),
    ("ffx_fsr2_api_dx12_x64.dll", "fsr2", "FSR 2 (DX12)"),
    ("ffx_fsr2_api_vk_x64.dll", "fsr2", "FSR 2 (Vulkan)"),
    ("libxess.dll", "xess", "XeSS"),
    ("libxess_dx11.dll", "xess", "XeSS (DX11)"),
    ("libxess_fg.dll", "xefg", "XeSS Frame Generation"),
    ("libxell.dll", "xell", "XeLL (baja latencia)"),
];

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub kind: &'static str,
    pub label: &'static str,
    /// Under the game root, `/`-separated.
    pub rel: String,
    pub version: Option<String>,
    /// Put there by a Tatu patch (e.g. OptiScaler's own libxess), not the game's.
    pub patched_by: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Scan {
    pub upscalers: Vec<Found>,
    pub anti_cheat: Vec<&'static str>,
    /// The walk hit [`MAX_ENTRIES`]: what's listed may be incomplete.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Proxy {
    pub name: &'static str,
    /// Why it can't be used: `None` when free.
    pub taken: Option<&'static str>,
}

/// Where OptiScaler goes: next to the exe that actually runs. Unreal games
/// start from a small root stub that launches the real one from
/// `<Project>/Binaries/Win64/` — usually `<Project>-Win64-Shipping.exe`, but
/// not always (FF7 Rebirth's is `End/Binaries/Win64/ff7rebirth_.exe`). The
/// release's setup scripts insist on that folder, and never `Engine/`.
pub fn target_dir(root: &Path, exe_rel: Option<&str>) -> String {
    let exe_dir = exe_rel.and_then(|exe| {
        exe.rsplit_once(['/', '\\'])
            .map(|(dir, _)| dir.replace('\\', "/"))
    });
    if let Some(dir) = &exe_dir
        && is_unreal_binaries(dir)
    {
        return dir.clone();
    }
    if let Some(dir) = unreal_project_binaries(root) {
        return dir;
    }
    if let Some(shipping) = find_shipping_exe(root, 0, &mut 0) {
        return rel_of(root, shipping.parent().unwrap_or(root));
    }
    exe_dir.unwrap_or_default()
}

fn is_unreal_binaries(dir: &str) -> bool {
    let dir = dir.to_lowercase();
    dir.ends_with("binaries/win64") || dir.ends_with("binaries/wingdk")
}

/// An Unreal layout (`Engine/` at the root): the `<Project>/Binaries/Win64`
/// (or `WinGDK`) holding the biggest exe that isn't a crash reporter.
fn unreal_project_binaries(root: &Path) -> Option<String> {
    let entries: Vec<_> = fs::read_dir(root).ok()?.flatten().collect();
    let is_engine = |e: &fs::DirEntry| {
        e.file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("engine")
    };
    if !entries.iter().any(|e| is_engine(e) && e.path().is_dir()) {
        return None;
    }
    let mut best: Option<(u64, PathBuf)> = None;
    for project in entries
        .iter()
        .filter(|e| !is_engine(e) && e.path().is_dir())
    {
        for platform in ["Binaries/Win64", "Binaries/WinGDK"] {
            let (dir, _) = patch::resolve_ci(&project.path(), platform);
            let Ok(files) = fs::read_dir(&dir) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().to_lowercase();
                if !name.ends_with(".exe") || name.contains("crash") {
                    continue;
                }
                let size = file.metadata().map(|m| m.len()).unwrap_or(0);
                if best.as_ref().is_none_or(|(b, _)| size > *b) {
                    best = Some((size, dir.clone()));
                }
            }
        }
    }
    best.map(|(_, dir)| rel_of(root, &dir))
}

fn find_shipping_exe(dir: &Path, depth: usize, seen: &mut usize) -> Option<PathBuf> {
    if depth > 4 || *seen > MAX_ENTRIES {
        return None;
    }
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(dir).ok()?.flatten() {
        *seen += 1;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let path = entry.path();
        if path.is_dir() {
            if name != "engine" && name != PATCHES_DIR {
                subdirs.push(path);
            }
        } else if name.ends_with("-win64-shipping.exe") || name.ends_with("-wingdk-shipping.exe") {
            return Some(path);
        }
    }
    subdirs.sort();
    subdirs
        .into_iter()
        .find_map(|d| find_shipping_exe(&d, depth + 1, seen))
}

/// Upscaler DLLs and anti-cheat anywhere in the game. Tatu's own backups are
/// skipped (they'd show the game's replaced DLLs as if still active).
pub fn scan(root: &Path) -> Scan {
    let patched: Vec<(String, String)> = patch::list(root)
        .into_iter()
        .flat_map(|m| {
            let id = m.id.clone();
            m.entries
                .into_iter()
                .map(move |e| (e.rel.to_lowercase(), id.clone()))
        })
        .collect();

    let mut scan = Scan::default();
    let mut seen = 0usize;
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                scan.truncated = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let path = entry.path();
            if path.is_dir() {
                if let Some(ac) = anti_cheat_dir(&name) {
                    add_unique(&mut scan.anti_cheat, ac);
                }
                if name != PATCHES_DIR && depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if let Some(ac) = anti_cheat_file(&name) {
                add_unique(&mut scan.anti_cheat, ac);
            }
            if let Some((_, kind, label)) = UPSCALERS.iter().find(|(file, _, _)| *file == name) {
                let rel = rel_of(root, &path);
                let patched_by = patched
                    .iter()
                    .find(|(p, _)| *p == rel.to_lowercase())
                    .map(|(_, id)| id.clone());
                scan.upscalers.push(Found {
                    kind,
                    label,
                    version: pe::file_version(&path),
                    rel,
                    patched_by,
                });
            }
        }
        if scan.truncated {
            break;
        }
    }
    scan.upscalers.sort_by(|a, b| a.rel.cmp(&b.rel));
    scan
}

/// The proxy names OptiScaler can take in `target`, in the release scripts'
/// order (the first free one is the default). A name is taken by whatever
/// file already has it, in any case.
pub fn proxies(root: &Path, target_rel: &str) -> Vec<Proxy> {
    let ours: Vec<String> = patch::load(root, crate::optiscaler::PATCH_ID)
        .ok()
        .flatten()
        .map(|m| {
            m.entries
                .into_iter()
                .map(|e| e.rel.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    PROXIES
        .iter()
        .map(|&name| {
            let rel = join_rel(target_rel, name);
            let (path, disk_rel) = patch::resolve_ci(root, &rel);
            let taken = if !path.is_file() {
                None
            } else if ours.contains(&disk_rel.to_lowercase()) {
                Some("OptiScaler (Tatu)")
            } else if pe::is_optiscaler(&path) {
                Some("OptiScaler")
            } else {
                Some("un archivo del juego o de otro mod")
            };
            Proxy { name, taken }
        })
        .collect()
}

/// Names older OptiScaler versions installed themselves as (the release's
/// setup_windows.bat still cleans these up): an OptiScaler build under one of
/// them would load alongside ours.
const LEGACY_NAMES: [&str; 2] = ["nvngx.dll", "OptiScaler.asi"];

/// An old OptiScaler install in `target_rel` under a name Tatu doesn't use.
pub fn legacy_optiscaler(root: &Path, target_rel: &str) -> Option<&'static str> {
    LEGACY_NAMES.into_iter().find(|name| {
        let (path, _) = patch::resolve_ci(root, &join_rel(target_rel, name));
        path.is_file() && pe::is_optiscaler(&path)
    })
}

/// A proxy that is an OptiScaler build Tatu didn't install (a manual or
/// older install). Installing next to it would load OptiScaler twice.
pub fn foreign_optiscaler(proxies: &[Proxy]) -> Option<&'static str> {
    proxies
        .iter()
        .find(|p| p.taken == Some("OptiScaler"))
        .map(|p| p.name)
}

fn anti_cheat_dir(name: &str) -> Option<&'static str> {
    match name {
        "easyanticheat" | "easyanticheat_eos" => Some("EasyAntiCheat"),
        "battleye" => Some("BattlEye"),
        "gameguard" => Some("nProtect GameGuard"),
        "xigncode" | "xigncode3" => Some("XIGNCODE3"),
        "anticheatexpert" => Some("ACE"),
        _ => None,
    }
}

fn anti_cheat_file(name: &str) -> Option<&'static str> {
    if name == "start_protected_game.exe" || name.starts_with("easyanticheat") {
        Some("EasyAntiCheat")
    } else if name.starts_with("beservice") {
        Some("BattlEye")
    } else {
        None
    }
}

fn add_unique(list: &mut Vec<&'static str>, item: &'static str) {
    if !list.contains(&item) {
        list.push(item);
    }
}

pub(crate) fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::tests::fake_pe;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn unreal_games_get_optiscaler_next_to_the_shipping_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("Hogwarts.exe"), b"stub");
        write(
            &root.join("Engine/Binaries/Win64/CrashReportClient-Win64-Shipping.exe"),
            b"x",
        );
        write(
            &root.join("Phoenix/Binaries/Win64/HogwartsLegacy-Win64-Shipping.exe"),
            b"x",
        );
        assert_eq!(
            target_dir(root, Some("Hogwarts.exe")),
            "Phoenix/Binaries/Win64"
        );

        // Unreal without the -Shipping name, behind a root stub (FF7 Rebirth).
        let ff7 = tempfile::tempdir().unwrap();
        write(&ff7.path().join("ff7rebirth.exe"), b"stub");
        write(
            &ff7.path()
                .join("Engine/Binaries/Win64/CrashReportClient.exe"),
            b"x",
        );
        write(
            &ff7.path().join("End/Binaries/Win64/ff7rebirth_.exe"),
            &[0u8; 64],
        );
        write(
            &ff7.path().join("End/Binaries/Win64/CrashReporter.exe"),
            &[0u8; 128],
        );
        assert_eq!(
            target_dir(ff7.path(), Some("ff7rebirth.exe")),
            "End/Binaries/Win64"
        );

        let other = tempfile::tempdir().unwrap();
        write(&other.path().join("bin/x64/game.exe"), b"x");
        assert_eq!(
            target_dir(other.path(), Some("bin/x64/game.exe")),
            "bin/x64"
        );
        assert_eq!(target_dir(other.path(), Some("game.exe")), "");
    }

    #[test]
    fn finds_upscalers_with_versions_and_anti_cheat() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            &root.join(
                "Engine/Plugins/Runtime/Nvidia/DLSS/Binaries/ThirdParty/Win64/nvngx_dlss.dll",
            ),
            &fake_pe([310, 2, 1, 0], "NVIDIA DLSS"),
        );
        write(
            &root.join("Game/Binaries/Win64/LibXess.dll"),
            &fake_pe([1, 3, 0, 0], "XeSS"),
        );
        write(&root.join("EasyAntiCheat/Settings.json"), b"{}");
        write(
            &root.join(".tatu-patches/optiscaler/backup/nvngx_dlss.dll"),
            b"backup",
        );

        let scan = scan(root);
        let found: Vec<(&str, &str, Option<&str>)> = scan
            .upscalers
            .iter()
            .map(|f| (f.kind, f.rel.as_str(), f.version.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                (
                    "dlss",
                    "Engine/Plugins/Runtime/Nvidia/DLSS/Binaries/ThirdParty/Win64/nvngx_dlss.dll",
                    Some("310.2.1.0")
                ),
                ("xess", "Game/Binaries/Win64/LibXess.dll", Some("1.3.0.0")),
            ]
        );
        assert_eq!(scan.anti_cheat, ["EasyAntiCheat"]);
    }

    #[test]
    fn proxy_names_taken_by_game_files_or_another_optiscaler() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            &root.join("bin/DXGI.dll"),
            &fake_pe([1, 0, 0, 0], "ReShade"),
        );
        write(
            &root.join("bin/winmm.dll"),
            &fake_pe([0, 9, 4, 0], "OptiScaler.dll"),
        );
        let proxies = proxies(root, "bin");
        assert_eq!(proxies[0].name, "dxgi.dll");
        assert_eq!(proxies[0].taken, Some("un archivo del juego o de otro mod"));
        assert_eq!(proxies[1].taken, Some("OptiScaler"));
        assert_eq!(proxies[2].taken, None);
        assert_eq!(foreign_optiscaler(&proxies), Some("winmm.dll"));
    }

    #[test]
    fn an_old_optiscaler_under_a_legacy_name_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("bin/NVNGX.dll"),
            &fake_pe([0, 7, 0, 0], "OptiScaler.dll"),
        );
        assert_eq!(legacy_optiscaler(tmp.path(), "bin"), Some("nvngx.dll"));
        let clean = tempfile::tempdir().unwrap();
        write(
            &clean.path().join("nvngx.dll"),
            &fake_pe([1, 0, 0, 0], "NVIDIA"),
        );
        assert_eq!(legacy_optiscaler(clean.path(), ""), None);
    }
}
