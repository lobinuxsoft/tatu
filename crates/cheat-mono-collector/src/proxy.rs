//! `winhttp.dll` proxy: re-export every winhttp function and forward each call
//! to the real implementation, so the collector can masquerade as `winhttp.dll`
//! next to a Unity game's exe (the BepInEx/Doorstop load vector) without
//! breaking the game's networking.
//!
//! Under Wine/Proton, static `.def` forwarders are a dead end: the prefix's
//! `winhttp.dll` is a *code-less fake PE* that redirects to an ELF `.so`, so
//! there is no real PE to rename and forward to. The robust approach — the one
//! the entire Unity-modding-under-Proton ecosystem runs on — is **runtime
//! trampolines**: at load, `LoadLibraryW` the real winhttp by absolute system
//! path (avoids re-entering our own proxy), `GetProcAddress` every export, and
//! have each exported stub `jmp` to the resolved pointer.
//!
//! Requires the user to set `WINEDLLOVERRIDES="winhttp=n,b"` so Wine loads our
//! native DLL instead of its builtin — the installer surfaces this.

use core::arch::naked_asm;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// Absolute path to the real winhttp. Loading by full system path (rather than
/// the bare name) stops Wine's override from resolving back to our own proxy in
/// the game directory.
const SYSTEM_WINHTTP: &str = "C:\\windows\\system32\\winhttp.dll";

/// Define, for each winhttp export: a resolved-pointer slot, a `#[no_mangle]`
/// naked stub that tail-jumps to it (preserving all args/registers untouched),
/// and a line in `resolve_all` that fills the slot via `GetProcAddress`.
///
/// `#[no_mangle]` makes the cdylib export the name verbatim, so no `.def` is
/// needed. The stub is naked so it adds zero prologue and forwards the call
/// transparently regardless of the real function's signature.
macro_rules! winhttp_proxy {
    ($($name:ident),+ $(,)?) => {
        mod slots {
            //! One resolved real-function pointer per export. Written once at
            //! load before the game makes any winhttp call. Names mirror the
            //! winhttp exports verbatim, so the casing lint doesn't apply.
            #![allow(non_upper_case_globals)]
            $( pub static mut $name: usize = 0; )+
        }

        $(
            // The export name must match winhttp byte-for-byte, so it can't be
            // snake_case.
            #[allow(non_snake_case)]
            #[unsafe(naked)]
            #[unsafe(no_mangle)]
            pub extern "system" fn $name() {
                naked_asm!("jmp qword ptr [rip + {ptr}]", ptr = sym slots::$name);
            }
        )+

        /// Resolve every export from the real winhttp module into its slot.
        fn resolve_all(real: HMODULE) {
            $(
                let proc = unsafe { GetProcAddress(real, concat!(stringify!($name), "\0").as_ptr()) };
                unsafe { slots::$name = proc.map_or(0, |f| f as usize); }
            )+
        }
    };
}

// The full winhttp.dll export surface (Wine `winhttp.spec`, 49 functions). All
// forwarded by name — Unity/.NET and Wine both bind winhttp by name, so no
// ordinals are needed.
winhttp_proxy!(
    DllCanUnloadNow,
    DllGetClassObject,
    DllRegisterServer,
    DllUnregisterServer,
    WinHttpAddRequestHeaders,
    WinHttpCheckPlatform,
    WinHttpCloseHandle,
    WinHttpConnect,
    WinHttpCrackUrl,
    WinHttpCreateProxyResolver,
    WinHttpCreateUrl,
    WinHttpDetectAutoProxyConfigUrl,
    WinHttpFreeProxyResult,
    WinHttpFreeProxyResultEx,
    WinHttpFreeProxySettings,
    WinHttpGetDefaultProxyConfiguration,
    WinHttpGetIEProxyConfigForCurrentUser,
    WinHttpGetProxyForUrl,
    WinHttpGetProxyForUrlEx,
    WinHttpGetProxyForUrlEx2,
    WinHttpGetProxyResult,
    WinHttpGetProxyResultEx,
    WinHttpGetProxySettingsVersion,
    WinHttpOpen,
    WinHttpOpenRequest,
    WinHttpQueryAuthSchemes,
    WinHttpQueryDataAvailable,
    WinHttpQueryHeaders,
    WinHttpQueryOption,
    WinHttpReadData,
    WinHttpReadProxySettings,
    WinHttpReceiveResponse,
    WinHttpResetAutoProxy,
    WinHttpSendRequest,
    WinHttpSetCredentials,
    WinHttpSetDefaultProxyConfiguration,
    WinHttpSetOption,
    WinHttpSetStatusCallback,
    WinHttpSetTimeouts,
    WinHttpTimeFromSystemTime,
    WinHttpTimeToSystemTime,
    WinHttpWebSocketClose,
    WinHttpWebSocketCompleteUpgrade,
    WinHttpWebSocketQueryCloseStatus,
    WinHttpWebSocketReceive,
    WinHttpWebSocketSend,
    WinHttpWebSocketShutdown,
    WinHttpWriteData,
    WinHttpWriteProxySettings,
);

/// Load the real winhttp and bind every forwarder. Call once, synchronously,
/// from `DllMain` on attach — before the game makes any winhttp call. Returns
/// `false` if the real winhttp couldn't be loaded (forwarders stay null; the
/// game's networking would break, but the collector still proceeds so symbol
/// resolution works even on a stripped prefix).
///
/// # Safety
/// Must run during DLL load, before any proxied export is invoked.
pub unsafe fn init_forwarding() -> bool {
    let path: Vec<u16> = SYSTEM_WINHTTP.encode_utf16().chain(Some(0)).collect();
    let real = unsafe { LoadLibraryW(path.as_ptr()) };
    if real.is_null() {
        return false;
    }
    resolve_all(real);
    true
}
