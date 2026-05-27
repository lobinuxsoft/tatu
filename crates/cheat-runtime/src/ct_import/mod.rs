//! Cheat Engine `.CT` (CheatTable XML) → manifest auto-importer.
//!
//! Cheat Engine stores tables as XML with a `<CheatTable>` root containing
//! arbitrarily nested `<CheatEntry>` elements. Each entry can be a value
//! tweak (typed `<Address>` + `<VariableType>` like `4 Bytes`), an
//! Auto-Assembler script (paired `<VariableType>Auto Assembler Script</VariableType>`
//! and `<AssemblerScript>`), a group header (`<GroupHeader>1</GroupHeader>`,
//! description only), or a Lua-shim entry (using `luacall(...)` or `{$lua}`
//! blocks). The runtime only understands compiled AA scripts, so we project
//! the table down to the subset our [`crate::executor`] can execute and skip
//! everything else.
//!
//! ### Projection rules
//!
//! | CT entry shape | Manifest output |
//! |---|---|
//! | `Auto Assembler Script` whose body contains real AA (`aobscanmodule`, `alloc(`, `db `, `registersymbol`) | `Toggle` with the script body verbatim |
//! | `GroupHeader=1` (and not pure ornament) | `Header` (description only, no script) |
//! | Lua-only script (no AA primitives detected) | skipped |
//! | `<Address>` + numeric `<VariableType>` (`4 Bytes` / `8 Bytes` / `Float` / `Double`) | `Value` with [`ValueSpec`] — base expression + pointer-chain offsets + [`VType`] |
//! | Anything else (`Byte`, `String`, `Binary`, custom types) | skipped |
//!
//! ### Exe binding
//!
//! `Manifest::exe` is recovered by scanning the produced toggles for the
//! first `aobscanmodule(_, <Exe.exe>, _)` line. CT files always carry the
//! module name there because that's how CE binds the scan to a specific
//! image. If the table has no AA toggle (header-only or value-only), the
//! conversion fails — there's no useful manifest to emit.
//!
//! ## Submodule layout
//!
//! - [`xml_walker`] — descent over `<CheatEntry>` nodes + projection to
//!   [`crate::manifest::ManifestFeature`].
//! - [`heuristics`] — header-vs-ornament / Lua-vs-AA / exe-recovery rules.
//!
//! ### #134 — no more disk-side JSON cache
//!
//! Up to #163 the importer also persisted each converted manifest as a JSON
//! sidecar under `trainers/<app_id>/`. The loader has since switched to
//! parsing `.ct` files directly on every call (see
//! [`crate::manifest::load_manifests_for`]), so the disk-writer helpers
//! were removed: there is no longer a `.json` cache to keep in sync, and
//! the round-trip degradation it caused (Lua bodies, comments and CE-only
//! tweaks were thrown away) is gone. The single primitive callers still
//! need is [`convert_ct_file_with_exe_hint`], used by both the loader and
//! the import command (the latter for parse-time validation).

mod heuristics;
mod xml_walker;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::Manifest;

use self::heuristics::derive_exe;
use self::xml_walker::walk_entries;

#[derive(Debug, thiserror::Error)]
pub enum CtImportError {
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid CT XML at {path}: {source}")]
    Xml {
        path: PathBuf,
        #[source]
        source: roxmltree::Error,
    },
    #[error("table at {path} contains no convertible cheats")]
    Empty { path: PathBuf },
    #[error("table at {path} has cheats but no aobscanmodule line to recover the module name from")]
    NoExeBinding { path: PathBuf },
}

/// Convert a single `.ct` file into an in-memory [`Manifest`].
///
/// Errors with `NoExeBinding` if neither the AA scripts (`aobscanmodule`,
/// `{ Game : X.exe }` comment block) nor the caller supply an exe name.
/// Use [`convert_ct_file_with_exe_hint`] from the Tauri import command
/// where Steam already knows the installed game's exe.
pub fn convert_ct_file(path: &Path) -> Result<Manifest, CtImportError> {
    convert_ct_file_with_exe_hint(path, None)
}

/// Same as [`convert_ct_file`] but accepts a final-resort `exe_hint`.
/// CE tables authored without an `aobscanmodule` line and without the
/// `{ Game : X.exe }` template comment (notably Mono / minimalist
/// FearLess uploads) can't infer the binding from their own contents.
/// When the caller knows the game's exe via another channel (e.g.
/// Steam's `appmanifest`), pass it here so the import still succeeds
/// — the resulting manifest renders in the UI even if specific cheats
/// later fail because the hard-coded addresses don't match.
pub fn convert_ct_file_with_exe_hint(
    path: &Path,
    exe_hint: Option<&str>,
) -> Result<Manifest, CtImportError> {
    let text = fs::read_to_string(path).map_err(|source| CtImportError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let doc = roxmltree::Document::parse(&text).map_err(|source| CtImportError::Xml {
        path: path.to_path_buf(),
        source,
    })?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".to_string());

    let mut features = Vec::new();
    walk_entries(doc.root_element(), &stem, &mut features);

    if features.is_empty() {
        return Err(CtImportError::Empty {
            path: path.to_path_buf(),
        });
    }

    let exe = derive_exe(&features)
        .or_else(|| exe_hint.map(str::to_owned))
        .ok_or_else(|| CtImportError::NoExeBinding {
            path: path.to_path_buf(),
        })?;

    Ok(Manifest {
        exe,
        title: stem,
        features,
    })
}

#[cfg(test)]
mod tests;
