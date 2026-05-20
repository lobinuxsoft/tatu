#!/usr/bin/env bash
# Build the Linux tatu-launcher binary and stage the full Steam
# compat-tool drop-in under target/dist/tatu-launcher/. If the
# Win32 bridge has already been built via build-tatu-bridge.sh
# (target/dist/tatu-bridge.exe), it is copied in too so the
# directory becomes a turnkey install.
#
# Output layout matches what install.sh expects to rsync into
# ~/.steam/root/compatibilitytools.d/tatu-launcher/.

set -euo pipefail

CRATE="tatu-launcher"
PROFILE="release"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

cargo build -p "$CRATE" --profile "$PROFILE"

DIST="target/dist/tatu-launcher"
mkdir -p "$DIST"
install -m 0755 "target/$PROFILE/tatu-launcher" "$DIST/tatu-launcher"
install -m 0755 tools/tatu-launcher/tatu-launcher.sh "$DIST/tatu-launcher.sh"
install -m 0755 tools/tatu-launcher/install.sh "$DIST/install.sh"
install -m 0644 tools/tatu-launcher/toolmanifest.vdf "$DIST/toolmanifest.vdf"
install -m 0644 tools/tatu-launcher/compatibilitytool.vdf "$DIST/compatibilitytool.vdf"
install -m 0644 tools/tatu-launcher/tatu-launcher.toml.example "$DIST/tatu-launcher.toml.example"
install -m 0644 tools/tatu-launcher/README.md "$DIST/README.md"

BRIDGE="target/dist/tatu-bridge.exe"
if [[ -f "$BRIDGE" ]]; then
    install -m 0755 "$BRIDGE" "$DIST/tatu-bridge.exe"
    printf 'staged: %s with tatu-bridge.exe\n' "$DIST"
else
    printf 'staged: %s (no tatu-bridge.exe — run scripts/build-tatu-bridge.sh first for a complete drop-in)\n' "$DIST"
fi
