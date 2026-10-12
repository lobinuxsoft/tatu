// Which library games are installed on THIS machine right now, per store —
// the "Solo instalados" filter. Read fresh from each launcher's own records
// on every call (a few small files and registry keys), never cached in
// state.json: an uninstall done outside Tatu must never leave a stale
// "installed" behind. Every source also checks the install folder still
// exists — launchers' records outlive games moved or deleted by hand, and
// drives that aren't plugged in (live: GOG's registry still pointed at a
// Downloads folder the game had left; Steam still listed an absent drive).
//
// Epic/GOG detection on Windows mirrors GameNativeTool's `game-scanner`
// (Galaxy's registry keys, the Epic launcher's `.item` manifests); Heroic's
// own JSON covers both stores on Linux/Steam Deck (and Heroic on Windows).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::egs_account::EgsOwnedGame;
use crate::state::AppState;

#[derive(Debug, Default, Serialize)]
pub struct InstalledGames {
    pub steam: Vec<u64>,
    pub gog: Vec<u64>,
    /// Tatu's derived EGS ids (`EgsOwnedGame::id`), not Epic's `app_name`s,
    /// so the frontend matches all three stores the same way.
    pub egs: Vec<u64>,
}

pub fn scan(egs_library: &[EgsOwnedGame]) -> InstalledGames {
    let heroic = heroic_roots();
    let epic = epic_installs(&heroic);
    InstalledGames {
        steam: steam_app_ids(),
        gog: gog_installs(&heroic).into_keys().collect(),
        egs: egs_library
            .iter()
            .filter(|g| epic.contains_key(&g.app_name))
            .map(|g| g.id)
            .collect(),
    }
}

/// A game as the install lookup needs it, copied out of the state: the
/// lookup itself reads registries, launcher records and Steam's files, and
/// that never happens while holding the state lock.
#[derive(Debug, Clone)]
pub enum GameRef {
    Steam(u64),
    NonSteam { name: String, root: String },
    Gog(u64),
    Egs { app_name: String },
}

pub fn game_ref(state: &AppState, source: &str, id: u64) -> Result<GameRef, String> {
    Ok(match source {
        "steam" => GameRef::Steam(id),
        "non_steam" => {
            let game = state
                .non_steam
                .iter()
                .find(|g| g.id == id)
                .ok_or_else(|| format!("Juego Non-Steam {id} no encontrado"))?;
            GameRef::NonSteam {
                name: game.name.clone(),
                root: game.install_root().to_string(),
            }
        }
        "gog" => GameRef::Gog(id),
        "egs" => GameRef::Egs {
            app_name: state
                .egs_library
                .iter()
                .find(|g| g.id == id)
                .map(|g| g.app_name.clone())
                .ok_or_else(|| format!("Juego de Epic {id} no encontrado"))?,
        },
        other => return Err(format!("Origen desconocido: {other}")),
    })
}

/// The folder a game is installed in on this machine, from its own store's
/// records: Steam's `common/<installdir>`, a Non-Steam shortcut's install
/// root, GOG/Epic from Heroic or their Windows launchers.
pub fn folder(game: &GameRef) -> Result<PathBuf, String> {
    const NOT_HERE: &str = "Este juego no está instalado en esta PC";
    let dir = match game {
        GameRef::Steam(id) => {
            crate::steam::find_install_path(&id.to_string()).map_err(|_| NOT_HERE.to_string())?
        }
        GameRef::NonSteam { name, root } => {
            if root.is_empty() {
                return Err(format!(
                    "\"{name}\" no tiene una carpeta del juego registrada — corregila en Info › Carpeta del juego"
                ));
            }
            PathBuf::from(root)
        }
        GameRef::Gog(id) => gog_installs(&heroic_roots())
            .remove(id)
            .ok_or_else(|| NOT_HERE.to_string())?,
        GameRef::Egs { app_name } => epic_installs(&heroic_roots())
            .remove(app_name)
            .ok_or_else(|| NOT_HERE.to_string())?,
    };
    if !dir.is_dir() {
        return Err(format!(
            "La carpeta \"{}\" no existe o no es accesible desde esta máquina",
            dir.display()
        ));
    }
    Ok(dir)
}

/// [`game_ref`] then [`folder`], for callers that already hold the state.
pub fn game_folder(state: &AppState, source: &str, id: u64) -> Result<PathBuf, String> {
    folder(&game_ref(state, source, id)?)
}

/// Steam's own per-library list (`libraryfolders.vdf`, the same read
/// "Escanear tamaño" does), kept only where the manifest AND the game folder
/// are really there: Steam keeps listing a library whose drive is unplugged
/// (live: an absent `E:\SteamLibrary` still "had" Alone in the Dark).
fn steam_app_ids() -> Vec<u64> {
    crate::steam::steam_install_dir()
        .and_then(|dir| crate::disk::scan_installed_sizes(&dir).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|size| size.app_id)
        .filter(|id| crate::steam::find_install_path(&id.to_string()).is_ok())
        .collect()
}

