#!/usr/bin/env bash
# A stand-in for integ-apply6.sh in the integrate10 fixture. A SUB with a `noapply` file is NOAPPLY (its
# .status and .apply written); any other is APPLIED, with its `files` list as I/SUB.files. Then the REGEN
# lines (STANDIN_REGEN_RC is the RuntimeGenerators rc) and the applied SUBs after "--applied--".
# STANDIN_APPLY_FAIL stops it as a broken integ repo does.
T=$1; I=$2; shift 2
echo "integ-apply: composing $T"
if [ -n "${STANDIN_APPLY_FAIL:-}" ]; then
  echo "integ-apply: commit of X-1 failed; the integ repo is broken, stopping"; echo "after the stop"; exit 2
fi
applied=()
for s in "$@"; do
  s=${s%/}; n=$(basename "$s")
  if [ -e "$s/noapply" ]; then
    echo NOAPPLY > "$I/$n.status"; cp "$s/noapply" "$I/$n.apply"; echo "NOAPPLY $n"; continue
  fi
  cp "$s/files" "$I/$n.files"; echo "APPLIED $n"; applied+=("$s")
done
echo "unrelated line"
printf 'regen %s\n' $(seq 1 25) > "$I/regen.RuntimeGenerators.jet.out"
{ printf 'x%.0s' $(seq 1 320); printf '\nlast line without a newline'; } > "$I/regen.CoreSourceRows.jet.out"
echo "REGEN Tools/agent/CoreSourceRows.jet  rc=0"
echo "REGEN Tools/agent/RuntimeGenerators.jet  rc=${STANDIN_REGEN_RC:-0}"
echo "--applied--"
[ ${#applied[@]} -eq 0 ] && echo
for s in "${applied[@]}"; do echo "$s"; done
