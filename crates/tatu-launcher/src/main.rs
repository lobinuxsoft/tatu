//! Tatu Steam compatibility tool entry binary. See `tools/tatu-launcher/README.md`.
//!
//! Steam invokes us via `toolmanifest.vdf`'s commandline through
//! `tatu-launcher.sh` (LD_PRELOAD scrub) with:
//!
//!     tatu-launcher <verb> <game.exe> [args...]
//!
//! For opted-in `[games.<appid>]` (tatu_enabled = true) on the
//! `waitforexitandrun` verb we rewrite argv into the Aurora-style
//! co-launch handoff and exec the user's real Proton. Every other
//! shape passes through verbatim so the Steam compat query verbs
//! (getcompatpath / getnativepath / run) keep working.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tatu_launcher::config::{Config, ConfigError, GameConfig};
use tatu_launcher::launch::{self, LaunchError};
use tatu_launcher::proton::{self, ProtonError};

const VERB_WAITFOREXITANDRUN: &str = "waitforexitandrun";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tatu-launcher: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let argv: Vec<String> = env::args().skip(1).collect();
    let (verb, cmd) = argv.split_first().ok_or(Error::MissingVerb)?;

    let config = Config::load()?;
    let steam_root = proton::steam_root()?;
    let app_id = env::var("SteamAppId").ok();
    let launcher_dir = self_dir()?;

    let game = app_id.as_deref().and_then(|id| config.game(id));
    let proton_name = game
        .and_then(|g| g.proton.as_deref())
        .unwrap_or(&config.default_proton);
    let proton_path = proton::resolve(proton_name, &steam_root)?;

    match (verb.as_str(), game) {
        (VERB_WAITFOREXITANDRUN, Some(g)) if bridge_active(g) => {
            launch::run_with_bridge(&proton_path, &launcher_dir, g, verb, cmd)?;
        }
        _ => launch::passthrough(&proton_path, verb, cmd)?,
    }
    Ok(())
}

fn bridge_active(game: &GameConfig) -> bool {
    game.tatu_enabled
}

/// Directory the binary is installed into — same dir as
/// `tatu-bridge.exe` and the VDFs.
fn self_dir() -> Result<PathBuf, Error> {
    let exe = env::current_exe().map_err(Error::SelfPath)?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or(Error::SelfDirMissing)
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("missing verb (Steam should pass: <verb> <command...>)")]
    MissingVerb,
    #[error("resolving own executable path: {0}")]
    SelfPath(#[source] std::io::Error),
    #[error("own executable has no parent directory")]
    SelfDirMissing,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Proton(#[from] ProtonError),
    #[error(transparent)]
    Launch(#[from] LaunchError),
}
