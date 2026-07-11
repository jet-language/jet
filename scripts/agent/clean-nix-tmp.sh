#!/usr/bin/env bash
# Clear stale `nix develop` tmp dirs — they accumulate ~200M each and fill the
# tmpfs, causing phantom ENOSPC test failures.
set -u

stale_minutes="${JET_NIX_TMP_STALE_MINUTES:-60}"
case "$stale_minutes" in
  '' | *[!0-9]*) stale_minutes=60 ;;
esac
uid="$(id -u)"

if [ -d /proc/self ] && stat -c %u /proc/self >/dev/null 2>&1; then
  declare -A live=()
  reliable=1

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

    if ! {
      while IFS= read -r -d '' value; do
        case "$value" in
          *'/tmp/nix-shell.'*) mark_target "/tmp/nix-shell.${value#*/tmp/nix-shell.}" ;;
        esac
      done <"$proc/environ"
    } 2>/dev/null; then
      [ -d "$proc" ] && reliable=0
    fi
  done

  if [ "$reliable" = "1" ]; then
    for candidate in /tmp/nix-shell.*; do
      [ -d "$candidate" ] || continue
      [ ! -L "$candidate" ] || continue
      [ "$(stat -c %u "$candidate" 2>/dev/null)" = "$uid" ] || continue
      find "$candidate" -maxdepth 0 -mmin "+$stale_minutes" -print -quit 2>/dev/null \
        | grep -q . || continue
      [ -z "${live[$candidate]:-}" ] || continue
      rm -rf -- "$candidate" 2>/dev/null
    done
  fi
fi

use=$(df --output=pcent /tmp 2>/dev/null | tail -1 | tr -dc '0-9')
if [ -n "$use" ] && [ "$use" -ge 80 ]; then
  echo "WARNING: /tmp at ${use}% full — treat test failures as possible phantom ENOSPC; check df -h /tmp first."
fi
exit 0
