#!/usr/bin/env bash
# Runs a ReaScript in Flathub's Reaper (Linux) on a scratch configuration and waits for it to
# quit. See ../driving-reaper.md.
#
#     docs/reaper/run-flatpak.sh <out-dir> <script.lua> [KEY=VALUE ...]
#
# Each KEY=VALUE reaches the script as an environment variable, as do OUT=<out-dir>, NICE_LOG,
# which sends the plugin's nice-plug log to <out-dir>/nice.log, and PRELUDE, the path of
# prelude.lua. RUN_TIMEOUT (seconds, default 120) bounds the run.
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

# A scratch resource folder with the user's settings, licence and plugin scan, so a run never
# changes the user's own configuration.
mkdir -p "$out/res"
config="$HOME/.var/app/fm.reaper.Reaper/config/REAPER"
for file in reaper.ini reaper-license.rk reaper-vstplugins64.ini reaper-clap-linux-x86_64.ini; do
    [[ -e "$out/res/$file" || ! -e "$config/$file" ]] || cp "$config/$file" "$out/res/"
done

envs=(--env="OUT=$out" --env="NICE_LOG=$out/nice.log" --env="PRELUDE=$here/prelude.lua")
for pair in "$@"; do envs+=(--env="$pair"); done

status=0
timeout "${RUN_TIMEOUT:-120}" flatpak run \
    --filesystem="$out" --filesystem="$(dirname "$script")" --filesystem="$here:ro" "${envs[@]}" \
    fm.reaper.Reaper -newinst -nosplash -cfgfile "$out/res/reaper.ini" "$script" \
    > "$out/reaper.stdout" 2>&1 || status=$?

if [[ $status -eq 124 ]]; then
    echo "Reaper didn't quit within ${RUN_TIMEOUT:-120} s: the script hung or a dialog is open." >&2
    exit 124
fi
exit "$status"
