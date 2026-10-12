// The tracker's side of the GameNative tab. "Enviar a Android": the adb work
// itself — push, SHA-256 verify, rename into place — is the `tatu-android`
// crate, and which local folder IS the game is `installed::game_folder`; this
// only keeps what to remember between sends. Plus each game's saved "best
// configuration" (`GameNativeConfig`).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Internal storage, outside `Android/data` — Android 11+ hides that whole
/// tree from the system folder picker GameNative's "Añadir juego
/// personalizado" uses, so a game copied there could never be added.
pub const DEFAULT_REMOTE_ROOT: &str = "/sdcard/Games";

/// Where "Descargar adb" puts Google's platform-tools — next to `state.json`.
pub fn platform_tools_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("backlog-tracker")
        .join("platform-tools")
}

/// Everything the GameNative tab keeps in `state.json`, in one place.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AndroidState {
    /// Explicit adb binary; empty = look in the SDK env vars, PATH and
    /// Android Studio's default SDK folder (`tatu_android::Adb::locate`).
    #[serde(default)]
    pub adb_path: String,
    /// Destination of the last send, so the next one starts there.
    #[serde(default)]
    pub remote_root: String,
    /// Keyed by [`config_key`] — `"steam:<appid>"` / `"non_steam:<id>"` —
    /// since a Steam appid and a shortcut id share no namespace.
    #[serde(default)]
    pub configs: HashMap<String, GameNativeConfig>,
}

/// A game's best-known GameNative setup, written down by the user so it
/// isn't lost — never applied automatically. `container` uses GameNative's
/// own container setting keys (Winlator's `ContainerData`, the same JSON its
/// "Export config" writes), so it could be handed back to GameNative's
/// "Import config" later. Which keys exist is the frontend's call; this only
/// stores them.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameNativeConfig {
    /// The user's own name for it ("60 fps estable", "Turnip + DXVK async").
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub container: serde_json::Map<String, serde_json::Value>,
    /// In-game settings that go with it (graphics preset, resolution, ...).
    #[serde(default)]
    pub notes: String,
}

impl GameNativeConfig {
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.container.is_empty() && self.notes.trim().is_empty()
    }
}

pub fn config_key(source: &str, id: u64) -> String {
    format!("{source}:{id}")
}

impl AndroidState {
    pub fn remote_root_or_default(&self) -> &str {
        if self.remote_root.is_empty() {
            DEFAULT_REMOTE_ROOT
        } else {
            &self.remote_root
        }
    }
}
