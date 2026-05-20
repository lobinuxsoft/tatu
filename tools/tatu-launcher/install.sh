#!/usr/bin/env bash
# Idempotent install of the Tatu Launcher Steam compatibility tool.
# Copies this directory's drop-in payload into
# ~/.steam/root/compatibilitytools.d/tatu-launcher/. After install,
# restart Steam and pick "Tatu Launcher" in a game's Properties →
# Compatibility.
#
# Run from a staged dist directory (target/dist/tatu-launcher/) or
# from the repo's tools/tatu-launcher/. In the latter case the
# launcher binary and tatu-bridge.exe must have been built first
# (scripts/build-tatu-launcher.sh + scripts/build-tatu-bridge.sh).

set -euo pipefail

SRC="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"

if [[ ! -f "$SRC/tatu-launcher" ]]; then
    cat >&2 <<EOF
[install] tatu-launcher binary missing next to install.sh.
[install] Run: scripts/build-tatu-launcher.sh
[install] Then re-run install.sh from target/dist/tatu-launcher/.
EOF
    exit 1
fi

STEAM_ROOT=""
for c in "$HOME/.steam/root" "$HOME/.steam/steam" "$HOME/.local/share/Steam"; do
    if [[ -d "$c" ]]; then STEAM_ROOT="$c"; break; fi
done
if [[ -z "$STEAM_ROOT" ]]; then
    echo "[install] no Steam install found under ~/.steam/{root,steam} or ~/.local/share/Steam" >&2
    exit 1
fi

DEST="$STEAM_ROOT/compatibilitytools.d/tatu-launcher"
mkdir -p "$DEST"

install -m 0755 "$SRC/tatu-launcher"               "$DEST/tatu-launcher"
install -m 0755 "$SRC/tatu-launcher.sh"            "$DEST/tatu-launcher.sh"
install -m 0644 "$SRC/toolmanifest.vdf"            "$DEST/toolmanifest.vdf"
install -m 0644 "$SRC/compatibilitytool.vdf"       "$DEST/compatibilitytool.vdf"

if [[ -f "$SRC/tatu-bridge.exe" ]]; then
    install -m 0755 "$SRC/tatu-bridge.exe" "$DEST/tatu-bridge.exe"
else
    echo "[install] WARN: tatu-bridge.exe not staged — bridge handoff will fail until you build it." >&2
fi

CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/tatu"
CONFIG_FILE="$CONFIG_DIR/launcher.toml"
mkdir -p "$CONFIG_DIR"
if [[ ! -f "$CONFIG_FILE" ]]; then
    install -m 0644 "$SRC/tatu-launcher.toml.example" "$CONFIG_FILE"
    printf '[install] seeded %s — edit before enabling games.\n' "$CONFIG_FILE"
else
    printf '[install] kept existing %s\n' "$CONFIG_FILE"
fi

printf '[install] tatu-launcher installed at %s\n' "$DEST"
printf '[install] Restart Steam, then pick "Tatu Launcher" in a game'\''s Properties → Compatibility.\n'
