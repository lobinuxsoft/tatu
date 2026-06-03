//! Windows→Linux compatibility shims for framework tables.
//!
//! Framework tables are written against CE-on-Windows: they read Windows
//! environment variables (`USERPROFILE`, `APPDATA`, …) to locate a data
//! directory for state/log files. Running natively on Linux those are unset, so
//! `os.getenv` returns `nil` and the bootstrap crashes on the first
//! concatenation.
//!
//! We wrap `os.getenv` to fall back to `$HOME`-based equivalents for the common
//! Windows variables. Path-separator normalisation (the `\\` in the resulting
//! paths) is a deeper port concern handled later; for now the directory is
//! created with a quirky name but nothing crashes.

use mlua::{Lua, Result as LuaResult};

/// Wrap `os.getenv` so unset Windows variables resolve to `$HOME`-based paths.
const PRELUDE: &str = r#"
local _getenv = os.getenv
local home = _getenv("HOME") or "/tmp"
local base = home .. "/.local/share/tatu"
local winmap = {
  USERPROFILE  = base,
  APPDATA      = base,
  LOCALAPPDATA = base,
  HOMEDRIVE    = "",
  HOMEPATH     = home,
  TEMP         = "/tmp",
  TMP          = "/tmp",
}
os.getenv = function(name)
  local v = _getenv(name)
  if v ~= nil then return v end
  return winmap[name]
end

-- CE threading model. tatu's runtime is single-threaded, so "are we on the
-- main thread?" is always true and synchronize runs the closure inline.
inMainThread = function() return true end
synchronize = function(a, b, ...)
  if type(a) == "function" then return a(b, ...) end
  if type(b) == "function" then return b(...) end
end
checkSynchronize = function() end
"#;

/// Install the compatibility shims (requires `os` to be loaded already).
pub(super) fn install(lua: &Lua) -> LuaResult<()> {
    lua.load(PRELUDE).set_name("@tatu:compat").exec()
}
