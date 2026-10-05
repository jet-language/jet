// Gauntlet adapter for axes.compile_throughput (Tower #4529): runs the
// cell's full lane through Tools/perf/throughput/harness.mjs with the
// gauntlet's `jet`, and projects its receipt onto the axis report. The
// harness owns measurement and every gate; a failed or incomplete receipt
// blocks publication with the harness's own failure lines.
import fs from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";

export const COMPILE_THROUGHPUT_SCHEMA = "gauntlet-axis-compile-throughput-v1";

export function compileThroughputContractIssues(axis) {
  const issues = [];
  if (!axis) return ["compile_throughput axis is missing"];
  if (axis.status !== "required") issues.push("compile_throughput must be required");
  if (axis.schema !== COMPILE_THROUGHPUT_SCHEMA) issues.push(`compile_throughput schema must be ${COMPILE_THROUGHPUT_SCHEMA}`);
  if (axis.metric !== "clean_build_wall_ns") issues.push("compile_throughput metric must be clean_build_wall_ns");
  if (axis.harness !== "Tools/perf/throughput/harness.mjs") issues.push("compile_throughput harness must be Tools/perf/throughput/harness.mjs");
  if (JSON.stringify((axis.programs ?? []).map((program) => program.id)) !== JSON.stringify(["bench300k", "selfhost"])) issues.push("compile_throughput programs must be bench300k and selfhost");
  if (JSON.stringify((axis.arms ?? []).map((arm) => arm.id)) !== JSON.stringify(["default", "single"])) issues.push("compile_throughput arms must be default and single");
  if (axis.samples !== 5 || axis.warmups !== 1) issues.push("compile_throughput takes 5 samples after 1 warmup");
  return issues;
}

export async function runCompileThroughputAxisAdapter(axis, { runDir, jetBin, repoDir }) {
  const axisDir = path.join(runDir, "axes", "compile_throughput");
  await fs.mkdir(axisDir, { recursive: true });
  const receiptPath = path.join(axisDir, "receipt.json");
  const args = [path.join(repoDir, axis.harness), "--lane", "full", "--compiler", `jet=${jetBin}`,
    "--samples", String(axis.samples), "--warmups", String(axis.warmups), "--work-dir", axisDir, "--receipt", receiptPath];
  const exit = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, { cwd: repoDir, stdio: ["ignore", "inherit", "inherit"] });
    child.on("error", reject);
    child.on("exit", (code) => resolve(code));
  });
  let receipt = null;
  try { receipt = JSON.parse(await fs.readFile(receiptPath, "utf8")); } catch { /* reported below */ }
  const blockers = receipt ? [...receipt.failures] : [`the throughput harness exited ${exit} without a receipt`];
  const complete = Boolean(receipt) && receipt.programs.length === axis.programs.length
    && receipt.programs.every((program) => program.arms.length === axis.arms.length && program.arms.every((arm) => arm.summary));
  const metrics = {};
  for (const program of receipt?.programs ?? []) {
    for (const arm of program.arms) {
      if (arm.summary) metrics[`${program.id}.${arm.id}`] = { clean_build_wall_ns: arm.summary.wall_ns, peak_rss_bytes: arm.summary.peak_rss_bytes };
    }
  }
  return {
    id: "compile_throughput",
    required: axis.status === "required",
    status: complete ? "complete" : "incomplete",
    contract: axis,
    schema: axis.schema,
    metric: axis.metric,
    receipt: receiptPath,
    compiler: receipt?.compiler ?? null,
    metrics,
    gates: receipt?.gates ?? null,
    publication: { status: receipt?.passed ? "ready" : "blocked", blockers },
  };
}
