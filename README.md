# Tatu

<div align="center">
  <strong>Steam backlog tracker, single-player cheat host, and portable game cartridge.</strong>

  [![License](https://img.shields.io/badge/License-Personal%20%26%20Share--Back-blue.svg)](LICENSE)
  [![Rust](https://img.shields.io/badge/Rust-stable-DEA584?logo=rust)](https://www.rust-lang.org/)
  [![Tauri](https://img.shields.io/badge/Tauri-v2-FFC131?logo=tauri)](https://tauri.app/)
</div>

> **Tatu** = *armadillo* in Guaraní. Sibling project to [Yryvu](https://github.com/lobinuxsoft/yryvu) (vulture, a Git client).

## What Tatu is

- A **backlog tracker** for the games you own on Steam (progress, achievements, trading cards, DRM, size on disk).
- A **single-player cheat host** — a clean-room CheatEngine Auto-Assembler engine driving a native Linux `ptrace` backend, applying `.CT`-table patches to processes you launched yourself.
- A **portable cartridge**: a removable drive Tatu formats and populates so a game library runs on any machine via a standalone Godot launcher, without that machine needing Tatu installed.

## What Tatu is NOT

- **NOT a piracy tool.** No DRM cracking, no distribution, no multiplayer interaction. It only operates on local processes you launched yourself.
- **NOT a multiplayer cheat.** Anti-cheat-protected games (EAC / BattlEye / Vanguard) are explicitly out of scope — detected and refused, no bypass attempts.
- **NOT a Cheat Engine fork.** A clean re-implementation consuming `.CT` tables, not a wrapper around `cheatengine.exe`.

## Platform support

| | Linux | Windows |
|---|---|---|
| Backlog tracker | ✅ | ✅ |
| Cheats | ✅ | ❌ hidden ([#181](https://github.com/lobinuxsoft/tatu/issues/181)) |

## Documentation

Everything past this point — how Tatu works, architecture, the cartridge/launcher subsystem, build instructions, in-progress design docs — lives in the **[Tatu Wiki](https://github.com/lobinuxsoft/tatu/wiki)**.

## Status

Tatu is **early-stage and not yet production-ready**. See the [Roadmap and Status](https://github.com/lobinuxsoft/tatu/wiki/Roadmap-and-Status) wiki page for what's shipped, in flight, and frozen.

## Quick build

```sh
git clone https://github.com/lobinuxsoft/tatu
cd tatu
cargo build --release -p tatu-tracker   # → target/release/tatu-tracker
```

Full requirements, platform dependencies, and the Godot launcher build are on the [Building](https://github.com/lobinuxsoft/tatu/wiki/Building) wiki page.

## Contributing

1. Fork the repository
2. Create a feature branch from `development`
3. Make your changes
4. Submit a PR to `development`

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

## License

Tatu Personal & Share-Back License — see [LICENSE](LICENSE) for details.

This means:
- Free for personal, noncommercial use — use it, modify it, learn from it
- No commercial use of any kind (selling, SaaS, use inside a for-profit business) without a separate agreement with the author
- Derivatives are allowed, but any modification you deploy or distribute must be shared back with the author (notice + source), not just published to the world
- No use of the Software as AI/ML training, fine-tuning, or retrieval data without a separate agreement (TDM rights reserved under EU Directive 2019/790 Art. 4)
- Versions released before 2026-09-24 remain under AGPLv3

**Contact for share-back / commercial or AI-training licensing:** [@lobinuxsoft](https://github.com/lobinuxsoft) — open an issue on this repo or a GitHub DM.

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
