#!/usr/bin/env bash
# #211 (D-CI1=A): complete cargo test-target inventory, sharded across CI jobs.
#
# Enumerates every cargo test target in the workspace from `cargo metadata`
# (lib/bin/test kinds — the only kinds that can hold `#[test]` fns) and
# assigns each one to a shard. Nothing here names a test file or package, so a
# new test target lands in a shard automatically on the next run — it can never
# silently fall out of every shard the way the old default-members-only
# `cargo test` silently skipped every workspace crate except ".", "jet-driver",
# "jetpack-bin", "jetos".
#
# #2075: the assignment is weighted, not round-robin. A Cargo target is atomic,
# so a 45-minute `golden` stayed a 45-minute shard while five other jobs idled —
# round-robin balances the COUNT of targets and ignores their cost. This script
# reads measured seconds from `tools/ci/test-weights.tsv` and assigns heaviest
# first to the currently-lightest shard (LPT, the standard greedy makespan
# heuristic: deterministic, and never worse than 4/3 of optimal). Targets with no
# measured row take DEFAULT_WEIGHT, so an unmeasured target is scheduled, never
# skipped.
#
# Output: one cargo-test argument line per selected target, e.g.
#   -p jet-lexer --lib
#   -p jet --test golden
# Callers loop over stdout and run `cargo test $line` per line. The stderr
# summary reports this shard's predicted seconds and the spread across all
# shards, which is the number that says whether the split is honest yet.
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

# Seconds charged to a target with no measured row. Deliberately small: an
# unmeasured target is assumed cheap, so one unmeasured monster shows up as a
# skewed real CI run (visible, fixable by measuring it) rather than as a
# guessed weight nobody ever revisits.
DEFAULT_WEIGHT=30
weights_file="tools/ci/test-weights.tsv"

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

# The weight table, keyed by the exact inventory row. A malformed row is a hard
# failure: a silently ignored weight is a shard split that lies about its cost.
declare -A weight=()
if [ -f "$weights_file" ]; then
  line_number=0
  while IFS= read -r line || [ -n "$line" ]; do
    line_number=$((line_number + 1))
    case "$line" in
      ''|'#'*) continue ;;
    esac
    IFS=$'\t' read -r w_pkg w_kind w_name w_secs w_extra <<<"$line"
    if [ -z "$w_pkg" ] || [ -z "$w_kind" ] || [ -z "$w_name" ] || [ -n "$w_extra" ]; then
      echo "error: $weights_file:$line_number must be '<package>\t<kind>\t<target>\t<seconds>'" >&2
      exit 65
    fi
    case "$w_secs" in
      ''|*[!0-9]*) echo "error: $weights_file:$line_number has non-integer seconds '$w_secs'" >&2; exit 65 ;;
    esac
    key="$w_pkg"$'\t'"$w_kind"$'\t'"$w_name"
    if [ -n "${weight[$key]+set}" ]; then
      echo "error: $weights_file:$line_number repeats '$w_pkg $w_kind $w_name'" >&2
      exit 65
    fi
    weight["$key"]="$w_secs"
  done <"$weights_file"
fi

# Heaviest first (weight descending, then the row itself ascending so equal
# weights — every unmeasured target — keep a stable, reviewable order).
ordered="$(
  for row in "${all_targets[@]}"; do
    printf '%012d\t%s\n' "${weight[$row]:-$DEFAULT_WEIGHT}" "$row"
  done | sort -t$'\t' -k1,1nr -k2,4
)"

loads=()
for ((shard = 0; shard < shard_count; shard++)); do
  loads[shard]=0
done
selected_lines=()
selected_seconds=0
total=0
while IFS= read -r line; do
  total=$((total + 1))
  seconds=$((10#${line%%$'\t'*}))
  row="${line#*$'\t'}"
  lightest=0
  for ((shard = 1; shard < shard_count; shard++)); do
    if [ "${loads[shard]}" -lt "${loads[lightest]}" ]; then
      lightest="$shard"
    fi
  done
  loads[lightest]=$((loads[lightest] + seconds))
  if [ "$lightest" -ne "$shard_index" ]; then
    continue
  fi
  selected_seconds=$((selected_seconds + seconds))
  IFS=$'\t' read -r pkg kind name <<<"$row"
  case "$kind" in
    lib) selected_lines+=("-p $pkg --lib") ;;
    bin) selected_lines+=("-p $pkg --bin $name") ;;
    test) selected_lines+=("-p $pkg --test $name") ;;
  esac
done <<<"$ordered"

# Emitted sorted: the shard's contents are a set, and a stable order keeps two
# runs of the same shard diffable.
if [ "${#selected_lines[@]}" -gt 0 ]; then
  printf '%s\n' "${selected_lines[@]}" | sort
fi

heaviest="${loads[0]}"
lightest_load="${loads[0]}"
for ((shard = 1; shard < shard_count; shard++)); do
  if [ "${loads[shard]}" -gt "$heaviest" ]; then heaviest="${loads[shard]}"; fi
  if [ "${loads[shard]}" -lt "$lightest_load" ]; then lightest_load="${loads[shard]}"; fi
done
if [ "$lightest_load" -gt 0 ]; then
  hundredths=$((heaviest * 100 / lightest_load))
  spread="$(printf '%d.%02dx' "$((hundredths / 100))" "$((hundredths % 100))")"
else
  spread="n/a"
fi
echo "test-shards: shard $shard_index/$shard_count selected ${#selected_lines[@]} of $total workspace test targets, predicted ${selected_seconds}s (shard loads: ${loads[*]}; spread ${heaviest}s/${lightest_load}s = $spread)" >&2
