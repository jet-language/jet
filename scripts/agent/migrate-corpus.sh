#!/usr/bin/env bash
# migrate-corpus.sh — respell every `.jet` file in the repository with the
# formatter, then show what changed.
#
# This exists for ratified surface migrations. D-LIT-DOT1=B dropped the dot from
# literal constructors and D-ARROW-UNIFY1=B collapsed every arrow to `:>`; that
# is 488 + 516 dotted constructors and 2,816 arrows across roughly a thousand
# files. No human and no agent should retype that. The formatter is the migration
# tool: once it emits only the ratified spelling, running it over the corpus IS
# the migration, and any file it cannot round-trip is a formatter gap worth
# finding rather than a file worth hand-editing.
#
# Usage:
#   scripts/agent/migrate-corpus.sh --check     # what would change, nothing written
#   scripts/agent/migrate-corpus.sh             # rewrite, then report
#
# After a rewrite: read the diff, then rebless snapshots and goldens
# (JET_UI_FILTER / UPDATE_EXPECT per fixture, JET_UPDATE_GOLDEN with
# JET_GOLDEN_FILTER per example) and regenerate grammars with
# `jet self devtools grammars`.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

scratch="${JET_TEST_SCRATCH:-$HOME/.cache/jet-test-scratch}"
mkdir -p "$scratch"
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch" JET_NIX_TMP_CLEANED=1

jet=./target/debug/jet
if [ ! -x "$jet" ]; then
  echo "no $jet — build first (scripts/agent/lane-check.sh only checks)" >&2
  exit 1
fi

roots=(examples tests)
mode="${1:-write}"

if [ "$mode" = "--check" ]; then
  echo "== files the formatter would rewrite"
  fails=0
  for root in "${roots[@]}"; do
    scripts/agent/jet-env "$jet" fmt --check "$root" || fails=1
  done
  exit "$fails"
fi

echo "== rewriting $((${#roots[@]})) root(s) with the formatter"
for root in "${roots[@]}"; do
  scripts/agent/jet-env "$jet" fmt "$root" || {
    echo "formatter refused inside $root — read the error above; that file is the finding" >&2
    exit 1
  }
done

echo
echo "== changed files"
git diff --stat -- '*.jet' | tail -1
echo
echo "== remaining old spellings (should be zero)"
node - <<'NODE'
const { execFileSync } = require("node:child_process");
const probes = [
  ["dotted constructor", String.raw`\w\.\{`],
  ["callable arrow", String.raw`=>`],
  ["control arrow", String.raw`->`],
  ["effect row", String.raw`=\[`],
];
for (const [label, pattern] of probes) {
  let out = "";
  try {
    out = execFileSync("git", ["grep", "-cE", pattern, "--", "examples/**/*.jet", "tests/**/*.jet"], {
      encoding: "utf8",
      maxBuffer: 1e9,
    });
  } catch {
    console.log(`  ${label}: 0 files`);
    continue;
  }
  const files = out.split("\n").filter(Boolean);
  const total = files.reduce((a, l) => a + Number(l.split(":").pop() || 0), 0);
  console.log(`  ${label}: ${total} occurrence(s) in ${files.length} file(s)`);
}
NODE
echo
echo "next: read the diff, rebless snapshots and goldens, regenerate grammars"
