#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: document_markdown_inspection INPUT_ROOT' >&2; exit 2; }

find "$1" -type f -print | sort | while IFS= read -r file; do
  relative=${file#"$1"/}
  read -r headings bullets malformed < <(awk '
    /^#/ {
      heading = $0
      sub(/^[[:space:]]+/, "", heading)
      sub(/[[:space:]]+$/, "", heading)
      if (heading == "#") malformed = 1
      else headings += 1
    }
    /^- / { bullets += 1 }
    END { printf "%d %d %d\n", headings + 0, bullets + 0, malformed + 0 }
  ' "$file")
  if [[ "$malformed" -eq 1 ]]; then
    printf 'reject|%s|empty-heading\n' "$relative"
  else
    printf 'document|%s|headings=%s|bullets=%s\n' "$relative" "$headings" "$bullets"
  fi
done | sort
