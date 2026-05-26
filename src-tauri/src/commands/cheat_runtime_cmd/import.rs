//! `cheat_runtime_import_ct` — user-driven `.CT` import via the UI button.
//!
//! Counterpart to the startup auto-importer (`auto_import_default_dirs`):
//! the frontend sends a fresh `.CT` blob, we drop it into
//! `~/.config/backlog-tracker/cheat-tables/<app_id>/<file_name>` and run
//! `auto_import_for_app` to materialise the manifest under
//! `trainers/<app_id>/`. The result is reported back to the UI so it can
//! refresh the panel and surface any per-table conversion failures.

use std::fs;

use cheat_runtime::{auto_import_for_app_with_exe_hint, ct_tables_dir_for};
use serde::Serialize;

use crate::steam::detect_game_exe;

/// Shape returned to the frontend. `ImportReport` from cheat-runtime
/// carries `PathBuf`s and a non-serde error struct, so we project it to a
/// flat shape the JSON layer can stringify directly.
#[derive(Debug, Serialize, Default)]
pub struct ImportSummary {
    /// `.ct` files the importer just turned into manifests.
    pub created: Vec<String>,
    /// `.ct` files that already had a matching manifest on disk (idempotent).
    pub skipped: Vec<String>,
    /// `(file_name, error_string)` pairs for tables that couldn't convert.
    /// Surfaced separately so one bad table doesn't fail the whole import.
    pub failed: Vec<(String, String)>,
    /// Path the imported `.ct` was written to, before auto-import ran.
    /// Useful for the UI's confirmation toast.
    pub written_to: String,
}

/// Delete a previously imported `.ct` plus its derived `.json` manifest
/// from the user's library. Counterpart to [`cheat_runtime_import_ct`]:
/// the UI exposes an X button per table row so the user can prune
/// minimalist / broken / superseded tables without leaving the app.
///
/// Idempotent: missing files are silently ignored — the goal is "after
/// this call, the named pair is gone", not "a strict atomic transaction
/// failed because half of it was already gone".
#[tauri::command]
pub fn cheat_runtime_remove_ct(app_id: String, file_name: String) -> Result<(), String> {
    // Same anti-traversal guard as `cheat_runtime_import_ct`. We never
    // accept paths, only bare filenames.
    if file_name.contains('/')
        || file_name.contains('\\')
        || file_name == "."
        || file_name == ".."
        || file_name.is_empty()
    {
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

    // The companion manifest is `<stem>.json` next to the trainers dir.
    // `ct_tables_dir_for` returns `…/cheat-tables/<app_id>/`; the trainers
    // dir is its sibling `…/trainers/<app_id>/`. Resolve via cheat-runtime's
    // own helper to avoid hard-coding the layout twice.
    let trainers_dir = cheat_runtime::manifests_dir_for(&app_id).map_err(|e| e.to_string())?;
    let stem = std::path::Path::new(&file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !stem.is_empty() {
        let manifest_path = trainers_dir.join(format!("{stem}.json"));
        if manifest_path.exists() {
            fs::remove_file(&manifest_path)
                .map_err(|e| format!("failed to delete {}: {e}", manifest_path.display()))?;
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
    // Guard against path traversal / nested writes — the frontend can
    // only send a plain filename, never a path. Tauri's IPC layer doesn't
    // validate this for us; we enforce here.
    if file_name.contains('/')
        || file_name.contains('\\')
        || file_name == "."
        || file_name == ".."
        || file_name.is_empty()
    {
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

    // Last-resort exe hint: many tables on FearLess (Mono / Unity, hand-rolled
    // minimalist tables) lack both `aobscanmodule(_, exe, _)` and the
    // `{ Game : X.exe }` template comment, so ct_import can't infer the
    // binding from the file alone. Steam already knows the installed game's
    // exe via `appmanifest`, so we feed it in here and the import still
    // produces a usable manifest. Failure of `detect_game_exe` is non-fatal
    // — without the hint the importer falls back to its existing
    // `NoExeBinding` error, which the UI then surfaces.
    let exe_hint = detect_game_exe(&app_id).ok();
    let report = auto_import_for_app_with_exe_hint(&app_id, exe_hint.as_deref())
        .map_err(|e| e.to_string())?;
    let summary = ImportSummary {
        created: report
            .created
            .iter()
            .filter_map(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .collect(),
        skipped: report
            .skipped
            .iter()
            .filter_map(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .collect(),
        failed: report
            .failed
            .iter()
            .map(|(p, e)| {
                (
                    p.file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    e.to_string(),
                )
            })
            .collect(),
        written_to: written.display().to_string(),
    };
    Ok(summary)
}
