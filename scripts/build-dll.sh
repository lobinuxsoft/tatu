#!/usr/bin/env bash
# Cross-compile cheat-runtime-dll for x86_64-pc-windows-gnu and stage
# the artefact under target/dist/ so installer code paths and CI both
# pick from a single, version-controlled location.
#
# Prerequisites on a Bazzite / Fedora-atomic dev host:
#
#   rpm-ostree install mingw64-gcc mingw64-gcc-c++ mingw64-winpthreads-static
#   # → reboot once for the layered packages to come online
#   rustup target add x86_64-pc-windows-gnu
#
# On a stock Fedora / Ubuntu CI image:
#
#   dnf install mingw64-gcc mingw64-gcc-c++ mingw64-winpthreads-static   # Fedora
#   apt-get install gcc-mingw-w64-x86-64                                  # Ubuntu
#   rustup target add x86_64-pc-windows-gnu

set -euo pipefail

CRATE="cheat-runtime-dll"
TARGET="x86_64-pc-windows-gnu"
PROFILE="release"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

cargo build -p "$CRATE" --target "$TARGET" --profile "$PROFILE"

ARTEFACT="target/$TARGET/$PROFILE/cheat_runtime_dll.dll"
DIST_DIR="target/dist"
mkdir -p "$DIST_DIR"
cp "$ARTEFACT" "$DIST_DIR/cheat_runtime_dll.dll"

printf 'staged: %s (%s bytes)\n' "$DIST_DIR/cheat_runtime_dll.dll" \
    "$(stat -c %s "$DIST_DIR/cheat_runtime_dll.dll")"
