//! Assorted CE globals a framework table touches at load/runtime.
//!
//! Two kinds live here:
//! * **Pure helpers** implemented for real (`extractFileNameWithoutExt`, the
//!   byte/float conversions) — cheap, deterministic, no host state.
//! * **Inert stubs** with sensible fixed answers for primitives whose real
//!   behaviour is GUI- or host-bound and out of scope for the headless runtime
//!   (`cheatEngineIs64Bit`, `getScreenDPI`, clipboard, custom-type registration,
//!   …). They return plausible values so the bootstrap and a self-contained
//!   cheat don't nil-crash; faithful custom types / clipboard land later.
//!
//! `findTableFile`/`readStringLocal` are deliberately *not* here — they depend
//! on the embedded files of a specific `.CT`, so the table loader injects them.

use mlua::{Lua, Result as LuaResult};

const PRELUDE: &str = r#"
-- Pure helpers -------------------------------------------------------------

-- Strip directory and extension: "a/b/Foo.lua" -> "Foo".
function extractFileNameWithoutExt(path)
  if type(path) ~= "string" then return path end
  return (path:gsub("%.%w+$", ""):gsub("^.*[\\/]", ""))
end

-- 4-byte little-endian table <-> float, via Lua 5.4 string.pack.
function byteTableToFloat(t)
  if type(t) ~= "table" or #t < 4 then return nil end
  return string.unpack("<f", string.char(t[1], t[2], t[3], t[4]))
end
function floatToByteTable(f)
  return { string.byte(string.pack("<f", f), 1, 4) }
end

-- Host/identity facts the table branches on.
function cheatEngineIs64Bit() return true end
function getScreenDPI() return 96 end
function getLuaEngine() return nil end

-- Timing. getTickCount is a millisecond clock; sleep is a no-op so a table's
-- busy-wait can't stall the headless runtime.
function getTickCount() return math.floor(os.clock() * 1000) end
function sleep() end

-- GUI / addresslist / clipboard primitives with no headless effect.
function setMethodProperty() end
function component_getComponent() return nil end
function memoryrecord_delete() end
function writeToClipboard() end
function ShellExecute() end

-- Custom value types: registration is accepted but inert (faithful decode is a
-- later phase). Returns an empty descriptor so callers can store it safely.
function registerCustomTypeLua() return {} end
function registerCustomType() return {} end

-- Misc utilities.
function md5file() return "" end
"#;

/// Install the assorted CE globals.
pub(super) fn install(lua: &Lua) -> LuaResult<()> {
    lua.load(PRELUDE).set_name("@tatu:ce_misc").exec()
}
