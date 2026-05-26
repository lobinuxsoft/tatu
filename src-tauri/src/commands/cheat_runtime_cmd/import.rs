//! `cheat_runtime_import_ct` — user-driven `.CT` import via the UI button.
//!
//! Counterpart to the startup auto-importer (`auto_import_default_dirs`):
//! the frontend sends a fresh `.CT` blob, we drop it into
//! `~/.config/backlog-tracker/cheat-tables/<app_id>/<file_name>` and run
//! `auto_import_for_app` to materialise the manifest under
//! `trainers/<app_id>/`. The result is reported back to the UI so it can
//! refresh the panel and surface any per-table conversion failures.

use std::fs;

use cheat_runtime::{auto_import_for_app, ct_tables_dir_for};
use serde::Serialize;

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

    let report = auto_import_for_app(&app_id).map_err(|e| e.to_string())?;
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
