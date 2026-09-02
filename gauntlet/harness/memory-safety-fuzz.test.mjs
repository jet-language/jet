import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { runMemorySafetyFuzzAxis } from "./memory-safety-fuzz.mjs";

const ROOT = path.resolve(".");
const AXIS = {
  status: "required",
  schema: "gauntlet-axis-memory-safety-fuzz-v1",
  metric: "memory_safety_findings",
  corpus: {
    path: "fuzz-input.bin",
    generator: "xorshift32-v1",
    seed: 2272,
    case_count: 5,
    bytes_per_case: 8,
  },
  budget: { wall_timeout_ms: 1000, cpu_seconds: 1, memory_mb: 64 },
  oracle: { algorithm: "memory-safety-case-summary-v1", output: "" },
  runners: [{
    id: "test-rail",
    language: "test",
    tools: ["test-tool"],
    files: [{ source: "gauntlet/axes/memory-safety-fuzz/jet/run.jet", target: "run.jet" }],
    compile: ["test-compiler", "source"],
    run: ["test-runner"],
    evidence: { kind: "test-diagnostic", patterns: ["failure"] },
  }],
  fairness: ["same generated input file"],
};

async function context(root, overrides = {}) {
  const stageAxisFiles = async (stageDir, files) => {
    for (const file of files) {
      const target = path.resolve(stageDir, file.target);
      await fs.mkdir(path.dirname(target), { recursive: true });
      await fs.copyFile(path.resolve(ROOT, file.source), target);
    }
    return files.map((file) => ({ source: file.source, target: file.target }));
  };
  const axisStagePath = (stageDir, relative) => path.resolve(stageDir, relative);
  const fileSha256 = async (file) => {
    const { createHash } = await import("node:crypto");
    return createHash("sha256").update(await fs.readFile(file)).digest("hex");
  };
  return {
    runDir: root,
    repoDir: ROOT,
    jetBin: path.join(ROOT, "target/debug/jet"),
    outputLimit: 8192,
    stageAxisFiles,
    axisStagePath,
    probeAxisTools: async () => [{ tool: "test-tool", status: "available" }],
    axisCommand: (command) => command,
    runMemoryCommand: async () => ({ command: ["test"], process: {
      code: 0,
      signal: null,
      timed_out: false,
      resource_exceeded: null,
      stdout: "",
      stderr: "",
    } }),
    fileSha256,
    ...overrides,
  };
}

