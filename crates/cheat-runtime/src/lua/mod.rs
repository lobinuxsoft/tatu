//! Native Lua runtime for framework cheat tables (Manifold-style).
//!
//! CE tables built on a Lua framework call CE's built-in primitives —
//! `readInteger`, `writeFloat`, `getAddress`, … — to drive memory from Lua
//! instead of pure Auto-Assembler. tatu ships no CE, so those globals don't
//! exist and such cheats can't run. This module embeds a Lua 5.4 interpreter
//! ([`mlua`], vendored) and registers CE-compatible globals backed by tatu's
//! own cross-process memory primitives.
//!
//! Phase 0 scope: the memory read/write family + `getAddress` (module-relative
//! and numeric). Process control, `AutoAssemble`, the memory-record model, and
//! stubbed UI follow in later phases — see the module roadmap.
//!
//! Semantics mirror CE: a failed read returns `nil` (not an error), so the
//! framework's `Safe*` wrappers behave exactly as they do under CE.

use mlua::{Lua, Result as LuaResult, Value, Variadic};
use nix::unistd::Pid;

use crate::elfsym::find_module_base;
use crate::memory::{read_bytes, write_bytes};

/// A Lua interpreter wired to a target process. Each instance owns its Lua
/// state and resolves every primitive against `pid`.
pub struct LuaRuntime {
    lua: Lua,
    pid: Pid,
}

impl LuaRuntime {
    /// Build a runtime bound to `pid` with the CE memory primitives installed.
    pub fn new(pid: Pid) -> LuaResult<Self> {
        let lua = Lua::new();
        register_memory(&lua, pid)?;
        Ok(Self { lua, pid })
    }

    /// The target process this runtime drives.
    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// Execute a Lua chunk for its side effects.
    pub fn exec(&self, chunk: &str) -> LuaResult<()> {
        self.lua.load(chunk).exec()
    }

    /// Evaluate a Lua expression and return the result.
    pub fn eval<T: mlua::FromLuaMulti>(&self, chunk: &str) -> LuaResult<T> {
        self.lua.load(chunk).eval()
    }
}

/// Resolve a CE address operand: a bare number, or a `module+hexoffset` /
/// `module` string (CE treats the offset as hex). Registered AA symbols come
/// in a later phase once the runtime shares the executor's symbol table.
fn resolve_address(pid: Pid, value: &Value) -> Option<u64> {
    match value {
        Value::Integer(n) => Some(*n as u64),
        Value::Number(n) => Some(*n as u64),
        Value::String(s) => resolve_symbol(pid, &s.to_str().ok()?),
        _ => None,
    }
}

fn resolve_symbol(pid: Pid, expr: &str) -> Option<u64> {
    let expr = expr.trim();
    // `module+offset` (offset is hex, CE convention) or bare `module`.
    if let Some((module, off)) = expr.split_once('+') {
        let base = find_module_base(pid, module.trim()).ok()?;
        let offset = u64::from_str_radix(off.trim().trim_start_matches("0x"), 16).ok()?;
        return Some(base.wrapping_add(offset));
    }
    if let Ok(addr) = u64::from_str_radix(expr.trim_start_matches("0x"), 16) {
        return Some(addr);
    }
    find_module_base(pid, expr).ok()
}

/// Read exactly `N` bytes, or `None` on any short/failed read (CE returns nil).
fn read_n<const N: usize>(pid: Pid, addr: u64) -> Option<[u8; N]> {
    read_bytes(pid, addr, N).ok()?.try_into().ok()
}

/// Install the CE memory read/write family + `getAddress` as Lua globals.
fn register_memory(lua: &Lua, pid: Pid) -> LuaResult<()> {
    let g = lua.globals();

    // getAddress(operand) — number passthrough or module+offset resolution.
    g.set(
        "getAddress",
        lua.create_function(move |_, v: Value| Ok(resolve_address(pid, &v)))?,
    )?;
    // getAddressSafe is the same contract (nil on failure) in our model.
    g.set(
        "getAddressSafe",
        lua.create_function(move |_, v: Value| Ok(resolve_address(pid, &v)))?,
    )?;

    // --- reads: nil on failure, signed where CE is signed ---
    g.set(
        "readBytes",
        lua.create_function(move |_, (v, count): (Value, usize)| {
            let Some(addr) = resolve_address(pid, &v) else {
                return Ok(None);
            };
            Ok(read_bytes(pid, addr, count).ok().map(|b| b.to_vec()))
        })?,
    )?;
    register_read::<1>(lua, pid, "readByte", |b| i64::from(b[0] as i8))?;
    register_read::<2>(lua, pid, "readSmallInteger", |b| {
        i64::from(i16::from_le_bytes(b))
    })?;
    register_read::<4>(lua, pid, "readInteger", |b| {
        i64::from(i32::from_le_bytes(b))
    })?;
    register_read::<8>(lua, pid, "readQword", i64::from_le_bytes)?;
    register_read::<8>(lua, pid, "readPointer", |b| u64::from_le_bytes(b) as i64)?;
    register_read_f::<4>(lua, pid, "readFloat", |b| f64::from(f32::from_le_bytes(b)))?;
    register_read_f::<8>(lua, pid, "readDouble", f64::from_le_bytes)?;

    // --- writes: silently no-op on a bad address (CE-ish), report short writes ---
    register_write(lua, pid, "writeByte", |v: i64| vec![v as u8])?;
    register_write(lua, pid, "writeSmallInteger", |v: i64| {
        (v as i16).to_le_bytes().to_vec()
    })?;
    register_write(lua, pid, "writeInteger", |v: i64| {
        (v as i32).to_le_bytes().to_vec()
    })?;
    register_write(lua, pid, "writeQword", |v: i64| v.to_le_bytes().to_vec())?;
    register_write_f(lua, pid, "writeFloat", |v: f64| {
        (v as f32).to_le_bytes().to_vec()
    })?;
    register_write_f(lua, pid, "writeDouble", |v: f64| v.to_le_bytes().to_vec())?;
    g.set(
        "writeBytes",
        lua.create_function(move |_, (v, bytes): (Value, Variadic<i64>)| {
            let Some(addr) = resolve_address(pid, &v) else {
                return Ok(false);
            };
            let data: Vec<u8> = bytes.iter().map(|b| *b as u8).collect();
            Ok(write_bytes(pid, addr, &data).is_ok())
        })?,
    )?;

    Ok(())
}

