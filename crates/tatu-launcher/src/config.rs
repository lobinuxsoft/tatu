use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// User-facing config at `~/.config/tatu/launcher.toml`.
///
/// Per-appid `[games.<id>]` overrides + global `default_proton`
/// fallback. The tracker writes this file when the user toggles
/// "Enable Tatu" or picks a Proton via the cheats panel. Hand-edits
/// stay valid because the writer round-trips through
/// [`toml::to_string_pretty`] and the schema is intentionally
/// append-friendly (every per-game field is optional so future
/// fields don't break existing files).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub default_proton: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub games: HashMap<String, GameConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_exe: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub tatu_enabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
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
    #[error("serialize {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
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

    /// Load the on-disk config, or fall back to a sensible empty
    /// scaffold when the file is missing. Used by the tracker so the
    /// first "Enable Tatu" click works on a fresh install without
    /// needing the user to seed `launcher.toml` first.
    pub fn load_or_default() -> Result<Self, ConfigError> {
        match Self::load() {
            Ok(c) => Ok(c),
            Err(ConfigError::Missing(_)) => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    pub fn game(&self, app_id: &str) -> Option<&GameConfig> {
        self.games.get(app_id)
    }

    /// Insert or overwrite the per-app config block. Empty entries
    /// (`GameConfig::default()` — all options None, tatu_enabled
    /// false) are dropped so the TOML stays tidy.
    pub fn upsert_game(&mut self, app_id: impl Into<String>, game: GameConfig) {
        let app_id = app_id.into();
        if game == GameConfig::default() {
            self.games.remove(&app_id);
        } else {
            self.games.insert(app_id, game);
        }
    }

    /// Drop the per-app config block entirely (post-Revert).
    pub fn remove_game(&mut self, app_id: &str) {
        self.games.remove(app_id);
    }

    /// Serialize back to disk via temp-file + rename so a crash mid
    /// write can never leave `launcher.toml` half-written. Creates
    /// the parent directory if missing.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let text = toml::to_string_pretty(self).map_err(|source| ConfigError::Serialize {
            path: path.clone(),
            source,
        })?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tatu.tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, text).map_err(|source| ConfigError::Io {
            path: tmp.clone(),
            source,
        })?;
        fs::rename(&tmp, &path).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_proton: "Proton - Experimental".to_string(),
            games: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        let mut games = HashMap::new();
        games.insert(
            "2725260".to_string(),
            GameConfig {
                proton: Some("GE-Proton9-25".to_string()),
                target_exe: Some("EnderMagnoliaSteam-Win64-Shipping.exe".to_string()),
                tatu_enabled: true,
            },
        );
        Config {
            default_proton: "Proton - Experimental".to_string(),
            games,
        }
    }

    #[test]
    fn round_trip_preserves_layout() {
        let original = sample();
        let serialized = toml::to_string_pretty(&original).unwrap();
        let parsed: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn upsert_drops_empty_entry() {
        let mut cfg = sample();
        cfg.upsert_game("9999999", GameConfig::default());
        assert!(!cfg.games.contains_key("9999999"));
    }

    #[test]
    fn upsert_overwrites_existing() {
        let mut cfg = sample();
        cfg.upsert_game(
            "2725260",
            GameConfig {
                proton: Some("Proton - Experimental".to_string()),
                target_exe: None,
                tatu_enabled: false,
            },
        );
        let entry = cfg.games.get("2725260").unwrap();
        assert_eq!(entry.proton.as_deref(), Some("Proton - Experimental"));
        assert!(entry.target_exe.is_none());
        assert!(!entry.tatu_enabled);
    }

    #[test]
    fn remove_game_clears_entry() {
        let mut cfg = sample();
        cfg.remove_game("2725260");
        assert!(cfg.games.is_empty());
    }

    #[test]
    fn serialized_skips_empty_optional_fields() {
        let mut games = HashMap::new();
        games.insert(
            "1".to_string(),
            GameConfig {
                proton: None,
                target_exe: None,
                tatu_enabled: true,
            },
        );
        let cfg = Config {
            default_proton: "Proton - Experimental".to_string(),
            games,
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        // Locate the [games.1] block to assert against only its body
        // (otherwise `default_proton =` at the top matches the
        // substring `proton =` and the check is meaningless).
        let games_body = text.split("[games.1]").nth(1).expect("games block");
        assert!(
            !games_body.contains("proton ="),
            "per-app proton should be skipped when None: {games_body:?}"
        );
        assert!(
            !games_body.contains("target_exe ="),
            "target_exe should be skipped when None: {games_body:?}"
        );
        assert!(
            games_body.contains("tatu_enabled = true"),
            "tatu_enabled stays when true: {games_body:?}"
        );
    }

    #[test]
    fn default_config_has_sensible_proton() {
        let cfg = Config::default();
        assert_eq!(cfg.default_proton, "Proton - Experimental");
        assert!(cfg.games.is_empty());
    }
}
