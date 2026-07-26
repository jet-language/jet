#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: git_diff_review INPUT_ROOT' >&2
  exit 2
}

output_file=.git_diff_review_output
error_file=.git_diff_review_error
timeout_file=.git_diff_review_timeout
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
  rm -f -- "$output_file" "$error_file" "$timeout_file"
}
trap cleanup EXIT

git -C "$1" -c core.quotePath=false diff \
  --no-index --no-renames --name-status -- before after \
  >"$output_file" 2>"$error_file" &
child=$!
(
  sleep 5
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

[[ ! -e $timeout_file ]] || {
  printf '%s\n' 'git diff timed out' >&2
  exit 2
}
((code == 1)) || {
  error=$(<"$error_file")
  printf 'git diff exit %d: %s\n' "$code" "$error" >&2
  exit 2
}

added=0
deleted=0
modified=0
rows=()
while IFS= read -r line || [[ -n $line ]]; do
  [[ -n $line && $line == *$'\t'* ]] || {
    printf 'bad git name-status row: %s\n' "$line" >&2
    exit 2
  }
  status=${line%%$'\t'*}
  raw_path=${line#*$'\t'}
  [[ $raw_path != *$'\t'* ]] || {
    printf 'bad git name-status row: %s\n' "$line" >&2
    exit 2
  }
  case $raw_path in
    before/*) path=${raw_path#before/} ;;
    after/*) path=${raw_path#after/} ;;
    *)
      printf 'git path escaped roots: %s\n' "$raw_path" >&2
      exit 2
      ;;
  esac
  case $status in
    A)
      kind=added
      ((added += 1))
      ;;
    D)
      kind=deleted
      ((deleted += 1))
      ;;
    M)
      kind=modified
      ((modified += 1))
      ;;
    *)
      printf 'bad git name-status row: %s\n' "$line" >&2
      exit 2
      ;;
  esac
  rows+=("$path|$kind")
done <"$output_file"

for ((i = 1; i < ${#rows[@]}; i += 1)); do
  value=${rows[i]}
  j=$i
  while ((j > 0)) && [[ ${rows[j - 1]} > $value ]]; do
    rows[j]=${rows[j - 1]}
    ((j -= 1))
  done
  rows[j]=$value
done

printf '%s\n' "${rows[@]}"
printf 'summary|added=%d|modified=%d|deleted=%d\n' \
  "$added" "$modified" "$deleted"
