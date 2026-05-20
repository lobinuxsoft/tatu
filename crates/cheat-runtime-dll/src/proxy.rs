//! `winmm.dll` proxy mechanics.
//!
//! Picked over `dinput8.dll` after a Phase 2 smoke against Ender
//! Magnolia revealed the game's import table does **not** reference
//! `dinput8.dll` (UE5 + xinput-only controller path). For a proxy to
//! hijack the loader, the game must actually import the DLL we're
//! impersonating. EM's import scan showed:
//!
//! - `WINMM.dll` → `timeBeginPeriod`, `timeEndPeriod`, `timeGetTime`
//! - `VERSION.dll` → version-info helpers
//! - `dxgi.dll`   → `CreateDXGIFactory`, `CreateDXGIFactory1`
//!
//! `winmm` won the tiebreak: BepInEx uses it as its primary proxy for
//! Unreal Engine games, the three exported functions are pure timer
//! API with no render-pipeline interaction (so vkd3d-proton / DXVK
//! see no interference under Proton), and the implementation surface
//! is minimal — three trampolines.
//!
//! ## How forwarding works
//!
//! 1. [`init`] runs from `DllMain` on `DLL_PROCESS_ATTACH`. It
//!    `LoadLibraryA`s the real `C:\Windows\System32\winmm.dll` (full
//!    absolute path avoids loading our own proxy recursively) and
//!    resolves the three game-imported exports via `GetProcAddress`.
//! 2. Resolved pointers go into a [`OnceLock<Option<ProxyFns>>`] so
//!    the forwarder functions can read them lock-free on every call.
//! 3. Each `#[unsafe(no_mangle)] pub unsafe extern "system" fn` below
//!    is one of the exports the loader will look up against us; the
//!    body is a thin trampoline that calls through to the real fn
//!    pointer, returning `0` only if `LoadLibrary` itself failed.
//!
//! ## Why a hard-coded absolute path
//!
//! `LoadLibraryA("winmm.dll")` would re-enter our own search order
//! and resolve to us — infinite recursion. The system32 absolute path
//! is the canonical Win32 idiom (REFramework, BepInEx, every CE
//! plugin). On Wine/Proton the same path resolves to the Wine builtin
//! `winmm.dll` under the prefix; tested behaviour is identical.

use std::os::raw::c_void;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

const REAL_DLL_PATH: &[u8] = b"C:\\Windows\\System32\\winmm.dll\0";

/// `MMRESULT` we return when proxy init failed and a forwarder is
/// still being called — `TIMERR_NOCANDO` (97 in mmsystem.h). Picks a
/// real winmm error code rather than 0 so the game's error path can
/// recognise it as a timer-API failure instead of "everything's fine".
const TIMERR_NOCANDO: u32 = 97;

// Function-pointer types for each export. Argument layouts come
// straight from `mmsystem.h`. `MMRESULT` is `u32` (DWORD); `UINT` is
// `u32` on Windows.

type TimeBeginPeriodFn = unsafe extern "system" fn(u_period: u32) -> u32;
type TimeEndPeriodFn = unsafe extern "system" fn(u_period: u32) -> u32;
type TimeGetTimeFn = unsafe extern "system" fn() -> u32;

struct ProxyFns {
    time_begin_period: TimeBeginPeriodFn,
    time_end_period: TimeEndPeriodFn,
    time_get_time: TimeGetTimeFn,
}

static PROXY_FNS: OnceLock<Option<ProxyFns>> = OnceLock::new();

/// Resolve the real `winmm.dll` once. Called from `DllMain`. Safe to
/// call repeatedly — `OnceLock` swallows the duplicate. If the load
/// fails for any reason (system32 missing the DLL, GetProcAddress
/// returning null), the cell is initialised to `None` and every
/// forwarder degrades to returning `TIMERR_NOCANDO` instead of
/// dereferencing a null pointer.
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
            time_begin_period: resolve(hmod, b"timeBeginPeriod\0")?,
            time_end_period: resolve(hmod, b"timeEndPeriod\0")?,
            time_get_time: resolve(hmod, b"timeGetTime\0")?,
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
    // winmm.dll; the caller's type annotation matches the real
    // signature documented in mmsystem.h.
    Some(unsafe { std::mem::transmute_copy(&raw) })
}

/// Helper used by every forwarder: read the proxy cell, unwrap the
/// optional, and return `None` if init never populated it. Inlined
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
// `objdump -p target/dist/cheat_runtime_dll.dll` must list all three
// names in the Export Table.

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeBeginPeriod(u_period: u32) -> u32 {
    match fns() {
        Some(p) => unsafe { (p.time_begin_period)(u_period) },
        None => TIMERR_NOCANDO,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeEndPeriod(u_period: u32) -> u32 {
    match fns() {
        Some(p) => unsafe { (p.time_end_period)(u_period) },
        None => TIMERR_NOCANDO,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn timeGetTime() -> u32 {
    match fns() {
        Some(p) => unsafe { (p.time_get_time)() },
        None => 0,
    }
}
