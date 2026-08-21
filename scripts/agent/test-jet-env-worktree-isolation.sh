#!/usr/bin/env bash
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=$(CDPATH= cd -- "$script_dir/../.." && pwd)

set +e
output=$(env -u CARGO_TARGET_DIR CARGO_TARGET_DIR="$repo/.." "$script_dir/jet-env" true 2>&1)
status=$?
set -e
[ "$status" -eq 64 ] || {
  printf 'external target: expected rc=64, got rc=%s\n%s\n' "$status" "$output" >&2
  exit 1
}
printf '%s\n' "$output" | grep -Fq 'CARGO_TARGET_DIR=' || {
  printf 'external target: missing target-dir diagnostic\n%s\n' "$output" >&2
  exit 1
}

actual=$(env -u CARGO_TARGET_DIR "$script_dir/jet-env" sh -c 'printf %s "$CARGO_TARGET_DIR"')
[ "$actual" = "$repo/target" ] || {
  printf 'default target: expected %s/target, got %s\n' "$repo" "$actual" >&2
  exit 1
}

printf 'jet-env worktree isolation checks passed\n'
