# Tatu

<div align="center">
  <strong>Steam backlog tracker with a native Linux cheat runtime.</strong>

  [![License](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
  [![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)](https://www.rust-lang.org/)
  [![Tauri](https://img.shields.io/badge/Tauri-v2-FFC131?logo=tauri)](https://tauri.app/)
</div>

> **Tatu** = *armadillo* in Guaraní. Sibling project to [Yryvu](https://github.com/lobinuxsoft/yryvu) (vulture, a Git client).

## Overview

Tatu is a desktop application for tracking what is in your Steam backlog and applying single-player cheats to games you own. The cheat layer is a clean-room re-implementation of the CheatEngine Auto-Assembler engine driving a native Linux `ptrace` backend — a Proton game is an ordinary Linux process from the kernel's point of view, so no Wine-side component is involved.

### What Tatu is

- A **backlog tracker** for the games you own on Steam (progress, achievements, trading cards, DRM, size on disk).
- A **single-player cheat host** that applies CheatEngine-style memory patches to a process you launched yourself.

### What Tatu is NOT

- **NOT a piracy tool.** Tatu does not crack DRM, distribute games, or interact with multiplayer state. It only operates on local processes you launched yourself.
- **NOT a multiplayer cheat.** Online games with anti-cheat (EAC / BattlEye / Vanguard) are explicitly out of scope — Tatu detects and refuses, no bypass attempts.
- **NOT a Cheat Engine fork.** The engine is a clean re-implementation that consumes `.CT` tables, not a wrapper around `cheatengine.exe`.

### Key Features

- **Steam library integration** — owned games, install state, play time, achievements, trading cards, DRM classification and size on disk.
- **`.CT` import** — parses existing Cheat Engine tables and surfaces their toggles in the tracker UI.
- **Native Linux cheat runtime** — `cheat-runtime` drives `process_vm_readv` / `process_vm_writev` and `ptrace`, against native ELF games and Proton games alike.
- **Orphan-hook recovery** — persistent undo log of every code patch, so an interrupted session can be rolled back cleanly.

## Platform support

| | Linux | Windows |
|---|---|---|
| Backlog tracker | ✅ | ✅ |
| Cheats | ✅ | ❌ hidden |

The tracker is portable. The cheat runtime is not: it is built on `ptrace`, `process_vm_readv`, `/proc/<pid>/maps` and ELF symbol lookup, none of which exist on Windows.

The engine above that backend already does compile for Windows — `tatu-mem` (the `MemoryAccess` trait, AOB scan, pointer-chain walk, CE address expressions) and `tatu-engine` (Auto-Assembler parser, x86_64 assembler, executor) are OS-agnostic by construction. What is missing is the Win32 sibling of the Linux backend, tracked in [#181](https://github.com/lobinuxsoft/tatu/issues/181).

Until it lands, the Windows build does not register the cheat commands at all and the Cheats tab is not rendered.

## Architecture

```
┌──────────────────────────────┐
│        tatu-tracker          │   Tauri 2 + vanilla ES modules
│  Steam library · .CT import  │
│  cheat toggles · undo log    │
└──────────────┬───────────────┘
               │
        ┌──────▼───────┐
        │ tatu-engine  │   CE Auto-Assembler parser + x86_64 assembler
        └──────┬───────┘   + executor.  OS-agnostic.
               │
        ┌──────▼───────┐
        │  tatu-mem    │   MemoryAccess trait, AOB scan, pointer chains,
        └──────┬───────┘   CE address expressions.  OS-agnostic.
               │
   ┌───────────┴────────────┐
   │                        │
┌──▼─────────────┐   ┌──────▼──────────┐
│ cheat-runtime  │   │   tatu-win      │   #181, not written yet
│ ptrace backend │   │ Win32 backend   │
└────────────────┘   └─────────────────┘
```

| Component | Role |
|-----------|------|
| **tatu-tracker** | Desktop app (Tauri 2, `src-tauri/`). Steam library view, `.CT` import, per-game cheat toggles, orphan recovery UI. The frontend under `frontend/` is plain ES modules — no bundler, no framework. |
| **tatu-mem** | Backend-agnostic memory primitives: the `MemoryAccess` trait plus the pure logic built on it — AOB pattern scan, pointer-chain walk, typed read/write, CE address-expression parser. |
| **tatu-engine** | Backend-agnostic CE Auto-Assembler engine: script parser, x86_64 assembler (`iced-x86`), and the executor state machine. |
| **cheat-runtime** | The Linux backend. `process_vm_readv`/`writev`, `ptrace` attach and POKEDATA, region enumeration from `/proc/<pid>/maps`, ELF symbol lookup, codecave allocator, freeze worker, orphan-hook persistence. |
| **cheat-mono-collector** | Proxy DLL dropped next to a Unity game so Proton loads it; reports Mono/IL2CPP class and field offsets back over a loopback socket. |
| **ce-launcher** | Installs and launches [Cheat Engine](https://www.cheatengine.org/) for Linux — useful for authoring the `.CT` tables Tatu then consumes. |

## Status

Tatu is **early-stage and not yet production-ready**. The tracker is usable day to day; the cheat runtime works but coverage varies by game and engine.

The Wine-side bridge (`tatu-bridge`, `tatu-launcher`, `tatu-proto`) described in earlier revisions of this file was removed in [#128](https://github.com/lobinuxsoft/tatu/issues/128) — a Proton game is a normal Linux process, so the whole Win32 detour bought nothing that `ptrace` did not already give.

## Building from source

### Requirements

- Rust stable: <https://rustup.rs>
- Linux only: WebKitGTK and GTK3 development headers (see below). The frontend needs no toolchain — it is plain ES modules served straight out of `frontend/`.

### Platform dependencies

| Platform | Dependencies |
|----------|--------------|
| Bazzite / Fedora Atomic | `rpm-ostree install webkit2gtk4.1-devel gtk3-devel` |
| Ubuntu / Debian | `apt install libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config build-essential` |
| Arch | `pacman -S webkit2gtk-4.1 gtk3 pkgconf base-devel` |
| Windows | None beyond the MSVC toolchain. WebView2 ships with Windows 10/11. |

### Build

```sh
git clone https://github.com/lobinuxsoft/tatu
cd tatu

cargo build --release -p tatu-tracker   # → target/release/tatu-tracker
cargo test --workspace
```

`target/` lives at the workspace root, not under `src-tauri/`.

To produce the Linux AppImage locally:

```sh
./build_appimage.sh                     # → dist/appimage/Tatu_<version>_x86_64.AppImage
```

## Configuration

Everything lives under the platform config dir — `$XDG_CONFIG_HOME` (`~/.config`) on Linux, `%APPDATA%` on Windows — in a `backlog-tracker/` subtree kept from the pre-rename days:

| Path | Contents |
|---|---|
| `backlog-tracker/state.json` | Library, completion flags, API key, caches |
| `backlog-tracker/cheat-tables/<app_id>/` | Imported `.CT` files (Linux only) |
| `backlog-tracker/trainers/<app_id>/` | Parsed cheat manifests (Linux only) |
| `backlog-tracker/active-hooks/` | Undo log of live code patches (Linux only) |

## Versioning

Tatu uses [SemVer](https://semver.org/). Releases are managed by [release-please](https://github.com/googleapis/release-please) and triggered by [Conventional Commits](https://www.conventionalcommits.org/) on `main`.

## Project structure

```
tatu/
├── Cargo.toml                          # Workspace root
├── src-tauri/                          # tatu-tracker (Tauri 2 backend + commands)
├── frontend/                           # Plain ES modules, no bundler
├── crates/
│   ├── tatu-mem/                       # MemoryAccess trait + backend-agnostic primitives
│   ├── tatu-engine/                    # CE Auto-Assembler parser, assembler, executor
│   ├── cheat-runtime/                  # Linux ptrace backend
│   ├── cheat-mono-collector/           # Unity Mono/IL2CPP offset collector (proxy DLL)
│   └── ce-launcher/                    # Cheat Engine for Linux installer/launcher
└── build_appimage.sh                   # Local AppImage packaging
```

## Contributing

1. Fork the repository
2. Create a feature branch from `development`
3. Make your changes
4. Submit a PR to `development`

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

## License

AGPL v3 — see [LICENSE](LICENSE) for details.

This means:
- Free to use, modify, and distribute
- Contributions welcome
- Derivatives must use the same license
- Source code must be provided (even for SaaS)
- Original authors must be credited

## Disclaimer

Tatu is a personal-use single-player cheat host plus backlog tracker. It is **not intended for use against multiplayer games, anti-cheat-protected games, or any service where modifying client state could harm other players or violate a publisher's Terms of Service**. The author assumes no responsibility for misuse. Users are solely responsible for complying with applicable laws, EULAs, and platform Terms of Service in their jurisdiction.

## Support

If you find Tatu useful, consider supporting development:

- **BTC**: `bc1qkxy898wa6mz04c9hrjekx6p0yht2ukz56e9xxq`
- **USDT (TRC20)**: `TF6AXBP3LKBCcbJkLG6RqyMsrPNs2JCpdQ`
- **USDT (BEP20)**: `0xd8d2Ed67C567CB3Af437f4638d3531e560575A20`
- **Binance Pay**: `78328894`

## Credits

- Built with [Tauri](https://tauri.app/)
- Cheat backend pattern inspired by [Aurora](https://www.cheathappens.com/) (clean-room re-implementation)
- Cheat Engine integration via the `.CT` table format
- Steam compatibility tool patterned after [Luxtorpeda](https://github.com/luxtorpeda-dev/luxtorpeda)
