//! Image settings Tatu offers on top of OptiScaler's in-game menu, each a few
//! keys of OptiScaler.ini: written at install, and editable afterwards
//! without reinstalling. OptiScaler.ini is OptiScaler's own file, so revert
//! removes it whatever it says by then.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::detect::join_rel;
use crate::ini::{self, IniOption};
use crate::{UpscalerError, io_err, patch};

/// DLSS's quality modes, as OptiScaler's per-mode keys name them
/// (`RenderPreset<mode>`).
pub const QUALITIES: [&str; 6] = [
    "DLAA",
    "UltraQuality",
    "Quality",
    "Balanced",
    "Performance",
    "UltraPerformance",
];

/// `None` (or an empty map) leaves OptiScaler's default (`auto`) — or puts it
/// back. Presets and FSR versions are codes the ini itself documents
/// ([`choices`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// DLSS render preset for every quality mode (`1` = A…). Only presets
    /// the game's DLSS version has do anything.
    pub dlss_preset: Option<u32>,
    /// Or one per quality mode ([`QUALITIES`] → preset); modes left out keep
    /// the game's. Not together with `dlss_preset`.
    pub dlss_quality_presets: BTreeMap<String, u32>,
    /// DLSS Ray Reconstruction's preset, for every quality mode.
    pub dlssd_preset: Option<u32>,
    /// Which FSR OptiScaler runs when it's the upscaler (`0` = FSR 4.0.2 in
    /// 0.9.4…); `None`: FSR 4 on RDNA 4, FSR 3.1 elsewhere.
    pub fsr_upscaler: Option<u8>,
    /// Which FSR frame generation it runs, the same way.
    pub fsr_fg: Option<u8>,
    /// RCAS sharpening on top of the upscaler, 0.0–1.3.
    pub sharpness: Option<f32>,
    /// Fixed upscale ratio for every quality mode: 1.0 renders at native
    /// resolution (DLAA-like), 2.0 at half, up to 3.0 (0.1–6.0 with
    /// OptiScaler's extended limits).
    pub upscale_ratio: Option<f32>,
    /// Output scaling: upscale to this multiple of the screen and scale back
    /// down (sharper, costlier), 0.5–3.0. DX12, and DX11 through DX12.
    pub output_scaling: Option<f32>,
    /// Frame rate cap through Reflex (the game must support Reflex; on AMD
    /// and Intel, fakenvapi provides it).
    pub fps_limit: Option<f32>,
    /// Anisotropic filtering forced on textures, 1–16 (the menu offers 1, 2,
    /// 4, 8, 16).
    pub anisotropy: Option<u8>,
}

/// The codes an OptiScaler.ini documents for the settings that take one.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Choices {
    /// Without `0` (the game's own): that's "no override".
    pub dlss_presets: Vec<IniOption>,
    pub dlssd_presets: Vec<IniOption>,
    pub fsr_upscalers: Vec<IniOption>,
    pub fsr_fg: Vec<IniOption>,
}

pub fn choices(text: &str) -> Choices {
    let presets = |section| {
        let mut list = ini::numbered_options(text, section, "RenderPresetForAll");
        list.retain(|o| o.code != "0");
        list
    };
    Choices {
        dlss_presets: presets("DLSS"),
        dlssd_presets: presets("DLSSD"),
        fsr_upscalers: ini::numbered_options(text, "FSR", "UpscalerIndex"),
        fsr_fg: ini::numbered_options(text, "FSR", "FGIndex"),
    }
}

