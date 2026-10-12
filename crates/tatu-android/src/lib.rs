//! Sending a locally installed game to an Android phone running GameNative
//! (<https://github.com/utkarshdalal/GameNative>), ported from GameNativeTool's
//! `adb-transport`.
//!
//! - [`Adb`] drives the user's own `adb` binary — Google's implementation
//!   handles USB on every OS far better than any Rust reimplementation of the
//!   protocol, and every Android user who can enable USB debugging can get it.
//!   [`platform_tools`] fetches it from Google for users who don't have it.
//! - [`send_folder`] is the whole transfer: push into a hidden staging folder,
//!   verify every file's SHA-256 against the original with ONE device-side
//!   command, then rename into place. GameNative never sees a half-copied game.
//!
//! The device is treated as untrusted: every remote path is validated
//! ([`validate_remote_path`]) and single-quoted before it reaches the phone's
//! shell.

mod adb;
pub mod perf;
pub mod platform_tools;
mod remote;
mod send;
mod verify;

pub use adb::{Adb, Device};
pub use remote::validate_remote_path;
pub use send::{Phase, Progress, send_folder};

/// Everything that can go wrong sending a game. Messages are user-facing
/// (Spanish, like the rest of Tatu's UI) — the tracker shows them verbatim.
#[derive(Debug, thiserror::Error)]
pub enum AndroidError {
    #[error(
        "No encontré adb. Instalá Android platform-tools (o Android Studio), o indicá dónde está adb"
    )]
    AdbNotFound,
    #[error("La ruta configurada para adb no existe: {0}")]
    AdbPathInvalid(String),
    #[error("No pude descargar adb: {0}")]
    Download(String),
    #[error("Conexión Wi-Fi: {0}")]
    Wireless(String),
    #[error("No pude ejecutar adb: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("adb falló ({command}): {stderr}")]
    Adb { command: String, stderr: String },
    #[error("Carpeta de destino inválida \"{path}\": {reason}")]
    InvalidRemotePath { path: String, reason: &'static str },
    #[error("No pude determinar el nombre de la carpeta del juego: {0}")]
    BadSource(String),
    #[error("No pude leer {path}: {source}")]
    LocalRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "Ya existe {0} en el dispositivo — borrala desde el teléfono o elegí otra carpeta de destino"
    )]
    AlreadyExists(String),
    #[error(
        "No entra en el dispositivo: el juego ocupa {needed_gb:.1} GB y quedan {available_gb:.1} GB libres"
    )]
    NoSpace { needed_gb: f64, available_gb: f64 },
    #[error("La copia en el dispositivo no coincide con la original: {0}")]
    VerifyMismatch(String),
    #[error("Envío cancelado")]
    Cancelled,
}
