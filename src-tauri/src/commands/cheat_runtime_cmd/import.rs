//! `cheat_runtime_import_ct` — user-driven `.CT` import via the UI button.
//!
//! Post-#134: there is no JSON sidecar to materialise — the loader parses
//! `.ct` files directly on every `list_features` call. Import is now just
//! "drop the bytes into `cheat-tables/<app_id>/<file_name>` and validate
//! the table parses". A failed validation still leaves the `.ct` on disk
//! so the user can edit it externally and retry without re-uploading.

use std::fs;

use cheat_runtime::{convert_ct_file_with_exe_hint, ct_tables_dir_for};
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

/// Delete a previously imported `.ct` from the user's library. Post-#134
/// there's no companion JSON manifest to clean up — the loader synthesises
/// manifests from `.ct` on demand, so removing the `.ct` is the whole job.
///
/// Idempotent: missing files are silently ignored — the goal is "after
/// this call, the named `.ct` is gone", not "a strict atomic transaction
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
    let ct_path = ct_dir.join(&file_name);
    if ct_path.exists() {
        fs::remove_file(&ct_path)
            .map_err(|e| format!("failed to delete {}: {e}", ct_path.display()))?;
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
