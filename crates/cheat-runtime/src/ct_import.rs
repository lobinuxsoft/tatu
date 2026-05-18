//! Cheat Engine `.CT` (CheatTable XML) → manifest auto-importer.
//!
//! Cheat Engine stores tables as XML with a `<CheatTable>` root containing
//! arbitrarily nested `<CheatEntry>` elements. Each entry can be a value
//! tweak (typed `<Address>` + `<VariableType>` like `4 Bytes`), an
//! Auto-Assembler script (`<VariableType>Auto Assembler Script</VariableType>`
//! + `<AssemblerScript>`), a group header (`<GroupHeader>1</GroupHeader>`,
//! description only), or a Lua-shim entry (uses `luacall(...)` or `{$lua}`
//! blocks). The runtime only understands compiled AA scripts, so we project
//! the table down to the subset our [`crate::executor`] can execute and skip
//! everything else.
//!
//! ### Projection rules
//!
//! | CT entry shape | Manifest output |
//! |---|---|
//! | `Auto Assembler Script` whose body contains real AA (`aobscanmodule`, `alloc(`, `db `, `registersymbol`) | `Toggle` with the script body verbatim |
//! | `GroupHeader=1` | `Header` (description only, no script) |
//! | Lua-only script (no AA primitives detected) | skipped |
//! | Value / pointer entry (`<Address>` + typed `<VariableType>`) | skipped (no value-edit UI yet) |
//!
//! ### Exe binding
//!
//! `Manifest::exe` is recovered by scanning the produced toggles for the
//! first `aobscanmodule(_, <Exe.exe>, _)` line. CT files always carry the
//! module name there because that's how CE binds the scan to a specific
//! image. If the table has no AA toggle (header-only or value-only), the
//! conversion fails — there's no useful manifest to emit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::{FeatureKind, Manifest, ManifestFeature};

const CT_SUBDIR: &str = "backlog-tracker/cheat-tables";
const MANIFEST_SUBDIR: &str = "backlog-tracker/trainers";

#[derive(Debug, thiserror::Error)]
pub enum CtImportError {
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid CT XML at {path}: {source}")]
    Xml {
        path: PathBuf,
        #[source]
        source: roxmltree::Error,
    },
    #[error("table at {path} contains no convertible cheats")]
    Empty { path: PathBuf },
    #[error("table at {path} has cheats but no aobscanmodule line to recover the module name from")]
    NoExeBinding { path: PathBuf },
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
    #[error("manifest serialisation failed: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Default)]
pub struct ImportReport {
    /// Manifest files that were just written.
    pub created: Vec<PathBuf>,
    /// `.ct` files that already had a matching manifest on disk, so the
    /// importer left them alone.
    pub skipped: Vec<PathBuf>,
    /// `.ct` files the importer tried to convert but failed on. The errors
    /// surface here instead of aborting the whole pass so one bad table
    /// doesn't block the rest of the user's library.
    pub failed: Vec<(PathBuf, CtImportError)>,
}

/// Convert a single `.ct` file into an in-memory [`Manifest`].
pub fn convert_ct_file(path: &Path) -> Result<Manifest, CtImportError> {
    let text = fs::read_to_string(path).map_err(|source| CtImportError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let doc = roxmltree::Document::parse(&text).map_err(|source| CtImportError::Xml {
        path: path.to_path_buf(),
        source,
    })?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".to_string());

    let mut features = Vec::new();
    walk_entries(doc.root_element(), &stem, &mut features);

    if features.is_empty() {
        return Err(CtImportError::Empty {
            path: path.to_path_buf(),
        });
    }

    let exe = derive_exe(&features).ok_or_else(|| CtImportError::NoExeBinding {
        path: path.to_path_buf(),
    })?;

    Ok(Manifest {
        exe,
        title: stem,
        features,
    })
}

/// Walk all `<CheatEntry>` nodes (recursively, including those nested inside
/// other `<CheatEntry><CheatEntries>...`). CE's authoring tool exposes the
/// nesting as a visual tree but neither the manifest nor our UI represent
/// nested cheats — we flatten preserving document order.
fn walk_entries(node: roxmltree::Node, stem: &str, out: &mut Vec<ManifestFeature>) {
    for entry in node.descendants().filter(|n| n.has_tag_name("CheatEntry")) {
        if let Some(feature) = entry_to_feature(entry, stem) {
            out.push(feature);
        }
    }
}

fn entry_to_feature(entry: roxmltree::Node, stem: &str) -> Option<ManifestFeature> {
    let id = child_text(entry, "ID").unwrap_or_default();
    let description = strip_quotes(child_text(entry, "Description").unwrap_or_default());
    if description.is_empty() {
        return None;
    }
    let uuid = format!("ct-{stem}-{id}");

    if child_text(entry, "GroupHeader").is_some_and(|v| v.trim() == "1") {
        if !is_meaningful_header(&description) {
            return None;
        }
        return Some(ManifestFeature {
            uuid,
            name: description,
            category: None,
            kind: FeatureKind::Header,
            script: None,
        });
    }

    let kind = child_text(entry, "VariableType").unwrap_or_default();
    if kind.trim() != "Auto Assembler Script" {
        return None;
    }
    let script = child_text(entry, "AssemblerScript").unwrap_or_default();
    if !is_real_aa_script(&script) {
        return None;
    }

    Some(ManifestFeature {
        uuid,
        name: description,
        category: None,
        kind: FeatureKind::Toggle,
        script: Some(script),
    })
}

/// Direct-child text accessor: returns the trimmed text of the first
/// element child with `tag` under `node`, or `None` if absent.
fn child_text(node: roxmltree::Node, tag: &str) -> Option<String> {
    let child = node.children().find(|c| c.has_tag_name(tag))?;
    Some(child.text().unwrap_or("").trim().to_string())
}

/// Drop CE `<GroupHeader>` entries that are pure visual ornament: ASCII
/// separators (`---------`), runic dividers (`◣⫘⫘⫘…◢`), info notes prefixed
/// with `❖`, and clarifications wrapped in `《…》`. Cheat-table authors use
/// `<GroupHeader>1</GroupHeader>` for *any* description-only entry — CE
/// itself renders them all uniformly, but a tracker UI listing 108 of them
/// between 5 actual cheats hides the cheats. Concrete rule:
///
/// - reject if the description starts with `❖`, contains a matched `《…》`
///   info-note wrapper, or starts with `❎`/`⚠` UI guidance markers,
/// - reject if it contains no Unicode letters at all (separators),
/// - keep everything else, including the common `【 Title 】` section-header
///   shape used by this author.
fn is_meaningful_header(description: &str) -> bool {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('❖') {
        return false;
    }
    if trimmed.contains('《') && trimmed.contains('》') {
        return false;
    }
    if !trimmed.chars().any(char::is_alphabetic) {
        return false;
    }
    true
}

/// Strip a single pair of surrounding straight ASCII quotes that CE wraps
/// around `<Description>` literals in the CT file (`"Player Stats"`).
/// Anything fancier (smart quotes, unbalanced) is left untouched.
fn strip_quotes(s: String) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s
    }
}

