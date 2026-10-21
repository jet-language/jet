#!/usr/bin/env bash
# #2075: shared weighted LPT assignment.
#
# Read tab-separated rows from stdin: <weight>\t<item>. Emit items assigned to
# SHARD_INDEX. The item may contain tabs; the last read field keeps the rest.
# Keeping assignment here gives cargo targets and corpus stems one sharding
# mechanism instead of two subtly different copies.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: weighted-shards.sh SHARD_INDEX SHARD_COUNT" >&2
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

ordered="$(
  while IFS=$'\t' read -r raw_weight item; do
    if [ -z "$raw_weight" ] || [ -z "$item" ]; then
      echo "error: weighted shard input rows must be '<weight>\\t<item>'" >&2
      exit 65
    fi
    case "$raw_weight" in
      *[!0-9]*) echo "error: weighted shard input has non-integer weight '$raw_weight'" >&2; exit 65 ;;
    esac
    weight=$((10#$raw_weight))
    printf '%012d\t%s\n' "$weight" "$item"
  done | sort -t$'\t' -k1,1nr -k2,99
)"

loads=()
for ((shard = 0; shard < shard_count; shard++)); do
  loads[shard]=0
done
selected=0
selected_seconds=0
total=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  total=$((total + 1))
  seconds=$((10#${line%%$'\t'*}))
  item="${line#*$'\t'}"
  lightest=0
  for ((shard = 1; shard < shard_count; shard++)); do
    if [ "${loads[shard]}" -lt "${loads[lightest]}" ]; then
      lightest="$shard"
    fi
  done
  loads[lightest]=$((loads[lightest] + seconds))
  if [ "$lightest" -eq "$shard_index" ]; then
    printf '%s\n' "$item"
    selected=$((selected + 1))
    selected_seconds=$((selected_seconds + seconds))
  fi
done <<<"$ordered"

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
echo "weighted-shards: shard $shard_index/$shard_count selected $selected of $total items, predicted ${selected_seconds}s (shard loads: ${loads[*]}; spread ${heaviest}s/${lightest_load}s = $spread)" >&2
