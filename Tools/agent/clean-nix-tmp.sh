#!/usr/bin/env bash
# Clear exited `nix develop` tmp dirs before every new shell. They accumulate
# ~200M each and fill tmpfs, causing phantom ENOSPC test failures.
#
# Perf contract (2026-07-24): this runs synchronously in every dev shellHook,
# so it must stay fast. Liveness scanning uses three batch passes (find over
# /proc symlinks, one environ sweep) — never one fork per fd; a machine with
# ~250k open fds made the old per-fd `readlink` loop take minutes per shell.
set -u

stale_minutes="${JET_NIX_TMP_STALE_MINUTES:-0}"
case "$stale_minutes" in
  '' | *[!0-9]*) stale_minutes=0 ;;
esac
uid="$(id -u)"

# Collect candidate dirs first; with none there is nothing to scan.
candidates=()
for candidate in /tmp/nix-shell.*; do
  [ -d "$candidate" ] || continue
  [ ! -L "$candidate" ] || continue
  [ "$(stat -c %u "$candidate" 2>/dev/null)" = "$uid" ] || continue
  if [ "$stale_minutes" -gt 0 ]; then
    find "$candidate" -maxdepth 0 -mmin "+$stale_minutes" -print -quit 2>/dev/null \
      | grep -q . || continue
  fi
  candidates+=("$candidate")
done

if [ "${#candidates[@]}" -gt 0 ] && [ -d /proc/self ]; then
  declare -A live=()

  mark_target() {
    case "$1" in
      /tmp/nix-shell.*)
        rest="${1#/tmp/nix-shell.}"
        name="${rest%%/*}"
        live["/tmp/nix-shell.${name%%:*}"]=1
        ;;
    esac
  }

  # Pass 1: cwd/root/fd symlinks of every readable process, in one `find`.
  # The kernel hides other users' proc links, so this is same-uid by
  # construction; errors from unreadable processes are dropped.
  while IFS= read -r target; do
    mark_target "$target"
  done < <(
    find /proc/[0-9]*/cwd /proc/[0-9]*/root /proc/[0-9]*/fd \
      -maxdepth 1 -lname '/tmp/nix-shell.*' -printf '%l\n' 2>/dev/null
  )

  # Pass 2: every readable process environment, in one sweep. Some sandboxes
  # hide another same-user process's environment; its cwd, root, and open
  # descriptors still protect any live shell path; never let an unrelated
  # unreadable environment block all stale cleanup.
  while IFS= read -r target; do
    mark_target "$target"
  done < <(
    cat /proc/[0-9]*/environ 2>/dev/null \
      | tr '\0' '\n' \
      | grep -o '/tmp/nix-shell\.[^/:[:space:]]*' \
      | sort -u
  )

  for candidate in "${candidates[@]}"; do
    [ -z "${live[$candidate]:-}" ] || continue
    rm -rf -- "$candidate" 2>/dev/null
  done
fi

use=$(df --output=pcent /tmp 2>/dev/null | tail -1 | tr -dc '0-9')
if [ -n "$use" ] && [ "$use" -ge 80 ]; then
  echo "WARNING: /tmp at ${use}% full — treat test failures as possible phantom ENOSPC; check df -h /tmp first."
fi
exit 0
