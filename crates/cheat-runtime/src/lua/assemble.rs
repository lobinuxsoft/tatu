//! `autoAssemble` + symbol registration — the bridge from Lua into tatu's
//! Auto-Assembler executor.
//!
//! A framework cheat written in Lua calls `autoAssemble([[ ...AA script... ]])`
//! to apply a codecave the same way an `[ENABLE]` block would. We parse the
//! script, seed a fresh engine with every symbol bound so far (CE keeps one
//! global symbol table), run it, fold the newly-resolved symbols back into the
//! shared table, and keep the enabled cheat alive so its hooks don't roll back.

use std::rc::Rc;

use mlua::{Lua, MultiValue, Result as LuaResult, Value};

use super::state::LuaState;
use crate::executor::Engine;
use crate::parser;

/// Install `autoAssemble`, `registerSymbol` and `unregisterSymbol`.
pub(super) fn register(lua: &Lua, state: &Rc<LuaState>) -> LuaResult<()> {
    let g = lua.globals();

    // autoAssemble(script) -> true | (false, errorMessage)
    {
        let st = Rc::clone(state);
        g.set(
            "autoAssemble",
            lua.create_function(move |lua, script: String| {
                Ok(run_auto_assemble(lua, &st, &script))
            })?,
        )?;
    }

    // registerSymbol(name, address[, onlyGlobal]) — bind into the shared table.
    {
        let st = Rc::clone(state);
        g.set(
            "registerSymbol",
            lua.create_function(move |_, (name, addr): (String, Value)| {
                if let Some(addr) = st.resolve(&addr) {
                    st.bind_symbol(name, addr);
                }
                Ok(())
            })?,
        )?;
    }

    // unregisterSymbol(name)
    {
        let st = Rc::clone(state);
        g.set(
            "unregisterSymbol",
            lua.create_function(move |_, name: String| {
                st.unbind_symbol(&name);
                Ok(())
            })?,
        )?;
    }

    Ok(())
}

/// Parse + enable an AA script against the opened process, threading the shared
/// symbol table in and out. Returns CE's `(ok[, error])` convention.
fn run_auto_assemble(lua: &Lua, state: &LuaState, script: &str) -> MultiValue {
    let parsed = match parser::parse(script) {
        Ok(s) => s,
        Err(e) => return err_tuple(lua, format!("parse error: {e}")),
    };

    let mut engine = Engine::new(state.target());
    for (name, addr) in state.snapshot_symbols() {
        engine.bind_symbol(name, addr);
    }

    match engine.enable(&parsed) {
        Ok(cheat) => {
            state.merge_symbols(cheat.symbols());
            state.keep_alive(cheat);
            MultiValue::from_iter([Value::Boolean(true)])
        }
        Err(e) => err_tuple(lua, format!("enable error: {e}")),
    }
}

fn err_tuple(lua: &Lua, message: String) -> MultiValue {
    let msg = lua
        .create_string(&message)
        .map(Value::String)
        .unwrap_or(Value::Nil);
    MultiValue::from_iter([Value::Boolean(false), msg])
}
