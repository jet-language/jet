import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  EXPECTED_OUTPUT,
  EXPECTED_SUM,
  INPUT_RANGE,
  ITERATIONS,
  METRIC,
  PAIR_ID,
  PLAIN_LABEL,
  PLAIN_PROGRAM,
  PLAIN_SOURCE,
  PERFORMANCE_SURFACE_SCHEMA,
  SURFACE_LABEL,
  SURFACE_PROGRAM,
  SURFACE_SOURCE,
  TARGET,
  TRIALS,
  WARMUPS,
  computeStatistics,
  performanceSurfaceInternals,
  validatePerformanceSurfaceReceipt,
} from "./performance-surface.mjs";

const outputBytes = Buffer.from(EXPECTED_OUTPUT, "utf8").toString("base64");
const artifactSha = "a".repeat(64);
const revision = "b".repeat(40);
const toolchain = { rustc: "rustc 1.0.0", node: "v22.0.0" };
const host = { hostname: "fixture", cpu_affinity: "0" };
const surfaceDigest = createHash("sha256").update(SURFACE_SOURCE).digest("hex");
const plainDigest = createHash("sha256").update(PLAIN_SOURCE).digest("hex");

function sample(label, wallTimeMs, source, context) {
  return {
    label,
    source_file: "main.jet",
    source_sha256: createHash("sha256").update(source).digest("hex"),
    pair_context_sha256: context.context_sha256,
    artifact_sha256: context.artifact_sha256,
    environment_sha256: context.environment_sha256,
    tier: context.tier,
    target: context.target,
    input_range: context.inputs.range,
    output_oracle: context.output_validation.oracle,
    wall_time_ms: wallTimeMs,
    status: 0,
    exit_code: 0,
    signal: null,
    timed_out: false,
    error_code: null,
    stdout: EXPECTED_OUTPUT,
    stderr: "",
    stdout_bytes_base64: outputBytes,
    stderr_bytes_base64: "",
    output_verified: true,
    output_matches: true,
    contaminated: false,
    directory_entries: ["main.jet"],
  };
}

function validFixture() {
  const surface = Array.from({ length: TRIALS }, (_value, index) => 10 + (index % 3));
  const plain = Array.from({ length: TRIALS }, (_value, index) => 20 + (index % 3));
  const trialOrders = Array.from({ length: TRIALS }, (_value, index) => index < TRIALS / 2 ? "surface_first" : "plain_first");
  const warmupOrders = Array.from({ length: WARMUPS }, (_value, index) => index < WARMUPS / 2 ? "surface_first" : "plain_first");
  const environment = { revision, toolchain, host };
  const contextBase = {
    inputs: { iterations: ITERATIONS, range: INPUT_RANGE, expected_sum: EXPECTED_SUM },
    tier: "interpreter",
    target: TARGET,
    metric: METRIC,
    artifact_sha256: artifactSha,
    environment_sha256: performanceSurfaceInternals.environmentDigest(environment),
    sampling: {
      strategy: "balanced_seeded_fisher_yates",
      warmups: WARMUPS,
      trials: TRIALS,
      warmup_seed: 2977,
      trial_seed: 2978,
      warmup_orders: warmupOrders,
      trial_orders: trialOrders,
      fresh_process_per_arm: true,
      fresh_source_file_per_arm: true,
    },
    output_validation: { encoding: "utf8", exact: true, oracle: EXPECTED_OUTPUT },
  };
  const context = { ...contextBase, context_sha256: performanceSurfaceInternals.measurementContextDigest(contextBase) };
  const stats = computeStatistics(surface, plain);
  const trials = trialOrders.map((order, index) => ({
    index,
    order,
    paired: true,
    surface: sample(`trial-${index}-surface`, surface[index], SURFACE_SOURCE, context),
    plain: sample(`trial-${index}-plain`, plain[index], PLAIN_SOURCE, context),
    surface_wall_time_ms: surface[index],
    plain_wall_time_ms: plain[index],
    ratio: surface[index] / plain[index],
    difference_ms: surface[index] - plain[index],
  }));
  const warmups = warmupOrders.map((order, index) => ({
    index,
    order,
    surface: sample(`warmup-${index}-surface`, 10, SURFACE_SOURCE, context),
    plain: sample(`warmup-${index}-plain`, 20, PLAIN_SOURCE, context),
  }));
  const row = {
    id: PAIR_ID,
    canonical: SURFACE_LABEL,
    alternate: PLAIN_LABEL,
    surface: SURFACE_LABEL,
    plain: PLAIN_LABEL,
    workload: "100000-element integer reduction with identical final stdout",
    surface_program: SURFACE_PROGRAM,
    plain_program: PLAIN_PROGRAM,
    expected_output: EXPECTED_OUTPUT,
    surface_source: SURFACE_SOURCE,
    plain_source: PLAIN_SOURCE,
    surface_source_sha256: surfaceDigest,
    plain_source_sha256: plainDigest,
    artifact_sha256: artifactSha,
    revision,
    measurement_context: context,
    measurement_context_sha256: context.context_sha256,
    environment_sha256: context.environment_sha256,
    tier: context.tier,
    target: context.target,
    input_range: context.inputs.range,
    timing_status: "measured",
    surface_output_matches: true,
    plain_output_matches: true,
    preflight_issues: [],
    warmup_orders: warmupOrders,
    trial_orders: trialOrders,
    warmups,
    trials,
    trial_pairs: trials,
    surface_trials_ms: surface,
    plain_trials_ms: plain,
    paired_ratios: stats.paired_ratios,
    paired_differences_ms: stats.paired_differences_ms,
    surface_wall_time_ms: stats.surface_median_ms,
    plain_wall_time_ms: stats.plain_median_ms,
    ratio: stats.ratio,
    confidence: stats.bootstrap,
    verdict: "win",
  };
  return {
    schema: PERFORMANCE_SURFACE_SCHEMA,
    version: 1,
    protocol: "paired-surface-v2",
    target: TARGET,
    tier: "interpreter",
    metric: METRIC,
    artifact: "target/debug/jet",
    artifact_sha256: artifactSha,
    artifact_sha256_after: artifactSha,
    revision,
    toolchain,
    host,
    environment_sha256: context.environment_sha256,
    measurement_context: context,
    input: { iterations: ITERATIONS, range: INPUT_RANGE, expected_sum: EXPECTED_SUM },
    warmups: WARMUPS,
    trials: TRIALS,
    output_oracle: EXPECTED_OUTPUT,
    pairs: [row],
  };
}

