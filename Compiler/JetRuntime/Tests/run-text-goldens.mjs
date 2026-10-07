#!/usr/bin/env node
// L3a's exit witness: baseline native behavior plus focused print/interpolation,
// byte/scalar comparison and split/rsplit cases, against the Jet text object.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, baseline, mode] = process.argv.slice(2);
if (!output || !baseline || (mode && mode !== "--plan") || process.argv.length > 5) {
  throw new Error("usage: run-text-goldens.mjs OUT BASELINE.results.tsv [--plan]");
}
const out = resolve(output);
const rows = readFileSync(baseline, "utf8").trim().split("\n").map((line) => line.split("\t"));
const passing = rows.filter(([, verdict]) => verdict === "pass").map(([stem]) => stem);
if (!passing.length || new Set(rows.map(([stem]) => stem)).size !== rows.length) throw new Error("empty or duplicate baseline");
const focused = ["basics/print_lines", "basics/interp_debug_label", "text/string_len_scalars", "text/runtime_text_core"];
const stems = [...new Set([...passing, ...focused])].sort();
for (const stem of focused) {
  if (!existsSync(join(root, "Examples/features", `${stem}.jet`)) || !existsSync(join(root, "Examples/features/expected", `${stem}.out`))) {
    throw new Error(`missing focused golden ${stem}`);
  }
}
mkdirSync(out, { recursive: true });
const cases = join(out, "text.cases");
writeFileSync(cases, `${stems.join("\n")}\n`);
console.log(`${passing.length} baseline passing stems; ${focused.length} focused text stems; ${stems.length} distinct requirements`);
if (mode !== "--plan") {
  const object = process.env.JET_RUNTIME_JET_OBJECT;
  const pack = process.env.JET_RUNTIME_PACK;
  if (!object || !pack || !existsSync(object) || !existsSync(pack)) throw new Error("JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK must name this tree's fresh Jet-object pack");
  const nm = spawnSync("nm", ["-g", "--defined-only", object], { encoding: "utf8" });
  if (nm.error || nm.status !== 0) throw new Error(nm.error?.message ?? nm.stderr);
  const names = new Set(nm.stdout.trim().split("\n").map((line) => line.trim().split(/\s+/u).at(-1)));
  const required = ["jet_rt_string_from_static", "jet_rt_string_clone", "jet_rt_string_drop", "jet_rt_string_compare", "jet_rt_string_slice_range", "jet_rt_native_iter_string_split", "jet_rt_native_iter_string_rsplit"];
  for (const name of required) if (!names.has(name)) throw new Error(`Jet object does not define ${name}`);
  const undefinedSymbols = spawnSync("nm", ["-u", object], { encoding: "utf8" });
  if (undefinedSymbols.error || undefinedSymbols.status !== 0) throw new Error(undefinedSymbols.error?.message ?? undefinedSymbols.stderr);
  writeFileSync(join(out, "jet-runtime-undefined.txt"), undefinedSymbols.stdout);
  const imports = undefinedSymbols.stdout.trim().split("\n").filter(Boolean).map((line) => line.trim().split(/\s+/u).at(-1));
  const unresolved = spawnSync("nm", ["-u", pack], { encoding: "utf8" });
  if (unresolved.error || unresolved.status !== 0) throw new Error(unresolved.error?.message ?? unresolved.stderr);
  writeFileSync(join(out, "pack-undefined.txt"), unresolved.stdout);
  if (unresolved.stdout.trim()) throw new Error("runtime pack has unresolved imports; see pack-undefined.txt");
  // The sole declared exact-Int import is the existing list-bounds seam in
  // Trap.jet; text itself never calls exact-Int arithmetic or conversions.
  const illegalInts = imports.filter((name) => name.startsWith("jet_int_") && name !== "jet_int_owned_to_i64");
  if (illegalInts.length) throw new Error(`unlisted exact-Int imports: ${illegalInts.join(", ")}`);
  const harness = spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--case-list", cases], {
    cwd: root, stdio: "inherit", env: { ...process.env, JET_MIR_EMIT: "0" },
  });
  const resultsPath = join(out, "results.tsv");
  const results = existsSync(resultsPath) ? new Map(readFileSync(resultsPath, "utf8").trim().split("\n").map((line) => line.split("\t"))) : new Map();
  const failures = stems.filter((stem) => results.get(stem) !== "pass");
  const receipt = { required: stems.length, passed: stems.length - failures.length, failures, imports, object, pack };
  writeFileSync(join(out, "text-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(JSON.stringify(receipt, null, 2));
  if (harness.error) console.error(harness.error.message);
  if (failures.length || harness.status !== 0) process.exitCode = 1;
}
