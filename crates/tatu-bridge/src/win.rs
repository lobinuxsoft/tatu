//! Win32-only implementation of tatu-bridge — gated in via `main.rs`'s
//! `#[cfg(target_os = "windows")] mod win;` so the workspace `cargo
//! build` from a Linux host doesn't try to resolve `windows-sys`
//! symbols.
//!
//! Two modes (dispatched on `argv[1]`):
//!
//! - `--launch <game.exe> --target-exe <name> [--iters N] [--bytes N]`
//!
//!   Bootstrap mode. Win32 entry point of the Steam launch wrapper.
//!   `CreateProcess`-spawns `self --connect …` AND the real game.exe
//!   as siblings of one Proton invocation. Both children share the
//!   SLR container, wineserver, and `/tmp` — what makes the bridge
//!   able to `OpenProcess` the game in the first place.
//!
//! - `--connect --target-exe <name> [--iters N] [--bytes N]`
//!
//!   Bridge worker. Polls `ToolHelp32` for `<name>` (race-tolerant,
//!   30 s deadline), opens the process, allocates a 4 KB R/W region
//!   inside it. Then:
//!
//!   - With `--iters 0` (the Phase 3 default): bind an AF_UNIX
//!     listener at `C:\users\Public\tatu-bridge.sock` (Wine forwards
//!     AF_UNIX to the Linux kernel; the Linux tracker dials the same
//!     socket via the wineprefix path) and serve the `CHRT v1`
//!     protocol from `tatu_proto` until the peer sends `Shutdown` or
//!     the bridge is terminated by the `--launch` parent.
//!   - With `--iters N > 0`: legacy smoke benchmark — write/read a
//!     `--bytes` payload against the allocated region for N
//!     iterations and report round-trip count. Kept for regression
//!     checks of the Win32 memory primitives.
//!
//! Both modes log to files inside the prefix
//! (`C:\users\Public\tatu-bridge.log` / `…-launch.log`) because Win32
//! stdout doesn't survive Wine + SLR + `CreateProcess` inheritance
//! reliably enough to use stdout-based logging.

use std::ffi::OsString;
use std::io::Write;
use std::os::raw::c_void;
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, INFINITE, OpenProcess, PROCESS_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};

// ReadProcessMemory / WriteProcessMemory aren't exposed by the
// Win32_System_Diagnostics_Debug slice we enable; declaring them
// directly keeps the windows-sys feature set tight.
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

const CONNECT_LOG: &str = r"C:\users\Public\tatu-bridge.log";
const LAUNCH_LOG: &str = r"C:\users\Public\tatu-bridge-launch.log";

fn log_to(path: &str, msg: impl std::fmt::Display) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{msg}");
    }
}

pub fn run() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(1).map(String::as_str) {
        Some("--launch") => launch_mode(&argv[2..]),
        Some("--connect") => connect_mode(&argv[2..]),
        _ => {
            eprintln!(
                "usage:\n  tatu-bridge.exe --launch <game.exe> --target-exe <name> [--iters N] [--bytes N]\n  tatu-bridge.exe --connect --target-exe <name> [--iters N] [--bytes N]"
            );
            ExitCode::from(2)
        }
    }
}

#[derive(Debug)]
struct ConnectArgs {
    target_exe: String,
    iters: u32,
    bytes: usize,
}

fn parse_connect_args(argv: &[String]) -> Result<ConnectArgs, String> {
    let mut target_exe: Option<String> = None;
    let mut iters = 0u32;
    let mut bytes = 256usize;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--target-exe" => {
                target_exe = Some(
                    argv.get(i + 1)
                        .cloned()
                        .ok_or("--target-exe needs a value")?,
                );
                i += 2;
            }
            "--iters" => {
                iters = argv
                    .get(i + 1)
                    .ok_or("--iters needs a value")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| format!("--iters: {e}"))?;
                i += 2;
            }
            "--bytes" => {
                bytes = argv
                    .get(i + 1)
                    .ok_or("--bytes needs a value")?
                    .parse()
                    .map_err(|e: std::num::ParseIntError| format!("--bytes: {e}"))?;
                i += 2;
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }
    Ok(ConnectArgs {
        target_exe: target_exe.ok_or("--target-exe is required")?,
        iters,
        bytes,
    })
}

// ---- launch (bootstrap) mode -------------------------------------------

