#!/usr/bin/env bash
# Time test binaries independently, one named verification set at a time.
# The in-process guard remains the authority: every binary runs on the 900s
# default unless tests/suite_budgets.txt names it (#677), and this script sets
# the external `timeout` from that same table so a committed row is not clipped
# by a shorter kill.
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

# The committed per-suite budget table (#677). The guard's default is 900s; a
# suite gets more only through a row here, with its reason. This script keeps no
# copy of those numbers, for the same reason it keeps no copy of the set
# membership above (AGENTS.md I8).
budgets="tests/suite_budgets.txt"

# JET_TEST_DEADLINE_SECS may only TIGHTEN, in the guard and here: an exported
# deadline is granted to all 290 targets at once, which is the drift the table
# exists to stop. Leave it unset for the committed budgets.
override="${JET_TEST_DEADLINE_SECS:-}"
# An absolute external timeout, when the caller wants one. Otherwise it is
# derived per target as budget + 100s, the slack this script has always used.
slack="${JET_TEST_TIMEOUT_SECS:-}"
if [ -n "$override" ]; then
  case "$override" in
    ''|*[!0-9]*) echo "error: JET_TEST_DEADLINE_SECS must be a positive integer" >&2; exit 64 ;;
  esac
  if [ "$override" -lt 1 ] || [ "$override" -gt 900 ]; then
    echo "error: JET_TEST_DEADLINE_SECS may only tighten, so it must be 1..900, got $override. A longer deadline is a committed row in $budgets with its reason (#677)." >&2
    exit 64
  fi
fi
if [ -n "$slack" ]; then
  case "$slack" in
    ''|*[!0-9]*) echo "error: JET_TEST_TIMEOUT_SECS must be a positive integer" >&2; exit 64 ;;
  esac
fi

if [ ! -f "$budgets" ]; then
  echo "error: $budgets is missing — it is the only table of per-suite guard budgets" >&2
  exit 1
fi

# One target's committed budget, or empty for the default. Same shape as
# ledger_rows below: the file is parsed, never mirrored.
budget_for() {
  awk -v want="$1" '
    /^[[:space:]]*#/ { next }
    /^[[:space:]]*$/ { next }
    $1 == want { print $2; exit }
  ' "$budgets"
}

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

echo "time-suites: set '$selection' — ${#rows[@]} target(s), guard 900s unless $budgets names them"

status=0
for row in "${rows[@]}"; do
  read -r -a args <<<"$(cargo_args_for "$row")"
  # The target name is the binary name, which is the spelling the budget table
  # and the guard both use.
  budget="$(budget_for "${row##*/}")"
  budget="${budget:-900}"
  budget_source="$budgets"
  if [ "$budget" -eq 900 ]; then
    budget_source="default"
  fi
  if [ -n "$override" ] && [ "$override" -lt "$budget" ]; then
    budget="$override"
    budget_source="JET_TEST_DEADLINE_SECS"
  fi
  external="${slack:-$((budget + 100))}"
  if [ "$external" -lt "$budget" ]; then
    echo "error: JET_TEST_TIMEOUT_SECS ($external) does not cover ${row##*/}'s ${budget}s guard budget" >&2
    exit 64
  fi
  echo "time-suites: $row (guard ${budget}s from ${budget_source}, timeout ${external}s)"
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
