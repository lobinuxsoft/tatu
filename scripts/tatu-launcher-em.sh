#!/usr/bin/env bash
# Steam launch wrapper for Ender Magnolia using tatu-bridge in --launch
# mode. Acts as the Win32 entry point inside Proton: substitutes the
# trailing game.exe with tatu-bridge.exe and passes --launch <game.exe>
# + --target-exe + iteration knobs.
#
# tatu-bridge then CreateProcess-spawns `self --connect` AND the real
# game.exe as siblings of one Proton invocation, so they share the SLR
# container + wineserver. The bridge in --connect mode polls
# ToolHelp32 for the game's inner exe, OpenProcesses it, and exercises
# WriteProcessMemory + ReadProcessMemory.
#
# Logs land under <prefix>/drive_c/users/Public/tatu-bridge*.log and
# are cat'd back to /tmp/tatu-launcher-em.log after the game exits.
#
# Install:
#   1. Build: `./scripts/build-tatu-bridge.sh`
#   2. Steam → Ender Magnolia → Properties → Launch Options:
#        /var/mnt/DATA/Repos/tatu/scripts/tatu-launcher-em.sh %command%
#   3. Launch EM normally; close it; read /tmp/tatu-launcher-em.log.

set -u

LOG=/tmp/tatu-launcher-em.log
BRIDGE_SRC="/var/mnt/DATA/Repos/tatu/target/x86_64-pc-windows-gnu/release/tatu-bridge.exe"
TARGET_EXE="EnderMagnoliaSteam-Win64-Shipping.exe"
ITERS="${ITERS:-1000}"
BYTES="${BYTES:-256}"

if [[ -z "${STEAM_COMPAT_DATA_PATH:-}" || ! -f "$BRIDGE_SRC" ]]; then
    echo "[launcher] missing STEAM_COMPAT_DATA_PATH or tatu-bridge.exe — falling back to direct launch" >"$LOG"
    exec "$@"
fi

PUBLIC="$STEAM_COMPAT_DATA_PATH/pfx/drive_c/users/Public"
mkdir -p "$PUBLIC"
cp "$BRIDGE_SRC" "$PUBLIC/tatu-bridge.exe"
# Wipe stale logs so we read fresh output each run.
rm -f "$PUBLIC/tatu-bridge.log" "$PUBLIC/tatu-bridge-launch.log"

# Rebuild argv: substitute the trailing game.exe Linux path with the
# in-prefix tatu-bridge.exe, then append `--launch <game.exe> --target-exe …`.
ARGS=("$@")
LAST_IDX=$((${#ARGS[@]} - 1))
GAME_EXE_ORIG="${ARGS[$LAST_IDX]}"
ARGS[$LAST_IDX]="$PUBLIC/tatu-bridge.exe"
ARGS+=("--launch" "$GAME_EXE_ORIG" "--target-exe" "$TARGET_EXE" "--iters" "$ITERS" "--bytes" "$BYTES")

{
    echo "--- launcher start ($(date)) ---"
    echo "original argv ($#): $*"
    echo "GAME_EXE_ORIG=$GAME_EXE_ORIG"
    echo "BRIDGE_HOST=$PUBLIC/tatu-bridge.exe"
    echo "rewritten argv (${#ARGS[@]}): ${ARGS[*]}"
    echo "---"
} >"$LOG"

"${ARGS[@]}" >>"$LOG" 2>&1
GAME_EXIT=$?

{
    echo "--- /tatu-bridge-launch.log ---"
    cat "$PUBLIC/tatu-bridge-launch.log" 2>/dev/null || echo "(file missing — launch mode never ran)"
    echo "--- /tatu-bridge.log ---"
    cat "$PUBLIC/tatu-bridge.log" 2>/dev/null || echo "(file missing — connect mode never ran)"
    echo "--- launcher end ($(date)) game exit=$GAME_EXIT ---"
} >>"$LOG"

exit "$GAME_EXIT"
