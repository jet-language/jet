#!/usr/bin/env bash
# integrate.sh BATCH SUB... : Main's integration pass (owner order 2026-11-08: leads write code, Main builds and
# tests). Each SUB is an inbox folder ($JET_SCRATCH_ROOT/lanes/Main/inbox/<Lead>-<n>/) holding `patch` (git apply
# -p1 against origin/dev), `msg`, `tests` (one shell command per line, run from the tree root with `jet` = the
# compiler this pass builds) and `cards`. Steps, all collect-all:
#   1. export origin/dev to integ/BATCH/tree and apply every SUB in order (a SUB that does not apply is reported
#      and skipped; the rest still run);
#   2. one census of the composed tree (check, build, guards, runtime pack, self-check, Core self-check) through
#      census-snap.sh under the shared precommit lock and a 32G cap; every keep-going row is attributed to the
#      SUBs whose touched files it names;
#   3. if the build produced a compiler, every SUB's tests run with it, each recorded with rc and output tail.
# Writes integ/BATCH/REPORT.md (one section per SUB: OK, or every failure found) and prints its path.
# Never edit this file in place while it runs: write a new file and mv it over.
# The original of gauntlet tool entry integrate10; Integrate10.jet beside it is its Jet twin. The batches are in
# $JET_SCRATCH_ROOT/lanes/Main/integ, the dev worktree is $JET_DEV_WORKTREE and the seed $JET_SEED; integ-apply6.sh
# and census-snap2.sh are the ones in $JET_ORCHESTRATOR_DIR (this file's directory unless set), and capped.sh and
# runsub6.sh are Main's, in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -u
. "$(dirname "$0")/../roots.sh"
BATCH=${1:?batch}; shift
M=$JET_SCRATCH_ROOT/lanes/Main
W=$JET_DEV_WORKTREE
G=$JET_STATE_ROOT/scratch/Main
O=${JET_ORCHESTRATOR_DIR:-$(dirname "$0")}
I=$M/integ/$BATCH; T=$I/tree
rm -rf "$I"; mkdir -p "$T" "$I/tests" "$I/bin"
( cd "$W" && git fetch -q origin && git rev-parse --short origin/dev > "$I/BASE" && git archive origin/dev | tar -x -C "$T" ) || { echo "integrate: export failed"; exit 2; }
R=$I/REPORT.md
printf '# Integration %s on origin/dev %s\n\n' "$BATCH" "$(cat "$I/BASE")" > "$R"
# INTEG_PRE="BATCH:SUB ..." stacks this batch on another batch still landing: those SUBs (already tested there) are
# applied first, untested and unreported; integ-land accepts origin/dev moving only by that batch commits.
for ps in ${INTEG_PRE:-}; do
  pb=${ps%%:*}; pd=${ps#*:}
  ( cd "$T" && git apply -p1 "$pd/patch" ) || { echo "integrate: pre $pd does not apply"; exit 2; }
  echo "$pb" >> "$I/PRE"; echo "PRE $(basename "$pd") from $pb"
done
# Apply (three-way, generated files excluded and regenerated): integ-apply.sh prints the applied SUBs last.
bash "$O/integ-apply6.sh" "$T" "$I" "$@" > "$I/apply.out" 2>&1 || { echo "integrate: the apply step failed: $(awk '/^integ-apply:/' "$I/apply.out" | tail -1)"; exit 2; }
awk "/^--applied--$/{exit} {print}" "$I/apply.out"
mapfile -t applied < <(awk "f{print} /^--applied--$/{f=1}" "$I/apply.out" | awk NF)
# A generator that fails on the composed sources leaves every generated table stale, so the built compiler would
# miss the batch's new rows and every later result would be noise: stop here and report the regeneration.
if awk '/^REGEN / && !/ rc=0$/{bad=1} END{exit !bad}' "$I/apply.out"; then
  { printf 'REGEN FAILED: the census did not run.\n'; for f in "$I"/regen.*.out; do printf '\n%s:\n```\n' "$(basename "$f")"; tail -20 "$f" | cut -c1-300; printf '```\n'; done; } >> "$R"
  echo REGENFAIL > "$I/REGENFAIL"; echo "$R"; exit 3
fi
# The census: every file any applied SUB touches selects the Jetpack pass when it is under Jetpack/.
mapfile -t touched < <(for s in "${applied[@]}"; do cat "$I/$(basename "$s").files"; done | sort -u)
echo "census start"
CENSUS_TREE=$T CENSUS_OUT=$I/census CAP_CPUS=16-31 bash "$G/capped.sh" 32G bash "$O/census-snap2.sh" "${touched[@]}" > "$I/census.out" 2>&1
crc=$?
echo "census rc=$crc"
# The repository ratchets and generator checks (Land.jet ratchets, as precommit.sh runs them), with the seed on
# the composed tree: a batch that passes here lands without a second census.
C=$(readlink -f "$JET_SEED")
mapfile -t ratchets < <(cd "$T" && JET_COMPILER_IDENTITY=$(cat "$C/IDENTITY") JET_TOOLCHAIN_ROOT=$T "$C/jet" Tools/agent/Land.jet ratchets 2>/dev/null)
mkdir -p "$I/ratchets"; rrc=0
[ ${#ratchets[@]} -gt 0 ] || { echo "no ratchets listed" > "$I/ratchets/none.out"; rrc=1; }
for tool in "${ratchets[@]}"; do
  ( cd "$T" && JET_COMPILER_IDENTITY=$(cat "$C/IDENTITY") JET_TOOLCHAIN_ROOT=$T timeout 900 nice -n 19 "$C/jet" "$tool" --check > "$I/ratchets/$(basename "$tool").out" 2>&1; echo $? > "$I/ratchets/$(basename "$tool").rc" ) &
done
wait
for f in "$I"/ratchets/*.rc; do [ -e "$f" ] || continue; [ "$(cat "$f")" = 0 ] || rrc=1; done
echo "ratchets rc=$rrc"
[ "$rrc" = 0 ] || crc=1
SELF=$T/Compiler/JetCli/.jet/build/jet
ID=$(cat "$(readlink -f "$JET_SEED")/IDENTITY")
built=0; [ -x "$SELF" ] && built=1 && ln -sfn "$SELF" "$I/bin/jet"
# The engine `jet` talks to must be the seed's jetpack, whose identity JET_COMPILER_IDENTITY names (not the one on the login PATH).
[ -x "$(readlink -f "$JET_SEED")/jetpack" ] && ln -sfn "$(readlink -f "$JET_SEED")/jetpack" "$I/bin/jetpack"
# The engine beside the batch compiler must be the jetpack this tree builds (the version cut makes the seed's
# jetpack disagree with the batch jet): the census's hosted bootstrap names it as hosted-binary.
HB=$(awk "/^hosted-binary: /{print \$2}" "$I/census/jetpack.log" 2>/dev/null | tail -1)
if [ -n "$HB" ] && [ -x "$HB" ]; then cp "$HB" "$I/bin/jetpack.built" && ln -sfn "$I/bin/jetpack.built" "$I/bin/jetpack" && ln -sfn "$I/bin/jetpack.built" "$(dirname "$(readlink -f "$SELF")")/jetpack"; echo "tests use the built jetpack $HB"; else echo "no built jetpack (census jetpack row failed); tests use the seed's"; fi
export I T SELF ID G
# Every SUB runs its test lines as one job, four SUBs at a time (owner 2026-11-09: land fast; serial tests took 1.5 h).
if [ $built = 1 ]; then
  for s in "${applied[@]}"; do [ -s "$s/tests" ] && printf "%s\n" "$s"; done | xargs -r -P 4 -I{} bash "$G/runsub6.sh" new {}
fi
# A failing line reruns on a clean origin/dev export with the seed, so rows that already fail on dev are told apart
# from the ones this batch introduces (I/tests/SUB.K.base.out; a FAIL row present in both counts as pre-existing).
if [ $built = 1 ] && cat "$I"/tests/*.rc 2>/dev/null | awk -F"\t" "\$1!=0{f=1} END{exit !f}"; then
  B=$I/basetree; C0=$(readlink -f "$JET_SEED"); mkdir -p "$B" "$I/basebin"
  ( cd "$W" && git archive "$(cat "$I/BASE")" | tar -x -C "$B" )
  ln -sfn "$C0/jet" "$I/basebin/jet"; ln -sfn "$C0/jetpack" "$I/basebin/jetpack"
  export B C0
  for rcf in "$I"/tests/*.rc; do awk -F"\t" "\$1!=0{f=1} END{exit !f}" "$rcf" && printf "%s\n" "$(basename "$rcf" .rc)"; done | xargs -r -P 4 -I{} bash "$G/runsub6.sh" base {}
  rm -rf "$B"
fi
# Goldens that cite a line of a Core or Prelude file this batch touches (`<corelib>/Core/X.jet:N`) drift when
# that file moves; run each one so the batch, not the release gate, catches it.
if [ $built = 1 ]; then
  mapfile -t corefiles < <(printf "%s\n" "${touched[@]}" | awk "/^(Core|Compiler\/Prelude)\/.*\.jet$/")
  : > "$I/goldens.list"
  for f in "${corefiles[@]}"; do ( cd "$T" && git grep -l --no-index -F "<corelib>/$f:" -- "*.out" 2>/dev/null ) >> "$I/goldens.list"; done
  sort -u -o "$I/goldens.list" "$I/goldens.list"
  while IFS= read -r g; do
    [ -n "$g" ] || continue; stem=${g%.out}; stem=${stem%.err}
    ( cd "$T" && env PATH="$I/bin:$PATH" JET="$SELF" JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$T" \
        CAP_CPUS=16-31 timeout 1800 bash "$G/capped.sh" 24G nice -n 19 jet test Compiler --profile golden "$stem" ) > "$I/goldens.$(printf %s "$stem" | tr / _).out" 2>&1
    printf "%s\t%s\n" "$?" "$stem" >> "$I/goldens.rc"
  done < "$I/goldens.list"
  # Reported in the summary, not folded into crc: Main attributes a drifted golden to the SUB that moved the line.
fi
# Report.
rows=$(cat "$I/census/"*keep-going.tsv 2>/dev/null)
{
  printf 'Census rc=%s (built compiler: %s). Summary:\n```\n' "$crc" "$([ $built = 1 ] && echo yes || echo no)"
  awk '/ rc=|rows=/' "$I/census.out"
  for f in "$I"/ratchets/*.rc; do [ -e "$f" ] || continue; printf 'ratchet %s rc=%s\n' "$(basename "$f" .rc)" "$(cat "$f")"; done
  [ -f "$I/goldens.rc" ] && awk -F"\t" '{print "golden " $2 " rc=" $1}' "$I/goldens.rc"
  printf '```\n'
  for f in "$I"/ratchets/*.rc; do [ -e "$f" ] && [ "$(cat "$f")" != 0 ] && { printf 'Ratchet %s failed:\n```\n' "$(basename "$f" .rc)"; tail -30 "${f%.rc}.out" | cut -c1-300; printf '```\n'; }; done
  printf '\n'
  for s in "$@"; do
    s=${s%/}; n=$(basename "$s")
    printf '## %s\n' "$n"
    if [ -f "$I/$n.status" ]; then printf 'Does not apply to origin/dev %s:\n```\n' "$(cat "$I/BASE")"; cut -c1-300 "$I/$n.apply" | awk 'NR<=20'; printf '```\n\n'; continue; fi
    bad=0
    hits=$(while IFS= read -r f; do [ -n "$f" ] && printf '%s\n' "$rows" | grep -F -- "$f"; done < "$I/$n.files" | sort -u | cut -c1-400)
    if [ -n "$hits" ]; then bad=1; printf 'Census rows in files this patch touches:\n```\n%s\n```\n' "$hits"; fi
    if [ -f "$I/tests/$n.rc" ]; then
      k=0
      while IFS=$'\t' read -r rc line; do
        k=$((k+1))
        if [ "$rc" = 0 ]; then printf -- '- PASS `%s`\n' "$line"; continue; fi
        o=$I/tests/$n.$k.out; bo=$I/tests/$n.$k.base.out
        if [ -f "$bo" ] && [ "$(cat "$I/tests/$n.$k.base.rc")" != 0 ]; then
          frows() { awk '/FAIL/{gsub(/[0-9]+(\.[0-9]+)? ?(ms|s|µs)/,""); print}' "$1" | sort -u; }
          new=$(comm -23 <(frows "$o") <(frows "$bo"))
          if [ -z "$new" ] && [ -n "$(frows "$o")" ]; then
            printf -- '- PRE-EXISTING rc=%s `%s`: every FAIL row also fails on origin/dev with the seed (%s)\n' "$rc" "$line" "$bo"; continue
          fi
          bad=1; printf -- '- FAIL rc=%s `%s` (full output %s; dev with the seed also fails, rc=%s, %s). FAIL rows not on dev:\n```\n' "$rc" "$line" "$o" "$(cat "$I/tests/$n.$k.base.rc")" "$bo"
          if [ -n "$new" ]; then printf '%s\n' "$new" | cut -c1-300 | awk 'NR<=25'; else awk '/error|Error|panic|failed/' "$o" | cut -c1-300 | awk 'NR<=25'; tail -5 "$o" | cut -c1-300; fi
          printf '```\n'; continue
        fi
        bad=1; printf -- '- FAIL rc=%s `%s` (full output %s; passes on origin/dev with the seed or was not rerun):\n```\n' "$rc" "$line" "$o"; awk '/FAIL|error|Error|panic|failed/' "$o" | cut -c1-300 | awk 'NR<=25'; tail -5 "$o" | cut -c1-300; printf '```\n'
      done < "$I/tests/$n.rc"
    elif [ $built = 0 ]; then bad=1; printf 'Tests not run: the composed tree did not build (see census).\n'
    else printf 'No tests listed.\n'; fi
    if [ $bad = 0 ] && [ "$crc" = 0 ]; then printf '\n**OK**\n\n'; echo OK > "$I/$n.status"; else printf '\n**NOT OK**\n\n'; echo BAD > "$I/$n.status"; fi
  done
  if [ "$crc" != 0 ]; then
    printf '## Census rows not in any submitted file\n```\n'
    all=$(for s in "${applied[@]}"; do cat "$I/$(basename "$s").files"; done)
    printf '%s\n' "$rows" | grep -v -F -f <(printf '%s\n' "$all" | awk 'NF') | cut -c1-300 | awk 'NR<=40'
    printf '```\n'
  fi
} >> "$R"
echo "$R"
