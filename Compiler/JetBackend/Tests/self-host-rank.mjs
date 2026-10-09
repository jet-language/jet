#!/usr/bin/env node
// Reuses NativeSelfHost2/rank-self.mjs's normalization and conditional greedy
// coverage curve. Accepts one probe directory or run-slices.sh's parent directory.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const input = process.argv[2];
if (!input) throw new Error("usage: self-host-rank.mjs <probe directory> [output directory]");
const root = resolve(input);
const output = resolve(process.argv[3] ?? root);
const lines = (path) => existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : [];
const normalize = (message) => {
  const missing = /\btype (?:\S+ )?(\S+?)(?:<>)? without a definition/.exec(message);
  if (missing) return `Core type ${missing[1].replace(/<.*$/, "")} (no definition / runtime handle)`;
  return message.replace(/-?[0-9]{2,}/g, "N").replace(/\bv[0-9]+/g, "vN").replace(/\bb[0-9]+/g, "bN");
};
const sliceRoot = join(root, "slices");
const probes = existsSync(sliceRoot)
  ? readdirSync(sliceRoot, { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => join(sliceRoot, entry.name)).sort()
  : [root];
if (probes.length === 0) throw new Error(`no package slices in ${root}`);

const universe = new Map();
const metadata = new Map();
const preparation = new Map();
const sliceRows = [];
let conversionErrors = 0;
let measuredSlices = 0;
const add = (fn, blocker) => {
  if (!universe.has(fn)) universe.set(fn, new Set());
  if (blocker) universe.get(fn).add(blocker);
};
for (const probe of probes) {
  const progress = lines(join(probe, "run/self.progress")).map((row) => row.split("\t").slice(2).join("\t"));
  const issuePath = join(probe, "run/self.issues");
  const complete = existsSync(issuePath);
  const issues = lines(complete ? issuePath : join(probe, "run/self.partial-issues"));
  const skips = lines(join(probe, "run/skip.txt"));
  const convertedErrors = lines(join(probe, "self.convert-errors"));
  const mapPath = existsSync(join(probe, "compiler.map.json")) ? join(probe, "compiler.map.json") : join(root, "compiler.map.json");
  const segments = existsSync(mapPath)
    ? JSON.parse(readFileSync(mapPath, "utf8")).source_segments.slice().sort((a, b) => a.generated.start - b.generated.start)
    : [];
  const sources = new Map(lines(join(probe, "self.sources.tsv")).map((row) => row.split("\t")));
  const byId = new Map();
  for (const row of lines(join(probe, "self.functions.tsv"))) {
    const [id, key, name, file, start] = row.split("\t");
    const path = sources.get(file) ?? "";
    let source = path;
    let pkg = path ? (path.startsWith("Core/") || path.startsWith("core") ? "Core/Prelude" : path) : "UNMAPPED";
    if (path.endsWith("selfhost.jet")) {
      let lo = 0, hi = segments.length - 1;
      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        const segment = segments[mid];
        if (Number(start) < segment.generated.start) hi = mid - 1;
        else if (Number(start) >= segment.generated.end) lo = mid + 1;
        else { source = segment.source_file; pkg = source.split("/").slice(0, 2).join("/"); break; }
      }
      if (source === path) pkg = "SelfHostEntry";
    }
    const entry = { pkg, source, name, key };
    byId.set(id, entry);
    if (key) metadata.set(key, entry);
    if (name && !metadata.has(name)) metadata.set(name, entry);
    metadata.set(`id:${id}`, entry);
  }
  const local = new Map();
  const touch = (fn, blocker) => {
    if (!local.has(fn)) local.set(fn, new Set());
    if (blocker) local.get(fn).add(blocker);
    add(fn, blocker);
  };
  for (const fn of progress) touch(fn);
  for (const fn of skips) touch(fn, "probe: lowering crashed (skipped)");
  for (const row of issues) {
    const tab = row.indexOf("\t");
    if (tab < 0) throw new Error(`malformed issue in ${probe}: ${row}`);
    const fn = row.slice(0, tab);
    const blocker = normalize(row.slice(tab + 1));
    if (fn !== "entry preparation") touch(fn, blocker);
  }
  // New drivers append completion after flushing issues. For the serial old
  // driver, the next progress row proves the prior function finished. Its last
  // started function is unknown until the final issues file exists.
  const donePath = join(probe, "run/self.completed");
  const done = new Set(lines(donePath));
  if (!complete) {
    const incomplete = existsSync(donePath) ? progress.filter((fn) => !done.has(fn)) : progress.slice(-1);
    for (const fn of incomplete) touch(fn, "probe: lowering did not complete (not a clean function)");
  }
  for (const row of lines(join(probe, "run/self.prepare-issues"))) {
    const tab = row.indexOf("\t");
    const blocker = normalize(row.slice(tab + 1)).replace(/^prepare: /, "");
    if (!preparation.has(blocker)) preparation.set(blocker, new Set());
    preparation.get(blocker).add(probe);
  }
  conversionErrors += convertedErrors.length;
  for (const row of convertedErrors) {
    const [table, id, ...message] = row.split("\t");
    if (table !== "functions") continue;
    const entry = byId.get(id);
    touch(entry?.key || entry?.name || `id:${id}`, normalize(`convert: ${message.join("\t")}`));
  }
  if (progress.length || skips.length || issues.length) measuredSlices++;
  sliceRows.push([probe, progress.length, [...local.values()].filter((set) => set.size === 0).length, local.size, complete ? "complete" : "partial-or-not-run"]);
}
if (measuredSlices === 0) throw new Error(`no measured lowering functions in ${root}; refusing to label absent evidence as 0% coverage`);
const lookup = (fn) => {
  if (fn.startsWith("glue:")) return { pkg: "glue", source: "" };
  return metadata.get(fn) ?? metadata.get(fn.slice(0, fn.indexOf("<"))) ?? { pkg: "UNMAPPED", source: "" };
};

