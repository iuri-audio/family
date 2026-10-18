#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf 'Usage: bash %s <new-out-dir> <script.lua> <input-file>... [-- KEY=VALUE ...]\n' "$0"
}

[[ $# -ge 3 ]] || { usage >&2; exit 2; }
out=$(realpath -m -- "$1")
script=$(realpath "$2")
shift 2
[[ "$out" =~ ^/mnt/[a-z]/ ]] || {
    printf 'Use an output folder on a Windows drive (/mnt/c/...).\n' >&2
    exit 2
}

inputs=()
environment=()
while (( $# )); do
    if [[ "$1" == -- ]]; then
        shift
        environment=("$@")
        break
    fi
    inputs+=("$1")
    shift
done
(( ${#inputs[@]} > 0 )) || { usage >&2; exit 2; }

for pair in "${environment[@]}"; do
    [[ "$pair" =~ ^[A-Za-z_][A-Za-z_0-9]*= ]] || {
        printf 'Expected KEY=VALUE after --, got: %s\n' "$pair" >&2
        exit 2
    }
done

for path in "$out/input" "$out/result.txt" "$out/nice.log" "$out/reaper.stdout"; do
    [[ ! -e "$path" ]] || {
        printf '%s already exists; use a new, empty output folder.\n' "$path" >&2
        exit 1
    }
done

mkdir -p "$out/input"
declare -A staged_names=()
for source in "${inputs[@]}"; do
    [[ -f "$source" ]] || { printf 'Input file not found: %s\n' "$source" >&2; exit 1; }
    name=${source##*/}
    [[ -z "${staged_names[$name]:-}" ]] || {
        printf 'Inputs share the same basename: %s\n' "$name" >&2
        exit 1
    }
    staged_names[$name]=1
    cp -- "$source" "$out/input/$name"
done

here=$(dirname "$(realpath "$0")")
/usr/bin/bash "$here/run-wsl.sh" "$out" "$script" "${environment[@]}"