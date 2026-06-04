//! Linux ptrace fallback backend for the Tatu cheat runtime.
//!
//! # Status
//!
//! This crate is the **fallback** path. The preferred backend for
//! every Windows game running under Proton/Wine is `tatu-bridge`
//! (the [`bridge_client`] module wires the tracker to it).
//! `tatu-bridge` runs as a Win32 worker inside the same Proton
//! invocation as the game; the atomic `WriteProcessMemory` +
//! `SuspendThread` + `FlushInstructionCache` cycle hits the game's
//! actual address space — the only safe way to patch `.text` on
//! Win64 (the i-cache stays stale otherwise).
//!
//! Keep this crate for:
//!
//! - **Linux-native ELF games**, where no wineprefix exists and the
//!   bridge has nothing to attach to. `process_vm_writev` +
//!   `PTRACE_ATTACH` is the only available primitive.
//! - **Tests + tooling** that operate on a local Linux process
//!   without the cross-compile / Wine bootstrap dance the bridge
//!   needs.
//!
//! Anti-cheat games (EAC / BattlEye / Vanguard) are explicitly out
//! of scope on both backends.
//!
//! # Architecture
//!
//! The crate is split into orthogonal layers; the lowest one is
//! `memory` + `maps`, providing out-of-process read/write to a
//! target Linux process via `process_vm_readv` / `process_vm_writev`
//! plus enumeration of mapped regions parsed from `/proc/<pid>/maps`.
//!
//! Higher layers — pattern scanner, executor, freeze registry and
//! Aurora JSON loader — sit on top. The CE Auto-Assembler parser
//! and single-line assembler now live in `tatu-engine` so the
//! bridge can share them.
//!
//! Design constraints (see `feedback_rust_dod_mandatory` + project
//! memory):
//! - Plain-old-data structs, slice inputs, `io::Result` returns.
//! - No `unwrap` in non-test code; errors bubble through `RuntimeError`.
//! - The runtime is process-agnostic: any Linux PID, Proton or native.

pub mod alloc;
pub mod aurora;
pub mod chain;
pub mod ct_import;
pub mod debug;
pub mod elfsym;
pub mod executor;
pub mod framework;
pub mod freeze;
pub mod linux_backend;
pub mod lua;
pub mod manifest;
pub mod maps;
pub mod memory;
pub mod memory_access;
pub mod memory_debug;
pub mod migrate;
pub mod mono_bridge;
pub mod persisted_hook;
pub mod prereqs;
pub mod process;
pub mod ptrace_helpers;
pub mod regions;
pub mod scanner;
pub mod symbol_table;
pub mod thread_context;
pub mod thread_control;
pub mod threads;
pub mod unity;

// parser + asm live in tatu-engine since Phase 7A1 — re-exported
// here as `cheat_runtime::parser` / `cheat_runtime::asm` for the
// existing call sites (executor, ce-launcher, tests, the tracker).
pub use tatu_engine::{analysis, asm, parser};

pub use alloc::{AllocError, alloc_remote, dealloc_remote};
pub use asm::{AsmError, compile_line as compile_asm_line};
pub use aurora::{AuroraError, Feature, Trainer, load_trainer, load_trainer_file};
pub use chain::{
    AddrExpr, ChainError, Value, parse_addr_expr, read_chain, read_value, resolve_addr_expr,
    walk_chain, write_chain, write_value,
};
pub use ct_import::{CtImportError, convert_ct_file, convert_ct_file_with_exe_hint};
pub use debug::{BpSize, BpType, DebugError, DebugEvent, DebugEventKind, Debugger};
pub use elfsym::{ElfSymError, find_libc_symbol, find_module_base, find_module_symbol};
pub use executor::{ActiveCheat, Engine, ExecError};
pub use framework::{
    FrameworkError, FrameworkRuntime, FrameworkTable, MemRec, framework_target_exe,
    is_framework_table, is_lua_cheat, lua_enable_disable, parse_framework_table,
};
pub use freeze::{FreezeError, FreezeHandle, FreezeKey, FreezeRegistry, FreezeTarget};
pub use manifest::{
    FeatureKind, Manifest, ManifestError, ManifestFeature, Prereq, RecursiveFeatureIter, VType,
    ValueSpec, ct_tables_dir_for, load_manifests_for, load_manifests_from_roots, manifests_dir_for,
};
pub use maps::{MemoryRegion, Perms, parse_maps, read_maps};
pub use memory::{RuntimeError, read_bytes, write_bytes};
pub use memory_debug::{read_via_ptrace, write_via_ptrace};
pub use migrate::{MigrateError, MigrateReport, migrate_default_dirs, migrate_dirs};
pub use mono_bridge::{MonoClient, MonoError};
pub use nix::unistd::Pid;
pub use parser::{ParseError as ScriptParseError, Script, Statement, parse as parse_script};
pub use persisted_hook::{
    LoadAllReport as PersistedLoadAllReport, PersistError, PersistedAlloc, PersistedHook,
    PersistedWrite, load_all as load_all_persisted_hooks, persist_dir,
};
pub use prereqs::{PrereqStatus, detect_reframework};
pub use process::{find_pid_by_exe, find_pids_by_exe};
pub use ptrace_helpers::{
    PtraceError, PtraceResult, attach_and_wait, install_sigchld_handler, safe_ptrace,
    wait_for_debug_event,
};
pub use regions::{
    MEM_MAPPED, MEM_PRIVATE, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE, RegionCache, RegionInfo, VQE_DIRTYONLY,
    VQE_NOSHARED, VQE_PAGEDONLY, enumerate_regions, enumerate_regions_in, linux_prot_to_windows,
    perms_to_memory_type, perms_to_windows_protection, query_region, query_region_in,
    windows_protection_to_linux,
};
pub use scanner::{ParseError as PatternParseError, Pattern, scan, scan_in_process};
pub use symbol_table::{Module, SymbolError, SymbolSpec, SymbolTable, enumerate_modules};
pub use thread_context::{
    ContextFlags, ThreadContext, get_thread_context, read_debug_register, set_thread_context,
    write_debug_register,
};
pub use thread_control::{find_paused_thread, resume_thread, suspend_count, suspend_thread};
pub use unity::{UnityBackend, detect_unity_backend, find_mono_runtime};
