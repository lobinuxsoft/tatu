//! OptiScaler installed the way the release's own setup scripts do it —
//! every file of the release into the folder of the exe that runs,
//! OptiScaler.dll renamed to a DLL the game loads, spoofing off when the user
//! doesn't want DLSS inputs on a non-NVIDIA GPU — but through [`patch`]: each
//! game file it replaces is backed up and the whole install reverts. The
//! release's own uninstaller can't do that; it leaves the FSR/XeSS DLLs it
//! overwrote.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::detect::{foreign_optiscaler, join_rel, legacy_optiscaler, proxies};
use crate::ini::{self, IniOption};
use crate::package::{NOT_INSTALLED, VERSION};
use crate::patch::{self, Plan, Source};
use crate::{UpscalerError, io_err, pe};

pub const PATCH_ID: &str = "optiscaler";

/// Names the game loads OptiScaler as, in the release scripts' order (the
/// first free one is the default). The scripts also offer `OptiScaler.asi`,
/// which needs an ASI loader the game may not have — left out.
pub const PROXIES: [&str; 7] = [
    "dxgi.dll",
    "winmm.dll",
    "version.dll",
    "dbghelp.dll",
    "d3d12.dll",
    "wininet.dll",
    "winhttp.dll",
];

/// The ini keys picking the upscaler per graphics API.
pub const UPSCALER_KEYS: [&str; 3] = ["Dx12Upscaler", "Dx11Upscaler", "VulkanUpscaler"];

/// Frame generation: where OptiScaler takes the frames from, and what
/// generates the new ones.
pub const FG_KEYS: [&str; 2] = ["FGInput", "FGOutput"];

/// Written by OptiScaler, fakenvapi and dlssg-to-fsr3 while the game runs —
/// the release's own uninstaller removes these too.
const RUNTIME: &[&str] = &[
    "OptiScaler.log",
    "fakenvapi.log",
    "dlssg_to_fsr3.log",
    "DlssOverrides",
];

/// Rewritten by the programs themselves (saved from OptiScaler's menu), so
/// ours whatever their content by the time of a revert.
const OWNED: &[&str] = &["OptiScaler.ini", "fakenvapi.ini"];

/// Third-party runtimes the release bundles. When the game already ships the
/// same or a newer version, the game's stays.
const THIRD_PARTY: &[&str] = &["amd_fidelityfx_", "libxess", "libxell"];

#[derive(Debug, Clone, Deserialize)]
pub struct InstallOptions {
    pub proxy: String,
    /// `Dx12Upscaler` etc. → code; absent or `auto` keeps OptiScaler's default.
    #[serde(default)]
    pub upscalers: BTreeMap<String, String>,
    /// Non-NVIDIA GPU: keep DLSS inputs (spoofing) on — the scripts' default.
    pub dlss_inputs: bool,
    pub nvidia: bool,
    /// The user's own OptiScaler build instead of the release's DLL; an
    /// OptiScaler.ini next to it is used too.
    pub custom_dll: Option<PathBuf>,
    /// OptiScaler's own frame generation — also on GPUs the game's DLSS FG
    /// refuses (RTX 20/30, AMD, Intel): OptiScaler already reports an RTX
    /// 4090 to Streamline, so the game offers its DLSS FG button and
    /// OptiScaler generates the frames instead.
    #[serde(default)]
    pub frame_gen: Option<FrameGen>,
    /// Look for FSR 2/3 compiled into the exe (no DLL to hook): OptiScaler's
    /// pattern matching, off by default because it slows the game's start.
    #[serde(default)]
    pub fsr_pattern: bool,
    /// Image settings ([`crate::settings`]).
    #[serde(default)]
    pub settings: crate::settings::Settings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FrameGen {
    /// `FGInput`: the game's DLSS FG (`dlssg`), its FSR FG (`fsrfg`), any
    /// upscaler (`upscaler`)…
    pub input: String,
    /// `FGOutput`: FSR FG (`fsrfg` — FSR 4 on RDNA4, FSR 3.1 on everything
    /// else, OptiScaler's own choice), XeSS FG (`xefg`)…
    pub output: String,
}

/// The codes each upscaler and frame generation key accepts, per the ini
/// that would be installed.
pub fn options(files: &Path, custom_dll: Option<&Path>) -> BTreeMap<&'static str, Vec<IniOption>> {
    let text = fs::read_to_string(ini_source(files, custom_dll)).unwrap_or_default();
    let mut options: BTreeMap<&'static str, Vec<IniOption>> = UPSCALER_KEYS
        .iter()
        .chain(FG_KEYS.iter())
        .map(|key| (*key, ini::codes_for_key(&text, key)))
        .collect();
    // The settings' choices, from the same ini.
    let choices = crate::settings::choices(&text);
    options.insert("DlssPreset", choices.dlss_presets);
    options.insert("DlssdPreset", choices.dlssd_presets);
    options.insert("FsrUpscaler", choices.fsr_upscalers);
    options.insert("FsrFg", choices.fsr_fg);
    options
}