fn launch_mode(argv: &[String]) -> ExitCode {
    let _ = std::fs::File::create(LAUNCH_LOG);

    let game_exe = match argv.first() {
        Some(p) => p.clone(),
        None => {
            log_to(LAUNCH_LOG, "launch: first arg must be <game.exe>");
            return ExitCode::from(2);
        }
    };
    let rest = &argv[1..];
    let connect_args = match parse_connect_args(rest) {
        Ok(c) => c,
        Err(e) => {
            log_to(LAUNCH_LOG, format_args!("launch: {e}"));
            return ExitCode::from(2);
        }
    };

    let self_exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            log_to(LAUNCH_LOG, format_args!("launch: current_exe failed: {e}"));
            return ExitCode::from(3);
        }
    };
    log_to(
        LAUNCH_LOG,
        format_args!(
            "launch: self={self_exe} game={game_exe} target={} iters={} bytes={}",
            connect_args.target_exe, connect_args.iters, connect_args.bytes,
        ),
    );

    let bridge_cmdline = format!(
        r#""{self_exe}" --connect --target-exe "{}" --iters {} --bytes {}"#,
        connect_args.target_exe, connect_args.iters, connect_args.bytes,
    );
    let bridge = match create_process(&bridge_cmdline) {
        Ok(info) => info,
        Err(e) => {
            log_to(
                LAUNCH_LOG,
                format_args!("launch: bridge CreateProcess failed: {e}"),
            );
            return ExitCode::from(3);
        }
    };
    log_to(
        LAUNCH_LOG,
        format_args!("launch: bridge pid {}", bridge.dwProcessId),
    );

    let game_cmdline = format!(r#""{game_exe}""#);
    let game = match create_process(&game_cmdline) {
        Ok(info) => info,
        Err(e) => {
            log_to(
                LAUNCH_LOG,
                format_args!("launch: game CreateProcess failed: {e}"),
            );
            unsafe { TerminateProcess(bridge.hProcess, 1) };
            unsafe {
                CloseHandle(bridge.hThread);
                CloseHandle(bridge.hProcess);
            }
            return ExitCode::from(4);
        }
    };
    log_to(
        LAUNCH_LOG,
        format_args!("launch: game pid {}", game.dwProcessId),
    );

    let rc = unsafe { WaitForSingleObject(game.hProcess, INFINITE) };
    log_to(
        LAUNCH_LOG,
        format_args!("launch: game exited (wait rc = {rc:#x})"),
    );

    unsafe {
        TerminateProcess(bridge.hProcess, 0);
        CloseHandle(bridge.hThread);
        CloseHandle(bridge.hProcess);
        CloseHandle(game.hThread);
        CloseHandle(game.hProcess);
    }
    ExitCode::SUCCESS
}

// ---- connect (bridge worker) mode --------------------------------------

fn connect_mode(argv: &[String]) -> ExitCode {
    let _ = std::fs::File::create(CONNECT_LOG);
    log_to(CONNECT_LOG, "connect: start");

    let args = match parse_connect_args(argv) {
        Ok(c) => c,
        Err(e) => {
            log_to(CONNECT_LOG, format_args!("connect: arg error: {e}"));
            return ExitCode::from(2);
        }
    };

    let pid = match wait_for_pid(&args.target_exe, Duration::from_secs(30)) {
        Some(p) => p,
        None => {
            log_to(
                CONNECT_LOG,
                format_args!(
                    "connect: no process matched '{}' after 30s",
                    args.target_exe
                ),
            );
            return ExitCode::from(3);
        }
    };
    log_to(
        CONNECT_LOG,
        format_args!("connect: target '{}' is pid {pid}", args.target_exe),
    );

    let access = PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE;
    let process = unsafe { OpenProcess(access, FALSE, pid) };
    if process.is_null() {
        log_to(
            CONNECT_LOG,
            format_args!("connect: OpenProcess({pid}) failed"),
        );
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
        log_to(CONNECT_LOG, "connect: VirtualAllocEx failed");
        unsafe { CloseHandle(process) };
        return ExitCode::from(5);
    }
    log_to(
        CONNECT_LOG,
        format_args!("connect: allocated {region_size}-byte region at {region:p}"),
    );

    let exit = if args.iters > 0 {
        smoke_mode(process, region, &args)
    } else {
        serve_mode()
    };

    unsafe {
        VirtualFreeEx(process, region, 0, MEM_RELEASE);
        CloseHandle(process);
    }
    exit
}

