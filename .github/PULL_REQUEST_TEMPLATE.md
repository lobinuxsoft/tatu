## Description

<!-- Brief description of the changes -->

## Related Issue

Closes #<!-- issue number -->

<!--
IMPORTANT REMINDERS:
- All PRs must target `development` branch
- Branch naming: feature/issue-XX-desc, fix/issue-XX-desc, docs/issue-XX-desc
- Every PR must be linked to an issue (Issue-First Development)
-->

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update
- [ ] Refactoring (no functional changes)

## Component

<!-- Check the components affected by this PR -->
- [ ] `tracker` — Tauri desktop application (UI + Tauri commands)
- [ ] `bridge` — Win32 worker (--launch / --connect modes)
- [ ] `launcher` — Steam compatibility tool (Linux ELF)
- [ ] `cheat-runtime` — Linux ptrace backend / pure-logic engine
- [ ] `proto` — Wire types (bincode 2 + serde)
- [ ] `ci` — Workflows, build scripts
- [ ] `documentation`

## Checklist

- [ ] This PR targets `development` branch
- [ ] My branch follows naming convention (`feature/issue-XX-desc`, `fix/issue-XX-desc`)
- [ ] This PR is linked to an existing issue
- [ ] My commits follow Conventional Commits format (in English)
- [ ] My code follows the project's code standards
- [ ] `cargo fmt --all --check` and `cargo clippy -- -D warnings` pass locally
- [ ] I have tested my changes locally
- [ ] I have updated documentation if needed

## Screenshots / Demo

<!-- If applicable, add screenshots or GIFs demonstrating the change -->

## Additional Notes

<!-- Any other context or information reviewers should know -->
