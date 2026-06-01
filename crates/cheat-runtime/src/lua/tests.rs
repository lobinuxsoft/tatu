//! Phase 0 (memory) + phase 1 (process / symbols / assembler / UI stub) tests.
//!
//! `process_vm_readv`/`writev` can target our own pid, so the memory PoCs drive
//! real cross-process primitives against a local buffer — no child spawn. The
//! Auto-Assembler enable path against a live game is covered by smoke, not here.

use super::*;

fn self_pid() -> Pid {
    nix::unistd::getpid()
}

fn runtime() -> LuaRuntime {
    LuaRuntime::new(self_pid()).unwrap()
}

// --- phase 0: memory family -------------------------------------------------

#[test]
fn reads_integer_float_and_pointer_from_process_memory() {
    let cells: [u64; 3] = [0x1234_5678, f32::to_bits(1.5) as u64, 0xDEAD_BEEF];
    let base = cells.as_ptr() as u64;
    let rt = runtime();

    let i: i64 = rt.eval(&format!("return readInteger({base})")).unwrap();
    assert_eq!(i, 0x1234_5678);

    let f: f64 = rt.eval(&format!("return readFloat({})", base + 8)).unwrap();
    assert_eq!(f, 1.5);

    let p: i64 = rt
        .eval(&format!("return readPointer({})", base + 16))
        .unwrap();
    assert_eq!(p as u64, 0xDEAD_BEEF);
}

#[test]
fn writes_back_into_process_memory() {
    let mut cell: u64 = 0;
    let addr = (&mut cell as *mut u64) as u64;
    let rt = runtime();

    rt.exec(&format!("writeInteger({addr}, 0x4242)")).unwrap();
    assert_eq!(cell as u32, 0x4242);

    rt.exec(&format!("writeFloat({addr}, 2.5)")).unwrap();
    assert_eq!(f32::from_bits(cell as u32), 2.5);
}

#[test]
fn failed_read_returns_nil_not_error() {
    let rt = runtime();
    // Page 0 is never mapped — CE returns nil, so must we (no Lua error).
    let v: Option<i64> = rt.eval("return readInteger(0)").unwrap();
    assert_eq!(v, None);
}

#[test]
fn read_write_round_trip_through_lua_logic() {
    let mut cell: u64 = 100;
    let addr = (&mut cell as *mut u64) as u64;
    let rt = runtime();
    rt.exec(&format!(
        "local v = readInteger({addr}); writeInteger({addr}, v * 2 + 1)"
    ))
    .unwrap();
    assert_eq!(cell as u32, 201);
}

// --- phase 1: process / modules ---------------------------------------------

#[test]
fn open_process_retargets_and_reports_pid() {
    let rt = runtime();
    let pid = self_pid().as_raw();
    let opened: i64 = rt.eval(&format!("return openProcess({pid})")).unwrap();
    assert_eq!(opened, pid as i64);
    let reported: i64 = rt.eval("return getOpenedProcessID()").unwrap();
    assert_eq!(reported, pid as i64);
}

#[test]
fn unknown_process_name_resolves_to_nil() {
    let rt = runtime();
    let v: Option<i64> = rt
        .eval("return getProcessIDFromProcessName('definitely-not-a-real-proc-xyz')")
        .unwrap();
    assert_eq!(v, None);
}

#[test]
fn enum_modules_lists_loaded_images() {
    let rt = runtime();
    let count: i64 = rt.eval("local m = enumModules(); return #m").unwrap();
    assert!(count > 0, "self process must have mapped modules");
    let first_addr: i64 = rt.eval("return enumModules()[1].Address").unwrap();
    assert!(first_addr > 0);
}

// --- phase 1: shared symbol table -------------------------------------------

#[test]
fn registered_symbol_resolves_through_get_address() {
    let rt = runtime();
    rt.exec("registerSymbol('mySym', 0x140000)").unwrap();
    let addr: i64 = rt.eval("return getAddress('mySym')").unwrap();
    assert_eq!(addr as u64, 0x140000);
    // CE treats the offset after `+` as hex.
    let off: i64 = rt.eval("return getAddress('mySym+10')").unwrap();
    assert_eq!(off as u64, 0x140010);
}

#[test]
fn unregister_symbol_removes_it() {
    let rt = runtime();
    rt.exec("registerSymbol('tmp', 0x1000)").unwrap();
    rt.exec("unregisterSymbol('tmp')").unwrap();
    let v: Option<i64> = rt.eval("return getAddress('tmp')").unwrap();
    assert_eq!(v, None);
}

#[test]
fn auto_assemble_parse_error_returns_false_and_message() {
    let rt = runtime();
    // `(((` is not a valid AA script — expect (false, message), not a crash.
    let (ok, msg): (bool, String) = rt
        .eval("local ok, m = autoAssemble('(((not valid'); return ok, m")
        .unwrap();
    assert!(!ok);
    assert!(!msg.is_empty());
}

// --- phase 1: UI stub bootstrap ---------------------------------------------

