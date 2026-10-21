#!/usr/bin/env node
// Runtime-layer generators share one freshness gate. Run all checks even when
// one fails so the owner fixes the whole class before the next proof.
// Usage: node Tools/agent/gen-runtime.mjs <allparts.rs> [--check]
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
if (args.length < 1 || args.length > 2 || args[0].startsWith("--") || (args.length === 2 && args[1] !== "--check")) {
  console.error("usage: gen-runtime.mjs <allparts.rs> [--check]");
  process.exit(64);
}
const runtime = resolve(args[0]);
const check = args.includes("--check") ? ["--check"] : [];
// Trap literals feed the runtime unit, then its export list owns the C symbols
// that the route generator omits from the remaining Rust adapters.
// Jet-authored generators run with the compiler built from this tree (JET).
const jet = process.env.JET ?? "jet";
const steps = [
  ["RuntimeGenerators", check],
  ["gen-runtime-lifecycle.mjs", check],
  ["gen-runtime-stop-exports.mjs", check],
  ["gen-c-abi-routes.mjs", [runtime, ...check]],
];
let failed = false;
for (const [name, flags] of steps) {
  const script = join("Tools/agent", name);
  const command = name.endsWith(".mjs") ? [process.execPath, join(root, script), ...flags] : [jet, "run", script, "--", ...flags];
  const result = spawnSync(command[0], command.slice(1), { cwd: root, stdio: "inherit" });
  if (result.error || result.status !== 0) {
    console.error(`${name} failed: ${result.error?.message ?? result.signal ?? `exit ${result.status}`}`);
    failed = true;
  }
}
if (failed) process.exitCode = 1;
else console.log(`runtime generators: ${check.length ? "CHECK OK" : "generated"}`);
