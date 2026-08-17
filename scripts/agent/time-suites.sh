#!/usr/bin/env bash
# Time test binaries independently, one named verification set at a time.
# The in-process guard remains the authority: no binary may run past 900s.
#
# #2025: the set names and their members come from tests/suites.txt and nowhere
# else. This script keeps no list of its own, so a target cannot be in a set the
# runner does not know, and a set cannot exist that the runner cannot run.
# tests/suite_membership.rs proves every declared target has a row there, and
# `the_suite_runner_reads_the_ledger` proves this file still reads it.
#
# usage:
#   time-suites.sh                 every executable set, in ledger order
#   time-suites.sh --set NAME      one named set (NAME=all is the union)
#   time-suites.sh TARGET...       ad-hoc root test targets, by name
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo"

ledger="tests/suites.txt"
# The one non-executable section: its members are parked BECAUSE they cannot run
# on a normal host, so `all` must skip them rather than fail on them.
parked="host_gated"

budget="${JET_TEST_DEADLINE_SECS:-900}"
external="${JET_TEST_TIMEOUT_SECS:-1000}"
case "$budget" in
  ''|*[!0-9]*) echo "error: JET_TEST_DEADLINE_SECS must be a positive integer" >&2; exit 64 ;;
esac
case "$external" in
  ''|*[!0-9]*) echo "error: JET_TEST_TIMEOUT_SECS must be a positive integer" >&2; exit 64 ;;
esac
if [ "$budget" -lt 1 ] || [ "$budget" -gt 900 ]; then
  echo "error: test-binary budget must be between 1 and 900 seconds, got $budget" >&2
  exit 64
fi
if [ "$external" -lt "$budget" ]; then
  echo "error: external timeout ($external) must cover the $budget-second test guard" >&2
  exit 64
fi

if [ ! -f "$ledger" ]; then
  echo "error: $ledger is missing — it is the only list of verification sets" >&2
  exit 1
fi

ledger_sets() {
  awk '/^[a-z_]+:$/ { print substr($0, 1, length($0) - 1) }' "$ledger"
}

# Rows of one section, or of every executable section when want=all.
ledger_rows() {
  awk -v want="$1" -v parked="$parked" '
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*$/ { next }
    /^[a-z_]+:$/ { section = substr($0, 1, length($0) - 1); next }
    section == "" { next }
    section == parked { next }
    want != "all" && section != want { next }
    { row = $1; sub(/:$/, "", row); print row }
  ' "$ledger"
}

# One row of the ledger becomes one cargo test invocation. The package of a
# crate target is read from its own Cargo.toml, never guessed from the directory.
cargo_args_for() {
  local row="$1" rest crate name pkg
  case "$row" in
    tests/*)
      printf -- '-p jet --test %s' "${row#tests/}"
      ;;
    crates/*/tests/*)
      rest="${row#crates/}"
      crate="${rest%%/*}"
      name="${row##*/}"
      pkg="$(awk -F'"' '
        /^\[package\]/ { in_pkg = 1; next }
        /^\[/ { in_pkg = 0 }
        in_pkg && /^name[[:space:]]*=/ { print $2; exit }
      ' "crates/$crate/Cargo.toml")"
      if [ -z "$pkg" ]; then
        echo "error: crates/$crate/Cargo.toml declares no package name" >&2
        return 1
      fi
      printf -- '-p %s --test %s' "$pkg" "$name"
      ;;
    *)
      echo "error: $ledger row '$row' is not a tests/<name> or crates/<crate>/tests/<name> path" >&2
      return 1
      ;;
  esac
}

selection="all"
rows=()
if [ "$#" -gt 0 ] && [ "$1" = "--set" ]; then
  if [ "$#" -ne 2 ]; then
    echo "usage: time-suites.sh --set NAME" >&2
    exit 64
  fi
  selection="$2"
  if [ "$selection" = "$parked" ]; then
    echo "error: '$parked' is not executable — its rows name targets that cannot run here" >&2
    exit 64
  fi
  if [ "$selection" != "all" ] && ! ledger_sets | grep -qx -- "$selection"; then
    echo "error: '$selection' is not a set in $ledger. Sets: $(ledger_sets | paste -sd' ')" >&2
    exit 64
  fi
  mapfile -t rows < <(ledger_rows "$selection")
elif [ "$#" -gt 0 ]; then
  selection="ad-hoc"
  for target in "$@"; do
    rows+=("tests/$target")
  done
else
  mapfile -t rows < <(ledger_rows all)
fi

if [ "${#rows[@]}" -eq 0 ]; then
  echo "error: set '$selection' selected no test targets" >&2
  exit 1
fi

echo "time-suites: set '$selection' — ${#rows[@]} target(s), guard ${budget}s"

status=0
for row in "${rows[@]}"; do
  read -r -a args <<<"$(cargo_args_for "$row")"
  echo "time-suites: $row (guard ${budget}s)"
  started=$SECONDS
  target_status=0
  if JET_TEST_DEADLINE_SECS="$budget" timeout "${external}s" \
    scripts/agent/jet-env cargo test "${args[@]}"; then
    :
  else
    target_status=$?
  fi
  elapsed=$((SECONDS - started))
  echo "time-suites: $row elapsed=${elapsed}s status=${target_status}"
  if [ "$target_status" -ne 0 ] && [ "$status" -eq 0 ]; then
    status="$target_status"
  fi
done
exit "$status"
