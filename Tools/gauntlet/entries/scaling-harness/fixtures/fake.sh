#!/bin/sh
# A deterministic stand-in compiler for the scaling-harness cases, run as the
# gate's `command:` compiler `sh ../../../fake.sh KIND {n} {trace}` from the
# rung's project directory (BUILD_ROOT/<hash>/project). It writes a one-span
# JET_TRACE_FILE trace and prints `JET_PHASE_COUNTERS` lines on both streams,
# its work growing with N by KIND (above the gate's counter resolution, 64):
# linear (100 N), quadratic (100 N^2), broken
# (linear, but exits 3 from N=4), errors (reports two `Error [E…]`
# diagnostics and exits 1), malformed (an unparsable counter line) or negative
# (a negative counter).
kind=$1
n=$2
trace=$3
ms=$(date +%s%3N)
printf '[\n{"name":"trace.start","ph":"i","ts":0,"pid":1,"tid":0,"args":{"unix_ms":%s}},\n{"name":"parse","cat":"parse","ph":"B","ts":100,"pid":1,"tid":0},\n{"name":"parse","cat":"parse","ph":"E","ts":%s,"pid":1,"tid":0,"args":{"items":%s}},\n' "$ms" $((100 + n)) "$n" >"$trace"
case $kind in
quadratic) work=$((100 * n * n)) ;;
*) work=$((100 * n)) ;;
esac
printf 'JET_PHASE_COUNTERS {"phase":"sema.check","scope_lookups":%s,"cache":{"hits":%s,"misses":0}}\n' "$work" "$n"
printf 'JET_PHASE_COUNTERS: {"phase":"parse","tokens":%s}\n' $((3 * n)) >&2
case $kind in
broken) if [ "$n" -ge 4 ]; then exit 3; fi ;;
errors)
    printf 'Error [E0001] first\nError [E0002] second\n'
    exit 1
    ;;
malformed) printf 'JET_PHASE_COUNTERS oops\n' ;;
negative) printf 'JET_PHASE_COUNTERS {"phase":"lower","items":-1}\n' ;;
esac
exit 0
