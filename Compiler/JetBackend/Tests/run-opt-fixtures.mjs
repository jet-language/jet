#!/usr/bin/env node
// Assemble all LIR optimizer passes and fixtures into one package-free unit,
// then run every fixture under the sprint's bounded cheap-check wrapper.
// Usage: JET=/path/to/jet node run-opt-fixtures.mjs <scratch-outdir>
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2] && resolve(process.argv[2]);
if (!outDir) {
  console.error("usage: run-opt-fixtures.mjs <scratch-outdir>");
  process.exit(64);
}
const read = (path) => readFileSync(resolve(repo, path), "utf8");
const coreUses = new Set();
function body(path) {
  const kept = [];
  let skipping = false;
  for (const line of read(path).split("\n")) {
    if (skipping) {
      if (line.startsWith("]")) skipping = false;
      continue;
    }
    if (/^use jet_\w+\.\[\s*$/.test(line)) {
      skipping = true;
      continue;
    }
    if (/^use jet_\w+/.test(line)) continue;
    if (/^use core\./.test(line)) coreUses.add(line);
    else kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}
const sorted = (dir) => readdirSync(resolve(repo, dir)).filter((name) => name.endsWith(".jet")).sort().map((name) => `${dir}/${name}`);
const typePath = "Compiler/JetFoundation/Source/MIR/MIR.jet";
const typeLine = read(typePath).split("\n").find((line) => /^(pub )?struct MIRTypeID /.test(line));
if (!typeLine) throw new Error(`${typePath}: missing MIRTypeID`);
const fixturePaths = sorted("Compiler/JetBackend/Tests/Fragments.fixture/OptFixtures");
if (fixturePaths.length === 0) throw new Error("no optimizer fixtures");
const fixtureNames = fixturePaths.map((path) => path.split("/").at(-1).slice(0, -4).toLowerCase());
const parts = [
  typeLine,
  ...["LIR", "Lint", "Print"].map((name) => body(`Compiler/JetBackend/Source/LIR/${name}.jet`)),
  ...sorted("Compiler/JetBackend/Source/Opt").map(body),
  ...fixturePaths.map(body),
  "fn run() {",
  "    passed := true",
  ...fixtureNames.map((name) => `    if !opt_fixtures_${name}() -> passed = false`),
  '    if passed -> print("OPT FIXTURES PASS") else -> print("OPT FIXTURES FAIL")',
  "}",
];
mkdirSync(outDir, { recursive: true });
writeFileSync(resolve(outDir, "unit.jet"), [...coreUses, "", ...parts].join("\n"));
writeFileSync(resolve(outDir, "package.jet"), 'name: "jet_backend_opt_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n');
const envPath = resolve(homedir(), ".cache/jet-dev/scratch/NativeLead4/release-52303d4e2.env");
const jet = process.env.JET ?? readFileSync(envPath, "utf8").split("\n").find((line) => line.startsWith("PQ_BIN_jet=")).slice("PQ_BIN_jet=".length);
const result = spawnSync(resolve(homedir(), ".cache/jet-dev/cheap.sh"), [jet, "run", "unit.jet"], {
  cwd: outDir, encoding: "utf8", maxBuffer: 1 << 28, env: process.env,
});
writeFileSync(resolve(outDir, "output.log"), (result.stdout ?? "") + (result.stderr ?? ""));
process.stdout.write(result.stdout ?? "");
process.stderr.write(result.stderr ?? "");
if (result.error) console.error(result.error.message);
const passed = (result.stdout ?? "").split(/\r?\n/).includes("OPT FIXTURES PASS");
process.exit(result.status === 0 && passed ? 0 : result.status || 1);
