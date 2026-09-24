//! `cheat-mono-collector` — the Windows-side half of tatu's Mono symbol bridge.
//!
//! Built as a Windows DLL and loaded into a Unity **Mono** game running under
//! Proton/Wine (via a `winhttp.dll` proxy / Doorstop-style vector, set up by
//! tatu's installer — analogous to the existing REFramework auto-install).
//! Once loaded it waits for the game's Mono runtime to come up, attaches, and
//! serves `Class:Method -> JIT address` resolution to native-Linux tatu over a
//! TCP loopback socket using a Cheat-Engine-compatible protocol.
//!
//! Only [`protocol`] is platform-independent (and unit-tested on the host); the
//! Mono FFI, resolver and server are Windows-only.

pub mod protocol;

#[cfg(windows)]
pub mod mono;
#[cfg(windows)]
pub mod proxy;
#[cfg(windows)]
pub mod resolver;
#[cfg(windows)]
pub mod server;

#[cfg(windows)]
mod entry {
    use std::ffi::c_void;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
    use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

    use crate::mono::MonoApi;
    use crate::server::serve;

    /// How often the bootstrap thread polls for Mono before it is loaded. The
    /// proxy DLL is mapped early — before Unity initialises its runtime — so we
    /// expect to spin a handful of times at startup, then never again.
    const MONO_POLL_INTERVAL: Duration = Duration::from_millis(250);

    /// Run in a dedicated thread (never in `DllMain` — that holds the loader
    /// lock): wait for Mono, attach this thread to it, then serve forever.
    fn bootstrap() {
        let api = loop {
            // SAFETY: we are in the target process; `load` returns `None` until
            // Mono is mapped, so the loop simply waits it out.
            if let Some(api) = unsafe { MonoApi::load() } {
                break api;
            }
            std::thread::sleep(MONO_POLL_INTERVAL);
        };

        // SAFETY: Mono is loaded; attach this serving thread before any call.
        unsafe { api.attach_current_thread() };
        // SAFETY: Mono is live and this thread is attached.
        unsafe { serve(&api) };
    }

    /// DLL entry point. On attach, kick off the bootstrap thread and return
    /// immediately so the host (the game) loads normally.
    ///
    /// # Safety
    /// Called by the Windows loader with the documented ABI.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn DllMain(
        _instance: HINSTANCE,
        reason: u32,
        _reserved: *mut c_void,
    ) -> BOOL {
        if reason == DLL_PROCESS_ATTACH {
            // Bind the winhttp forwarders synchronously: the game may call
            // winhttp before our bootstrap thread runs, so the real-function
            // pointers must be live the instant DllMain returns.
            // SAFETY: runs during DLL load, before any proxied export is called.
            unsafe { crate::proxy::init_forwarding() };
            // Mono work goes on its own thread — never in DllMain (loader lock).
            std::thread::spawn(bootstrap);
        }
        TRUE
    }
}
