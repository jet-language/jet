#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: process_batch INPUT_FILE' >&2
  exit 2
}

temp_files=()
child=
watchdog=
cleanup() {
  if [[ -n $child ]]; then
    kill -KILL "$child" 2>/dev/null || true
    wait "$child" 2>/dev/null || true
  fi
  if [[ -n $watchdog ]]; then
    kill -KILL "$watchdog" 2>/dev/null || true
    wait "$watchdog" 2>/dev/null || true
  fi
  ((${#temp_files[@]} == 0)) || rm -f -- "${temp_files[@]}"
}
trap cleanup EXIT

line_number=0
while IFS= read -r line || [[ -n "$line" ]]; do
  ((line_number += 1))
  ((line_number == 1)) && continue
  [[ -z "$line" ]] && continue

  rest=$line
  fields=()
  while [[ $rest == *$'\t'* ]]; do
    fields+=("${rest%%$'\t'*}")
    rest=${rest#*$'\t'}
  done
  fields+=("$rest")
  ((${#fields[@]} == 4)) || {
    printf 'bad process row %d\n' "$line_number" >&2
    exit 2
  }

  label=${fields[0]}
  program=${fields[1]}
  arguments_text=${fields[2]}
  arguments=()
  rest=$arguments_text
  while [[ $rest == *'|'* ]]; do
    arguments+=("${rest%%'|'*}")
    rest=${rest#*'|'}
  done
  arguments+=("$rest")
  timeout_ms=${fields[3]}
  [[ $timeout_ms =~ ^[0-9]+$ ]] || {
    printf 'bad timeout\n' >&2
    exit 2
  }
  printf -v timeout_seconds '%d.%03d' \
    "$((timeout_ms / 1000))" "$((timeout_ms % 1000))"

  output_file=".process_batch_output.$line_number"
  timeout_file=".process_batch_timeout.$line_number"
  temp_files+=("$output_file" "$timeout_file")
  "$program" "${arguments[@]}" >"$output_file" 2>/dev/null &
  child=$!
  (
    sleep "$timeout_seconds"
    if kill -0 "$child" 2>/dev/null; then
      : >"$timeout_file"
      kill -KILL "$child" 2>/dev/null || true
    fi
  ) &
  watchdog=$!

  if wait "$child" 2>/dev/null; then
    code=0
  else
    code=$?
  fi
  child=
  kill -KILL "$watchdog" 2>/dev/null || true
  wait "$watchdog" 2>/dev/null || true
  watchdog=

  if [[ -e $timeout_file ]]; then
    printf '%s|timeout\n' "$label"
  else
    output=$(<"$output_file")
    printf '%s|exit=%d|stdout=%s\n' "$label" "$code" "$output"
  fi
done <"$1"
