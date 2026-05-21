//! tatu-bridge binary entry point. The full Win32 implementation
//! lives in `win.rs`; this file only routes by target so the
//! workspace `cargo build` on a Linux dev host can still produce a
//! binary that yells an error rather than a confusing link failure.
//!
//! See `win.rs` for the two modes (`--launch`, `--connect`) and the
//! Aurora-style co-launch architecture they implement.

#[cfg(target_os = "windows")]
mod alloc;
#[cfg(target_os = "windows")]
mod modules;
#[cfg(target_os = "windows")]
mod patch;
#[cfg(target_os = "windows")]
mod remote_mem;
#[cfg(target_os = "windows")]
mod scan;
#[cfg(target_os = "windows")]
mod win;

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    win::run()
}

#[cfg(not(target_os = "windows"))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "tatu-bridge.exe is a Windows-only binary. Cross-compile with: \
         cargo build -p tatu-bridge --target x86_64-pc-windows-gnu --release"
    );
    std::process::ExitCode::from(1)
}
