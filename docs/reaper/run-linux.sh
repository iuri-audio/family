#!/usr/bin/env bash
# Runs a ReaScript in a Reaper installed natively on Linux on a scratch configuration and waits
# for it to quit. See ../driving-reaper.md.
#
#     docs/reaper/run-linux.sh <out-dir> <script.lua> [KEY=VALUE ...]
#
# Each KEY=VALUE reaches the script as an environment variable, as do OUT=<out-dir>, NICE_LOG,
# which sends the plugin's nice-plug log to <out-dir>/nice.log, and PRELUDE, the path of
# prelude.lua. RUN_TIMEOUT (seconds, default 120) bounds the run. REAPER_EXE overrides the
# executable, else `reaper` on the PATH, else Reaper's installer's default folders. REAPER_CONFIG
# overrides the configuration folder copied from (default ~/.config/REAPER).
set -euo pipefail

[[ $# -ge 2 ]] || { sed -n '5p' "$0" >&2; exit 2; }
out=$(realpath -m "$1")
script=$(realpath "$2")
here=$(dirname "$(realpath "$0")")
shift 2

for file in result.txt nice.log reaper.stdout; do
    if [[ -e "$out/$file" ]]; then
        echo "$out already holds a run's results: give this run a new out-dir, so they stay readable." >&2
        exit 1
    fi
done

if pgrep -x reaper > /dev/null; then
    echo "Reaper is already running: close it first, so the run doesn't share its plugins and audio." >&2
    exit 1
fi

exe=${REAPER_EXE:-}
if [[ -z $exe ]]; then
    exe=$(command -v reaper || true)
fi
if [[ -z $exe ]]; then
    for candidate in "$HOME/.local/share/REAPER/reaper" /opt/REAPER/reaper "$HOME/opt/REAPER/reaper"; do
        if [[ -x $candidate ]]; then exe=$candidate; break; fi
    done
fi
if [[ -z $exe || ! -x $exe ]]; then
    echo "Reaper wasn't found: put it on the PATH or set REAPER_EXE." >&2
    exit 1
fi

# A scratch resource folder with the user's settings, licence and plugin scan, so a run never
# changes the user's own configuration.
mkdir -p "$out/res"
config=${REAPER_CONFIG:-$HOME/.config/REAPER}
for file in reaper.ini reaper-license.rk reaper-vstplugins64.ini reaper-clap-linux-x86_64.ini; do
    [[ -e "$out/res/$file" || ! -e "$config/$file" ]] || cp "$config/$file" "$out/res/"
done

status=0
env OUT="$out" NICE_LOG="$out/nice.log" PRELUDE="$here/prelude.lua" "$@" \
    timeout "${RUN_TIMEOUT:-120}" "$exe" -newinst -nosplash -cfgfile "$out/res/reaper.ini" "$script" \
    > "$out/reaper.stdout" 2>&1 || status=$?

if [[ $status -eq 124 ]]; then
    echo "Reaper didn't quit within ${RUN_TIMEOUT:-120} s: the script hung or a dialog is open." >&2
    exit 124
fi
exit "$status"
