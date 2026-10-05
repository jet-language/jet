#!/usr/bin/env node
// Ranked blocker table for the self-host probe (run-probe.sh output).
// Unit = one lowered function of the compiler (original, instance or glue).
// A function is gated by every distinct normalized issue it reports; the
// greedy curve picks, each step, the blocker whose fix fully unblocks the
// most functions, and accumulates clean functions. Preparation issues,
// conversion errors and crash skips are listed as global/per-function
// blockers too. Golden overlap: how many goldens show the same normalized
// blocker in the newest golden lowering issues (g13 partial, else g12).
// usage: node rank-self.mjs <probe outdir> [RANKED.tsv path]
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const O = resolve(process.argv[2] ?? "");
const outPath = resolve(process.argv[3] ?? join(process.env.HOME, ".cache/jet-dev/scratch/NativeSelfHostProbe/RANKED.tsv"));
const lines = (path) => (existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : []);

// BackendO11/rank.mjs normalization, so keys compare with the golden curve.
const normalize = (message) => {
  const missing = /\btype (?:\S+ )?(\S+?)(?:<>)? without a definition/.exec(message);
  // Type arguments dropped so `Iter<Int>` and the goldens' `Iter` meet.
  if (missing) return `Core type ${missing[1].replace(/<.*$/, "")} (no definition / runtime handle)`;
  return message.replace(/-?[0-9]{2,}/g, "N").replace(/\bv[0-9]+/g, "vN").replace(/\bb[0-9]+/g, "bN");
};
const L = "Compiler/JetBackend/Source/Lower/";
const abi = "crates/jet-codegen/src/Prelude/Core/CAbi.rs", gen = "Tools/agent/gen-c-abi-routes.mjs";
function paths(b) {
  if (/^probe: .*crash/.test(b)) return ["UNDIAGNOSED:crash"];
  if (/^convert:/.test(b)) return ["Compiler/JetBackend/Tests/mir-debug.mjs"];
  if (/^prepare:/.test(b)) return [L + "Mono.jet", L + "Reach.jet"];
  if (/IOError|IOContext|apply_conversion|Err<>|err_with_context/.test(b)) return [L + "Routes.jet", gen, abi];
  if (/SendFn|Task|Sender|Receiver|Group|Closed|Shared/.test(b)) return [L + "Closures.jet", L + "Lower.jet", gen, abi];
  if (/Iter|iter_|list_lazy/.test(b)) return [L + "Iter.jet", L + "Lists.jet", abi];
  if (/Core type|runtime handle/.test(b)) return [L + "Lower.jet", L + "RuntimeRoutes.jet", gen, abi];
  if (/BigInt|constants|constant /.test(b)) return [L + "Statics.jet", L + "Constants.jet", abi];
  if (/codec|DataTree|Dyn|DataEntriesToMap|encoding/.test(b)) return [L + "Reach.jet", L + "Mono.jet", abi];
  if (/Slice|List|list_|View/.test(b)) return [L + "Lists.jet", L + "Iter.jet", L + "Combinators.jet", abi];
  if (/Map|map_/.test(b)) return [L + "Maps.jet", L + "Equal.jet"];
  if (/closure|Closure/.test(b)) return [L + "Closures.jet", L + "Lower.jet", abi];
  if (/display|show|fmt_debug|render|Debug/.test(b)) return [L + "Display.jet", gen, abi];
  if (/comparison|equality|jet_eq/.test(b)) return [L + "Equal.jet", L + "Lower.jet"];
  if (/numeric|conversion|Float|Int|trap|wrapping|saturating|arithmetic|binary|overflow/.test(b)) return [L + "Numeric.jet", L + "Convert.jet", abi];
  if (/route|runtime pack lacks|CoreCall|HandleMethod|PreciseBuiltin|StaticPreludeCall/.test(b)) return [gen, L + "RuntimeRoutes.jet", L + "Routes.jet", abi];
  if (/LIR lint/.test(b)) return [L + "Lower.jet", L + "Values.jet", "Compiler/JetBackend/Source/LIR/"];
  return [L + "Lower.jet"];
}

