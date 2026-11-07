#!/usr/bin/env bash
# batch-land.sh <list file>: land many lane patches with ONE verification (collect-all).
# List lines: <lane>\t<abs msg file>\t<abs patch file>, in dependency order.
# Under the commit lock: apply each patch that still applies (cumulative), regenerate the runtime stop
# exports, run `Land.jet precommit` once on every touched path, then commit each patch as its own commit (plus
# one commit for regenerated files). On precommit failure: revert exactly what was applied. Report:
# $JET_RUN_ROOT/batch-land.log. Roots: Tools/agent/roots.sh; the batch lands on $JET_DEV_WORKTREE.
# Stacks: a lane's patches in one list are a stack (each may build on the ones before it). Once one of them
# does not apply, every later patch of that lane is left for the lane too, untried.
# All or nothing: the commits are built in a private index and chained on the pre-batch HEAD; the branch
# moves (one compare-and-swap update-ref) only once every commit exists and the chain's tree is exactly the
# verified worktree. Any failure leaves HEAD at the pre-batch commit and reverts the worktree.
set -uo pipefail
LIST=${1:?list}
M=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$M/../roots.sh"
W=$JET_DEV_WORKTREE
LOG=$JET_RUN_ROOT/batch-land.log
: > "$LOG"
say() { echo "$*" | tee -a "$LOG"; }
exec 9>"$JET_RUN_ROOT/commit.lock"
flock 9
cd "$W" || exit 2
[ -z "$(git status --porcelain --untracked-files=no)" ] || { say "worktree has tracked changes; refusing"; git status --porcelain | head | tee -a "$LOG"; exit 3; }
pre=$(git rev-parse --verify HEAD) || exit 2
# The paths a patch touches, one per line, each line newline-terminated (a `read` loop drops an unterminated
# last line; that once kept every single-file patch out of the file-overlap grouping below). `git apply
# --numstat` names only the new path of a rename, so the old path comes from the patch's `rename from` headers.
files_of() { git apply --numstat -z "$1" 2>/dev/null | node -e '
  const fs=require("fs");const b=fs.readFileSync(0).toString("utf8").split("\0");const out=[];
  for(let i=0;i<b.length;i++){const p=b[i];if(!p)continue;const m=p.match(/^(\S+)\t(\S+)\t(.*)$/s);if(!m)continue;
    if(m[3]===""){out.push(b[i+1],b[i+2]);i+=2}else out.push(m[3])}
  const unq=s=>{if(s[0]!=="\"")return s;const by=[];for(let i=1;i<s.length-1;i++){let c=s[i];if(c!=="\\"){by.push(c.charCodeAt(0));continue}
    c=s[++i];if(/[0-7]/.test(c)){by.push(parseInt(s.substr(i,3),8));i+=2}else by.push({n:10,t:9,a:7,b:8,f:12,r:13,v:11}[c]??c.charCodeAt(0))}
    return Buffer.from(by).toString("utf8")};
  for(const m of fs.readFileSync(process.argv[1],"utf8").matchAll(/^rename from (.*)$/gm))out.push(unq(m[1]));
  for(const f of new Set(out.filter(Boolean)))process.stdout.write(f+"\n")' "$1"; }
applied=() ; all=() ; pfiles=() ; declare -A broken=()
GEN="Compiler/JetCli/Source/Cli/CoreSources.jet Compiler/JetFoundation/Source/Registry/CoreCallRows.jet Compiler/JetFoundation/Source/Registry/CoreEnumRows.jet Compiler/JetFoundation/Source/Registry/CoreSourceOwned.jet Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet"
GENX=$(for f in $GEN; do printf -- "--exclude=%s " "$f"; done); rowregen=0
while IFS=$'\t' read -r lane msg patch; do
  [ -z "$lane" ] && continue; case "$lane" in \#*) continue;; esac
  if [ -n "${broken[$lane]+x}" ]; then say "DOES NOT APPLY $lane $(basename "$patch") - an earlier patch of its stack did not apply (${broken[$lane]}); left for its lane"; continue; fi
  if git apply -R --check "$patch" 2>/dev/null; then say "SKIP (already on dev) $lane $(basename "$patch")"; continue; fi
  ok=0
  if git apply --check "$patch" 2>/dev/null && git apply "$patch"; then ok=1
  # A patch whose only conflict is in the generated CoreSourceRows views applies around them; the views
  # are regenerated once from the batch sources below and committed with the patches that touch them.
  elif git apply --check $GENX "$patch" 2>/dev/null && git apply $GENX "$patch"; then ok=1; rowregen=1; say "applied around the CoreSourceRows views: $(basename "$patch")"
  fi
  if [ $ok = 1 ]; then
    fl=$(files_of "$patch"); mapfile -t fs <<< "$fl"
    applied+=("$lane"$'\t'"$msg"$'\t'"$patch"); pfiles+=("$fl"); all+=("${fs[@]}")
    say "APPLIED $lane $(basename "$patch") (${#fs[@]} files)"
  else say "DOES NOT APPLY $lane $(basename "$patch") - left for its lane"; broken[$lane]=$(basename "$patch"); fi
done < "$LIST"
[ ${#applied[@]} -gt 0 ] || { say "nothing applied"; exit 0; }
revert_all() {
  # The batch started from a verified-clean worktree, so everything dirty now is the batch's own:
  # reverse-apply (best effort), then restore every still-dirty tracked path to HEAD and remove every
  # untracked path the batch created; finally verify clean.
  for ((k=${#applied[@]}-1; k>=0; k--)); do IFS=$'\t' read -r l m p <<< "${applied[$k]}"; git apply -R "$p" 2>/dev/null || say "reverse-apply failed (restoring paths instead): $(basename "$p")"; done
  git status --porcelain --untracked-files=no | awk '{print $NF}' | while read -r f; do git checkout HEAD -- "$f" 2>/dev/null || git rm -q --cached -- "$f" 2>/dev/null; done
  for f in "${all[@]}"; do [ -e "$f" ] && ! git ls-files --error-unmatch -- "$f" >/dev/null 2>&1 && rm -rf -- "$f"; done
  if [ -n "$(git status --porcelain --untracked-files=no)" ]; then say "REVERT LEFT THE WORKTREE DIRTY"; git status --porcelain | head -20 | tee -a "$LOG"; touch "$JET_RUN_ROOT/lander.stop"; fi
}
# Regenerate the runtime generators' outputs (stop exports, lifecycle) for the combined runtime sources.
# The runtime generators: the Jet package while it is the in-service form, else the node originals (in service until
# their Jet candidate passes the cutover gate).
if [ $rowregen = 1 ]; then
  rr=$(JET_COMPILER_IDENTITY="$(awk 1 "$JET_SEED/IDENTITY")" JET_TOOLCHAIN_ROOT="$PWD" timeout 900 "$JET_BASE_ROOT/latest/jet" run Tools/agent/CoreSourceRows.jet 2>&1); rrc=$?
  say "CoreSourceRows regeneration rc=$rrc $(printf '%s' "$rr" | tail -1)"
  [ $rrc -eq 0 ] || { say "CoreSourceRows regeneration FAILED; reverting"; revert_all; exit 5; }
fi
gen=$( if git ls-files --error-unmatch Tools/agent/RuntimeGenerators >/dev/null 2>&1; then timeout 900 "$JET_SEEDS/bin/jet" run Tools/agent/RuntimeGenerators; else for g in gen-runtime-trap-messages gen-runtime-lifecycle gen-runtime-stop-exports gen-runtime-host-errors gen-runtime-scheduler-messages gen-runtime-services-messages gen-runtime-test-messages gen-runtime-observe-messages gen-compiler-needed-text; do node Tools/agent/$g.mjs || exit 1; done; fi 2>&1); grc=$?
say "RuntimeGenerators rc=$grc $(printf '%s' "$gen" | tail -2 | tr '\n' ' ')"
# A set lookup, not `printf … | grep -q`: under pipefail, grep -q exiting early SIGPIPEs printf once the
# path list passes the pipe buffer, which miscounted a large patch's own files as regenerated.
declare -A inbatch=(); for f in "${all[@]}"; do [ -n "$f" ] && inbatch[$f]=1; done
mapfile -t genfiles < <(git status --porcelain --untracked-files=no | awk '{print $2}' | while read -r f; do [ -n "${inbatch[$f]+x}" ] || echo "$f"; done)
say "regenerated: ${genfiles[*]:-none}"
res=$(JET_COMPILER_IDENTITY="$(awk 1 "$JET_SEED/IDENTITY")" JET_TOOLCHAIN_ROOT="$PWD" "$JET_SEED/jet" Tools/agent/Land.jet precommit "${all[@]}" "${genfiles[@]}" 2>&1); prc=$?
printf '%s\n' "$res" | grep -vE '^nm: |^skip ' | tail -25 | tee -a "$LOG"
if [ $prc -ne 0 ]; then say "PRECOMMIT FAILED for the batch; reverting"; revert_all; exit 5; fi
# Jobs whose patches share a file cannot be committed separately (staging a whole file takes the other job's
# hunks too): group them by file overlap (transitively) and land each group as ONE commit carrying every message.
declare -A owner=(); group=(); n=${#applied[@]}
for ((i=0;i<n;i++)); do group[$i]=$i; done
find_g() { local x=$1; while [ "${group[$x]}" != "$x" ]; do x=${group[$x]}; done; echo "$x"; }
for ((i=0;i<n;i++)); do
  mapfile -t fs <<< "${pfiles[$i]}"
  for f in "${fs[@]}"; do [ -z "$f" ] && continue
    if [ -n "${owner[$f]+x}" ]; then a=$(find_g "${owner[$f]}"); b=$(find_g "$i"); [ "$a" != "$b" ] && group[$b]=$a; else owner[$f]=$i; fi
  done
done
# Build every commit in a private index, chained on the pre-batch HEAD; nothing visible changes yet.
idx=$(mktemp -u "$JET_RUN_ROOT/batch-land.index.XXXXXX"); cm=$(mktemp)
gi() { GIT_INDEX_FILE=$idx GIT_LITERAL_PATHSPECS=1 git "$@"; }
# live <paths...>: the paths the private index can stage. A path one member creates and a later member removes
# (a move within the group) is neither on disk nor tracked; staging it would fail the whole group.
live() { local f; for f in "$@"; do [ -z "$f" ] && continue; if [ -e "$f" ] || gi ls-files --error-unmatch -- "$f" >/dev/null 2>&1; then printf '%s\n' "$f"; fi; done; }
# chain <description> <paths...>: stage the paths, and commit the private index on $tip with message $cm.
chain() { local what=$1 tree; shift; mapfile -t lv < <(live "$@")
  [ ${#lv[@]} -gt 0 ] || { fail="$what: none of its paths is on disk or tracked"; return 1; }
  gi add -A -- "${lv[@]}" || { fail="$what: git add failed"; return 1; }
  tree=$(gi write-tree) || { fail="$what: write-tree failed"; return 1; }
  [ "$tree" != "$(git rev-parse "$tip^{tree}")" ] || { fail="$what: would be an empty commit (its net change is nothing, or an earlier commit of this batch took it: a grouping bug)"; return 1; }
  git stripspace < "$cm" > "$cm.s" && tip=$(git commit-tree "$tree" -p "$tip" -F "$cm.s") || { fail="$what: commit-tree failed"; return 1; }
}
tip=$pre; fail=""; landed=()
gi read-tree "$pre" || fail="read-tree of the pre-batch HEAD failed"
for ((g=0;g<n;g++)); do
  [ -z "$fail" ] || break; [ "$(find_g "$g")" = "$g" ] || continue
  members=(); fsall=(); for ((i=0;i<n;i++)); do [ "$(find_g "$i")" = "$g" ] && members+=("$i"); done
  if [ ${#members[@]} -eq 1 ]; then IFS=$'\t' read -r lane msg patch <<< "${applied[$g]}"; cp "$msg" "$cm"
  else
    first=1
    for i in "${members[@]}"; do IFS=$'\t' read -r lane msg patch <<< "${applied[$i]}"
      if [ $first = 1 ]; then printf '%s (with %d more landed together: shared files)\n\n' "$(head -1 "$msg")" $((${#members[@]}-1)) > "$cm"; first=0; fi
      printf '== %s (%s)\n' "$lane" "$(basename "$patch")" >> "$cm"; cat "$msg" >> "$cm"; printf '\n' >> "$cm"
    done
  fi
  names=(); for i in "${members[@]}"; do IFS=$'\t' read -r lane msg patch <<< "${applied[$i]}"; names+=("$lane $(basename "$patch")"); mapfile -t fs <<< "${pfiles[$i]}"; fsall+=("${fs[@]}"); done
  chain "group [${names[*]}]" "${fsall[@]}" || break
  for nm in "${names[@]}"; do landed+=("$tip $nm"); done
done
if [ -z "$fail" ] && [ ${#genfiles[@]} -gt 0 ]; then
  echo "runtime: regenerated stop exports and lifecycle for the landed runtime routes (runtime generators)" > "$cm"
  chain "regenerated files" "${genfiles[@]}" && landed+=("$tip regenerated")
fi
# The chain must carry exactly the verified worktree: every path the batch changed, nothing else.
if [ -z "$fail" ]; then
  mapfile -d '' changed < <(git diff --name-only -z "$pre")
  full=$(mktemp -u "$JET_RUN_ROOT/batch-land.index.XXXXXX")
  # The verified tree starts from the pre-batch HEAD, so a path the batch deleted counts if HEAD tracked it
  # (the private index has already dropped it, so live() would leave it out and keep it in the verified tree).
  mapfile -t lv < <(for f in "${all[@]}" "${genfiles[@]}" "${changed[@]}"; do [ -z "$f" ] && continue; if [ -e "$f" ] || git cat-file -e "$pre:$f" 2>/dev/null; then printf "%s\n" "$f"; fi; done)
  want=$(GIT_INDEX_FILE=$full GIT_LITERAL_PATHSPECS=1 git read-tree "$pre" && GIT_INDEX_FILE=$full GIT_LITERAL_PATHSPECS=1 git add -A -- "${lv[@]}" && GIT_INDEX_FILE=$full git write-tree)
  rm -f "$full"
  [ -n "$want" ] && [ "$want" = "$(git rev-parse "$tip^{tree}")" ] || fail="the commit chain's tree ${tip:0:10} is not the verified worktree (${want:-none}): some changed path is in no commit"; [ -n "$want" ] && say "tree diff (chain vs verified): $(git diff-tree -r --name-status "$tip^{tree}" "$want" | head -20 | tr "\n" " ")"
fi
# Publish all commits at once: the branch moves only if it is still at the pre-batch HEAD.
[ -z "$fail" ] && { git update-ref -m "batch-land: ${#landed[@]} commit(s) on ${pre:0:10}" HEAD "$tip" "$pre" || fail="update-ref of HEAD ${pre:0:10} -> ${tip:0:10} failed (HEAD moved?)"; }
rm -f "$idx" "$cm" "$cm.s"
if [ -n "$fail" ]; then
  say "!!!!!!!! BATCH COMMIT FAILED: $fail"
  say "!!!!!!!! nothing landed: this batch moved no ref (HEAD $(git rev-parse --short HEAD), pre-batch ${pre:0:10}); reverting the batch's worktree changes"
  for a in "${applied[@]}"; do IFS=$'\t' read -r lane msg patch <<< "$a"; say "COMMIT FAILED $lane $patch"; done
  revert_all; exit 6
fi
# The worktree already is the new HEAD; bring the index along (stat data kept for unchanged paths; the
# worktree is not touched).
{ git read-tree --reset HEAD && git update-index -q --refresh; } || say "!!!!!!!! index sync after landing failed: run git read-tree --reset HEAD in $W"
git hook run --ignore-missing post-commit >/dev/null 2>&1
for l in "${landed[@]}"; do say "COMMITTED $(git rev-parse --short "${l%% *}") ${l#* }"; done
say "cleanup: $(bash "$M/unstick.sh" 2>&1 | tr "\n" " ")"
say "$(bash "$M/publish-latest.sh" 2>&1 | tail -1)"
# Keep the published compiler as base/<sha> for lanes (and seed the chain.sh stage store, queue its golden census).
grep -q '^published install-latest at ' "$LOG" && say "$(bash "$M/publish-base.sh" "$(git rev-parse HEAD)" 2>&1 | tail -1)"
say "DONE HEAD $(git rev-parse --short HEAD)"
bash "$M/release-train.sh" "$(git rev-parse HEAD)"
