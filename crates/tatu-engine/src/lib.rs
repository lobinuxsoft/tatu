//! Backend-agnostic CE Auto-Assembler engine.
//!
//! Hosts the pure-logic modules both backends share:
//!
//! - [`parser`] — CE Auto-Assembler script parser. Produces a
//!   [`parser::Script`] from a `[ENABLE]` / `[DISABLE]` text body.
//! - [`asm`] — single-line x86_64 assembler used by the executor to
//!   compile `Statement::Raw` lines into bytes the platform backend
//!   writes.
//! - [`backend`] — the `Backend` trait + the generic [`Engine<B>`]
//!   that drives a script's lifecycle against any backend
//!   implementing it.
//!
//! Two backends consume the trait today: `cheat-runtime`'s
//! `LinuxBackend` (ptrace fallback for Linux-native ELF games) and
//! `tatu-bridge`'s `Win32Backend` (the preferred path for every
//! Proton/Wine game — see the README's "Backend selection" section
//! for the why). Memory I/O sits behind
//! [`tatu_mem::MemoryAccess`]; the [`Backend`] trait adds
//! cross-process primitives (allocate, suspend, scan) so the engine
//! can drive both paths from one state machine.

pub mod analysis;
pub mod asm;
pub mod backend;
pub mod executor;
pub mod parser;

pub use backend::{Backend, BackendError, ReadableRegion, RegionPerms};
pub use executor::{EnableOutcome, Engine, ExecError, rollback};