// Function key -> package, through the span and compiler.map.json.
const map = existsSync(join(O, "compiler.map.json")) ? JSON.parse(readFileSync(join(O, "compiler.map.json"), "utf8")) : { source_segments: [] };
const segments = map.source_segments.map((s) => ({ start: s.generated.start, end: s.generated.end, path: s.source_file })).sort((a, b) => a.start - b.start);
const segmentAt = (offset) => {
  let lo = 0, hi = segments.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (offset < segments[mid].start) hi = mid - 1;
    else if (offset >= segments[mid].end) lo = mid + 1;
    else return segments[mid];
  }
  return null;
};
const sources = new Map(lines(join(O, "self.sources.tsv")).map((row) => row.split("\t")));
const byKey = new Map();
for (const row of lines(join(O, "self.functions.tsv"))) {
  const [id, key, name, file, start] = row.split("\t");
  const path = sources.get(file) ?? "";
  let pkg = "Core/Prelude";
  let source = path;
  if (path.endsWith("selfhost.jet")) {
    const segment = segmentAt(Number(start));
    source = segment?.path ?? "selfhost.jet";
    pkg = segment ? segment.path.split("/").slice(0, 2).join("/") : "SelfHostEntry";
  } else if (path) pkg = path.startsWith("Core/") || path.startsWith("core") ? `Core:${path}` : path;
  const entry = { id, key, name, pkg, source };
  byKey.set(key || name, entry);
  byKey.set(`id:${id}`, entry);
}
const lookup = (fn) => {
  if (fn.startsWith("glue:")) return { pkg: "glue", source: "" };
  if (byKey.has(fn)) return byKey.get(fn);
  const cut = fn.indexOf("<");
  if (cut > 0 && byKey.has(fn.slice(0, cut))) return byKey.get(fn.slice(0, cut));
  return { pkg: "UNMAPPED", source: "" };
};

// Universe: every function the driver started (progress), every skipped one,
// every function with an issue.
const universe = new Map();
const touch = (fn) => {
  if (!universe.has(fn)) universe.set(fn, new Set());
  return universe.get(fn);
};
for (const row of lines(join(O, "run/self.progress"))) touch(row.split("\t").slice(2).join("\t"));
for (const fn of lines(join(O, "run/skip.txt"))) touch(fn).add("probe: lowering crashed (skipped)");
// A run stopped early (timeout/OOM) has only the incrementally appended issues.
const issueFile = existsSync(join(O, "run/self.issues")) ? join(O, "run/self.issues") : join(O, "run/self.partial-issues");
for (const row of lines(issueFile)) {
  const tab = row.indexOf("\t");
  const fn = row.slice(0, tab);
  if (fn === "entry preparation") continue;
  touch(fn).add(normalize(row.slice(tab + 1)));
}
const prepare = lines(join(O, "run/self.prepare-issues")).map((row) => normalize(row.slice(row.indexOf("\t") + 1)));
const convertErrors = lines(join(O, "self.convert-errors"));
for (const row of convertErrors) {
  const [table, id, message] = row.split("\t");
  if (table !== "functions") continue;
  const known = byKey.get(`id:${id}`);
  touch(known?.key || `id:${id}`).add(normalize(`convert: ${message}`));
}

// Golden overlap.
const goldenIssues = new Map();
for (const dir of ["/mnt/jetscratch/candidates/BackendO11/g13/lower", join(process.env.HOME, ".cache/jet-dev/scratch/BackendO9/g12/lower")]) {
  if (!existsSync(dir)) continue;
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".issues"))) {
    const stem = file.slice(0, -".issues".length);
    if (goldenIssues.has(stem)) continue;
    goldenIssues.set(stem, new Set(lines(join(dir, file)).map((row) => normalize(row.slice(row.indexOf("\t") + 1)))));
  }
}
const goldenCount = new Map();
for (const set of goldenIssues.values()) for (const b of set) goldenCount.set(b, (goldenCount.get(b) ?? 0) + 1);

