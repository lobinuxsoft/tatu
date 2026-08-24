// Portable multi-game cartridge (#192): a removable drive laid out as a
// standard Steam library so Steam's own client handles the download. New
// as of #193 — `disk.rs` is Steam library *size estimation*, unrelated
// despite the similar name.
mod drives;
mod marker;

pub use drives::{RemovableDrive, list_removable_drives};
pub use marker::has_cartridge_structure;

// CartridgeApp/CartridgeMarker/read_marker/MARKER_FILENAME/MARKER_FORMAT_VERSION
// are the schema #194 (format writer) and #195/#196 (install progress,
// marker refresh) build on. Not re-exported here — nothing outside this
// module calls them until those land; add the `pub use` when they do.
