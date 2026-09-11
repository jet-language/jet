#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: browser_automation_preflight INPUT_FILE' >&2; exit 2; }

# The peer contract exposes parsing/profile support, but no browser transport.
available_capabilities=(profile timeout)

has_capability() {
  local requested=$1 capability
  for capability in "${available_capabilities[@]}"; do
    [[ "$capability" == "$requested" ]] && return 0
  done
  return 1
}

supports_profile() {
  case "$1" in
    bidi-2025.5|bidi-2024.11) return 0 ;;
    *) return 1 ;;
  esac
}

valid_timeout() {
  local raw=$1 sign digits
  raw="${raw#"${raw%%[![:space:]]*}"}"
  raw="${raw%"${raw##*[![:space:]]}"}"
  [[ "$raw" =~ ^([+-]?)([0-9]+)$ ]] || return 1
  sign=${BASH_REMATCH[1]}
  [[ "$sign" != "-" ]] || return 1
  digits=${BASH_REMATCH[2]}
  while [[ ${#digits} -gt 1 && ${digits:0:1} == 0 ]]; do
    digits=${digits:1}
  done
  [[ ${#digits} -le 6 ]] || return 1
  (( 10#$digits >= 1 && 10#$digits <= 600000 ))
}

line_number=0
while IFS= read -r line || [[ -n "$line" ]]; do
  ((line_number += 1))
  [[ -z "$line" ]] && continue
  [[ "$line" == *$'\r' ]] && line=${line%$'\r'}
  [[ "$line" == $'operation\tvalue' ]] && continue
  if [[ "$line" != *$'\t'* ]]; then
    printf 'malformed browser row %d\n' "$line_number" >&2
    exit 2
  fi
  operation=${line%%$'\t'*}
  value=${line#*$'\t'}
  if [[ "$value" == *$'\t'* ]]; then
    printf 'malformed browser row %d\n' "$line_number" >&2
    exit 2
  fi
  accepted=0
  case "$operation" in
    profile)
      if has_capability profile && supports_profile "$value"; then
        accepted=1
      fi
      ;;
    timeout)
      if has_capability timeout && valid_timeout "$value"; then
        accepted=1
      fi
      ;;
    connect)
      accepted=0
      ;;
    *)
      printf 'unknown browser operation %s\n' "$operation" >&2
      exit 2
      ;;
  esac
  if (( accepted )); then
    outcome=accepted
  else
    outcome=rejected
  fi
  printf '%s|%s|%s\n' "$operation" "$value" "$outcome"
done < "$1"
