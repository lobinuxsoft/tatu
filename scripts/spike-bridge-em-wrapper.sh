#!/usr/bin/env bash
# Steam launch wrapper that spawns cheat-bridge.exe as a sibling
# process of the game UNDER THE SAME Proton/Wine session.
#
# Steam passes a chain of wrappers as "$@":
#   steam-launch-wrapper -- reaper SteamLaunch -- _v2-entry-point
#   -- /path/to/Proton X/proton waitforexitandrun /path/to/game.exe
#
# We scan "$@" to find the actual Proton binary, copy the bridge into
# the prefix's `drive_c/users/Public/` (Steam Linux Runtime sandboxes
# host paths like /var/mnt/DATA/ behind bind mounts; in-prefix paths
# always work), then spawn it via `proton waitforexitandrun` so it
# shares the same wineserver as the game.
#
# Install:
#   1. Build: `./scripts/build-bridge.sh`
#   2. Steam → Ender Magnolia → Properties → Launch Options:
#        /var/mnt/DATA/Repos/game-progress-tracker/scripts/spike-bridge-em-wrapper.sh %command%
#   3. Launch EM normally; close it; read /tmp/cheat-bridge-spike.log.

set -u

LOG=/tmp/cheat-bridge-spike.log
BRIDGE_SRC="/var/mnt/DATA/Repos/game-progress-tracker/target/dist/cheat-bridge.exe"
TARGET_EXE="EnderMagnoliaSteam-Win64-Shipping.exe"
ITERS="${ITERS:-1000}"
BYTES="${BYTES:-256}"
WARMUP_SECONDS="${WARMUP_SECONDS:-20}"

# Scan "$@" for the actual proton binary (handles Proton-Experimental /
# Proton 9.0 / GE-Proton / proton-tkg-* layouts).
PROTON=""
for arg in "$@"; do
    case "$arg" in
        */Proton*/proton | */proton-*/proton | */GE-Proton*/proton)
            PROTON="$arg"
            break
            ;;
    esac
done

# Stage the bridge inside the prefix so the SLR sandbox sees it without
# needing a host bind mount for /var/mnt/DATA/.
BRIDGE_IN_PFX=""
if [[ -n "${STEAM_COMPAT_DATA_PATH:-}" && -f "$BRIDGE_SRC" ]]; then
    PUBLIC="$STEAM_COMPAT_DATA_PATH/pfx/drive_c/users/Public"
    mkdir -p "$PUBLIC"
    cp "$BRIDGE_SRC" "$PUBLIC/cheat-bridge.exe"
    # Wine maps drive_c → C:\, so the in-prefix Win32 path is:
    BRIDGE_IN_PFX='C:\\users\\Public\\cheat-bridge.exe'
fi

{
    echo "--- spike start ($(date)) ---"
    echo "argv ($#): $*"
    echo "STEAM_COMPAT_DATA_PATH=${STEAM_COMPAT_DATA_PATH:-unset}"
    echo "PROTON_DETECTED=${PROTON:-<<none>>}"
    echo "BRIDGE_IN_PFX=${BRIDGE_IN_PFX:-<<not staged>>}"
    echo "TARGET_EXE=$TARGET_EXE  ITERS=$ITERS  BYTES=$BYTES"
} >"$LOG"

if [[ -n "$PROTON" && -n "$BRIDGE_IN_PFX" ]]; then
    {
        echo "[wrapper] sleeping ${WARMUP_SECONDS}s before spawning bridge"
        sleep "$WARMUP_SECONDS"
        echo "[wrapper] spawning bridge via: $PROTON waitforexitandrun $BRIDGE_IN_PFX ..."
        "$PROTON" waitforexitandrun "$BRIDGE_IN_PFX" \
            --target-exe "$TARGET_EXE" \
            --iters "$ITERS" \
            --bytes "$BYTES"
        rc=$?
        echo "[wrapper] bridge exit code: $rc"
        echo "--- bridge done ($(date)) ---"
    } >>"$LOG" 2>&1 &
    BRIDGE_BG=$!
else
    echo "[wrapper] skipping bridge: missing PROTON or staging failed" >>"$LOG"
    BRIDGE_BG=""
fi

# Foreground: launch the game.
"$@"
GAME_EXIT=$?

if [[ -n "$BRIDGE_BG" ]]; then
    wait "$BRIDGE_BG" 2>/dev/null || true
fi
exit "$GAME_EXIT"
