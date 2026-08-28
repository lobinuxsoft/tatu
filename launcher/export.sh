#!/usr/bin/env bash
# Builds the cartridge launcher for both platforms, stamping build_info.txt
# with the repo's VERSION + the short commit it was built from first — see
# main.gd's corner label, added after live testing kept confusing a stale
# binary for a freshly fixed one with no way to tell them apart on screen.
#
# Usage: launcher/export.sh <godot-binary> [output-dir]
set -euo pipefail

GODOT="${1:?usage: export.sh <path-to-godot-binary> [output-dir]}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LAUNCHER_DIR="$REPO_ROOT/launcher"
# Must be absolute: Godot resolves a relative --export-release path against
# the project root (--path launcher/), not this script's own working
# directory, and silently fails with "export path doesn't exist" otherwise.
OUT_DIR="${2:-$REPO_ROOT/dist/launcher}"

VERSION="$(cat "$REPO_ROOT/VERSION")"
COMMIT="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
DIRTY=""
git -C "$REPO_ROOT" diff --quiet -- "$LAUNCHER_DIR" || DIRTY="-dirty"
echo "${VERSION}+g${COMMIT}${DIRTY}" > "$LAUNCHER_DIR/build_info.txt"

mkdir -p "$OUT_DIR"
"$GODOT" --headless --path "$LAUNCHER_DIR" --export-release "Linux" "$OUT_DIR/tatu-launcher"
"$GODOT" --headless --path "$LAUNCHER_DIR" --export-release "Windows Desktop" "$OUT_DIR/tatu-launcher.exe"

echo "Built $(cat "$LAUNCHER_DIR/build_info.txt") -> $OUT_DIR"
