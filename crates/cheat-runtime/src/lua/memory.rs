//! CE memory read/write family, backed by tatu's cross-process primitives.
//!
//! Every read returns `nil` on a failed/short transfer (CE semantics, so the
//! framework's `Safe*` wrappers behave identically); every write returns a
//! boolean success flag. All of them resolve their address operand and target
//! PID through the shared [`LuaState`].

use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, Value, Variadic};

use super::state::LuaState;
use crate::memory::{read_bytes, write_bytes};

/// Read exactly `N` bytes from the opened process, or `None` on any
/// short/failed read.
fn read_n<const N: usize>(state: &LuaState, addr: u64) -> Option<[u8; N]> {
    read_bytes(state.target(), addr, N).ok()?.try_into().ok()
}

/// Install `getAddress` + the read/write family as Lua globals.
pub(super) fn register(lua: &Lua, state: &Rc<LuaState>) -> LuaResult<()> {
    let g = lua.globals();

    // getAddress(operand) / getAddressSafe — both nil on failure in our model.
    for name in ["getAddress", "getAddressSafe"] {
        let st = Rc::clone(state);
        g.set(
            name,
            lua.create_function(move |_, v: Value| Ok(st.resolve(&v)))?,
        )?;
    }

    // --- reads: nil on failure, signed where CE is signed ---
    {
        let st = Rc::clone(state);
        g.set(
            "readBytes",
            lua.create_function(move |_, (v, count): (Value, usize)| {
                let Some(addr) = st.resolve(&v) else {
                    return Ok(None);
                };
                Ok(read_bytes(st.target(), addr, count)
                    .ok()
                    .map(|b| b.to_vec()))
            })?,
        )?;
    }
    register_read::<1>(lua, state, "readByte", |b| i64::from(b[0] as i8))?;
    register_read::<2>(lua, state, "readSmallInteger", |b| {
        i64::from(i16::from_le_bytes(b))
    })?;
    register_read::<4>(lua, state, "readInteger", |b| {
        i64::from(i32::from_le_bytes(b))
    })?;
    register_read::<8>(lua, state, "readQword", i64::from_le_bytes)?;
    register_read::<8>(lua, state, "readPointer", |b| u64::from_le_bytes(b) as i64)?;
    register_read_f::<4>(lua, state, "readFloat", |b| {
        f64::from(f32::from_le_bytes(b))
    })?;
    register_read_f::<8>(lua, state, "readDouble", f64::from_le_bytes)?;

    // --- writes: false on a bad address, false on a short write ---
    register_write(lua, state, "writeByte", |v: i64| vec![v as u8])?;
    register_write(lua, state, "writeSmallInteger", |v: i64| {
        (v as i16).to_le_bytes().to_vec()
    })?;
    register_write(lua, state, "writeInteger", |v: i64| {
        (v as i32).to_le_bytes().to_vec()
    })?;
    register_write(lua, state, "writeQword", |v: i64| v.to_le_bytes().to_vec())?;
    register_write_f(lua, state, "writeFloat", |v: f64| {
        (v as f32).to_le_bytes().to_vec()
    })?;
    register_write_f(lua, state, "writeDouble", |v: f64| v.to_le_bytes().to_vec())?;
    {
        let st = Rc::clone(state);
        g.set(
            "writeBytes",
            lua.create_function(move |_, (v, bytes): (Value, Variadic<i64>)| {
                let Some(addr) = st.resolve(&v) else {
                    return Ok(false);
                };
                let data: Vec<u8> = bytes.iter().map(|b| *b as u8).collect();
                Ok(write_bytes(st.target(), addr, &data).is_ok())
            })?,
        )?;
    }

    Ok(())
}

fn register_read<const N: usize>(
    lua: &Lua,
    state: &Rc<LuaState>,
    name: &str,
    conv: fn([u8; N]) -> i64,
) -> LuaResult<()> {
    let st = Rc::clone(state);
    lua.globals().set(
        name,
        lua.create_function(move |_, v: Value| {
            let Some(addr) = st.resolve(&v) else {
                return Ok(None);
            };
            Ok(read_n::<N>(&st, addr).map(conv))
        })?,
    )
}

fn register_read_f<const N: usize>(
    lua: &Lua,
    state: &Rc<LuaState>,
    name: &str,
    conv: fn([u8; N]) -> f64,
) -> LuaResult<()> {
    let st = Rc::clone(state);
    lua.globals().set(
        name,
        lua.create_function(move |_, v: Value| {
            let Some(addr) = st.resolve(&v) else {
                return Ok(None);
            };
            Ok(read_n::<N>(&st, addr).map(conv))
        })?,
    )
}

fn register_write(
    lua: &Lua,
    state: &Rc<LuaState>,
    name: &str,
    enc: fn(i64) -> Vec<u8>,
) -> LuaResult<()> {
    let st = Rc::clone(state);
    lua.globals().set(
        name,
        lua.create_function(move |_, (v, val): (Value, i64)| {
            let Some(addr) = st.resolve(&v) else {
                return Ok(false);
            };
            Ok(write_bytes(st.target(), addr, &enc(val)).is_ok())
        })?,
    )
}

fn register_write_f(
    lua: &Lua,
    state: &Rc<LuaState>,
    name: &str,
    enc: fn(f64) -> Vec<u8>,
) -> LuaResult<()> {
    let st = Rc::clone(state);
    lua.globals().set(
        name,
        lua.create_function(move |_, (v, val): (Value, f64)| {
            let Some(addr) = st.resolve(&v) else {
                return Ok(false);
            };
            Ok(write_bytes(st.target(), addr, &enc(val)).is_ok())
        })?,
    )
}
