//! Error type for the executor. Lifted out of `mod.rs` so the rest of the
//! split modules can refer to it without cycling through the root.

use crate::alloc::AllocError;
use crate::asm::AsmError;
use crate::memory::RuntimeError;
use crate::scanner;
use crate::threads::ThreadPauseError;

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("pattern parse error in aobscanmodule: {0}")]
    Pattern(#[from] scanner::ParseError),
    #[error("memory io: {0}")]
    Memory(#[from] RuntimeError),
    #[error("remote alloc: {0}")]
    Alloc(#[from] AllocError),
    #[error("asm compile: {0}")]
    Asm(#[from] AsmError),
    #[error("aobscanmodule({symbol}): no match in any executable region")]
    PatternNotFound { symbol: String },
    #[error("aobscanmodule({symbol}): {count} matches found, pattern must be unique")]
    PatternAmbiguous { symbol: String, count: usize },
    #[error("unknown symbol {0:?}")]
    UnknownSymbol(String),
    #[error("dealloc({0:?}) before matching alloc — symbol not in active region table")]
    DeallocUnknown(String),
    #[error("write outside any label site: {0:?}")]
    OrphanWrite(String),
    #[error("unsupported statement: {0}")]
    Unsupported(String),
    #[error("thread pause: {0}")]
    ThreadPause(#[from] ThreadPauseError),
}
