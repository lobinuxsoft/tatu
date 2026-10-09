//! Test helpers shared by the modules' tests: the real release ini, and a
//! model of how OptiScaler reads it.

use std::collections::BTreeMap;

/// The OptiScaler.ini of the 0.9.4 release, byte for byte (checked
/// against the SHA-256 the release is pinned by).
pub(crate) const RELEASE_INI: &str = include_str!("../tests/fixtures/OptiScaler-0.9.4.ini");

/// How OptiScaler reads its ini, kept apart from Tatu's own reader so the
/// settings are checked against OptiScaler's rules: SimpleIni (section and
/// key names case-insensitive, `key = value` with or without spaces, the
/// last of a repeated key wins) and Config.cpp's readers — `auto` is
/// unset, booleans are exactly `true`/`false`, numbers must be the whole
/// value.
pub(crate) struct Opti(BTreeMap<(String, String), String>);

impl Opti {
    pub(crate) fn load(text: &str) -> Self {
        let mut values = BTreeMap::new();
        let mut section = String::new();
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                section = line.trim_matches(['[', ']']).to_lowercase();
            } else if !line.starts_with(';')
                && !line.starts_with('#')
                && let Some((key, value)) = line.split_once('=')
            {
                // A repeated key: the last one wins, like SimpleIni.
                values.insert(
                    (section.clone(), key.trim().to_lowercase()),
                    value.trim().to_string(),
                );
            }
        }
        Opti(values)
    }

    pub(crate) fn string(&self, section: &str, key: &str) -> Option<&str> {
        self.0
            .get(&(section.to_lowercase(), key.to_lowercase()))
            .map(String::as_str)
            .filter(|v| !v.eq_ignore_ascii_case("auto"))
    }

    pub(crate) fn boolean(&self, section: &str, key: &str) -> Option<bool> {
        match self.string(section, key)?.to_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    pub(crate) fn int(&self, section: &str, key: &str) -> Option<i64> {
        self.string(section, key)?.parse().ok()
    }

    pub(crate) fn float(&self, section: &str, key: &str) -> Option<f32> {
        self.string(section, key)?.parse().ok()
    }

    /// The preset OptiScaler hands DLSS (or DLSS-D) for a quality mode —
    /// DLSSFeature.cpp: only with `RenderPresetOverride`, and
    /// `RenderPresetForAll.value_or(RenderPreset<mode>)`; `None`: the
    /// game's own.
    pub(crate) fn preset(&self, section: &str, quality: &str) -> Option<i64> {
        let count = if section == "DLSS" { 17 } else { 7 };
        let valid = |v: &i64| (0..count).contains(v);
        self.boolean(section, "RenderPresetOverride")
            .filter(|on| *on)
            .and_then(|_| {
                self.int(section, "RenderPresetForAll")
                    .filter(valid)
                    .or_else(|| {
                        self.int(section, &format!("RenderPreset{quality}"))
                            .filter(valid)
                    })
            })
    }
}
