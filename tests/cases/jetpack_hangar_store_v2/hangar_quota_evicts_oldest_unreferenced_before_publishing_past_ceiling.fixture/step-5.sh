#!/bin/sh
# rs:1764-1772: ceiling = logical footprint - 2 MiB, written to the root's config/hangar-max-bytes.
set -eu
before=$(find root/hangar ! -type d -exec stat -c %s {} + | awk '{ s += $1 } END { print s + 0 }')
limit=$((before - 2097152))
test "$before" -gt "$limit"
mkdir -p root/config
printf '%s\n' "$limit" > root/config/hangar-max-bytes
printf '%s\n' "$limit" > limit
