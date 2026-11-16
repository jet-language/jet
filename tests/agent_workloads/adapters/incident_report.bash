#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: incident_report INPUT_FILE' >&2
  exit 2
}

services=()
statuses=()
unique_services=()
rejects=()
line_number=0
while IFS= read -r line || [[ -n "$line" ]]; do
  ((line_number += 1))
  ((line_number == 1)) && continue
  [[ -z "$line" ]] && continue
  rest=$line
  field_count=1
  while [[ $rest == *$'\t'* ]]; do
    rest=${rest#*$'\t'}
    ((field_count += 1))
  done
  if ((field_count != 3)); then
    rejects+=("reject|$line_number|field-count")
  else
    service=${line%%$'\t'*}
    rest=${line#*$'\t'}
    status=${rest%%$'\t'*}
    if [[ "$status" != ok && "$status" != error ]]; then
      rejects+=("reject|$line_number|status")
    elif [[ -z "$service" ]]; then
      rejects+=("reject|$line_number|service")
    else
      services+=("$service")
      statuses+=("$status")
      seen=false
      for known in "${unique_services[@]}"; do
        [[ "$known" == "$service" ]] && seen=true
      done
      $seen || unique_services+=("$service")
    fi
  fi
done < "$1"

for ((i = 0; i < ${#unique_services[@]}; i += 1)); do
  for ((j = i + 1; j < ${#unique_services[@]}; j += 1)); do
    if [[ ${unique_services[j]} < ${unique_services[i]} ]]; then
      swap=${unique_services[i]}
      unique_services[i]=${unique_services[j]}
      unique_services[j]=$swap
    fi
  done
done

printf 'accepted|%d\n' "${#services[@]}"
printf 'rejected|%d\n' "${#rejects[@]}"
for reject in "${rejects[@]}"; do
  printf '%s\n' "$reject"
done
for service in "${unique_services[@]}"; do
  ok=0
  errors=0
  for ((i = 0; i < ${#services[@]}; i += 1)); do
    if [[ ${services[i]} == "$service" ]]; then
      if [[ ${statuses[i]} == ok ]]; then
        ((ok += 1))
      else
        ((errors += 1))
      fi
    fi
  done
  printf '%s|ok=%d|error=%d\n' "$service" "$ok" "$errors"
done
