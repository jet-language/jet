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
const assembleOnly = args.includes("--assemble-only");
const tests = args.includes("--tests");
const secretTests = args.includes("--secret-tests");
if (!output || args.some((arg) => !["--assemble-only", "--tests", "--secret-tests"].includes(arg)) || new Set(args).size !== args.length || (tests && secretTests)) {
  throw new Error("usage: check-runtime.mjs OUT [--assemble-only] [--tests|--secret-tests]");
}
const out = resolve(output);
const paths = readFileSync(join(root, "Compiler/JetRuntime/sources.list"), "utf8").split("\n")
  .map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
if (!paths.length || new Set(paths).size !== paths.length) throw new Error("empty or duplicate runtime source manifest");
if (tests) paths.push("Compiler/JetRuntime/Tests/AllocTests.jet");
if (secretTests) paths.push("Compiler/JetRuntime/Tests/SecretTests.jet");
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
writeFileSync(join(out, "package.jet"), readFileSync(join(root, "Compiler/JetRuntime/package.jet")));
console.log(unit);
if (!assembleOnly) {
  const home = process.env.HOME;
  const check = spawnSync(join(home, ".cache/jet-dev/cheap.sh"), [
    join(root, "Tools/agent/jet-env"), process.env.JET_CHECKER ?? join(home, ".cache/jet-dev/checker-jet"), "check", unit,
  ], { cwd: out, stdio: "inherit" });
  if (check.error) throw check.error;
  process.exitCode = check.status ?? 1;
}
