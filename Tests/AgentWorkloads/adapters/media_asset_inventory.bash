#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: media_asset_inventory INPUT_ROOT' >&2; exit 2; }

find "$1" -type f -print | sort | while IFS= read -r file; do
  relative=${file#"$1"/}
  case "${relative##*.}" in
    ppm) type=image/x-portable-pixmap ;;
    svg) type=image/svg+xml ;;
    mp3) type=audio/mpeg ;;
    *) printf 'reject|%s|extension\n' "$relative"; continue ;;
  esac
  printf 'asset|%s|%s|%s\n' "$relative" "$type" "$(wc -c < "$file")"
done