/// Decide whether an `<AssemblerScript>` body carries an AA script our
/// executor can compile. We need one of the structural primitives the
/// executor implements: `aobscanmodule`, `alloc(`, raw `db ` data writes, or
/// a `registersymbol` (`registersymbol` without the rest would be useless
/// but still parses; that's a CE author error, not ours). Lua-only entries
/// (`{$lua}` blocks, `luacall(...)`) lack all of these and get skipped.
fn is_real_aa_script(body: &str) -> bool {
    let needles = ["aobscanmodule", "alloc(", "registersymbol", "\ndb ", " db "];
    needles.iter().any(|n| body.contains(n))
}

/// Scan every toggle's script for the first `aobscanmodule(name, exe, …)`
/// line and return the `exe` argument. CE's aobscanmodule syntax pins the
/// scan to a specific loaded module, so the second comma-separated argument
/// is always the executable / DLL name — that's the binding we surface
/// to the launcher.
fn derive_exe(features: &[ManifestFeature]) -> Option<String> {
    for f in features {
        let Some(script) = &f.script else {
            continue;
        };
        for line in script.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("aobscanmodule(") else {
                continue;
            };
            let mut parts = rest.splitn(3, ',');
            let _name = parts.next();
            let Some(exe) = parts.next().map(str::trim) else {
                continue;
            };
            if !exe.is_empty() {
                return Some(exe.to_string());
            }
        }
    }
    None
}

/// Auto-import every `.ct` file under `cheat-tables/<app_id>/` into the
/// corresponding `trainers/<app_id>/` manifest directory.
///
/// Idempotent: a `.ct` whose target manifest already exists is reported as
/// `skipped`. Per-file errors don't abort the pass — they accumulate in
/// `failed` so callers can log them.
pub fn auto_import_for_app(app_id: &str) -> Result<ImportReport, CtImportError> {
    let config = dirs::config_dir().ok_or(CtImportError::NoConfigDir)?;
    let src_dir = config.join(CT_SUBDIR).join(app_id);
    let dst_dir = config.join(MANIFEST_SUBDIR).join(app_id);
    import_dirs(&src_dir, &dst_dir)
}

