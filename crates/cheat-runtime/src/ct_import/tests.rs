//! Integration tests covering: end-to-end XML projection (header / toggle /
//! value), pointer-chain offset preservation, idempotent disk writes,
//! exe-binding failure modes, header-ornament filter, quote stripping.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::manifest::{FeatureKind, VType};

use super::heuristics::{is_meaningful_header, strip_quotes};
use super::{CtImportError, convert_ct_file, import_dirs};

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
fn convert_emits_header_toggle_and_value_skipping_lua() {
    let tmp = TempDir::new().unwrap();
    let ct = write_fixture(tmp.path(), "Game.ct", FIXTURE);
    let manifest = convert_ct_file(&ct).unwrap();
    assert_eq!(manifest.exe, "Game.exe");
    assert_eq!(manifest.title, "Game");
    assert_eq!(manifest.features.len(), 3);

    assert_eq!(manifest.features[0].kind, FeatureKind::Header);
    assert_eq!(manifest.features[0].name, "=== Player ===");
    assert!(manifest.features[0].script.is_none());

    assert_eq!(manifest.features[1].kind, FeatureKind::Toggle);
    assert_eq!(manifest.features[1].name, "Invincibility");
    let script = manifest.features[1].script.as_deref().unwrap();
    assert!(script.contains("aobscanmodule(INJECT,Game.exe"));

    assert_eq!(manifest.features[2].kind, FeatureKind::Value);
    assert_eq!(manifest.features[2].name, "HP");
    let v = manifest.features[2].value.as_ref().unwrap();
    assert_eq!(v.base_expr, "0x12345678");
    assert_eq!(v.vtype, VType::U32);
    assert!(v.offsets.is_empty());
}

