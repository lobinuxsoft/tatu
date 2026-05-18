# cheat-core (DEPRECATED)

> This crate is **deprecated**. Use [`cheat-runtime`](../cheat-runtime/) for new
> work. `cheat-core` stays in the workspace temporarily so existing user data
> at `~/.config/backlog-tracker/cheats/<appid>.json` continues to work until
> the migration tool ships.

## Why it's deprecated

`cheat-core` was written before the project had a clear picture of what
serving the cheat-engineering use case from Linux requires. It covers only
two `.CT`-style value semantics — **Static** addresses and **PointerChain**
dereferences — and has its own freeze loop that re-applies a value on a
timer.

`cheat-runtime` (introduced in issue #64) is a full Rust port of the CE
Auto-Assembler dialect: AOB scanner, parser, executor with atomic
enable/disable, manifest format with explicit feature → script binding, and
a Tauri integration that already lives in the panel under "Trainer
features". It is a strict superset of `cheat-core`:

| Capability | cheat-core | cheat-runtime |
|---|---|---|
| Static address write | ✓ (JSON `static` action) | ✓ (manifest: `aobscanmodule` + `db ...`) |
| Pointer-chain write | ✓ (JSON `pointer_chain`) | ✓ (script `readmem(...)` + offset arithmetic) |
| CE Auto-Assembler scripts | ✗ | ✓ |
| Atomic enable/disable rollback | partial | ✓ |
| Freeze (continuous re-apply) | ✓ (registry + thread per cheat) | ✗ — to be ported in subtask 7-B |
| AOB scan | ✗ | ✓ (memchr SIMD, multi-pattern, mask-aware) |
| Manifest format | per-game JSON, one schema | self-describing JSON, scales beyond static/pointer |

## Migration plan

Tracked under issue #64 subtask 7:

- **7-A (this PR)**: doc-level deprecation. Crate stays compiling, Tauri
  commands stay wired, legacy panel section stays visible (now tagged
  "deprecated" so users see the warning). No data loss possible.
- **7-B (next PR)**: ship a `cheat_core_to_manifest` converter that reads
  `cheats/<appid>.json` and emits `trainers/<appid>/legacy.json` in the
  manifest format. Migrate the user's existing data in-place. Then:
  - Remove `cheat-core` from the workspace.
  - Delete `commands/cheat_cmd.rs`.
  - Remove the legacy panel section from the frontend.
  - Port the freeze loop (continuous re-apply) into `cheat-runtime::freeze`
    so trainers that rely on it keep working.

## What it covers today (during the deprecation window)

The Tauri commands `cheat_list`, `cheat_trigger`, `cheat_status`,
`cheat_freeze_toggle`, `cheat_freeze_status` still call into this crate.
They will emit a `[deprecated]` log line at each invocation so it's
observable when the user (or the panel) still depends on the legacy path.

No new features will be added here. Bug fixes are accepted only if the
migration to `cheat-runtime` is blocked on them.
