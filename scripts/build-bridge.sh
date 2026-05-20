#!/usr/bin/env bash
# Cross-compile cheat-bridge.exe for x86_64-pc-windows-gnu and stage the
# artefact under target/dist/ so the spike launch script and any future
# bundler pick from one canonical location.
#
# Prerequisites are identical to scripts/build-dll.sh — same mingw +
# rust target. See that script's header for OS-specific install
# instructions.

set -euo pipefail

CRATE="cheat-bridge"
TARGET="x86_64-pc-windows-gnu"
PROFILE="release"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

cargo build -p "$CRATE" --target "$TARGET" --profile "$PROFILE"

ARTEFACT="target/$TARGET/$PROFILE/cheat-bridge.exe"
DIST_DIR="target/dist"
mkdir -p "$DIST_DIR"
cp "$ARTEFACT" "$DIST_DIR/cheat-bridge.exe"

printf 'staged: %s (%s bytes)\n' "$DIST_DIR/cheat-bridge.exe" \
    "$(stat -c %s "$DIST_DIR/cheat-bridge.exe")"
