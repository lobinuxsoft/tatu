//! Win32 bridge — runs under the same Wine prefix as the target game
//! and exercises cross-process memory ops via the Win32 API. The point
//! is to measure whether `VirtualAllocEx` + `WriteProcessMemory` +
//! `ReadProcessMemory` between two Win32 PE processes (bridge + game)
//! living inside Wine carry the same COW variance the Linux
//! `cheat-runtime` backend hits when it `process_vm_writev`s from a
//! Linux ELF into a Win32 game.
//!
//! Aurora on native Windows uses these same APIs and is reliable; the
//! hypothesis is that staying inside Wine's NT emulation (instead of
//! crossing the Linux↔Wine boundary) recovers Aurora-grade reliability.
//! This spike validates that hypothesis or kills it before the epic
//! commits to the architecture.
//!
//! ## Usage
//!
//! ```sh
//! cheat-bridge --target-exe EnderMagnoliaSteam-Win64-Shipping.exe \
//!              [--iters 1000] [--bytes 256]
//! ```
//!
//! Launch this under the game's Wine prefix (e.g. with
//! `protontricks-launch --no-bwrap --appid 2725260 cheat-bridge.exe …`,
//! see `docs/spike-win32-bridge.md`).
//!
//! ## Test loop
//!
//! 1. Enumerate processes with `CreateToolhelp32Snapshot` and pick the
//!    one whose executable matches `--target-exe`.
//! 2. `OpenProcess` with `PROCESS_VM_OPERATION | PROCESS_VM_WRITE |
//!    PROCESS_VM_READ`.
//! 3. `VirtualAllocEx` a 4 KB R/W region inside the target (this is
//!    the cheat-runtime "codecave" shape, so the test mirrors what
//!    the real backend would do).
//! 4. For each iteration: generate `--bytes` random bytes, write them,
//!    read them back, compare. Tally matches / mismatches.
//! 5. Free the region. Print a summary line:
//!
//!    ```
//!    bridge: 1000/1000 round-trips matched (0 variance events)
//!    ```
//!
//! Any mismatch — even one — kills the bridge architecture: it means
//! the same variance the Linux backend hits also bites a Win32 caller
//! under Wine, just at a different rate. A clean 1000/1000 (or
//! 10000/10000 if pushed) is the green light to pivot the epic.

#![cfg(target_os = "windows")]

use std::io::Write;
use std::os::raw::c_void;
use std::process::ExitCode;
use std::ptr;
use std::time::Instant;

const LOG_PATH: &str = r"C:\users\Public\cheat-bridge.log";

/// File-based logging — Win32 console stdout doesn't survive
/// Wine + SLR + CreateProcess inheritance reliably, so we write into
/// the prefix where the host wrapper can read it back after the game
/// exits.
fn log(msg: impl std::fmt::Display) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
    {
        let _ = writeln!(f, "{msg}");
    }
}

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

// `ReadProcessMemory` / `WriteProcessMemory` live under
// `Win32_System_Diagnostics_Debug` in windows-sys 0.59 — we declare them
// ourselves to avoid pulling that whole feature for two functions.
unsafe extern "system" {
    fn ReadProcessMemory(
        hProcess: HANDLE,
        lpBaseAddress: *const c_void,
        lpBuffer: *mut c_void,
        nSize: usize,
        lpNumberOfBytesRead: *mut usize,
    ) -> i32;

    fn WriteProcessMemory(
        hProcess: HANDLE,
        lpBaseAddress: *mut c_void,
        lpBuffer: *const c_void,
        nSize: usize,
        lpNumberOfBytesWritten: *mut usize,
    ) -> i32;
}

#[derive(Debug)]
struct Args {
    target_exe: String,
    iters: u32,
    bytes: usize,
}

