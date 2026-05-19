//! `dinput8.dll` proxy mechanics.
//!
//! The game's Win32 loader resolves `dinput8.dll` against the game's
//! own directory FIRST, then `%SYSTEMROOT%\System32\` (default search
//! order). Dropping `cheat_runtime_dll.dll` into the game folder
//! renamed as `dinput8.dll` puts us first, so the game loads US for
//! every `dinput8.dll` import.
//!
//! We forward every export the real `dinput8.dll` ships with — the
//! game must observe identical behaviour for `DirectInput8Create` &
//! friends or controllers / DirectInput-based startup checks break.
//! Failing the smoke ("game boots and plays identically") means a
//! forwarder is misbehaving and Phase 3+ work piles on broken ground.
//!
//! ## How forwarding works
//!
//! 1. [`init`] runs from `DllMain` on `DLL_PROCESS_ATTACH`. It
//!    `LoadLibraryA`s the real `C:\Windows\System32\dinput8.dll` (full
//!    absolute path avoids loading our own proxy recursively) and
//!    resolves the six standard exports via `GetProcAddress`.
//! 2. The resolved pointers go into a [`OnceLock<Option<ProxyFns>>`]
//!    so the forwarder functions can read them lock-free on every call.
//! 3. Each `#[unsafe(no_mangle)] pub unsafe extern "system" fn` below
//!    is one of the exports the loader will look up against us; the
//!    body is a thin trampoline that calls through to the real fn
//!    pointer, returning `E_FAIL` only if `LoadLibrary` itself failed
//!    (in which case `init` set the cell to `None`).
//!
//! ## Why a hard-coded absolute path
//!
//! `LoadLibraryA("dinput8.dll")` would re-enter our own search order
//! and resolve to us — infinite recursion. The system32 absolute path
//! is the canonical Win32 idiom (REFramework, BepInEx, every CE
//! plugin). On Wine/Proton the same path resolves to the Wine builtin
//! `dinput8.dll` under the prefix; tested behaviour is identical.

use std::os::raw::c_void;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HINSTANCE, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

const REAL_DLL_PATH: &[u8] = b"C:\\Windows\\System32\\dinput8.dll\0";

/// HRESULT we return when proxy init failed and a forwarder is still
/// being called (e.g. system32 doesn't carry `dinput8.dll`, which
/// should never happen on a sane Wine prefix but we degrade safely).
const E_FAIL: i32 = 0x80004005u32 as i32;

// Function-pointer types for each export. Argument layouts come straight
// from `dinput.h` / the COM ABI; we use `*const c_void` for GUID / IID /
// CLSID pointers because the forwarder doesn't need to interpret them —
// it just passes them through to the real implementation.

type DirectInput8CreateFn = unsafe extern "system" fn(
    hinst: HINSTANCE,
    dw_version: u32,
    riidltf: *const c_void,
    ppv_out: *mut *mut c_void,
    punk_outer: *mut c_void,
) -> i32;

type NoArgHResultFn = unsafe extern "system" fn() -> i32;

type DllGetClassObjectFn = unsafe extern "system" fn(
    rclsid: *const c_void,
    riid: *const c_void,
    ppv: *mut *mut c_void,
) -> i32;

type GetdfDIJoystickFn = unsafe extern "system" fn() -> *const c_void;

struct ProxyFns {
    direct_input8_create: DirectInput8CreateFn,
    dll_can_unload_now: NoArgHResultFn,
    dll_get_class_object: DllGetClassObjectFn,
    dll_register_server: NoArgHResultFn,
    dll_unregister_server: NoArgHResultFn,
    getdf_di_joystick: GetdfDIJoystickFn,
}

static PROXY_FNS: OnceLock<Option<ProxyFns>> = OnceLock::new();

