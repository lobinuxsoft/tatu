//! Per-game cheat manifest format.
//!
//! A manifest is a **self-describing** JSON file that explicitly binds each
//! user-facing feature to the CE Auto-Assembler script(s) that implement it:
//!
//! ```json
//! {
//!   "exe": "EnderMagnoliaSteam-Win64-Shipping.exe",
//!   "title": "Ender Magnolia",
//!   "features": [
//!     {
//!       "uuid": "12710713-cf53-47f2-8a7a-c3139fda2677",
//!       "name": "God Mode",
//!       "category": "Player",
//!       "script": "[ENABLE]\naobscanmodule(...)\n...\n[DISABLE]\n..."
//!     }
//!   ]
//! }
//! ```
//!
//! Manifests live under `$XDG_CONFIG_HOME/backlog-tracker/trainers/<app_id>/`
//! and are loaded eagerly by [`load_manifests_for`]. Many manifests can
//! coexist per game (e.g. one per trainer source).
//!
//! Aurora's raw JSON (handled by the `aurora` module) is **not** a manifest
//! — it exposes features and scripts as parallel lists with no binding.
//! Converting Aurora payloads to manifests is gated on reverse-engineering
//! the feature→script binding, which is documented in personal memory as a
//! still-open problem.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MANIFEST_SUBDIR: &str = "backlog-tracker/trainers";
const CT_SUBDIR: &str = "backlog-tracker/cheat-tables";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub exe: String,
    #[serde(default)]
    pub title: String,
    pub features: Vec<ManifestFeature>,
    /// Per-game prerequisites the runtime must satisfy before any feature in
    /// this manifest can be enabled. Populated by the CT importer when it
    /// recognises the game family (e.g. RE Engine exes get a
    /// [`Prereq::Reframework`] auto-attached — see #98). Empty for the
    /// common case; the UI hides the prereq banner when the vec is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prereqs: Vec<Prereq>,
    /// `true` when this table is driven by an embedded Lua framework
    /// (Manifold-style): its `<LuaScript>` bootstraps modules from `<Files>`,
    /// and its toggles are `{$lua}` cheats ([`ManifestFeature::lua`]) run
    /// through [`crate::FrameworkRuntime`] rather than the AA executor.
    #[serde(default, skip_serializing_if = "is_false")]
    pub framework: bool,
}

/// serde `skip_serializing_if` helper for `bool` fields defaulting to `false`.
fn is_false(b: &bool) -> bool {
    !*b
}

/// External dependency the runtime needs in place before its scripts can
/// run safely. Tagged enum so future variants (EAC bypass, Denuvo offline
/// patches, BepInEx for IL2CPP Mono games, …) can join without breaking
/// existing manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Prereq {
    /// praydog's REFramework `dinput8.dll` proxy — required by every RE
    /// Engine game (RE2/3/4, MHW, DD2, SF6, PRAGMATA, …) to neutralise
    /// Capcom Anti-Tamper's periodic page-integrity scans + anti-debug
    /// syscalls. Without it, AOB scans + trampolines crash the game within
    /// seconds. The installer side lives in `tatu-tracker::prereqs` (the
    /// runtime crate is filesystem-free for portability).
    Reframework,
}

/// Visual / behavioural category of a [`ManifestFeature`].
///
/// Mirrors Cheat Engine's distinction between functional cheats and pure
/// grouping headers in `.CT` tables (CE uses `<GroupHeader>1</GroupHeader>`
/// in the XML element — `MemoryRecordUnit.pas:148` documents it as
/// *"set if it's a groupheader, only the description matters then"*).
///
/// `Toggle` is the default so older manifests (which omit `kind` entirely)
/// continue to deserialize as functional cheats without a migration step.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    /// Functional cheat — has an Auto-Assembler script and renders as a
    /// switch in the UI.
    #[default]
    Toggle,
    /// Visual section title. No script, no switch — the UI renders just
    /// the description as a header above the features that follow.
    Header,
    /// Pointer-chain value entry — CE-style `<Address>` + `<Offsets>` that
    /// resolves to a numeric address whose contents the UI can read, write
    /// and freeze. Requires the [`ValueSpec`] sibling field to be present.
    Value,
}

