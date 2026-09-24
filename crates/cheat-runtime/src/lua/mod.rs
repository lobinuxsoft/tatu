//! Native Lua runtime for framework cheat tables (Manifold-style).
//!
//! CE tables built on a Lua framework call CE's built-in primitives —
//! `readInteger`, `writeFloat`, `getAddress`, `autoAssemble`, … — to drive
//! memory from Lua instead of pure Auto-Assembler. tatu ships no CE, so those
//! globals don't exist and such cheats can't run. This module embeds a Lua 5.4
//! interpreter ([`mlua`], vendored) and registers CE-compatible globals backed
//! by tatu's own cross-process memory, process, and Auto-Assembler engines.
//!
//! All primitives share one [`LuaState`]: an opened-process PID (mutable via
//! `openProcess`) plus a global symbol table (written by `autoAssemble` and
//! `registerSymbol`, read by `getAddress`). This mirrors CE, where one symbol
//! table and one opened process are visible to every script.
//!
//! Read semantics mirror CE: a failed read returns `nil` (not an error), so the
//! framework's `Safe*` wrappers behave exactly as they do under CE. UI calls are
//! stubbed (see [`ui_stub`]) so a table's form-building bootstrap doesn't crash.

mod assemble;
mod ce_misc;
mod compat;
mod defines;
mod lfs;
mod memory;
mod process;
mod state;
mod ui_stub;

use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, StdLib};
use nix::unistd::Pid;

use state::LuaState;

/// A Lua interpreter wired to a target process. Each instance owns its Lua
/// state and resolves every primitive against the shared [`LuaState`].
pub struct LuaRuntime {
    lua: Lua,
    state: Rc<LuaState>,
}

impl LuaRuntime {
    /// Build a runtime targeting `pid` with the CE primitives, constants and
    /// UI stubs installed.
    ///
    /// Loads the full Lua standard library including `io`/`os`/`package`:
    /// framework modules (`CustomIO`, `Logger`) read and write state/log files
    /// on disk, exactly as they do under CE. This grants a (chosen) table
    /// filesystem access — consistent with CE's trust model, where the same
    /// table already injects arbitrary assembly.
    pub fn new(pid: Pid) -> LuaResult<Self> {
        let lua = Lua::new();
        // `io`/`os`/`package` aren't in mlua's safe default set.
        lua.load_std_libs(StdLib::IO | StdLib::OS | StdLib::PACKAGE)?;
        let state = Rc::new(LuaState::new(pid));
        compat::install(&lua)?;
        defines::install(&lua)?;
        ce_misc::install(&lua)?;
        lfs::install(&lua)?;
        memory::register(&lua, &state)?;
        process::register(&lua, &state)?;
        assemble::register(&lua, &state)?;
        ui_stub::install(&lua)?;
        Ok(Self { lua, state })
    }

    /// The process this runtime currently drives (changes after `openProcess`).
    pub fn pid(&self) -> Pid {
        self.state.target()
    }

    /// Execute a Lua chunk for its side effects.
    pub fn exec(&self, chunk: &str) -> LuaResult<()> {
        self.lua.load(chunk).exec()
    }

    /// Evaluate a Lua expression and return the result.
    pub fn eval<T: mlua::FromLuaMulti>(&self, chunk: &str) -> LuaResult<T> {
        self.lua.load(chunk).eval()
    }

    /// Set a global to a string value (no source interpolation, so arbitrary
    /// bytes — e.g. a cheat description — are safe).
    pub fn set_string(&self, name: &str, value: &str) -> LuaResult<()> {
        self.lua.globals().set(name, value)
    }

    /// Mount a table's embedded files so its bootstrap can load its modules.
    ///
    /// Framework headers pull their modules via CE's `findTableFile(name)` +
    /// `readStringLocal(stream.memory, stream.size)`. Those resolve against the
    /// *embedded files of one specific `.CT`*, so they aren't part of the base
    /// runtime — the table loader supplies them here from the already-decoded
    /// `name → contents` map. `findTableFile` returns the CE-shaped descriptor;
    /// `readStringLocal` hands the contents straight back.
    pub fn mount_table_files<I, K, V>(&self, files: I) -> LuaResult<()>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let table = self.lua.create_table()?;
        for (name, contents) in files {
            table.set(name.as_ref(), contents.as_ref())?;
        }
        self.lua.globals().set("__tatu_table_files", table)?;
        self.lua
            .load(
                r#"
                function findTableFile(name)
                    local c = __tatu_table_files[name]
                    if not c then return nil end
                    return { stream = { memory = c, size = #c } }
                end
                function readStringLocal(mem, _size) return mem end
            "#,
            )
            .set_name("@tatu:table_files")
            .exec()
    }
}

#[cfg(test)]
mod tests;
