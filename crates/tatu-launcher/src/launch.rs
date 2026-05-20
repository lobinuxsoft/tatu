use std::env;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::GameConfig;

/// Bridge filename the launcher copies into the prefix on every
/// opted-in launch. Must match what `scripts/build-tatu-launcher.sh`
/// drops next to this binary.
const BRIDGE_NAME: &str = "tatu-bridge.exe";

/// Rewrites `<verb> <game.exe> [args...]` into
/// `<verb> <prefix>/users/Public/tatu-bridge.exe --launch <game.exe>
///         --target-exe <name> [args...]` and execs the real Proton.
///
/// Never returns on success (execs the child process tree).
pub fn run_with_bridge(
    proton: &Path,
    launcher_dir: &Path,
    game: &GameConfig,
    verb: &str,
    cmd: &[String],
) -> Result<(), LaunchError> {
    let (game_exe, rest) = cmd.split_first().ok_or(LaunchError::EmptyCommand)?;
    let prefix = compat_data_path()?;
    let public = prefix.join("pfx/drive_c/users/Public");
    fs::create_dir_all(&public).map_err(|source| LaunchError::Io {
        path: public.clone(),
        source,
    })?;

    let bridge_src = launcher_dir.join(BRIDGE_NAME);
    let bridge_dst = public.join(BRIDGE_NAME);
    fs::copy(&bridge_src, &bridge_dst).map_err(|source| LaunchError::Io {
        path: bridge_src,
        source,
    })?;

    let target_exe = game
        .target_exe
        .clone()
        .or_else(|| basename(game_exe))
        .ok_or_else(|| LaunchError::TargetExeUnresolvable(game_exe.clone()))?;

    let mut args = Vec::with_capacity(cmd.len() + 4);
    args.push(verb.to_owned());
    args.push(bridge_dst.to_string_lossy().into_owned());
    args.push("--launch".to_owned());
    args.push(game_exe.clone());
    args.push("--target-exe".to_owned());
    args.push(target_exe);
    args.extend_from_slice(rest);

    exec(proton, &args)
}

/// Verbatim delegation. Steam calls `getcompatpath` / `getnativepath`
/// / `run` for housekeeping; rewriting them would break compat
/// queries. Also the codepath for unopted games on any verb.
pub fn passthrough(proton: &Path, verb: &str, cmd: &[String]) -> Result<(), LaunchError> {
    let mut args = Vec::with_capacity(cmd.len() + 1);
    args.push(verb.to_owned());
    args.extend_from_slice(cmd);
    exec(proton, &args)
}

fn exec(proton: &Path, args: &[String]) -> Result<(), LaunchError> {
    let err = Command::new(proton).args(args).exec();
    Err(LaunchError::Exec {
        proton: proton.to_owned(),
        source: err,
    })
}

fn compat_data_path() -> Result<PathBuf, LaunchError> {
    env::var_os("STEAM_COMPAT_DATA_PATH")
        .map(PathBuf::from)
        .ok_or(LaunchError::NoCompatDataPath)
}

fn basename(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("STEAM_COMPAT_DATA_PATH not set — Steam should always set this for waitforexitandrun")]
    NoCompatDataPath,
    #[error("Steam passed an empty command after the verb")]
    EmptyCommand,
    #[error("target_exe missing from config and basename of {0} unresolvable")]
    TargetExeUnresolvable(String),
    #[error("io on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("exec proton at {proton}: {source}")]
    Exec {
        proton: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