/// The patch installing OptiScaler into `target_rel` (under the game `root`)
/// from the extracted release in `files`. The edited OptiScaler.ini is
/// written into `staging`, which must outlive [`patch::apply`]. `running`:
/// the game's exes (see [`patch::game_exes`]). `files` must have passed
/// [`crate::package::verify`].
pub fn plan(
    root: &Path,
    target_rel: &str,
    files: &Path,
    opts: &InstallOptions,
    staging: &Path,
    running: Vec<String>,
) -> Result<Plan, UpscalerError> {
    if !PROXIES.contains(&opts.proxy.as_str()) {
        return Err(UpscalerError::Invalid(format!(
            "Nombre de DLL no válido: {}",
            opts.proxy
        )));
    }
    if let Some(dll) = &opts.custom_dll
        && !pe::is_optiscaler(dll)
    {
        return Err(UpscalerError::Invalid(format!(
            "{} no es un OptiScaler.dll",
            dll.display()
        )));
    }

    if let Some(old) = legacy_optiscaler(root, target_rel) {
        return Err(UpscalerError::Invalid(format!(
            "Hay un OptiScaler de una versión vieja instalado como {old}: quitalo antes de instalar \
             (si no, se cargarían dos)"
        )));
    }
    let proxies = proxies(root, target_rel);
    match foreign_optiscaler(&proxies) {
        Some(foreign) if foreign != opts.proxy => {
            return Err(UpscalerError::Invalid(format!(
                "Ya hay un OptiScaler instalado como {foreign} (no por Tatu). Instalá con ese mismo \
                 nombre para reemplazarlo (queda respaldado), o quitalo primero"
            )));
        }
        Some(_) => {}
        None => {
            if let Some(taken) = proxies
                .iter()
                .find(|p| p.name == opts.proxy)
                .and_then(|p| p.taken)
            {
                return Err(UpscalerError::Invalid(format!(
                    "{} ya existe en la carpeta del juego ({taken}) — elegí otro nombre",
                    opts.proxy
                )));
            }
        }
    }

    let ini_path = staging.join("OptiScaler.ini");
    fs::create_dir_all(staging).map_err(|e| io_err(staging, e))?;
    fs::write(&ini_path, edited_ini(files, opts)?).map_err(|e| io_err(&ini_path, e))?;

    let mut sources = Vec::new();
    for (from, name) in release_files(files)? {
        if NOT_INSTALLED.contains(&name.as_str()) {
            continue;
        }
        let (from, dest) = match name.as_str() {
            "OptiScaler.dll" => (opts.custom_dll.clone().unwrap_or(from), opts.proxy.clone()),
            "OptiScaler.ini" => (ini_path.clone(), name.clone()),
            _ => (from, name.clone()),
        };
        let rel = join_rel(target_rel, &dest);
        let lower = name.to_lowercase();
        if THIRD_PARTY.iter().any(|p| lower.starts_with(p)) {
            let (existing, _) = patch::resolve_ci(root, &rel);
            if existing.is_file() && pe::version_newer_or_equal(&existing, &from) {
                continue;
            }
        }
        sources.push(Source {
            from,
            rel,
            owned: OWNED.contains(&name.as_str()),
        });
    }

    // The DLL the game loads goes in last: until then, a half-finished
    // install is files nothing loads.
    let proxy_at = sources
        .iter()
        .position(|s| s.rel == join_rel(target_rel, &opts.proxy));
    if let Some(at) = proxy_at {
        let proxy = sources.remove(at);
        sources.push(proxy);
    }

    Ok(Plan {
        id: PATCH_ID.to_string(),
        sources,
        runtime: RUNTIME.iter().map(|r| join_rel(target_rel, r)).collect(),
        meta: serde_json::json!({
            "version": if opts.custom_dll.is_some() { "build propio" } else { VERSION },
            "proxy": opts.proxy,
            "target": target_rel,
            "frame_gen": opts.frame_gen.as_ref().map(|fg| format!("{} → {}", fg.input, fg.output)),
            "fsr_pattern": opts.fsr_pattern,
        }),
        running,
    })
}

