#!/bin/sh
# rs:1775-1782: the Hangar is at or under the ceiling, the oldest object is gone, newer and admitted remain.
set -eu
limit=$(cat limit)
after=$(find root/hangar ! -type d -exec stat -c %s {} + | awk '{ s += $1 } END { print s + 0 }')
if [ "$after" -gt "$limit" ]; then
  echo "quota admitted $after logical bytes above ceiling $limit" >&2
  exit 1
fi
for name in quota-oldest quota-newer quota-admitted; do
  set -- root/hangar/"$name"-*
  if [ -e "$1" ]; then echo "$name present"; else echo "$name absent"; fi
done > presence
printf 'quota-oldest absent\nquota-newer present\nquota-admitted present\n' | cmp -s - presence