fn register_read<const N: usize>(
    lua: &Lua,
    pid: Pid,
    name: &str,
    conv: fn([u8; N]) -> i64,
) -> LuaResult<()> {
    lua.globals().set(
        name,
        lua.create_function(move |_, v: Value| {
            let Some(addr) = resolve_address(pid, &v) else {
                return Ok(None);
            };
            Ok(read_n::<N>(pid, addr).map(conv))
        })?,
    )
}

fn register_read_f<const N: usize>(
    lua: &Lua,
    pid: Pid,
    name: &str,
    conv: fn([u8; N]) -> f64,
) -> LuaResult<()> {
    lua.globals().set(
        name,
        lua.create_function(move |_, v: Value| {
            let Some(addr) = resolve_address(pid, &v) else {
                return Ok(None);
            };
            Ok(read_n::<N>(pid, addr).map(conv))
        })?,
    )
}

fn register_write(lua: &Lua, pid: Pid, name: &str, enc: fn(i64) -> Vec<u8>) -> LuaResult<()> {
    lua.globals().set(
        name,
        lua.create_function(move |_, (v, val): (Value, i64)| {
            let Some(addr) = resolve_address(pid, &v) else {
                return Ok(false);
            };
            Ok(write_bytes(pid, addr, &enc(val)).is_ok())
        })?,
    )
}

fn register_write_f(lua: &Lua, pid: Pid, name: &str, enc: fn(f64) -> Vec<u8>) -> LuaResult<()> {
    lua.globals().set(
        name,
        lua.create_function(move |_, (v, val): (Value, f64)| {
            let Some(addr) = resolve_address(pid, &v) else {
                return Ok(false);
            };
            Ok(write_bytes(pid, addr, &enc(val)).is_ok())
        })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // process_vm_readv/writev can target our own pid, so the PoC drives real
    // cross-process primitives against a local buffer — no child spawn needed.
    fn self_pid() -> Pid {
        nix::unistd::getpid()
    }

    #[test]
    fn reads_integer_float_and_pointer_from_process_memory() {
        let cells: [u64; 3] = [0x1234_5678, f32::to_bits(1.5) as u64, 0xDEAD_BEEF];
        let base = cells.as_ptr() as u64;
        let rt = LuaRuntime::new(self_pid()).unwrap();

        let i: i64 = rt.eval(&format!("return readInteger({base})")).unwrap();
        assert_eq!(i, 0x1234_5678);

        let f: f64 = rt.eval(&format!("return readFloat({})", base + 8)).unwrap();
        assert_eq!(f, 1.5);

        let p: i64 = rt
            .eval(&format!("return readPointer({})", base + 16))
            .unwrap();
        assert_eq!(p as u64, 0xDEAD_BEEF);
    }

    #[test]
    fn writes_back_into_process_memory() {
        let mut cell: u64 = 0;
        let addr = (&mut cell as *mut u64) as u64;
        let rt = LuaRuntime::new(self_pid()).unwrap();

        rt.exec(&format!("writeInteger({addr}, 0x4242)")).unwrap();
        assert_eq!(cell as u32, 0x4242);

        rt.exec(&format!("writeFloat({addr}, 2.5)")).unwrap();
        assert_eq!(f32::from_bits(cell as u32), 2.5);
    }

    #[test]
    fn failed_read_returns_nil_not_error() {
        let rt = LuaRuntime::new(self_pid()).unwrap();
        // Page 0 is never mapped — CE returns nil, so must we (no Lua error).
        let v: Option<i64> = rt.eval("return readInteger(0)").unwrap();
        assert_eq!(v, None);
    }

    #[test]
    fn read_write_round_trip_through_lua_logic() {
        let mut cell: u64 = 100;
        let addr = (&mut cell as *mut u64) as u64;
        let rt = LuaRuntime::new(self_pid()).unwrap();
        // Exercises Lua arithmetic between a read and a write.
        rt.exec(&format!(
            "local v = readInteger({addr}); writeInteger({addr}, v * 2 + 1)"
        ))
        .unwrap();
        assert_eq!(cell as u32, 201);
    }
}