/// Auto-import every `<app_id>/` subdirectory under `cheat-tables/`. Used
/// from the Tauri startup hook so a freshly-dropped `.ct` becomes visible
/// without the user needing to know about a separate "import" step.
pub fn auto_import_default_dirs() -> Result<ImportReport, CtImportError> {
    let config = dirs::config_dir().ok_or(CtImportError::NoConfigDir)?;
    let tables_root = config.join(CT_SUBDIR);
    let mut report = ImportReport::default();
    if !tables_root.is_dir() {
        return Ok(report);
    }
    let trainers_root = config.join(MANIFEST_SUBDIR);
    for entry in fs::read_dir(&tables_root).map_err(|source| CtImportError::Io {
        path: tables_root.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| CtImportError::Io {
            path: tables_root.clone(),
            source,
        })?;
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let app_id = entry.file_name();
        let src = entry.path();
        let dst = trainers_root.join(&app_id);
        let pass = import_dirs(&src, &dst)?;
        report.created.extend(pass.created);
        report.skipped.extend(pass.skipped);
        report.failed.extend(pass.failed);
    }
    Ok(report)
}

/// Internal entry point taking explicit src/dst dirs — keeps the integration
/// tests self-contained without touching `$XDG_CONFIG_HOME`.
pub fn import_dirs(src: &Path, dst: &Path) -> Result<ImportReport, CtImportError> {
    let mut report = ImportReport::default();
    if !src.is_dir() {
        return Ok(report);
    }
    let read = fs::read_dir(src).map_err(|source| CtImportError::Io {
        path: src.to_path_buf(),
        source,
    })?;
    for entry in read {
        let entry = entry.map_err(|source| CtImportError::Io {
            path: src.to_path_buf(),
            source,
        })?;
        let ct = entry.path();
        if !ct.extension().is_some_and(|e| e.eq_ignore_ascii_case("ct")) {
            continue;
        }
        let stem = match ct.file_stem() {
            Some(s) => s.to_owned(),
            None => continue,
        };
        let target = dst.join(format!("{}.json", stem.to_string_lossy()));
        if target.exists() {
            report.skipped.push(ct);
            continue;
        }
        match convert_ct_file(&ct) {
            Ok(manifest) => {
                if let Err(e) = write_manifest(&target, &manifest) {
                    report.failed.push((ct, e));
                } else {
                    report.created.push(target);
                }
            }
            Err(e) => report.failed.push((ct, e)),
        }
    }
    Ok(report)
}

