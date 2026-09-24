//! Round-trip tests for the legacy-JSON → manifest migrator.

use std::path::Path;

use tempfile::TempDir;

use crate::parser::{Script, Statement, parse as parse_script};

use super::*;

const EM_LEGACY: &str = r#"{
    "app_id": 2725260,
    "game_name": "ENDER MAGNOLIA",
    "exe_pattern": "EnderMagnoliaSteam-Win64-Shipping.exe",
    "cheats": [{
        "id": "1",
        "name": "Materials",
        "description": null,
        "address": { "kind": "Absolute", "address": 708486264 },
        "action": { "kind": "WriteOnce", "value": { "type": "u32", "value": 0 } }
    }]
}"#;

const PRAGMATA_LEGACY: &str = r#"{
    "app_id": 3357650,
    "game_name": "PRAGMATA",
    "exe_pattern": "PRAGMATA.exe",
    "cheats": [{
        "id": "full_heal",
        "name": "Full Heal",
        "description": "Smoke test heal",
        "address": { "kind": "Absolute", "address": "0xb056ec28" },
        "action": { "kind": "WriteOnce", "value": { "type": "f32", "value": 1600.0 } }
    }]
}"#;

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

#[test]
fn migrates_em_to_manifest_with_absolute_site() {
    let tmp = TempDir::new().unwrap();
    let legacy = tmp.path().join("legacy");
    let trainers = tmp.path().join("trainers");
    std::fs::create_dir_all(&legacy).unwrap();
    write(&legacy, "2725260.json", EM_LEGACY);

    let report = migrate_dirs(&legacy, &trainers).unwrap();
    assert_eq!(report.migrated, vec![2_725_260]);

    let target = trainers.join("2725260").join("legacy.json");
    let m: Manifest = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(m.exe, "EnderMagnoliaSteam-Win64-Shipping.exe");
    assert_eq!(m.title, "ENDER MAGNOLIA");
    assert_eq!(m.features.len(), 1);
    assert_eq!(m.features[0].name, "Materials");

    // The synthesised script must parse into AbsoluteSite + a db raw line.
    let script_src = m.features[0]
        .script
        .as_deref()
        .expect("migrated cheats are Toggle with a script");
    let Script { enable, .. } = parse_script(script_src).unwrap();
    assert!(matches!(enable[0], Statement::AbsoluteSite(708_486_264)));
    assert!(matches!(&enable[1], Statement::Raw(line) if line.starts_with("db ")));
}

#[test]
fn migrates_pragmata_with_hex_address_and_f32_bytes() {
    let tmp = TempDir::new().unwrap();
    let legacy = tmp.path().join("legacy");
    let trainers = tmp.path().join("trainers");
    std::fs::create_dir_all(&legacy).unwrap();
    write(&legacy, "3357650.json", PRAGMATA_LEGACY);

    migrate_dirs(&legacy, &trainers).unwrap();
    let target = trainers.join("3357650").join("legacy.json");
    let m: Manifest = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    let script = m.features[0]
        .script
        .as_deref()
        .expect("migrated cheats are Toggle with a script");
    // 1600.0f32 little-endian = 00 00 C8 44
    assert!(script.contains("0xB056EC28:"));
    assert!(script.contains("db 00 00 C8 44"));
}

#[test]
fn second_run_is_idempotent_and_reports_skipped() {
    let tmp = TempDir::new().unwrap();
    let legacy = tmp.path().join("legacy");
    let trainers = tmp.path().join("trainers");
    std::fs::create_dir_all(&legacy).unwrap();
    write(&legacy, "2725260.json", EM_LEGACY);

    migrate_dirs(&legacy, &trainers).unwrap();
    // Mutate the produced manifest, then re-run: skip must preserve it.
    let target = trainers.join("2725260").join("legacy.json");
    std::fs::write(&target, r#"{"exe":"x","title":"x","features":[]}"#).unwrap();

    let report = migrate_dirs(&legacy, &trainers).unwrap();
    assert_eq!(report.skipped, vec![2_725_260]);
    assert!(report.migrated.is_empty());
    let after = std::fs::read_to_string(&target).unwrap();
    assert!(after.contains(r#""features":[]"#));
}

#[test]
fn missing_legacy_dir_is_not_an_error() {
    let tmp = TempDir::new().unwrap();
    let report = migrate_dirs(
        &tmp.path().join("does-not-exist"),
        &tmp.path().join("trainers"),
    )
    .unwrap();
    assert!(report.migrated.is_empty());
    assert!(report.skipped.is_empty());
}

#[test]
fn static_address_is_reported_as_unsupported() {
    let tmp = TempDir::new().unwrap();
    let legacy = tmp.path().join("legacy");
    let trainers = tmp.path().join("trainers");
    std::fs::create_dir_all(&legacy).unwrap();
    let static_only = r#"{
        "app_id": 999,
        "game_name": "test",
        "exe_pattern": "x.exe",
        "cheats": [{
            "id": "s1",
            "name": "Static",
            "address": { "kind": "Static", "module": "x.exe", "offset": "0x100" },
            "action": { "kind": "WriteOnce", "value": { "type": "u32", "value": 1 } }
        }]
    }"#;
    write(&legacy, "999.json", static_only);

    let report = migrate_dirs(&legacy, &trainers).unwrap();
    assert_eq!(report.unsupported.len(), 1);
    assert_eq!(report.unsupported[0].0, 999);
    // A manifest is still written, just with zero features (the file
    // existed in legacy form so something was processed).
    let m: Manifest = serde_json::from_str(
        &std::fs::read_to_string(trainers.join("999").join("legacy.json")).unwrap(),
    )
    .unwrap();
    assert!(m.features.is_empty());
}

#[test]
fn invalid_json_surfaces_path_in_error() {
    let tmp = TempDir::new().unwrap();
    let legacy = tmp.path().join("legacy");
    let trainers = tmp.path().join("trainers");
    std::fs::create_dir_all(&legacy).unwrap();
    write(&legacy, "broken.json", "{not json");

    let err = migrate_dirs(&legacy, &trainers).unwrap_err();
    match err {
        MigrateError::Json { path, .. } => {
            assert!(path.ends_with("broken.json"));
        }
        other => panic!("expected Json error, got {other:?}"),
    }
}