#[test]
fn ui_stub_absorbs_framework_bootstrap() {
    let rt = runtime();
    // Mimics the shape of a Manifold load block: build widgets, wire handlers,
    // set properties, register highlights. None of it must error.
    rt.exec(
        r#"
        local form = createForm()
        form.Caption = "Trainer"
        form.Width = 400
        local btn = createButton(form)
        btn.OnClick = function() end
        btn:setCaption("Enable")
        local timer = createTimer(form)
        timer.Interval = 100
        timer.OnTimer = function() end
        getMainForm().Menu = createMainMenu()
        registerLuaFunctionHighlight("toggleCheat")
        AddressList.getMemoryRecordByDescription("Health")
        "#,
    )
    .unwrap();
}

#[test]
fn ui_stub_fields_are_truthy() {
    let rt = runtime();
    // `if widget.Visible then` must take the truthy branch, not nil-crash.
    let v: bool = rt
        .eval("if MainForm.Visible then return true else return false end")
        .unwrap();
    assert!(v);
}

// --- smoke: load the real Manifold framework from a .CT (no game) -----------
//
// Decodes the framework modules straight out of a CE table — CE custom base85
// → raw deflate → `[u32 LE size][lua]` — mounts them via the runtime's loader
// API, and runs Manifold's bootstrap end to end. The table itself is the
// author's copyrighted artefact (gitignored), so the test skips when absent;
// drop a `.CT` in `.local-ct/` to exercise it locally:
//   cargo test -p cheat-runtime --lib manifold_bootstrap -- --ignored --nocapture
#[test]
#[ignore = "needs a CE table in .local-ct/ (gitignored)"]
fn manifold_bootstrap_loads_without_crashing() {
    use std::path::Path;
    let ct = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local-ct/DD2_v6.0.0_Full.ct");
    if !ct.exists() {
        eprintln!("SKIP: no CE table at {ct:?}");
        return;
    }
    let xml = std::fs::read_to_string(&ct).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();

    // Header (plain Lua) + the embedded *.lua modules (encoded).
    let header = doc
        .descendants()
        .find(|n| n.has_tag_name("LuaScript"))
        .and_then(|n| n.text())
        .expect("table has a LuaScript header")
        .to_string();
    let files = doc
        .descendants()
        .find(|n| n.has_tag_name("Files"))
        .expect("table has embedded Files");
    // Mount every embedded file (.lua modules, .json themes, .CEA scripts) the
    // same way the real loader would — all share the base85+deflate encoding.
    let modules: Vec<(String, String)> = files
        .children()
        .filter(|n| n.attribute("Encoding").is_some())
        .map(|n| {
            let name = n.tag_name().name().to_string();
            let bytes = decode_ce_module(n.text().unwrap_or(""));
            (name, String::from_utf8_lossy(&bytes).into_owned())
        })
        .collect();
    let lua_count = modules.iter().filter(|(n, _)| n.ends_with(".lua")).count();
    assert!(lua_count >= 10, "expected the Manifold module set");

    // Drive the *real* runtime; the loader API supplies the embedded modules.
    // The bootstrap logs a couple of non-fatal `[UI] ... theme` errors: the
    // theme menu walks CE's real LCL component tree (`miTable`), which the UI
    // stub can't synthesise. Manifold wraps that in pcall, so it's expected
    // noise — the bootstrap still completes. Replicating CE's GUI is out of
    // scope by design.
    let rt = runtime();
    rt.mount_table_files(modules).unwrap();
    match rt.exec(&header) {
        Ok(()) => eprintln!("\n==== MANIFOLD BOOTSTRAP: OK (no crash) ===="),
        Err(e) => panic!("\n==== MANIFOLD BOOTSTRAP FAILED ====\n{e}"),
    }
}

/// Decode one CE-embedded module: custom base85 → raw deflate → strip the
/// 4-byte little-endian length prefix, yielding the Lua source bytes.
#[cfg(test)]
fn decode_ce_module(blob: &str) -> Vec<u8> {
    use std::io::Read;
    let raw = ce_base85_decode(blob);
    let mut inflated = Vec::new();
    flate2::read::DeflateDecoder::new(&raw[..])
        .read_to_end(&mut inflated)
        .expect("raw deflate");
    let size = u32::from_le_bytes(inflated[..4].try_into().unwrap()) as usize;
    inflated[4..4 + size].to_vec()
}

/// CE's custom base85 (`custombase85.pas`): big-endian, 5 chars → 4 bytes,
/// short final group padded with the top digit. Non-charset bytes (the XML
/// indentation/newlines) are skipped.
#[cfg(test)]
fn ce_base85_decode(s: &str) -> Vec<u8> {
    const CHARSET: &[u8; 85] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%()*+,-./:;=?@[]^_{}";
    let digits: Vec<u8> = s
        .bytes()
        .filter_map(|b| CHARSET.iter().position(|&c| c == b).map(|p| p as u8))
        .collect();
    let mut out = Vec::new();
    for group in digits.chunks(5) {
        let pad = 5 - group.len();
        let mut value: u64 = 0;
        for i in 0..5 {
            let d = group.get(i).copied().unwrap_or(84) as u64;
            value = value * 85 + d;
        }
        let bytes = (value as u32).to_be_bytes();
        out.extend_from_slice(&bytes[..4 - pad]);
    }
    out
}
