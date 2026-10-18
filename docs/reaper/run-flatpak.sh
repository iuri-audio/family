#!/usr/bin/env bash
# Runs a ReaScript in Flathub's Reaper (Linux) on a scratch configuration and waits for it to
# quit. See ../driving-reaper.md.
#
#     docs/reaper/run-flatpak.sh <out-dir> <script.lua> [KEY=VALUE ...]
#
# Each KEY=VALUE reaches the script as an environment variable, as do OUT=<out-dir> and NICE_LOG,
# which sends the plugin's nice-plug log to <out-dir>/nice.log. RUN_TIMEOUT (seconds, default 120)
# bounds the run.
set -euo pipefail

[[ $# -ge 2 ]] || { sed -n '5p' "$0" >&2; exit 2; }
out=$(realpath -m "$1")
script=$(realpath "$2")
shift 2

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
rm -f "$out/nice.log"

envs=(--env="OUT=$out" --env="NICE_LOG=$out/nice.log")
for pair in "$@"; do envs+=(--env="$pair"); done

status=0
timeout "${RUN_TIMEOUT:-120}" flatpak run \
    --filesystem="$out" --filesystem="$(dirname "$script")" "${envs[@]}" \
    fm.reaper.Reaper -newinst -nosplash -cfgfile "$out/res/reaper.ini" "$script" \
    > "$out/reaper.stdout" 2>&1 || status=$?

if [[ $status -eq 124 ]]; then
    echo "Reaper didn't quit within ${RUN_TIMEOUT:-120} s: the script hung or a dialog is open." >&2
    exit 124
fi
exit "$status"
