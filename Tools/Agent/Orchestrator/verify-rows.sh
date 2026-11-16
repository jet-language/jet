#!/usr/bin/env bash
# verify-rows.sh ROWS OUTDIR [JET]: runs every "criterion<TAB>profile<TAB>filter" row of ROWS as
# `jet test Compiler --profile P --filter F` from the dev worktree, six at a time on CPUs 0-11, and prints
# one line per row: criterion, profile, filter, PASS count, FAIL count, rc. Used to prove "already met" claims.
# The original of gauntlet tool entry verify-rows; its Jet twin is VerifyRows.jet beside it, the program in use, and
# this file stays as the entry's reference.
set -u
ROWS=${1:?rows}; OUT=${2:?outdir}; JETBIN=${3:-jet}
. "$(dirname "$0")/../roots.sh"
W=$JET_DEV_WORKTREE
# A JET given as a relative path still names it after the cd into the worktree.
case $JETBIN in /*) ;; */*) JETBIN=$PWD/$JETBIN ;; esac
mkdir -p "$OUT"
export OUT JETBIN W
one() {
  c=$1; p=$2; f=$3
  o="$OUT/$c.$p.$f.out"
  ( cd "$W" && timeout 600 "$JETBIN" test Compiler --profile "$p" --filter "$f" ) > "$o" 2>&1
  rc=$?
  line=$(awk '/^\{"PASS"/' "$o" | awk 'END{print}')
  pass=$(printf '%s' "$line" | awk -F'"PASS":' '{split($2,a,","); print a[1]+0}')
  fail=$(printf '%s' "$line" | awk -F'"FAIL":' '{split($2,a,","); print a[1]+0}')
  printf '%s\t%s\t%s\tPASS=%s\tFAIL=%s\trc=%s\n' "$c" "$p" "$f" "${pass:-0}" "${fail:-0}" "$rc"
}
export -f one
awk -F'\t' 'NF>=3{print $1, $2, $3}' "$ROWS" | taskset -c 0-11 xargs -P 6 -L 1 bash -c 'one "$0" "$1" "$2"' | sort -V
