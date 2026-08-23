#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: desktop_interaction_focus INPUT_FILE' >&2; exit 2; }

focus=(Save Cancel)
index=1
while IFS= read -r key || [[ -n "$key" ]]; do
  [[ -z "$key" || "$key" == key ]] && continue
  if [[ "$key" == Tab ]]; then
    printf 'focus|%s\n' "${focus[$index]}"
    index=$(( (index + 1) % 2 ))
  elif [[ "$key" == Empty ]]; then
    printf 'event|Empty|observed\n'
  else
    printf 'event|%s|observed\n' "$key"
  fi
done < "$1"
