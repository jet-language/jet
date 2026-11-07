#!/usr/bin/env bash
# publish-base.sh COMMIT : keep the lander's batch-verified compiler for dev commit COMMIT as a lane baseline.
#   $JET_BASE_ROOT/<sha>/{jet,jet-runtime/,IDENTITY,COMMIT}  (newest 6 published; base/latest -> newest)
#   base/<sha>/goldens.json : one golden census (the Examples rows of `jet test Compiler --profile golden` at the none
#   level, minus GOLD_EXCLUDE, default Systems/Process/OSProcessControl; its --results document) with that compiler
#   and its pack, from an export of <sha>;
#   goldens.meta says how it ran, goldens.failed replaces it when the census produced no counts.
# Source: $JET_SCRATCH_ROOT/discovery/b2/install-latest, which must say COMMIT (publish-latest.sh runs first).
# It also stores that compiler as the s1 (and its pack as pack1) of the census tree in the chain.sh stage store, so a
# lane's `chain.sh TREE s1|full` of an unmodified HEAD reuses it (chain.sh seed-published checks the claim).
# Goldens: queued in build-slot.sh with SLOT_SMALL=0 by one detached worker (systemd unit base-goldens-*); at most
# one is pending, and a run takes the newest published commit when its slot starts, so older pending ones are skipped.
# Called by batch-land.sh after publish-latest.sh; prints one summary line last. Roots: Tools/agent/roots.sh.
set -uo pipefail
ulimit -c 0
M=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$M/../roots.sh"
SELF=$M/publish-base.sh
SLOT=$M/../chain/build-slot.sh
CHAIN=$M/../chain/chain.sh
REPO=$JET_DEV_WORKTREE
IL=$JET_SCRATCH_ROOT/discovery/b2/install-latest
B=$JET_BASE_ROOT
KEEP=6
mkdir -p "$B"
say() { echo "$(date '+%F %T') $*"; }

# Exits with the want lock held by the caller's fd 7 so a publish between "nothing left" and exit is never lost.
golden_worker() {
  exec 8>"$B/.golden-worker.lock"; flock -n 8 || exit 0
  while :; do
    exec 7>"$B/.golden-want.lock"; flock 7
    local want; want=$(awk 'NR==1' "$B/.golden-want" 2>/dev/null)
    if [ -z "$want" ] || [ ! -d "$B/$want" ] || [ -e "$B/$want/goldens.json" ] || [ -e "$B/$want/goldens.failed" ]; then
      exec 8>&-; exit 0
    fi
    exec 7>&-
    say "worker: queueing a golden run (want ${want:0:10})"
    SLOT_SMALL=0 bash "$SLOT" bash "$SELF" --golden-run 7>&- 8>&-
  done
}