/// Numeric type read from / written to a pointer-chain target.
///
/// Combines CE's [`VariableType`] enum with the `ShowAsSigned` flag into a
/// single rust-typed variant: CE keeps them separate so the same bytes can
/// be displayed differently, but the importer collapses them at conversion
/// time (`VariableType=4 Bytes` + `ShowAsSigned=1` → `I32`) so downstream
/// code can dispatch on a single tag.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum VType {
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl VType {
    pub const fn size_bytes(self) -> usize {
        match self {
            VType::U32 | VType::I32 | VType::F32 => 4,
            VType::U64 | VType::I64 | VType::F64 => 8,
        }
    }

    pub const fn is_float(self) -> bool {
        matches!(self, VType::F32 | VType::F64)
    }
}

/// Pointer-chain value resolution descriptor.
///
/// `base_expr` is the verbatim CE address expression
/// (e.g. `"[base_address]+30"` or `"[shop]+4B0"`). Evaluating it gives the
/// starting address — see [`crate::chain`] for the parser, which mirrors
/// `symbolhandler.pas:getAddressFromName` for the subset the importer
/// emits (single-level `[symbol]` deref + optional `+|-hex` literal).
///
/// `offsets` is stored in **the same order as CE's `<Offsets>` XML**: the
/// last element is the outermost pointer to follow first, and `offsets[0]`
/// is the innermost — it lands directly in the final address with no
/// trailing deref. The walker iterates in reverse.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValueSpec {
    pub base_expr: String,
    #[serde(default)]
    pub offsets: Vec<u64>,
    pub vtype: VType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestFeature {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub kind: FeatureKind,
    /// CE Auto-Assembler script for [`FeatureKind::Toggle`]. Always `None`
    /// for [`FeatureKind::Header`] / [`FeatureKind::Value`]. Omitted in
    /// older manifests (before this field existed) — those load as
    /// `Toggle` with `script: None` and the runtime command rejects them
    /// at enable time with a clear error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Pointer-chain spec for [`FeatureKind::Value`]. Always `None` for
    /// Toggle and Header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ValueSpec>,
    /// `true` when [`Self::script`] is a `{$lua}` framework cheat rather than
    /// an Auto-Assembler script. Such toggles run through
    /// [`crate::FrameworkRuntime`] (the table's bootstrapped Lua), not the AA
    /// executor. Only set on [`FeatureKind::Toggle`] of a [`Manifest`] with
    /// `framework == true`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lua: bool,
    /// Nested children. CE tables routinely group cheats under
    /// [`FeatureKind::Header`] entries, sometimes arbitrarily deep — the
    /// `ender_magnolia_v11.ct` import exposes 5 levels, `elden_ring`
    /// tables go up to 7. The UI renders this as a collapsible tree.
    ///
    /// Flat manifests (no nesting) deserialise with `children = []` and
    /// the UI still works — the field is optional in JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ManifestFeature>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid manifest json at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
}

impl ManifestFeature {
    /// Depth-first iterator over `self` + every descendant in
    /// `self.children`. Use this from any code that wants to find a
    /// feature by UUID without caring about the tree structure
    /// (`cheat_runtime_enable`, `locate_feature_script`,
    /// `locate_value_feature`).
    pub fn iter_recursive(&self) -> RecursiveFeatureIter<'_> {
        RecursiveFeatureIter { stack: vec![self] }
    }
}

impl Manifest {
    /// Depth-first iterator over every feature in every subtree of this
    /// manifest. Equivalent to chaining `iter_recursive` over each
    /// top-level feature.
    pub fn features_recursive(&self) -> impl Iterator<Item = &ManifestFeature> {
        self.features.iter().flat_map(|f| f.iter_recursive())
    }
}

/// Depth-first iterator returned by [`ManifestFeature::iter_recursive`].
/// Yields the root first, then descends into each child's subtree in order.
pub struct RecursiveFeatureIter<'a> {
    stack: Vec<&'a ManifestFeature>,
}

impl<'a> Iterator for RecursiveFeatureIter<'a> {
    type Item = &'a ManifestFeature;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // Push children in reverse so the leftmost child is visited
        // first (LIFO stack flips order).
        for child in node.children.iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}

