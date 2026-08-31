#!/usr/bin/env node

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";
import {
  AdapterBlockedError,
  HarnessUsageError,
  RegressionError,
  baselineFromReport,
  buildControlContext,
  compareBaseline,
  preflightAdapters,
  validateCapsule,
} from "./run-cold-context.mjs";

const descriptor = (name) => ({ path: `${name}.fixture`, bytes: 1, sha256: name });
const adapters = [
  {
    id: "anthropic",
    family: "anthropic",
    transport: "command",
    command_sha256: "anthropic-command",
  },
  {
    id: "openai",
    family: "openai",
    transport: "api",
    protocol: "openai-chat",
    model: "gpt-stock",
    endpoint_sha256: "openai-endpoint",
  },
];
const tasks = [{ id: "hello", domain: "hello", mode: "batch", spec_sha256: "hello-task", expected_sha256: "hello-output" }];

test("preflight stays blocked when neither provider path is configured", async () => {
  const configPath = fileURLToPath(new URL("./adapters.json", import.meta.url));
  const config = JSON.parse(await fs.readFile(configPath, "utf8"));
  const unavailable = config.adapters.map((adapter) => ({
    ...adapter,
    default_transport: "command",
    command: { ...adapter.command, default_argv: undefined },
  }));
  const result = preflightAdapters(unavailable, {});
  assert.deepEqual(result.resolved, []);
  assert.deepEqual(result.blocked, [
    "openai: set JET_COLD_AGENT_OPENAI_COMMAND for command transport",
    "anthropic: set JET_COLD_AGENT_ANTHROPIC_COMMAND for command transport",
  ]);
});

test("preflight accepts both built-in OMP command transports", async () => {
  const configPath = fileURLToPath(new URL("./adapters.json", import.meta.url));
  const config = JSON.parse(await fs.readFile(configPath, "utf8"));
  const result = preflightAdapters(config.adapters, {});
  assert.deepEqual(result.resolved.map(({ adapter }) => adapter.family), ["openai", "anthropic"]);
  assert.deepEqual(result.blocked, []);
  const openai = config.adapters.find((adapter) => adapter.family === "openai");
  assert.deepEqual(openai.command.default_argv.slice(0, 3), ["omp", "--model", "openai-codex/gpt-5.6-luna"]);
  const anthropic = config.adapters.find((adapter) => adapter.family === "anthropic");
  assert.deepEqual(anthropic.command.default_argv.slice(0, 3), ["omp", "--model", "opus"]);
});

function report(capsuleScore) {
  const rows = adapters.flatMap((adapter) => [
    { adapter: adapter.id, family: adapter.family, context: "capsule", task: "hello", compile_score: capsuleScore, run_score: capsuleScore, score: capsuleScore },
    { adapter: adapter.id, family: adapter.family, context: "control", task: "hello", compile_score: 1, run_score: 1, score: 1 },
  ]);
  return {
    schema: "jet.cold-agent.scoreboard.v1",
    status: "recorded",
    contexts: ["capsule", "control"],
    required_families: ["openai", "anthropic"],
    capsule: descriptor("capsule"),
    control: {
      path: "llms.text",
      source_bytes: 20,
      source_sha256: "llms",
      context_budget_bytes: 10,
      context_sha256: "control",
    },
    fixtures: { task_file: descriptor("tasks"), adapter_file: descriptor("adapters") },
    harness: descriptor("harness"),
    adapters,
    tasks,
    rows,
    summary: {
      capsule: { cases: 2, passes: capsuleScore * 2, compile_passes: capsuleScore * 2, run_passes: capsuleScore * 2, pass_rate: capsuleScore },
      control: { cases: 2, passes: 2, compile_passes: 2, run_passes: 2, pass_rate: 1 },
    },
    blocked_reasons: [],
  };
}

test("truncated llms control is exactly the capsule byte budget", () => {
  const source = "alpha\nβeta\n";
  const budget = Buffer.byteLength("alpha\nβ", "utf8");
  const control = buildControlContext(source, budget);
  assert.equal(Buffer.byteLength(control, "utf8"), budget);
  assert.equal(control, "alpha\nβ");
});

test("baseline comparison rejects a lower capsule score", () => {
  const baseline = baselineFromReport(report(1));
  assert.throws(() => compareBaseline(report(0), baseline), RegressionError);
});

test("baseline comparison binds the task and adapter identities", () => {
  const baseline = baselineFromReport(report(1));
  const changed = report(1);
  changed.tasks = [{ ...changed.tasks[0], spec_sha256: "changed-task" }];
  assert.throws(() => compareBaseline(changed, baseline), RegressionError);
  const changedAdapter = report(1);
  changedAdapter.adapters = changedAdapter.adapters.map((adapter) => (
    adapter.id === "openai" ? { ...adapter, model: "different-stock-model" } : adapter
  ));
  assert.throws(() => compareBaseline(changedAdapter, baseline), RegressionError);
});

test("control-only runs cannot become capsule baselines", () => {
  const controlOnly = report(1);
  controlOnly.contexts = ["control"];
  controlOnly.rows = controlOnly.rows.filter((row) => row.context === "control");
  controlOnly.summary.capsule = { cases: 0, passes: 0, compile_passes: 0, run_passes: 0, pass_rate: null };
  assert.throws(() => baselineFromReport(controlOnly), HarnessUsageError);
});

test("baseline comparison fails closed before any score is claimed", () => {
  assert.throws(() => compareBaseline(report(1), { schema: "jet.cold-agent.baseline.v1", status: "blocked" }), AdapterBlockedError);
  const empty = baselineFromReport(report(1));
  empty.rows = [];
  assert.throws(() => compareBaseline(report(1), empty), AdapterBlockedError);
});

test("capsule removal canary rejects a changed verb or missing program", async () => {
  const capsulePath = fileURLToPath(new URL("./jet-context-capsule.md", import.meta.url));
  const capsule = await fs.readFile(capsulePath, "utf8");
  validateCapsule(capsule);
  assert.throws(() => validateCapsule(capsule.replace("`term.print`", "`term.removed`")), HarnessUsageError);
  assert.throws(() => validateCapsule(capsule.replace("fn run() {\n    print(\"Hello, Jet\")\n}", "print(\"Hello, Jet\")")), HarnessUsageError);
  assert.throws(() => validateCapsule(capsule.replace(/\n### 10\. HTTP health endpoint[\s\S]*?\n## 7\. Cold-agent rules/u, "\n## 7. Cold-agent rules")), HarnessUsageError);
});
