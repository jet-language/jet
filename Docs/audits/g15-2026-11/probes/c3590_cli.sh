#!/usr/bin/env bash
# #3590 c7, c8 and c11: `jet add`, `jet fetch` and `jet build --locked` in a fresh `jet new` project.
# Usage: c3590_cli.sh EMPTY_DIR   (uses `jet` from PATH, or $JET when set)
set -u
jet=${JET:-jet}
dir=${1:?usage: c3590_cli.sh EMPTY_DIR}
cd "$dir" || exit 2
"$jet" new newp || exit 2
cd newp || exit 2
for cmd in "add textkit" "fetch" "build --locked"; do
  # shellcheck disable=SC2086
  "$jet" $cmd
  echo "jet $cmd -> exit $?"
done
