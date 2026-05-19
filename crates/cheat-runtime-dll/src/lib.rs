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
//! ## Module layout
//!
//! - [`debug`] — `CHEAT_RUNTIME_DLL_DEBUG` env-var probe + MessageBox
//!   banner (load-chain confirmation only, no functional impact).
//! - [`proxy`] — `dinput8.dll` proxy: `LoadLibraryA` the real DLL +
//!   six forwarder exports (`DirectInput8Create`, `DllCanUnloadNow`,
//!   `DllGetClassObject`, `DllRegisterServer`, `DllUnregisterServer`,
//!   `GetdfDIJoystick`) trampolining through resolved function pointers.
//!
//! ## Build
//!
//! ```sh
//! ./scripts/build-dll.sh
//! ```
//!
//! Wraps `cargo build -p cheat-runtime-dll --target
//! x86_64-pc-windows-gnu --release` and stages the artefact at
//! `target/dist/cheat_runtime_dll.dll`.

#![cfg(target_os = "windows")]

mod debug;
mod proxy;

use std::os::raw::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::CreateThread;

/// Loader entry point. The Windows loader holds the loader lock for
/// the duration of this call — we MUST NOT do any meaningful work
/// here (no `std::thread::spawn` which secretly touches Rust's
/// runtime init).
///
/// `proxy::init` is the one exception: `LoadLibraryA` against an
/// already-loaded system DLL (the game pulled `dinput8.dll` in
/// before us, modulo cold-boot edge cases handled by Wine's loader)
/// is the well-known REFramework / BepInEx pattern. Doing it
/// synchronously here means the first game-side `DirectInput8Create`
/// call always finds a real function pointer instead of racing the
/// worker thread.
///
/// `CreateThread` is the canonical escape hatch for everything else.
#[unsafe(no_mangle)]
pub extern "system" fn DllMain(_hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        proxy::init();

        // Safety: null security attributes + 0 stack hint + 0 creation
        // flags is the Win32-canonical "spawn a default worker" shape.
        // The returned handle is dropped on purpose; the OS reclaims
        // the thread at process exit.
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

/// Worker entry. Runs after the loader has released its lock. Today it
/// only surfaces the debug banner; Phase 3 will bind the named-pipe
/// server here.
unsafe extern "system" fn worker_main(_lpv: *mut c_void) -> u32 {
    if debug::enabled() {
        debug::show_banner();
    }
    0
}
