#!/usr/bin/env bash
# target-prune.sh — drop stale build-artifact generations from target/debug/deps.
#
# Cargo never prunes `deps`. Every rebuild of a test target writes a new
# `<name>-<hash>.{rlib,rmeta,d,}` set and leaves the old one forever. On this
# repo each test binary is ~250MB and there are ~150 of them, so one workspace
# test build writes ~190G and a few builds reach half a terabyte — which is how
# a session ended in an OOM kill with 517G in one target dir.
#
# This keeps the newest generation of every build unit (all files sharing that
# unit's hash) and deletes older generations. Deleting a live artifact only
# costs a rebuild of that one unit, so this is safe to run whenever.
#
# Usage:
#   scripts/agent/target-prune.sh            # prune, print what was reclaimed
#   scripts/agent/target-prune.sh --dry      # report only
#   scripts/agent/target-prune.sh --keep 2   # keep two generations per unit
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

exec node - "$@" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const argv = process.argv.slice(2);
const dry = argv.includes("--dry");
const keepIdx = argv.indexOf("--keep");
const keep = keepIdx === -1 ? 1 : Math.max(1, Number(argv[keepIdx + 1]) || 1);

const dir = "target/debug/deps";
if (!fs.existsSync(dir)) {
  console.log("no target/debug/deps — nothing to prune");
  process.exit(0);
}

// A build unit is <prefix>-<hash>; its files share the hash and differ by
// extension. Group by prefix, then by hash, so a kept generation keeps all of
// its parts.
const units = new Map();
let total = 0;
for (const name of fs.readdirSync(dir)) {
  const full = path.join(dir, name);
  let st;
  try { st = fs.lstatSync(full); } catch { continue; }
  if (!st.isFile()) continue;
  total += st.size;
  const m = name.match(/^(.*?)-([0-9a-f]{8,})(\..*)?$/);
  if (!m) continue;
  const [, prefix, hash] = m;
  if (!units.has(prefix)) units.set(prefix, new Map());
  const gens = units.get(prefix);
  if (!gens.has(hash)) gens.set(hash, { mtime: 0, size: 0, files: [] });
  const gen = gens.get(hash);
  gen.mtime = Math.max(gen.mtime, st.mtimeMs);
  gen.size += st.size;
  gen.files.push(full);
}

let freed = 0;
let dropped = 0;
for (const gens of units.values()) {
  if (gens.size <= keep) continue;
  const ordered = [...gens.values()].sort((a, b) => b.mtime - a.mtime);
  for (const gen of ordered.slice(keep)) {
    for (const f of gen.files) {
      if (!dry) { try { fs.rmSync(f); } catch { continue; } }
      dropped += 1;
      freed += 0;
    }
    freed += gen.size;
  }
}

const gb = (n) => (n / 1073741824).toFixed(1) + "G";
console.log(
  `${dry ? "would drop" : "dropped"} ${dropped} file(s), ${gb(freed)} of ${gb(total)} ` +
    `(keeping ${keep} generation${keep > 1 ? "s" : ""} per build unit)`,
);
NODE
