#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 1 ]]; then
    printf 'Usage: bash %s NEW_DIRECTORY\n' "$0" >&2
    exit 2
fi

lock=$(cd -- "$(dirname -- "$0")/.." && pwd)/toolchain.lock.json
if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
    printf 'The pinned toolchain requires Linux x86_64.\n' >&2
    exit 1
fi
mkdir -- "$1"
cd -- "$1"

fetch() {
    local url checksum archive
    url=$(jq -er --arg tool "$1" '.[$tool].archive_url' "$lock")
    checksum=$(jq -er --arg tool "$1" '.[$tool].archive_sha256' "$lock")
    archive=${url##*/}
    curl --fail --silent --show-error --location --retry 2 "$url" --output "$archive"
    printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check >&2
    printf '%s\n' "$archive"
}

blender_archive=$(fetch blender)
tar -xf "$blender_archive"
blender=${blender_archive%.tar.xz}/blender

godot_archive=$(fetch godot)
unzip -q "$godot_archive"
godot=${godot_archive%.zip}

printf 'Blender: %s/%s\nGodot: %s/%s\n' "$PWD" "$blender" "$PWD" "$godot"
