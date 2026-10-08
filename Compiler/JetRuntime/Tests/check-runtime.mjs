#!/usr/bin/env node
// Assemble only the runtime's explicit source manifest. The E2 object driver
// consumes the same unit; neither bootstrap nor backend harnesses include it.
// A default invocation runs the narrow jet check through the single cheap slot.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const output = args.shift();
let assembleOnly = false;
let test = null;
for (let at = 0; at < args.length; at++) {
  if (args[at] === "--assemble-only" && !assembleOnly) assembleOnly = true;
  else if (args[at] === "--test" && test === null && /^[A-Z][A-Za-z0-9]*$/u.test(args[at + 1] ?? "")) test = args[++at];
  else throw new Error("usage: check-runtime.mjs OUT [--assemble-only] [--test NAME]");
}
if (!output) throw new Error("usage: check-runtime.mjs OUT [--assemble-only] [--test NAME]");
const out = resolve(output);
const paths = readFileSync(join(root, "Compiler/JetRuntime/sources.list"), "utf8").split("\n")
  .map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
if (!paths.length || new Set(paths).size !== paths.length) throw new Error("empty or duplicate runtime source manifest");
if (test !== null) paths.push(`Compiler/JetRuntime/Tests/${test}Tests.jet`);
const uses = new Set();
const bodies = paths.map((path) => {
  const lines = readFileSync(join(root, path), "utf8").split("\n");
  let importing = false;
  const body = lines.filter((line) => {
    if (line.startsWith("use ")) {
      if (line.startsWith("use core.")) uses.add(line);
      importing = line.includes("[") && !line.includes("]");
      return false;
    }
    if (importing) {
      if (line.includes("]")) importing = false;
      return false;
    }
    return true;
  }).join("\n");
  return `// [unit source: ${path}]\n${body}`;
});
mkdirSync(out, { recursive: true });
const unit = join(out, "unit.jet");
writeFileSync(unit, [...uses, "", ...bodies].join("\n"));
const packageSource = test !== null
  ? 'name: "jet_runtime_tests"\nversion: "0.0.1"\nedition: "2028"\n'
  : readFileSync(join(root, "Compiler/JetRuntime/package.jet"));
writeFileSync(join(out, "package.jet"), packageSource);
console.log(unit);
if (!assembleOnly) {
  const home = process.env.HOME;
  const check = spawnSync(join(home, ".cache/jet-dev/cheap.sh"), [
    join(root, "Tools/agent/jet-env"), process.env.JET_CHECKER ?? join(home, ".cache/jet-dev/checker-jet"), "check", unit,
  ], { cwd: out, stdio: "inherit" });
  if (check.error) throw check.error;
  process.exitCode = check.status ?? 1;
}
