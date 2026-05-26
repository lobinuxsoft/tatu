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
//! - [`disk`] — directory traversal + idempotent on-disk writes.

mod disk;
mod heuristics;
mod xml_walker;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::Manifest;

use self::heuristics::derive_exe;
use self::xml_walker::walk_entries;

pub use self::disk::{auto_import_default_dirs, auto_import_for_app, import_dirs};

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
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
    #[error("manifest serialisation failed: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Default)]
pub struct ImportReport {
    /// Manifest files that were just written.
    pub created: Vec<PathBuf>,
    /// `.ct` files that already had a matching manifest on disk, so the
    /// importer left them alone.
    pub skipped: Vec<PathBuf>,
    /// `.ct` files the importer tried to convert but failed on. The errors
    /// surface here instead of aborting the whole pass so one bad table
    /// doesn't block the rest of the user's library.
    pub failed: Vec<(PathBuf, CtImportError)>,
}

/// Convert a single `.ct` file into an in-memory [`Manifest`].
pub fn convert_ct_file(path: &Path) -> Result<Manifest, CtImportError> {
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

    let exe = derive_exe(&features).ok_or_else(|| CtImportError::NoExeBinding {
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
