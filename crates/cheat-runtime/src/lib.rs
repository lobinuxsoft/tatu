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

pub mod alloc;
pub mod asm;
pub mod aurora;
pub mod chain;
pub mod ct_import;
pub mod elfsym;
pub mod executor;
pub mod extension;
pub mod freeze;
pub mod inject;
pub mod manifest;
pub mod maps;
pub mod memory;
pub mod migrate;
pub mod parser;
pub mod process;
pub mod scanner;

pub use alloc::{AllocError, alloc_remote, dealloc_remote};
pub use asm::{AsmError, compile_line as compile_asm_line};
pub use aurora::{AuroraError, Feature, Trainer, load_trainer, load_trainer_file};
pub use chain::{
    AddrExpr, ChainError, Value, parse_addr_expr, read_chain, read_value, resolve_addr_expr,
    walk_chain, write_chain, write_value,
};
pub use ct_import::{
    CtImportError, ImportReport, auto_import_default_dirs, auto_import_for_app, convert_ct_file,
    import_dirs as ct_import_dirs,
};
pub use elfsym::{ElfSymError, find_libc_symbol, find_module_base, find_module_symbol};
pub use executor::{ActiveCheat, Engine, ExecError};
pub use extension::{Extension, ExtensionError};
pub use freeze::{FreezeError, FreezeHandle, FreezeKey, FreezeRegistry};
pub use inject::{InjectError, inject_so};
pub use manifest::{
    FeatureKind, Manifest, ManifestError, ManifestFeature, VType, ValueSpec, load_manifests_for,
    manifests_dir_for,
};
pub use maps::{MemoryRegion, Perms, parse_maps, read_maps};
pub use memory::{RuntimeError, read_bytes, write_bytes};
pub use migrate::{MigrateError, MigrateReport, migrate_default_dirs, migrate_dirs};
pub use parser::{ParseError as ScriptParseError, Script, Statement, parse as parse_script};
pub use process::{find_pid_by_exe, find_pids_by_exe};
pub use scanner::{ParseError as PatternParseError, Pattern, scan, scan_in_process};
