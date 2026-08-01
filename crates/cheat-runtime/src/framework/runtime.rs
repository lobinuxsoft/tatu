//! A bootstrapped framework runtime: one Lua interpreter with the framework's
//! modules loaded and its globals (`memory`, `utils`, `state`, …) live, ready to
//! run a cheat's `[ENABLE]`/`[DISABLE]` blocks.
//!
//! One [`FrameworkRuntime`] is kept alive per opened table for the duration of a
//! session — the framework holds the symbol table, allocations and state that a
//! later toggle depends on, so it must outlive individual enables (just like
//! CE keeps the table's Lua state resident).

use nix::unistd::Pid;

use super::FrameworkTable;
use crate::lua::LuaRuntime;

/// The minimal memory-record view a cheat's Lua block reads (`memrec.ID`,
/// `memrec.Description`). Sourced from the originating `<CheatEntry>`.
#[derive(Debug, Clone)]
pub struct MemRec {
    pub id: i64,
    pub description: String,
}

/// Errors loading or running a framework table.
#[derive(Debug, thiserror::Error)]
pub enum FrameworkError {
    #[error("framework bootstrap failed: {0}")]
    Bootstrap(#[source] mlua::Error),
    #[error("cheat block failed: {0}")]
    Block(#[source] mlua::Error),
}

/// A framework table bootstrapped against a target process.
pub struct FrameworkRuntime {
    lua: LuaRuntime,
}

impl FrameworkRuntime {
    /// Bootstrap `table` against `pid`: mount its modules, point the framework
    /// at the process, and run its header once. The resulting globals stay live
    /// for subsequent [`Self::enable`]/[`Self::disable`] calls.
    pub fn load(pid: Pid, table: &FrameworkTable) -> Result<Self, FrameworkError> {
        let lua = LuaRuntime::new(pid).map_err(FrameworkError::Bootstrap)?;
        lua.mount_table_files(table.files.iter().map(|(n, c)| (n, c)))
            .map_err(FrameworkError::Bootstrap)?;
        // Point CE's process model at the target before the header runs, so the
        // framework's ProcessHandler binds the same PID we drive memory through.
        lua.exec(&format!("openProcess({})", pid.as_raw()))
            .map_err(FrameworkError::Bootstrap)?;
        lua.exec(&table.header).map_err(FrameworkError::Bootstrap)?;
        Ok(Self { lua })
    }

    /// Run a cheat's `[ENABLE]` Lua with its `memrec` in scope.
    pub fn enable(&self, memrec: &MemRec, enable_src: &str) -> Result<(), FrameworkError> {
        self.run_block(memrec, enable_src)
    }

    /// Run a cheat's `[DISABLE]` Lua with its `memrec` in scope.
    pub fn disable(&self, memrec: &MemRec, disable_src: &str) -> Result<(), FrameworkError> {
        if disable_src.trim().is_empty() {
            return Ok(());
        }
        self.run_block(memrec, disable_src)
    }

    /// The process this runtime drives.
    pub fn pid(&self) -> Pid {
        self.lua.pid()
    }

    fn run_block(&self, memrec: &MemRec, src: &str) -> Result<(), FrameworkError> {
        // `memrec`/`syntaxcheck` are the names CE has in scope while a cheat's
        // block runs. Description goes through set_string to avoid interpolating
        // arbitrary bytes; ID is a plain integer. Unknown memrec fields fall
        // back to an inert stub so a block touching `memrec.Foo` can't nil-crash.
        self.lua
            .set_string("__tatu_memrec_desc", &memrec.description)
            .map_err(FrameworkError::Block)?;
        self.lua
            .exec(&format!(
                r#"
                syntaxcheck = false
                local __stub = {{}}
                setmetatable(__stub, {{ __index = function() return __stub end,
                                        __call = function() return __stub end }})
                memrec = setmetatable(
                    {{ ID = {id}, Description = __tatu_memrec_desc, Active = false }},
                    {{ __index = function() return __stub end }})
                "#,
                id = memrec.id
            ))
            .map_err(FrameworkError::Block)?;
        self.lua.exec(src).map_err(FrameworkError::Block)
    }
}
