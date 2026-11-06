#!/usr/bin/env bash
# norust.sh CMD... : run CMD with every PATH directory that holds cargo, rustc or
# rustup removed (jet-env keeps rustc for user `extern rust`/RustBridge builds;
# the compiler chain must not need it). Refuses if any of them stays reachable.
set -uo pipefail
kept=""
IFS=: read -r -a dirs <<< "$PATH"
for d in "${dirs[@]}"; do
  skip=""
  for b in cargo rustc rustup; do [ -x "$d/$b" ] && skip=1; done
  [ -n "$skip" ] && { echo "norust: dropped PATH entry $d" >&2; continue; }
  kept="$kept${kept:+:}$d"
done
export PATH="$kept"
for b in cargo rustc rustup; do
  if command -v "$b" >/dev/null 2>&1; then echo "norust: REFUSE $b still on PATH" >&2; exit 2; fi
done
exec "$@"