test("accepts only a strict proven win", () => {
  const result = validatePerformanceSurfaceReceipt(validFixture());
  assert.equal(result.status, "pass");
  assert.equal(result.valid, true);
  assert.equal(result.verdict, "win");
});

test("rejects missing confidence", () => {
  const receipt = validFixture();
  delete receipt.pairs[0].confidence;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "missing-confidence"));
});

test("rejects overlapping confidence", () => {
  const receipt = validFixture();
  receipt.pairs[0].confidence.ratio_upper95 = 1;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "overlapping-confidence"));
});

test("rejects stale pair identity", () => {
  const receipt = validFixture();
  receipt.pairs[0].surface_source = receipt.pairs[0].surface_source.replace("total += i", "total = total + i");
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "stale-identity"));
});

test("rejects wrong ratio", () => {
  const receipt = validFixture();
  receipt.pairs[0].ratio = 0.25;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "wrong-ratio"));
});

test("rejects wrong output", () => {
  const receipt = validFixture();
  receipt.pairs[0].trials[0].surface.output_verified = false;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "wrong-output"));
});

test("rejects missing samples", () => {
  const receipt = validFixture();
  receipt.pairs[0].surface_trials_ms.pop();
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "missing-sample"));
});

test("rejects unpaired rows", () => {
  const receipt = validFixture();
  delete receipt.pairs[0].trials[7].plain;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "unpaired-row"));
});
test("rejects timeout, signal, and nonzero arms", () => {
  for (const [field, value, kind] of [
    ["timed_out", true, "timeout"],
    ["signal", "SIGTERM", "signal"],
    ["status", 2, "nonzero"],
  ]) {
    const receipt = validFixture();
    receipt.pairs[0].trials[0].surface[field] = value;
    assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === kind));
  }
});

test("rejects a missing shared pair context", () => {
  const receipt = validFixture();
  delete receipt.measurement_context;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "missing-identity"));
});

test("rejects arm identity substitution", () => {
  const receipt = validFixture();
  receipt.pairs[0].trials[0].plain.artifact_sha256 = "c".repeat(64);
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "identity-mismatch"));
});

test("rejects warmup process failure", () => {
  const receipt = validFixture();
  receipt.pairs[0].warmups[0].surface.status = 1;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "nonzero"));
});

test("rejects forged bootstrap confidence", () => {
  const receipt = validFixture();
  receipt.pairs[0].confidence.ratio_upper95 = 0.1;
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "wrong-confidence"));
});

test("rejects a preflight-contaminated measurement even when point ratio wins", () => {
  const receipt = validFixture();
  receipt.preflight_issues = ["compiler artifact changed during measurement"];
  assert.ok(validatePerformanceSurfaceReceipt(receipt).findings.some((finding) => finding.kind === "unavailable"));
});
