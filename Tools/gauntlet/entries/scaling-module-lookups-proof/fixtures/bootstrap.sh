#!/bin/sh
# A deterministic stand-in for a built bootstrap compiler (the gate's runner
# protocol: the project in JET_BOOTSTRAP_SOURCE_ROOT, JET_TRACE_FILE and
# JET_PHASE_COUNTERS in the environment, `complete=true` in
# JET_BOOTSTRAP_RECEIPT), run through the KIND wrappers next to it: its
# sema.check module lookups are the generated sources' bytes, linear (the
# bytes), quadratic (bytes^2 / 1000) or broken (no compile succeeds).
kind=$1
ms=$(date +%s%3N)
printf '[\n{"name":"trace.start","ph":"i","ts":0,"pid":1,"tid":0,"args":{"unix_ms":%s}},\n{"name":"sema.check","cat":"sema.check","ph":"B","ts":100,"pid":1,"tid":0},\n{"name":"sema.check","cat":"sema.check","ph":"E","ts":200,"pid":1,"tid":0},\n' "$ms" >"$JET_TRACE_FILE"
bytes=$(find "$JET_BOOTSTRAP_SOURCE_ROOT" -name '*.jet' -exec cat {} + | wc -c)
case $kind in
quadratic) lookups=$((bytes * bytes / 1000)) ;;
broken)
    printf 'Error [E0001] the stand-in compiler fails\n'
    exit 1
    ;;
*) lookups=$bytes ;;
esac
printf 'JET_PHASE_COUNTERS {"phase":"sema.check","module_lookups":%s}\n' "$lookups"
printf 'complete=true\n' >"$JET_BOOTSTRAP_RECEIPT"
exit 0
