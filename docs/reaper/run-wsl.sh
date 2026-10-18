#!/usr/bin/env bash
# Runs a ReaScript in Windows Reaper with a scratch configuration, as run-flatpak.sh does.
# Usage: bash docs/reaper/run-wsl.sh <out-dir> <script.lua> [KEY=VALUE ...]
# REAPER_CONFIG and REAPER_EXE override the usual Windows installation paths.
# Use an out-dir on a Windows drive (/mnt/c/...), where Reaper can write its configuration.
# REAPER_RESOURCE_DIR can reuse a scratch resource folder between runs; never use the real one.
set -euo pipefail

[[ $# -ge 2 ]] || { sed -n '3p' "$0" >&2; exit 2; }
out=$(realpath -m "$1")
script=$(realpath "$2")
here=$(dirname "$(realpath "$0")")
shift 2
[[ "$out" =~ ^/mnt/[a-z]/ ]] || { echo 'Use an out-dir on a Windows drive (/mnt/c/...). ' >&2; exit 2; }
for file in result.txt nice.log reaper.stdout; do
    if [[ -e "$out/$file" ]]; then
        echo "$out already holds a run's results: choose a new out-dir." >&2
        exit 1
    fi
done
tasks=$(/mnt/c/WINDOWS/System32/tasklist.exe /FI 'IMAGENAME eq reaper.exe' /FO CSV /NH)
if [[ "$tasks" == *'"reaper.exe"'* ]]; then
    echo 'Reaper is already running: close it first.' >&2
    exit 1
fi
config=${REAPER_CONFIG:-/mnt/c/Users/$USER/AppData/Roaming/REAPER}
exe=${REAPER_EXE:-/mnt/c/Program Files/REAPER (x64)/reaper.exe}
res=${REAPER_RESOURCE_DIR:-$out/res}
mkdir -p "$out" "$res"
for file in REAPER.ini reaper-license.rk reaper-vstplugins64.ini reaper-clap-win64.ini; do
    [[ -e "$res/$file" || ! -e "$config/$file" ]] || cp "$config/$file" "$res/"
done
# A copied configuration may reference a default project template that isn't in the scratch
# resource directory. Start empty instead of opening a modal error before the script runs.
python3 - "$res/REAPER.ini" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
lines = p.read_text().splitlines()
p.write_text('\n'.join('newprojtmpl=' if line.startswith('newprojtmpl=') else
                       'loadlastproj=0' if line.startswith('loadlastproj=') else line
                       for line in lines) + '\n')
PY
export OUT="$out" NICE_LOG="$out/nice.log" PRELUDE="${PRELUDE:-$here/prelude.lua}"
export WSLENV="${WSLENV:+$WSLENV:}OUT/p:NICE_LOG/p:PRELUDE/p"
for pair in "$@"; do
    [[ "$pair" =~ ^[A-Za-z_][A-Za-z_0-9]*= ]] || { echo 'Expected KEY=VALUE' >&2; exit 2; }
    export "$pair"
    WSLENV+=":${pair%%=*}"
done
status=0
# Wait on the specific Windows process: timeout(1) cannot reliably terminate a Windows child.
export REAPER_RUN_EXE="$exe" REAPER_RUN_CONFIG="$res/REAPER.ini" REAPER_RUN_SCRIPT="$script"
export REAPER_RUN_TIMEOUT="${RUN_TIMEOUT:-120}"
WSLENV+=':REAPER_RUN_EXE/p:REAPER_RUN_CONFIG/p:REAPER_RUN_SCRIPT/p:REAPER_RUN_TIMEOUT'
cp "$here/run-wsl.ps1" "$out/launch.ps1"
cd "$out"
/mnt/c/WINDOWS/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -ExecutionPolicy Bypass \
    -File "$(wslpath -w "$out/launch.ps1")" \
    > "$out/reaper.stdout" 2>&1 || status=$?
if [[ $status -eq 124 ]]; then
    echo "Reaper didn't quit within ${RUN_TIMEOUT:-120} s." >&2
fi
exit "$status"
