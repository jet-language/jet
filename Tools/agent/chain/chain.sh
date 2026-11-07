#!/usr/bin/env bash
# chain.sh : the shared self-host chain (seed -> s1 -> s2 -> s3) with content-addressed stage reuse.
# Roots come from Tools/agent/roots.sh; JET_DEV_WORKTREE names the dev checkout and JET_PACK_MUSL_LIB the musl dir.
#
#   chain.sh export DIR [COMMIT] [PATCH...]
#       fresh `git archive COMMIT` (default dev HEAD) of the dev worktree in DIR, then `git apply` each PATCH;
#       DIR/.chain-base records COMMIT (the base --auto and `check` compare against).
#   chain.sh TREE LEVEL|--auto [--out DIR] [--base COMMIT] [--dry-run|--check-gate]
#       LEVEL check        : seed `jet check` of the packages TREE changes against its base (no build)
#             s1           : s1 (seed builds Compiler) + pack1 (runtime pack by s1)
#             full         : s1, pack1, s2 (s1 builds), pack2, s3 (s2 builds), FIXED POINT s2 == s3
#             full+threads : full, plus t1 (s2 builds with --threads 1) compared with s3
#       --auto picks LEVEL from the paths TREE changes against its base and prints why.
#       --dry-run prints the level, the stage keys it can resolve and which stages the store holds; builds nothing.
#       full and full+threads (self-hosting) start only with TREE's collect-all preflight receipt for the seed
#       (`Tools/agent/Land.jet receipt`, written by `Land.jet census`); --check-gate says which and stops.
#       Output (default TREE/.chain): RESULT (TSV: stage status rc wall_s peak_rss_mb sha256 key note; last row
#       `verdict`), REPORT.md, s1/ s2/ s3/ t1/ (jet [+ jet.o], jet-runtime/ beside it), logs/, keys/.
#
# Stage reuse: each stage's key is sha256 over its builder compiler's bytes, the content hash of the runtime pack
# beside the builder, the content hash of exactly the tree inputs the stage reads (chain-lib.mjs `inputs`), the
# identity, flags and env that reach the output, and the canonical build root. A store entry
# ($JET_SCRATCH_ROOT/stagestore/<key>/, or $JET_RUN_ROOT/stagestore when the scratch root is short of space; LRU,
# capped) is copied instead of rebuilding and the stage reads REUSED. Entries are built in a temp dir and
# renamed in, and only clean stages (rc 0, no keep-going rows) are stored.
# Canonical root: a built compiler embeds its build tree's path, so every build and pack runs with TREE
# bind-mounted at $JET_SCRATCH_ROOT/discovery/r14/snap in a private mount namespace (same uid). That is the
# lander's census root, so the compiler the lander publishes is the s1 of HEAD byte for byte and publish-base.sh
# seeds the store with it. The real snap is never touched: the namespace mounts its own dir over r14.
# Slots: chain.sh queues itself in build-slot.sh once, for every stage the store does not hold (SLOT_SMALL=1 when
# only runtime packs or the check remain); a fully reused chain takes no slot. Never wrap chain.sh in
# build-slot.sh yourself (inside a slot, i.e. SLOT_CPUS set, it runs inline).
set -uo pipefail
ulimit -c 0
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$HERE/../roots.sh"
SELF=$HERE/chain.sh
LIB=$HERE/chain-lib.mjs
SLOT=$HERE/build-slot.sh
NR=$HERE/norust.sh
TOOLS=$(cd "$HERE/../../.." && pwd)
# tool FILE ARG...: the Jet tool Tools/agent/FILE of this checkout, run by the seed.
tool() { (cd "$TOOLS" && JET_COMPILER_IDENTITY=$CID JET_TOOLCHAIN_ROOT=$TOOLS "$SEED/jet" "Tools/agent/$1" "${@:2}"); }
REPO=$JET_DEV_WORKTREE
SEEDS=$JET_SEEDS
R14=$JET_SCRATCH_ROOT/discovery/r14
ROOT=$R14/snap
STORES=("$JET_SCRATCH_ROOT/stagestore" "$JET_RUN_ROOT/stagestore")
export CARGO_INCREMENTAL=0 JET_TARGET_CAP_GB=120
unset CARGO_TARGET_DIR CARGO_HOME RUSTUP_HOME RUSTC
usage() { awk 'NR>=2 && /^#/{sub(/^# ?/,""); print; next} NR>=2{exit}' "$SELF" | awk 'NR<=19'; exit 64; }

