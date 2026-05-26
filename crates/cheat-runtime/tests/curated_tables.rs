//! Issue #136 — table-driven regression suite.
//!
//! Two layers of coverage:
//!
//! 1. **Curated fixtures** in `tests/fixtures/ct/` — small synthetic `.CT`
//!    XML files authored to exercise one parser/executor surface each
//!    (basic toggle, wildcard cleanup, multi-arg lists, `globalalloc` +
//!    `define`, Lua-only script, mixed `{$lua}` + `{$asm}` blocks). These
//!    run on every `cargo test`; if they break, the parser or executor
//!    contract has regressed.
//!
//! 2. **Optional external corpus walker** — when the
//!    `TATU_CT_CORPUS=/path/to/dir` env var is set the suite walks the
//!    directory and asserts the global parse rate is ≥ 99%. The fixture
//!    layer is licensing-clean (we wrote the synthetic `.CT`); this
//!    layer lets the developer point at a private corpus (e.g. the
//!    `/tmp/ct-audit` set the FearLess audit downloaded) without
//!    redistributing third-party tables.
//!
//! Tests deliberately stop at parse + (where possible) `Engine::enable`
//! against a no-op backend. Running the full enable cycle needs a real
//! game process; that's covered by the `#[ignore]`d
//! `alloc_remote_smoke` test (closed under #130).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use cheat_runtime::parser::{NameList, Statement, parse};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ct");

/// Extract every `<AssemblerScript>` body from a `.CT` XML file.
fn extract_scripts(xml: &str) -> Vec<String> {
    use roxmltree::Document;
    let mut out = Vec::new();
    let Ok(doc) = Document::parse(xml) else {
        return out;
    };
    for node in doc
        .descendants()
        .filter(|n| n.has_tag_name("AssemblerScript"))
    {
        if let Some(t) = node.text() {
            out.push(t.to_string());
        }
    }
    out
}

fn load_fixture(name: &str) -> Vec<String> {
    let path = Path::new(FIXTURE_DIR).join(name);
    let xml = fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"));
    let scripts = extract_scripts(&xml);
    assert!(
        !scripts.is_empty(),
        "fixture {name} has no <AssemblerScript> blocks — bad XML?"
    );
    scripts
}

#[test]
fn fixture_01_basic_toggle() {
    let scripts = load_fixture("01_basic_toggle.ct");
    let script = parse(&scripts[0]).expect("must parse");
    assert!(!script.lua_only);

    // Sanity: ENABLE has an aobscanmodule + alloc + label-site, DISABLE
    // has dealloc + unregistersymbol.
    let has_aobscan = script
        .enable
        .iter()
        .any(|s| matches!(s, Statement::AobScanModule { symbol, .. } if symbol == "injectsite"));
    let has_alloc = script
        .enable
        .iter()
        .any(|s| matches!(s, Statement::Alloc { symbol, .. } if symbol == "newmem"));
    let has_dealloc = script.disable.iter().any(
        |s| matches!(s, Statement::Dealloc(NameList::Names(n)) if n.iter().any(|x| x == "newmem")),
    );
    assert!(has_aobscan, "expected aobscanmodule(injectsite, …)");
    assert!(has_alloc, "expected alloc(newmem, …)");
    assert!(has_dealloc, "expected dealloc(newmem) in DISABLE");
}

#[test]
fn fixture_02_wildcard_cleanup() {
    // Pre-#131 this parsed with 2 BadCall errors (`unregistersymbol(*)`
    // and `dealloc(*)`). Pinning the post-#131 shape here makes any
    // regression immediately visible.
    let scripts = load_fixture("02_wildcard_cleanup.ct");
    let script = parse(&scripts[0]).expect("wildcard cleanup must parse");

    let wildcard_unreg = script
        .disable
        .iter()
        .any(|s| matches!(s, Statement::UnregisterSymbol(NameList::Wildcard)));
    let wildcard_dealloc = script
        .disable
        .iter()
        .any(|s| matches!(s, Statement::Dealloc(NameList::Wildcard)));
    assert!(wildcard_unreg, "expected unregistersymbol(*) in DISABLE");
    assert!(wildcard_dealloc, "expected dealloc(*) in DISABLE");
}

#[test]
fn fixture_03_multi_arg_lists() {
    // Pre-#131 the space-separated `label(a b c …)` raised BadCall(label).
    // Pin both shapes (space- and comma-separated) here.
    let scripts = load_fixture("03_multi_arg_lists.ct");
    let script = parse(&scripts[0]).expect("multi-arg lists must parse");

    let label_names = script
        .enable
        .iter()
        .find_map(|s| match s {
            Statement::Label(NameList::Names(names)) => Some(names.clone()),
            _ => None,
        })
        .expect("expected a Label(Names(...)) statement");
    assert_eq!(
        label_names,
        vec![
            "pHealth".to_string(),
            "pSpirit".into(),
            "pStamina".into(),
            "bMaxStamina".into(),
            "bInfHealth".into(),
        ],
        "label(a b c d e) must split on whitespace",
    );

    let reg_names = script
        .enable
        .iter()
        .find_map(|s| match s {
            Statement::RegisterSymbol(NameList::Names(names)) => Some(names.clone()),
            _ => None,
        })
        .expect("expected a RegisterSymbol(Names(...))");
    assert_eq!(
        reg_names,
        vec![
            "scanSite".to_string(),
            "pHealth".into(),
            "bMaxStamina".into()
        ],
        "registersymbol(a,b,c) must split on comma",
    );
}

