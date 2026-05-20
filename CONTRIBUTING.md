# Contributing to Tatu

Thank you for your interest in contributing! This document outlines our development workflow and standards.

## Quick Reference

| Item | Value |
|------|-------|
| PRs target | `development` branch |
| Commit language | English |
| Commit format | [Conventional Commits](https://www.conventionalcommits.org/) |
| Code comments | English |
| License | AGPL v3 |

## Issue-First Development

**Always create an issue before coding.**

```
Create Issue → Create Branch → Develop → PR to development → Close Issue
```

This ensures work is tracked, discussed, and properly scoped before implementation begins.

## Branch Naming

Create branches from issues using this pattern:

```
feature/issue-XX-short-description
fix/issue-XX-short-description
docs/issue-XX-short-description
refactor/issue-XX-short-description
chore/issue-XX-short-description
```

Example: `feature/issue-42-bridge-pointer-chain`

## Commit Messages

Write commits in **English** using Conventional Commits format:

```
feat(bridge): port AOB scanner to Win32 ReadProcessMemory
fix(tracker): persist target_exe override across reloads
docs(launcher): document toolmanifest.vdf shape
refactor(cheat-runtime): split executor into engine/active/rollback
test(proto): round-trip every wire variant
chore(ci): bump rust-toolchain action
```

### Types

| Type | Use for |
|------|---------|
| `feat` | New features |
| `fix` | Bug fixes |
| `docs` | Documentation only |
| `refactor` | Code changes that neither fix bugs nor add features |
| `test` | Adding or updating tests |
| `chore` | Maintenance tasks |
| `build` | Build system changes |
| `ci` | CI/CD changes |

Only `feat`, `fix`, and `BREAKING CHANGE` trigger version bumps in release-please.

## Pull Requests

1. **Target branch**: `development` (never `main` directly)
2. **Title**: Clear description of the change
3. **Body**: Reference the issue with `Closes #XX`
4. **Size**: Keep PRs focused and reviewable — large refactors should be discussed in the linked issue first

```sh
gh pr create --base development --title "feat(bridge): pointer-chain walker" --body "Closes #42"
```

## Code Standards

### Rust

| Item | Convention |
|------|------------|
| Modules / files | `snake_case` |
| Types / traits | `PascalCase` |
| Functions / vars | `snake_case` |
| Constants | `SCREAMING_SNAKE_CASE` |
| Errors | `thiserror`-style typed enums; wrap with context (`#[source]`) |
| Style | `cargo fmt --all --check` must pass; `cargo clippy -- -D warnings` must pass |
| Data layout | Data-oriented (SoA over AoS, `u32` indices over pointers) where applicable |
| Comments | English |

### TypeScript / SolidJS

| Item | Convention |
|------|------------|
| Components | `PascalCase` (`GameList.tsx`, `CheatRow.tsx`) |
| Files (non-components) | `kebab-case` (`api-client.ts`, `types.ts`) |
| Variables / functions | `camelCase` |
| Types / interfaces | `PascalCase` |
| Props access | Never destructure `props` — access inline (`props.x`) to preserve reactivity |
| Comments | English |

### General Guidelines

- **Comments**: Write in English
- **Type hints**: Always — TypeScript strict, Rust no `_` placeholders in public APIs
- **Error handling**: Wrap with context (`thiserror` in Rust, error chains in TS)
- **Security**: Never log credentials, pairing tokens, or memory contents of third-party processes

## Building the Project

**Always use the provided scripts.** See [README.md → Building from source](README.md#building-from-source) for the full matrix.

```sh
# Tracker (development)
cd src-tauri && cargo tauri dev

# Win32 bridge cross-compile
./scripts/build-tatu-bridge.sh

# Steam compat tool drop-in
./scripts/build-tatu-launcher.sh
```

Before pushing Rust changes, run:

```sh
cargo fmt --all --check
cargo clippy --all-targets --workspace -- -D warnings
cargo build --workspace --release
```

CI runs the same checks and a rustfmt failure costs a rerun.

## Labels

When creating issues, use appropriate labels:

| Category | Labels |
|----------|--------|
| Priority | `priority:critical`, `priority:high`, `priority:medium`, `priority:low` |
| Difficulty | `difficulty:easy`, `difficulty:medium`, `difficulty:hard` |
| Area | `tracker`, `bridge`, `launcher`, `cheat-runtime`, `proto`, `ci`, `docs` |

## Getting Help

- **Questions**: Open a [Discussion](https://github.com/lobinuxsoft/tatu/discussions)
- **Bugs**: Create an [Issue](https://github.com/lobinuxsoft/tatu/issues)

## License

By contributing, you agree that your contributions will be licensed under the AGPL v3.