if [ "${1:-}" = export ]; then
  D=${2:?usage: chain.sh export DIR [COMMIT] [PATCH...]}; C=${3:-HEAD}; shift; shift; [ $# -gt 0 ] && shift
  sha=$(git -C "$REPO" rev-parse --verify "$C^{commit}") || exit 2
  case $(realpath -m "$D") in "$R14"|"$R14"/*) echo "refuse: $D is under $R14"; exit 2;; esac
  rm -rf "$D" && mkdir -p "$D" && git -C "$REPO" archive "$sha" | tar -x -C "$D" || { echo "EXPORT FAILED"; exit 1; }
  for p in "$@"; do (cd "$D" && git apply "$(realpath "$p")") || { echo "APPLY FAILED $p"; exit 1; }; done
  echo "$sha" > "$D/.chain-base"
  echo "exported ${sha:0:10} + $# patch(es) to $D"
  exit 0
fi
[ -n "${JET_PACK_MUSL_LIB:-}" ] || { echo "chain.sh: set JET_PACK_MUSL_LIB to the static musl lib dir (in the environment or the jet env file Tools/agent/roots.sh reads)"; exit 2; }
export JET_PACK_MUSL_LIB

# seed-published INSTALL_DIR (publish-base.sh): INSTALL_DIR holds the compiler the lander's census built at the
# canonical root; it is that tree's s1 (and its pack, pack1), so store both under the keys a chain computes.
SEEDPUB=
if [ "${1:-}" = seed-published ]; then
  SEEDPUB=$(realpath "${2:?usage: chain.sh seed-published INSTALL_DIR}") || exit 2
  set -- "$ROOT" s1 --out "$JET_RUN_ROOT/chain-work/seedpub-$$"
fi
INNER=; [ "${1:-}" = --inner ] && { INNER=1; shift; }
[ $# -ge 1 ] || usage
TREE=$(realpath "$1") || exit 2; shift
LEVEL=; AUTO=; OUT=; BASE=; DRY=; GATE=
while [ $# -gt 0 ]; do
  case $1 in
    --auto) AUTO=1;; --dry-run) DRY=1;; --check-gate) GATE=1;;
    --out) OUT=${2:?--out DIR}; shift;; --base) BASE=${2:?--base COMMIT}; shift;;
    check|s1|full|full+threads) LEVEL=$1;;
    *) echo "chain.sh: unknown argument $1"; usage;;
  esac; shift
done
[ -n "$LEVEL$AUTO" ] || usage
[ -d "$TREE/Compiler" ] && [ -d "$TREE/Tools/agent" ] || { echo "chain.sh: $TREE is not a source export"; exit 2; }
[ -z "$SEEDPUB" ] && case $TREE in "$R14"|"$R14"/*) echo "chain.sh: refuse a tree under $R14 (the canonical root)"; exit 2;; esac
base_src=${CHAIN_BASE_SRC:---base}
if [ -z "$BASE" ]; then
  if [ -f "$TREE/.chain-base" ]; then BASE=$(awk 'NR==1' "$TREE/.chain-base"); base_src=".chain-base"
  else BASE=HEAD; base_src="dev HEAD (no .chain-base; pass --base)"; fi
fi
BASE=$(git -C "$REPO" rev-parse --verify "$BASE^{commit}") || { echo "chain.sh: unknown base $BASE"; exit 2; }
OUT=$(realpath -m "${OUT:-$TREE/.chain}")
LOGS=$OUT/logs
mkdir -p "$OUT" "$LOGS" "$OUT/keys"
SEED=$(readlink -f "${CHAIN_SEED:-$SEEDS/current}"); CID=$(<"$SEED/IDENTITY"); VER=${CID%%#*}
MEM=${CHAIN_MEMMAX:-24G}
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN{printf "%.1f", b-a}'; }
sha() { sha256sum < "$1" | awk '{print $1}'; }
say() { echo "$(date +%T) $*"; }
STORE=${CHAIN_STORE:-}
if [ -z "$STORE" ]; then
  if [ "$(df --output=avail -BG "$JET_SCRATCH_ROOT" | awk 'NR==2{print int($1)}')" -ge 40 ]; then STORE=${STORES[0]}; CAP=20; else STORE=${STORES[1]}; CAP=4; fi
fi
CAP=${CHAIN_STORE_CAP_GB:-${CAP:-20}}
mkdir -p "$STORE"

# The canonical root: CMD runs from $ROOT with $TREE mounted there (and our own empty dir over r14, so nothing
# the lander deletes is a mount point of ours), as the same uid/gid.
# JET_NIX_TMP_CLEANED=1: a user namespace cannot see host processes, so a tree older than dev 020b890bb8
# (#6131) would run a blind clean-nix-tmp.sh there and delete every live /tmp/nix-shell.* dir.
NSDIR=$JET_RUN_ROOT/chain-ns/$$
inroot() {
  mkdir -p "$NSDIR/snap"
  unshare -r -m --propagation private bash -c 'mount --bind "$1" "$3" && mount --bind "$2" "$4" || { echo "chain: canonical root bind failed" >&2; exit 97; }
    cd "$4" || exit 97; u=$5 g=$6; shift 6
    exec env JET_NIX_TMP_CLEANED=1 unshare --user --map-user="$u" --map-group="$g" "$@"' chain-ns "$NSDIR" "$TREE" "$R14" "$ROOT" "$(id -u)" "$(id -g)" "$@"
}
WORK=$JET_RUN_ROOT/chain-work/$$
mkdir -p "$WORK/tmp"
cleanup() { rm -rf "$WORK"; rmdir "$NSDIR/snap" "$NSDIR" 2>/dev/null; }
trap cleanup EXIT

meta() { awk -v k="$2" 'index($0, k "=") == 1 {print substr($0, length(k) + 2); exit}' "$1/meta"; }
key() { printf '%s\n' chain-stage-v1 "$@" "root=$ROOT" | sha256sum | awk '{print $1}'; }
find_entry() { local s; for s in "$STORE" "${STORES[@]}"; do [ -f "$s/$1/meta" ] && { echo "$s/$1"; return 0; }; done; return 1; }
# Plain copies, never hard links: the compiler refuses source and pack inputs with more than one link (E2105).
copy_file() { cp "$1" "$2" && chmod u+w "$2"; }
copy_dir() { cp -a "$1" "$2" && chmod -R u+w "$2"; }
errhead() { awk '!/^norust: /{if(++n<=2)print}' "$1" | tr '\t\n' '  ' | cut -c1-160; }
store_put() { # KEY KIND SRC_DIR ; meta lines on stdin
  local k=$1 kind=$2 src=$3 tmp=$STORE/.tmp.$1.$$ f
  [ -d "$STORE/$k" ] && { cat > /dev/null; return 0; }
  rm -rf "$tmp"; mkdir -p "$tmp"
  case $kind in
    build) for f in jet jet.o; do [ -e "$src/$f" ] && copy_file "$src/$f" "$tmp/$f"; done;;
    pack) copy_dir "$src/jet-runtime" "$tmp/jet-runtime";;
    check) cp "$src"/* "$tmp/";;
  esac
  { cat; echo "created=$(date '+%F %T')"; echo "bytes=$(du -sb "$tmp" | awk '{print $1}')"; } > "$tmp/meta"
  find "$tmp" -type f -exec chmod a-w {} +
  touch "$tmp/.used"
  mv -T "$tmp" "$STORE/$k" 2>/dev/null || rm -rf "$tmp"
  ( flock 8; node "$LIB" evict "$STORE" "$CAP" > "$LOGS/evict.log" 2>&1 ) 8>"$STORE/.evict.lock"
}

# Stage results: one TSV row each (stage status rc wall_s peak_rss_mb sha256 key note).
RES=$OUT/RESULT.part
rec() { printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$@" >> "$RES"; }
timefile() { # TIME_FILE -> "wall_s rss_mb"
  awk -F': ' '/Elapsed \(wall clock\)/{n=split($2,p,":"); s=0; for(i=1;i<=n;i++) s=s*60+p[i]} /Maximum resident/{r=int($2/1024)} END{printf "%.1f %d", s, r}' "$1" 2>/dev/null || echo "0 0"
}
lane_of() { printf '%s' "$TREE" | awk 'match($0,/(tdev|lanes)\/[A-Za-z0-9]+/){split(substr($0,RSTART,RLENGTH),a,"/"); print a[2]; exit} END{}'; }
CPUSET=(); [ -n "${SLOT_CPUS:-}" ] && CPUSET=(taskset -c "$SLOT_CPUS")
declare -A SHA PK
MISS=
build_mat() { MAT=("kind=build" "builder=$1" "builderpack=$2" "inputs=$BIN" "identity=$3" "args=build --threads $4 Compiler" "env=JET_KEEP_GOING=1"); }
pack_mat() { MAT=("kind=pack" "compiler=$1" "inputs=$PIN" "identity=$2" "musl=$JET_PACK_MUSL_LIB" "args=PrebuildRuntime.jet"); }

# build_stage NAME BUILDER_DIR BUILDER_SHA BUILDER_PACKHASH IDENTITY THREADS
build_stage() {
  local n=$1 bd=$2 bs=$3 bp=$4 id=$5 th=$6 t0 e k
  build_mat "$bs" "$bp" "$id" "$th"; local mat=("${MAT[@]}")
  k=$(key "${mat[@]}"); printf '%s\n' "${mat[@]}" > "$OUT/keys/$n.txt"
  t0=$(now)
  if e=$(find_entry "$k"); then
    SHA[$n]=$(meta "$e" sha256)
    if [ "$MODE" != dry ]; then
      mkdir -p "$OUT/$n"; rm -f "$OUT/$n/jet" "$OUT/$n/jet.o"
      copy_file "$e/jet" "$OUT/$n/jet"; [ -e "$e/jet.o" ] && copy_file "$e/jet.o" "$OUT/$n/jet.o"
      if [ "$(sha "$OUT/$n/jet")" != "${SHA[$n]}" ]; then rm -f "$OUT/$n/jet"; e=; fi
    fi
    if [ -n "$e" ]; then
      touch "$e/.used" 2>/dev/null
      rec "$n" REUSED 0 "$(since "$t0")" "$(meta "$e" rss_mb)" "${SHA[$n]}" "$k" "built $(meta "$e" created) in $(meta "$e" wall_s)s by $(meta "$e" lane)"
      say "$n REUSED ${k:0:16} (jet ${SHA[$n]:0:16})"
      return 0
    fi
  fi
  if [ "$MODE" != run ]; then MISS=$n; say "$n needs a build (key ${k:0:16})"; return 3; fi
  say "$n: $(basename "$bd") builds Compiler (--threads $th)"
  rm -rf "$TREE/Compiler/JetCli/.jet/build" "$OUT/$n"; mkdir -p "$OUT/$n"
  JET_COMPILER_IDENTITY=$id JET_TOOLCHAIN_ROOT=$ROOT JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$LOGS/$n-keep-going.tsv" TMPDIR=$WORK/tmp \
    inroot systemd-run --user --scope -q -p MemoryMax="$MEM" -p MemorySwapMax=0 env time -v -o "$LOGS/$n.time" timeout 3600 \
    prlimit --core=0 --stack=1073741824:1073741824 -- "${CPUSET[@]}" bash "$NR" "$bd/jet" build --threads "$th" Compiler > "$LOGS/$n.out" 2> "$LOGS/$n.err"
  local rc=$? rows; rows=$(awk 'NR>1' "$LOGS/$n-keep-going.tsv" 2>/dev/null | wc -l)
  local w r; read -r w r <<< "$(timefile "$LOGS/$n.time")"
  local b=$TREE/Compiler/JetCli/.jet/build/jet
  if [ -x "$b" ]; then
    cp "$b" "$OUT/$n/jet"; [ -f "${b}.o" ] && cp "${b}.o" "$OUT/$n/jet.o"
  fi
  rm -rf "$TREE/Compiler/JetCli/.jet/build"
  if [ ! -x "$OUT/$n/jet" ]; then
    rec "$n" FAILED "$rc" "$w" "$r" - "$k" "no binary; keep-going rows=$rows; $(errhead "$LOGS/$n.err")"
    say "$n FAILED rc=$rc rows=$rows (logs $LOGS/$n.err)"; return 1
  fi
  SHA[$n]=$(sha "$OUT/$n/jet")
  # #5662: a seed without the census fix installs keep-going `todo` traps with no row; the binary tells.
  local traps=0
  tool Chain.jet trapscan "$OUT/$n/jet" > "$LOGS/$n-traps.txt"
  case $? in 0) ;; 1) traps=$(wc -l < "$LOGS/$n-traps.txt") ;; *) traps=scan-failed ;; esac
  if [ "$rc" = 0 ] && [ "$rows" = 0 ] && [ "$traps" = 0 ]; then
    printf '%s\n' "stage=$n" "kind=build" "sha256=${SHA[$n]}" "wall_s=$w" "rss_mb=$r" "lane=$(lane_of)" "tree=$TREE" "${mat[@]}" | store_put "$k" build "$OUT/$n"
    rec "$n" BUILT 0 "$w" "$r" "${SHA[$n]}" "$k" "stored"
  else
    rec "$n" DIRTY "$rc" "$w" "$r" "${SHA[$n]}" "$k" "binary with rc=$rc keep-going rows=$rows traps=$traps (not stored; $LOGS/$n-traps.txt)"
  fi
  say "$n built rc=$rc rows=$rows traps=$traps wall ${w}s rss ${r}MB (jet ${SHA[$n]:0:16})"
}

# pack_stage NAME COMPILER_DIR COMPILER_SHA IDENTITY : the runtime pack COMPILER_DIR/jet-runtime
pack_stage() {
  local n=$1 cd=$2 cs=$3 id=$4 t0 e k
  pack_mat "$cs" "$id"; local mat=("${MAT[@]}")
  k=$(key "${mat[@]}"); printf '%s\n' "${mat[@]}" > "$OUT/keys/$n.txt"
  t0=$(now)
  if e=$(find_entry "$k"); then
    PK[$n]=$(meta "$e" packhash)
    if [ "$MODE" != dry ]; then
      rm -rf "$cd/jet-runtime"
      copy_dir "$e/jet-runtime" "$cd/jet-runtime"
      [ "$(node "$LIB" dirhash "$cd/jet-runtime")" = "${PK[$n]}" ] || { rm -rf "$cd/jet-runtime"; e=; }
    fi
    if [ -n "$e" ]; then
      touch "$e/.used" 2>/dev/null
      rec "$n" REUSED 0 "$(since "$t0")" "$(meta "$e" rss_mb)" "${PK[$n]}" "$k" "native.key $(meta "$e" native_key | cut -c1-16); built $(meta "$e" created) in $(meta "$e" wall_s)s by $(meta "$e" lane)"
      say "$n REUSED ${k:0:16} (pack ${PK[$n]:0:16})"
      return 0
    fi
  fi
  if [ "$MODE" != run ]; then MISS=$n; say "$n needs a build (key ${k:0:16})"; return 3; fi
  say "$n: PrebuildRuntime.jet with $(basename "$cd")"
  rm -rf "$cd/jet-runtime"; mkdir -p "$WORK/tmp"
  # The seed runs the tool; the stage's compiler lowers the runtime under the stage identity. The pack compiles
  # the runtime's vendored C with a host C toolchain, which only jet-env supplies until jet does.
  JET_COMPILER_IDENTITY=$CID JET=$SEED/jet JET_TOOLCHAIN_ROOT=$ROOT TMPDIR=$WORK/tmp \
    inroot systemd-run --user --scope -q -p MemoryMax="$MEM" -p MemorySwapMax=0 env time -v -o "$LOGS/$n.time" timeout 1800 \
    prlimit --core=0 --stack=1073741824:1073741824 -- "${CPUSET[@]}" "$ROOT/Tools/agent/jet-env" bash "$NR" "$SEED/jet" "$ROOT/Tools/packaging/PrebuildRuntime.jet" "$cd/jet" "$cd/jet-runtime" --repo "$ROOT" --identity "$id" > "$LOGS/$n.out" 2> "$LOGS/$n.err"
  local rc=$? w r; read -r w r <<< "$(timefile "$LOGS/$n.time")"
  if [ "$rc" != 0 ] || [ ! -f "$cd/jet-runtime/native.key" ]; then
    rec "$n" FAILED "$rc" "$w" "$r" - "$k" "no pack; $(errhead "$LOGS/$n.err")"
    say "$n FAILED rc=$rc (logs $LOGS/$n.err)"; return 1
  fi
  PK[$n]=$(node "$LIB" dirhash "$cd/jet-runtime")
  local nk; nk=$(awk 'NR==1' "$cd/jet-runtime/native.key")
  printf '%s\n' "stage=$n" "kind=pack" "packhash=${PK[$n]}" "native_key=$nk" "wall_s=$w" "rss_mb=$r" "lane=$(lane_of)" "tree=$TREE" "${mat[@]}" | store_put "$k" pack "$cd"
  rec "$n" BUILT 0 "$w" "$r" "${PK[$n]}" "$k" "native.key ${nk:0:16}; stored"
  say "$n built wall ${w}s rss ${r}MB (pack ${PK[$n]:0:16}, native.key ${nk:0:16})"
}

# check_stage : seed `jet check` of each member (from TREE, no canonical root: check writes no artifact).
check_stage() {
  local t0 e k m n rc rows w r all=0 sum=() mat
  mat=("kind=check" "seed=$SEED_SHA" "seedpack=$SEED_PK" "base=$BASE" "diff=$DIGEST" "members=${MEMBERS[*]:-}" "identity=$CID" "args=check --threads 12" "env=JET_KEEP_GOING=1")
  k=$(key "${mat[@]}"); printf '%s\n' "${mat[@]}" > "$OUT/keys/check.txt"
  t0=$(now)
  if [ ${#MEMBERS[@]} = 0 ]; then rec check OK 0 0 0 - "$k" "no package touched: nothing to check"; say "check: no package touched"; return 0; fi
  if e=$(find_entry "$k"); then
    if [ "$MODE" != dry ]; then cp "$e"/check-* "$LOGS/" 2>/dev/null; chmod u+w "$LOGS"/check-* 2>/dev/null; fi
    touch "$e/.used" 2>/dev/null
    rec check "REUSED" "$(meta "$e" rc)" "$(since "$t0")" "$(meta "$e" rss_mb)" - "$k" "$(meta "$e" summary)"
    say "check REUSED ${k:0:16}: $(meta "$e" summary)"
    [ "$(meta "$e" rc)" = 0 ]; return
  fi
  if [ "$MODE" != run ]; then MISS=check; say "check needs a run (key ${k:0:16})"; return 3; fi
  local maxr=0 wall=0 transient=
  for m in "${MEMBERS[@]}"; do
    n=check-$(printf '%s' "$m" | tr '/' '_')
    say "check $m with $(basename "$SEED")"
    (cd "$TREE" && JET_COMPILER_IDENTITY=$CID JET_TOOLCHAIN_ROOT=$TREE JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$LOGS/$n-keep-going.tsv" TMPDIR=$WORK/tmp \
      systemd-run --user --scope -q -p MemoryMax="$MEM" -p MemorySwapMax=0 env time -v -o "$LOGS/$n.time" timeout 1200 \
      prlimit --core=0 --stack=1073741824:1073741824 -- "${CPUSET[@]}" bash "$NR" "$SEED/jet" check --threads 12 "$m" > "$LOGS/$n.out" 2> "$LOGS/$n.err")
    rc=$?; rows=$(awk 'NR>1' "$LOGS/$n-keep-going.tsv" 2>/dev/null | wc -l)
    read -r w r <<< "$(timefile "$LOGS/$n.time")"
    [ "$rc" -ge 124 ] && transient=1
    [ "$rc" = 0 ] && [ "$rows" = 0 ] || all=1
    [ "$r" -gt "$maxr" ] && maxr=$r; wall=$(awk -v a="$wall" -v b="$w" 'BEGIN{print a+b}')
    sum+=("$m rc=$rc rows=$rows")
    say "check $m rc=$rc rows=$rows ${w}s"
  done
  local s; s=$(IFS=';'; echo "${sum[*]}")
  if [ -z "$transient" ]; then
    mkdir -p "$WORK/chk"; cp "$LOGS"/check-* "$WORK/chk/"
    printf '%s\n' "stage=check" "kind=check" "rc=$all" "summary=$s" "wall_s=$wall" "rss_mb=$maxr" "lane=$(lane_of)" "tree=$TREE" "${mat[@]}" | store_put "$k" check "$WORK/chk"
  fi
  rec check "$([ $all = 0 ] && echo OK || echo FAILED)" "$all" "$wall" "$maxr" - "$k" "$s"
  [ $all = 0 ]
}

same_build() { cmp -s "$OUT/$1/jet" "$OUT/$2/jet" && { [ ! -e "$OUT/$1/jet.o" ] && [ ! -e "$OUT/$2/jet.o" ] || cmp -s "$OUT/$1/jet.o" "$OUT/$2/jet.o"; }; }

# The chain for LEVEL in MODE (dry: resolve keys from the store only; probe: reuse what the store holds and stop
# at the first stage it lacks; run: build every stage it lacks). Returns 3 at a MISS in dry/probe.
chain() {
  : > "$RES"; MISS=
  if [ "$LEVEL" = check ]; then check_stage; return; fi
  build_stage s1 "$SEED" "$SEED_SHA" "$SEED_PK" "$CID" 12 || return
  pack_stage pack1 "$OUT/s1" "${SHA[s1]}" "$CID" || return
  [ "$LEVEL" = s1 ] && return 0
  build_stage s2 "$OUT/s1" "${SHA[s1]}" "${PK[pack1]}" "$CID" 12 || return
  NID="$VER#${SHA[s2]}"
  pack_stage pack2 "$OUT/s2" "${SHA[s2]}" "$NID" || return
  build_stage s3 "$OUT/s2" "${SHA[s2]}" "${PK[pack2]}" "$NID" 12 || return
  if [ "$LEVEL" = full+threads ]; then build_stage t1 "$OUT/s2" "${SHA[s2]}" "${PK[pack2]}" "$NID" 1 || return; fi
  return 0
}

started=$(now)
DIGEST=-; MEMBERS=(); REASONS=(); NCHANGED="not computed (explicit level)"
if [ -n "$AUTO" ] || [ "$LEVEL" = check ]; then
  node "$LIB" diff "$TREE" "$BASE" "$REPO" > "$OUT/diff.json" || { echo "chain.sh: diff against ${BASE:0:10} failed"; exit 2; }
  node "$LIB" plan "$TREE" "$OUT/diff.json" > "$OUT/plan.json" || { echo "chain.sh: classification failed"; exit 2; }
  DIGEST=$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).digest)' "$OUT/diff.json")
  NCHANGED=$(node -e 'const c=JSON.parse(require("fs").readFileSync(process.argv[1])).changed; console.log(c.length + " file(s)" + (c.length ? ": " + c.slice(0,6).map(x=>x.status+" "+x.path).join(", ") + (c.length>6?", ...":"") : ""))' "$OUT/diff.json")
  mapfile -t MEMBERS < <(node -e 'for (const m of JSON.parse(require("fs").readFileSync(process.argv[1])).members) console.log(m)' "$OUT/plan.json")
  mapfile -t REASONS < <(node -e 'for (const m of JSON.parse(require("fs").readFileSync(process.argv[1])).reasons) console.log(m)' "$OUT/plan.json")
  if [ -n "$AUTO" ]; then
    auto=$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1])).level)' "$OUT/plan.json")
    [ -z "$LEVEL" ] && LEVEL=$auto
    if [ -z "$INNER" ]; then
      echo "--auto: level $auto (base ${BASE:0:10} from $base_src; changes: $NCHANGED)"
      printf '  why: %s\n' "${REASONS[@]}"
      [ ${#MEMBERS[@]} -gt 0 ] && echo "  check members: ${MEMBERS[*]}"
    fi
  fi
fi
# Self-hosting needs the tree's preflight receipt for this seed; check, s1 and --dry-run are ungated. The --inner
# relaunch under build-slot.sh passed the gate already.
if [ -n "$GATE" ] || [ -z "$DRY$INNER" ]; then
  case $LEVEL in
    full|full+threads) tool Land.jet receipt check chain.sh "$TREE" "$SEED" || exit 2;;
    *) [ -n "$GATE" ] && echo "chain.sh: level $LEVEL is ungated (full and full+threads need a preflight receipt)";;
  esac
  [ -n "$GATE" ] && exit 0
fi
SEED_SHA=$(sha "$SEED/jet"); SEED_PK=$(node "$LIB" dirhash "$SEED/jet-runtime")
if [ "$LEVEL" != check ]; then read -r BIN PIN _ <<< "$(node "$LIB" inputs "$TREE")"; else BIN=-; PIN=-; fi

report() { # VERDICT_STATUS VERDICT_RC NOTE SLOTNOTE
  local total; total=$(since "$started")
  local peak; peak=$(awk -F'\t' '$2!="REUSED" && $5+0>m{m=$5+0} END{print m+0}' "$RES")
  { printf 'stage\tstatus\trc\twall_s\tpeak_rss_mb\tsha256\tkey\tnote\n'; awk 1 "$RES"
    printf 'verdict\t%s\t%s\t%s\t%s\t%s\t-\tlevel=%s %s\n' "$1" "$2" "$total" "$peak" "${SHA[s3]:-${SHA[s1]:--}}" "$LEVEL" "$3"; } > "$OUT/RESULT.new"
  mv "$OUT/RESULT.new" "$OUT/RESULT"
  {
    echo "# chain.sh $LEVEL: $TREE"
    echo ""
    echo "- base: ${BASE:0:10} ($base_src); changes: $NCHANGED"
    [ ${#REASONS[@]} -gt 0 ] && { echo "- why this level:$([ -n "$AUTO" ] && echo " (--auto)")"; printf '  - %s\n' "${REASONS[@]}"; }
    [ "$LEVEL" = check ] && echo "- check members: ${MEMBERS[*]:-none}"
    echo "- seed: $(basename "$SEED") ($CID); build inputs ${BIN:0:16}, pack inputs ${PIN:0:16}"
    echo "- store: $STORE (cap ${CAP} GB, LRU); canonical build root $ROOT (private mount namespace)"
    echo "- slot: $4"
    echo ""
    echo "| stage | status | rc | wall s | peak RSS MB | sha256 | key | note |"
    echo "|---|---|---:|---:|---:|---|---|---|"
    awk -F'\t' '{printf "| %s | %s | %s | %s | %s | %s | %s | %s |\n", $1, ($2=="REUSED"?"**REUSED**":$2), $3, $4, $5, substr($6,1,16), substr($7,1,16), $8}' "$RES"
    echo ""
    echo "**$1** $3 (wall ${total} s)"
  } > "$OUT/REPORT.md"
  echo "== $OUT/REPORT.md"; awk 1 "$OUT/REPORT.md"
}

finish() { # SLOTNOTE -> exit code
  local v=OK vrc=0 note=""
  local failed; failed=$(awk -F'\t' '$2=="FAILED"{print $1}' "$RES" | tr '\n' ' ')
  if [ "$LEVEL" = check ]; then
    [ "$(awk -F'\t' '$1=="check"{print $3}' "$RES")" = 0 ] || { v=FAILED; vrc=1; }
  elif [ -n "$failed" ]; then v=FAILED; vrc=1; note="stage(s) failed: $failed"
  elif [ "$LEVEL" = s1 ]; then note="s1 jet ${SHA[s1]:0:16}, pack1 ${PK[pack1]:0:16}"
  else
    if same_build s2 s3; then v=FIXED_POINT; note="s2 == s3 (jet ${SHA[s2]:0:16}$([ -e "$OUT/s2/jet.o" ] && echo ", jet.o $(sha "$OUT/s2/jet.o" | cut -c1-16)"))"
    else v=NOT_FIXED; vrc=1; note="s2 ${SHA[s2]:0:16} != s3 ${SHA[s3]:0:16}"; fi
    rec fixed_point "$([ $v = FIXED_POINT ] && echo SAME || echo DIFFERENT)" "$vrc" 0 0 "${SHA[s3]}" - "cmp s2 s3 (jet and jet.o)"
    if [ "$LEVEL" = full+threads ]; then
      if same_build t1 s3; then rec threads1 SAME 0 0 0 "${SHA[t1]}" - "cmp t1 (--threads 1) s3"; note="$note; t1 (--threads 1) == s3"
      else rec threads1 DIFFERENT 1 0 0 "${SHA[t1]}" - "cmp t1 (--threads 1) s3"; v=THREADS_DIFFER; vrc=1; note="$note; t1 ${SHA[t1]:0:16} != s3"; fi
    fi
    local p1 p2; p1=$(awk -F'\t' '$1=="pack1"{print $6}' "$RES"); p2=$(awk -F'\t' '$1=="pack2"{print $6}' "$RES")
    note="$note; pack1 $([ "$p1" = "$p2" ] && echo "==" || echo "!=") pack2 bytes"
  fi
  report "$v" "$vrc" "$note" "$1"
  return $vrc
}

# Store INSTALL_DIR as s1/pack1 of the census snap only when nothing about that claim is assumed: the snap's build
# output is still these bytes, it is newer than the seed, the census build was clean, the identity is the seed's,
# and the pack is the one the census made with this compiler.
seed_published() {
  local sj=$ROOT/Compiler/JetCli/.jet/build/jet kg=$R14/census/build-keep-going.tsv cs k nk said=()
  cs=$(sha "$SEEDPUB/jet")
  [ -x "$sj" ] && [ "$(sha "$sj")" = "$cs" ] || { echo "seed-published: $ROOT no longer holds this build of $SEEDPUB/jet; nothing stored"; return 1; }
  [ "$SEED/jet" -ot "$sj" ] || { echo "seed-published: seed $(basename "$SEED") is newer than the census build; nothing stored"; return 1; }
  [ -f "$kg" ] && [ "$(awk 'NR>1' "$kg" | wc -l)" = 0 ] && [ "$kg" -nt "$SEED/jet" ] || { echo "seed-published: census build was not clean; nothing stored"; return 1; }
  [ "$(<"$SEEDPUB/IDENTITY")" = "$CID" ] || { echo "seed-published: identity is not the seed's; nothing stored"; return 1; }
  build_mat "$SEED_SHA" "$SEED_PK" "$CID" 12; k=$(key "${MAT[@]}")
  mkdir -p "$OUT/s1"; copy_file "$SEEDPUB/jet" "$OUT/s1/jet"
  if [ -d "$STORE/$k" ]; then said+=("s1 ${k:0:16} already stored")
  else
    printf '%s\n' "stage=s1" "kind=build" "sha256=$cs" "wall_s=-" "rss_mb=-" "lane=lander census" "tree=$ROOT ($(awk 1 "$SEEDPUB/COMMIT" 2>/dev/null))" "${MAT[@]}" | store_put "$k" build "$OUT/s1"
    said+=("s1 ${k:0:16} stored")
  fi
  if [ -f "$SEEDPUB/jet-runtime/provenance.json" ] && [ -d "$R14/census/runtime/out" ] \
     && node -e 'process.exit(JSON.parse(require("fs").readFileSync(process.argv[1])).compilerSha256 === process.argv[2] ? 0 : 1)' "$SEEDPUB/jet-runtime/provenance.json" "$cs" \
     && [ "$(node "$LIB" dirhash "$SEEDPUB/jet-runtime")" = "$(node "$LIB" dirhash "$R14/census/runtime/out")" ]; then
    pack_mat "$cs" "$CID"; k=$(key "${MAT[@]}")
    if [ -d "$STORE/$k" ]; then said+=("pack1 ${k:0:16} already stored")
    else
      cp -a "$SEEDPUB/jet-runtime" "$OUT/s1/jet-runtime"; nk=$(awk 'NR==1' "$OUT/s1/jet-runtime/native.key")
      printf '%s\n' "stage=pack1" "kind=pack" "packhash=$(node "$LIB" dirhash "$OUT/s1/jet-runtime")" "native_key=$nk" "wall_s=-" "rss_mb=-" "lane=lander census" "tree=$ROOT" "${MAT[@]}" | store_put "$k" pack "$OUT/s1"
      said+=("pack1 ${k:0:16} stored")
    fi
  else said+=("pack1 not stored (pack is not the census pack of this compiler)"); fi
  rm -rf "$OUT"
  echo "seed-published: ${said[*]}"
}
if [ -n "$SEEDPUB" ]; then seed_published; exit; fi

if [ -n "$DRY" ]; then
  MODE=dry; chain; r=$?
  echo "dry run: level $LEVEL, store $STORE"
  awk -F'\t' '{printf "  %-6s %s key %s %s\n", $1, $2, substr($7,1,16), $8}' "$RES"
  [ "$r" = 3 ] && echo "  $MISS and every later stage would build (later keys depend on $MISS's output)"
  [ "$r" = 0 ] && echo "  every stage is in the store: the run would take no slot"
  exit 0
fi
if [ -n "$INNER" ] || [ -n "${SLOT_CPUS:-}" ]; then
  MODE=run; chain; finish "${SLOT_CPUS:+CPUs $SLOT_CPUS}${INNER:+ (one build-slot.sh acquisition for every stage the store lacked)}"; exit
fi
# Outer: reuse what the store holds; a complete hit finishes here without a slot.
exec 9>"$TREE/.chain-lock"; flock 9
MODE=probe; chain; r=$?
if [ "$r" = 0 ]; then finish "none (every stage REUSED)"; exit; fi
if [ "$r" != 3 ]; then finish "none (a stage failed while reusing)"; exit; fi
case $LEVEL in check) order=(check);; s1) order=(s1 pack1);; full) order=(s1 pack1 s2 pack2 s3);; *) order=(s1 pack1 s2 pack2 s3 t1);; esac
small=1; seen=
for s in "${order[@]}"; do [ "$s" = "$MISS" ] && seen=1; [ -n "$seen" ] && case $s in s[0-9]|t1) small=0;; esac; done
say "queueing in build-slot.sh (SLOT_SMALL=$small) from stage $MISS"
CHAIN_BASE_SRC=$base_src SLOT_SMALL=$small bash "$SLOT" bash "$SELF" --inner "$TREE" "$LEVEL" ${AUTO:+--auto} --out "$OUT" --base "$BASE"