#[test]
fn fixture_04_globalalloc_and_define() {
    let scripts = load_fixture("04_globalalloc_and_define.ct");
    let script = parse(&scripts[0]).expect("globalalloc + define must parse");

    let has_globalalloc = script
        .enable
        .iter()
        .any(|s| matches!(s, Statement::GlobalAlloc { symbol, size } if symbol == "newmem" && *size == 0x100));
    assert!(has_globalalloc, "expected globalalloc(newmem, 0x100)");

    let defines: HashMap<&str, &str> = script
        .enable
        .iter()
        .filter_map(|s| match s {
            Statement::Define { name, value } => Some((name.as_str(), value.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(defines.get("slot"), Some(&"0x40"));
    assert_eq!(defines.get("injectOffset"), Some(&"fixture.exe+CD2FC1"));
}

#[test]
fn fixture_05_lua_only_marks_script() {
    // No [ENABLE] block. Parser must NOT return MissingEnable — it must
    // surface `lua_only=true` so the UI can mark the feature as
    // "Lua scripting not supported" without breaking the table.
    let scripts = load_fixture("05_lua_only.ct");
    let script = parse(&scripts[0]).expect("lua-only must parse");
    assert!(
        script.lua_only,
        "{{$lua}}-only script must set lua_only=true"
    );
    assert!(script.enable.is_empty());
    assert!(script.disable.is_empty());
}

#[test]
fn fixture_06_mixed_lua_asm_marks_lua_only() {
    // {$lua} inside an [ENABLE] block also forces lua_only=true — CE
    // would hand the block to its Lua interpreter, and we can't
    // half-execute the surrounding asm safely.
    let scripts = load_fixture("06_mixed_lua_asm.ct");
    let script = parse(&scripts[0]).expect("mixed lua+asm must parse");
    assert!(
        script.lua_only,
        "{{$lua}} inside ENABLE must force lua_only=true"
    );
}

/// Walk every fixture, assert each script parses without error AND that
/// none surfaces a `MissingEnable` for a fixture that was authored with
/// an `[ENABLE]` block. Catches accidental regressions in `parse()`
/// header detection.
#[test]
fn every_fixture_parses_cleanly() {
    let mut total = 0usize;
    let mut errors = Vec::new();
    let dir = Path::new(FIXTURE_DIR);
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ct") {
            continue;
        }
        let xml = fs::read_to_string(&path).unwrap();
        for (idx, script) in extract_scripts(&xml).into_iter().enumerate() {
            total += 1;
            if let Err(e) = parse(&script) {
                errors.push(format!(
                    "{}#{idx}: {e}",
                    path.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    assert!(
        total >= 6,
        "expected at least 6 fixture scripts, got {total}"
    );
    assert!(
        errors.is_empty(),
        "{} fixture scripts failed to parse:\n  {}",
        errors.len(),
        errors.join("\n  ")
    );
}

/// Optional walker over a private `.CT` corpus pointed at via the
/// `TATU_CT_CORPUS` env var. We use this against the FearLess audit set
/// (`/tmp/ct-audit`, 17 tables / 287 scripts at the time of #131) without
/// committing third-party tables to the repo.
///
/// `#[ignore]` by default — CI cannot point at a private corpus, and the
/// developer running this locally explicitly opts in:
///
/// ```bash
/// TATU_CT_CORPUS=/tmp/ct-audit \
///   cargo test -p cheat-runtime --test curated_tables -- --ignored
/// ```
#[test]
#[ignore = "set TATU_CT_CORPUS=/path/to/.ct/dir and rerun with --ignored"]
fn external_corpus_parse_rate_at_least_99_percent() {
    let dir = std::env::var("TATU_CT_CORPUS")
        .expect("TATU_CT_CORPUS must point at a directory of .ct files");
    let dir = Path::new(&dir);

    let mut total = 0usize;
    let mut ok = 0usize;
    let mut errors = Vec::new();

    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ct") {
            continue;
        }
        let xml = fs::read_to_string(&path).unwrap();
        for (idx, script) in extract_scripts(&xml).into_iter().enumerate() {
            total += 1;
            match parse(&script) {
                Ok(_) => ok += 1,
                Err(e) => errors.push(format!(
                    "{}#{idx}: {e}",
                    path.file_name().unwrap().to_string_lossy()
                )),
            }
        }
    }

    assert!(total > 0, "no scripts found in corpus {dir:?}");
    let rate = ok as f64 / total as f64;
    println!("corpus parse rate: {ok}/{total} = {:.2}%", rate * 100.0);
    if !errors.is_empty() {
        eprintln!("first 10 errors:");
        for e in errors.iter().take(10) {
            eprintln!("  {e}");
        }
    }
    assert!(
        rate >= 0.99,
        "corpus parse rate {:.2}% < 99% threshold ({} failures)",
        rate * 100.0,
        errors.len()
    );
}
