#!/usr/bin/env bash
# Steam launch wrapper for the Aurora-style co-launch spike (#102 spike B).
#
# Substitutes the game.exe (last arg of "$@") with cheat-bootstrap.exe
# and passes the original game path as bootstrap's argument. Bootstrap
# then CreateProcess-spawns the bridge AND the real game as siblings
# INSIDE THE SAME Proton invocation — they share the SLR container,
# wineserver, and `/tmp` namespace, so ToolHelp32 inside the bridge
# can see the game's PID.
#
# Both bootstrap and bridge log to files inside the prefix
# (drive_c/users/Public/*.log), which the wrapper concatenates back
# into /tmp/cheat-bridge-spike.log after the game exits. We can't
# rely on Win32 stdout propagating up through Proton + SLR reliably.

set -u

LOG=/tmp/cheat-bridge-spike.log
BRIDGE_SRC="/var/mnt/DATA/Repos/game-progress-tracker/target/x86_64-pc-windows-gnu/release/cheat-bridge.exe"
BOOTSTRAP_SRC="/var/mnt/DATA/Repos/game-progress-tracker/target/x86_64-pc-windows-gnu/release/cheat-bootstrap.exe"

if [[ -z "${STEAM_COMPAT_DATA_PATH:-}" || ! -f "$BRIDGE_SRC" || ! -f "$BOOTSTRAP_SRC" ]]; then
    echo "[wrapper] missing STEAM_COMPAT_DATA_PATH or source binaries — falling back to direct game launch" >"$LOG"
    exec "$@"
fi

PUBLIC="$STEAM_COMPAT_DATA_PATH/pfx/drive_c/users/Public"
mkdir -p "$PUBLIC"
cp "$BRIDGE_SRC" "$PUBLIC/cheat-bridge.exe"
cp "$BOOTSTRAP_SRC" "$PUBLIC/cheat-bootstrap.exe"
# Wipe any stale logs so we read fresh ones at the end.
rm -f "$PUBLIC/cheat-bootstrap.log" "$PUBLIC/cheat-bridge.log"

# Rebuild argv: substitute the trailing game.exe with bootstrap, then
# pass the original game path as bootstrap's argument.
ARGS=("$@")
LAST_IDX=$((${#ARGS[@]} - 1))
GAME_EXE_ORIG="${ARGS[$LAST_IDX]}"
ARGS[$LAST_IDX]="$PUBLIC/cheat-bootstrap.exe"
ARGS+=("$GAME_EXE_ORIG")

{
    echo "--- spike start ($(date)) ---"
    echo "original argv ($#): $*"
    echo "GAME_EXE_ORIG=$GAME_EXE_ORIG"
    echo "BOOTSTRAP_HOST=$PUBLIC/cheat-bootstrap.exe"
    echo "rewritten argv (${#ARGS[@]}): ${ARGS[*]}"
    echo "---"
} >"$LOG"

# Run the chain (NOT exec — we need to keep going to collect logs).
# All Proton / SLR / Wine chatter still goes to $LOG.
"${ARGS[@]}" >>"$LOG" 2>&1
GAME_EXIT=$?

# Collect the in-prefix logs.
{
    echo "--- /cheat-bootstrap.log ---"
    cat "$PUBLIC/cheat-bootstrap.log" 2>/dev/null || echo "(file missing — bootstrap never ran)"
    echo "--- /cheat-bridge.log ---"
    cat "$PUBLIC/cheat-bridge.log" 2>/dev/null || echo "(file missing — bridge never ran)"
    echo "--- spike end ($(date)) game exit=$GAME_EXIT ---"
} >>"$LOG"

exit "$GAME_EXIT"
