//! In-process Windows DLL backend for `cheat-runtime`. Compiled as a
//! `cdylib` for `x86_64-pc-windows-gnu` and shipped as
//! `cheat_runtime_dll.dll`. Installed into a game directory as a
//! `dinput8.dll` proxy (or `version.dll` / `winmm.dll` per-game
//! override) so the Win32 loader pulls it in at startup.
//!
//! Once injected, every memory read/write happens **inside** the game
//! process — no `process_vm_writev`, no ptrace, no Wine COW traversal.
//! That's the whole reason this crate exists; see #102 for the cross-
//! process variance the Linux backend hits.
//!
//! ## Phase 1 (this commit)
//!
//! Only [`DllMain`] + a worker thread that pops a debug `MessageBoxA`
//! when `CHEAT_RUNTIME_DLL_DEBUG` is present in the game's environment.
//! Confirms the loader chain (game → dinput8.dll → our cdylib) before
//! any IPC or cheat machinery lands.
//!
//! Build (from the repo root):
//!
//! ```sh
//! ./scripts/build-dll.sh
//! ```
//!
//! The build script wraps `cargo build -p cheat-runtime-dll
//! --target x86_64-pc-windows-gnu --release` and copies the artefact
//! to `target/dist/cheat_runtime_dll.dll`.

#![cfg(target_os = "windows")]

use std::ffi::CString;
use std::os::raw::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, HWND, TRUE};
use windows_sys::Win32::System::Environment::GetEnvironmentVariableA;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::CreateThread;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_OK, MessageBoxA};

/// Null-terminated for direct hand-off to `GetEnvironmentVariableA`.
const DEBUG_ENV_VAR: &[u8] = b"CHEAT_RUNTIME_DLL_DEBUG\0";

/// Loader entry point. The Windows loader holds the loader lock for
/// the duration of this call — we MUST NOT do any meaningful work
/// here (no `LoadLibrary`, no `std::thread::spawn` which secretly
/// touches Rust's runtime init). `CreateThread` is the canonical escape
/// hatch (REFramework, every CE plugin, BepInEx). The worker runs
/// after the loader has released its lock.
#[unsafe(no_mangle)]
pub extern "system" fn DllMain(_hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        // Safety: passing null for security attributes + 0 stack hint +
        // 0 creation flags is the Win32-canonical "spawn a default
        // worker" shape. We drop the returned handle on purpose; the
        // OS reclaims the thread at process exit.
        unsafe {
            let _ = CreateThread(
                ptr::null(),
                0,
                Some(worker_main),
                ptr::null(),
                0,
                ptr::null_mut(),
            );
        }
    }
    TRUE
}

/// Worker entry. Today it only surfaces the debug banner; Phase 3 will
/// bind the named-pipe server here.
unsafe extern "system" fn worker_main(_lpv: *mut c_void) -> u32 {
    if debug_enabled() {
        show_debug_banner();
    }
    0
}

/// Presence-only probe: `GetEnvironmentVariableA` returns 0 on miss
/// and the required buffer length on hit. We don't care about the
/// value — any non-zero return means the variable is set.
fn debug_enabled() -> bool {
    let mut probe = [0u8; 1];
    let n = unsafe {
        GetEnvironmentVariableA(
            DEBUG_ENV_VAR.as_ptr(),
            probe.as_mut_ptr(),
            probe.len() as u32,
        )
    };
    n != 0
}

fn show_debug_banner() {
    let body = CString::new("cheat-runtime-dll injected. Phase 1 — DllMain stub.").unwrap();
    let title = CString::new("cheat-runtime-dll").unwrap();
    // Safety: null HWND owner is documented OK (dialog parented to the
    // desktop). Both CStrings outlive the call.
    unsafe {
        MessageBoxA(
            ptr::null_mut::<c_void>() as HWND,
            body.as_ptr() as *const u8,
            title.as_ptr() as *const u8,
            MB_OK,
        );
    }
}
