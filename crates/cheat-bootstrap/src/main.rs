//! Bootstrap Win32 EXE — entry point of the Aurora-style co-launch
//! spike. Steam invokes `proton waitforexitandrun bootstrap.exe
//! <game.exe>` (via the wrapper script); bootstrap then CreateProcess-
//! spawns the bridge and the real game as siblings INSIDE THE SAME
//! Proton invocation. Because both children inherit the parent's SLR
//! container + wineserver, the bridge's `ToolHelp32` enumeration can
//! see the game's PID — unlike the previous spike where the bridge
//! lived in a separate `proton waitforexitandrun` invocation and got
//! its own isolated container.
//!
//! Usage (from the wrapper script):
//!
//!   cheat-bootstrap.exe <game-win32-path> [game args...]
//!
//! The bridge path is hard-coded to `C:\users\Public\cheat-bridge.exe`
//! (the in-prefix location the wrapper stages).

#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, INFINITE, PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess,
    WaitForSingleObject,
};

const BRIDGE_PATH: &str = r"C:\users\Public\cheat-bridge.exe";
const BRIDGE_ARGS: &str =
    r#"--target-exe EnderMagnoliaSteam-Win64-Shipping.exe --iters 1000 --bytes 256"#;
const LOG_PATH: &str = r"C:\users\Public\cheat-bootstrap.log";

/// Stdout/stderr from CreateProcess'd children isn't reliably routed
/// up to the Proton invocation's stdout under Wine + SLR, so we log
/// to a file inside the prefix that the host wrapper can read after
/// the game exits.
fn log(msg: impl std::fmt::Display) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
    {
        let _ = writeln!(f, "{msg}");
    }
}

fn main() -> ExitCode {
    // Truncate the log so each run starts clean.
    let _ = std::fs::File::create(LOG_PATH);
    log(format_args!("bootstrap: start, argv = {:?}", std::env::args().collect::<Vec<_>>()));

    let mut argv = std::env::args();
    let _self = argv.next();
    let game_exe = match argv.next() {
        Some(p) => p,
        None => {
            log("usage: cheat-bootstrap.exe <game-win32-path> [game args...]");
            return ExitCode::from(2);
        }
    };
    let extra_args: Vec<String> = argv.collect();
    log(format_args!("bootstrap: game_exe = {game_exe}"));

    // Spawn the bridge first. The bridge polls ToolHelp32 for up to
    // 30 s so racing the game's inner exe is fine.
    let bridge_cmdline = format!(r#""{BRIDGE_PATH}" {BRIDGE_ARGS}"#);
    let bridge = match create_process(&bridge_cmdline) {
        Ok(info) => info,
        Err(e) => {
            log(format_args!("bootstrap: bridge CreateProcessW failed: {e}"));
            return ExitCode::from(3);
        }
    };
    log(format_args!(
        "bootstrap: spawned bridge pid {}",
        bridge.dwProcessId
    ));

    // Spawn the game with its original args. Best-effort quoting — for
    // most Steam invocations there are no extra args, just the path.
    let mut game_cmdline = format!(r#""{game_exe}""#);
    for arg in &extra_args {
        game_cmdline.push(' ');
        game_cmdline.push_str(arg);
    }
    let game = match create_process(&game_cmdline) {
        Ok(info) => info,
        Err(e) => {
            log(format_args!("bootstrap: game CreateProcessW failed: {e}"));
            unsafe { TerminateProcess(bridge.hProcess, 1) };
            unsafe {
                CloseHandle(bridge.hThread);
                CloseHandle(bridge.hProcess);
            }
            return ExitCode::from(4);
        }
    };
    log(format_args!(
        "bootstrap: spawned game pid {}",
        game.dwProcessId
    ));

    // Block on the game; bridge runs to completion (1000 iters ~ 30 ms)
    // and exits long before this returns.
    let wait_rc = unsafe { WaitForSingleObject(game.hProcess, INFINITE) };
    log(format_args!(
        "bootstrap: game exited (wait rc = {wait_rc:#x})"
    ));

    // Reap. TerminateProcess on bridge is no-op if it already exited.
    unsafe {
        TerminateProcess(bridge.hProcess, 0);
        CloseHandle(bridge.hThread);
        CloseHandle(bridge.hProcess);
        CloseHandle(game.hThread);
        CloseHandle(game.hProcess);
    }

    ExitCode::SUCCESS
}

fn create_process(cmdline: &str) -> Result<PROCESS_INFORMATION, String> {
    let mut wide: Vec<u16> = OsString::from(cmdline)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            ptr::null(),
            wide.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            FALSE,
            0,
            ptr::null(),
            ptr::null(),
            &mut si,
            &mut pi,
        )
    };
    if ok == 0 {
        Err("CreateProcessW returned 0".to_string())
    } else {
        Ok(pi)
    }
}
