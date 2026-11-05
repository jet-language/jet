#!/usr/bin/env bash
# lander.sh : the landing service. Lanes enqueue patches; every cycle the lander verifies ALL queued patches
# with ONE precommit (batch-land.sh) and commits each as its own commit. On a failed batch it bisects
# (halves) automatically, lands every clean half, and returns failing patches to their lane with the rows.
#
# Enqueue (lanes): Tools/agent/land/q.mjs, or write a file $JET_RUN_ROOT/land-queue/<epoch>-<Lane>.job with ONE line:
#   <Lane><TAB><absolute msg file><TAB><absolute patch file>
# Results (lanes): $JET_RUN_ROOT/land-results/<same name>.result  (LANDED <sha> | REJECTED <reason/rows> | STALE does not apply)
# Log: $JET_RUN_ROOT/lander.log. Stop: touch $JET_RUN_ROOT/lander.stop. Roots: Tools/agent/roots.sh; the lander
# commits to $JET_DEV_WORKTREE.
set -uo pipefail
M=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$M/../roots.sh"
W=$JET_DEV_WORKTREE
Q=$JET_RUN_ROOT/land-queue; RS=$JET_RUN_ROOT/land-results; LOG=$JET_RUN_ROOT/lander.log
mkdir -p "$Q" "$RS" "$Q/taken"
say() { echo "$(date +%T) $*" >> "$LOG"; }
verify() { # <list file> -> 0 clean (and committed) | 5 precommit failed | 6 commit failed (both reverted, HEAD unmoved)
  bash "$M/batch-land.sh" "$1" > "$1.out" 2>&1; local rc=$?
  cat "$1.out" >> "$LOG"; return $rc
}
result_for() { # <list file> <status> : write a result file for every job in the list
  while IFS=$'\t' read -r lane msg patch job; do
    local sha; sha=$(grep -E "COMMITTED [0-9a-f]+ $lane $(basename "$patch")" "$1.out" 2>/dev/null | awk '{print $2}' | tail -1)
    if [ -n "$sha" ]; then echo "LANDED $sha" > "$RS/$job.result"
    elif grep -qF "DOES NOT APPLY $lane $(basename "$patch") - an earlier patch of its stack did not apply" "$1.out" 2>/dev/null; then echo "STALE an earlier patch of its stack did not apply; rebase the stack onto HEAD" > "$RS/$job.result"
    elif grep -q "DOES NOT APPLY $lane $(basename "$patch")" "$1.out" 2>/dev/null; then echo "STALE does not apply to HEAD; rebase" > "$RS/$job.result"
    elif grep -q "SKIP (already on dev) $lane $(basename "$patch")" "$1.out" 2>/dev/null; then echo "LANDED already-on-dev" > "$RS/$job.result"
    else { echo "REJECTED $2"; grep -vE '^nm: ' "$1.out" | tail -25; } > "$RS/$job.result"; fi
  done < "$1"
}
# stack_cut <done list> <todo list>: a lane's jobs are a stack, so once one of them did not land, its later
# jobs in <todo> are left for the lane too (a bisected batch must not land a stack's top without its base).
stack_cut() {
  local lane msg patch job; local -A bad=()
  while IFS=$'\t' read -r lane msg patch job; do grep -q '^LANDED' "$RS/$job.result" 2>/dev/null || bad[$lane]=$job; done < "$1"
  [ ${#bad[@]} -gt 0 ] || return 0
  : > "$2.keep"
  while IFS=$'\t' read -r lane msg patch job; do
    if [ -n "${bad[$lane]+x}" ]; then echo "STALE an earlier patch of its stack did not land (${bad[$lane]}); rebase the stack onto HEAD" > "$RS/$job.result"; say "stack: $job left for its lane (${bad[$lane]} did not land)"
    else printf '%s\t%s\t%s\t%s\n' "$lane" "$msg" "$patch" "$job" >> "$2.keep"; fi
  done < "$2"
  mv "$2.keep" "$2"
}
land() { # <list file with 4 cols> : verify; on failure split in halves and recurse
  local list=$1 n rc; n=$(wc -l < "$list")
  cut -f1-3 "$list" > "$list.3"
  verify "$list.3"; rc=$?
  cp "$list.3.out" "$list.out"
  if [ $rc -eq 0 ]; then result_for "$list" ok; return; fi
  if [ "$n" -le 1 ]; then result_for "$list" "$([ $rc -eq 6 ] && echo "the batch could not be committed (nothing landed; see BATCH COMMIT FAILED)" || echo "precommit failed for this patch")"; return; fi
  local h=$(( (n + 1) / 2 ))
  head -n "$h" "$list" > "$list.a"; tail -n +"$((h + 1))" "$list" > "$list.b"
  say "batch of $n failed (rc $rc); bisecting $h + $((n - h))"
  land "$list.a"; stack_cut "$list.a" "$list.b"
  [ -s "$list.b" ] && land "$list.b"
}
say "lander up"
while :; do
  [ -f "$JET_RUN_ROOT/lander.stop" ] && { say "lander stopping"; exit 0; }
  # A dirty shared worktree (someone mid-commit) is never the patches' fault: wait, do not reject.
  if [ -n "$(git -C "$W" status --porcelain --untracked-files=no)" ]; then sleep 30; continue; fi
  mapfile -t jobs < <(ls "$Q"/*.job 2>/dev/null | sort)
  if [ ${#jobs[@]} -gt 0 ]; then
    L=$Q/taken/batch-$(date +%s).list; : > "$L"
    # Stacked admission: one seed check of HEAD + all queued patches; if clean, admit them all at once.
    mapfile -t qp < <(for j in "${jobs[@]}"; do head -1 "$j" | cut -f3; done)
    if [ ${#jobs[@]} -gt 1 ]; then
      nq=${#qp[@]}
      if pre=$(bash "$M/prescreen.sh" "${qp[$((nq-1))]}" "${qp[@]:0:$((nq-1))}" 2>&1); then
        for j in "${jobs[@]}"; do name=$(basename "$j" .job); line=$(head -1 "$j"); mv "$j" "$Q/taken/"; printf '%s\t%s\n' "$line" "$name" >> "$L"; done
        say "prescreen clean (stacked) ${#jobs[@]} job(s)"; jobs=()
      else
        say "stacked prescreen failed; admitting one by one"
      fi
    fi
    for j in "${jobs[@]}"; do
      name=$(basename "$j" .job); line=$(head -1 "$j"); patch=$(printf '%s' "$line" | cut -f3)
      # Admission: a job must check clean on its own before it may join a batch (no bisect rounds).
      # Jobs stack in queue order: earlier admitted patches of this batch apply first.
      mapfile -t prior < <(cut -f3 "$L" 2>/dev/null)
      pre=$(bash "$M/prescreen.sh" "$patch" "${prior[@]}" 2>&1); prc=$?
      mv "$j" "$Q/taken/"
      if [ $prc -ne 0 ]; then { [ $prc = 3 ] && echo "STALE does not apply to HEAD; rebase" || echo "REJECTED at prescreen (seed check of HEAD + this patch alone)"; printf '%s\n' "$pre"; } > "$RS/$name.result"; say "prescreen rejected $name"; continue; fi
      say "prescreen clean $name"
      printf '%s\t%s\n' "$line" "$name" >> "$L"
    done
    [ -s "$L" ] || { sleep 30; continue; }
    say "batch: $(wc -l < "$L") job(s)"
    land "$L"
    say "batch done; HEAD $(git -C "$W" rev-parse --short HEAD)"
  fi
  sleep 60
done
