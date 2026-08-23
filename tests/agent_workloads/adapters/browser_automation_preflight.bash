#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: browser_automation_preflight INPUT_FILE' >&2; exit 2; }

while IFS=$'\t' read -r operation value || [[ -n "$operation$value" ]]; do
  [[ -z "$operation" ]] && continue
  [[ "$operation" == operation ]] && continue
  case "$operation:$value" in
    profile:bidi-2025.5|profile:bidi-2024.11|timeout:500)
      printf '%s|%s|accepted\n' "$operation" "$value" ;;
    connect:*)
      printf '%s|%s|rejected\n' "$operation" "$value" ;;
    profile:*|timeout:*)
      printf '%s|%s|rejected\n' "$operation" "$value" ;;
    *)
      printf 'unknown browser operation %s\n' "$operation" >&2
      exit 2 ;;
  esac
done < "$1"
