#!/bin/sh
# run.sh [probe.jet ...]: runs each #6563 c1 probe (default: every *.jet here) with `jet run -A` and
# `jet run -A --release`, printing its output and exit code.
# The compiler is $JET (Tools/agent/jet-env); JET_COMPILER_IDENTITY passes through unchanged.
# JET_TOOLCHAIN_ROOT defaults to the checkout that holds this folder.
# p1328 and p1644 are expected to fail with a diagnostic; the others print the output their audit row records.
D=$(cd "$(dirname "$0")" && pwd)
R=${JET_TOOLCHAIN_ROOT:-$(git -C "$D" rev-parse --show-toplevel)}
export JET_TOOLCHAIN_ROOT="$R" NO_COLOR=1
cd "$D" || exit 1
[ $# -gt 0 ] || set -- *.jet
for p in "$@"; do
  for mode in "" "--release"; do
    echo "== $p: jet run -A $mode"
    "$R/Tools/agent/jet-env" jet run -A $mode "$p"
    echo "rc=$?"
  done
done
