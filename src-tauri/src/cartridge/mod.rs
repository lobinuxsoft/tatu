// Portable multi-game cartridge (#192): a removable drive laid out as a
// standard Steam library so Steam's own client handles the download. New
// as of #193 — `disk.rs` is Steam library *size estimation*, unrelated
// despite the similar name.
mod assets;
mod drives;
mod goldberg;
mod install;
mod launcher;
mod marker;
mod prepare;
mod runtime;
mod usage;

// Windows has no verified, non-elevated, silent format API (#194) — see the
// PR discussion. Only the Linux path (udisks2 Block.Format, no sudo) exists
// so far.
#[cfg(unix)]
mod format;

// Same story as `format` above: the udisks2 mount-options fix only applies
// to Linux's own automounter. Windows never routes NTFS through udisks2.
#[cfg(unix)]
mod symlinks;

pub use assets::{
    fetch_cartridge_art, fetch_cartridge_description, fetch_cartridge_screenshots,
    fetch_cartridge_trailer, fetch_gog_cartridge_art, fetch_gog_cartridge_description,
    fetch_gog_cartridge_screenshots, fetch_gog_cartridge_trailer,
};
pub use drives::{RemovableDrive, list_removable_drives};
#[cfg(unix)]
pub use format::{format_as_cartridge, mount_cartridge};
pub use goldberg::inject_goldberg;
pub use install::{
    find_pending_cartridge, install_url, is_registered_library, poll_install_status,
    sync_marker_with_installed_apps, uninstall_from_cartridge,
};
pub use launcher::install_launcher_binaries;
pub use marker::{AppSource, CartridgeApp, add_app, has_cartridge_structure, list_apps};
pub use prepare::{PrepareDrmResult, refresh_drm_and_inject};
pub use runtime::bundle_linux_runtime;
#[cfg(unix)]
pub use symlinks::{SymlinksOutcome, ensure_symlinks};
pub use usage::{CartridgeUsage, usage};

// CartridgeMarker/read_marker/MARKER_FILENAME stay private — nothing outside
// this module needs the raw marker, only the app list (`list_apps`).
