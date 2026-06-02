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

// The end-to-end Manifold smoke (load a real .CT, bootstrap, run a cheat block)
// now lives in `framework` — it exercises the production loader, not a
// test-local decoder.
