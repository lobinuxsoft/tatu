# cheat-runtime

Linux-native Cheat Engine Auto-Assembler runtime in pure Rust. Out-of-process
memory access via `process_vm_readv` / `process_vm_writev`, AOB scanner with
SIMD fast path, full CE Auto-Assembler parser, atomic executor with rollback,
and a manifest format that binds user-facing features to scripts.

## What it does

```
              ┌───────────────┐
manifest.json │   manifest    │   self-describing JSON binding
              │  (loader)     │   features → CE Auto-Assembler scripts
              └──────┬────────┘
                     │
              ┌──────▼────────┐
              │    parser     │   [ENABLE]/[DISABLE], aobscanmodule,
              │   (parser.rs) │   registersymbol, label sites, db/dq/...
              └──────┬────────┘
                     │
              ┌──────▼────────┐    ┌─────────────┐
              │   executor    │───▶│  scanner    │   memchr SIMD + mask
              │ (executor.rs) │    │ (scanner.rs)│
              └──────┬────────┘    └──────┬──────┘
                     │                    │
                     │  process_vm_*v     │
                     ▼                    ▼
                 ┌─────────────────────────────┐
                 │  target Linux PID (Proton)  │
                 └─────────────────────────────┘
```

The target can be **any Linux PID** the calling UID can access, including
Steam Proton games — the kernel sees them as ordinary Linux processes
regardless of the Wine layer inside.

## Status

Issue [#64](https://github.com/lobinuxsoft/game-progress-tracker/issues/64)
tracks the crate. Subtasks 1–6 shipped the core pipeline; subtask 7 deprecated
the predecessor `cheat-core`; subtask 8 (this PR) sealed the public API for
external reuse.

What it covers today:

- ✅ Memory R/W via `process_vm_readv` / `process_vm_writev`
- ✅ `/proc/<pid>/maps` parsing into typed `MemoryRegion`s
- ✅ AOB scanner with `??` wildcards (memchr SIMD, ~1.1 GiB/s on Ryzen 9800X3D)
- ✅ CE Auto-Assembler parser (line-based, lossless, ~10 typed statement kinds)
- ✅ Executor with atomic enable/disable rollback
- ✅ Manifest loader (`~/.config/backlog-tracker/trainers/<app_id>/*.json`)
- ✅ PID lookup by exe name (handles 15-byte `comm` truncation + Wine paths)
- ✅ Aurora SD Tool / CheatHappens JSON loader (shape-based, no obfuscation map)

What's still **out of scope** (issue #64 documents it):

- ❌ `alloc` / `dealloc` (needs ptrace-mediated remote `mmap`)
- ❌ Inline assembly (`push`, `mov`, `jmp`, …) — needs `iced-x86`
- ❌ Code-injection hooks (overwriting game code with `jmp` to allocated page)
- ❌ Windows port (the abstraction is in place; no impl yet)
- ❌ Aurora feature ↔ script binding (the binding inside Aurora payloads
  is undocumented; the loader exposes both as parallel lists until reverse-eng
  resolves it)

## Using it from another Rust crate

Add as a path or git dependency:

```toml
[dependencies]
cheat-runtime = { path = "../crates/cheat-runtime" }
# or
cheat-runtime = { git = "https://github.com/lobinuxsoft/game-progress-tracker", branch = "development" }
```

Minimal end-to-end usage:

```rust
use cheat_runtime::{Engine, find_pid_by_exe, load_manifests_for, parse_script};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_id = "2725260"; // Ender Magnolia
    let manifests = load_manifests_for(app_id)?;
    let manifest = manifests.into_iter().next().expect("no manifest");

    let pid = find_pid_by_exe(&manifest.exe).expect("game not running");
    let mut engine = Engine::new(pid);

    let feature = &manifest.features[0]; // pick a feature
    let script = parse_script(&feature.script)?;
    let active = engine.enable(&script)?;

    // ... feature is now applied ...

    active.disable()?;
    Ok(())
}
```

The full public surface lives at the crate root (`pub use` from `lib.rs`):

| Module | Key public items |
|---|---|
| `aurora` | `Trainer`, `Feature`, `load_trainer`, `load_trainer_file` |
| `executor` | `Engine`, `ActiveCheat`, `ExecError` |
| `manifest` | `Manifest`, `ManifestFeature`, `load_manifests_for` |
| `maps` | `MemoryRegion`, `Perms`, `read_maps`, `parse_maps` |
| `memory` | `read_bytes`, `write_bytes`, `RuntimeError` |
| `parser` | `Script`, `Statement`, `parse_script` |
| `process` | `find_pid_by_exe`, `find_pids_by_exe` |
| `scanner` | `Pattern`, `scan`, `scan_in_process` |

## Using it from Decky (Python FFI)

The crate is built with `crate-type = ["rlib", "cdylib"]`, so a release build
emits `libcheat_runtime.so`. There are two practical FFI paths:

### Option 1 — PyO3 wrapper (recommended)

Create a sibling **separate repo** `decky-cheat-runtime` that adds a thin
`#[pymodule]` over `cheat-runtime`. Skeleton:

```toml
# decky-cheat-runtime/Cargo.toml
[package]
name = "decky_cheat_runtime"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
cheat-runtime = { git = "https://github.com/lobinuxsoft/game-progress-tracker" }
pyo3 = { version = "0.22", features = ["extension-module"] }
```

```rust
// decky-cheat-runtime/src/lib.rs
use pyo3::prelude::*;
use cheat_runtime::{Engine, find_pid_by_exe, load_manifests_for, parse_script};

#[pyfunction]
fn enable_feature(app_id: &str, uuid: &str) -> PyResult<()> {
    let manifests = load_manifests_for(app_id)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    // ... find feature, parse script, engine.enable() ...
    Ok(())
}

#[pymodule]
fn decky_cheat_runtime(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(enable_feature, m)?)?;
    Ok(())
}
```

Build with `maturin develop` or `maturin build --release` and copy the
resulting `.so` into the Decky plugin's Python `plugin/` directory. Decky's
Python backend imports it like any module.

### Option 2 — C ABI + ctypes

If PyO3 is overkill (for a tiny Decky plugin it usually is), add a small
`extern "C"` shim in a sibling crate, expose a handful of stable C functions,
and call them from Python via `ctypes.CDLL("libcheat_runtime.so")`. Build the
crate with `cargo build --release --lib` and ship the `.so` directly.

## Stability promise

The crate is `version = "0.1.0"` and behind `publish = false`. Public items
will continue to change while issue #64's follow-up subtasks land (alloc/asm
support, Aurora binding, Windows port). Pin to a specific git commit in
downstream consumers until a `0.2` cut is tagged.

## Testing

```
cargo test -p cheat-runtime
```

69 unit + integration tests, including a 100 MiB scanner benchmark, a real
Aurora EM trainer fixture, an end-to-end enable/disable roundtrip, and a
rollback-on-failure test that verifies atomicity.

## Personal-memory cross-refs

- `project_aurora_reverse` — CheatHappens Aurora reverse-engineering session
  that informed this crate's scope.
- `feedback_rust_best_practices`, `feedback_rust_dod_mandatory` — the style
  this crate is held to.
