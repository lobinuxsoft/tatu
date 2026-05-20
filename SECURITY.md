# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| Latest | Yes |
| Older | No |

As an early-stage project, only the latest version receives updates.

## Reporting a Concern

If you discover a potential security issue, please report it responsibly:

1. **Do NOT** open a public issue
2. **Do** contact the maintainer privately via [GitHub Discussions](https://github.com/lobinuxsoft/tatu/discussions) (private message) or [GitHub Security Advisories](https://github.com/lobinuxsoft/tatu/security/advisories/new)
3. Include as much detail as possible to help reproduce and understand the issue

## Response Timeline

- **Acknowledgment**: Within 72 hours
- **Initial assessment**: Within 1 week
- **Resolution timeline**: Depends on complexity, communicated after assessment

## Scope

This policy applies to:

- `tatu-tracker` — the Tauri desktop application (Tauri commands, IPC, persisted state)
- `tatu-bridge` — the Win32 worker running under Wine (`OpenProcess` / `VirtualAllocEx` / `WriteProcessMemory` / `ReadProcessMemory` against the game)
- `tatu-launcher` — the Linux Steam compatibility tool that delegates to the user's real Proton
- `cheat-runtime` — the Linux ptrace backend (`/proc/<pid>/mem` access)
- `.CT` table parsing — untrusted input handling

**Out of scope**:

- Third-party dependencies — report upstream
- Cheat-table contents authored by third parties (`.CT` files distributed elsewhere) — Tatu only parses them, it does not vouch for their behavior
- The Wine / Proton compatibility stack itself
- Cheat Engine upstream (`ce-launcher` only invokes it under a sandboxed prefix)

## Security Considerations

Tatu performs operations that overlap with security-sensitive surfaces:

- **Cross-process memory writes**: `tatu-bridge` issues `WriteProcessMemory` against the game it co-launched. The bridge does not target processes the user did not explicitly opt into via `~/.config/tatu/launcher.toml`.
- **Steam compatibility tool**: `tatu-launcher` runs under the user's Steam session. It defers to a real Proton specified in `launcher.toml` — never silently substitutes an unverified Proton.
- **`.CT` import**: `.CT` files are XML and may contain Lua snippets. Tatu's parser does not execute Lua; auto-assembler payloads are parsed to the runtime's typed IR and executed only on user toggle.
- **Anti-cheat aware**: games with EAC / BattlEye / Vanguard / VAC are detected and refused — Tatu does not attempt bypasses.
- **Local-only**: Tatu has no network surface beyond Steam's own protocol; no remote-control hooks, no telemetry uploads.

### Best Practices for Users

1. Only import `.CT` files from sources you trust
2. Keep your `launcher.toml` private — it lists which games are opted in
3. Do not enable Tatu against multiplayer games even if Steam doesn't flag them
4. Keep the application updated

## Recognition

Contributors who responsibly report valid issues will be credited in release notes (unless they prefer anonymity).
