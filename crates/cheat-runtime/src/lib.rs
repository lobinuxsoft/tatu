//! Linux-native Cheat Engine Auto-Assembler runtime in Rust.
//!
//! The crate is split into orthogonal layers; the lowest one (this PR) is
//! `memory` + `maps`, providing out-of-process read/write to a target Linux
//! process via `process_vm_readv` / `process_vm_writev` plus enumeration of
//! mapped regions parsed from `/proc/<pid>/maps`.
//!
//! Higher layers — pattern scanner, CE Auto-Assembler parser, executor and
//! Aurora JSON loader — land in subsequent PRs of issue #64.
//!
//! Design constraints (see `feedback_rust_dod_mandatory` + project memory):
//! - Plain-old-data structs, slice inputs, `io::Result` returns.
//! - No `unwrap` in non-test code; errors bubble through `RuntimeError`.
//! - The runtime is process-agnostic: any Linux PID, Proton or native.

pub mod aurora;
pub mod executor;
pub mod maps;
pub mod memory;
pub mod parser;
pub mod scanner;

pub use aurora::{AuroraError, Feature, Trainer, load_trainer, load_trainer_file};
pub use executor::{ActiveCheat, Engine, ExecError};
pub use maps::{MemoryRegion, Perms, parse_maps, read_maps};
pub use memory::{RuntimeError, read_bytes, write_bytes};
pub use parser::{ParseError as ScriptParseError, Script, Statement, parse as parse_script};
pub use scanner::{ParseError as PatternParseError, Pattern, scan, scan_in_process};