/// `text` (an OptiScaler.ini) with `settings` written into it.
///
/// Only the settings that differ from what the ini has now ([`read`]) are
/// checked and written: everything else — values set from OptiScaler's own
/// menu, including ones Tatu doesn't model (sharpening without RCAS,
/// per-mode Ray Reconstruction presets, extended limits) — stays exactly as
/// it is. What does change must be something this ini documents and
/// OptiScaler accepts.
pub fn apply(text: &str, settings: &Settings) -> Result<String, UpscalerError> {
    let s = settings;
    let now = read(text);
    let invalid = |what: &str| {
        Err(UpscalerError::Invalid(format!(
            "Valor fuera de rango: {what}"
        )))
    };
    let available = choices(text);
    let offered = |list: &[IniOption], v: Option<u32>| {
        v.is_none_or(|v| list.iter().any(|o| o.code == v.to_string()))
    };
    let code = |v: Option<u8>| v.map(u32::from);
    // OptiScaler's own limits (Config.cpp clamps, its menu's sliders).
    let in_range = |v: Option<f32>, lo: f32, hi: f32| {
        v.is_none_or(|v| v.is_finite() && (lo..=hi).contains(&v))
    };

    let flag = |on: bool| if on { "true" } else { "auto" }.to_string();
    let value = |v: Option<u32>| v.map_or_else(|| "auto".to_string(), |v| v.to_string());
    let float = |v: Option<f32>| v.map_or_else(|| "auto".to_string(), |v| v.to_string());
    let mut keys: Vec<(&str, String, String)> = Vec::new();

    if s.dlss_preset != now.dlss_preset || s.dlss_quality_presets != now.dlss_quality_presets {
        if !offered(&available.dlss_presets, s.dlss_preset)
            || s.dlss_quality_presets
                .values()
                .any(|p| !offered(&available.dlss_presets, Some(*p)))
        {
            return invalid("preset de DLSS");
        }
        if s.dlss_preset.is_some() && !s.dlss_quality_presets.is_empty() {
            return invalid(
                "preset de DLSS: uno para todas las calidades o uno por calidad, no los dos",
            );
        }
        if s.dlss_quality_presets
            .keys()
            .any(|q| !QUALITIES.contains(&q.as_str()))
        {
            return invalid("calidad de DLSS");
        }
        let on = s.dlss_preset.is_some() || !s.dlss_quality_presets.is_empty();
        keys.push(("DLSS", "RenderPresetOverride".into(), flag(on)));
        keys.push(("DLSS", "RenderPresetForAll".into(), value(s.dlss_preset)));
        for quality in QUALITIES {
            keys.push((
                "DLSS",
                format!("RenderPreset{quality}"),
                value(s.dlss_quality_presets.get(quality).copied()),
            ));
        }
    }
    if s.dlssd_preset != now.dlssd_preset {
        if !offered(&available.dlssd_presets, s.dlssd_preset) {
            return invalid("preset de Ray Reconstruction");
        }
        keys.push((
            "DLSSD",
            "RenderPresetOverride".into(),
            flag(s.dlssd_preset.is_some()),
        ));
        keys.push(("DLSSD", "RenderPresetForAll".into(), value(s.dlssd_preset)));
    }
    if s.fsr_upscaler != now.fsr_upscaler {
        if !offered(&available.fsr_upscalers, code(s.fsr_upscaler)) {
            return invalid("versión de FSR");
        }
        keys.push(("FSR", "UpscalerIndex".into(), value(code(s.fsr_upscaler))));
    }
    if s.fsr_fg != now.fsr_fg {
        if !offered(&available.fsr_fg, code(s.fsr_fg)) {
            return invalid("versión del frame generation de FSR");
        }
        keys.push(("FSR", "FGIndex".into(), value(code(s.fsr_fg))));
    }
    if s.sharpness != now.sharpness {
        if !in_range(s.sharpness, 0.0, 1.3) {
            return invalid("nitidez");
        }
        let on = s.sharpness.is_some();
        keys.push(("CAS", "Enabled".into(), flag(on)));
        keys.push(("Sharpness", "OverrideSharpness".into(), flag(on)));
        keys.push(("Sharpness", "Sharpness".into(), float(s.sharpness)));
    }
    if s.upscale_ratio != now.upscale_ratio {
        if !in_range(s.upscale_ratio, 0.1, 6.0) {
            return invalid("resolución interna");
        }
        keys.push((
            "UpscaleRatio",
            "UpscaleRatioOverrideEnabled".into(),
            flag(s.upscale_ratio.is_some()),
        ));
        keys.push((
            "UpscaleRatio",
            "UpscaleRatioOverrideValue".into(),
            float(s.upscale_ratio),
        ));
    }
    if s.output_scaling != now.output_scaling {
        if !in_range(s.output_scaling, 0.5, 3.0) {
            return invalid("output scaling");
        }
        keys.push((
            "OutputScaling",
            "Enabled".into(),
            flag(s.output_scaling.is_some()),
        ));
        keys.push((
            "OutputScaling",
            "Multiplier".into(),
            float(s.output_scaling),
        ));
    }
    if s.fps_limit != now.fps_limit {
        if !in_range(s.fps_limit, 0.1, 1000.0) {
            return invalid("límite de FPS");
        }
        keys.push(("Framerate", "FramerateLimit".into(), float(s.fps_limit)));
    }
    if s.anisotropy != now.anisotropy {
        if s.anisotropy.is_some_and(|a| !(1..=16).contains(&a)) {
            return invalid("filtrado anisotrópico");
        }
        keys.push((
            "Anisotropy",
            "AnisotropyOverride".into(),
            value(code(s.anisotropy)),
        ));
    }

    let mut text = text.to_string();
    for (section, key, value) in keys {
        match ini::set_key(&text, section, &key, &value) {
            Some(edited) => text = edited,
            // A custom build's ini without the key: fine to leave at default.
            None if value == "auto" => {}
            None => {
                return Err(UpscalerError::Invalid(format!(
                    "Este OptiScaler.ini no tiene la opción [{section}] {key}"
                )));
            }
        }
    }
    Ok(text)
}

