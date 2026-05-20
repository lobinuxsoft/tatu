//! Executor that walks a parsed [`Script`] and applies it to a live process.
//!
//! Scope (matches issue #64's out-of-scope list):
//! - **Supported**: `aobscanmodule`, `registersymbol`, `unregistersymbol`,
//!   `label`, label sites (`name:`), and write directives following a label
//!   site (`db`, `dq`, `nop N`, `readmem(symbol, len)`).
//! - **Unsupported (returns [`ExecError::Unsupported`])**: `alloc`, `dealloc`,
//!   inline assembly mnemonics (`push`, `mov`, `jmp`, …), and any other
//!   `Statement::Raw` line we don't recognise.
//!
//! Atomicity: [`Engine::enable`] keeps an undo log of every byte sequence
//! it overwrites. If any later statement fails, every previously applied
//! write is reverted before returning the error — there is no partial state.
//!
//! [`ActiveCheat::disable`] reverts the same writes in reverse order. After
//! disable the target process's memory is byte-for-byte identical to what
//! it was before [`Engine::enable`] was called.
//!
//! ## Submodule layout
//!
//! - [`engine`] — [`Engine`], the two-pass enable orchestrator and `scan_unique`.
//! - [`active`] — [`ActiveCheat`], the undo-log owner with `Drop` rollback.
//! - [`rollback`] — ptrace attach/detach + the shared rollback walker.
//! - [`error`] — [`ExecError`].
//! - [`length`] — pass-1 instruction length estimator.
//! - [`raw_compiler`] — pass-2 byte emission for `Statement::Raw`.

mod active;
mod engine;
mod error;
mod length;
mod raw_compiler;
mod rollback;

pub use active::ActiveCheat;
pub use engine::Engine;
pub use error::ExecError;

#[cfg(test)]
mod tests;
