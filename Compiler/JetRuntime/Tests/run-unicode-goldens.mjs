#!/usr/bin/env node
// L3b exit witness: run Unicode and Core/text casing goldens against a pack
// containing the Jet runtime object. --plan writes the exact nonempty corpus
// without compiling, linking, running a checker, or executing a golden.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { collectGoldenEntries } from "../../../Tools/agent/compiler-diff.mjs";
import { candidateJetc0, runJetFront } from "../../JetBackend/Tests/jet-front.mjs";
import {readGoldenResults} from "./golden-results.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, mode, sharedResults] = process.argv.slice(2);
if (!output || (mode && mode !== "--plan" && mode !== "--results") ||
    (mode === "--results" ? !sharedResults || process.argv.length !== 5 : process.argv.length > 4)) {
  throw new Error("usage: run-unicode-goldens.mjs OUT [--plan | --results fresh-results.tsv]");
}
const out = resolve(output);
const symbols = ["count", "index_of", "last_index_of", "lower", "trim", "upper"].map((name) => `jet_unicode_${name}`);
const stems = collectGoldenEntries(join(root, "Examples/features")).map(({ stem }) => stem)
  .filter((stem) => /^text\/(?:unicode_.*|runtime_unicode|runtime_string_equal|strings|core_text_calls|text_tabs_title|text_boundaries)$/u.test(stem) || stem.startsWith("conformance/core/text/"))
  .sort();
if (!stems.includes("text/runtime_unicode") || !stems.includes("text/unicode_17") || !stems.includes("text/unicode_text")) {
  throw new Error("Unicode exit corpus lacks a required witness");
}
mkdirSync(out, { recursive: true });
const cases = join(out, "unicode.cases");
writeFileSync(cases, `${stems.join("\n")}\n`);
console.log(`${stems.length} Unicode/casing stems:\n${stems.join("\n")}`);
if (mode !== "--plan") {
  const pack = process.env.JET_RUNTIME_PACK;
  const object = process.env.JET_RUNTIME_JET_OBJECT;
  if (!pack || !object) throw new Error("JET_RUNTIME_PACK and JET_RUNTIME_JET_OBJECT must name the same E2 cutover's pack and Jet object");
  const failures = [];
  function nm(file, options, receipt) {
    const run = spawnSync("nm", [...options, "--format=posix", file], { encoding: "utf8", maxBuffer: 1 << 28 });
    writeFileSync(join(out, receipt), `${run.stdout ?? ""}${run.stderr ?? ""}`);
    if (run.error || run.status !== 0) {
      failures.push(`nm ${file}: ${run.error?.message ?? run.stderr}`);
      return [];
    }
    return run.stdout.split("\n").filter(Boolean).map((line) => line.trim().split(/\s+/u));
  }
  const objectSymbols = nm(object, ["-g", "--defined-only"], "unicode-object-symbols.txt");
  const packSymbols = nm(pack, ["-g", "--defined-only"], "unicode-pack-symbols.txt");
  // Each direct ABI symbol is a strong function in BOTH files, never merely a
  // Rust implementation that made the golden pass without the Jet definition.
  for (const symbol of symbols) {
    for (const [label, rows] of [["Jet object", objectSymbols], ["pack", packSymbols]]) {
      const definitions = rows.filter(([name, type]) => name === symbol && type === "T");
      if (definitions.length !== 1) failures.push(`${label}: ${symbol} has ${definitions.length} strong function definitions`);
    }
  }
  // Preserve the standalone object's import inventory for the layer reviewer;
  // remaining layers may still be supplied by Rust in this pack cutover.
  const imports = nm(object, ["-u"], "unicode-object-imports.txt");
  const admitted = new Set(readFileSync(join(root, "Compiler/JetRuntime/imports.list"), "utf8").split("\n")
    .map((line) => line.trim()).filter((line) => line && !line.startsWith("#")));
  const illegalInts = imports.map(([name]) => name).filter((name) => name.startsWith("jet_int_") && !admitted.has(name));
  if (illegalInts.length) failures.push(`unlisted exact-Int imports (Unicode itself has none): ${illegalInts.join(", ")}`);
  const unresolved = nm(pack, ["-u"], "unicode-pack-imports.txt");
  if (unresolved.length) failures.push(`unresolved final pack symbols: ${unresolved.map(([name]) => name).join(", ")}`);
  const run = mode === "--results" ? null : spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--front", "jet", "--case-list", cases], {
    cwd: root, stdio: "inherit", env: process.env,
  });
  // The general backend harness also reports unsupported results. This exit
  // witness requires EVERY selected stem to pass, not merely zero wrong cases.
  let rows = new Map();
  try { rows = readGoldenResults(sharedResults ? resolve(sharedResults) : join(out, "results.tsv")); }
  catch (error) { failures.push(`missing golden results: ${error.message}`); }
  for (const stem of stems) {
    if (rows.get(stem) !== "pass") failures.push(`${stem}: ${rows.get(stem) ?? "missing result"}`);
  }
  if (run && (run.error || run.status !== 0)) failures.push(`native harness: ${run.error?.message ?? `rc=${run.status}`}`);
  // NUL is not a quoted-text escape in Jet. The raw runtime-only witness also
  // observes Option payload writes and verifies that a miss leaves the slot
  // untouched. It uses the same assembler and Jet-native front-end path.
  const raw = join(out, "raw");
  const assemble = spawnSync(process.execPath, [
    join(root, "Compiler/JetRuntime/Tests/check-runtime.mjs"), raw, "--assemble-only", "--test", "Unicode",
  ], { cwd: root, stdio: "inherit" });
  if (assemble.error || assemble.status !== 0) {
    failures.push(`raw witness assembly: ${assemble.error?.message ?? `rc=${assemble.status}`}`);
  } else {
    try {
      const native = join(out, "raw-native");
      mkdirSync(native, { recursive: true });
      const executable = join(native, "unit.jet");
      writeFileSync(executable, `${readFileSync(join(raw, "unit.jet"), "utf8")}\nfn run() { unicode_tests() }\n`);
      const prefix = join(native, "unicode-test");
      const problem = runJetFront({
        candidate: candidateJetc0(), file: executable, root: native,
        cwd: root, prefix,
        pack, store: join(native, "store"),
      });
      if (problem) failures.push(`raw witness compile: ${problem.detail}`);
      else {
        const witness = spawnSync(`${prefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
        writeFileSync(join(raw, "run.stdout"), witness.stdout ?? "");
        writeFileSync(join(raw, "run.stderr"), witness.stderr ?? "");
        writeFileSync(join(raw, "run.status.json"), `${JSON.stringify({ status: witness.status, signal: witness.signal, error: witness.error?.message ?? null })}\n`);
        if (witness.error || witness.status !== 0 || witness.signal) {
          failures.push(`raw witness run: ${witness.error?.message ?? `rc=${witness.status}, signal=${witness.signal}`}`);
        }
      }
    } catch (error) { failures.push(`raw witness: ${error.message}`); }
  }
  writeFileSync(join(out, "unicode-exit-result.json"), `${JSON.stringify({ stems, symbols, failures }, null, 2)}\n`);
  for (const failure of failures) console.error(failure);
  if (failures.length) process.exitCode = 1;
}
