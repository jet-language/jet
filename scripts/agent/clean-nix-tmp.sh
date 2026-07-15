#!/usr/bin/env bash
# Clear exited `nix develop` tmp dirs before every new shell. They accumulate
# ~200M each and fill tmpfs, causing phantom ENOSPC test failures.
set -u

stale_minutes="${JET_NIX_TMP_STALE_MINUTES:-0}"
case "$stale_minutes" in
  '' | *[!0-9]*) stale_minutes=0 ;;
esac
uid="$(id -u)"

if [ -d /proc/self ] && stat -c %u /proc/self >/dev/null 2>&1; then
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

  for proc in /proc/[0-9]*; do
    [ -d "$proc" ] || continue
    proc_uid="$(stat -c %u "$proc" 2>/dev/null)" || continue
    [ "$proc_uid" = "$uid" ] || continue

    for link in "$proc/cwd" "$proc/root" "$proc"/fd/*; do
      target="$(readlink "$link" 2>/dev/null)" || continue
      mark_target "$target"
    done

    # Some sandboxes hide another same-user process's environment. Its cwd,
    # root, and open descriptors still protect any live shell path; never let
    # an unrelated unreadable environment block all stale cleanup.
    {
      while IFS= read -r -d '' value; do
        case "$value" in
          *'/tmp/nix-shell.'*) mark_target "/tmp/nix-shell.${value#*/tmp/nix-shell.}" ;;
        esac
      done <"$proc/environ"
    } 2>/dev/null || true
  done

  for candidate in /tmp/nix-shell.*; do
    [ -d "$candidate" ] || continue
    [ ! -L "$candidate" ] || continue
    [ "$(stat -c %u "$candidate" 2>/dev/null)" = "$uid" ] || continue
    if [ "$stale_minutes" -gt 0 ]; then
      find "$candidate" -maxdepth 0 -mmin "+$stale_minutes" -print -quit 2>/dev/null \
        | grep -q . || continue
    fi
    [ -z "${live[$candidate]:-}" ] || continue
    rm -rf -- "$candidate" 2>/dev/null
  done
fi

use=$(df --output=pcent /tmp 2>/dev/null | tail -1 | tr -dc '0-9')
if [ -n "$use" ] && [ "$use" -ge 80 ]; then
  echo "WARNING: /tmp at ${use}% full — treat test failures as possible phantom ENOSPC; check df -h /tmp first."
fi
exit 0
