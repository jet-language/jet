#!/usr/bin/env bash
# lane-check.sh — the ONE command an implementation worker may run.
#
# Why this exists: workers are source-only so that one agent owns the build
# lane and the disk. That rule was costing more than it saved — nine separate
# integration breaks in one session (unclosed `impl`, stale module paths after a
# split, wrong visibility, borrow errors, a renamed enum variant, a mismatched
# delimiter). Every one of them is what `cargo check` prints in seconds.
#
# So a worker now type-checks its own patch, in the main checkout, against the
# warm shared target dir. Cargo serializes concurrent invocations on its own
# build lock, so several workers may call this at once; they queue, they do not
# corrupt.
#
# What this is NOT: it does not run tests, does not run formatters, does not
# regenerate artifacts, does not build a release binary. Those stay with the
# orchestrator.
#
# Usage:
#   scripts/agent/lane-check.sh                 # whole workspace, all targets
#   scripts/agent/lane-check.sh -p jet-sema     # one crate while iterating
#
# Reading the output: errors are grouped by file. An error in a file YOU did not
# touch belongs to another worker editing the same checkout — report it in your
# final message, do not fix it.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1

if [ "$#" -eq 0 ]; then
  set -- --workspace --all-targets
fi

# /tmp is RAM-backed here, and a check can fork rustc: keep scratch on disk and
# skip incremental artifacts, which reached 24G in one target dir.
scratch="${JET_TEST_SCRATCH:-$HOME/.cache/jet-test-scratch}"
mkdir -p "$scratch"
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
export CARGO_INCREMENTAL=0
export JET_NIX_TMP_CLEANED=1
# One rustc per hardware thread put this machine into swap on a cold build.
# Eight was still too many: with up to 30 lanes queuing on cargo's build lock,
# each holder forking eight rustc drove the box into an OOM kill. Four keeps a
# single holder near ~8G peak while staying fast on a warm target dir.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

# Memory floor. Cargo's lock serializes the *check*, but a lane can still hold
# rustc while the orchestrator builds. Rather than race, wait for headroom and
# then proceed; a lane that waits is cheaper than a box that dies.
floor_mb="${JET_MIN_FREE_MB:-8000}"
for _ in $(seq 1 60); do
  avail="$(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo)"
  [ "${avail:-0}" -ge "$floor_mb" ] && break
  echo "lane-check: waiting for memory (${avail}MB < ${floor_mb}MB floor)" >&2
  sleep 10
done
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

timeout 1800 scripts/agent/jet-env cargo check "$@" >"$tmp" 2>&1
code=$?

node - "$tmp" <<'NODE'
const fs = require("fs");
const text = fs.readFileSync(process.argv[2], "utf8");
const lines = text.split("\n");
const errors = [];
for (let i = 0; i < lines.length; i += 1) {
  if (!/^error(\[|:)/.test(lines[i])) continue;
  const head = lines[i];
  let where = "";
  for (let j = i + 1; j < Math.min(i + 6, lines.length); j += 1) {
    const m = lines[j].match(/^\s*-->\s+(\S+)/);
    if (m) { where = m[1]; break; }
  }
  errors.push({ head, where });
}
if (errors.length === 0) {
  console.log("CHECK OK");
  process.exit(0);
}
const byFile = new Map();
for (const e of errors) {
  const key = e.where || "(no location)";
  if (!byFile.has(key)) byFile.set(key, []);
  byFile.get(key).push(e.head);
}
console.log(`CHECK FAILED — ${errors.length} error(s) in ${byFile.size} file(s)`);
for (const [file, heads] of byFile) {
  console.log(`\n${file}`);
  for (const h of [...new Set(heads)].slice(0, 6)) console.log(`  ${h}`);
}
const first = errors[0].head;
const at = text.indexOf(first);
console.log("\n--- first error in full ---");
console.log(text.slice(at, at + 1600));
NODE

exit "$code"
