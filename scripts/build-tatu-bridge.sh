#!/usr/bin/env bash
# Cross-compile tatu-bridge.exe for x86_64-pc-windows-gnu and stage
# the artefact under target/dist/ so the launcher script and any
# eventual installer pick from one canonical location.
#
# Prerequisites on a Bazzite / Fedora-atomic dev host:
#
#   rpm-ostree install mingw64-gcc mingw64-gcc-c++ mingw64-winpthreads-static
#   rustup target add x86_64-pc-windows-gnu

set -euo pipefail

CRATE="tatu-bridge"
TARGET="x86_64-pc-windows-gnu"
PROFILE="release"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

cargo build -p "$CRATE" --target "$TARGET" --profile "$PROFILE"

ARTEFACT="target/$TARGET/$PROFILE/tatu-bridge.exe"
DIST_DIR="target/dist"
mkdir -p "$DIST_DIR"
cp "$ARTEFACT" "$DIST_DIR/tatu-bridge.exe"

printf 'staged: %s (%s bytes)\n' "$DIST_DIR/tatu-bridge.exe" \
    "$(stat -c %s "$DIST_DIR/tatu-bridge.exe")"
