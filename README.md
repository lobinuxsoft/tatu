# Tatu

<div align="center">
  <strong>Steam backlog tracker with an Aurora-style cheat backend under Proton/Wine.</strong>

  [![License](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
  [![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)](https://www.rust-lang.org/)
  [![Tauri](https://img.shields.io/badge/Tauri-v2-FFC131?logo=tauri)](https://tauri.app/)
  [![SolidJS](https://img.shields.io/badge/SolidJS-1.x-2C4F7C?logo=solid)](https://www.solidjs.com/)
</div>

> **Tatu** = *armadillo* in Guaraní. Sibling project to [Yryvu](https://github.com/lobinuxsoft/yryvu) (vulture, a Git client).

## Overview

Tatu is a desktop application for tracking what is in your Steam backlog and applying single-player cheats to games you own. The cheat layer is a clean-room re-implementation of the in-process pattern used by Aurora-style trainers, ported to a Linux host through a Win32 bridge running under Proton + Steam Linux Runtime.

### What Tatu is

- A **backlog tracker** for the games you own on Steam (progress, time invested, notes).
- A **single-player cheat host** that applies CheatEngine-style memory patches via a Win32 worker running inside the same Proton container as the game.
- A **Steam compatibility tool** (`tatu-launcher`) so the bridge handoff is reachable from the Steam UI — pick *Properties → Compatibility → Tatu Launcher* and the rest is transparent.

### What Tatu is NOT

- **NOT a piracy tool.** Tatu does not crack DRM, distribute games, or interact with multiplayer state. It only operates on local processes you launched yourself.
- **NOT a multiplayer cheat.** Online games with anti-cheat (EAC / BattlEye / Vanguard) are explicitly out of scope — Tatu detects and refuses, no bypass attempts.
- **NOT a Cheat Engine fork.** The cheat-runtime crate is a clean re-implementation that consumes `.CT` tables, not a wrapper around `cheatengine.exe`.

### Key Features

- **Steam library integration** — reads owned games, install state, and play time.
- **`.CT` import** — opens existing Cheat Engine tables and surfaces toggles in the tracker UI.
- **Aurora-style cheat backend** — `tatu-bridge.exe` runs as a Win32 worker inside the same Proton invocation as the game (shared SLR container + wineserver), so `OpenProcess` / `VirtualAllocEx` / `WriteProcessMemory` / `ReadProcessMemory` all see the game's PID natively.
- **Linux ptrace fallback** — `cheat-runtime` keeps a ptrace-based backend for native Linux games (no bridge needed).
- **Steam compat tool packaging** — `tatu-launcher` is a drop-in for `~/.steam/root/compatibilitytools.d/` that delegates to the user's real Proton (Experimental / GE / etc.); the per-game Proton picker stays meaningful.
- **Orphan-hook recovery** — persistent undo log of every code patch so an interrupted session can be rolled back cleanly.

## Architecture

```
┌────────────────────────────────┐         IPC (named pipe)         ┌──────────────────────────────┐
│        Tatu Tracker            │◄────────────────────────────────►│      tatu-bridge (Win32)     │
│   (Linux Tauri + SolidJS)      │                                  │   inside one Proton invoke   │
│                                │                                  │   ┌────────────────────────┐ │
│ - Steam library                │                                  │   │     game.exe           │ │
│ - .CT parser                   │                                  │   │  (CreateProcess child) │ │
│ - Cheat UI + activations       │                                  │   └────────────────────────┘ │
│ - Persistent undo log          │                                  │ OpenProcess + memory ops     │
└────────────────────────────────┘                                  └──────────────────────────────┘
            ▲
            │ Linux ptrace backend (fallback for native games)
            ▼
   /proc/<pid>/mem via cheat-runtime
```

| Component | Role |
|-----------|------|
| **tatu-tracker** | Desktop app (Tauri + SolidJS). Steam library view, `.CT` import, per-game cheat toggles, orphan recovery UI. |
| **tatu-bridge** | Win32 binary (cross-compiled with mingw-w64). Two modes: `--launch` (bootstrap — CreateProcess `self --connect` and the real game.exe inside one Proton invocation) and `--connect` (the cheat worker — talks Win32 APIs against the game). |
| **tatu-launcher** | Linux ELF. Steam compatibility tool that swaps `<game.exe>` for `<tatu-bridge.exe --launch game.exe>` on opted-in appids; passthrough on every other verb / unopted game so the per-game Proton picker stays intact. |
| **tatu-proto** | Wire types shared by tracker and bridge. Bincode 2 + Serde. |
| **cheat-runtime** | Pure-logic engine (`.CT` parser, AOB scanner, code patcher, codecave allocator, pointer-chain walker). Linux ptrace backend, will be shared with the Win32 bridge via a `MemoryAccess` trait. |
| **ce-launcher** | Linux launcher for [Cheat Engine](https://www.cheatengine.org/) running under its own Wine prefix — useful for editing `.CT` tables that Tatu then consumes. |

## Status

Tatu is **early-stage and not yet production-ready**. The Win32 bridge has been smoke-validated end-to-end (1000/1000 round-trips, 0 variance, 42 µs avg under Ender Magnolia + Proton Experimental), but a full Steam UI flow is still in flight (see [#106](https://github.com/lobinuxsoft/tatu/issues/106)).

No public releases have been cut yet — the version listed in `Cargo.toml` reflects pre-release work.

## Backend selection

Tatu ships two cheat backends; the tracker picks one per game. The bridge under Wine is the **preferred** path for every Windows game and the only path that handles modern engines correctly; the Linux ptrace runtime is a **fallback** kept alive for native Linux titles where the bridge cannot apply.

| Game runs as… | Preferred backend | Why |
|---|---|---|
| Windows binary under Proton/Wine | **`tatu-bridge` (Bridge)** | `OpenProcess` / `VirtualAllocEx` / `WriteProcessMemory` are native Win32 calls under Wine; cross-process `WriteProcessMemory` plus `SuspendThread` + `FlushInstructionCache` is the only safe way to patch `.text` on Win64 (kernel auto-lifts protection but the i-cache stays stale, see PR [#121](https://github.com/lobinuxsoft/tatu/pull/121)). |
| Linux native ELF | `cheat-runtime` (Linux ptrace) | No wineprefix exists; the bridge has nothing to attach to. `process_vm_writev` + `PTRACE_ATTACH` is the only available primitive. |
| Anti-cheat (EAC / BattlEye / Vanguard) | **Refused** | Out of scope (see "What Tatu is NOT"). Tracker detects and refuses both backends. |

The toggle is per-game and lives in the cheats panel banner — *Switch to Tatu* installs the compat tool drop-in idempotently, patches Steam's `CompatToolMapping`, and persists the bridge choice in the tracker state. *Revert to Linux* flips the routing only; `config.vdf` stays at whatever the user last set so a manual Proton-GE override survives toggling cheats off.

**Why the bridge wins for Proton games.** `process_vm_writev` over an emulated Win64 address space lands at file-mapped pages whose protection bits Wine has not synchronised with the kernel's actual mapping; cross-process patches succeed silently and corrupt the i-cache. Doing the patch from a Win32 worker inside the same Proton invocation hits `WriteProcessMemory`'s atomic `SuspendThread` + `VirtualProtect` + `FlushInstructionCache` cycle that the platform actually guarantees. For Unreal Engine titles in particular, anything else risks `FMallocBinned2` canary mismatches after a few seconds of combat.

## Building from source

### Requirements

- Rust stable: <https://rustup.rs>
- Bun: <https://bun.sh>
- mingw-w64 cross toolchain (for `tatu-bridge.exe`)

### Platform dependencies

| Platform | Dependencies |
|----------|--------------|
| Bazzite / Fedora Atomic | `rpm-ostree install mingw64-gcc mingw64-gcc-c++ mingw64-winpthreads-static webkit2gtk4.1-devel gtk3-devel` |
| Ubuntu / Debian | `apt install gcc-mingw-w64-x86-64 libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config build-essential` |
| Arch | `pacman -S mingw-w64-gcc webkit2gtk-4.1 gtk3 pkgconf base-devel` |

Add the Windows Rust target once: `rustup target add x86_64-pc-windows-gnu`.

### Build

```sh
git clone https://github.com/lobinuxsoft/tatu
cd tatu

# Tracker (Tauri desktop app)
cd src-tauri && cargo tauri dev   # development
cd src-tauri && cargo tauri build # release

# Win32 bridge (cross-compile)
./scripts/build-tatu-bridge.sh    # → target/dist/tatu-bridge.exe

# Steam compat tool drop-in
./scripts/build-tatu-launcher.sh  # → target/dist/tatu-launcher/
target/dist/tatu-launcher/install.sh
```

After `install.sh`, restart Steam and pick *Tatu Launcher* in a game's *Properties → Compatibility*. The opt-in per-appid lives in `~/.config/tatu/launcher.toml` (template seeded by the installer).

## Configuration

- **Tracker**: `~/.config/com.lobinux.tatu-tracker/` (Linux), `%APPDATA%\com.lobinux.tatu-tracker\` (Windows)
- **Steam compat tool**: `~/.steam/root/compatibilitytools.d/tatu-launcher/`
- **Launcher config**: `~/.config/tatu/launcher.toml`

## Versioning

Tatu uses [SemVer](https://semver.org/). Releases are managed by [release-please](https://github.com/googleapis/release-please) and triggered by [Conventional Commits](https://www.conventionalcommits.org/) on `main`.

## Project structure

```
tatu/
├── Cargo.toml                          # Workspace root
├── src-tauri/                          # tatu-tracker (Tauri + SolidJS)
├── crates/
│   ├── tatu-bridge/                    # Win32 PE (--launch + --connect modes)
│   ├── tatu-launcher/                  # Linux ELF Steam compat tool
│   ├── tatu-proto/                     # Wire types (bincode 2 + serde)
│   ├── cheat-runtime/                  # Pure-logic cheat engine + Linux ptrace backend
│   ├── cheat-runtime-extension/        # Cheat-runtime extension hooks
│   └── ce-launcher/                    # Wine launcher for Cheat Engine itself
├── tools/
│   └── tatu-launcher/                  # Drop-in payload (toolmanifest.vdf, install.sh, ...)
├── scripts/                            # Build helpers (bridge, launcher)
└── tests/                              # Workspace-level integration tests
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

- Built with [Tauri](https://tauri.app/) + [SolidJS](https://www.solidjs.com/)
- Cheat backend pattern inspired by [Aurora](https://www.cheathappens.com/) (clean-room re-implementation)
- Cheat Engine integration via the `.CT` table format
- Steam compatibility tool patterned after [Luxtorpeda](https://github.com/luxtorpeda-dev/luxtorpeda)