golden_run() { # inside the slot: census the newest wanted commit
  local sha d w t s rc sel n out counts
  sha=$(awk 'NR==1' "$B/.golden-want" 2>/dev/null); d=$B/$sha
  [ -n "$sha" ] && [ -d "$d" ] && [ ! -e "$d/goldens.json" ] && [ ! -e "$d/goldens.failed" ] || exit 0
  w=$B/.work/${sha:0:9}; t=$w/tree
  rm -rf "$w"; mkdir -p "$t" "$w/state" "$w/tmp"
  trap "rm -rf '$w'" EXIT
  say "goldens ${sha:0:10}: export"
  git -C "$REPO" archive "$sha" | tar -x -C "$t" || { echo "export failed" > "$d/goldens.failed"; exit 1; }
  local ex=${GOLD_EXCLUDE-Systems/Process/OSProcessControl}
  sel=$w/select.txt; out=$w/goldens.json
  # The census rows: the golden profile's Examples rows (`--list`: suite<TAB>path) minus the GOLD_EXCLUDE example
  # stems; each stem must drop at least one row, or the exclusion could not hold.
  (cd "$t" && JET_COMPILER_IDENTITY=$(<"$d/IDENTITY") JET_TOOLCHAIN_ROOT=$t "$d/jet" test Compiler --profile golden --optimize=none --list) \
    > "$w/listed.txt" 2> "$w/goldens.err"
  awk -F'\t' -v ex="$ex" '
    BEGIN { n = split(ex, x, " ") }
    $1 == "golden" && index($2, "Examples/") == 1 {
      for (i = 1; i <= n; i++) if ($2 == "Examples/" x[i] ".jet" || $2 == "Examples/" x[i] || index($2, "Examples/" x[i] "/") == 1) { hit[i]++; next }
      print $2
    }
    END { for (i = 1; i <= n; i++) if (!hit[i]) { print "exclusion " x[i] " matched no example" > "/dev/stderr"; exit 1 } }' \
    "$w/listed.txt" > "$sel" 2>> "$w/goldens.err" && [ -s "$sel" ] \
    || { { echo "golden selection failed"; awk '{l[NR]=$0} END{for(i=(NR>20?NR-19:1);i<=NR;i++) print l[i]}' "$w/goldens.err"; } > "$d/goldens.failed"; exit 1; }
  n=$(wc -l < "$sel")
  s=$(date +%s)
  say "goldens ${sha:0:10}: $n examples with $d/jet"
  # Golden examples run host tools (C compilers, interpreters) that only the dev shell
  # (Tools/agent/JetEnv.jet) supplies until jet does.
  (cd "$t" && JET_COMPILER_IDENTITY=$(<"$d/IDENTITY") JET_TOOLCHAIN_ROOT=$t JET_TEST_STATE_DIR=$w/state TMPDIR=$w/tmp \
    systemd-run --user --scope -q --expand-environment=no -p MemoryMax=24G -p MemorySwapMax=0 timeout 7200 \
    prlimit --core=0 --stack=1073741824:1073741824 -- ${SLOT_CPUS:+taskset -c "$SLOT_CPUS"} "$d/jet" "$t/Tools/agent/JetEnv.jet" \
    "$d/jet" test Compiler --profile golden --optimize=none "--cases=$sel" --jobs=6 --time-limit=120s "--results=$out" \
    > "$w/goldens.log" 2> "$w/goldens.err")
  rc=$?
  [ -d "$d" ] || exit 0
  # The results document opens with the compiler and its counts: {"jet":…,"counts":{…,"suites":[…]},"results":[…]}.
  counts=$(awk 'match($0, /"counts":\{[^}]*\}/) { print substr($0, RSTART + 9, RLENGTH - 9); exit }' "$out" 2>/dev/null)
  if [ -n "$counts" ]; then
    mv "$out" "$d/goldens.json.tmp" && mv "$d/goldens.json.tmp" "$d/goldens.json"
    printf '%s\n' "commit=$sha" "compiler=$d/jet" "identity=$(<"$d/IDENTITY")" "examples=$n" "exclude=$ex" \
      "args=--profile golden --optimize=none --cases --jobs=6 --time-limit=120s" "rc=$rc" "secs=$(( $(date +%s) - s ))" "cpus=${SLOT_CPUS:-}" \
      "counts=$counts" "finished=$(date '+%F %T')" > "$d/goldens.meta"
    say "goldens ${sha:0:10}: $(awk -F= '$1=="counts"{print $2}' "$d/goldens.meta") in $(( $(date +%s) - s ))s"
  else
    { echo "rc=$rc: no counts after $(( $(date +%s) - s ))s; results bytes $(wc -c < "$out" 2>/dev/null)"; echo "-- stderr tail"; awk '{l[NR]=$0} END{for(i=(NR>60?NR-59:1);i<=NR;i++) print l[i]}' "$w/goldens.err"; echo "-- stdout tail"; awk '{l[NR]=$0} END{for(i=(NR>20?NR-19:1);i<=NR;i++) print substr(l[i],1,400)}' "$w/goldens.log"; } > "$d/goldens.failed"
    cp "$w/goldens.err" "$d/goldens.err" 2>/dev/null; cp "$w/goldens.log" "$d/goldens.stdout" 2>/dev/null
    say "goldens ${sha:0:10}: FAILED rc=$rc"
  fi
  jetpack_build "$sha" "$t" "$w/tmp"
}

