#!/usr/bin/env node
// Exact Int layer exit corpus, including boxed literals and numeric stops.
// --plan assembles selection without consuming a build/proof slot.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { collectGoldenEntries } from "../../../Tools/agent/compiler-diff.mjs";
import {readGoldenResults} from "./golden-results.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, mode, sharedResults] = process.argv.slice(2);
if (!output || (mode && mode !== "--plan" && mode !== "--results") ||
    (mode === "--results" ? !sharedResults || process.argv.length !== 5 : process.argv.length > 4)) {
  throw Error("usage: run-int-goldens.mjs OUT [--plan | --results fresh-results.tsv]");
}
const out = resolve(output);
const names = new Set([
  "basics/digits", "math/bit_not", "math/copy", "math/exact_division_extremes",
  "math/float_bits_range_stop", "math/float_bits_wide", "math/floor_div",
  "math/int_div", "math/int_euclidean", "math/int_from_u64_extreme",
  "math/int_runtime_boundary", "math/modulo", "math/numeric_floor",
  "math/power_operator", "math/power_semantics", "math/xor_operator",
  "lowlevel/sized_integers", "operators/shift_count_width", "operators/spaceship",
]);
const entries = collectGoldenEntries(join(root, "Examples/features"));
const available = new Set(entries.map(({ stem }) => stem));
for (const stem of names) if (!available.has(stem)) throw Error(`missing required numeric golden ${stem}`);
// Include every registered big-Int stem, not just current filename spellings.
for (const { stem, source } of entries) {
  const path = typeof source === "string" && existsSync(source) ? source : join(root, "Examples/features", `${stem}.jet`);
  if (existsSync(path) && /\b\d[\d_]{19,}\b/u.test(readFileSync(path, "utf8"))) names.add(stem);
}
const stems = [...names].sort();
mkdirSync(out, { recursive: true });
const cases = join(out, "int.cases");
writeFileSync(cases, stems.join("\n") + "\n");
console.log(`exact Int goldens: ${stems.length} stems`);
if (mode !== "--plan") {
  if (!process.env.JET_RUNTIME_PACK) throw Error("JET_RUNTIME_PACK must name the pack containing Jet exact-Int exports");
  const run = mode === "--results" ? null : spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--front", "jet", "--case-list", cases], {
    cwd: root, stdio: "inherit", env: { ...process.env, JET_MIR_EMIT: "0" },
  });
  const resultsPath = sharedResults ? resolve(sharedResults) : join(out, "results.tsv");
  const rows = readGoldenResults(resultsPath);
  const failures = stems.filter((stem) => rows.get(stem) !== "pass");
  writeFileSync(join(out, "int-parity.json"), JSON.stringify({ required: stems.length, passing: stems.length - failures.length, failures }, null, 2) + "\n");
  if ((run && (run.error || run.status !== 0)) || failures.length || (!sharedResults && rows.size !== stems.length)) throw Error(`numeric golden proof failed: ${failures.join(", ")}; harness=${run?.status ?? run?.signal ?? "shared results"}`);
}
