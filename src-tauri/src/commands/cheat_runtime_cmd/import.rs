//! `cheat_runtime_import_ct` — user-driven `.CT` import via the UI button.
//!
//! Post-#134: there is no JSON sidecar to materialise — the loader parses
//! `.ct` files directly on every `list_features` call. Import is now just
//! "drop the bytes into `cheat-tables/<app_id>/<file_name>` and validate
//! the table parses". A failed validation still leaves the `.ct` on disk
//! so the user can edit it externally and retry without re-uploading.

use std::fs;
use std::path::Path;

use cheat_runtime::{convert_ct_file_with_exe_hint, ct_tables_dir_for, manifests_dir_for};
use serde::Serialize;

use crate::steam::detect_game_exe;

/// Shape returned to the frontend. Kept similar to the pre-#134 summary so
/// the UI's toast renderer doesn't need to change: `imported` is now the
/// `.ct` file the user just dropped (always 0 or 1 element), and
/// `failed` carries the validation error if parse + convert can't build a
/// usable manifest. `skipped` was meaningful when the importer ran across
/// every `.ct` in the dir and skipped already-converted ones; with the
/// JSON cache gone there's nothing to skip and the field disappears.
#[derive(Debug, Serialize, Default)]
pub struct ImportSummary {
    /// `.ct` file the importer accepted (parses + has an exe binding).
    /// Empty when validation failed.
    pub imported: Vec<String>,
    /// `(file_name, error_string)` when validation failed. The `.ct` is
    /// still on disk — the user can fix it and retry without re-uploading.
    pub failed: Vec<(String, String)>,
    /// Path the `.ct` was written to, before validation ran. Used by the
    /// UI's confirmation toast.
    pub written_to: String,
}

/// Delete a previously imported `.ct` from the user's library. Besides the
/// `.ct` itself, this also drops any legacy `trainers/<app_id>/<stem>.json`
/// sidecar sharing the same file stem: pre-#134 imports (and the cheat-core
/// migrator) left a JSON manifest there, and the loader still falls back to
/// it when no `.ct` of that stem wins. Without this cleanup, removing the
/// `.ct` resurrects the stale JSON as phantom toggles in the UI.
///
/// Idempotent: missing files are silently ignored — the goal is "after
/// this call, the named table is gone", not "a strict atomic transaction
/// failed because half of it was already gone".
#[tauri::command]
pub fn cheat_runtime_remove_ct(app_id: String, file_name: String) -> Result<(), String> {
    if !is_safe_basename(&file_name) {
        return Err(format!(
            "rejected file name {file_name:?} — must be a bare basename"
        ));
    }
    if !file_name.to_ascii_lowercase().ends_with(".ct") {
        return Err(format!(
            "rejected {file_name:?} — only `.ct` filenames are accepted"
        ));
    }

    let ct_dir = ct_tables_dir_for(&app_id).map_err(|e| e.to_string())?;
    let legacy_dir = manifests_dir_for(&app_id).map_err(|e| e.to_string())?;
    remove_ct_from_dirs(&ct_dir, &legacy_dir, &file_name).map_err(|e| e.to_string())
}

