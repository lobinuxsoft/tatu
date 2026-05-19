//! Filesystem entry points: scan `cheat-tables/<app_id>/`, write into
//! `trainers/<app_id>/`. Idempotent — already-converted tables get skipped.

use std::fs;
use std::path::Path;

use crate::manifest::Manifest;

use super::{CtImportError, ImportReport, convert_ct_file};

const CT_SUBDIR: &str = "backlog-tracker/cheat-tables";
const MANIFEST_SUBDIR: &str = "backlog-tracker/trainers";

/// Auto-import every `.ct` file under `cheat-tables/<app_id>/` into the
/// corresponding `trainers/<app_id>/` manifest directory.
///
/// Idempotent: a `.ct` whose target manifest already exists is reported as
/// `skipped`. Per-file errors don't abort the pass — they accumulate in
/// `failed` so callers can log them.
pub fn auto_import_for_app(app_id: &str) -> Result<ImportReport, CtImportError> {
    let config = dirs::config_dir().ok_or(CtImportError::NoConfigDir)?;
    let src_dir = config.join(CT_SUBDIR).join(app_id);
    let dst_dir = config.join(MANIFEST_SUBDIR).join(app_id);
    import_dirs(&src_dir, &dst_dir)
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
        match convert_ct_file(&ct) {
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
