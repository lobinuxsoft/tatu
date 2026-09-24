//! Backend-agnostic AA-script executor.
//!
//! Walks a parsed [`crate::parser::Script`] and produces an
//! [`EnableOutcome`] POD describing every byte the cheat would write,
//! every codecave it allocated, and the symbol table it built. The
//! caller's [`crate::Backend`] impl supplies the platform I/O:
//! `read/write`, remote `alloc/dealloc`, region enumeration,
//! attach/detach, instruction-cache flush.
//!
//! Two-pass evaluation matching CE's own autoassembler:
//! - Pass 1: side-effecting symbol providers (`aobscanmodule`,
//!   `alloc`) plus a virtual cursor that estimates the byte length
//!   of every `Statement::Raw` so forward labels resolve before
//!   pass 2 emits bytes.
//! - Pass 2: walks again with the now-complete symbol table and
//!   writes the bytes through the backend, wrapping the whole batch
//!   in [`Backend::attach`] + [`Backend::detach`] when the backend
//!   reports an attach is meaningful.
//!
//! On any error every prior write is rolled back via the same
//! backend before returning — atomic enable, no half-state.

mod engine;
mod error;
mod length;
mod outcome;
mod raw_compiler;

pub use engine::Engine;
pub use error::ExecError;
pub use outcome::EnableOutcome;

use crate::backend::Backend;

/// Replay the `outcome.undo` log in reverse and release every alloc
/// it still owns. Standalone helper so callers that hold an
/// [`EnableOutcome`] (e.g. the Linux runtime's `ActiveCheat::disable`,
/// the bridge's recovery handler) can drive rollback without
/// reviving the original [`Engine`].
///
/// Mirrors the path [`Engine::enable`] uses internally on failure:
/// one [`Backend::attach`] around the batch, instruction-cache flush
/// after each restored write, [`Backend::dealloc`] for codecaves.
pub fn rollback<B: Backend>(backend: &mut B, outcome: &mut EnableOutcome) {
    let _attached = backend.attach();
    while let Some((addr, bytes)) = outcome.undo.pop() {
        if let Err(e) = backend.write(addr, &bytes) {
            eprintln!("[engine] rollback write @{addr:#x} failed: {e}");
            outcome.undo.push((addr, bytes));
            break;
        }
        let _ = backend.flush_instruction_cache(addr, 0);
    }
    backend.detach();
    for (_, (addr, size)) in outcome.allocs.drain() {
        if let Err(e) = backend.dealloc(addr, size) {
            eprintln!("[engine] rollback dealloc @{addr:#x} failed: {e}");
        }
    }
}
