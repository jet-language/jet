#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: structured_data INPUT_FILE' >&2
  exit 2
}

if ! output=$(jq -c '
  if (.events | type) != "array" then error("events")
  else {
    total_events: (.events | length),
    summaries: (.events
      | sort_by(.service)
      | group_by(.service)
      | map({service: .[0].service, count: length, total_ms: (map(.duration_ms) | add)}))
  }
  end
' "$1"); then
  printf '%s\n' 'invalid-json'
else
  printf '%s\n' "$output"
fi