/// The settings an OptiScaler.ini has now — as Tatu wrote them, or as the
/// in-game menu saved them — the way OptiScaler takes them: its clamps
/// applied, and a preset for every mode winning over per-mode ones.
pub fn read(text: &str) -> Settings {
    let get = |section, key: &str| ini::get_key(text, section, key);
    let on = |section, key| get(section, key).is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let float = |section, key| {
        get(section, key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
    };
    let code = |section, key: &str| get(section, key).and_then(|v| v.parse::<u8>().ok());
    let preset = |section, key: &str| {
        get(section, key)
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|p| *p > 0)
    };
    let dlss_on = on("DLSS", "RenderPresetOverride");
    let dlss_preset = dlss_on
        .then(|| preset("DLSS", "RenderPresetForAll"))
        .flatten();
    Settings {
        dlss_preset,
        // Only in effect without a preset for every mode (DLSSFeature.cpp:
        // `RenderPresetForAll.value_or(RenderPreset<mode>)`).
        dlss_quality_presets: QUALITIES
            .iter()
            .filter(|_| dlss_on && dlss_preset.is_none())
            .filter_map(|q| Some((q.to_string(), preset("DLSS", &format!("RenderPreset{q}"))?)))
            .collect(),
        dlssd_preset: on("DLSSD", "RenderPresetOverride")
            .then(|| preset("DLSSD", "RenderPresetForAll"))
            .flatten(),
        fsr_upscaler: code("FSR", "UpscalerIndex"),
        fsr_fg: code("FSR", "FGIndex"),
        // OptiScaler's own defaults when the value itself is `auto`, and its
        // clamps (Config.cpp).
        sharpness: on("CAS", "Enabled")
            .then(|| float("Sharpness", "Sharpness").map_or(0.3, |v| v.clamp(0.0, 1.3))),
        upscale_ratio: on("UpscaleRatio", "UpscaleRatioOverrideEnabled")
            .then(|| float("UpscaleRatio", "UpscaleRatioOverrideValue").unwrap_or(1.3)),
        output_scaling: on("OutputScaling", "Enabled")
            .then(|| float("OutputScaling", "Multiplier").map_or(1.5, |v| v.clamp(0.5, 3.0))),
        fps_limit: float("Framerate", "FramerateLimit").filter(|v| *v > 0.0),
        anisotropy: get("Anisotropy", "AnisotropyOverride")
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|a| (1..=16).contains(a)),
    }
}

/// The OptiScaler.ini installed in `target_rel`.
pub fn ini_path(root: &Path, target_rel: &str) -> PathBuf {
    patch::resolve_ci(root, &join_rel(target_rel, "OptiScaler.ini")).0
}

