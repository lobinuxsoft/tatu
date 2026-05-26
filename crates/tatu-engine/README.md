# tatu-engine

Backend-agnostic Cheat Engine Auto-Assembler engine in pure Rust:

- `parser` — line-based `.CT` `<AssemblerScript>` body → typed `Statement`s.
- `asm` — single-line x86_64 assembler (uses `iced-x86`) for the bytes the
  executor writes.
- `backend` — `Backend` trait + generic `Engine<B>` state machine; injected
  by either `cheat-runtime`'s Linux ptrace backend or `tatu-bridge`'s Win32
  backend.

This crate ships no I/O. The platform layer plugs into `Backend` and
`tatu_mem::MemoryAccess`; everything above (parser, assembler, executor)
compiles on Linux + Windows.

## AA command coverage

Tracked under issue #131. The matrix below is derived from an empirical
audit (`cargo run -p tatu-engine --example ct_audit`) of 17 `.CT` tables
pulled from FearLess Revolution (Crimson Desert, Dragon's Dogma 2, Elden
Ring, Ender Magnolia, Enigma of Fear, Enotria, Mass Effect: Andromeda,
Pragmata — variants for each game). 287 `<AssemblerScript>` blocks total.

### Supported

| Command | Notes |
|---|---|
| `aobscanmodule(symbol, scope, pattern)` | `??` wildcards; scope is informational (we always scan readable regions). |
| `registersymbol(name [name …])` | Comma-separated, whitespace-separated, or `(*)` wildcard all accepted. No-op in tatu (we have no implicit-symbol concept). |
| `unregistersymbol(name [name …])` | Same shapes as `registersymbol`. `(*)` is the dominant `[DISABLE]` idiom. |
| `label(name [name …])` | Local symbol declaration. |
| `alloc(symbol, size [, near])` | `near` triggers `MAP_FIXED_NOREPLACE` walk for `jmp rel32` reachability. |
| `globalalloc(symbol, size)` | Shape-equivalent to `alloc`. CE's "global" scope semantics do not apply — tatu owns lifetime via per-toggle rollback. |
| `dealloc(name [name …])` | Wildcard `(*)` releases every codecave the current script allocated. Unknown names are a lenient no-op (matches CE; the wild routinely cross-script-deallocs already-freed names). |
| `define(name, value)` | Numeric values bind eagerly into the symbol table; non-numeric values are deferred for the `asm` layer to resolve at compile time. |
| `<symbol>:` label sites | Symbolic and absolute (`0xADDR:`, `$ADDR:`, decimal). |
| `@@:` / `@f` / `@b` | Anonymous label CE-AA convention. Forward/backward resolved during parse. |
| `{$...}` compiler directives | Captured verbatim. `{$lua}` and `{$asm}` recognised structurally (see below). |
| inline x86_64 assembly | Via `iced-x86`. Supports `jmp rel32`, `mov`, `lea`, control flow, `(float)` immediates, `dword ptr`/`qword ptr` size hints. |
| `db`, `dq`, `dw`, `dd` | Byte / word / dword / qword literals inside label bodies. |
| `nop N` | Multi-byte NOP fill. |
| `readmem(addr, n)` pseudo | Inside `db` bodies — copies live bytes from the target into the label. |

### Not supported (and how tatu handles them)

| Command / shape | Frequency | Behaviour |
|---|---|---|
| `{$lua}` block in `[ENABLE]` | 28 / 287 scripts | Script marked `lua_only`; executor returns `ExecError::LuaNotSupported`. The UI surfaces "Lua scripting not supported" so the surrounding table keeps working. |
| Pure-Lua script (no `[ENABLE]`) | 6 / 287 | Same handling — `Script { lua_only: true, … }` instead of `MissingEnable`. |
| `createthread`, `luacall`, `createtimer` | 11 / 287 (combined) | Almost exclusively inside `{$lua}` — covered transitively by the lua-only path. |
| `writeBytes` / `writeByte` / `writeInteger` | 14 / 287 | Same — inside `{$lua}` bodies. Direct top-level top-level usage falls through to `Statement::Raw` and would error at execute time with `Unsupported`. |
| `pause`, `unpause`, `error`, `sleep`, `openprocess`, `getaddresslist`, `getdissectcode`, `writetoclipboard`, `executecode`, `autoassemble`, `reassemble`, `launchmonodatacollector`, `mono_initialize`, `fullaccess` | 1–2 each | CE introspection / control-flow helpers. All observed inside `{$lua}` blocks; lua-only path covers them. Top-level usage falls through as `Raw` and would error. |
| `speedhack_setspeed` | 2 / 287 | Phase E feature, deferred under #135 (`cheat-runtime-extension` in-process `.so`). |
| `loadlibrary`, `loadbinary` | 0 / 287 | Listed in the issue but not observed in the audited corpus; will add when a real `.CT` needs them. |

### Re-running the audit

```
cargo run -p tatu-engine --example ct_audit -- /path/to/ct-corpus
```

The example expects a directory of `.ct` (XML) files. Zipped CT files
should be unzipped first (`unzip -p foo.ct > foo_unpacked.ct`). The output
includes a per-file pass/fail summary plus the full call-frequency table.

Update this README when:

- A new `.CT` corpus surfaces a top-level use of a command currently
  marked as "covered transitively via lua".
- A "not supported" command gets implemented (move the row up and document
  the executor handler).
- `Phase E` lands and `speedhack_setspeed` becomes runnable.
