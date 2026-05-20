use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// User-facing config at `~/.config/tatu/launcher.toml`.
///
/// Per-appid `[games.<id>]` overrides + global `default_proton`
/// fallback. Phase 6/7 will gain a writer from the tracker UI; the
/// schema is intentionally append-friendly (every per-game field is
/// optional so future fields don't break existing files).
#[derive(Debug, Deserialize)]
pub struct Config {
    pub default_proton: String,
    #[serde(default)]
    pub games: HashMap<String, GameConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GameConfig {
    pub proton: Option<String>,
    pub target_exe: Option<String>,
    #[serde(default)]
    pub tatu_enabled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no config dir resolvable (HOME unset?)")]
    NoConfigDir,
    #[error("config not found at {0}")]
    Missing(PathBuf),
    #[error("read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

impl Config {
    pub fn path() -> Result<PathBuf, ConfigError> {
        dirs::config_dir()
            .map(|d| d.join("tatu").join("launcher.toml"))
            .ok_or(ConfigError::NoConfigDir)
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::path()?;
        if !path.exists() {
            return Err(ConfigError::Missing(path));
        }
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| ConfigError::Parse { path, source })
    }

    pub fn game(&self, app_id: &str) -> Option<&GameConfig> {
        self.games.get(app_id)
    }
}
