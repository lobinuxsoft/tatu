# `.CT` regression fixtures

Synthetic Cheat Engine table fixtures driving `tests/curated_tables.rs`.
Each file exercises one parser/executor surface; together they form the
licensing-clean baseline that catches regressions before they hit a real
`.CT`.

## Current coverage

| File | Surface |
|---|---|
| `01_basic_toggle.ct` | Happy path: `aobscanmodule` + `alloc(.., near)` + label site + `db` restore in DISABLE. |
| `02_wildcard_cleanup.ct` | `unregistersymbol(*)` + `dealloc(*)` wildcards in DISABLE. Pre-#131 these raised `BadCall`. |
| `03_multi_arg_lists.ct` | `label(a b c d e)` space-separated + `registersymbol(a,b,c)` comma-separated. Pre-#131 the space-separated form raised `BadCall(label)`. |
| `04_globalalloc_and_define.ct` | `globalalloc(symbol, size)` + `define(name, value)` with both numeric and module-relative-offset values. |
| `05_lua_only.ct` | Pure `{$lua}` payload, no `[ENABLE]` block. Must surface `Script::lua_only = true`, NOT `MissingEnable`. |
| `06_mixed_lua_asm.ct` | `{$lua}` block inside `[ENABLE]` + `{$asm}` divider. Forces `lua_only = true`. |

## Adding a new fixture

1. **Pick a surface, not a game**. Each fixture should isolate one
   parser/executor contract — copying a 50-entry real `.CT` makes the
   suite slow and noisy. If a real `.CT` from the FearLess audit
   exposes a regression, shrink the offending entry to the minimal
   reproducer.
2. **Stay licensing-clean**. Authored-here AA scripts only. Do not
   commit `.CT` content lifted from FearLess, Aurora, or other
   community sources. For corpus-wide validation use the
   `TATU_CT_CORPUS` env-var path (see `curated_tables.rs`).
3. **Add a matching `#[test]` in `tests/curated_tables.rs`** that
   asserts the contract the fixture is meant to exercise — not just
   "parses without error". A test that only checks `parse(...).is_ok()`
   regresses silently when the parser starts producing the wrong
   `Statement` variant.
4. **Keep XML minimal**. One `<CheatEntry>` per fixture, only the fields
   the parser cares about (`<AssemblerScript>`, sometimes
   `<VariableType>`). Skip `<LastState>`, `<Color>`, etc.

## Running

```bash
# Just the fixtures (CI default)
cargo test -p cheat-runtime --test curated_tables

# Plus the external corpus walker (developer-only)
TATU_CT_CORPUS=/path/to/.ct/dir \
  cargo test -p cheat-runtime --test curated_tables -- --ignored --nocapture
```

The walker asserts ≥ 99% parse rate. Against the FearLess audit
(`/tmp/ct-audit`, 17 tables / 287 scripts) the post-#131 parser hits
100% — any drop indicates a regression.
