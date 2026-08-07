#!/usr/bin/env bash
# #211 (D-CI1=A): complete cargo test-target inventory, sharded across CI jobs.
#
# Enumerates every cargo test target in the workspace from `cargo metadata`
# (lib/bin/test kinds — the only kinds that can hold `#[test]` fns) and
# assigns each one to a shard by simple round-robin. Nothing here names a
# test file or package, so a new test target lands in a shard automatically
# on the next run — it can never silently fall out of every shard the way
# the old default-members-only `cargo test` silently skipped every
# workspace crate except ".", "jet-driver", "jetpack-bin", "jetos".
#
# Output: one cargo-test argument line per selected target, e.g.
#   -p jet-lexer --lib
#   -p jet --test golden
# Callers loop over stdout and run `cargo test $line` per line.
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

# Sorted, deterministic full inventory: "<package>\t<kind>\t<target-name>".
# A plain assignment from a `set -o pipefail` pipeline (not `< <(...)`
# process substitution, whose failure `mapfile` would silently swallow) so a
# `cargo metadata`/jq failure aborts the script instead of reporting a
# quietly-empty inventory.
raw_targets="$(
  cargo metadata --no-deps --format-version 1 |
    jq -r '.packages[] | .name as $pkg | .targets[]
      | select(.kind[0] == "lib" or .kind[0] == "bin" or .kind[0] == "test")
      | "\($pkg)\t\(.kind[0])\t\(.name)"' |
    sort -u
)"
if [ -z "$raw_targets" ]; then
  echo "error: cargo metadata | jq reported zero lib/bin/test targets — the workspace always has some; treating this as a hard failure, not an empty shard" >&2
  exit 1
fi
mapfile -t all_targets <<<"$raw_targets"

total=${#all_targets[@]}
selected=0
for i in "${!all_targets[@]}"; do
  if [ "$((i % shard_count))" -ne "$shard_index" ]; then
    continue
  fi
  IFS=$'\t' read -r pkg kind name <<<"${all_targets[$i]}"
  case "$kind" in
    lib) printf -- '-p %s --lib\n' "$pkg" ;;
    bin) printf -- '-p %s --bin %s\n' "$pkg" "$name" ;;
    test) printf -- '-p %s --test %s\n' "$pkg" "$name" ;;
  esac
  selected=$((selected + 1))
done

echo "test-shards: shard $shard_index/$shard_count selected $selected of $total workspace test targets" >&2
