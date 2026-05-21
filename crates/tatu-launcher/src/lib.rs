//! Library half of the Tatu Steam compatibility tool. The `[[bin]]`
//! target consumes these modules to do its job at Steam invocation
//! time; the tracker consumes [`config`] from `src-tauri` so the
//! "Enable Tatu" UI can read and write `launcher.toml` without
//! shelling out to the binary or duplicating the schema.

pub mod config;
pub mod launch;
pub mod proton;
