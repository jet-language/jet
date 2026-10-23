#!/usr/bin/env bash
# Draft five immutable-source S5-r4 completion-only holds; Main admits the pin.
# FULL_SHA also seals preserved captures from that immutable source export.
set -euo pipefail
SHA=${1:-}
if [[ -n "$SHA" && ! "$SHA" =~ ^[a-f0-9]{40}$ ]]; then echo 'full source SHA required' >&2; exit 2; fi
C=/mnt/jetscratch/candidates/RuntimeLead12/s5-r4
Q=/home/nate/.cache/jet-dev/proofq/queue
P=Main-RuntimeLead12-s5
HOST=${S5_SHARED_HOST_JOB:-Main-DevGreen-host-r2}
PACK=${S5_SHARED_PACK_JOB:-Main-RuntimePackLead-native-pack-r1}
mkdir -p "$C"
if [[ -n "$SHA" ]]; then
  SOURCE=/mnt/jetscratch/candidates/source-${SHA}/source
  node "$SOURCE/Tools/agent/s5/stage-frozen-inputs.mjs" "$SHA" "$SOURCE" "$C/inputs"
fi
job() {
  local phase=$1 memory=$2 minutes=$3 dependencies=$4 extra=$5
  local kind=test
  if [[ "$phase" == build ]]; then kind=build; fi
  local file="$Q/$P-$phase-r4.sh.mainhold"
  {
    printf '#!/usr/bin/env bash\n'
    printf '# proofq: mem=%sG time=%sM kind=%s after=%s,%s%s\n' "$memory" "$minutes" "$kind" "$HOST" "$PACK" "${dependencies:+,$dependencies}"
    printf '# queue rc = completed, NOT pass; inspect driver.rc and steps.json/verdict.txt\n'
    printf '# PQ-OUTPUT: %s\n' "$C"
    printf "export JET_SHARED_SOURCE_SHA='%s'\n" "$SHA"
    printf 'export S5_PHASE=%s S5_OUT=%s/%s\n' "$phase" "$C" "$phase"
    printf 'export S5_BUILD=%s/build S5_GATES=%s/gates S5_RUNTIME=%s/runtime S5_BOUNDARY=%s/boundary S5_INPUTS=%s/inputs\n' "$C" "$C" "$C" "$C" "$C"
    printf 'export S5_SHARED_HOST_JOB=%s S5_SHARED_PACK_JOB=%s\n' "$HOST" "$PACK"
    printf 'export PQ_JOB=%s-%s-r4\n' "$P" "$phase"
    cat <<'PREFLIGHT'
export S5_RUN_ID="${PQ_JOB}-$(date +%s%N)-$$"
mkdir -p "$S5_OUT"
blocked() {
  node --input-type=commonjs - "$S5_OUT" "$JET_SHARED_SOURCE_SHA" "$S5_PHASE" "$S5_RUN_ID" "$1" <<'RECEIPT'
const fs=require('fs'),path=require('path');
const [directory,head,phase,runId,because]=process.argv.slice(2);
const file=path.join(directory,'steps.json');
if(fs.existsSync(file))fs.renameSync(file,path.join(directory,'steps.incomplete-'+runId+'.json'));
const receipt={schema:'jet-s5-phase/v1',head,phase,runId,complete:true,verdict:'BLOCKED',detail:because,steps:[{name:'preflight',verdict:'BLOCKED',because}],runners:[]};
const write=(name,text)=>{const out=path.join(directory,name),temporary=out+'.tmp-'+process.pid;fs.writeFileSync(temporary,text);fs.renameSync(temporary,out);};
write('steps.json',JSON.stringify(receipt,null,2)+'\n');write('verdict.txt','BLOCKED '+because+'\n');write('driver.rc','2\n');
RECEIPT
  exit 0
}
if [[ ! "$JET_SHARED_SOURCE_SHA" =~ ^[a-f0-9]{40}$ ]]; then blocked 'Main must admit a full source pin'; fi
export W="/mnt/jetscratch/candidates/source-${JET_SHARED_SOURCE_SHA}/source"
export S5_EXPECT_HEAD="$JET_SHARED_SOURCE_SHA" S5_SHARED_ARTIFACTS="${W%/source}/ARTIFACTS.json"
export S5_SHARED_PACK_DIRECTORY="${W%/source}/runtime-pack"
export COMPOSED_SHA="$JET_SHARED_SOURCE_SHA" JET_TOOLCHAIN_ROOT="$W"
if [ ! -d "$W" ]; then blocked 'immutable source export absent'; fi
cd "$W" || blocked 'cannot enter immutable source export'
CARGO_TARGET_DIR="$(node --input-type=module -e 'const {assertPinnedSource,proofTargetDirectory}=await import("./Tools/agent/proof-source.mjs");assertPinnedSource(process.cwd(),process.argv[1]);console.log(proofTargetDirectory(process.argv[1]));' "$JET_SHARED_SOURCE_SHA")" || blocked 'immutable source guard or target derivation failed'
export CARGO_TARGET_DIR
if [ -z "$CARGO_TARGET_DIR" ]; then blocked 'per-SHA target directory absent'; fi
mkdir -p "$CARGO_TARGET_DIR" || blocked 'cannot create per-SHA target directory'
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
PREFLIGHT
    printf 'export JET_SCRATCH_ROOT=/mnt/jetscratch JET_NO_SCCACHE=1 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=16\n'
    printf 'export TMPDIR=%s/tmp-%s JET_LOWER_TMPDIR=%s/tmp-%s JET_FRONT_TIMEOUT_MS=900000\n' "$C" "$phase" "$C" "$phase"
    printf 'export JET_PACK_MUSL_LIB=/nix/store/wrng0xbxmz3m7slj6k14vip516wvkjdi-musl-static-x86_64-unknown-linux-musl-1.2.5/lib\n'
    printf 'export %s\n' "$extra"
    printf 'mkdir -p "$TMPDIR" || blocked "cannot create phase temporary directory"\n'
    printf '"$W/Tools/agent/jet-env" env -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER timeout --kill-after=30s %sm /run/current-system/sw/bin/systemd-run --user --scope --quiet -p MemoryMax=%sG -p MemorySwapMax=0 node "$W/Tools/agent/s5/s5-%s.mjs"\n' "$((minutes - 1))" "$memory" "$phase"
    cat <<'FINALIZE'
rc=$?
node "$W/Tools/agent/s5/finish-driver.mjs" "$S5_OUT" "$JET_SHARED_SOURCE_SHA" "$S5_PHASE" "$rc"
exit 0
FINALIZE
  } > "$file"
  chmod +x "$file"
  bash -n "$file"
  printf '%s\n' "$file"
}
job build 4 10 '' 'S5_CONSUMER_ONLY=1'
job gates 16 50 '' 'S5_GENERATOR_CHECK_ONLY=1'
job runtime 16 50 "$P-build-r4" 'S5_SHARED_PACK_ONLY=1'
job boundary 20 55 "$P-build-r4,$P-gates-r4" 'S5_FIXTURE_TIMEOUT_S=3000'
job fixtures 20 55 "$P-runtime-r4" 'S5_FIXTURE_WIDTH=4'
