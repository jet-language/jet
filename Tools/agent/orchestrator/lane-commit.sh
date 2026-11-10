#!/usr/bin/env bash
# lane-commit.sh <lane> <message file> <patch file>
# The ONLY way lanes put changes onto dev. Lanes develop in their own export (outside the repo), never
# in the shared worktree. To commit: make a patch of your changes (repo-relative paths with a/ b/
# prefixes; adds, deletes and renames are all fine), then run this script.
#
# Batched landing: the call queues the patch under $JET_RUN_ROOT/commit-queue and waits. Whoever holds
# the machine-wide commit lock drains every queued job at once: it applies each patch that applies
# cleanly and touches no path another job in the batch touches, runs ONE precommit over the union of
# touched paths, and on success commits each job separately with its own message. If the batch
# precommit fails, it reverts everything and lands the jobs one at a time, so a bad patch only fails
# itself.
# Prints `lane-commit: <sha> by <lane>` on success; exit codes: 2 bad patch, 3 dirty path, 4 does not
# apply, 5 precommit failed, 6 commit failed, 7 the lander was interrupted.
# Never edit this file in place while it runs: write a new file and mv it over.
#
# The original of gauntlet tool entry lane-commit; LaneCommit.jet beside it is its Jet twin. The
# worktree is $LANE_COMMIT_WORKTREE, else $JET_DEV_WORKTREE; the queue, lock, hold switch and path
# freeze are in $JET_RUN_ROOT; the precommit is Main's precommit.sh, in $JET_STATE_ROOT/scratch/Main
# (Tools/agent/roots.sh). A relative root names a path in the caller's directory.
set -uo pipefail
unset GIT_INDEX_FILE GIT_DIR GIT_WORK_TREE
LANE=${1:?lane} MSG=${2:?message file} PATCH=${3:?patch file}
. "$(dirname "$0")/../roots.sh"
# A root as an absolute path: the drain works in the worktree.
abs() { case $1 in /*) printf '%s' "$1";; *) printf '%s/%s' "$PWD" "$1";; esac; }
W=$(abs "${LANE_COMMIT_WORKTREE:-$JET_DEV_WORKTREE}")
Q=$(abs "${LANE_COMMIT_QUEUE:-$JET_RUN_ROOT/commit-queue}")
PRE=$(abs "${LANE_COMMIT_PRECOMMIT:-$JET_STATE_ROOT/scratch/Main/precommit.sh}")
LOCK=$(abs "${LANE_COMMIT_LOCK:-$JET_RUN_ROOT/commit.lock}")
HOLD=$(abs "$JET_RUN_ROOT/commit-hold")
FREEZE=$(abs "$JET_RUN_ROOT/path-freeze")
mkdir -p "$Q"
# Hold switch: while $HOLD exists, only the lanes it lists (one per line) land.
while [ -f "$HOLD" ] && ! grep -qxF "$LANE" "$HOLD"; do sleep 20; done

job="$Q/$(date +%s%N)-$$"
mkdir "$job" && cp "$MSG" "$job/msg" && cp "$PATCH" "$job/patch" && printf '%s\n' "$LANE" > "$job/lane" && printf '%s\n' "$$" > "$job/pid" && touch "$job/ready" || { echo "lane-commit: cannot queue the job"; exit 2; }

# Every path a patch touches, one per line, each ending in a newline (a last line without one is skipped
# by `while read`); a rename contributes both its old and its new path.
files_of() {
  git apply --numstat -z "$1" 2>/dev/null | node -e '
    const b = require("fs").readFileSync(0); const parts = b.toString("utf8").split("\0"); const out = [];
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i]; if (!p) continue;
      const m = p.match(/^(\S+)\t(\S+)\t(.*)$/s);
      if (!m) continue;
      if (m[3] === "") { out.push(parts[i + 1], parts[i + 2]); i += 2; } else out.push(m[3]);
    }
    for (const line of require("fs").readFileSync(process.argv[1], "utf8").split("\n")) { const r = line.match(/^rename from (.+)$/); if (r) out.push(r[1]); }
    const paths = [...new Set(out.filter(Boolean))];
    process.stdout.write(paths.length ? paths.join("\n") + "\n" : "");' "$1"
}
finish() { # job rc text
  rm -f "$1/applied"
  printf '%s\n' "$3" > "$1/out"; printf '%s\n' "$2" > "$1/rc"; touch "$1/result"
}
revert_job() { # job
  local j=$1 f
  git apply -R "$j/patch"
  while IFS= read -r f; do [ -n "$f" ] && [ -e "$f" ] && ! git ls-files --error-unmatch -- "$f" >/dev/null 2>&1 && rm -f -- "$f"; done < "$j/files"
  rm -f "$j/applied"
}
# A lock holder killed mid-batch must not leave patches applied in the shared tree: revert every job
# still marked applied (newest first) whose paths still differ from HEAD (a committed job matches HEAD).
on_signal() {
  local j
  [ -n "$PRE_PID" ] && { pkill -TERM -P "$PRE_PID" 2>/dev/null; kill -TERM "$PRE_PID" 2>/dev/null; wait "$PRE_PID" 2>/dev/null; }
  cd "$W" || exit 143
  for j in $(ls -d "$Q"/*/ 2>/dev/null | sort -r); do
    j=${j%/}
    [ -f "$j/applied" ] || continue
    mapfile -t fl < "$j/files"
    git reset -q -- "${fl[@]}" 2>/dev/null
    if ! git diff --quiet HEAD -- "${fl[@]}" 2>/dev/null || [ -n "$(git ls-files --others --exclude-standard -- "${fl[@]}")" ]; then revert_job "$j"; fi
    finish "$j" 7 "lane-commit: lander interrupted; reverted, resubmit"
  done
  exit 143
}
# Precommit runs in the background so a signal interrupts `wait` at once (bash defers traps until a
# foreground child exits); the trap kills it before reverting.
PRE_PID=""
run_pre() {
  local log="$Q/.precommit.$$.out"
  bash "$PRE" "$@" > "$log" 2>&1 & PRE_PID=$!
  wait "$PRE_PID"; PRE_RC=$?; PRE_PID=""
  PRE_OUT=$(cat "$log"); rm -f "$log"
}
commit_job() { # job
  local j=$1 lane; lane=$(cat "$j/lane")
  mapfile -t fl < "$j/files"
  git add -A -f -- "${fl[@]}"
  if git commit -q -F "$j/msg" -- "${fl[@]}"; then
    finish "$j" 0 "lane-commit: $(git rev-parse --short HEAD) by $lane"
  else
    git reset -q -- "${fl[@]}"; revert_job "$j"; finish "$j" 6 "lane-commit: commit failed; reverted"
  fi
}
# Apply one job onto the current tree (with earlier batch jobs applied). Returns 0 if applied.
apply_job() { # job
  local j=$1 f
  files_of "$j/patch" > "$j/files"
  [ -s "$j/files" ] || { finish "$j" 2 "lane-commit: patch lists no files or does not parse"; return 1; }
  while IFS= read -r f; do
    if [ -n "$(git status --porcelain -- "$f")" ]; then finish "$j" 3 "lane-commit: $f has uncommitted changes in the shared worktree; refusing"; return 1; fi
  done < "$j/files"
  git apply --check "$j/patch" 2>/dev/null || { finish "$j" 4 "lane-commit: patch does not apply to HEAD $(git rev-parse --short HEAD); rebase your export"; return 1; }
  touch "$j/applied"
  git apply "$j/patch" || { finish "$j" 4 "lane-commit: patch failed to apply"; return 1; }
  return 0
}
# Path freeze: $FREEZE holds the end time (epoch seconds) on line 1, the exempt lane on line 2, then
# one repo-relative path prefix per line. Until the end time, a job from another lane that touches a
# frozen prefix waits in the queue (it neither fails nor lands).
frozen() { # lane filesfile
  local end exempt p f
  [ -f "$FREEZE" ] || return 1
  end=$(sed -n 1p "$FREEZE"); exempt=$(sed -n 2p "$FREEZE")
  [ -n "$end" ] && [ "$(date +%s)" -lt "$end" ] || return 1
  [ "$1" = "$exempt" ] && return 1
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    while IFS= read -r f; do case "$f" in "$p"*) return 0;; esac; done < "$2"
  done < <(sed -n "3,\$p" "$FREEZE")
  return 1
}
drain() {
  cd "$W" || return
  local j lane batch=() union=() taken
  for j in $(ls -d "$Q"/*/ 2>/dev/null | sort); do
    j=${j%/}
    [ -f "$j/ready" ] && [ ! -f "$j/result" ] || continue
    # A job whose caller died is dropped, never landed.
    if ! kill -0 "$(cat "$j/pid")" 2>/dev/null; then rm -rf "$j"; continue; fi
    lane=$(cat "$j/lane")
    if [ -f "$HOLD" ] && ! grep -qxF "$lane" "$HOLD"; then continue; fi
    files_of "$j/patch" > "$j/files.peek"
    frozen "$lane" "$j/files.peek" && continue
    # A job that touches a path already in the batch waits for the next round.
    taken=0
    while IFS= read -r f; do for u in "${union[@]}"; do [ "$f" = "$u" ] && taken=1; done; done < "$j/files.peek"
    [ $taken = 1 ] && continue
    if apply_job "$j"; then
      batch+=("$j"); mapfile -t -O "${#union[@]}" union < "$j/files"
    fi
  done
  [ ${#batch[@]} -gt 0 ] || return
  local out rc
  run_pre "${union[@]}"; rc=$PRE_RC; out=$PRE_OUT
  if [ $rc = 0 ]; then
    for j in "${batch[@]}"; do commit_job "$j"; done
    return
  fi
  # Batch failed: revert all (newest first), then land each job alone so only the bad one fails.
  for ((i=${#batch[@]}-1; i>=0; i--)); do revert_job "${batch[$i]}"; done
  if [ ${#batch[@]} = 1 ]; then finish "${batch[0]}" 5 "$(printf '%s\n' "$out" | tail -20)
lane-commit: precommit failed; reverted"; return; fi
  for j in "${batch[@]}"; do
    apply_job "$j" || continue
    mapfile -t fl < "$j/files"
    run_pre "${fl[@]}"; out=$PRE_OUT
    if [ "$PRE_RC" = 0 ]; then commit_job "$j"; else revert_job "$j"; finish "$j" 5 "$(printf '%s\n' "$out" | tail -20)
lane-commit: precommit failed; reverted"; fi
  done
}

exec 9>"$LOCK"
while [ ! -f "$job/result" ]; do
  if flock -n 9; then trap on_signal TERM INT HUP; drain; trap - TERM INT HUP; flock -u 9; else sleep 5; fi
done
cat "$job/out"; rc=$(cat "$job/rc"); rm -rf "$job"; exit "$rc"
