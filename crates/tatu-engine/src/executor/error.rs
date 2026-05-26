//! Error type for the executor. Backend-agnostic: backend-specific
//! failures (Linux ptrace `EFAULT`, Win32 `WriteProcessMemory`
//! returning 0, etc.) are boxed under [`ExecError::Backend`] via the
//! `Backend::Error` alias from `crate::backend`.

use crate::asm::AsmError;
use crate::backend::BackendError;
use crate::parser::ParseError as ScriptParseError;
use tatu_mem::pattern::ParseError as PatternParseError;

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("script parse: {0}")]
    ScriptParse(#[from] ScriptParseError),
    #[error("pattern parse error in aobscanmodule: {0}")]
    Pattern(#[from] PatternParseError),
    #[error("backend: {0}")]
    Backend(#[from] BackendError),
    #[error("asm compile: {0}")]
    Asm(#[from] AsmError),
    #[error("aobscanmodule({symbol}): no match in any readable region")]
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
}
