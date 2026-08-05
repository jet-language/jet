#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: build_test_recovery INPUT_ROOT' >&2
  exit 2
}

project=project
trap 'rm -rf -- "$project"' EXIT
cp -R -- "$1" "$project"

if bash -n "$project/invalid.sh" 2>/dev/null; then
  printf '%s\n' 'invalid source passed' >&2
  exit 1
fi
bash -n "$project/valid.sh"
result=$(bash "$project/valid.sh")
printf '%s\n' 'recovery=ok'
printf 'test=%s\n' "$result"