# Hosted jetpack engine for base/<sha>: base/<sha>/jetpack + jetpack.receipt. A published jetpack is reused when
# the two commits differ in no $JP_INPUTS path (the engine is built only from those plus the compiler they make).
JP_INPUTS=(Jetpack Core Compiler)
jetpack_reuse() { # SHA : 0 when base/SHA/jetpack exists or was copied from an input-identical published commit
  local sha=$1 d=$B/$1 p prev
  [ -x "$d/jetpack" ] && return 0
  [ -d "$d" ] || return 1
  for p in $(ls -1t --time=mtime -d "$B"/*/jetpack 2>/dev/null); do
    prev=$(basename "$(dirname "$p")")
    [ "$prev" != "$sha" ] && [ -f "$B/$prev/jetpack.receipt" ] || continue
    git -C "$REPO" diff --quiet "$prev" "$sha" -- "${JP_INPUTS[@]}" 2>/dev/null || continue
    # Receipt first: Tools/agent/Suite.jet keys on the jetpack file, so it never sees a jetpack without its receipt.
    { awk 1 "$B/$prev/jetpack.receipt"; echo "reused-for: $sha"; echo "reused-from: $prev (no change under ${JP_INPUTS[*]} between them)"; } > "$d/.jetpack.receipt.tmp" \
      && mv "$d/.jetpack.receipt.tmp" "$d/jetpack.receipt" || return 1
    cp "$p" "$d/.jetpack.tmp" && mv "$d/.jetpack.tmp" "$d/jetpack" || return 1
    return 0
  done
  return 1
}

jetpack_build() { # SHA TREE TMP : inside a slot; TREE is an export of SHA
  local sha=$1 t=$2 tmp=$3 d=$B/$1 worker bin rc s receipt
  [ -d "$d" ] || return 0
  if jetpack_reuse "$sha"; then say "jetpack ${sha:0:10}: $(awk -F': ' '$1=="reused-from"{r=substr($2,1,10)} END{print r=="" ? "present" : "reused from " r}' "$d/jetpack.receipt")"; return 0; fi
  worker=base-${sha:0:9}; s=$(date +%s)
  say "jetpack ${sha:0:10}: hosted build with $d/jet"
  mkdir -p "$tmp"
  (cd "$t" && JETPACK_WORKER=$worker JET=$d/jet JET_COMPILER_IDENTITY=$(<"$d/IDENTITY") JET_TOOLCHAIN_ROOT=$t TMPDIR=$tmp \
    systemd-run --user --scope -q --expand-environment=no -p MemoryMax=16G -p MemorySwapMax=0 timeout 6000 \
    prlimit --core=0 -- ${SLOT_CPUS:+taskset -c "$SLOT_CPUS"} "$d/jet" Jetpack/Bootstrap/Check.jet --hosted) > "$d/jetpack.log" 2>&1
  rc=$?
  bin=$(ls -dt "$JET_STATE_ROOT"/jetpack-bootstrap/$worker-*/hosted/project/.jet/build/jetpack 2>/dev/null | awk 'NR==1')
  if [ $rc -ne 0 ] || [ ! -x "$bin" ]; then echo "rc=$rc bin=${bin:-none}; see jetpack.log" > "$d/jetpack.failed"; say "jetpack ${sha:0:10}: FAILED rc=$rc"; return 1; fi
  receipt=${bin%/hosted/project/.jet/build/jetpack}/hosted.receipt
  cp "$bin" "$d/.jetpack.tmp" && mv "$d/.jetpack.tmp" "$d/jetpack" || return 1
  { [ -f "$receipt" ] && awk 1 "$receipt"; echo "commit: $sha"; echo "secs: $(( $(date +%s) - s ))"; } > "$d/.jetpack.receipt.tmp" && mv "$d/.jetpack.receipt.tmp" "$d/jetpack.receipt"
  rm -f "$d/jetpack.failed"
  rm -rf "$JET_STATE_ROOT"/jetpack-bootstrap/$worker-*
  say "jetpack ${sha:0:10}: built in $(( $(date +%s) - s ))s sha256=$(sha256sum "$d/jetpack" | awk '{print substr($1,1,12)}')"
}

jetpack_run() { # SHA : inside a slot, give an already-published base its jetpack (reuse, else export and build)
  local sha d w
  sha=$(git -C "$REPO" rev-parse --verify "$1^{commit}") || { echo "unknown commit $1"; exit 1; }
  d=$B/$sha; [ -d "$d" ] || { echo "not published: $d"; exit 1; }
  jetpack_reuse "$sha" && { say "jetpack ${sha:0:10}: present or reused"; exit 0; }
  w=$B/.work/jp-${sha:0:9}; rm -rf "$w"; mkdir -p "$w/tree"
  trap "rm -rf '$w'" EXIT
  git -C "$REPO" archive "$sha" | tar -x -C "$w/tree" || { echo "export failed"; exit 1; }
  jetpack_build "$sha" "$w/tree" "$w/tmp"
}

