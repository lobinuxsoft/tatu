#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's GStreamer pipeline hitting real VA-API hardware video
    // decode crashed the whole app (glibc `free(): corrupted unsorted
    // chunks`) as soon as the art picker (#328) showed an animated
    // SteamGridDB preview — a known VA-API/driver instability class, not
    // specific to this app (confirmed against public GStreamer/WebKitGTK
    // bug reports). Forcing VA-API init to fail with a bogus driver name
    // makes GStreamer fall back to software decode, which doesn't touch
    // the broken hardware path at all. Set before any GTK/WebKit init
    // happens inside `run()`, and only ever from this single, still-
    // single-threaded point — safe despite `set_var` being `unsafe` since
    // edition 2024.
    #[cfg(unix)]
    unsafe {
        std::env::set_var("LIBVA_DRIVER_NAME", "tatu-disable-vaapi");
    }

    tatu_tracker::run();
}