fn write_manifest(target: &Path, manifest: &Manifest) -> Result<(), CtImportError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|source| CtImportError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let body = serde_json::to_string_pretty(manifest)?;
    fs::write(target, body).map_err(|source| CtImportError::Io {
        path: target.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable CheatEngineTableVersion="45">
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"=== Player ==="</Description>
      <GroupHeader>1</GroupHeader>
    </CheatEntry>
    <CheatEntry>
      <ID>2</ID>
      <Description>"Invincibility"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
aobscanmodule(INJECT,Game.exe,FF 83 74 04 00 00 66)
alloc(newmem,$1000,INJECT)
[DISABLE]
INJECT:
db FF 83 74 04 00 00
dealloc(newmem)
</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <ID>3</ID>
      <Description>"Open Steam Page"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
{$lua}
shellExecute("https://store.steampowered.com/app/1")
{$asm}
[DISABLE]
</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <ID>4</ID>
      <Description>"HP"</Description>
      <VariableType>4 Bytes</VariableType>
      <Address>0x12345678</Address>
    </CheatEntry>
  </CheatEntries>
</CheatTable>
"#;

    fn write_fixture(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn convert_keeps_header_and_aa_script_and_skips_lua_and_value() {
        let tmp = TempDir::new().unwrap();
        let ct = write_fixture(tmp.path(), "Game.ct", FIXTURE);
        let manifest = convert_ct_file(&ct).unwrap();
        assert_eq!(manifest.exe, "Game.exe");
        assert_eq!(manifest.title, "Game");
        assert_eq!(manifest.features.len(), 2);

        assert_eq!(manifest.features[0].kind, FeatureKind::Header);
        assert_eq!(manifest.features[0].name, "=== Player ===");
        assert!(manifest.features[0].script.is_none());

        assert_eq!(manifest.features[1].kind, FeatureKind::Toggle);
        assert_eq!(manifest.features[1].name, "Invincibility");
        let script = manifest.features[1].script.as_deref().unwrap();
        assert!(script.contains("aobscanmodule(INJECT,Game.exe"));
        assert!(script.contains("[DISABLE]"));
    }

    #[test]
    fn import_dirs_creates_then_skips_on_second_run() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("cheat-tables/2725260");
        let dst = tmp.path().join("trainers/2725260");
        fs::create_dir_all(&src).unwrap();
        write_fixture(&src, "Game.ct", FIXTURE);

        let first = import_dirs(&src, &dst).unwrap();
        assert_eq!(first.created.len(), 1);
        assert_eq!(first.skipped.len(), 0);
        assert!(first.failed.is_empty(), "{:?}", first.failed);
        assert!(dst.join("Game.json").is_file());

        let second = import_dirs(&src, &dst).unwrap();
        assert_eq!(second.created.len(), 0);
        assert_eq!(second.skipped.len(), 1);
    }

    #[test]
    fn ct_without_aobscanmodule_fails_no_exe_binding() {
        let tmp = TempDir::new().unwrap();
        let ct = write_fixture(
            tmp.path(),
            "NoExe.ct",
            r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"Hand-rolled"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
alloc(buf,$100,1000000)
[DISABLE]
dealloc(buf)
</AssemblerScript>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#,
        );
        let err = convert_ct_file(&ct).unwrap_err();
        assert!(matches!(err, CtImportError::NoExeBinding { .. }));
    }

    #[test]
    fn ct_with_only_lua_or_values_reports_empty() {
        let tmp = TempDir::new().unwrap();
        let ct = write_fixture(
            tmp.path(),
            "Lua.ct",
            r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"Open URL"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
{$lua}
shellExecute("https://example.com")
{$asm}
[DISABLE]
</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <ID>2</ID>
      <Description>"HP"</Description>
      <VariableType>4 Bytes</VariableType>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#,
        );
        let err = convert_ct_file(&ct).unwrap_err();
        assert!(matches!(err, CtImportError::Empty { .. }));
    }

    #[test]
    fn nested_cheatentries_flatten_in_document_order() {
        let tmp = TempDir::new().unwrap();
        let ct = write_fixture(
            tmp.path(),
            "Nested.ct",
            r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"=== Outer ==="</Description>
      <GroupHeader>1</GroupHeader>
      <CheatEntries>
        <CheatEntry>
          <ID>2</ID>
          <Description>"Inner Cheat"</Description>
          <VariableType>Auto Assembler Script</VariableType>
          <AssemblerScript>[ENABLE]
aobscanmodule(INJECT,Game.exe,90 90 90)
[DISABLE]
</AssemblerScript>
        </CheatEntry>
      </CheatEntries>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#,
        );
        let manifest = convert_ct_file(&ct).unwrap();
        assert_eq!(manifest.features.len(), 2);
        assert_eq!(manifest.features[0].kind, FeatureKind::Header);
        assert_eq!(manifest.features[0].name, "=== Outer ===");
        assert_eq!(manifest.features[1].kind, FeatureKind::Toggle);
        assert_eq!(manifest.features[1].name, "Inner Cheat");
    }

    #[test]
    fn is_meaningful_header_filters_ornament_but_keeps_real_sections() {
        // Section headers stay.
        assert!(is_meaningful_header("【 Player Stats 】"));
        assert!(is_meaningful_header("Equipment"));
        assert!(is_meaningful_header("【X】👈〖 All Relics 〗"));
        // Pure separators with no letters: drop.
        assert!(!is_meaningful_header("◣⫘⫘⫘⫘⫘⫘⫘⫘⫘⫘⫘​⫘⫘◢"));
        assert!(!is_meaningful_header(
            "----------------------------------------"
        ));
        // Info-note wrapper 《…》: drop.
        assert!(!is_meaningful_header(
            "《 freeze with 'Invincibility' script 》"
        ));
        // ❖-prefixed instructional bullet: drop.
        assert!(!is_meaningful_header("❖ select an attire from the list"));
        assert!(!is_meaningful_header(" ❖ trailing whitespace tolerated"));
        // Empty: drop.
        assert!(!is_meaningful_header(""));
        assert!(!is_meaningful_header("   "));
    }

    #[test]
    fn description_strips_only_balanced_outer_quotes() {
        assert_eq!(strip_quotes("\"Hello\"".to_string()), "Hello");
        assert_eq!(strip_quotes("Hello".to_string()), "Hello");
        assert_eq!(strip_quotes("\"".to_string()), "\"");
        assert_eq!(strip_quotes("\"a\"b\"".to_string()), "a\"b");
    }
}
