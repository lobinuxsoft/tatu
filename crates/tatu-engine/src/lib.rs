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
//! Today only `tatu-bridge`'s `Win32Backend` is wired through the
//! tracker — Linux-native ELF games are out of scope; see the
//! README's "Scope" section. `cheat-runtime`'s `LinuxBackend`
//! still implements the trait for tests / local research, but
//! nothing in the user-facing flow routes through it. Memory I/O
//! sits behind [`tatu_mem::MemoryAccess`]; the [`Backend`] trait
//! adds cross-process primitives (allocate, suspend, scan) so the
//! engine can drive any conforming backend from one state machine.

pub mod asm;
pub mod backend;
pub mod executor;
pub mod parser;

pub use backend::{Backend, BackendError, ReadableRegion, RegionPerms};
pub use executor::{EnableOutcome, Engine, ExecError, rollback};
