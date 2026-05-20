#!/usr/bin/env bash
# Spike runner: invoke target/dist/cheat-bridge.exe inside Ender
# Magnolia's Wine prefix using protontricks-launch, so the bridge
# becomes a Win32 sibling of the live game process. The bridge then
# attaches to the game via OpenProcess / VirtualAllocEx /
# WriteProcessMemory / ReadProcessMemory and reports the round-trip
# variance count. See docs/spike-win32-bridge.md for what the numbers
# mean and what counts as a green/red light for pivoting the #102 epic.
#
# Prereq: EM must already be running under its Proton prefix when this
# script is invoked. Launch EM from Steam first, then run this.

set -euo pipefail

APPID="${APPID:-2725260}" # Ender Magnolia: Bloom in the Mist
TARGET_EXE="${TARGET_EXE:-EnderMagnoliaSteam-Win64-Shipping.exe}"
ITERS="${ITERS:-1000}"
BYTES="${BYTES:-256}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BRIDGE="$REPO_ROOT/target/dist/cheat-bridge.exe"

if [[ ! -x "$BRIDGE" ]]; then
    echo "bridge artefact missing — run scripts/build-bridge.sh first" >&2
    exit 1
fi

if ! command -v protontricks-launch >/dev/null 2>&1; then
    echo "protontricks-launch not in PATH (rpm-ostree install protontricks)" >&2
    exit 1
fi

# `--no-bwrap` avoids the bubblewrap sandbox so the bridge can see the
# game's PID. `--appid` pins the prefix to EM's compatdata directory so
# the OpenProcess call lands inside the same Wine instance.
exec protontricks-launch --no-bwrap --appid "$APPID" "$BRIDGE" \
    --target-exe "$TARGET_EXE" \
    --iters "$ITERS" \
    --bytes "$BYTES"