const goldenPath = process.env.SELFHOST_GOLDEN_LOWER;
const goldenFiles = goldenPath && existsSync(goldenPath) ? readdirSync(goldenPath).filter((file) => file.endsWith(".issues")) : [];
const goldenCounts = new Map();
for (const file of goldenFiles) {
  const blockers = new Set(lines(join(goldenPath, file)).map((row) => normalize(row.slice(row.indexOf("\t") + 1))));
  for (const blocker of blockers) goldenCounts.set(blocker, (goldenCounts.get(blocker) ?? 0) + 1);
}
const classification = (blocker) => goldenFiles.length === 0 ? "overlap-unmeasured" : (goldenCounts.get(blocker) ?? 0) > 0 ? "shared" : "compiler-only";
const gates = new Map();
const packages = new Map();
for (const [fn, blockers] of universe) {
  const pkg = lookup(fn).pkg;
  if (!packages.has(pkg)) packages.set(pkg, [0, 0]);
  const counts = packages.get(pkg);
  counts[0]++;
  if (blockers.size === 0) counts[1]++;
  for (const blocker of blockers) {
    if (!gates.has(blocker)) gates.set(blocker, new Set());
    gates.get(blocker).add(fn);
  }
}
const total = universe.size;
const initiallyClean = [...universe.values()].filter((set) => set.size === 0).length;
let clean = initiallyClean;
const remaining = new Map([...universe].map(([fn, set]) => [fn, new Set(set)]));
const fixed = new Set();
const curve = [];
while (fixed.size < gates.size) {
  let best, bestUnlock = -1, bestGates = -1;
  for (const [blocker, fns] of gates) {
    if (fixed.has(blocker)) continue;
    let unlock = 0;
    for (const fn of fns) if (remaining.get(fn).size === 1) unlock++;
    if (unlock > bestUnlock || (unlock === bestUnlock && fns.size > bestGates)) {
      best = blocker; bestUnlock = unlock; bestGates = fns.size;
    }
  }
  fixed.add(best);
  for (const fn of gates.get(best)) remaining.get(fn).delete(best);
  clean += bestUnlock;
  const affected = new Map();
  for (const fn of gates.get(best)) {
    const pkg = lookup(fn).pkg;
    affected.set(pkg, (affected.get(pkg) ?? 0) + 1);
  }
  curve.push([curve.length + 1, best, bestGates, bestUnlock, clean, (100 * clean / total).toFixed(1), classification(best), goldenCounts.get(best) ?? 0,
    [...affected].sort((a, b) => b[1] - a[1]).map(([pkg, n]) => `${pkg}:${n}`).join(";"), [...gates.get(best)].slice(0, 3).join(" | ")]);
}
const packageRows = [...packages].sort(([a], [b]) => a.localeCompare(b)).map(([pkg, [reached, lowered]]) => [pkg, reached, lowered, (100 * lowered / reached).toFixed(1)]);
const preamble = [
  `# input=${root}; measured_slices=${measuredSlices}/${probes.length}; functions=${total}; lowered_clean=${initiallyClean}`,
  "# Functions are deduplicated by descriptor name across slices of the same MIR dump; instances and glue count separately.",
  "# Local lowering coverage only: preparation/conversion/object/link failures remain independent build blockers.",
  `# conversion_error_rows=${conversionErrors}; preparation_blockers=${preparation.size}; golden_issue_files=${goldenFiles.length}`,
  "# Cumulative coverage is conditional, not proof of linking: fixing a blocker may expose another.",
  ...packageRows.map((row) => `# package\t${row.join("\t")}`),
];
const tsv = (path, rows) => writeFileSync(join(output, path), rows.map((row) => Array.isArray(row) ? row.join("\t") : row).join("\n") + "\n");
tsv("RANKED.tsv", [...preamble, "step\tblocker\tgates_functions\tunlocks\tcumulative_clean\tcumulative_pct\tclass\tgolden_cases\tpackages\tsample_functions", ...curve]);
tsv("PACKAGES.tsv", ["package\tfunctions_reached\tlowered_clean\tpct_clean", ...packageRows]);
tsv("SLICES.tsv", ["probe\tfunctions_started\tlocally_clean\tfunctions_with_evidence\tstatus", ...sliceRows]);
// Per package: every blocker gating its functions, and how many functions it alone holds back (the order a
// package-first milestone such as JetLexer then JetParser native fixes them in).
const packageBlockers = [];
for (const [blocker, fns] of gates) {
  const counts = new Map();
  for (const fn of fns) {
    const pkg = lookup(fn).pkg;
    if (!counts.has(pkg)) counts.set(pkg, [0, 0]);
    counts.get(pkg)[0]++;
    if (universe.get(fn).size === 1) counts.get(pkg)[1]++;
  }
  for (const [pkg, [gated, only]] of counts) packageBlockers.push([pkg, blocker, gated, only, classification(blocker), goldenCounts.get(blocker) ?? 0]);
}
packageBlockers.sort((a, b) => a[0].localeCompare(b[0]) || b[2] - a[2] || a[1].localeCompare(b[1]));
tsv("PACKAGE-BLOCKERS.tsv", ["package\tblocker\tgates_functions\tsole_blocker_functions\tclass\tgolden_cases", ...packageBlockers]);
const overlap = [...gates].map(([blocker, fns]) => [blocker, fns.size, goldenCounts.get(blocker) ?? 0, classification(blocker), "lower"]);
for (const [blocker, probes] of preparation) overlap.push([blocker, "global", goldenCounts.get(blocker) ?? 0, classification(blocker), `prepare:${probes.size} slices`]);
overlap.sort((a, b) => (Number(b[1]) || 0) - (Number(a[1]) || 0) || a[0].localeCompare(b[0]));
tsv("OVERLAP.tsv", ["blocker\tselfhost_functions\tgolden_cases\tclass\tphase", ...overlap]);
console.log(`SELFHOST RANK functions=${total} clean=${initiallyClean} packages=${packages.size} slices=${measuredSlices}/${probes.length} blockers=${gates.size} golden_issue_files=${goldenFiles.length}`);
