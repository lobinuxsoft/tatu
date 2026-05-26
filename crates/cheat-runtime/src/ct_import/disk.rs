//! Filesystem entry points: scan `cheat-tables/<app_id>/`, write into
//! `trainers/<app_id>/`. Idempotent — already-converted tables get skipped.

use std::fs;
use std::path::Path;

use crate::manifest::Manifest;

use super::{CtImportError, ImportReport, convert_ct_file_with_exe_hint};

const CT_SUBDIR: &str = "backlog-tracker/cheat-tables";
const MANIFEST_SUBDIR: &str = "backlog-tracker/trainers";

/// Resolve `$XDG_CONFIG_HOME/backlog-tracker/cheat-tables/<app_id>/`. Used
/// by the UI's "Import .CT" command to drop a fresh `.ct` into the
/// directory the auto-importer scans.
pub fn ct_tables_dir_for(app_id: &str) -> Result<std::path::PathBuf, CtImportError> {
    let config = dirs::config_dir().ok_or(CtImportError::NoConfigDir)?;
    Ok(config.join(CT_SUBDIR).join(app_id))
}

/// Auto-import every `.ct` file under `cheat-tables/<app_id>/` into the
/// corresponding `trainers/<app_id>/` manifest directory.
///
/// Idempotent: a `.ct` whose target manifest already exists is reported as
/// `skipped`. Per-file errors don't abort the pass — they accumulate in
/// `failed` so callers can log them.
pub fn auto_import_for_app(app_id: &str) -> Result<ImportReport, CtImportError> {
    auto_import_for_app_with_exe_hint(app_id, None)
}

/// Variant that takes a fallback exe hint — see
/// [`crate::ct_import::convert_ct_file_with_exe_hint`]. The Tauri import
/// command passes Steam's detected exe so tables that authored without
/// `aobscanmodule` or `{ Game : X.exe }` still convert.
pub fn auto_import_for_app_with_exe_hint(
    app_id: &str,
    exe_hint: Option<&str>,
) -> Result<ImportReport, CtImportError> {
    let config = dirs::config_dir().ok_or(CtImportError::NoConfigDir)?;
    let src_dir = config.join(CT_SUBDIR).join(app_id);
    let dst_dir = config.join(MANIFEST_SUBDIR).join(app_id);
    import_dirs_with_exe_hint(&src_dir, &dst_dir, exe_hint)
}

/// Auto-import every `<app_id>/` subdirectory under `cheat-tables/`. Used
/// from the Tauri startup hook so a freshly-dropped `.ct` becomes visible
/// without the user needing to know about a separate "import" step.
pub fn auto_import_default_dirs() -> Result<ImportReport, CtImportError> {
    let config = dirs::config_dir().ok_or(CtImportError::NoConfigDir)?;
    let tables_root = config.join(CT_SUBDIR);
    let mut report = ImportReport::default();
    if !tables_root.is_dir() {
        return Ok(report);
    }
    let trainers_root = config.join(MANIFEST_SUBDIR);
    for entry in fs::read_dir(&tables_root).map_err(|source| CtImportError::Io {
        path: tables_root.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| CtImportError::Io {
            path: tables_root.clone(),
            source,
        })?;
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let app_id = entry.file_name();
        let src = entry.path();
        let dst = trainers_root.join(&app_id);
        let pass = import_dirs(&src, &dst)?;
        report.created.extend(pass.created);
        report.skipped.extend(pass.skipped);
        report.failed.extend(pass.failed);
    }
    Ok(report)
}

/// Internal entry point taking explicit src/dst dirs — keeps the integration
/// tests self-contained without touching `$XDG_CONFIG_HOME`.
pub fn import_dirs(src: &Path, dst: &Path) -> Result<ImportReport, CtImportError> {
    import_dirs_with_exe_hint(src, dst, None)
}

/// Variant accepting a fallback exe hint applied to every `.ct` in `src`.
pub fn import_dirs_with_exe_hint(
    src: &Path,
    dst: &Path,
    exe_hint: Option<&str>,
) -> Result<ImportReport, CtImportError> {
    let mut report = ImportReport::default();
    if !src.is_dir() {
        return Ok(report);
    }
    let read = fs::read_dir(src).map_err(|source| CtImportError::Io {
        path: src.to_path_buf(),
        source,
    })?;
    for entry in read {
        let entry = entry.map_err(|source| CtImportError::Io {
            path: src.to_path_buf(),
            source,
        })?;
        let ct = entry.path();
        if !ct.extension().is_some_and(|e| e.eq_ignore_ascii_case("ct")) {
            continue;
        }
        let stem = match ct.file_stem() {
            Some(s) => s.to_owned(),
            None => continue,
        };
        let target = dst.join(format!("{}.json", stem.to_string_lossy()));
        if target.exists() {
            report.skipped.push(ct);
            continue;
        }
        match convert_ct_file_with_exe_hint(&ct, exe_hint) {
            Ok(manifest) => {
                if let Err(e) = write_manifest(&target, &manifest) {
                    report.failed.push((ct, e));
                } else {
                    report.created.push(target);
                }
            }
            Err(e) => report.failed.push((ct, e)),
        }
    }
    Ok(report)
}

fn write_manifest(target: &Path, manifest: &Manifest) -> Result<(), CtImportError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|source| CtImportError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let body = serde_json::to_string_pretty(manifest)?;
    fs::write(target, body).map_err(|source| CtImportError::Io {
        path: target.to_path_buf(),
        source,
    })?;
    Ok(())
}