/// Resolve the real `dinput8.dll` once. Called from `DllMain`. Safe to
/// call repeatedly — `OnceLock` swallows the duplicate. If the load
/// fails for any reason (system32 missing the DLL, GetProcAddress
/// returning null), the cell is initialised to `None` and every
/// forwarder degrades to returning `E_FAIL` instead of trampolining
/// through a null pointer.
pub(crate) fn init() {
    let _ = PROXY_FNS.get_or_init(try_load);
}

fn try_load() -> Option<ProxyFns> {
    // Safety: LoadLibraryA + GetProcAddress are documented thread-safe
    // and the only state we mutate is the OnceLock below. The string
    // is statically null-terminated.
    unsafe {
        let hmod: HMODULE = LoadLibraryA(REAL_DLL_PATH.as_ptr());
        if hmod.is_null() {
            return None;
        }
        Some(ProxyFns {
            direct_input8_create: resolve(hmod, b"DirectInput8Create\0")?,
            dll_can_unload_now: resolve(hmod, b"DllCanUnloadNow\0")?,
            dll_get_class_object: resolve(hmod, b"DllGetClassObject\0")?,
            dll_register_server: resolve(hmod, b"DllRegisterServer\0")?,
            dll_unregister_server: resolve(hmod, b"DllUnregisterServer\0")?,
            getdf_di_joystick: resolve(hmod, b"GetdfDIJoystick\0")?,
        })
    }
}

/// Type-erased `GetProcAddress` + transmute. `GetProcAddress` returns
/// `Option<unsafe extern "system" fn() -> isize>` in windows-sys; the
/// `?` short-circuits on missing exports, and `transmute_copy` widens
/// the empty-signature function pointer into whichever real fn type
/// the caller annotated.
unsafe fn resolve<F: Copy>(hmod: HMODULE, name: &[u8]) -> Option<F> {
    let raw = unsafe { GetProcAddress(hmod, name.as_ptr()) }?;
    // Safety: every export we ask for is a known C-ABI function in
    // dinput8.dll; the caller's type annotation matches the real
    // signature documented in dinput.h.
    Some(unsafe { std::mem::transmute_copy(&raw) })
}

/// Helper used by every forwarder: read the proxy cell, unwrap the
/// optional, and return `E_FAIL` if init never populated it. Inlined
/// so the trampoline stays a few instructions.
#[inline(always)]
fn fns() -> Option<&'static ProxyFns> {
    PROXY_FNS.get().and_then(|cell| cell.as_ref())
}

// ---- Forwarder exports ----------------------------------------------------
//
// Each `#[unsafe(no_mangle)] pub unsafe extern "system" fn` lands in the
// produced DLL's export table under the un-decorated symbol name, which
// is what the game's import table is looking for. Phase 2 acceptance:
// `objdump -p target/dist/cheat_runtime_dll.dll | grep -i ordinal`
// must list all six names.

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DirectInput8Create(
    hinst: HINSTANCE,
    dw_version: u32,
    riidltf: *const c_void,
    ppv_out: *mut *mut c_void,
    punk_outer: *mut c_void,
) -> i32 {
    match fns() {
        Some(p) => unsafe {
            (p.direct_input8_create)(hinst, dw_version, riidltf, ppv_out, punk_outer)
        },
        None => E_FAIL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllCanUnloadNow() -> i32 {
    match fns() {
        Some(p) => unsafe { (p.dll_can_unload_now)() },
        None => E_FAIL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const c_void,
    riid: *const c_void,
    ppv: *mut *mut c_void,
) -> i32 {
    match fns() {
        Some(p) => unsafe { (p.dll_get_class_object)(rclsid, riid, ppv) },
        None => E_FAIL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllRegisterServer() -> i32 {
    match fns() {
        Some(p) => unsafe { (p.dll_register_server)() },
        None => E_FAIL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllUnregisterServer() -> i32 {
    match fns() {
        Some(p) => unsafe { (p.dll_unregister_server)() },
        None => E_FAIL,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetdfDIJoystick() -> *const c_void {
    match fns() {
        Some(p) => unsafe { (p.getdf_di_joystick)() },
        None => std::ptr::null(),
    }
}