fn smoke_mode(process: HANDLE, region: *mut c_void, args: &ConnectArgs) -> ExitCode {
    match run_loop(process, region, args) {
        Ok((matched, total, elapsed_us)) => {
            log_to(
                CONNECT_LOG,
                format_args!(
                    "connect: smoke {matched}/{total} round-trips matched ({} variance events, {} µs avg)",
                    total - matched,
                    elapsed_us / total as u128,
                ),
            );
            if matched == total {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(msg) => {
            log_to(CONNECT_LOG, format_args!("connect: smoke {msg}"));
            ExitCode::from(6)
        }
    }
}

/// AF_UNIX server loop. Single-threaded, one client at a time. The
/// tracker is the only legitimate peer; concurrency would only mask
/// races. Wine forwards AF_UNIX bind/connect to the Linux kernel, so
/// the socket file lives at
/// `<wineprefix>/drive_c/users/Public/tatu-bridge.sock` as seen from
/// Linux.
///
/// Lives until the peer sends `Request::Shutdown` or the `--launch`
/// parent terminates us when the game exits.
fn serve_mode() -> ExitCode {
    use tatu_proto::BRIDGE_SOCKET_WIN_PATH;
    use uds_windows::UnixListener;

    // Clean stale socket from a prior crashed run. UnixListener::bind
    // refuses to bind on top of an existing path.
    let _ = std::fs::remove_file(BRIDGE_SOCKET_WIN_PATH);

    let listener = match UnixListener::bind(BRIDGE_SOCKET_WIN_PATH) {
        Ok(l) => l,
        Err(e) => {
            log_to(
                CONNECT_LOG,
                format_args!("connect: bind {BRIDGE_SOCKET_WIN_PATH}: {e}"),
            );
            return ExitCode::from(7);
        }
    };
    log_to(
        CONNECT_LOG,
        format_args!("connect: listening on {BRIDGE_SOCKET_WIN_PATH}"),
    );

    for stream in listener.incoming() {
        match stream {
            Ok(mut s) => {
                log_to(CONNECT_LOG, "connect: client connected");
                match serve_client(&mut s) {
                    Ok(ShouldStop::Yes) => {
                        log_to(CONNECT_LOG, "connect: shutdown received, exiting");
                        let _ = std::fs::remove_file(BRIDGE_SOCKET_WIN_PATH);
                        return ExitCode::SUCCESS;
                    }
                    Ok(ShouldStop::No) => {
                        log_to(CONNECT_LOG, "connect: client disconnected");
                    }
                    Err(e) => {
                        log_to(CONNECT_LOG, format_args!("connect: client error: {e}"));
                    }
                }
            }
            Err(e) => {
                log_to(CONNECT_LOG, format_args!("connect: accept: {e}"));
                return ExitCode::from(8);
            }
        }
    }
    ExitCode::SUCCESS
}

#[derive(Debug)]
enum ShouldStop {
    Yes,
    No,
}

fn serve_client(stream: &mut uds_windows::UnixStream) -> Result<ShouldStop, String> {
    use tatu_proto::{Request, Response, read_frame, read_handshake, write_frame, write_handshake};

    write_handshake(stream).map_err(|e| format!("write_handshake: {e}"))?;
    read_handshake(stream).map_err(|e| format!("read_handshake: {e}"))?;

    loop {
        let req: Request = match read_frame(stream) {
            Ok(r) => r,
            Err(tatu_proto::ProtocolError::Io(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(ShouldStop::No);
            }
            Err(e) => return Err(format!("read_frame: {e}")),
        };
        let resp = dispatch(req);
        let stop = matches!(resp, Response::ShutdownAck);
        write_frame(stream, &resp).map_err(|e| format!("write_frame: {e}"))?;
        if stop {
            return Ok(ShouldStop::Yes);
        }
    }
}

fn dispatch(req: tatu_proto::Request) -> tatu_proto::Response {
    use tatu_proto::{Request, Response};
    match req {
        Request::Ping => Response::Pong,
        Request::Shutdown => Response::ShutdownAck,
        other => Response::Err {
            message: format!("{other:?} not implemented in Phase 3 of #106"),
        },
    }
}

fn run_loop(
    process: HANDLE,
    region: *mut c_void,
    args: &ConnectArgs,
) -> Result<(u32, u32, u128), String> {
    let mut write_buf = vec![0u8; args.bytes];
    let mut read_buf = vec![0u8; args.bytes];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;

    let started = Instant::now();
    let mut matched = 0u32;
    for _ in 0..args.iters {
        for b in write_buf.iter_mut() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
    Ok((matched, args.iters, started.elapsed().as_micros()))
}

// ---- shared helpers -----------------------------------------------------

fn wait_for_pid(name: &str, timeout: Duration) -> Option<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(pid) = find_pid(name) {
            return Some(pid);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

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

fn create_process(cmdline: &str) -> Result<PROCESS_INFORMATION, String> {
    let mut wide: Vec<u16> = OsString::from(cmdline)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
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
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        Err("CreateProcessW returned 0".to_string())
    } else {
        Ok(pi)
    }
}