async function runFailure(overrides) {
  const root = await mkdtemp(path.join(ROOT, ".tmp-memory-axis-test-"));
  try {
    return await runMemorySafetyFuzzAxis(AXIS, await context(root, overrides));
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

function assertBlockedWithIdentity(report, stage, copied = report.corpus.sha256) {
  assert.equal(report.status, "incomplete");
  assert.equal(report.publication.status, "blocked");
  assert.ok(report.publication.blockers.some((blocker) => blocker.includes(`test-rail: ${stage} execution failed`)));
  assert.equal(report.runners.length, 1);
  const runner = report.runners[0];
  assert.equal(runner.status, "failed");
  assert.equal(runner.failed_stage, stage);
  assert.equal(runner.failure.stage, stage);
  assert.equal(runner.failure.status, "failed");
  assert.equal(runner[stage].status, "failed");
  assert.equal(runner[stage].failure.stage, stage);
  assert.equal(runner.corpus.generated_sha256, report.corpus.sha256);
  assert.equal(runner.corpus.copied_sha256, copied);
  assert.equal(runner.corpus.matches_generator, copied === report.corpus.sha256);
  assert.equal(runner.corpus.case_count, report.corpus.case_count);
  assert.equal(runner.corpus.bytes_per_case, report.corpus.bytes_per_case);
  assert.equal(runner.corpus.bytes, report.corpus.bytes);
  assert.deepEqual(runner.corpus.case_kinds, report.corpus.case_kinds);
  assert.equal(report.metrics.valid_cases, null);
  assert.equal(report.metrics.adversarial_cases, null);
}

test("setup exceptions retain corpus identity and block publication", async () => {
  const report = await runFailure({
    stageAxisFiles: async () => { throw new Error("staging failed"); },
  });
  assertBlockedWithIdentity(report, "setup", null);
  assert.equal(report.runners[0].setup.process.stderr, "staging failed");
});

test("compile exceptions retain corpus identity and block publication", async () => {
  const report = await runFailure({
    runMemoryCommand: async () => { throw new Error("compile failed"); },
  });
  assertBlockedWithIdentity(report, "compile");
  assert.equal(report.runners[0].compile.process.stderr, "compile failed");
  assert.deepEqual(report.runners[0].compile.command, AXIS.runners[0].compile);
});

test("run exceptions retain corpus identity and block publication", async () => {
  const report = await runFailure({
    runMemoryCommand: async (_cwd, command) => {
      if (command[0] === "test-runner") throw new Error("run failed");
      return { command, process: {
        code: 0,
        signal: null,
        timed_out: false,
        resource_exceeded: null,
        stdout: "",
        stderr: "",
      } };
    },
  });
  assertBlockedWithIdentity(report, "run");
  assert.equal(report.runners[0].run.process.stderr, "run failed");
  assert.deepEqual(report.runners[0].run.command, AXIS.runners[0].run);
});

test("a copied corpus digest mismatch fails closed before any rail executes", async () => {
  const report = await runFailure({
    fileSha256: async () => "bad-digest",
  });
  assertBlockedWithIdentity(report, "setup", "bad-digest");
  assert.match(report.runners[0].failure.reason, /does not match generated digest/);
  assert.equal(report.runners[0].setup.process.code, null);
});

test("findings retain exact classification and the Tower dedup command path", async () => {
  const report = await runFailure({
    runMemoryCommand: async (_cwd, command) => ({
      command,
      process: {
        code: 0,
        signal: null,
        timed_out: false,
        resource_exceeded: null,
        stdout: "",
        stderr: "",
      },
    }),
  });
  assert.equal(report.status, "complete");
  assert.equal(report.publication.status, "blocked");
  assert.match(report.publication.blockers.join("; "), /1 deduplicated finding receipt/);
  assert.equal(report.metrics.memory_safety_findings, 1);
  assert.equal(report.metrics.finding_occurrences, 1);
  assert.equal(report.findings.length, 1);
  const receipt = report.findings[0];
  assert.equal(receipt.kind, "wrong-output");
  assert.equal(receipt.classification, "correctness");
  assert.deepEqual(receipt.rails, ["test-rail"]);
  assert.equal(receipt.occurrences.length, 1);
  assert.equal(receipt.occurrences[0].classification, "wrong-output");
  assert.equal(receipt.occurrences[0].correctness_finding, true);
  assert.equal(receipt.occurrences[0].performance_exclusion, false);
  assert.deepEqual(receipt.tower_tracking.command, ["node", "plugins/tower/tower.mjs", "card", "add", "--file", "-"]);
  assert.equal(receipt.tower_tracking.status, "pending");
  assert.equal(receipt.tower_tracking.transport, "tower-cli");
  assert.equal(receipt.tower_tracking.dedup_key, receipt.tower_tracking.payload.hardeningDedupKey);
  assert.equal(receipt.tower_tracking.payload.classification, "wrong-output");
  assert.equal(receipt.tower_tracking.payload.correctness_finding, true);
  assert.equal(receipt.tower_tracking.payload.performance_exclusion, false);
  assert.equal(receipt.tower_tracking.payload.seed, report.corpus.seed);
  assert.equal(receipt.tower_tracking.payload.bytes, report.corpus.bytes);
  assert.equal(receipt.tower_tracking.payload.minimized_case.kind, "wrong-output");
});
