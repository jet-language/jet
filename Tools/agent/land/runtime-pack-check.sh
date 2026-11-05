#!/usr/bin/env bash
# runtime-pack-check.sh <compiler> <tree> <work dir> <identity>
# Lowers <tree>'s Jet runtime pack with <compiler>, exactly as chain s1's pack step does, including its
# handling of the `jet doc` exit linger: once the four pack outputs are stable for 15 s, the lingering
# process group is killed and the written outputs are packaged by a reuse copy of prebuild-runtime.mjs.
# The reuse copy carries the runtime IR module (jet_runtime_jet.ir -> runtime.ir) with the object.
# Prints "runtime rc=<rc> issues=<n>" plus issue rows; exit 0 only when the pack is produced.
set +e
JET=${1:?compiler} T=${2:?tree} WD=${3:?work dir} ID=${4:?identity}
rm -rf "$WD" && mkdir -p "$WD/tmp" "$WD/out" "$WD/reuse"
node -e '
const fs=require("fs");const src=process.argv[1]+"/Tools/packaging/prebuild-runtime.mjs";let t=fs.readFileSync(src,"utf8");
const re=/ +const run = spawnSync\(jet, \[\x27doc\x27[\s\S]*?Jet runtime production failed \(\$\{run\.status\}\)`\);/;
if(!re.test(t))process.exit(3);
t=t.replace(re,"    const reuse = process.env.JET_PACK_REUSE;\n    if (!reuse) throw Error(\x27JET_PACK_REUSE not set\x27);\n    for (const s of [\x27.o\x27, \x27.ir\x27, \x27.abi.json\x27, \x27.exports\x27]) copyFileSync(join(reuse, \x27jet_runtime_jet\x27 + s), prefix + s);\n    copyFileSync(join(reuse, \x27jet_runtime.pack.o\x27), pack);");
fs.writeFileSync(process.argv[2],t)' "$T" "$T/Tools/packaging/prebuild-runtime-reuse.mjs" || { echo "runtime rc=3 issues=0 (reuse packager not generated)"; exit 1; }
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
[ -n "${JET_PACK_MUSL_LIB:-}" ] || { echo "runtime rc=2 issues=0 (set JET_PACK_MUSL_LIB to the static musl lib dir)"; exit 1; }
export TMPDIR="$WD/tmp" JET_PACK_MUSL_LIB
export JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$T"
cd "$T" || exit 2
# The pack job is its own session: it never outlives this watcher (trap for signals; a guard in the job's
# group for SIGKILL), and timeout bounds a lingering `jet doc`.
watcher=$$
setsid bash -c "(while kill -0 $watcher 2>/dev/null; do sleep 10; done; kill -KILL 0) & exec timeout 1200 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 Tools/agent/jet-env node Tools/packaging/prebuild-runtime.mjs '$JET' '$WD/out' --repo '$T'" > "$WD/pack.log" 2>&1 &
job=$!
trap 'kill -KILL -- "-$job" 2>/dev/null' EXIT
trap 'exit 143' INT TERM HUP
last= ; stable=0 ; reused=0
while kill -0 "$job" 2>/dev/null; do
  sleep 5
  d=$(ls -d "$WD"/tmp/*/jet-runtime-pack-* "$WD"/tmp/jet-runtime-pack-* 2>/dev/null | head -1)
  [ -n "$d" ] && [ -f "$d/jet_runtime.pack.o" ] && [ -f "$d/jet_runtime_jet.exports" ] && [ -f "$d/jet_runtime_jet.ir" ] || continue
  sig=$(stat -c '%s %Y' "$d"/jet_runtime.pack.o "$d"/jet_runtime_jet.o "$d"/jet_runtime_jet.ir "$d"/jet_runtime_jet.abi.json "$d"/jet_runtime_jet.exports 2>/dev/null | tr '\n' ' ')
  if [ "$sig" = "$last" ]; then stable=$((stable+5)); else stable=0; last=$sig; fi
  if [ "$stable" -ge 15 ] && [ ! -f "$WD/out/native.key" ]; then
    cp -p "$d"/jet_runtime.pack.o "$d"/jet_runtime_jet.o "$d"/jet_runtime_jet.ir "$d"/jet_runtime_jet.abi.json "$d"/jet_runtime_jet.exports "$WD/reuse/"
    kill -TERM -- "-$job" 2>/dev/null; sleep 3; kill -KILL -- "-$job" 2>/dev/null; wait "$job" 2>/dev/null
    rm -rf "$WD/out"; mkdir -p "$WD/out"
    JET_PACK_REUSE="$WD/reuse" Tools/agent/jet-env node Tools/packaging/prebuild-runtime-reuse.mjs "$JET" "$WD/out" --repo "$T" >> "$WD/pack.log" 2>&1
    reused=1
    break
  fi
done
wait "$job" 2>/dev/null
n=$(cat "$WD"/out/runtime*.issues 2>/dev/null | awk 'END{print NR}')
if [ -f "$WD/out/native.key" ]; then echo "runtime rc=0 issues=${n:-0} (reuse=$reused)"; exit 0; fi
echo "runtime rc=1 issues=${n:-0}"
cat "$WD"/out/runtime*.issues 2>/dev/null | cut -c1-200 | head -10
tail -c 600 "$WD/pack.log"
exit 1
