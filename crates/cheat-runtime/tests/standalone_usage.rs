//! Integration test that compiles the way a downstream crate (a Decky plugin
//! wrapper, a CLI, anything outside `src-tauri`) would consume `cheat-runtime`.
//!
//! Purpose: catch any accidental tauri / project-specific coupling slipping
//! into the public API. The test deliberately uses **only** the crate root
//! exports (no `cheat_runtime::executor::...` deep paths) so renaming an
//! internal module can't make this pass while breaking external consumers.
//!
//! No network, no Steam, no privilege. Spins up an in-process scenario:
//! - Hand-build a `Manifest` programmatically.
//! - Parse its script through the public `parse_script`.
//! - Drive the `Engine` against our own PID with `bind_symbol` to skip the
//!   AOB-scan path (the scanner has its own coverage).
//! - Apply, verify the write, disable, verify the rollback.

use std::collections::HashMap;

use cheat_runtime::{
    ActiveCheat, Engine, Feature, Manifest, ManifestFeature, MemoryRegion, Pattern, Perms, Script,
    Statement, Trainer, find_pids_by_exe, parse_script, read_bytes, scan, scan_in_process,
    write_bytes,
};
use nix::unistd::Pid;

#[test]
fn downstream_can_drive_full_enable_disable_via_public_root() {
    let mut victim = [0u8; 32];
    let original: [u8; 8] = [0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80];
    victim[8..16].copy_from_slice(&original);
    let target_addr = victim.as_ptr() as u64 + 8;

    let manifest = Manifest {
        exe: "doesntmatter.exe".into(),
        title: "Downstream Smoke".into(),
        prereqs: vec![],
        features: vec![ManifestFeature {
            uuid: "11111111-2222-3333-4444-555555555555".into(),
            name: "Zero out".into(),
            category: None,
            kind: cheat_runtime::FeatureKind::Toggle,
            script: Some(
                "[ENABLE]\n\
                 registersymbol(victim)\n\
                 victim:\n\
                 db 00 00 00 00 00 00 00 00\n\
                 [DISABLE]\n\
                 victim:\n\
                 db 10 20 30 40 50 60 70 80\n\
                 unregistersymbol(victim)\n"
                    .into(),
            ),
            value: None,
        }],
    };

    let feature = manifest.features.into_iter().next().unwrap();
    let script: Script =
        parse_script(feature.script.as_deref().expect("toggle has script")).expect("parse script");
    assert!(matches!(
        script.enable.first(),
        Some(Statement::RegisterSymbol(_))
    ));

    let mut engine = Engine::new(Pid::this());
    engine.bind_symbol("victim", target_addr);

    let active: ActiveCheat = engine.enable(&script).expect("enable should succeed");
    assert_eq!(active.writes(), 1);
    assert_eq!(&victim[8..16], &[0u8; 8]);

    active.disable().expect("disable should succeed");
    assert_eq!(&victim[8..16], &original);
}

#[test]
fn downstream_can_compose_scanner_with_memory_read() {
    // Buffer with a unique-ish needle, scan it, then verify we can read it
    // back at the address the scanner reports. Exercises the
    // scanner + memory + maps surface as a downstream crate would.
    let mut buf = [0u8; 256];
    let needle: [u8; 6] = [0xC0, 0xFF, 0xEE, 0xBA, 0xBE, 0x42];
    buf[100..106].copy_from_slice(&needle);

    let pat = Pattern::parse("C0 FF EE BA BE 42").expect("parse pattern");

    // Pure scan
    let hits = scan(&buf, &pat);
    assert_eq!(hits, vec![100]);

    // Process scan over an artificial region pointing at our buffer.
    let region = MemoryRegion {
        start: buf.as_ptr() as u64,
        end: buf.as_ptr() as u64 + buf.len() as u64,
        perms: Perms {
            read: true,
            write: true,
            execute: false,
            shared: false,
        },
        offset: 0,
        path: std::path::PathBuf::new(),
    };
    let abs_hits = scan_in_process(Pid::this(), &region, &pat).expect("process scan");
    assert_eq!(abs_hits, vec![buf.as_ptr() as u64 + 100]);

    let read_back = read_bytes(Pid::this(), abs_hits[0], 6).expect("read back");
    assert_eq!(read_back, needle);
}

#[test]
fn downstream_can_write_via_root_export() {
    let mut buf = [0u8; 16];
    let addr = buf.as_mut_ptr() as u64;
    write_bytes(Pid::this(), addr, b"hello-from-down!").expect("write_bytes via root");
    assert_eq!(&buf[..], b"hello-from-down!");
}

#[test]
fn downstream_can_use_aurora_and_manifest_types_at_root() {
    // Type-check that the Aurora and Manifest types are reachable from the
    // crate root, plus a HashMap-shaped use to verify the struct fields are
    // public the way a downstream consumer would expect.
    let mut by_uuid: HashMap<String, Feature> = HashMap::new();
    by_uuid.insert(
        "u1".into(),
        Feature {
            uuid: "u1".into(),
            name: "God Mode".into(),
            category: None,
        },
    );
    assert_eq!(by_uuid["u1"].name, "God Mode");

    let trainer = Trainer {
        exe: "Game.exe".into(),
        title: "Demo".into(),
        version: "1.0.0".into(),
        appid: Some(12345),
        title_id: None,
        features: vec![],
        scripts: vec![],
    };
    assert_eq!(trainer.appid, Some(12345));
}

#[test]
fn downstream_can_call_process_lookup_at_root() {
    // Just verifies the symbol is callable; result depends on the host.
    let _ = find_pids_by_exe("not-going-to-match-this-magic-string-49583");
}
