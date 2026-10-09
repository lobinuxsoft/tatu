//! Upscalers for games installed on this PC.
//!
//! - [`detect`] reads a game folder: the upscaler DLLs it ships and their
//!   versions, anti-cheat, where OptiScaler would go, what's already patched.
//! - [`gpu`] says which GPUs this PC has: vendor, DLSS-capable driver, RDNA
//!   generation.
//! - [`package`] fetches the pinned OptiScaler release from GitHub, SHA-256
//!   verified — downloaded by the user's own copy of Tatu, never bundled.
//! - [`patch`] is the one way this crate changes a game folder: journaled and
//!   reversible. Every file a patch replaces is moved into a backup first, and
//!   revert puts back exactly what was there — or leaves both alone and says
//!   so when the game changed the file in between.
//! - [`optiscaler`] builds the OptiScaler install on top of [`patch`],
//!   following the release's own setup scripts; [`settings`] are the image
//!   options Tatu writes into its ini, at install or later.
//! - [`official`] fetches each upscaler's newest official runtime (DLSS, FSR,
//!   XeSS) from its vendor's own SDK repository, pinned like the OptiScaler
//!   release.

pub mod detect;
pub mod gpu;
mod ini;
pub mod official;
pub mod optiscaler;
pub mod package;
pub mod patch;
mod pe;
pub mod settings;
#[cfg(test)]
mod test_support;

pub use ini::IniOption;
pub use pe::{file_version, is_dlss_dll, is_optiscaler, version_newer_or_equal};

/// Everything that can go wrong. Messages are user-facing (Spanish, like the
/// rest of Tatu's UI) — the tracker shows them verbatim.
#[derive(Debug, thiserror::Error)]
pub enum UpscalerError {
    #[error("La descarga falló: {0}")]
    Download(String),
    #[error("Lo descargado no coincide con la versión esperada (SHA-256 distinto)")]
    Checksum,
    #[error("No pude descomprimir OptiScaler: {0}")]
    Extract(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("El juego está abierto — cerralo antes de modificar sus archivos")]
    GameRunning,
    #[error(
        "No puedo escribir en {0}. ¿Está en una carpeta protegida o en un disco de solo lectura?"
    )]
    NotWritable(String),
    #[error("No hay espacio: hacen falta {needed_mb} MB y quedan {available_mb} MB")]
    NoSpace { needed_mb: u64, available_mb: u64 },
    #[error("Ya hay un parche \"{0}\" aplicado en este juego — revertilo primero")]
    AlreadyApplied(String),
    #[error("{0} ya lo modificó otro parche de Tatu (\"{1}\") — revertí ese primero")]
    Overlap(String, String),
    #[error("Ya existe un respaldo de {0} y no lo piso")]
    BackupExists(String),
    #[error(
        "Quedó un respaldo de una instalación anterior en {0} (puede ser el original del juego). \
         Revisalo antes de volver a instalar"
    )]
    OrphanBackup(String),
    #[error("No hay ningún parche \"{0}\" aplicado en este juego")]
    NotApplied(String),
    #[error("El registro del parche está dañado: {0}")]
    Manifest(String),
    #[error("Descarga cancelada")]
    Cancelled,
    #[error(
        "El OptiScaler descargado cambió desde la descarga ({0} no coincide; ¿lo tocó un antivirus?). \
         Lo borré: volvé a prepararlo"
    )]
    Tampered(String),
    #[error("{0}")]
    Invalid(String),
}

pub(crate) fn io_err(path: &std::path::Path, source: std::io::Error) -> UpscalerError {
    UpscalerError::Io {
        path: path.display().to_string(),
        source,
    }
}
