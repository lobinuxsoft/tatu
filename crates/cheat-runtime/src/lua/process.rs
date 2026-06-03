//! CE process/module discovery primitives.
//!
//! `openProcess` is the one that mutates shared state — it re-targets every
//! later memory and assembler call. The rest are read-only lookups against
//! `/proc`. A Manifold bootstrap typically calls `getProcessIDFromProcessName`
//! then `openProcess`, so both must succeed before anything else runs.

use std::path::Path;
use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, Table, Value};
use nix::unistd::Pid;

use super::state::LuaState;
use crate::process::find_pid_by_exe;
use crate::symbol_table::enumerate_modules;

/// Install `getProcessIDFromProcessName`, `openProcess`, `getOpenedProcessID`
/// and `enumModules` as Lua globals.
pub(super) fn register(lua: &Lua, state: &Rc<LuaState>) -> LuaResult<()> {
    let g = lua.globals();

    // getProcessIDFromProcessName(name) -> pid | nil
    g.set(
        "getProcessIDFromProcessName",
        lua.create_function(|_, name: String| {
            Ok(find_pid_by_exe(&name).map(|p| p.as_raw() as i64))
        })?,
    )?;

    // openProcess(name|pid) — set the opened process. CE accepts either a
    // process name or a numeric PID; returns the resolved PID or nil.
    {
        let st = Rc::clone(state);
        g.set(
            "openProcess",
            lua.create_function(move |_, v: Value| {
                let pid = match v {
                    Value::Integer(n) => Some(Pid::from_raw(n as i32)),
                    Value::Number(n) => Some(Pid::from_raw(n as i32)),
                    Value::String(s) => find_pid_by_exe(&s.to_str()?),
                    _ => None,
                };
                if let Some(pid) = pid {
                    st.set_target(pid);
                    return Ok(Some(pid.as_raw() as i64));
                }
                Ok(None)
            })?,
        )?;
    }

    // getOpenedProcessID() -> pid
    {
        let st = Rc::clone(state);
        g.set(
            "getOpenedProcessID",
            lua.create_function(move |_, ()| Ok(st.target().as_raw() as i64))?,
        )?;
    }

    // enumModules() -> { {Name=..., Address=...}, ... } in load order.
    {
        let st = Rc::clone(state);
        g.set(
            "enumModules",
            lua.create_function(move |lua, ()| {
                let modules = enumerate_modules(st.target()).unwrap_or_default();
                let out = lua.create_table()?;
                for (i, m) in modules.iter().enumerate() {
                    let name = Path::new(&m.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&m.path);
                    let entry: Table = lua.create_table()?;
                    entry.set("Name", name)?;
                    entry.set("Address", m.load_base)?;
                    out.set(i + 1, entry)?;
                }
                Ok(out)
            })?,
        )?;
    }

    Ok(())
}