fn main() -> ExitCode {
    let _ = std::fs::File::create(LOG_PATH);
    log("bridge: start");

    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            log(format_args!("bridge: arg error: {msg}"));
            return ExitCode::from(2);
        }
    };

    let pid = match wait_for_pid(&args.target_exe, std::time::Duration::from_secs(30)) {
        Some(p) => p,
        None => {
            log(format_args!(
                "bridge: no process matched '{}' after 30s",
                args.target_exe
            ));
            return ExitCode::from(3);
        }
    };
    log(format_args!(
        "bridge: target '{}' is pid {pid}",
        args.target_exe
    ));

    let access = PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE;
    let process = unsafe { OpenProcess(access, FALSE, pid) };
    if process.is_null() {
        log(format_args!("bridge: OpenProcess({pid}) failed"));
        return ExitCode::from(4);
    }

    let region_size = 4096usize;
    let region = unsafe {
        VirtualAllocEx(
            process,
            ptr::null(),
            region_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if region.is_null() {
        log("bridge: VirtualAllocEx failed");
        unsafe { CloseHandle(process) };
        return ExitCode::from(5);
    }
    log(format_args!(
        "bridge: allocated {region_size}-byte region at {region:p}"
    ));

    let result = run_loop(process, region, &args);

    unsafe {
        VirtualFreeEx(process, region, 0, MEM_RELEASE);
        CloseHandle(process);
    }

    match result {
        Ok((matched, total, elapsed_us)) => {
            log(format_args!(
                "bridge: {matched}/{total} round-trips matched ({} variance events, {} µs avg)",
                total - matched,
                elapsed_us / total as u128,
            ));
            if matched == total {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(msg) => {
            log(format_args!("bridge: {msg}"));
            ExitCode::from(6)
        }
    }
}

fn run_loop(
    process: HANDLE,
    region: *mut c_void,
    args: &Args,
) -> Result<(u32, u32, u128), String> {
    let mut write_buf = vec![0u8; args.bytes];
    let mut read_buf = vec![0u8; args.bytes];
    // A deterministic LCG so the spike is reproducible across runs
    // without dragging `rand` in. Quality is irrelevant — we just need
    // distinct payloads each iteration.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;

    let started = Instant::now();
    let mut matched = 0u32;
    for _ in 0..args.iters {
        // Refresh payload.
        for b in write_buf.iter_mut() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *b = (seed >> 56) as u8;
        }

        let mut written = 0usize;
        let ok = unsafe {
            WriteProcessMemory(
                process,
                region,
                write_buf.as_ptr() as *const c_void,
                args.bytes,
                &mut written,
            )
        };
        if ok == 0 || written != args.bytes {
            return Err(format!(
                "WriteProcessMemory failed (rc={ok}, written={written}/{} bytes)",
                args.bytes
            ));
        }

        let mut got = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                process,
                region,
                read_buf.as_mut_ptr() as *mut c_void,
                args.bytes,
                &mut got,
            )
        };
        if ok == 0 || got != args.bytes {
            return Err(format!(
                "ReadProcessMemory failed (rc={ok}, read={got}/{} bytes)",
                args.bytes
            ));
        }

        if write_buf == read_buf {
            matched += 1;
        }
    }
    let elapsed_us = started.elapsed().as_micros();
    Ok((matched, args.iters, elapsed_us))
}

fn parse_args() -> Result<Args, String> {
    let mut argv = std::env::args().skip(1);
    let mut target_exe: Option<String> = None;
    let mut iters = 1000u32;
    let mut bytes = 256usize;
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--target-exe" => {
                target_exe = Some(argv.next().ok_or("--target-exe needs a value")?);
            }
            "--iters" => {
                iters = argv
                    .next()
                    .ok_or("--iters needs a value")?
                    .parse()
                    .map_err(|e| format!("--iters: {e}"))?;
            }
            "--bytes" => {
                bytes = argv
                    .next()
                    .ok_or("--bytes needs a value")?
                    .parse()
                    .map_err(|e| format!("--bytes: {e}"))?;
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }
    Ok(Args {
        target_exe: target_exe.ok_or("--target-exe is required")?,
        iters,
        bytes,
    })
}

/// Poll `find_pid` every 500 ms until it hits or the deadline expires.
/// Used by the bootstrap path where the bridge can outrace the game's
/// inner exe by a few seconds.
fn wait_for_pid(name: &str, timeout: std::time::Duration) -> Option<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(pid) = find_pid(name) {
            return Some(pid);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Walk the process snapshot and return the PID whose `szExeFile`
/// matches `name` case-insensitively. Done with `ToolHelp32` rather
/// than `EnumProcesses + GetModuleBaseName` because ToolHelp32 carries
/// the exe name directly on the entry, no second handle needed.
fn find_pid(name: &str) -> Option<u32> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut found: Option<u32> = None;
    if unsafe { Process32FirstW(snap, &mut entry) } != 0 {
        loop {
            let exe = wide_to_string(&entry.szExeFile);
            if exe.eq_ignore_ascii_case(name) {
                found = Some(entry.th32ProcessID);
                break;
            }
            if unsafe { Process32NextW(snap, &mut entry) } == 0 {
                break;
            }
        }
    }
    unsafe { CloseHandle(snap) };
    found
}

fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
