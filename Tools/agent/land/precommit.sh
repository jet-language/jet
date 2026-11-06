#!/usr/bin/env bash
# precommit.sh <file>... : MANDATORY before every commit that touches .jet compiler/Core/runtime
# sources. Runs the collect-all census of HEAD + the named files with the seed, in package form:
# `jet check` AND a keep-going `jet build` (optimize, lowering, object and link rows), so a commit
# cannot break chain s1. Prints PRECOMMIT OK or the rows. Run from anywhere; paths are repo-relative
# to the dev worktree ($JET_DEV_WORKTREE, Tools/agent/roots.sh).
# Never edit this file in place while it runs: write a new file and mv it over.
set +e
M=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$M/../roots.sh"
W=$JET_DEV_WORKTREE
fail=0
# (The standalone seed parse step is gone: a0_008+ has no `emit --ast`, and the census check below
# parses every compiler file anyway.)
compiler=0
# Compiler inputs (files of the packages `jet build Compiler` compiles, its workspace files, Core:
# Tools/agent/compiler-packages.mjs), Jetpack sources and any sources.list change need the compiler
# census; tests, tools and examples do not.
# Every compiler census builds the runtime pack with the compiler it just built (census-snap.sh): the
# self-check runs compile-time evaluation on the pack IR, and a runtime/backend change must still lower it.
for f in "$@"; do
  case "$f" in Compiler/JetRuntime/Source/*|Compiler/JetRuntime/sources.list|Compiler/JetBackend/Source/*|Core/*|Jetpack/*/Source/*|*/sources.list) compiler=1 ;; esac
done
inputs=$(node --input-type=module -e 'const [module, root, ...files] = process.argv.slice(1); const m = await import(module); const roots = m.compilerInputRoots(root); console.log(files.filter(file => m.isCompilerInput(root, file, roots)).join("\n"))' "$W/Tools/agent/compiler-packages.mjs" "$W" "$@") || { echo "PRECOMMIT: cannot read the compiler packages"; fail=1; }
[ -n "$inputs" ] && compiler=1
if [ $compiler = 1 ]; then
  res=$(CAP_CPUS=16-31 bash "$M/capped.sh" 32G bash "$M/census-snap.sh" "$@" 2>&1)
  crc=$?
  printf '%s\n' "$res" | tail -16
  [ $crc = 0 ] && printf '%s' "$res" | grep -q 'check rc=0' && printf '%s' "$res" | grep -q 'build rc=0 rows=0' && printf '%s' "$res" | grep -q 'runtime rc=0' && printf '%s' "$res" | grep -q 'selfcheck rc=0 rows=0' || fail=1
fi
[ $fail = 0 ] && echo "PRECOMMIT OK" || echo "PRECOMMIT FAILED"
exit $fail
