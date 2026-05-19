//! `CHEAT_RUNTIME_DLL_DEBUG` env-var probe + `MessageBoxA` banner. Lets
//! us verify the loader chain (game → dinput8.dll proxy → cheat-runtime-
//! dll) without any IPC or cheat surface getting in the way.

use std::ffi::CString;
use std::os::raw::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Environment::GetEnvironmentVariableA;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_OK, MessageBoxA};

/// Null-terminated for direct hand-off to `GetEnvironmentVariableA`.
const DEBUG_ENV_VAR: &[u8] = b"CHEAT_RUNTIME_DLL_DEBUG\0";

/// Presence-only probe: `GetEnvironmentVariableA` returns 0 on miss
/// and the required buffer length on hit. We don't care about the
/// value — any non-zero return means the variable is set.
pub(crate) fn enabled() -> bool {
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

pub(crate) fn show_banner() {
    let body = CString::new("cheat-runtime-dll injected. Phase 2 — dinput8 proxy forwarders live.")
        .unwrap();
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