/// Disk side of [`cheat_runtime_remove_ct`], split out with explicit dirs so
/// unit tests can run against a `TempDir` without poisoning the process-wide
/// `XDG_CONFIG_HOME`. Removes `<ct_dir>/<file_name>` and the legacy
/// `<legacy_dir>/<stem>.json` sidecar sharing the same stem. Idempotent.
fn remove_ct_from_dirs(ct_dir: &Path, legacy_dir: &Path, file_name: &str) -> std::io::Result<()> {
    let ct_path = ct_dir.join(file_name);
    if ct_path.exists() {
        fs::remove_file(&ct_path)?;
    }

    // The loader dedupes `cheat-tables/*.ct` against `trainers/*.json` by
    // file stem, so a leftover `<stem>.json` would re-appear as a phantom
    // manifest after the `.ct` is gone.
    // Build the JSON name by hand rather than `with_extension`: stems like
    // `DD2_v6.0.0_Full` contain dots, and `with_extension` would mistake the
    // last segment for an extension and clobber it.
    if let Some(stem) = Path::new(file_name).file_stem().and_then(|s| s.to_str()) {
        let legacy_path = legacy_dir.join(format!("{stem}.json"));
        if legacy_path.exists() {
            fs::remove_file(&legacy_path)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn cheat_runtime_import_ct(
    app_id: String,
    file_name: String,
    contents: Vec<u8>,
) -> Result<ImportSummary, String> {
    // Guard against path traversal — Tauri's IPC layer doesn't validate
    // this for us, so we enforce here. The frontend can only send a plain
    // filename, never a path.
    if !is_safe_basename(&file_name) {
        return Err(format!(
            "rejected file name {file_name:?} — must be a bare basename"
        ));
    }
    if !file_name.to_ascii_lowercase().ends_with(".ct") {
        return Err(format!(
            "rejected {file_name:?} — only `.ct` files are accepted"
        ));
    }

    let dir = ct_tables_dir_for(&app_id).map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    let written = dir.join(&file_name);
    fs::write(&written, &contents)
        .map_err(|e| format!("failed to write {}: {e}", written.display()))?;

    // Validate now so a malformed table surfaces in the import toast,
    // not later as a silent "table disappeared from the list". The Steam
    // exe hint covers Mono / Unity hand-rolled tables that lack both
    // `aobscanmodule(_, exe, _)` and the `{ Game : X.exe }` template
    // comment — without it those would fail validation here even though
    // the loader will pick them up fine at list time (using the same
    // hint, looked up per-call).
    let exe_hint = detect_game_exe(&app_id).ok();
    let mut summary = ImportSummary {
        written_to: written.display().to_string(),
        ..Default::default()
    };
    match convert_ct_file_with_exe_hint(&written, exe_hint.as_deref()) {
        Ok(_) => summary.imported.push(file_name),
        Err(e) => summary.failed.push((file_name, e.to_string())),
    }
    Ok(summary)
}

/// Same anti-traversal guard the pre-#134 import/remove path used —
/// frontend may only send a plain filename, never a path or relative
/// component. Factored out so import and remove agree on the rule.
fn is_safe_basename(file_name: &str) -> bool {
    !(file_name.contains('/')
        || file_name.contains('\\')
        || file_name == "."
        || file_name == ".."
        || file_name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn remove_ct_also_drops_legacy_json_sidecar() {
        let ct_dir = TempDir::new().unwrap();
        let legacy_dir = TempDir::new().unwrap();
        let ct = ct_dir.path().join("DD2_v6.0.0_Full.ct");
        let json = legacy_dir.path().join("DD2_v6.0.0_Full.json");
        fs::write(&ct, b"[ENABLE]").unwrap();
        fs::write(&json, b"{}").unwrap();

        remove_ct_from_dirs(ct_dir.path(), legacy_dir.path(), "DD2_v6.0.0_Full.ct").unwrap();

        assert!(!ct.exists(), "the .ct should be gone");
        assert!(!json.exists(), "the legacy JSON sidecar should be gone too");
    }

    #[test]
    fn remove_ct_is_idempotent_when_nothing_exists() {
        let ct_dir = TempDir::new().unwrap();
        let legacy_dir = TempDir::new().unwrap();
        // Neither file present — must not error.
        remove_ct_from_dirs(ct_dir.path(), legacy_dir.path(), "absent.ct").unwrap();
    }

    #[test]
    fn remove_ct_leaves_unrelated_stems_untouched() {
        let ct_dir = TempDir::new().unwrap();
        let legacy_dir = TempDir::new().unwrap();
        let other_json = legacy_dir.path().join("OtherTable.json");
        fs::write(&other_json, b"{}").unwrap();

        remove_ct_from_dirs(ct_dir.path(), legacy_dir.path(), "DD2.ct").unwrap();

        assert!(other_json.exists(), "a different stem must survive");
    }
}
