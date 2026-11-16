#!/usr/bin/env bash
# A stand-in for Main's capped.sh in the integrate10 fixture: it runs the command without a cap. The `jet`
# a golden run names is $JET, the batch compiler (the fixture's relative bin/jet link does not resolve).
shift
if [ "${1:-} ${2:-} ${3:-} ${4:-}" = "nice -n 19 jet" ]; then shift 4; exec "$JET" "$@"; fi
exec "$@"
