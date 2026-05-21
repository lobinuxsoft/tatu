//! Backend-agnostic CE Auto-Assembler engine.
//!
//! Currently exposes the two pure-Rust modules that have no
//! platform-specific I/O:
//!
//! - [`parser`] — CE Auto-Assembler script parser. Produces a
//!   [`parser::Script`] from a `[ENABLE]` / `[DISABLE]` text body.
//! - [`asm`] — single-line x86_64 assembler used by the executor to
//!   compile `Statement::Raw` lines into bytes that the platform
//!   backend will eventually write.
//!
//! The platform-specific I/O (ptrace runtime on Linux, Win32 bridge
//! under Wine) lives behind the [`tatu_mem::MemoryAccess`] trait plus
//! the backend extension traits that land in PR 7A2 (`Backend`).
//! Until those land, `cheat-runtime`'s `Engine` keeps its existing
//! Linux-only state machine; this crate is the foundation other
//! backends — and the upcoming generic `Engine<B>` — will share.

pub mod asm;
pub mod parser;
