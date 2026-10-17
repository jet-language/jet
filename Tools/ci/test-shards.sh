#!/usr/bin/env bash
# Native test-suite inventory, sharded across CI jobs.
#
# The complete native suite set is fixed here: golden, ui, cases, registries.
# Each suite is one row of the inventory; the runner that executes a suite owns
# its contents. A new suite must be added to `suites` below, which is the single
# place that decides what CI can shard.
#
# Assignment goes through the shared weighted LPT engine (weighted-shards.sh).
# Suites have no measured cost yet, so every suite carries DEFAULT_WEIGHT and the
# split is an even count. Measured per-suite seconds, when recorded, slot in as
# the weight column without changing the engine.
#
# Output: one suite ID per line, sorted, for the selected shard. Callers loop
# over stdout and run each suite. The engine's stderr summary reports predicted
# load and spread.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: test-shards.sh SHARD_INDEX SHARD_COUNT" >&2
  exit 64
fi
shard_index="$1"
shard_count="$2"

case "$shard_count" in
  ''|*[!0-9]*) echo "error: SHARD_COUNT must be a positive integer, got '$shard_count'" >&2; exit 64 ;;
esac
if [ "$shard_count" -lt 1 ]; then
  echo "error: SHARD_COUNT must be >= 1, got '$shard_count'" >&2
  exit 64
fi
case "$shard_index" in
  ''|*[!0-9]*) echo "error: SHARD_INDEX must be a non-negative integer, got '$shard_index'" >&2; exit 64 ;;
esac
if [ "$shard_index" -ge "$shard_count" ]; then
  echo "error: SHARD_INDEX ($shard_index) must be < SHARD_COUNT ($shard_count)" >&2
  exit 64
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo"

# Seconds charged to a suite with no measured cost.
DEFAULT_WEIGHT=30

suites=(golden ui cases registries)

# Shared weighted LPT assignment: one "<weight>\t<suite>" row per suite.
selected="$(
  for suite in "${suites[@]}"; do
    printf '%s\t%s\n' "$DEFAULT_WEIGHT" "$suite"
  done | bash "$repo/Tools/ci/weighted-shards.sh" "$shard_index" "$shard_count"
)"

# Emitted sorted: the shard's contents are a set, and a stable order keeps two
# runs of the same shard diffable.
if [ -n "$selected" ]; then
  printf '%s\n' "$selected" | sort
fi
