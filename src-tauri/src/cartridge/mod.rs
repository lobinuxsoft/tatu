// Portable multi-game cartridge (#192): a removable drive laid out as a
// standard Steam library so Steam's own client handles the download. New
// as of #193 — `disk.rs` is Steam library *size estimation*, unrelated
// despite the similar name.
mod drives;
mod marker;

// Windows has no verified, non-elevated, silent format API (#194) — see the
// PR discussion. Only the Linux path (udisks2 Block.Format, no sudo) exists
// so far.
#[cfg(unix)]
mod format;

pub use drives::{RemovableDrive, list_removable_drives};
#[cfg(unix)]
pub use format::format_as_cartridge;
pub use marker::has_cartridge_structure;

// CartridgeApp/CartridgeMarker/read_marker/MARKER_FILENAME are the schema
// #195/#196 (install progress, marker refresh) build on. Not re-exported
// here — nothing outside this module calls them until those land.
