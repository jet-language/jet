#!/usr/bin/env bash
# proofq: mem=8G time=120M kind=test
# One standing #4619 job. safety-schedule.mjs supplies immutable pins/inputs.
set -euo pipefail
: "${WT:?}" "${SHA:?}" "${ARTIFACTS:?}" "${ARTIFACTS_SHA256:?}" "${OUT:?}" "${SEED:?}" "${COUNT:?}"
[ ! -e "$HOME/.cache/jet-dev/proofq/PAUSE" ] || exit 75
[ "$(git -C "$WT" rev-parse HEAD)" = "$SHA" ] || exit 2
export JET_SCRATCH_ROOT=/mnt/jetscratch JET_NIX_TMP_CLEANED=1
unset CARGO_TARGET_DIR # Artifact consumer: no Cargo build or cold target.
export TMPDIR=/mnt/jetscratch/candidates/SafetyNet8/tmp
mkdir -p "$TMPDIR"
cd "$WT"
node -e 'const fs=require("node:fs"),c=require("node:crypto"); if(c.createHash("sha256").update(fs.readFileSync(process.argv[1])).digest("hex")!==process.argv[2]) process.exit(2)' "$ARTIFACTS" "$ARTIFACTS_SHA256"
exec Tools/agent/jet-env node Tools/agent/safety-gate.mjs "$OUT" "$ARTIFACTS" "$SEED" "$COUNT"
