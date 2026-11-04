#!/usr/bin/env bash
# SP05c (#4530/#4533): whole-compiler isocheck of the latest SP05c snapshot
# (~/.cache/jet-dev/scratch/SP05c/latest: sp05b.patch + the SP05c items named
# in its ITEMS file, on master).
set -e
S="$HOME/.cache/jet-dev/scratch/SP05c/latest"
cd "$S"
echo "== items: $(cat ITEMS)"
out=$(flock -o "$HOME/.cache/jet-dev/iso-priority.lock" env ISOCHECK_BASE="$S" "$HOME/.cache/jet-dev/isocheck-priority.sh" SP05c)
echo "$out"
echo "$out" | head -1 | grep -q 'rc=0 errors=0'
