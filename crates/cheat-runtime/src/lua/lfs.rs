//! A minimal native LuaFileSystem (`lfs`) — the subset framework tables use.
//!
//! CE bundles [LuaFileSystem](https://keplerproject.github.io/luafilesystem/) so
//! tables can probe and create their data/state directories. tatu ships no such
//! C module, so `lfs` is `nil` and the bootstrap crashes the moment a table
//! touches the filesystem. We implement the three functions Manifold uses —
//! `attributes`, `mkdir`, `dir` — over `std::fs`.
//!
//! Two deliberate divergences from upstream lfs, both toward robustness for
//! Windows-authored tables running on Linux:
//! * every path is normalised `\` → `/` first (tables hardcode `\\`);
//! * `mkdir` creates parents (`create_dir_all`) so a multi-level data dir
//!   succeeds in one call.

use std::fs;
use std::path::Path;

use mlua::{Lua, Result as LuaResult, Value};

/// Normalise a Windows-style path to a Linux one (`\` → `/`).
fn normalize(path: &str) -> String {
    path.replace('\\', "/")
}

/// Install the `lfs` global table.
pub(super) fn install(lua: &Lua) -> LuaResult<()> {
    let lfs = lua.create_table()?;

    // lfs.attributes(path[, request]) -> table | value | nil
    lfs.set(
        "attributes",
        lua.create_function(|lua, (path, request): (String, Option<Value>)| {
            let Ok(meta) = fs::metadata(normalize(&path)) else {
                return Ok(Value::Nil);
            };
            let mode = if meta.is_dir() {
                "directory"
            } else if meta.is_file() {
                "file"
            } else {
                "other"
            };
            // `lfs.attributes(path, "mode")` returns just that field.
            if let Some(Value::String(s)) = request {
                return match s.to_str()?.as_ref() {
                    "mode" => Ok(Value::String(lua.create_string(mode)?)),
                    "size" => Ok(Value::Integer(meta.len() as i64)),
                    _ => Ok(Value::Nil),
                };
            }
            let t = lua.create_table()?;
            t.set("mode", mode)?;
            t.set("size", meta.len() as i64)?;
            Ok(Value::Table(t))
        })?,
    )?;

    // lfs.mkdir(path) -> true | (nil, errorMessage). Parents included.
    lfs.set(
        "mkdir",
        lua.create_function(
            |lua, path: String| match fs::create_dir_all(normalize(&path)) {
                Ok(()) => Ok((Value::Boolean(true), Value::Nil)),
                Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
            },
        )?,
    )?;

    // lfs.dir(path) -> stateful iterator yielding "." , ".." then entry names.
    // An unreadable directory yields an empty iterator (upstream errors; we stay
    // lenient so a missing data dir doesn't abort a bootstrap loop).
    lfs.set(
        "dir",
        lua.create_function(|lua, path: String| {
            let mut entries = vec![".".to_string(), "..".to_string()];
            if let Ok(rd) = fs::read_dir(normalize(&path)) {
                for e in rd.flatten() {
                    if let Some(name) = Path::new(&e.file_name()).to_str() {
                        entries.push(name.to_string());
                    }
                }
            }
            let mut iter = entries.into_iter();
            lua.create_function_mut(move |_, ()| Ok(iter.next()))
        })?,
    )?;

    lua.globals().set("lfs", lfs)?;
    Ok(())
}
