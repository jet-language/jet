#!/usr/bin/env node
// The compiler-diff canary candidate (#670): runs the real compiler named by
// COMPILER_DIFF_CANARY_REAL unchanged, except for the divergences that
// tests/compiler-diff/canary/canary.json plants on named cases and phases.
// No source is mutated; only this candidate's observable output differs.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const real = process.env.COMPILER_DIFF_CANARY_REAL;
if (!real) {
  process.stderr.write("canary-candidate: COMPILER_DIFF_CANARY_REAL is not set\n");
  process.exit(2);
}
const args = process.argv.slice(2);
const manifest = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), "canary/canary.json"), "utf8"));
const phase = args[0] === "run" ? (args.includes("--interpret") ? "interpret" : "run") : args[0];
const file = args.findLast((arg) => arg.endsWith(".jet"));
const plant = file && manifest.plants.find((row) => row.case === basename(file) && row.phase === phase);

const run = spawnSync(real, args, { stdio: ["inherit", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024 });
if (run.error) {
  process.stderr.write(`canary-candidate: ${run.error.message}\n`);
  process.exit(2);
}
let stdout = run.stdout.toString("utf8");
let stderr = run.stderr.toString("utf8");
let status = run.status;
if (plant?.replace) {
  const [from, to] = plant.replace;
  if (plant.stream === "stdout") stdout = stdout.replaceAll(from, to);
  else stderr = stderr.replaceAll(from, to);
}
if (plant && Number.isInteger(plant.exit)) status = plant.exit;
process.stdout.write(stdout);
process.stderr.write(stderr);
if (run.signal && !plant) process.kill(process.pid, run.signal);
process.exitCode = status ?? 1;