#[test]
fn value_entry_with_offsets_preserves_document_order() {
    let tmp = TempDir::new().unwrap();
    let ct = write_fixture(
        tmp.path(),
        "Values.ct",
        r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"Bootstrap"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>[ENABLE]
aobscanmodule(INJECT,Game.exe,90 90 90)
alloc(base_address,8)
[DISABLE]
</AssemblerScript>
    </CheatEntry>
    <CheatEntry>
      <ID>2</ID>
      <Description>"Current HP"</Description>
      <VariableType>4 Bytes</VariableType>
      <Address>[base_address]+30</Address>
      <Offsets>
        <Offset>13C</Offset>
        <Offset>8B8</Offset>
        <Offset>2D0</Offset>
      </Offsets>
    </CheatEntry>
    <CheatEntry>
      <ID>3</ID>
      <Description>"Signed Counter"</Description>
      <ShowAsSigned>1</ShowAsSigned>
      <VariableType>4 Bytes</VariableType>
      <Address>[base_address]+8</Address>
    </CheatEntry>
    <CheatEntry>
      <ID>4</ID>
      <Description>"World X"</Description>
      <VariableType>Float</VariableType>
      <Address>[base_address]+10</Address>
    </CheatEntry>
    <CheatEntry>
      <ID>5</ID>
      <Description>"Inventory Ptr"</Description>
      <VariableType>8 Bytes</VariableType>
      <Address>[base_address]+18</Address>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#,
    );
    let m = convert_ct_file(&ct).unwrap();
    // Toggle + 4 values
    assert_eq!(m.features.len(), 5);
    let hp = m.features[1].value.as_ref().unwrap();
    assert_eq!(hp.base_expr, "[base_address]+30");
    // Document order preserved (NOT reversed at parse time — the walker
    // reverses, the schema stores them as the .CT does).
    assert_eq!(hp.offsets, vec![0x13C, 0x8B8, 0x2D0]);
    assert_eq!(hp.vtype, VType::U32);

    let signed = m.features[2].value.as_ref().unwrap();
    assert_eq!(signed.vtype, VType::I32);
    assert!(signed.offsets.is_empty());

    assert_eq!(m.features[3].value.as_ref().unwrap().vtype, VType::F32);
    assert_eq!(m.features[4].value.as_ref().unwrap().vtype, VType::U64);
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
fn nested_cheatentries_become_tree_children() {
    // CE wraps a CheatEntry's children in a single `<CheatEntries>` block.
    // Post-#133 the importer preserves that as `ManifestFeature::children`
    // instead of flattening it — the UI renders depth as a collapsible
    // tree.
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
    assert_eq!(manifest.features.len(), 1, "outer Header is the sole root");
    let outer = &manifest.features[0];
    assert_eq!(outer.kind, FeatureKind::Header);
    assert_eq!(outer.name, "=== Outer ===");
    assert_eq!(
        outer.children.len(),
        1,
        "outer must own the inner Toggle as child"
    );
    assert_eq!(outer.children[0].kind, FeatureKind::Toggle);
    assert_eq!(outer.children[0].name, "Inner Cheat");
}

#[test]
fn deep_nesting_preserves_full_tree() {
    // Depth-3 case (Header → Header → Toggle). Mirrors the FearLess
    // tables `crimson_desert_p6.ct` (depth 4) and `ender_magnolia_v11.ct`
    // (depth 5) at a tractable size.
    let tmp = TempDir::new().unwrap();
    let ct = write_fixture(
        tmp.path(),
        "Deep.ct",
        r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID><Description>"Player"</Description><GroupHeader>1</GroupHeader>
      <CheatEntries>
        <CheatEntry>
          <ID>2</ID><Description>"Combat"</Description><GroupHeader>1</GroupHeader>
          <CheatEntries>
            <CheatEntry>
              <ID>3</ID><Description>"God Mode"</Description>
              <VariableType>Auto Assembler Script</VariableType>
              <AssemblerScript>[ENABLE]
aobscanmodule(INJECT,Game.exe,90 90 90)
[DISABLE]
</AssemblerScript>
            </CheatEntry>
          </CheatEntries>
        </CheatEntry>
      </CheatEntries>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#,
    );
    let manifest = convert_ct_file(&ct).unwrap();
    assert_eq!(manifest.features.len(), 1);
    let leaf = &manifest.features[0].children[0].children[0];
    assert_eq!(leaf.kind, FeatureKind::Toggle);
    assert_eq!(leaf.name, "God Mode");
}

#[test]
fn skipped_parent_promotes_its_surviving_children() {
    // The outer Description is decorative ornament — `is_meaningful_header`
    // drops it. Without child-promotion the inner Toggle would be deleted
    // from the manifest too, which would baffle a user staring at a CE
    // table that visibly has the cheat.
    let tmp = TempDir::new().unwrap();
    let ct = write_fixture(
        tmp.path(),
        "Promote.ct",
        r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"-"</Description>
      <GroupHeader>1</GroupHeader>
      <CheatEntries>
        <CheatEntry>
          <ID>2</ID>
          <Description>"Real Cheat"</Description>
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
    assert_eq!(manifest.features.len(), 1);
    assert_eq!(manifest.features[0].kind, FeatureKind::Toggle);
    assert_eq!(manifest.features[0].name, "Real Cheat");
}

#[test]
fn is_meaningful_header_filters_ornament_but_keeps_real_sections() {
    // Section headers stay.
    assert!(is_meaningful_header("【 Player Stats 】"));
    assert!(is_meaningful_header("Equipment"));
    assert!(is_meaningful_header("【X】👈〖 All Relics 〗"));
    // Pure separators with no letters: drop.
    assert!(!is_meaningful_header("◣⫘⫘⫘⫘⫘⫘⫘⫘⫘⫘⫘\u{200B}⫘⫘◢"));
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
fn derive_exe_falls_back_to_comment_block_for_mono_tables() {
    // Real-world shape from the Enigma of Fear table (Mono game): no
    // `aobscanmodule(_, exe, _)` line — only `aobscan(INJECT, ...)`
    // because Mono targets aren't module-scoped. The exe name lives in
    // the convention comment block CE inserts at the top of generated
    // scripts. Without this fallback the import errors with
    // `NoExeBinding` and the user sees a cryptic failure.
    let tmp = TempDir::new().unwrap();
    let ct = write_fixture(
        tmp.path(),
        "Mono.ct",
        r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"healthnodamage aob"</Description>
      <VariableType>Auto Assembler Script</VariableType>
      <AssemblerScript>{ Game   : Enigma.exe
  Version: 1.1
  Date   : 2024-12-09
  Author : User
}

[ENABLE]
aobscan(INJECT, F2 0F 5C C1 F2 0F 5A E8)
alloc(newmem,$1000)
label(code)
label(return)
newmem:
code:
  jmp return
INJECT:
  jmp newmem
return:
[DISABLE]
</AssemblerScript>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#,
    );
    let manifest =
        convert_ct_file(&ct).expect("Mono table with `Game : X.exe` comment must import");
    assert_eq!(manifest.exe, "Enigma.exe");
}

#[test]
fn description_strips_only_balanced_outer_quotes() {
    assert_eq!(strip_quotes("\"Hello\"".to_string()), "Hello");
    assert_eq!(strip_quotes("Hello".to_string()), "Hello");
    assert_eq!(strip_quotes("\"".to_string()), "\"");
    assert_eq!(strip_quotes("\"a\"b\"".to_string()), "a\"b");
}