pub fn manifests_dir_for(app_id: &str) -> Result<PathBuf, ManifestError> {
    Ok(dirs::config_dir()
        .ok_or(ManifestError::NoConfigDir)?
        .join(MANIFEST_SUBDIR)
        .join(app_id))
}

/// Resolve `$XDG_CONFIG_HOME/backlog-tracker/cheat-tables/<app_id>/`. Pre-#134
/// this lived in `ct_import::disk`; the loader now consumes the `.ct` files
/// directly so the dir helper moves next to its primary caller.
pub fn ct_tables_dir_for(app_id: &str) -> Result<PathBuf, ManifestError> {
    Ok(dirs::config_dir()
        .ok_or(ManifestError::NoConfigDir)?
        .join(CT_SUBDIR)
        .join(app_id))
}

/// Load every per-game manifest, parsing `.ct` source files directly.
///
/// Order of precedence (#134 — JSON intermediate dropped as canonical):
///
/// 1. `cheat-tables/<app_id>/*.ct` — primary source. Each `.ct` is parsed +
///    converted via [`crate::ct_import::convert_ct_file_with_exe_hint`] on
///    every call. Per-file conversion failures are logged to stderr and
///    skipped so one malformed table doesn't blank the user's library.
/// 2. `trainers/<app_id>/*.json` — legacy fallback for stems with no
///    matching `.ct` (e.g. the synthetic `legacy.json` produced by the
///    cheat-core migrator, or manifests authored by hand). A `.ct` of the
///    same stem always wins.
///
/// `exe_hint` is forwarded to the importer for tables that lack both an
/// `aobscanmodule(_, exe, _)` line and a `{ Game : X.exe }` template
/// comment (notably hand-rolled Mono / Unity tables). Pass
/// `Some(detect_game_exe(app_id).ok())` from the Tauri layer — `None` is
/// fine when the caller has no Steam binding available and the table is
/// expected to carry its own exe info.
///
/// Missing directories are not errors — an empty vec is returned so a UI
/// can call this on a game with no tables and render an empty state.
pub fn load_manifests_for(
    app_id: &str,
    exe_hint: Option<&str>,
) -> Result<Vec<Manifest>, ManifestError> {
    let ct_dir = ct_tables_dir_for(app_id)?;
    let legacy_dir = manifests_dir_for(app_id)?;
    load_manifests_from_roots(&ct_dir, &legacy_dir, exe_hint)
}

/// Test-friendly variant of [`load_manifests_for`] that takes the two
/// source directories explicitly. Lets the unit tests build a self-
/// contained tree under `TempDir` without poisoning the process-wide
/// `XDG_CONFIG_HOME`. Same semantics as the public entry point: `.ct`
/// primary, `.json` legacy, dedupe by file stem, sort by title.
pub fn load_manifests_from_roots(
    ct_dir: &Path,
    legacy_dir: &Path,
    exe_hint: Option<&str>,
) -> Result<Vec<Manifest>, ManifestError> {
    let mut out = Vec::new();
    let mut seen_stems: HashSet<String> = HashSet::new();

    if ct_dir.is_dir() {
        for entry in std::fs::read_dir(ct_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ct"))
            {
                continue;
            }
            let stem = match path.file_stem() {
                Some(s) => s.to_string_lossy().into_owned(),
                None => continue,
            };
            match crate::ct_import::convert_ct_file_with_exe_hint(&path, exe_hint) {
                Ok(manifest) => {
                    seen_stems.insert(stem);
                    out.push(manifest);
                }
                Err(e) => {
                    eprintln!("[cheat-runtime load] skipping {}: {e}", path.display());
                }
            }
        }
    }

    if legacy_dir.is_dir() {
        for entry in std::fs::read_dir(legacy_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            let stem = match path.file_stem() {
                Some(s) => s.to_string_lossy().into_owned(),
                None => continue,
            };
            if seen_stems.contains(&stem) {
                continue;
            }
            out.push(load_one(&path)?);
        }
    }

    out.sort_by_key(|m| m.title.to_lowercase());
    Ok(out)
}

