#!/usr/bin/env bash
# prescreen.sh <patch> [earlier admitted patches...] : fast admission check for one land job. Applies the patch to a fresh export of dev HEAD
# and runs the seed `jet check Compiler/JetCli` with keep-going (light-slot resources). Exit 0 = clean; 3 = does
# not apply; 1 = check rows (printed). Keeps broken patches out of lander batches (no bisect rounds).
set -uo pipefail
P=$(realpath "${1:?patch}"); shift; PRIOR=("$@")
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
W=$JET_DEV_WORKTREE
mkdir -p "$JET_SCRATCH_ROOT/discovery"
T=$(mktemp -d "$JET_SCRATCH_ROOT/discovery/prescreen.XXXXXX")
trap 'rm -rf "$T"' EXIT
git -C "$W" archive HEAD | tar -x -C "$T" || exit 2
# A patch whose only conflict is in the generated CoreSourceRows views applies around them (batch-land
# regenerates the views from the batch sources).
GENX="--exclude=Compiler/JetCli/Source/Cli/CoreSources.jet --exclude=Compiler/JetFoundation/Source/Registry/CoreCallRows.jet --exclude=Compiler/JetFoundation/Source/Registry/CoreEnumRows.jet --exclude=Compiler/JetFoundation/Source/Registry/CoreSourceOwned.jet --exclude=Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet"
for q in "${PRIOR[@]}"; do ( cd "$T" && { git apply "$q" || git apply $GENX "$q"; } ) 2>/dev/null; done
( cd "$T" && { git apply "$P" || git apply $GENX "$P"; } ) 2>/dev/null || { echo "prescreen: does not apply to HEAD $(git -C "$W" rev-parse --short HEAD)"; exit 3; }
# Patches that touch no compiler input need only the apply check (precommit uses the same classifier).
mapfile -t files < <(for q in "${PRIOR[@]}" "$P"; do git apply --numstat "$q" 2>/dev/null | cut -f3; done | sort -u)
inputs=$(node --input-type=module -e 'const [module, root, ...files] = process.argv.slice(1); const m = await import(module); const roots = m.compilerInputRoots(root); console.log(files.filter(file => m.isCompilerInput(root, file, roots)).join("\n"))' "$W/Tools/agent/compiler-packages.mjs" "$W" "${files[@]}") || inputs=unknown
[ -z "$inputs" ] && { echo "prescreen: clean (no compiler inputs; apply check only)"; exit 0; }
C=$(readlink -f "$JET_SEED")
cd "$T" && JET_COMPILER_IDENTITY=$(cat "$C/IDENTITY") JET_TOOLCHAIN_ROOT=$T JET_KEEP_GOING=1 \
  timeout 1200 systemd-run --user --scope --quiet --collect -p MemoryMax=32G -p MemorySwapMax=0 -- \
  nice -n 10 ionice -c2 -n7 taskset -c 16-27 prlimit --stack=1073741824:1073741824 -- "$C/jet" check --threads 10 Compiler/JetCli > "$T/check.out" 2> "$T/check.err"
rc=$?
rows=$(awk '/^Error/' "$T/check.err" | wc -l)
if [ $rc = 0 ] && [ "$rows" = 0 ]; then echo "prescreen: clean"; exit 0; fi
echo "prescreen: check rc=$rc errors=$rows"
awk '/^Error/{e=$0; getline; print e" "$2}' "$T/check.err" | sort | uniq -c | head -20
exit 1