case ${1:-} in
  --golden-worker) golden_worker; exit;;
  --golden-run) golden_run; exit;;
  --jetpack-run) [ -n "${2:-}" ] || { echo "usage: publish-base.sh --jetpack-run COMMIT (inside build-slot.sh)"; exit 64; }; jetpack_run "$2"; exit;;
  ""|-*) echo "usage: publish-base.sh COMMIT"; exit 64;;
esac

sha=$(git -C "$REPO" rev-parse --verify "$1^{commit}") || { echo "publish-base: unknown commit $1"; exit 1; }
il=$(awk 'NR==1' "$IL/COMMIT" 2>/dev/null)
[ "$il" = "$sha" ] || { echo "publish-base: install-latest is ${il:0:10}, not ${sha:0:10}; nothing published"; exit 1; }
d=$B/$sha
if [ -d "$d" ]; then state="already published"
else
  t=$B/.new.$$; rm -rf "$t"; mkdir -p "$t"
  cp "$IL/jet" "$IL/IDENTITY" "$IL/COMMIT" "$t/" && cp -r "$IL/jet-runtime" "$t/jet-runtime" || { rm -rf "$t"; echo "publish-base: copy failed"; exit 1; }
  [ "$(awk 'NR==1' "$t/COMMIT")" = "$sha" ] && cmp -s "$t/jet" "$IL/jet" || { rm -rf "$t"; echo "publish-base: install-latest changed while copying; nothing published"; exit 1; }
  date '+%F %T' > "$t/.published"
  # The published compiler is stage s1 (built by the seed). A lane A/B must compare s2 with s2; this file says so.
  printf '%s\n' "generation=s1" "built_by_seed=$(readlink "$JET_SEED")" \
    "note=an A/B of a lane s2 against this compiler compares two code generations; build a base s2 for same-generation A/Bs" > "$t/GENERATION"
  mv -T "$t" "$d" 2>/dev/null || rm -rf "$t"
  state="published"
fi
ln -sfn "$sha" "$B/.latest.$$" && mv -T "$B/.latest.$$" "$B/latest"
# Keep the newest $KEEP by publish time.
# Keep the KEEP newest bases, plus the newest one that holds a finished goldens.json, so lanes always have a
# golden baseline to compare against instead of running their own.
gkeep=$(ls -1t --time=mtime -d "$B"/*/goldens.json 2>/dev/null | awk 'NR==1')
ls -1t --time=mtime -d "$B"/*/.published 2>/dev/null | awk -v k="$KEEP" 'NR>k' | while read -r p; do [ -n "$gkeep" ] && [ "${p%/.published}" = "${gkeep%/goldens.json}" ] && continue; rm -rf "${p%/.published}"; done
seeded=$(bash "$CHAIN" seed-published "$d" 2>&1 | awk 'END{print}')
# A jetpack whose inputs did not change is copied now; otherwise the golden run builds one after its census.
jp=$(jetpack_reuse "$sha" && echo "jetpack ready" || echo "jetpack build follows the golden census")
# One golden census per published commit: record it as wanted; start the worker unless one is alive (it re-reads the
# want before exiting, under the same lock).
(
  flock 7
  echo "$sha" > "$B/.golden-want.new" && mv "$B/.golden-want.new" "$B/.golden-want"
  if flock -n "$B/.golden-worker.lock" true; then
    # A user unit starts from the user manager's environment: carry this caller's PATH and roots into it.
    systemd-run --user --unit="base-goldens-${sha:0:9}-$(date +%s)" --collect -q --expand-environment=no -p MemoryMax=4G -p MemorySwapMax=0 -p LimitCORE=0 \
      -E PATH -E JET_STATE_ROOT -E JET_SCRATCH_ROOT -E JET_RUN_ROOT -E JET_BASE_ROOT -E JET_SEEDS -E JET_SEED -E JET_DEV_WORKTREE \
      ${JET_PACK_MUSL_LIB:+-E JET_PACK_MUSL_LIB} \
      bash -c "exec bash '$SELF' --golden-worker >> '$B/goldens-worker.log' 2>&1" < /dev/null > /dev/null 2>&1 && echo "golden run queued" || echo "golden worker failed to start"
  else echo "golden run wanted (worker busy; it takes the newest)"; fi
) 7>"$B/.golden-want.lock" > "$B/.golden-state" 2>&1
echo "publish-base: $state $d; $seeded; $jp; $(awk 'END{print}' "$B/.golden-state")"