/// Write `settings` into the installed OptiScaler.ini, replacing it in one
/// step (a temporary copy next to it, then a rename).
pub fn save(root: &Path, target_rel: &str, settings: &Settings) -> Result<(), UpscalerError> {
    let path = ini_path(root, target_rel);
    let text = fs::read_to_string(&path).map_err(|e| io_err(&path, e))?;
    let edited = apply(&text, settings)?;
    let tmp = path.with_extension("ini.tatu-tmp");
    let written = fs::write(&tmp, edited).and_then(|()| fs::rename(&tmp, &path));
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(&path, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Opti, RELEASE_INI};

    const INI: &str = "[DLSS]\r\nRenderPresetOverride=auto\r\n\
        ; 0 = Default | 1 = A | 2 = B | 3 = C | 4 = D | 5 = E | 6 = F | 7 = G\r\n\
        ; 8 = H | 9 = I | 10 = J | 11 = K | 12 = L | 13 = M | 14 = N | 15 = O\r\n\
        ; Default (auto) is 0\r\nRenderPresetForAll=auto\r\nRenderPresetDLAA=auto\r\n\
        RenderPresetUltraQuality=auto\r\nRenderPresetQuality=auto\r\nRenderPresetBalanced=auto\r\n\
        RenderPresetPerformance=auto\r\nRenderPresetUltraPerformance=auto\r\n\
        [DLSSD]\r\nRenderPresetOverride=auto\r\n; 0 = Default | 1 = A | 2 = B | 3 = C | 4 = D | 5 = E\r\n\
        RenderPresetForAll=auto\r\n\
        [FSR]\r\n; Selects upscaler backend for FSR3.X/4\r\n; 0 = FSR 4.0.2 | 1 = FSR 3.1.5 | 2 = FSR 2.3.4\r\n\
        ; Depending on GPU - Default (auto) is 0 - FSR4 for RDNA4, FSR3 for the rest\r\nUpscalerIndex=auto\r\n\r\n\
        ; 0 = FSR 4.0.0 | 1 = FSR 3.1.6\r\nFGIndex=auto\r\n\
        [CAS]\r\nEnabled=auto\r\n[Sharpness]\r\nOverrideSharpness=auto\r\nSharpness=auto\r\n\
        [UpscaleRatio]\r\nUpscaleRatioOverrideEnabled=auto\r\nUpscaleRatioOverrideValue=auto\r\n\
        [OutputScaling]\r\nEnabled=auto\r\nMultiplier=auto\r\n[Framerate]\r\nFramerateLimit=auto\r\n\
        [Anisotropy]\r\nAnisotropyOverride=auto\r\n";

    #[test]
    fn the_choices_come_from_the_ini_comments() {
        let c = choices(INI);
        assert_eq!(c.dlss_presets.len(), 15, "A to O, without the game's own");
        assert_eq!(
            (
                c.dlss_presets[10].code.as_str(),
                c.dlss_presets[10].label.as_str()
            ),
            ("11", "K")
        );
        assert_eq!(c.dlssd_presets.len(), 5);
        let fsr: Vec<_> = c.fsr_upscalers.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(fsr, ["FSR 4.0.2", "FSR 3.1.5", "FSR 2.3.4"]);
        assert_eq!(c.fsr_fg.len(), 2);
    }

    #[test]
    fn settings_round_trip_and_turn_back_off() {
        let wanted = Settings {
            dlss_quality_presets: BTreeMap::from([("DLAA".into(), 11), ("Performance".into(), 13)]),
            dlssd_preset: Some(5),
            fsr_upscaler: Some(1),
            fsr_fg: Some(0),
            sharpness: Some(0.5),
            upscale_ratio: Some(1.0),
            output_scaling: Some(1.5),
            fps_limit: Some(60.0),
            anisotropy: Some(16),
            ..Default::default()
        };
        let edited = apply(INI, &wanted).unwrap();
        assert!(edited.contains("RenderPresetDLAA=11\r\n"));
        assert!(edited.contains("RenderPresetForAll=auto\r\nRenderPresetDLAA"));
        assert!(edited.contains("[CAS]\r\nEnabled=true\r\n"));
        assert_eq!(read(&edited), wanted);

        let same_for_all = Settings {
            dlss_preset: Some(11),
            ..Default::default()
        };
        assert_eq!(read(&apply(&edited, &same_for_all).unwrap()), same_for_all);

        let off = apply(&edited, &Settings::default()).unwrap();
        assert_eq!(off, INI, "every key back to auto, nothing else touched");
        assert_eq!(read(&off), Settings::default());
    }

    #[test]
    fn out_of_range_values_and_missing_keys_are_refused() {
        let bad = [
            Settings {
                dlss_preset: Some(16),
                ..Default::default()
            },
            Settings {
                dlss_preset: Some(11),
                dlss_quality_presets: BTreeMap::from([("DLAA".into(), 11)]),
                ..Default::default()
            },
            Settings {
                dlss_quality_presets: BTreeMap::from([("Turbo".into(), 11)]),
                ..Default::default()
            },
            Settings {
                dlssd_preset: Some(6),
                ..Default::default()
            },
            Settings {
                fsr_upscaler: Some(3),
                ..Default::default()
            },
            Settings {
                sharpness: Some(2.0),
                ..Default::default()
            },
            Settings {
                upscale_ratio: Some(f32::NAN),
                ..Default::default()
            },
            Settings {
                anisotropy: Some(17),
                ..Default::default()
            },
        ];
        for s in bad {
            assert!(apply(INI, &s).is_err(), "{s:?}");
        }
        // An ini without these keys (a custom build): defaults are fine,
        // an actual setting isn't silently dropped.
        assert!(apply("[Upscalers]\nDx12Upscaler=auto\n", &Settings::default()).is_ok());
        let ratio = Settings {
            upscale_ratio: Some(1.0),
            ..Default::default()
        };
        assert!(apply("[Upscalers]\nDx12Upscaler=auto\n", &ratio).is_err());
    }

    #[test]
    fn saving_edits_the_installed_ini_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("bin")).unwrap();
        fs::write(tmp.path().join("bin/OptiScaler.ini"), INI).unwrap();
        let s = Settings {
            anisotropy: Some(8),
            ..Default::default()
        };
        save(tmp.path(), "bin", &s).unwrap();
        let text = fs::read_to_string(tmp.path().join("bin/OptiScaler.ini")).unwrap();
        assert_eq!(read(&text), s);
        assert!(!tmp.path().join("bin/OptiScaler.ini.tatu-tmp").exists());
    }

    #[test]
    fn the_fixture_is_the_released_ini() {
        use sha2::{Digest, Sha256};
        let pinned = crate::package::FILES
            .iter()
            .find(|(name, _, _)| *name == "OptiScaler.ini")
            .unwrap();
        assert_eq!(RELEASE_INI.len() as u64, pinned.1);
        assert_eq!(
            format!("{:x}", Sha256::digest(RELEASE_INI.as_bytes())),
            pinned.2
        );
        // Untouched, the release overrides nothing.
        let opti = Opti::load(RELEASE_INI);
        assert_eq!(read(RELEASE_INI), Settings::default());
        assert_eq!(opti.preset("DLSS", "Quality"), None);
        assert_eq!(opti.boolean("CAS", "Enabled"), None);
    }

    #[test]
    fn every_setting_reaches_optiscaler_as_meant() {
        let s = Settings {
            dlss_preset: None,
            dlss_quality_presets: BTreeMap::from([
                ("DLAA".into(), 11),
                ("Quality".into(), 10),
                ("UltraPerformance".into(), 13),
            ]),
            dlssd_preset: Some(5),
            fsr_upscaler: Some(0),
            fsr_fg: Some(1),
            sharpness: Some(0.35),
            upscale_ratio: Some(1.0),
            output_scaling: Some(2.0),
            fps_limit: Some(144.5),
            anisotropy: Some(16),
        };
        let edited = apply(RELEASE_INI, &s).unwrap();
        let opti = Opti::load(&edited);

        // DLSS: the chosen modes get theirs, the rest keep the game's.
        assert_eq!(opti.preset("DLSS", "DLAA"), Some(11));
        assert_eq!(opti.preset("DLSS", "Quality"), Some(10));
        assert_eq!(opti.preset("DLSS", "UltraPerformance"), Some(13));
        assert_eq!(opti.preset("DLSS", "Balanced"), None);
        assert_eq!(opti.preset("DLSSD", "Quality"), Some(5));
        // FSR 4.0.2 is code 0 — a real choice, not "unset".
        assert_eq!(opti.int("FSR", "UpscalerIndex"), Some(0));
        assert_eq!(opti.int("FSR", "FGIndex"), Some(1));
        // RCAS on with the strength, and the game's own sharpness overridden.
        assert_eq!(opti.boolean("CAS", "Enabled"), Some(true));
        assert_eq!(opti.boolean("Sharpness", "OverrideSharpness"), Some(true));
        assert_eq!(opti.float("Sharpness", "Sharpness"), Some(0.35));
        assert_eq!(
            opti.boolean("UpscaleRatio", "UpscaleRatioOverrideEnabled"),
            Some(true)
        );
        assert_eq!(
            opti.float("UpscaleRatio", "UpscaleRatioOverrideValue"),
            Some(1.0)
        );
        assert_eq!(opti.boolean("OutputScaling", "Enabled"), Some(true));
        assert_eq!(opti.float("OutputScaling", "Multiplier"), Some(2.0));
        assert_eq!(opti.float("Framerate", "FramerateLimit"), Some(144.5));
        assert_eq!(opti.int("Anisotropy", "AnisotropyOverride"), Some(16));
        // And Tatu reads back exactly what it wrote.
        assert_eq!(read(&edited), s);
    }

    #[test]
    fn one_preset_for_all_wins_over_every_mode() {
        let per_mode = Settings {
            dlss_quality_presets: BTreeMap::from([("Quality".into(), 10)]),
            ..Default::default()
        };
        let for_all = Settings {
            dlss_preset: Some(12),
            ..Default::default()
        };
        let edited = apply(&apply(RELEASE_INI, &per_mode).unwrap(), &for_all).unwrap();
        let opti = Opti::load(&edited);
        for (quality, _) in QUALITIES.iter().map(|q| (*q, ())) {
            assert_eq!(opti.preset("DLSS", quality), Some(12), "{quality}");
        }
        // Switching back to per mode clears the for-all value, or it would win.
        let back = apply(&edited, &per_mode).unwrap();
        let opti = Opti::load(&back);
        assert_eq!(opti.preset("DLSS", "Quality"), Some(10));
        assert_eq!(opti.preset("DLSS", "DLAA"), None);
    }

    #[test]
    fn turning_everything_off_gives_back_the_release_ini_byte_for_byte() {
        let everything = Settings {
            dlss_preset: Some(11),
            dlssd_preset: Some(4),
            fsr_upscaler: Some(1),
            fsr_fg: Some(0),
            sharpness: Some(1.3),
            upscale_ratio: Some(3.0),
            output_scaling: Some(0.5),
            fps_limit: Some(30.0),
            anisotropy: Some(2),
            ..Default::default()
        };
        let edited = apply(RELEASE_INI, &everything).unwrap();
        assert_ne!(edited, RELEASE_INI);
        assert_eq!(read(&edited), everything);
        assert_eq!(apply(&edited, &Settings::default()).unwrap(), RELEASE_INI);
    }

    #[test]
    fn what_optiscalers_own_menu_saves_reads_back() {
        // SimpleIni writes `key = value`, and the menu turns things off with
        // `false` rather than `auto`.
        let saved = RELEASE_INI
            .replace("; Enables RCAS sharpening \r\n; true or false - Default (auto) is false\r\nEnabled=auto", "; Enables RCAS sharpening \r\n; true or false - Default (auto) is false\r\nEnabled = false")
            .replace("RenderPresetOverride=auto\r\n\r\n; Render presets for quality settings\r\n; Depends on DLSS version",
                     "RenderPresetOverride = true\r\n\r\n; Render presets for quality settings\r\n; Depends on DLSS version")
            .replace("RenderPresetForAll=auto\r\nRenderPresetDLAA=auto\r\nRenderPresetUltraQuality=auto\r\nRenderPresetQuality=auto\r\nRenderPresetBalanced=auto\r\nRenderPresetPerformance=auto\r\nRenderPresetUltraPerformance=auto\r\n\r\n; Use generic",
                     "RenderPresetForAll = 11\r\nRenderPresetDLAA=auto\r\nRenderPresetUltraQuality=auto\r\nRenderPresetQuality=auto\r\nRenderPresetBalanced=auto\r\nRenderPresetPerformance=auto\r\nRenderPresetUltraPerformance=auto\r\n\r\n; Use generic")
            .replace("UpscalerIndex=auto", "UpscalerIndex = 1");
        let opti = Opti::load(&saved);
        assert_eq!(
            opti.preset("DLSS", "Balanced"),
            Some(11),
            "the fixture edit took"
        );
        let s = read(&saved);
        assert_eq!(s.dlss_preset, Some(11));
        assert_eq!(s.fsr_upscaler, Some(1));
        assert_eq!(
            opti.boolean("CAS", "Enabled"),
            Some(false),
            "the fixture edit took"
        );
        assert_eq!(s.sharpness, None, "RCAS explicitly off");
        // Saving from Tatu keeps what the menu set and changes only what's asked.
        let more = Settings {
            anisotropy: Some(8),
            ..s.clone()
        };
        let edited = apply(&saved, &more).unwrap();
        assert_eq!(read(&edited), more);
        assert_eq!(
            Opti::load(&edited).preset("DLSS", "UltraPerformance"),
            Some(11)
        );
    }

    #[test]
    fn the_release_documents_the_choices_the_ui_offers() {
        let c = choices(RELEASE_INI);
        let labels = |list: &[IniOption]| {
            list.iter()
                .map(|o| format!("{}={}", o.code, o.label))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(&c.dlss_presets).first().map(String::as_str),
            Some("1=A")
        );
        assert_eq!(
            labels(&c.dlss_presets).last().map(String::as_str),
            Some("15=O")
        );
        assert_eq!(
            labels(&c.dlssd_presets),
            ["1=A", "2=B", "3=C", "4=D", "5=E"]
        );
        assert_eq!(
            labels(&c.fsr_upscalers),
            ["0=FSR 4.0.2", "1=FSR 3.1.5", "2=FSR 2.3.4"]
        );
        assert_eq!(labels(&c.fsr_fg), ["0=FSR 4.0.0", "1=FSR 3.1.6"]);
        // Every choice offered is accepted by OptiScaler's range checks
        // (Config.cpp) and reaches it as that very value.
        for o in &c.dlss_presets {
            let code: u32 = o.code.parse().unwrap();
            let s = Settings {
                dlss_preset: Some(code),
                ..Default::default()
            };
            let opti = Opti::load(&apply(RELEASE_INI, &s).unwrap());
            assert_eq!(
                opti.preset("DLSS", "Balanced"),
                Some(i64::from(code)),
                "DLSS {}",
                o.label
            );
        }
        for o in &c.dlssd_presets {
            let code: u32 = o.code.parse().unwrap();
            let s = Settings {
                dlssd_preset: Some(code),
                ..Default::default()
            };
            let opti = Opti::load(&apply(RELEASE_INI, &s).unwrap());
            assert_eq!(
                opti.preset("DLSSD", "Quality"),
                Some(i64::from(code)),
                "DLSSD {}",
                o.label
            );
        }
        for o in &c.fsr_upscalers {
            let code: u8 = o.code.parse().unwrap();
            let s = Settings {
                fsr_upscaler: Some(code),
                ..Default::default()
            };
            let opti = Opti::load(&apply(RELEASE_INI, &s).unwrap());
            assert_eq!(
                opti.int("FSR", "UpscalerIndex"),
                Some(i64::from(code)),
                "{}",
                o.label
            );
        }
    }

    #[test]
    fn the_ui_request_deserializes_into_these_settings() {
        // Exactly what pc_settings.js `readSettings` sends: every control,
        // `null` for "del juego", numbers, the per-mode map.
        let json = r#"{
            "dlss_preset": null,
            "dlss_quality_presets": {"DLAA": 11, "Performance": 13},
            "dlssd_preset": 5, "fsr_upscaler": 0, "fsr_fg": null,
            "sharpness": 0.4, "upscale_ratio": 1, "output_scaling": null,
            "fps_limit": 60, "anisotropy": 16
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.dlss_quality_presets.get("Performance"), Some(&13));
        assert_eq!(
            (s.fsr_upscaler, s.upscale_ratio, s.fps_limit),
            (Some(0), Some(1.0), Some(60.0))
        );
        assert!(apply(RELEASE_INI, &s).is_ok());
        // An install request without settings (older frontend) is all defaults.
        let bare: crate::optiscaler::InstallOptions = serde_json::from_str(
            r#"{"proxy": "dxgi.dll", "dlss_inputs": true, "nvidia": true, "custom_dll": null}"#,
        )
        .unwrap();
        assert_eq!(bare.settings, Settings::default());
        // A value the ini doesn't document is refused, never written.
        let odd: Settings = serde_json::from_str(r#"{"fsr_upscaler": 7}"#).unwrap();
        assert!(apply(RELEASE_INI, &odd).is_err());
    }

    #[test]
    fn what_the_menu_set_beyond_tatus_lists_stays_saveable() {
        // OptiScaler's menu can set things Tatu's lists don't offer: the
        // "latest" DLSS preset, extended-limit ratios, 1× anisotropy, and a
        // preset for every mode on top of per-mode ones.
        let menu = RELEASE_INI
            .replacen("RenderPresetOverride=auto", "RenderPresetOverride=true", 1)
            .replacen("RenderPresetForAll=auto", "RenderPresetForAll=16777215", 1)
            .replacen("RenderPresetDLAA=auto", "RenderPresetDLAA=11", 1)
            .replacen(
                "UpscaleRatioOverrideEnabled=auto",
                "UpscaleRatioOverrideEnabled=true",
                1,
            )
            .replacen(
                "UpscaleRatioOverrideValue=auto",
                "UpscaleRatioOverrideValue=5.5",
                1,
            )
            .replacen("AnisotropyOverride=auto", "AnisotropyOverride=1", 1)
            // Line start: `OverrideSharpness=auto` comes first.
            .replacen("\nSharpness=auto", "\nSharpness=2.0", 1)
            .replacen("; Enables RCAS sharpening \r\n; true or false - Default (auto) is false\r\nEnabled=auto", "; Enables RCAS sharpening \r\n; true or false - Default (auto) is false\r\nEnabled=true", 1);
        let now = read(&menu);
        assert_eq!(now.dlss_preset, Some(16_777_215));
        assert!(
            now.dlss_quality_presets.is_empty(),
            "the for-all preset is the one in effect"
        );
        assert_eq!(now.upscale_ratio, Some(5.5));
        assert_eq!(now.anisotropy, Some(1));
        assert_eq!(now.sharpness, Some(1.3), "clamped like OptiScaler does");
        // Saving an unrelated change keeps all of it.
        let edited = apply(
            &menu,
            &Settings {
                fps_limit: Some(60.0),
                ..now.clone()
            },
        )
        .unwrap();
        let opti = Opti::load(&edited);
        assert_eq!(opti.int("DLSS", "RenderPresetForAll"), Some(16_777_215));
        assert_eq!(
            opti.float("UpscaleRatio", "UpscaleRatioOverrideValue"),
            Some(5.5)
        );
        assert_eq!(opti.float("Framerate", "FramerateLimit"), Some(60.0));
        // Nor does a save touch what Tatu doesn't model: sharpening through
        // the game's own DLSS sharpness (no RCAS), per-mode Ray
        // Reconstruction presets.
        let menu = RELEASE_INI
            .replacen("OverrideSharpness=auto", "OverrideSharpness=true", 1)
            .replacen("\nSharpness=auto", "\nSharpness=0.6", 1)
            .replace(
                "[DLSSD]\r\n; -------------------------------------------------------\r\n; Set this to true to enable custom render preset overrides\r\n; true or false - Default (auto) is false\r\nRenderPresetOverride=auto",
                "[DLSSD]\r\n; -------------------------------------------------------\r\n; Set this to true to enable custom render preset overrides\r\n; true or false - Default (auto) is false\r\nRenderPresetOverride=true",
            );
        let menu = {
            // The DLSSD per-mode key right after its for-all one.
            let at = menu.find("[DLSSD]").unwrap();
            let (head, tail) = menu.split_at(at);
            format!(
                "{head}{}",
                tail.replacen("RenderPresetDLAA=auto", "RenderPresetDLAA=4", 1)
            )
        };
        let before = Opti::load(&menu);
        assert_eq!(
            before.preset("DLSSD", "DLAA"),
            Some(4),
            "the fixture edit took"
        );
        assert_eq!(before.boolean("Sharpness", "OverrideSharpness"), Some(true));
        let edited = apply(
            &menu,
            &Settings {
                anisotropy: Some(8),
                ..read(&menu)
            },
        )
        .unwrap();
        let after = Opti::load(&edited);
        assert_eq!(after.preset("DLSSD", "DLAA"), Some(4));
        assert_eq!(after.boolean("Sharpness", "OverrideSharpness"), Some(true));
        assert_eq!(after.float("Sharpness", "Sharpness"), Some(0.6));
        assert_eq!(after.int("Anisotropy", "AnisotropyOverride"), Some(8));
        assert_eq!(
            edited
                .lines()
                .zip(menu.lines())
                .filter(|(a, b)| a != b)
                .count(),
            1,
            "only the line that changed"
        );

        // But Tatu itself can't pick them.
        let picked = Settings {
            dlss_preset: Some(16_777_215),
            ..Default::default()
        };
        assert!(apply(RELEASE_INI, &picked).is_err());
        let picked = Settings {
            upscale_ratio: Some(7.0),
            ..Default::default()
        };
        assert!(apply(RELEASE_INI, &picked).is_err());
    }
}