fn load_one(path: &Path) -> Result<Manifest, ManifestError> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| ManifestError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn load_dir(dir: &Path) -> Vec<Manifest> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(m) = load_one(&path)
            {
                out.push(m);
            }
        }
        out
    }

    #[test]
    fn parses_minimal_manifest() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"u","name":"God Mode","script":"[ENABLE]\n[DISABLE]\n"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(m.exe, "Game.exe");
        assert_eq!(m.features.len(), 1);
        assert_eq!(m.features[0].name, "God Mode");
    }

    #[test]
    fn defaults_apply_for_optional_fields() {
        let m: Manifest = serde_json::from_str(r#"{"exe":"X.exe","features":[]}"#).unwrap();
        assert_eq!(m.title, "");
    }

    #[test]
    fn round_trip_serialisation_stable() {
        let m = Manifest {
            exe: "Game.exe".into(),
            title: "My Game".into(),
            features: vec![
                ManifestFeature {
                    uuid: "u".into(),
                    name: "God Mode".into(),
                    category: Some("Player".into()),
                    kind: FeatureKind::Toggle,
                    script: Some("[ENABLE]\n[DISABLE]\n".into()),
                    value: None,
                    lua: false,
                    children: Vec::new(),
                },
                ManifestFeature {
                    uuid: "h".into(),
                    name: "=== Player ===".into(),
                    category: None,
                    kind: FeatureKind::Header,
                    script: None,
                    value: None,
                    lua: false,
                    children: Vec::new(),
                },
                ManifestFeature {
                    uuid: "v".into(),
                    name: "Current HP".into(),
                    category: Some("Player Stats".into()),
                    kind: FeatureKind::Value,
                    script: None,
                    value: Some(ValueSpec {
                        base_expr: "[base_address]+30".into(),
                        offsets: vec![0x13C, 0x8B8, 0x2D0],
                        vtype: VType::U32,
                    }),
                    lua: false,
                    children: Vec::new(),
                },
            ],
            prereqs: Vec::new(),
            framework: false,
        };
        let text = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn value_kind_round_trip_preserves_offsets_and_vtype() {
        let json = r#"{
            "exe": "Game.exe",
            "features": [{
                "uuid": "hp",
                "name": "Current HP",
                "kind": "value",
                "value": {
                    "base_expr": "[base_address]+30",
                    "offsets": [316, 2232, 720],
                    "vtype": "u32"
                }
            }]
        }"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.features[0].kind, FeatureKind::Value);
        let spec = m.features[0].value.as_ref().unwrap();
        assert_eq!(spec.base_expr, "[base_address]+30");
        assert_eq!(spec.offsets, vec![316, 2232, 720]);
        assert_eq!(spec.vtype, VType::U32);
    }

    #[test]
    fn vtype_byte_sizes_match_ce() {
        assert_eq!(VType::U32.size_bytes(), 4);
        assert_eq!(VType::I32.size_bytes(), 4);
        assert_eq!(VType::F32.size_bytes(), 4);
        assert_eq!(VType::U64.size_bytes(), 8);
        assert_eq!(VType::I64.size_bytes(), 8);
        assert_eq!(VType::F64.size_bytes(), 8);
        assert!(VType::F32.is_float() && VType::F64.is_float());
        assert!(!VType::U32.is_float() && !VType::I64.is_float());
    }

    #[test]
    fn nested_children_round_trip_preserves_tree() {
        // Mirrors the real-world CE shape recovered from
        // `ender_magnolia_v11.ct` (depth 5) and `elden_ring` tables
        // (depth 7): a Header parent owns child Toggles + grandchildren.
        let leaf = ManifestFeature {
            uuid: "leaf".into(),
            name: "God Mode".into(),
            category: None,
            kind: FeatureKind::Toggle,
            script: Some("[ENABLE]\n[DISABLE]\n".into()),
            value: None,
            lua: false,
            children: Vec::new(),
        };
        let sub_header = ManifestFeature {
            uuid: "sub".into(),
            name: "Combat".into(),
            category: None,
            kind: FeatureKind::Header,
            script: None,
            value: None,
            lua: false,
            children: vec![leaf.clone()],
        };
        let m = Manifest {
            exe: "Game.exe".into(),
            title: "Nested".into(),
            features: vec![ManifestFeature {
                uuid: "root".into(),
                name: "Player".into(),
                category: None,
                kind: FeatureKind::Header,
                script: None,
                value: None,
                lua: false,
                children: vec![sub_header],
            }],
            prereqs: Vec::new(),
            framework: false,
        };
        let text = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
        // Tree shape preserved at depth 3.
        assert_eq!(back.features[0].children[0].children[0].uuid, "leaf");
    }

    #[test]
    fn flat_manifest_without_children_field_still_loads() {
        // Manifests written before #133 don't have a `children` array.
        // `#[serde(default)]` must populate it as empty so older files
        // on disk continue to work without an out-of-band migration.
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"u","name":"God Mode","script":"[ENABLE]\n[DISABLE]\n"}
                ]
            }"#,
        )
        .unwrap();
        assert!(m.features[0].children.is_empty());
    }

    #[test]
    fn empty_children_omitted_in_serialised_output() {
        // skip_serializing_if = "Vec::is_empty" keeps the JSON tidy for
        // the 99% of features that don't have children. Without this,
        // every flat-manifest dump would gain a noisy `"children": []`.
        let m = Manifest {
            exe: "G".into(),
            title: "T".into(),
            features: vec![ManifestFeature {
                uuid: "u".into(),
                name: "n".into(),
                category: None,
                kind: FeatureKind::Toggle,
                script: Some("[ENABLE]\n[DISABLE]\n".into()),
                value: None,
                lua: false,
                children: Vec::new(),
            }],
            prereqs: Vec::new(),
            framework: false,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("\"children\""));
        // Same omission guarantee for prereqs — the 99% of manifests
        // without any RE Engine / Mono / etc. dependency should serialise
        // clean. See #98 (Prereq enum) for the schema.
        assert!(!json.contains("\"prereqs\""));
    }

    #[test]
    fn deserialises_old_shape_without_kind_as_toggle() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"u","name":"God Mode","script":"[ENABLE]\n[DISABLE]\n"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(m.features[0].kind, FeatureKind::Toggle);
        assert_eq!(
            m.features[0].script.as_deref(),
            Some("[ENABLE]\n[DISABLE]\n")
        );
    }

    #[test]
    fn header_kind_round_trips_without_script() {
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [
                    {"uuid":"sep1","name":"== Combat ==","kind":"header"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(m.features[0].kind, FeatureKind::Header);
        assert!(m.features[0].script.is_none());
        // Round-trip omits the script field when None.
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("\"script\""));
    }

    #[test]
    fn load_dir_picks_up_json_only() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "a.json",
            r#"{"exe":"a.exe","title":"Alpha","features":[]}"#,
        );
        write(
            tmp.path(),
            "b.json",
            r#"{"exe":"b.exe","title":"Beta","features":[]}"#,
        );
        write(tmp.path(), "ignore.txt", "not json");

        let mut all = load_dir(tmp.path());
        all.sort_by(|a, b| a.title.cmp(&b.title));
        let titles: Vec<&str> = all.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn malformed_json_surfaces_path_in_error() {
        let tmp = TempDir::new().unwrap();
        let bad = tmp.path().join("bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        let err = load_one(&bad).unwrap_err();
        match err {
            ManifestError::Json { path, .. } => assert_eq!(path, bad),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    const BASIC_CT: &str = include_str!("../tests/fixtures/ct/01_basic_toggle.ct");

    #[test]
    fn loads_ct_directly_when_no_legacy_json_present() {
        let tmp = TempDir::new().unwrap();
        let ct_dir = tmp.path().join("ct");
        let json_dir = tmp.path().join("json");
        std::fs::create_dir_all(&ct_dir).unwrap();

        std::fs::write(ct_dir.join("alpha.ct"), BASIC_CT).unwrap();

        let manifests = load_manifests_from_roots(&ct_dir, &json_dir, None).unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].title, "alpha");
        // The fixture's aobscanmodule line pins fixture.exe.
        assert_eq!(manifests[0].exe, "fixture.exe");
        assert_eq!(manifests[0].features.len(), 1);
    }

    #[test]
    fn ct_takes_precedence_over_matching_legacy_json() {
        // Dedupe is by file stem. A user that imported `mytable.ct` once
        // (back when imports also wrote `mytable.json`) and then re-edited
        // the `.ct` upstream should see the fresh `.ct` parse, not the
        // stale JSON snapshot.
        let tmp = TempDir::new().unwrap();
        let ct_dir = tmp.path().join("ct");
        let json_dir = tmp.path().join("json");
        std::fs::create_dir_all(&ct_dir).unwrap();
        std::fs::create_dir_all(&json_dir).unwrap();

        std::fs::write(ct_dir.join("alpha.ct"), BASIC_CT).unwrap();
        std::fs::write(
            json_dir.join("alpha.json"),
            r#"{"exe":"stale.exe","title":"Stale","features":[]}"#,
        )
        .unwrap();

        let manifests = load_manifests_from_roots(&ct_dir, &json_dir, None).unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].title, "alpha");
        assert_eq!(manifests[0].exe, "fixture.exe");
    }

    #[test]
    fn legacy_json_loads_when_no_matching_ct() {
        // The cheat-core migrator writes synthetic `legacy.json` manifests
        // with no `.ct` companion — those must still surface. Likewise any
        // hand-curated manifest the user dropped into trainers/ pre-#134.
        let tmp = TempDir::new().unwrap();
        let ct_dir = tmp.path().join("ct");
        let json_dir = tmp.path().join("json");
        std::fs::create_dir_all(&json_dir).unwrap();

        std::fs::write(
            json_dir.join("legacy.json"),
            r#"{"exe":"old.exe","title":"Old","features":[]}"#,
        )
        .unwrap();

        let manifests = load_manifests_from_roots(&ct_dir, &json_dir, None).unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].title, "Old");
    }

    #[test]
    fn malformed_ct_is_skipped_not_fatal() {
        // A single broken `.ct` must not blank the library — the loader
        // logs and moves on so a typo in one table doesn't hide the rest.
        let tmp = TempDir::new().unwrap();
        let ct_dir = tmp.path().join("ct");
        let json_dir = tmp.path().join("json");
        std::fs::create_dir_all(&ct_dir).unwrap();

        std::fs::write(ct_dir.join("good.ct"), BASIC_CT).unwrap();
        std::fs::write(ct_dir.join("bad.ct"), "<not really xml").unwrap();

        let manifests = load_manifests_from_roots(&ct_dir, &json_dir, None).unwrap();
        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].title, "good");
    }

    #[test]
    fn manifest_without_prereqs_field_defaults_to_empty() {
        // Existing on-disk JSON manifests (every legacy manifest written
        // before #98) lack the `prereqs` array. `#[serde(default)]` must
        // populate it as empty so they keep loading without a one-shot
        // migration. Same contract as `children` field (post-#133).
        let m: Manifest = serde_json::from_str(
            r#"{
                "exe": "Game.exe",
                "features": [{"uuid":"u","name":"X","script":"[ENABLE]\n[DISABLE]\n"}]
            }"#,
        )
        .unwrap();
        assert!(m.prereqs.is_empty());
    }

    #[test]
    fn reframework_prereq_round_trips_with_kind_tag() {
        // External `kind` tag (`#[serde(tag = "kind")]`) means the JSON
        // shape is `{"kind":"reframework"}`, not a bare string. Future
        // variants (BepInEx, EAC bypass, …) extend the same envelope.
        let json = r#"{
            "exe": "PRAGMATA.exe",
            "title": "Pragmata",
            "features": [],
            "prereqs": [{"kind":"reframework"}]
        }"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.prereqs, vec![Prereq::Reframework]);
        let back = serde_json::to_string(&m).unwrap();
        assert!(back.contains("\"kind\":\"reframework\""));
    }

    #[test]
    fn missing_dirs_return_empty_not_error() {
        let tmp = TempDir::new().unwrap();
        let manifests = load_manifests_from_roots(
            &tmp.path().join("nope-ct"),
            &tmp.path().join("nope-json"),
            None,
        )
        .unwrap();
        assert!(manifests.is_empty());
    }
}
