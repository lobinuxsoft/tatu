#!/usr/bin/env bash
# Steam compat tool entry. Steam invokes:
#   tatu-launcher.sh <verb> <command...>
# Verbs: waitforexitandrun, run, getcompatpath, getnativepath, ...
#
# Two responsibilities here:
#   1. Strip LD_PRELOAD (Steam Overlay) — the Win32 bridge spike
#      validated only without it; consistent with Luxtorpeda's wrapper.
#   2. exec the Rust binary that does the real work (config parse,
#      Proton resolution, argv rewriting for the bridge handoff).

if [[ -n "${LD_PRELOAD:-}" ]]; then
    export ORIGINAL_LD_PRELOAD="$LD_PRELOAD"
    unset LD_PRELOAD
fi

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
exec "$DIR/tatu-launcher" "$@"
