#!/usr/bin/env bash
# Saved-project/new-instance check, with physical host/canvas measurements on Windows.
# Usage: bash run-wsl-new-instance.sh <out-dir> <PLUGIN> <width-px> <height-px> [dpi]
# NEW_INSTANCES defaults to 3. EDITOR_TITLE overrides the inferred *plugin name* pattern.
# REAPER_EXE, REAPER_CONFIG, REAPER_RESOURCE_DIR and RUN_TIMEOUT work as in run-wsl.sh.
set -euo pipefail
[[ $# -ge 4 && $# -le 5 ]] || { sed -n '3p' "$0" >&2; exit 2; }
out=$(realpath -m "$1")
plugin=$2
width=$3
height=$4
dpi=${5:-0}
new_instances=${NEW_INSTANCES:-3}
for number in "$width" "$height" "$new_instances"; do
    [[ "$number" =~ ^[1-9][0-9]*$ ]] || { echo 'Sizes and NEW_INSTANCES must be positive integers.' >&2; exit 2; }
done
[[ "$dpi" =~ ^(0|[1-9][0-9]*)$ ]] || { echo 'DPI must be zero or a positive integer.' >&2; exit 2; }
[[ "$out" =~ ^/mnt/[a-z]/ ]] || { echo 'Use an out-dir on a Windows drive.' >&2; exit 2; }
[[ ! -e "$out" ]] || { echo 'Choose a new out-dir for this run.' >&2; exit 2; }
here=$(dirname "$(realpath "$0")")
title=${EDITOR_TITLE:-*${plugin#*:}*}
mkdir -p "$out"
cp "$here/check-editor-windows.ps1" "$out/check-editor-windows.ps1"
/mnt/c/WINDOWS/System32/WindowsPowerShell/v1.0/powershell.exe \
    -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w "$out/check-editor-windows.ps1")" \
    -Title "$title" -Width "$width" -Height "$height" -Dpi "$dpi" \
    -PhaseDirectory "$(wslpath -w "$out")" -Phases "$((2 + 2 * new_instances))" \
    -Seconds "${RUN_TIMEOUT:-120}" > "$out/measurement.stdout" 2>&1 &
observer_pid=$!
host_status=0
bash "$here/run-wsl.sh" "$out" "$here/new-instance.lua" \
    "PLUGIN=$plugin" "NEW_INSTANCES=$new_instances" || host_status=$?
if [[ $host_status -ne 0 ]]; then
    printf 'ERROR: host launcher exited %s\n' "$host_status" > "$out/finished.txt"
fi
measurement_status=0
wait "$observer_pid" || measurement_status=$?
for file in result.txt measurement.stdout; do
    [[ ! -e "$out/$file" ]] || cat "$out/$file"
done
[[ $host_status -eq 0 ]] || exit "$host_status"
exit "$measurement_status"
