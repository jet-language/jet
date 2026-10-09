#!/usr/bin/env node
// L3a's exit witness: baseline native behavior plus focused print/interpolation,
// byte/scalar comparison and split/rsplit cases, against the Jet text object.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { candidateJetc0, runJetFront } from "../../JetBackend/Tests/jet-front.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
import {readGoldenResults} from "./golden-results.mjs";
const [output, baseline, mode, sharedResults] = process.argv.slice(2);
if (!output || !baseline || (mode && mode !== "--plan" && mode !== "--results") ||
    (mode === "--results" ? !sharedResults || process.argv.length !== 6 : process.argv.length > 5)) {
  throw new Error("usage: run-text-goldens.mjs OUT BASELINE.results.tsv [--plan | --results fresh-results.tsv]");
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
  const failures = [];
  function nm(file, args, receipt) {
    const run = spawnSync("nm", [...args, "--format=posix", file], { encoding: "utf8", maxBuffer: 1 << 28 });
    writeFileSync(join(out, receipt), `${run.stdout ?? ""}${run.stderr ?? ""}`);
    if (run.error || run.status !== 0) {
      failures.push(`nm ${file}: ${run.error?.message ?? run.stderr}`);
      return [];
    }
    return run.stdout.split("\n").filter(Boolean).map((line) => line.trim().split(/\s+/u));
  }
  const definitions = nm(object, ["-g", "--defined-only"], "text-object-symbols.txt");
  const packDefinitions = nm(pack, ["-g", "--defined-only"], "text-pack-symbols.txt");
  const required = ["jet_rt_string_from_static", "jet_rt_string_clone", "jet_rt_string_concat", "jet_rt_string_drop", "jet_rt_string_compare", "jet_rt_string_slice_range", "jet_rt_native_iter_string_split", "jet_rt_native_iter_string_rsplit"];
  for (const name of required) {
    for (const [label, symbols] of [["Jet object", definitions], ["pack", packDefinitions]]) {
      const count = symbols.filter(([symbol, type]) => symbol === name && type === "T").length;
      if (count !== 1) failures.push(`${label}: ${name} has ${count} strong function definitions`);
    }
  }
  const imports = nm(object, ["-u"], "jet-runtime-undefined.txt").map(([name]) => name);
  const unresolved = nm(pack, ["-u"], "pack-undefined.txt");
  if (unresolved.length) failures.push(`runtime pack has unresolved imports: ${unresolved.map(([name]) => name).join(", ")}`);
  const admitted = new Set(readFileSync(join(root, "Compiler/JetRuntime/imports.list"), "utf8").split("\n")
    .map((line) => line.trim()).filter((line) => line && !line.startsWith("#")));
  const illegalImports = imports.filter((name) => !admitted.has(name));
  if (illegalImports.length) failures.push(`unlisted runtime imports: ${illegalImports.join(", ")}`);
  // The combined L3a/L5 proof runs the union once; raw and symbol witnesses
  // remain mandatory here when consuming its actual result stream.
  const harness = mode === "--results" ? null : spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--front", "jet", "--case-list", cases], {
    cwd: root, stdio: "inherit", env: process.env,
  });
  const resultsPath = sharedResults ? resolve(sharedResults) : join(out, "results.tsv");
  const results = existsSync(resultsPath) ? readGoldenResults(resultsPath) : new Map();
  failures.push(...stems.filter((stem) => results.get(stem) !== "pass").map((stem) => `${stem}: ${results.get(stem) ?? "missing"}`));
  if (harness && (harness.error || harness.status !== 0)) failures.push(`native harness: ${harness.error?.message ?? `rc=${harness.status}`}`);
  // The raw witness checks carrier ownership, lossy malformed UTF-8 and
  // iterator exhaustion directly, using the existing Jet-native factory path.
  const raw = join(out, "raw");
  const assemble = spawnSync(process.execPath, [
    join(root, "Compiler/JetRuntime/Tests/check-runtime.mjs"), raw, "--assemble-only", "--test", "Text",
  ], { cwd: root, stdio: "inherit" });
  if (assemble.error || assemble.status !== 0) {
    failures.push(`raw witness assembly: ${assemble.error?.message ?? `rc=${assemble.status}`}`);
  } else {
    try {
      // Keep the executable outside raw/package.jet's library boundary.
      const executableRoot = join(out, "raw-native");
      mkdirSync(executableRoot, { recursive: true });
      const library = readFileSync(join(raw, "unit.jet"), "utf8");
      const candidate = candidateJetc0();
      const program = join(executableRoot, "unit.jet");
      writeFileSync(program, `${library}\nfn run() { text_tests() }\n`);
      const prefix = join(executableRoot, "text-test");
      const problem = runJetFront({
        candidate, file: program, root: executableRoot,
        cwd: root, prefix, pack, store: join(raw, "store"),
      });
      if (problem) failures.push(`raw witness compile: ${problem.detail}`);
      else {
        const witness = spawnSync(`${prefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
        writeFileSync(join(raw, "run.stdout"), witness.stdout ?? "");
        writeFileSync(join(raw, "run.stderr"), witness.stderr ?? "");
        writeFileSync(join(raw, "run.status.json"), `${JSON.stringify({ status: witness.status, signal: witness.signal, error: witness.error?.message ?? null })}\n`);
        if (witness.error || witness.status !== 0 || witness.signal || witness.stdout || witness.stderr) {
          failures.push(`raw witness run: ${witness.error?.message ?? `rc=${witness.status}, signal=${witness.signal}, stdout=${JSON.stringify(witness.stdout)}, stderr=${JSON.stringify(witness.stderr)}`}`);
        }
      }
      for (const [name, entry, message] of [
        ["cstring-nul", "text_c_string_nul_stop", "native C String contains NUL"],
        ["cstring-invalid", "text_c_string_invalid_stop", "native C String is not UTF-8"],
        ["cstring-null", "text_c_string_null_stop", "native C String is null"],
        ["concat-capacity", "text_concat_capacity_stop", "String capacity overflow"],
      ]) {
        const directory = join(executableRoot, name);
        mkdirSync(directory, { recursive: true });
        const file = join(directory, "unit.jet");
        const prefix = join(directory, "stop");
        writeFileSync(file, `${library}\nfn run() { ${entry}() }\n`);
        const problem = runJetFront({
          candidate, file, root: directory, cwd: root, prefix,
          pack, store: join(raw, "store"),
        });
        if (problem) failures.push(`${name} compile: ${problem.detail}`);
        else {
          const stopped = spawnSync(`${prefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
          writeFileSync(join(directory, "run.stdout"), stopped.stdout ?? "");
          writeFileSync(join(directory, "run.stderr"), stopped.stderr ?? "");
          writeFileSync(join(directory, "run.status.json"), `${JSON.stringify({ status: stopped.status, signal: stopped.signal, error: stopped.error?.message ?? null })}\n`);
          const headline = `Stop [E3001]: \`panic: ${message}\`\n  --> <core.prelude>:0\n`;
          if (stopped.error || stopped.status !== 70 || stopped.signal || stopped.stdout || !stopped.stderr?.startsWith(headline)) {
            failures.push(`${name}: ${stopped.error?.message ?? `rc=${stopped.status}, signal=${stopped.signal}, stdout=${JSON.stringify(stopped.stdout)}, stderr=${JSON.stringify(stopped.stderr)}`}`);
          }
        }
      }
    } catch (error) { failures.push(`raw witness: ${error.message}`); }
  }
  const receipt = { required: stems.length, passed: stems.filter((stem) => results.get(stem) === "pass").length, failures, imports, object, pack, resultsPath };
  writeFileSync(join(out, "text-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(JSON.stringify(receipt, null, 2));
  for (const failure of failures) console.error(failure);
  if (failures.length) process.exitCode = 1;
}
