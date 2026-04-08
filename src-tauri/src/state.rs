use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::shortcuts::NonSteamGame;
use crate::steam::Game;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    pub games: Vec<Game>,
    pub completed: HashSet<u64>,
    pub completed_nonsteam: HashSet<u64>,
    pub last_sync: Option<String>,
    #[serde(default)]
    pub non_steam: Vec<NonSteamGame>,
}

impl AppState {
    fn path() -> PathBuf {
        let config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = config.join("backlog-tracker");
        let _ = fs::create_dir_all(&dir);
        dir.join("state.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Ok(data) = serde_json::to_string(self) {
            let _ = fs::write(&path, data);
        }
    }
}
