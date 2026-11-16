#!/usr/bin/env bash
# batch2.sh BATCH SUB... : one integration cycle that does not let one bad submission hold back the rest.
#   1. SUBs whose meta says `seed:` something other than none wait for that seed and are listed as HELD.
#   2. integrate3.sh BATCH with the rest.
#   3. If the census failed, a SUB is QUARANTINED when
#      - a census row names one of its touched files (Core/X/Y.jet, <corelib>/Core/X/Y.jet, or module path Core/X/Y), or
#      - an error's quoted source line (`NNN | text` under an `Error [` header in census/*.err) is a line its patch adds
#        (this attributes failures in concatenated units such as the runtime pack's unit.jet, which have no rows);
#      with every later SUB of the same lead. integrate3.sh BATCH-r then runs once more with the others.
# integ-land3.sh lands from whichever pass is all OK.
# The original of gauntlet tool entry batch10; Batch10.jet beside it is its Jet twin and the program in use, and
# this file stays as the entry's reference. The batches live in
# $JET_SCRATCH_ROOT/lanes/Main/integ (Tools/agent/roots.sh); integrate10.sh is the one in $JET_ORCHESTRATOR_DIR,
# this directory unless set.
set -u
. "$(dirname "$0")/../roots.sh"
M=$JET_SCRATCH_ROOT/lanes/Main; G=${JET_ORCHESTRATOR_DIR:-$(dirname "$0")}
# One batch at a time: two runs of the same BATCH would write into one integ dir.
exec 9>"$M/integ/.batch.lock"; flock -n 9 || { echo "batch: another batch holds the lock"; exit 75; }
BATCH=${1:?batch}; shift
run=()
for s in "$@"; do
  s=${s%/}; sd=$(awk -F': *' '$1=="seed"{print $2}' "$s/meta" 2>/dev/null)
  if [ -n "$sd" ] && [ "$sd" != none ]; then echo "HELD $(basename "$s") (seed: $sd)"; else run+=("$s"); fi
done
"$G/integrate10.sh" "$BATCH" "${run[@]}"; irc=$?
# rc 2: the export or the apply step stopped (integrate10 printed why); there is no census to attribute.
[ $irc = 2 ] && { echo "batch: integrate stopped before the census (rc 2)"; exit 2; }
I=$M/integ/$BATCH
if [ -f "$I/REGENFAIL" ]; then
  # Blame the SUBs whose added lines carry a name the failing generator quotes (`core.math.__from_bits_f32` gives
  # __from_bits_f32), with every later SUB of the same lead.
  names=$(cat "$I"/regen.*.out | awk '{while (match($0, /`[^`]+`/)) {t=substr($0, RSTART+1, RLENGTH-2); n=split(t, p, "."); if (length(p[n]) > 3) print p[n]; $0=substr($0, RSTART+RLENGTH)}}' | sort -u)
  keep=(); blamed=()
  for s in "${run[@]}"; do
    n=$(basename "$s"); hit=0
    [ "$(cat "$I/$n.status" 2>/dev/null)" = NOAPPLY ] && continue
    [ -n "$names" ] && awk '/^\+[^+]/' "$s/patch" | grep -qwF -f <(printf '%s\n' "$names") && hit=1
    lead=${n%-*}; for x in "${blamed[@]}"; do [ "${x%-*}" = "$lead" ] && hit=1; done
    if [ $hit = 1 ]; then blamed+=("$n"); else keep+=("$s"); fi
  done
  [ ${#blamed[@]} -gt 0 ] || { echo "batch: a generator failed but no submission adds a name it quotes; see $I/REPORT.md"; exit 1; }
  echo "QUARANTINED (regen) ${blamed[*]}"
  "$G/integrate10.sh" "$BATCH-r" "${keep[@]}"
  exit $?
fi
grep -q '^Census rc=0 ' "$I/REPORT.md" && exit 0
rows=$(cat "$I/census/"*keep-going.tsv "$I/census.out" 2>/dev/null | awk -F'\t' 'NF>=2{print $2}' | sed -e 's#<corelib>/##' -e 's#:[0-9.]*$##' -e 's#\.jet$##' | sort -u)
# Source lines quoted under Error headers (not warnings), trimmed.
errlines=$(cat "$I/census/"*.err 2>/dev/null | awk '/^Error \[/{e=1; next} /^(Warning|Note) \[/{e=0} e && /^ *[0-9]+ \| /{sub(/^ *[0-9]+ \| */,""); sub(/[ \t]+$/,""); if (length($0)>8) print}' | sort -u)
keep=(); blamed=()
for s in "${run[@]}"; do
  n=$(basename "$s"); hit=0
  [ "$(cat "$I/$n.status" 2>/dev/null)" = NOAPPLY ] && continue
  if [ -n "$rows" ]; then
    while IFS= read -r f; do
      [ -n "$f" ] || continue; m=${f%.jet}
      printf '%s\n' "$rows" | grep -qxF -e "$m" -e "$f" && { hit=1; break; }
    done < "$I/$n.files"
  fi
  if [ $hit = 0 ] && [ -n "$errlines" ]; then
    P="$s/patch"; [ -s "$I/$n.rebased" ] && P="$I/$n.rebased"
    awk '/^\+[^+]/{sub(/^\+[ \t]*/,""); sub(/[ \t]+$/,""); print}' "$P" | grep -qxF -f <(printf '%s\n' "$errlines") && hit=1
  fi
  # A quarantined SUB takes every later SUB of the same lead with it (they may stack on it).
  lead=${n%-*}; for b in "${blamed[@]}"; do [ "${b%-*}" = "$lead" ] && hit=1; done
  if [ $hit = 1 ]; then blamed+=("$n"); else keep+=("$s"); fi
done
[ ${#blamed[@]} -gt 0 ] || { echo "batch: the census failed but no row or error line names a submission; see $I/REPORT.md"; exit 1; }
echo "QUARANTINED ${blamed[*]}"
[ ${#keep[@]} -gt 0 ] || exit 1
"$G/integrate10.sh" "$BATCH-r" "${keep[@]}"