// Greedy curve.
const gated = new Map();
for (const [fn, blockers] of universe) for (const b of blockers) {
  if (!gated.has(b)) gated.set(b, new Set());
  gated.get(b).add(fn);
}
const total = universe.size;
let clean = [...universe.values()].filter((s) => s.size === 0).length;
const remaining = new Map([...universe].map(([fn, s]) => [fn, new Set(s)]));
const fixed = new Set();
const curve = [];
while (fixed.size < gated.size) {
  let best = null, bestUnlock = -1, bestGates = -1;
  for (const [b, fns] of gated) {
    if (fixed.has(b)) continue;
    let unlock = 0;
    for (const fn of fns) {
      const s = remaining.get(fn);
      if (s.size === 1 && s.has(b)) unlock++;
    }
    if (unlock > bestUnlock || (unlock === bestUnlock && fns.size > bestGates)) [best, bestUnlock, bestGates] = [b, unlock, fns.size];
  }
  fixed.add(best);
  for (const fn of gated.get(best)) remaining.get(fn).delete(best);
  clean += bestUnlock;
  const pkgs = new Map();
  for (const fn of gated.get(best)) {
    const pkg = lookup(fn).pkg;
    pkgs.set(pkg, (pkgs.get(pkg) ?? 0) + 1);
  }
  const pkgText = [...pkgs].sort((a, b) => b[1] - a[1]).map(([p, n]) => `${p}:${n}`).join(";");
  const samples = [...gated.get(best)].slice(0, 3).join(" | ");
  curve.push([curve.length + 1, best, gated.get(best).size, pkgText, `+${bestUnlock}`, clean, `${((100 * clean) / Math.max(1, total)).toFixed(1)}%`, goldenCount.get(best) ?? 0, paths(best).join(";"), samples].join("\t"));
}

const stats = Object.fromEntries(lines(join(O, "run/self.stats")).map((row) => row.split("\t")));
const header = [
  `# self-host native probe ${O} (rank ${new Date().toISOString()})`,
  `# unit: assembled Jet compiler + Compiler/JetCli/main.jet run; functions reached and lowered: ${total} (originals+instances+glue); clean now: ${[...universe.values()].filter((s) => s.size === 0).length}`,
  `# stats: ${JSON.stringify(stats)}`,
  `# preparation (Mono/Reach) issues, global: ${prepare.length}${prepare.length ? ` -> ${[...new Set(prepare)].slice(0, 20).join(" || ")}` : ""}`,
  `# conversion errors (rows left out of the stream): ${convertErrors.length}`,
  `# crash skips: ${lines(join(O, "run/skip.txt")).length}; object issues: ${lines(join(O, "run/self.object-issues")).length}; link issues: ${lines(join(O, "run/self.link-issues")).length}`,
  "# cumulative_clean is a conditional projection: a clean function may still call a blocked one, and fixing a blocker can reveal another.",
  "step\tblocker\tgates_functions\tpackages\tunlocks\tcumulative_clean\tcumulative_pct\tgolden_cases_same_blocker\tcandidate_paths_INFERENCE\tsample_functions",
];
writeFileSync(outPath, [...header, ...curve].join("\n") + "\n");
// Overlap with the golden curve: shared blockers get fixed once (BackendO11),
// compiler-only ones are this probe's batch.
const prepareCounts = new Map();
for (const b of prepare) prepareCounts.set(`prepare: ${b}`, (prepareCounts.get(`prepare: ${b}`) ?? 0) + 1);
const overlapRows = [...[...gated].map(([b, fns]) => [b, fns.size]), ...prepareCounts]
  .map(([b, n]) => [b, n, goldenCount.get(b.replace(/^prepare: /, "")) ?? 0])
  .sort((a, b) => b[1] - a[1]);
const overlapPath = outPath.replace(/RANKED\.tsv$/, "OVERLAP.tsv").replace(/\.tsv$/, outPath.endsWith("RANKED.tsv") ? ".tsv" : ".overlap.tsv");
writeFileSync(overlapPath, ["blocker\tselfhost_functions\tgolden_cases\tclass", ...overlapRows.map(([b, n, g]) => `${b}\t${n}\t${g}\t${g > 0 ? "shared" : "compiler-only"}`)].join("\n") + "\n");
console.log(header.slice(0, 7).join("\n"));
console.log(curve.slice(0, 40).join("\n"));
console.log(`shared with goldens: ${overlapRows.filter((r) => r[2] > 0).length}; compiler-only: ${overlapRows.filter((r) => r[2] === 0).length} -> ${overlapPath}`);