/// Whether reverting patch `id` would pull DLLs from under the installed
/// OptiScaler: an FSR or XeSS update in the folder OptiScaler went into.
/// OptiScaler kept the update's DLLs instead of placing its own (they were
/// as new — see [`plan`]), and the revert takes them back to the game's
/// older ones, the FSR providers gone.
pub fn relies_on(root: &Path, id: &str) -> bool {
    if id == PATCH_ID {
        return false;
    }
    let (Ok(Some(opti)), Ok(Some(other))) = (patch::load(root, PATCH_ID), patch::load(root, id))
    else {
        return false;
    };
    let target = opti.meta["target"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    let folder = |rel: &str| {
        rel.rsplit_once('/')
            .map_or("", |(dir, _)| dir)
            .to_lowercase()
    };
    matches!(other.meta["kind"].as_str(), Some("fsr" | "xess"))
        && other.entries.iter().any(|e| folder(&e.rel) == target)
}

fn edited_ini(files: &Path, opts: &InstallOptions) -> Result<String, UpscalerError> {
    let source = ini_source(files, opts.custom_dll.as_deref());
    let original = fs::read_to_string(&source).map_err(|e| io_err(&source, e))?;
    let mut text = original.clone();
    let missing = |key: &str| {
        UpscalerError::Invalid(format!("{} no tiene la opción {key}", source.display()))
    };
    // A code must be one the ini documents for that key (when it documents
    // any: a custom build's ini might not), and plain.
    let documented = |key: &str, code: &str| {
        let codes = ini::codes_for_key(&original, key);
        codes.is_empty() || codes.iter().any(|o| o.code == code)
    };
    let valid_code = |key: &str, code: &str| {
        !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && documented(key, code)
    };
    for (key, code) in &opts.upscalers {
        if code == "auto" {
            continue;
        }
        if !UPSCALER_KEYS.contains(&key.as_str()) || !valid_code(key, code) {
            return Err(UpscalerError::Invalid(format!(
                "Upscaler no válido: {key}={code}"
            )));
        }
        text = ini::set_key(&text, "Upscalers", key, code).ok_or_else(|| missing(key))?;
    }
    if let Some(fg) = &opts.frame_gen {
        for (key, code) in [("FGInput", &fg.input), ("FGOutput", &fg.output)] {
            if !valid_code(key, code) || code == "nofg" {
                return Err(UpscalerError::Invalid(format!(
                    "Frame generation no válido: {key}={code}"
                )));
            }
            text = ini::set_key(&text, "FrameGen", key, code).ok_or_else(|| missing(key))?;
        }
        text =
            ini::set_key(&text, "FrameGen", "Enabled", "true").ok_or_else(|| missing("Enabled"))?;
    }
    if opts.fsr_pattern {
        for key in ["Fsr2Pattern", "Fsr3Pattern"] {
            text = ini::set_key(&text, "Inputs", key, "true").ok_or_else(|| missing(key))?;
        }
    }
    // The release's scripts: DLSS frame generation needs the spoofing that
    // "DLSS inputs" turns off on AMD/Intel — it wins.
    let fg_needs_dlss = opts
        .frame_gen
        .as_ref()
        .is_some_and(|fg| fg.input == "dlssg" || fg.input == "nukems");
    if !opts.nvidia && !opts.dlss_inputs && !fg_needs_dlss {
        text = ini::set_key(&text, "Spoofing", "Dxgi", "false").ok_or_else(|| missing("Dxgi"))?;
    }
    crate::settings::apply(&text, &opts.settings)
}

fn ini_source(files: &Path, custom_dll: Option<&Path>) -> PathBuf {
    custom_dll
        .and_then(Path::parent)
        .map(|dir| dir.join("OptiScaler.ini"))
        .filter(|ini| ini.is_file())
        .unwrap_or_else(|| files.join("OptiScaler.ini"))
}

/// Every file of the extracted release: (path, `/`-separated name under it).
fn release_files(files: &Path) -> Result<Vec<(PathBuf, String)>, UpscalerError> {
    let mut out = Vec::new();
    let mut pending = vec![files.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| io_err(&dir, e))?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let rel = path
                    .strip_prefix(files)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((path, rel));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pe::tests::fake_pe;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    const INI: &str = "[Upscalers]\r\n; xess, fsr21, fsr22, fsr31 (also for FSR4), dlss - Default (auto) is xess\r\nDx12Upscaler=auto\r\n[Spoofing]\r\nDxgi=auto\r\n[FrameGen]\r\nEnabled=auto\r\n; nofg, dlssg, nukems, fsrfg, upscaler, fsrfg30 - Default (auto) is nofg\r\nFGInput=auto\r\n; xefg   - requires libxess_fg.dll, libxell.dll and latest fakenvapi dll\r\n; nofg, fsrfg, xefg, nukems - Default (auto) is nofg\r\nFGOutput=auto\r\n[Inputs]\r\nFsr2Pattern=auto\r\nFsr3Pattern=auto\r\n";

    fn release(dir: &Path) {
        write(
            &dir.join("OptiScaler.dll"),
            &fake_pe([0, 9, 4, 0], "OptiScaler.dll"),
        );
        write(&dir.join("OptiScaler.ini"), INI.as_bytes());
        write(&dir.join("libxess.dll"), &fake_pe([2, 0, 0, 0], "XeSS"));
        write(
            &dir.join("amd_fidelityfx_dx12.dll"),
            &fake_pe([2, 0, 0, 0], "FidelityFX"),
        );
        write(&dir.join("D3D12_Optiscaler/D3D12Core.dll"), b"core");
        write(&dir.join("setup_linux.sh"), b"#!/bin/sh");
    }

    fn opts(proxy: &str) -> InstallOptions {
        InstallOptions {
            proxy: proxy.into(),
            upscalers: BTreeMap::from([("Dx12Upscaler".to_string(), "fsr31".to_string())]),
            dlss_inputs: false,
            nvidia: false,
            custom_dll: None,
            frame_gen: None,
            fsr_pattern: false,
            settings: Default::default(),
        }
    }

    #[test]
    fn installs_like_the_setup_scripts_and_keeps_newer_game_dlls() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        let root = tmp.path().join("game");
        write(&root.join("bin/game.exe"), b"exe");
        write(
            &root.join("bin/libxess.dll"),
            &fake_pe([3, 0, 0, 0], "XeSS"),
        ); // newer: stays
        write(
            &root.join("bin/amd_fidelityfx_dx12.dll"),
            &fake_pe([1, 0, 0, 0], "FFX"),
        ); // older: replaced

        let plan = plan(
            &root,
            "bin",
            &files,
            &opts("dxgi.dll"),
            &tmp.path().join("staging"),
            Vec::new(),
        )
        .unwrap();
        let rels: Vec<&str> = plan.sources.iter().map(|s| s.rel.as_str()).collect();
        // No libxess (the game's is newer), no setup script, and the DLL the
        // game loads last: a half-finished install is inert.
        assert_eq!(
            rels,
            [
                "bin/D3D12_Optiscaler/D3D12Core.dll",
                "bin/OptiScaler.ini",
                "bin/amd_fidelityfx_dx12.dll",
                "bin/dxgi.dll"
            ]
        );

        let manifest = patch::apply(&root, &plan).unwrap();
        assert_eq!(
            manifest.backed_up(),
            vec!["bin/amd_fidelityfx_dx12.dll".to_string()]
        );
        let ini = fs::read_to_string(root.join("bin/OptiScaler.ini")).unwrap();
        assert!(ini.contains("Dx12Upscaler=fsr31\r\n") && ini.contains("Dxgi=false\r\n"));
        assert!(pe::is_optiscaler(&root.join("bin/dxgi.dll")));

        assert!(patch::revert(&root, PATCH_ID).unwrap().complete());
        assert_eq!(
            pe::file_version(&root.join("bin/amd_fidelityfx_dx12.dll")).as_deref(),
            Some("1.0.0.0")
        );
        assert!(!root.join("bin/dxgi.dll").exists());
    }

    #[test]
    fn proxy_names_taken_by_the_game_are_refused_but_a_manual_optiscaler_is_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        let root = tmp.path().join("game");
        write(&root.join("dxgi.dll"), &fake_pe([6, 0, 0, 0], "ReShade"));
        let staging = tmp.path().join("staging");
        assert!(plan(&root, "", &files, &opts("dxgi.dll"), &staging, Vec::new()).is_err());
        assert!(plan(&root, "", &files, &opts("winmm.dll"), &staging, Vec::new()).is_ok());

        let manual = tempfile::tempdir().unwrap();
        write(
            &manual.path().join("version.dll"),
            &fake_pe([0, 9, 0, 0], "OptiScaler.dll"),
        );
        assert!(
            plan(
                manual.path(),
                "",
                &files,
                &opts("dxgi.dll"),
                &staging,
                Vec::new()
            )
            .is_err()
        );
        let plan = plan(
            manual.path(),
            "",
            &files,
            &opts("version.dll"),
            &staging,
            Vec::new(),
        )
        .unwrap();
        let manifest = patch::apply(manual.path(), &plan).unwrap();
        assert_eq!(manifest.backed_up(), vec!["version.dll".to_string()]);
    }

    #[test]
    fn nvidia_or_dlss_inputs_keep_spoofing_on() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        let mut o = opts("dxgi.dll");
        o.nvidia = true;
        assert!(edited_ini(&files, &o).unwrap().contains("Dxgi=auto"));
        o.nvidia = false;
        o.dlss_inputs = true;
        assert!(edited_ini(&files, &o).unwrap().contains("Dxgi=auto"));
        o.upscalers
            .insert("Dx12Upscaler".into(), "fsr31; rm".into());
        assert!(edited_ini(&files, &o).is_err());
    }

    #[test]
    fn fsr_compiled_into_the_exe_turns_on_pattern_matching() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        let mut o = opts("dxgi.dll");
        assert!(
            edited_ini(&files, &o)
                .unwrap()
                .contains("Fsr2Pattern=auto\r\n")
        );
        o.fsr_pattern = true;
        let ini = edited_ini(&files, &o).unwrap();
        assert!(ini.contains("[Inputs]\r\nFsr2Pattern=true\r\nFsr3Pattern=true\r\n"));
    }

    #[test]
    fn frame_generation_lands_in_its_section() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        let codes: Vec<String> = options(&files, None)["FGOutput"]
            .iter()
            .map(|o| o.code.clone())
            .collect();
        assert_eq!(codes, ["nofg", "fsrfg", "xefg", "nukems"]);

        let mut o = opts("dxgi.dll");
        o.frame_gen = Some(FrameGen {
            input: "dlssg".into(),
            output: "fsrfg".into(),
        });
        let ini = edited_ini(&files, &o).unwrap();
        assert!(ini.contains("[FrameGen]\r\nEnabled=true\r\n"));
        assert!(ini.contains("FGInput=dlssg\r\n") && ini.contains("FGOutput=fsrfg\r\n"));
        // Only [FrameGen]'s Enabled: nothing else in the file changes.
        assert_eq!(ini.matches("Enabled=true").count(), 1);

        o.frame_gen = Some(FrameGen {
            input: "dlssg".into(),
            output: "nofg".into(),
        });
        assert!(edited_ini(&files, &o).is_err());
    }

    #[test]
    fn settings_chosen_at_install_land_in_the_game_and_edits_revert_cleanly() {
        use crate::settings::{self, Settings};
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        // The real release ini, not the trimmed one.
        write(
            &files.join("OptiScaler.ini"),
            include_bytes!("../tests/fixtures/OptiScaler-0.9.4.ini"),
        );
        let root = tmp.path().join("game");
        write(&root.join("bin/game.exe"), b"exe");
        let before = fs::read_dir(root.join("bin")).unwrap().count();

        let chosen = Settings {
            dlss_quality_presets: BTreeMap::from([("Quality".into(), 11)]),
            fsr_upscaler: Some(1),
            sharpness: Some(0.4),
            anisotropy: Some(16),
            ..Default::default()
        };
        let mut o = opts("dxgi.dll");
        o.settings = chosen.clone();
        let plan = plan(
            &root,
            "bin",
            &files,
            &o,
            &tmp.path().join("staging"),
            Vec::new(),
        )
        .unwrap();
        patch::apply(&root, &plan).unwrap();
        let installed = fs::read_to_string(root.join("bin/OptiScaler.ini")).unwrap();
        assert_eq!(settings::read(&installed), chosen);
        assert!(
            installed.contains("Dx12Upscaler=fsr31\r\n"),
            "the other edits too"
        );

        // Changed later, without reinstalling.
        let later = Settings {
            dlss_preset: Some(12),
            fps_limit: Some(60.0),
            ..Default::default()
        };
        settings::save(&root, "bin", &later).unwrap();
        let saved = fs::read_to_string(root.join("bin/OptiScaler.ini")).unwrap();
        assert_eq!(settings::read(&saved), later);

        // Revert takes OptiScaler's ini out even though it changed since.
        assert!(patch::revert(&root, PATCH_ID).unwrap().complete());
        assert!(!root.join("bin/OptiScaler.ini").exists());
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), before);
    }

    /// The install form's options (frame generation, FSR in the exe,
    /// spoofing), through `plan` on the real release ini, read the way
    /// OptiScaler reads them.
    #[test]
    fn install_options_reach_optiscaler_as_meant() {
        use crate::test_support::{Opti, RELEASE_INI};
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        write(&files.join("OptiScaler.ini"), RELEASE_INI.as_bytes());
        let root = tmp.path().join("game");
        write(&root.join("game.exe"), b"exe");
        let staged = |o: &InstallOptions| {
            let staging = tmp.path().join("staging");
            let plan = plan(&root, "", &files, o, &staging, Vec::new()).unwrap();
            let ini = plan
                .sources
                .iter()
                .find(|s| s.rel == "OptiScaler.ini")
                .unwrap();
            Opti::load(&fs::read_to_string(&ini.from).unwrap())
        };

        // AMD, the game's DLSS FG button as the source: spoofing must stay on
        // even with DLSS inputs off, or the game hides the button.
        let mut o = opts("dxgi.dll");
        o.frame_gen = Some(FrameGen {
            input: "dlssg".into(),
            output: "fsrfg".into(),
        });
        o.fsr_pattern = true;
        let opti = staged(&o);
        assert_eq!(opti.boolean("FrameGen", "Enabled"), Some(true));
        assert_eq!(opti.string("FrameGen", "FGInput"), Some("dlssg"));
        assert_eq!(opti.string("FrameGen", "FGOutput"), Some("fsrfg"));
        assert_eq!(opti.boolean("Inputs", "Fsr2Pattern"), Some(true));
        assert_eq!(opti.boolean("Inputs", "Fsr3Pattern"), Some(true));
        assert_eq!(
            opti.boolean("Spoofing", "Dxgi"),
            None,
            "OptiScaler's default: on"
        );
        assert_eq!(opti.string("Upscalers", "Dx12Upscaler"), Some("fsr31"));

        // Frame gen from the upscaler, DLSS inputs off on AMD: no spoofing.
        o.frame_gen = Some(FrameGen {
            input: "upscaler".into(),
            output: "xefg".into(),
        });
        o.fsr_pattern = false;
        let opti = staged(&o);
        assert_eq!(opti.string("FrameGen", "FGInput"), Some("upscaler"));
        assert_eq!(opti.string("FrameGen", "FGOutput"), Some("xefg"));
        assert_eq!(opti.boolean("Inputs", "Fsr2Pattern"), None);
        assert_eq!(opti.boolean("Spoofing", "Dxgi"), Some(false));

        // NVIDIA: nothing to spoof, OptiScaler's defaults stay.
        let mut nvidia = opts("dxgi.dll");
        nvidia.nvidia = true;
        let opti = staged(&nvidia);
        assert_eq!(opti.boolean("Spoofing", "Dxgi"), None);
        assert_eq!(opti.boolean("FrameGen", "Enabled"), None);

        // A code the release's ini doesn't document is refused.
        let mut bad = opts("dxgi.dll");
        bad.upscalers.insert("Dx12Upscaler".into(), "fsr99".into());
        assert!(
            plan(
                &root,
                "",
                &files,
                &bad,
                &tmp.path().join("staging"),
                Vec::new()
            )
            .is_err()
        );
    }

    #[test]
    fn an_update_optiscaler_relies_on_is_kept_until_optiscaler_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("release");
        release(&files);
        let root = tmp.path().join("game");
        write(&root.join("bin/game.exe"), b"exe");
        write(
            &root.join("bin/amd_fidelityfx_dx12.dll"),
            &fake_pe([1, 0, 1, 0], "FFX"),
        );
        // The game's FSR updated first: the release's loader is then "as
        // new", and OptiScaler's install keeps it instead of its own.
        let update =
            crate::official::update_for("bin/amd_fidelityfx_dx12.dll", Some("1.0.1.0")).unwrap();
        let fetched: Vec<(PathBuf, String)> = crate::official::placements(&root, &update)
            .into_iter()
            .map(|(name, dest)| {
                let from = tmp.path().join("cache").join(name);
                write(&from, &fake_pe([2, 3, 0, 2740], "FidelityFX"));
                (from, dest)
            })
            .collect();
        let update = crate::official::plan(&root, &update, fetched, Vec::new());
        patch::apply(&root, &update).unwrap();
        let installed = plan(
            &root,
            "bin",
            &files,
            &opts("dxgi.dll"),
            &tmp.path().join("staging"),
            Vec::new(),
        )
        .unwrap();
        assert!(
            installed
                .sources
                .iter()
                .all(|s| !s.rel.contains("fidelityfx")),
            "OptiScaler keeps the updated FSR"
        );
        patch::apply(&root, &installed).unwrap();

        assert!(relies_on(&root, &update.id));
        assert!(!relies_on(&root, PATCH_ID));
        assert_eq!(
            patch::owner(&root, "BIN/AMD_FIDELITYFX_DX12.DLL").as_deref(),
            Some(update.id.as_str())
        );
        patch::revert(&root, PATCH_ID).unwrap();
        assert!(
            !relies_on(&root, &update.id),
            "OptiScaler gone: free to revert"
        );
    }
}
