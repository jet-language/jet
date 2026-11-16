#!/usr/bin/env bash
# A stand-in for jpwrap.sh check --hosted: OK when the census gave it its
# environment and STANDIN_JETPACK is not `fail`; else Error rows.
if [ -x "$JET" ] && [ -d "$HOME" ] && [ "$JETPACK_WORKER" = precommit ] && [ "$*" = "check --hosted" ] && [ "${STANDIN_JETPACK:-ok}" != fail ]; then
  echo "assembling the hosted jetpack"
  echo "JETPACK HOSTED OK"
  exit 0
fi
echo "Error: runtime pack beside the compiler: $(readlink "$(dirname "$JET")/jet-runtime" || echo "not a link")"
for i in $(seq 1 12); do
  echo "Error [E0209]: the hosted jetpack's Store module $i calls a function its package no longer exports, which every later module of the pass repeats verbatim"
  echo "  --> Jetpack/Store/Source/F$i.jet:$i:5"
  echo "    |"
done
exit 1