/// Heroic's config folder: native (`~/.config/heroic`, `%APPDATA%\heroic`)
/// and Flatpak — the usual way Heroic lands on a Steam Deck.
fn heroic_roots() -> Vec<PathBuf> {
    let native = dirs::config_dir().map(|config| config.join("heroic"));
    #[cfg(unix)]
    let flatpak = dirs::home_dir()
        .map(|home| home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"));
    #[cfg(not(unix))]
    let flatpak = None;
    native.into_iter().chain(flatpak).collect()
}

fn gog_installs(heroic: &[PathBuf]) -> HashMap<u64, PathBuf> {
    let mut installs = HashMap::new();
    for root in heroic {
        // `{"installed": [{"appName": "<product id>", "install_path": ...}]}`
        let Some(json) = read_json(&root.join("gog_store/installed.json")) else {
            continue;
        };
        for game in json["installed"].as_array().into_iter().flatten() {
            if let Some(id) = game["appName"].as_str().and_then(|s| s.parse().ok())
                && let Some(dir) = existing_dir(game["install_path"].as_str())
            {
                installs.insert(id, dir);
            }
        }
    }
    #[cfg(windows)]
    installs.extend(galaxy_installs());
    installs
}

/// One `GOG.com\Games\<product id>` key per install, written by GOG's own
/// installers (Galaxy's or the offline ones) whether or not Galaxy runs.
#[cfg(windows)]
fn galaxy_installs() -> Vec<(u64, PathBuf)> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut installs = Vec::new();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for subkey in [
            r"SOFTWARE\WOW6432Node\GOG.com\Games",
            r"SOFTWARE\GOG.com\Games",
        ] {
            let Ok(games) = RegKey::predef(hive).open_subkey(subkey) else {
                continue;
            };
            for name in games.enum_keys().flatten() {
                let path = games
                    .open_subkey(&name)
                    .and_then(|game| game.get_value::<String, _>("path"))
                    .ok();
                if let Ok(id) = name.parse()
                    && let Some(dir) = existing_dir(path.as_deref())
                {
                    installs.push((id, dir));
                }
            }
        }
    }
    installs
}

fn epic_installs(heroic: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut installs = HashMap::new();
    for root in heroic {
        // Legendary's own format: `{"<app_name>": {"app_name", "install_path", ...}}`
        let Some(json) = read_json(&root.join("legendaryConfig/legendary/installed.json")) else {
            continue;
        };
        for game in json.as_object().into_iter().flat_map(|o| o.values()) {
            if let Some(name) = game["app_name"].as_str()
                && let Some(dir) = existing_dir(game["install_path"].as_str())
            {
                installs.insert(name.to_string(), dir);
            }
        }
    }
    #[cfg(windows)]
    installs.extend(epic_launcher_installs());
    installs
}

/// The Epic Games Launcher's per-install `.item` manifests (JSON). A
/// download still in progress already has one — `bIsIncompleteInstall`.
#[cfg(windows)]
fn epic_launcher_installs() -> Vec<(String, PathBuf)> {
    let Some(program_data) = std::env::var_os("ProgramData") else {
        return Vec::new();
    };
    let dir = PathBuf::from(program_data).join(r"Epic\EpicGamesLauncher\Data\Manifests");
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "item"))
        .filter_map(|path| read_json(&path))
        .filter(|item| !item["bIsIncompleteInstall"].as_bool().unwrap_or(false))
        .filter_map(|item| {
            let dir = existing_dir(item["InstallLocation"].as_str())?;
            Some((item["AppName"].as_str()?.to_string(), dir))
        })
        .collect()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn existing_dir(path: Option<&str>) -> Option<PathBuf> {
    path.filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn heroic_installs_count_only_while_their_folder_exists() {
        let root = tempfile::tempdir().unwrap();
        let game_dir = root.path().join("Games/Hades");
        fs::create_dir_all(&game_dir).unwrap();
        let there = game_dir.to_string_lossy().replace('\\', "/");
        let gone = root
            .path()
            .join("moved")
            .to_string_lossy()
            .replace('\\', "/");

        fs::create_dir_all(root.path().join("gog_store")).unwrap();
        fs::write(
            root.path().join("gog_store/installed.json"),
            format!(
                r#"{{"installed":[{{"appName":"900000001","install_path":"{there}"}},
                                 {{"appName":"900000002","install_path":"{gone}"}}]}}"#
            ),
        )
        .unwrap();
        fs::create_dir_all(root.path().join("legendaryConfig/legendary")).unwrap();
        fs::write(
            root.path().join("legendaryConfig/legendary/installed.json"),
            format!(
                r#"{{"TatuTestThere":{{"app_name":"TatuTestThere","install_path":"{there}"}},
                    "TatuTestGone":{{"app_name":"TatuTestGone","install_path":"{gone}"}}}}"#
            ),
        )
        .unwrap();

        let heroic = [root.path().to_path_buf()];
        let gog = gog_installs(&heroic);
        assert_eq!(gog.get(&900000001), Some(&game_dir));
        assert!(!gog.contains_key(&900000002));
        let epic = epic_installs(&heroic);
        assert_eq!(epic.get("TatuTestThere"), Some(&game_dir));
        assert!(!epic.contains_key("TatuTestGone"));
    }
}
