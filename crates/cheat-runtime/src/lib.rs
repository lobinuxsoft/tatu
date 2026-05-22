//! Internal cheat-engine library — shared logic + Linux primitives.
//!
//! # Status
//!
//! Not surfaced as a backend from the tracker. The Win32 bridge
//! (`tatu-bridge`) is the only path the user can enable for a game;
//! its [`bridge_client`] module is what the tracker dials at runtime.
//!
//! This crate stays alive for three reasons:
//!
//! - **Shared logic**: manifest schema, CT importer, value-chain
//!   parser, Aurora JSON loader, freeze registry. The bridge runs
//!   on Windows but consumes the same on-disk manifests.
//! - **Tests + research**: the local Linux ptrace primitives are
//!   handy for verifying parser / executor changes without the
//!   cross-compile + Wine bootstrap the bridge needs.
//! - **Persistence + recovery**: [`PersistedHook`](persisted_hook::PersistedHook)
//!   round-trips both Bridge and (legacy) Linux records; the tracker
//!   drops legacy Linux records on recovery (no live re-attach), but
//!   the disk format keeps both shapes for backward compatibility.
//!
//! Anti-cheat games (EAC / BattlEye / Vanguard) are explicitly out
//! of scope.
//!
//! # Architecture
//!
//! Layered: `memory` + `maps` at the bottom (out-of-process read/
//! write via `process_vm_readv` / `process_vm_writev` plus parsing
//! of `/proc/<pid>/maps`), pattern scanner, executor, freeze
//! registry, and Aurora JSON loader on top. The CE Auto-Assembler
//! parser and single-line assembler live in `tatu-engine` so the
//! bridge can share them.
//!
//! Design constraints (see `feedback_rust_dod_mandatory` + project
//! memory):
//! - Plain-old-data structs, slice inputs, `io::Result` returns.
//! - No `unwrap` in non-test code; errors bubble through `RuntimeError`.
//! - The runtime is process-agnostic: any Linux PID, Proton or native.

pub mod alloc;
pub mod aurora;
pub mod bridge_client;
pub mod chain;
pub mod ct_import;
pub mod elfsym;
pub mod executor;
pub mod extension;
pub mod freeze;
pub mod inject;
pub mod linux_backend;
pub mod manifest;
pub mod maps;
pub mod memory;
pub mod memory_access;
pub mod migrate;
pub mod persisted_hook;
pub mod process;
pub mod scanner;
pub mod threads;

// parser + asm live in tatu-engine since Phase 7A1 — re-exported
// here as `cheat_runtime::parser` / `cheat_runtime::asm` for the
// existing call sites (executor, ce-launcher, tests, the tracker).
pub use tatu_engine::{asm, parser};

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
pub use freeze::{FreezeError, FreezeHandle, FreezeKey, FreezeRegistry, FreezeTarget};
pub use inject::{InjectError, inject_so};
pub use manifest::{
    FeatureKind, Manifest, ManifestError, ManifestFeature, Prereq, VType, ValueSpec,
    load_manifests_for, manifests_dir_for,
};
pub use maps::{MemoryRegion, Perms, parse_maps, read_maps};
pub use memory::{RuntimeError, read_bytes, write_bytes};
pub use migrate::{MigrateError, MigrateReport, migrate_default_dirs, migrate_dirs};
pub use nix::unistd::Pid;
pub use parser::{ParseError as ScriptParseError, Script, Statement, parse as parse_script};
pub use persisted_hook::{
    BackendKind, LoadAllReport as PersistedLoadAllReport, PersistError, PersistedAlloc,
    PersistedHook, PersistedWrite, load_all as load_all_persisted_hooks, persist_dir,
};
pub use process::{find_pid_by_exe, find_pids_by_exe};
pub use scanner::{ParseError as PatternParseError, Pattern, scan, scan_in_process};
