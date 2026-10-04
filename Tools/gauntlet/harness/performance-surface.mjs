#!/usr/bin/env node
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync, spawnSync } from "node:child_process";
import { performance } from "node:perf_hooks";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../../..");

export const PERFORMANCE_SURFACE_SCHEMA = "jet.gauntlet.performance-surface-evidence.v1";
export const PERFORMANCE_SURFACE_VERSION = 1;
export const PAIR_ID = "compound_assign_vs_binary_assign";
export const SURFACE_LABEL = "compound assignment";
export const PLAIN_LABEL = "binary expression followed by assignment";
export const SURFACE_PROGRAM = `gauntlet:performance-surface:${PAIR_ID}:surface`;
export const PLAIN_PROGRAM = `gauntlet:performance-surface:${PAIR_ID}:plain`;
export const ITERATIONS = 100000;
export const INPUT_RANGE = "0..<100000";
export const EXPECTED_SUM = 4999950000;
export const EXPECTED_OUTPUT = `performance-surface:${PAIR_ID}:${EXPECTED_SUM}`;
export const TARGET = "jet run --interpret";
export const METRIC = "wall_time_ms";
export const WARMUPS = 10;
export const TRIALS = 100;
export const ORDER_SEED = 2977;
export const BOOTSTRAP_RESAMPLES = 10000;
export const BOOTSTRAP_SEED = 2977;
export const BOOTSTRAP_LEVEL = 0.95;
export const BOOTSTRAP_LOWER_RANK = 250;
export const BOOTSTRAP_UPPER_RANK = 9750;
export const DEFAULT_TIMEOUT_MS = 120000;
export const HISTORICAL_RECEIPTS = Object.freeze([
  "Tools/gauntlet/performance-surface-evidence-20260906T154128Z.json",
  "Tools/gauntlet/performance-surface-evidence-20260906T155749Z.json",
  "Tools/gauntlet/performance-surface-evidence-20260906T160409Z.json",
]);

export const SURFACE_SOURCE = [
  `// Real paired workload: ${PAIR_ID} surface spelling`,
  "fn run() {",
  "    total := 0",
  `    loop i in ${INPUT_RANGE} {`,
  "        total += i",
  "    }",
  `    print(\"performance-surface:${PAIR_ID}:{total}\")`,
  "}",
].join("\n");

export const PLAIN_SOURCE = [
  `// Real paired workload: ${PAIR_ID} plain spelling`,
  "fn run() {",
  "    total := 0",
  `    loop i in ${INPUT_RANGE} {`,
  "        total = total + i",
  "    }",
  `    print(\"performance-surface:${PAIR_ID}:{total}\")`,
  "}",
].join("\n");

const WORKLOAD = "100000-element integer reduction with identical final stdout";
const SETUP = "fresh interpreter process and fresh main.jet per warmup/trial; no generated artifact";
const ORDER_LABELS = Object.freeze(["surface_first", "plain_first"]);
export const PAIR_MEASUREMENT = Object.freeze({
  inputs: Object.freeze({ iterations: ITERATIONS, range: INPUT_RANGE, expected_sum: EXPECTED_SUM }),
  tier: "interpreter",
  target: TARGET,
  artifact_binding: "one_artifact_sha256",
  environment_binding: "one_host_toolchain_affinity_identity",
  sampling: "balanced_seeded_fisher_yates; fresh process and fresh main.jet per arm",
  output_validation: "exact_utf8_stdout_bytes",
});

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const finite = (value) => typeof value === "number" && Number.isFinite(value);
const nonEmpty = (value) => typeof value === "string" && value.trim().length > 0;

export function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

export function md5(value) {
  return createHash("md5").update(value).digest("hex");
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (isObject(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}
function measurementContextDigest(context) {
  if (!isObject(context)) return null;
  const { context_sha256: _ignored, ...base } = context;
  return sha256(canonical(base));
}

function environmentDigest(identity) {
  return sha256(canonical({
    revision: identity?.revision ?? null,
    toolchain: identity?.toolchain ?? null,
    host: identity?.host ?? null,
  }));
}

function makeMeasurementContext(identity, warmupOrders, trialOrders) {
  const context = {
    inputs: { iterations: ITERATIONS, range: INPUT_RANGE, expected_sum: EXPECTED_SUM },
    tier: PAIR_MEASUREMENT.tier,
    target: TARGET,
    metric: METRIC,
    artifact_sha256: identity?.artifact?.sha256 ?? null,
    environment_sha256: environmentDigest(identity),
    sampling: {
      strategy: "balanced_seeded_fisher_yates",
      warmups: WARMUPS,
      trials: TRIALS,
      warmup_seed: ORDER_SEED,
      trial_seed: ORDER_SEED + 1,
      warmup_orders: warmupOrders,
      trial_orders: trialOrders,
      fresh_process_per_arm: true,
      fresh_source_file_per_arm: true,
    },
    output_validation: {
      encoding: "utf8",
      exact: true,
      oracle: EXPECTED_OUTPUT,
    },
  };
  return { ...context, context_sha256: measurementContextDigest(context) };
}

function closeEnough(left, right) {
  return finite(left) && finite(right)
    && Math.abs(left - right) <= Math.max(1e-9, Math.abs(left) * 1e-9, Math.abs(right) * 1e-9);
}

export function median(values) {
  if (!Array.isArray(values) || values.length === 0 || values.some((value) => !finite(value))) return null;
  const ordered = [...values].sort((left, right) => left - right);
  return ordered[Math.floor(ordered.length / 2)];
}

function nearestRank(values, rank) {
  if (!Array.isArray(values) || values.length === 0 || !Number.isInteger(rank) || rank < 1 || rank > values.length) return null;
  const ordered = [...values].sort((left, right) => left - right);
  return ordered[rank - 1];
}

function roundMs(value) {
  return finite(value) ? Number(value.toFixed(3)) : null;
}

function seededRandom(seed) {
  let state = (Number(seed) >>> 0);
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 0x100000000;
  };
}

export function balancedOrder(count, seed = ORDER_SEED) {
  if (!Number.isInteger(count) || count <= 0 || count % 2 !== 0) throw new Error("balanced order count must be a positive even integer");
  const order = Array.from({ length: count }, (_value, index) => index < count / 2 ? "surface_first" : "plain_first");
  const random = seededRandom(seed);
  for (let index = order.length - 1; index > 0; index -= 1) {
    const swap = Math.floor(random() * (index + 1));
    [order[index], order[swap]] = [order[swap], order[index]];
  }
  return order;
}

function digestNumbers(values) {
  return sha256(values.map((value) => finite(value) ? value.toPrecision(17) : "null").join("\n"));
}

export function pairedBootstrap(surface, plain, options = {}) {
  const resamples = options.resamples ?? BOOTSTRAP_RESAMPLES;
  const seed = options.seed ?? BOOTSTRAP_SEED;
  if (!Array.isArray(surface) || !Array.isArray(plain) || surface.length !== plain.length || surface.length === 0) {
    throw new Error("paired bootstrap requires equal non-empty vectors");
  }
  if (!surface.every(finite) || !plain.every(finite) || plain.some((value) => value <= 0) || surface.some((value) => value <= 0)) {
    throw new Error("paired bootstrap requires positive finite vectors");
  }
  if (!Number.isInteger(resamples) || resamples <= 0) throw new Error("paired bootstrap requires positive resamples");
  const random = seededRandom(seed);
  const ratioReplicates = new Array(resamples);
  const differenceReplicates = new Array(resamples);
  const surfaceSample = new Array(surface.length);
  const plainSample = new Array(plain.length);
  const differenceSample = new Array(surface.length);
  for (let replicate = 0; replicate < resamples; replicate += 1) {
    for (let index = 0; index < surface.length; index += 1) {
      const selected = Math.floor(random() * surface.length);
      surfaceSample[index] = surface[selected];
      plainSample[index] = plain[selected];
      differenceSample[index] = surface[selected] - plain[selected];
    }
    const surfaceMedian = median(surfaceSample);
    const plainMedian = median(plainSample);
    ratioReplicates[replicate] = surfaceMedian / plainMedian;
    differenceReplicates[replicate] = median(differenceSample);
  }
  const ratioLower95 = nearestRank(ratioReplicates, options.lowerRank ?? BOOTSTRAP_LOWER_RANK);
  const ratioUpper95 = nearestRank(ratioReplicates, options.upperRank ?? BOOTSTRAP_UPPER_RANK);
  const differenceLower95 = nearestRank(differenceReplicates, options.lowerRank ?? BOOTSTRAP_LOWER_RANK);
  const differenceUpper95 = nearestRank(differenceReplicates, options.upperRank ?? BOOTSTRAP_UPPER_RANK);
  return {
    method: "paired_index_bootstrap",
    statistic: "median(surface_wall_time_ms) / median(plain_wall_time_ms)",
    difference_statistic: "median(surface_wall_time_ms - plain_wall_time_ms)",
    confidence: BOOTSTRAP_LEVEL,
    resamples,
    seed,
    quantile: "nearest_rank_one_based",
    lower_rank: options.lowerRank ?? BOOTSTRAP_LOWER_RANK,
    upper_rank: options.upperRank ?? BOOTSTRAP_UPPER_RANK,
    ratio_lower95: ratioLower95,
    ratio_upper95: ratioUpper95,
    difference_lower95: differenceLower95,
    difference_upper95: differenceUpper95,
    ratio_bootstrap_sha256: digestNumbers(ratioReplicates),
    difference_bootstrap_sha256: digestNumbers(differenceReplicates),
  };
}

export function computeStatistics(surface, plain, options = {}) {
  if (!Array.isArray(surface) || !Array.isArray(plain) || surface.length !== plain.length || surface.length === 0) {
    throw new Error("statistics require equal non-empty vectors");
  }
  if (!surface.every((value) => finite(value) && value > 0) || !plain.every((value) => finite(value) && value > 0)) {
    throw new Error("statistics require positive finite vectors");
  }
  const ratios = surface.map((value, index) => value / plain[index]);
  const differences = surface.map((value, index) => value - plain[index]);
  const surfaceMedian = median(surface);
  const plainMedian = median(plain);
  const ratio = surfaceMedian / plainMedian;
  return {
    surface_median_ms: surfaceMedian,
    plain_median_ms: plainMedian,
    ratio,
    paired_ratios: ratios,
    paired_differences_ms: differences,
    bootstrap: pairedBootstrap(surface, plain, options),
  };
}

export function classifyVerdict(ratio, confidence) {
  if (!finite(ratio) || !isObject(confidence)
    || !finite(confidence.ratio_lower95) || !finite(confidence.ratio_upper95)) return "unavailable";
  if (ratio < 1 && confidence.ratio_upper95 < 1) return "win";
  if (ratio >= 1 || confidence.ratio_lower95 >= 1) return "loss";
  return "inconclusive";
}

function verdictReason(verdict, confidence) {
  if (verdict === "win") return "strict surface/plain win: ratio and paired-bootstrap upper 95% bound are both below 1";
  if (verdict === "loss") return "surface/plain is not strictly below 1: equality or a lower confidence bound at/above 1 is a loss";
  if (verdict === "inconclusive") return `paired-bootstrap 95% interval overlaps 1 (ratio interval ${confidence?.ratio_lower95 ?? "missing"}..${confidence?.ratio_upper95 ?? "missing"})`;
  return "measurement unavailable: required identity, process, output, sample, or confidence evidence is missing";
}

function sampleTime(sample) {
  if (!isObject(sample)) return null;
  return [sample.wall_time_ms, sample.duration_ms, sample.elapsed_ms].find(finite) ?? null;
}

function sampleStatus(sample) {
  if (!isObject(sample)) return false;
  return sample.status === 0 && sample.signal == null && sample.timed_out !== true && sample.error_code == null;
}

function sampleOutputMatches(sample, expectedOutput) {
  if (!isObject(sample)) return false;
  if (sample.output_verified === false || sample.output_matches === false) return false;
  const expectedBytes = Buffer.from(expectedOutput, "utf8").toString("base64");
  if (sample.stdout_bytes_base64 !== undefined) {
    return sample.stdout_bytes_base64 === expectedBytes
      && sample.output_verified === true
      && sample.output_matches === true;
  }
  return sample.stdout === expectedOutput
    && sample.output_verified === true
    && sample.output_matches === true;
}

function pairRowsFromReceipt(receipt, row) {
  if (Array.isArray(row?.trial_pairs)) return row.trial_pairs;
  if (Array.isArray(row?.trials)) return row.trials;
  if (Array.isArray(receipt?.trial_pairs)) return receipt.trial_pairs;
  return [];
}

function armSample(row, arm) {
  if (!isObject(row)) return null;
  const hasNestedArm = Object.hasOwn(row, "surface") || Object.hasOwn(row, "plain");
  const direct = row[arm];
  if (isObject(direct)) return direct;
  if (hasNestedArm) return null;
  const value = row[`${arm}_wall_time_ms`];
  if (finite(value)) return { wall_time_ms: value, status: 0, output_verified: true, signal: null, timed_out: false };
  return null;
}

function pairIdentityIssues(receipt, row, pairSpec = null) {
  const issues = [];
  const expect = pairSpec ?? {};
  if (receipt?.schema !== PERFORMANCE_SURFACE_SCHEMA || receipt?.version !== PERFORMANCE_SURFACE_VERSION) {
    issues.push({ kind: "stale-identity", cause: "receipt schema/version is not the current performance-surface contract" });
  }
  if (row?.id !== PAIR_ID || (expect.id && expect.id !== PAIR_ID)) {
    issues.push({ kind: "stale-identity", cause: `pair id must be ${PAIR_ID}` });
  }
  if (row?.canonical !== SURFACE_LABEL || row?.surface !== SURFACE_LABEL || (expect.surface && expect.surface !== SURFACE_LABEL)) {
    issues.push({ kind: "identity-mismatch", cause: "surface spelling identity differs from compound assignment" });
  }
  if (row?.alternate !== PLAIN_LABEL || row?.plain !== PLAIN_LABEL || (expect.plain && expect.plain !== PLAIN_LABEL)) {
    issues.push({ kind: "identity-mismatch", cause: "plain spelling identity differs from binary expression followed by assignment" });
  }
  if (row?.surface_program !== SURFACE_PROGRAM || row?.plain_program !== PLAIN_PROGRAM) {
    issues.push({ kind: "identity-mismatch", cause: "surface/plain program identities do not match the pair" });
  }
  if (row?.surface_source !== SURFACE_SOURCE || row?.plain_source !== PLAIN_SOURCE) {
    issues.push({ kind: "stale-identity", cause: "source spelling is stale or differs from the ratified pair" });
  }
  if (row?.surface_source_sha256 !== sha256(SURFACE_SOURCE) || row?.plain_source_sha256 !== sha256(PLAIN_SOURCE)) {
    issues.push({ kind: "stale-identity", cause: "source digest does not match the ratified pair" });
  }
  if (receipt?.target !== TARGET || receipt?.metric !== METRIC || receipt?.tier !== PAIR_MEASUREMENT.tier) {
    issues.push({ kind: "identity-mismatch", cause: "receipt target, tier, or metric differs from jet run --interpret / wall_time_ms" });
  }
  if (receipt?.input?.range !== INPUT_RANGE || receipt?.input?.iterations !== ITERATIONS || receipt?.input?.expected_sum !== EXPECTED_SUM) {
    issues.push({ kind: "stale-identity", cause: "workload range or integer reduction identity differs from 0..<100000" });
  }
  if (receipt?.output_oracle !== EXPECTED_OUTPUT || row?.expected_output !== EXPECTED_OUTPUT) {
    issues.push({ kind: "wrong-output", cause: "output oracle is not the exact compound-assignment pair oracle" });
  }
  if (receipt?.warmups !== WARMUPS || receipt?.trials !== TRIALS) {
    issues.push({ kind: "stale-identity", cause: `receipt must contain exactly ${WARMUPS} warmups and ${TRIALS} trial pairs` });
  }
  return issues;
}

function identityCompletenessIssues(receipt, row) {
  const issues = [];
  const artifactSha = receipt?.artifact_sha256 ?? receipt?.artifact?.sha256;
  if (!nonEmpty(artifactSha) || !/^[0-9a-f]{64}$/i.test(artifactSha)) {
    issues.push({ kind: "missing-identity", cause: "compiler artifact SHA-256 is missing" });
  }
  if (!nonEmpty(receipt?.revision)) issues.push({ kind: "missing-identity", cause: "compiler revision is missing" });
  if (!isObject(receipt?.toolchain) || !nonEmpty(receipt.toolchain.rustc) || !nonEmpty(receipt.toolchain.node)) {
    issues.push({ kind: "missing-identity", cause: "toolchain identity is missing" });
  }
  if (!isObject(receipt?.host) || !nonEmpty(receipt.host.hostname) || !nonEmpty(receipt.host.cpu_affinity)) {
    issues.push({ kind: "missing-identity", cause: "host or CPU affinity identity is missing" });
  }
  if (row?.artifact_sha256 && artifactSha && row.artifact_sha256 !== artifactSha) {
    issues.push({ kind: "stale-identity", cause: "pair artifact digest differs from receipt artifact digest" });
  }
  if (row?.revision && receipt?.revision && row.revision !== receipt.revision) {
    issues.push({ kind: "stale-identity", cause: "pair revision differs from receipt revision" });
  }
  return issues;
}

function sampleIssues(sample, expectedOutput, location) {
  const issues = [];
  const time = sampleTime(sample);
  if (!finite(time) || time <= 0) issues.push({ kind: "missing-sample", cause: `${location} has no positive wall_time_ms sample` });
  if (!sampleStatus(sample)) {
    if (sample?.timed_out === true) issues.push({ kind: "timeout", cause: `${location} timed out` });
    if (sample?.signal != null) issues.push({ kind: "signal", cause: `${location} terminated by ${sample.signal}` });
    if (sample?.status !== 0) issues.push({ kind: "nonzero", cause: `${location} did not exit successfully` });
    if (sample?.error_code) issues.push({ kind: "process-failure", cause: `${location} failed with ${sample.error_code}` });
  }
  if (!sampleOutputMatches(sample, expectedOutput)) issues.push({ kind: "wrong-output", cause: `${location} stdout does not exactly match the output oracle` });
  if (sample?.contaminated === true) issues.push({ kind: "contamination", cause: `${location} source directory contains generated or unexpected files` });
  if (Array.isArray(sample?.directory_entries) &&
      (sample.directory_entries.length !== 1 || sample.directory_entries[0] !== "main.jet")) {
    issues.push({ kind: "contamination", cause: `${location} source directory contains unexpected entries` });
  }
  return issues;
}

function confidenceFrom(row, receipt) {
  return row?.confidence ?? row?.bootstrap ?? receipt?.confidence ?? receipt?.bootstrap ?? null;
}
function measurementContextIssues(receipt, row) {
  const issues = [];
  const context = receipt?.measurement_context;
  const add = (kind, cause) => issues.push({ kind, cause });
  if (!isObject(context)) {
    add("missing-identity", "shared pair measurement context is missing");
    return issues;
  }
  const environment = {
    revision: receipt?.revision ?? null,
    toolchain: receipt?.toolchain ?? null,
    host: receipt?.host ?? null,
  };
  if (context.context_sha256 !== measurementContextDigest(context)) {
    add("stale-identity", "shared pair measurement context digest is stale");
  }
  if (canonical(context.inputs) !== canonical(PAIR_MEASUREMENT.inputs) ||
      context.tier !== PAIR_MEASUREMENT.tier ||
      context.target !== TARGET ||
      context.metric !== METRIC ||
      context.artifact_sha256 !== (receipt?.artifact_sha256 ?? null) ||
      context.environment_sha256 !== environmentDigest(environment)) {
    add("identity-mismatch", "plain and surface arms do not share the declared input, tier, artifact, or environment identity");
  }
  const sampling = context.sampling;
  if (!isObject(sampling) ||
      sampling.strategy !== "balanced_seeded_fisher_yates" ||
      sampling.warmups !== WARMUPS ||
      sampling.trials !== TRIALS ||
      sampling.warmup_seed !== ORDER_SEED ||
      sampling.trial_seed !== ORDER_SEED + 1 ||
      sampling.fresh_process_per_arm !== true ||
      sampling.fresh_source_file_per_arm !== true ||
      !Array.isArray(sampling.warmup_orders) ||
      !Array.isArray(sampling.trial_orders)) {
    add("missing-sample", "shared pair sampling contract is missing or stale");
  } else {
    if (row && canonical(row.warmup_orders) !== canonical(sampling.warmup_orders)) {
      add("unpaired-row", "warmup order vector differs from the shared sampling contract");
    }
    if (row && canonical(row.trial_orders) !== canonical(sampling.trial_orders)) {
      add("unpaired-row", "trial order vector differs from the shared sampling contract");
    }
  }
  const output = context.output_validation;
  if (!isObject(output) || output.encoding !== "utf8" || output.exact !== true || output.oracle !== EXPECTED_OUTPUT) {
    add("wrong-output", "shared pair output validation is not exact UTF-8 stdout against the oracle");
  }
  if (row?.measurement_context_sha256 !== context.context_sha256 ||
      canonical(row?.measurement_context) !== canonical(context) ||
      row?.artifact_sha256 !== context.artifact_sha256 ||
      row?.tier !== context.tier ||
      row?.target !== context.target ||
      row?.input_range !== context.inputs?.range ||
      row?.environment_sha256 !== context.environment_sha256) {
    add("identity-mismatch", "pair row is not bound to the shared measurement context");
  }
  if (receipt?.artifact_sha256_after !== receipt?.artifact_sha256) {
    add("stale-identity", "compiler artifact changed or disappeared during measurement");
  }
  return issues;
}

function sampleBindingIssues(sample, source, context, location) {
  const issues = [];
  const add = (kind, cause) => issues.push({ kind, cause });
  if (!isObject(sample)) return [{ kind: "unpaired-row", cause: `${location} arm is missing` }];
  if (sample.source_file !== "main.jet" || sample.source_sha256 !== sha256(source)) {
    add("identity-mismatch", `${location} source file or digest is not bound to its declared program`);
  }
  if (sample.pair_context_sha256 !== context?.context_sha256 ||
      sample.artifact_sha256 !== context?.artifact_sha256 ||
      sample.environment_sha256 !== context?.environment_sha256 ||
      sample.tier !== context?.tier ||
      sample.target !== context?.target ||
      sample.input_range !== INPUT_RANGE ||
      sample.output_oracle !== EXPECTED_OUTPUT) {
    add("identity-mismatch", `${location} does not share the pair input, tier, artifact, environment, or output identity`);
  }
  return issues;
}

export function validatePerformanceSurfaceReceipt(receipt, options = {}) {
  const findings = [];
  const add = (kind, cause) => findings.push({ kind, cell: PAIR_ID, metric: METRIC, cause });
  const rows = Array.isArray(receipt?.pairs) ? receipt.pairs : [];
  const pairSpec = options.pairSpec ?? options.pair ?? null;
  const row = rows.find((candidate) => candidate?.id === (pairSpec?.id ?? PAIR_ID)) ?? rows[0] ?? null;
  if (rows.length !== 1) add("unpaired-row", "receipt must contain exactly one configured performance surface pair");
  for (const issue of pairIdentityIssues(receipt, row, pairSpec)) add(issue.kind, issue.cause);
  for (const issue of identityCompletenessIssues(receipt, row)) add(issue.kind, issue.cause);
  for (const issue of measurementContextIssues(receipt, row)) add(issue.kind, issue.cause);
  for (const issue of (Array.isArray(receipt?.preflight_issues) ? receipt.preflight_issues : [])) {
    add("unavailable", `measurement preflight failed: ${issue}`);
  }
  if (!row) {
    add("missing-sample", "receipt has no performance surface pair row");
    return { schema: receipt?.schema ?? null, version: receipt?.version ?? null, status: "fail", valid: false, verdict: "unavailable", findings, pair: null };
  }
  if (receipt?.status !== undefined && receipt.status !== "pass") {
    add("unavailable", `receipt status is ${receipt.status}`);
  }
  if (row.timing_status !== "measured") {
    add("unavailable", `pair timing status is ${row.timing_status ?? "missing"}`);
  }
  if (row.surface_output_matches !== true || row.plain_output_matches !== true) {
    add("wrong-output", "pair output validation flags are not both true");
  }
  for (const issue of (Array.isArray(row.preflight_issues) ? row.preflight_issues : [])) {
    add("unavailable", `pair preflight failed: ${issue}`);
  }

  const context = receipt?.measurement_context;
  const warmupOrders = Array.isArray(row.warmup_orders) ? row.warmup_orders : (Array.isArray(receipt?.warmup_orders) ? receipt.warmup_orders : []);
  const warmupRows = Array.isArray(row.warmups) ? row.warmups : [];
  if (warmupOrders.length !== WARMUPS ||
      warmupOrders.some((order) => !ORDER_LABELS.includes(order)) ||
      warmupOrders.filter((order) => order === "surface_first").length !== WARMUPS / 2 ||
      warmupRows.length !== WARMUPS) {
    add("missing-sample", "warmup order or sample vector is missing or not balanced");
  }
  for (let index = 0; index < WARMUPS; index += 1) {
    const warmup = warmupRows[index];
    if (!isObject(warmup) || warmup.order !== warmupOrders[index]) {
      add("unpaired-row", `warmup row ${index} does not match its declared order`);
      continue;
    }
    const surfaceSample = armSample(warmup, "surface");
    const plainSample = armSample(warmup, "plain");
    for (const issue of sampleIssues(surfaceSample, EXPECTED_OUTPUT, `warmup ${index} surface`)) add(issue.kind, issue.cause);
    for (const issue of sampleIssues(plainSample, EXPECTED_OUTPUT, `warmup ${index} plain`)) add(issue.kind, issue.cause);
    for (const issue of sampleBindingIssues(surfaceSample, SURFACE_SOURCE, context, `warmup ${index} surface`)) add(issue.kind, issue.cause);
    for (const issue of sampleBindingIssues(plainSample, PLAIN_SOURCE, context, `warmup ${index} plain`)) add(issue.kind, issue.cause);
  }

  const trialRows = pairRowsFromReceipt(receipt, row);
  const trialOrders = Array.isArray(row.trial_orders) ? row.trial_orders : (Array.isArray(receipt?.trial_orders) ? receipt.trial_orders : []);
  if (trialRows.length !== TRIALS || trialOrders.length !== TRIALS) {
    add("missing-sample", `receipt must contain exactly ${TRIALS} matched trial rows and order labels`);
  }
  const surfaceVector = Array.isArray(row.surface_trials_ms) ? row.surface_trials_ms : [];
  const plainVector = Array.isArray(row.plain_trials_ms) ? row.plain_trials_ms : [];
  const ratioVector = Array.isArray(row.paired_ratios) ? row.paired_ratios : (Array.isArray(row.ratios) ? row.ratios : []);
  const differenceVector = Array.isArray(row.paired_differences_ms) ? row.paired_differences_ms : (Array.isArray(row.differences_ms) ? row.differences_ms : []);
  if (surfaceVector.length !== TRIALS || plainVector.length !== TRIALS) add("missing-sample", "raw surface/plain vectors do not contain all 100 samples");
  if (ratioVector.length !== TRIALS || differenceVector.length !== TRIALS) add("missing-sample", "paired ratio/difference vectors do not contain all 100 samples");
  const orders = [];
  const surfaceSamples = [];
  const plainSamples = [];
  for (let index = 0; index < TRIALS; index += 1) {
    const trial = trialRows[index];
    if (!isObject(trial)) {
      add("unpaired-row", `trial row ${index} is missing`);
      continue;
    }
    if (trial.index !== undefined && trial.index !== index) add("unpaired-row", `trial row ${index} has stale index ${trial.index}`);
    if (trial.paired !== true) add("unpaired-row", `trial row ${index} is not marked paired`);
    const order = trial.order;
    orders.push(order);
    if (!ORDER_LABELS.includes(order) || order !== trialOrders[index]) add("unpaired-row", `trial row ${index} has no valid shared order label`);
    const surfaceSample = armSample(trial, "surface");
    const plainSample = armSample(trial, "plain");
    if (!surfaceSample || !plainSample) {
      add("unpaired-row", `trial row ${index} does not contain both surface and plain arms`);
      continue;
    }
    const surfaceTime = sampleTime(surfaceSample);
    const plainTime = sampleTime(plainSample);
    if (finite(surfaceTime)) surfaceSamples.push(surfaceTime);
    if (finite(plainTime)) plainSamples.push(plainTime);
    for (const issue of sampleIssues(surfaceSample, EXPECTED_OUTPUT, `trial ${index} surface`)) add(issue.kind, issue.cause);
    for (const issue of sampleIssues(plainSample, EXPECTED_OUTPUT, `trial ${index} plain`)) add(issue.kind, issue.cause);
    for (const issue of sampleBindingIssues(surfaceSample, SURFACE_SOURCE, context, `trial ${index} surface`)) add(issue.kind, issue.cause);
    for (const issue of sampleBindingIssues(plainSample, PLAIN_SOURCE, context, `trial ${index} plain`)) add(issue.kind, issue.cause);
    if (finite(surfaceTime) && finite(plainTime) && (!finite(trial.ratio) || !closeEnough(trial.ratio, surfaceTime / plainTime))) {
      add("wrong-ratio", `trial ${index} ratio is not surface/plain`);
    }
    if (finite(surfaceTime) && finite(plainTime) && (!finite(trial.difference_ms) || !closeEnough(trial.difference_ms, surfaceTime - plainTime))) {
      add("wrong-ratio", `trial ${index} difference is not surface minus plain`);
    }
  }
  if (orders.length !== TRIALS || orders.filter((order) => order === "surface_first").length !== TRIALS / 2 ||
      canonical(orders) !== canonical(trialOrders)) {
    add("unpaired-row", "trial order labels are missing or not balanced");
  }
  if (surfaceSamples.length !== TRIALS || plainSamples.length !== TRIALS) add("missing-sample", "one or more trial arms has no usable timing sample");

  const confidence = confidenceFrom(row, receipt);
  if (!isObject(confidence)) {
    add("missing-confidence", "paired-bootstrap confidence evidence is missing");
  } else {
    if (confidence.method !== "paired_index_bootstrap" || confidence.resamples !== BOOTSTRAP_RESAMPLES || confidence.seed !== BOOTSTRAP_SEED
      || confidence.quantile !== "nearest_rank_one_based" || confidence.lower_rank !== BOOTSTRAP_LOWER_RANK || confidence.upper_rank !== BOOTSTRAP_UPPER_RANK) {
      add("missing-confidence", "paired-bootstrap confidence method, seed, resamples, or ranks differ from the predeclared contract");
    }
    if (!closeEnough(confidence.confidence, BOOTSTRAP_LEVEL)) add("missing-confidence", "paired-bootstrap confidence level is missing or not 95%");
    if (!finite(confidence.ratio_lower95) || !finite(confidence.ratio_upper95)
      || !finite(confidence.difference_lower95) || !finite(confidence.difference_upper95)) {
      add("missing-confidence", "paired-bootstrap ratio/difference bounds are missing");
    }
    if (finite(confidence.ratio_lower95) && finite(confidence.ratio_upper95) && confidence.ratio_lower95 > confidence.ratio_upper95) {
      add("missing-confidence", "paired-bootstrap ratio bounds are reversed");
    }
    if (finite(confidence.ratio_upper95) && confidence.ratio_upper95 >= 1) add("overlapping-confidence", "paired-bootstrap ratio upper 95% bound overlaps or reaches 1");
  }

  let statistics = null;
  if (surfaceSamples.length === TRIALS && plainSamples.length === TRIALS) {
    try {
      statistics = computeStatistics(surfaceSamples, plainSamples, {
        resamples: confidence?.resamples ?? BOOTSTRAP_RESAMPLES,
        seed: confidence?.seed ?? BOOTSTRAP_SEED,
        lowerRank: confidence?.lower_rank ?? BOOTSTRAP_LOWER_RANK,
        upperRank: confidence?.upper_rank ?? BOOTSTRAP_UPPER_RANK,
      });
      const declaredSurface = row.surface_wall_time_ms ?? row.statistics?.surface_median_ms;
      const declaredPlain = row.plain_wall_time_ms ?? row.statistics?.plain_median_ms;
      const declaredRatio = row.ratio ?? row.statistics?.ratio;
      if (!finite(declaredSurface) || !closeEnough(declaredSurface, statistics.surface_median_ms)) add("wrong-ratio", "reported surface median differs from all raw samples");
      if (!finite(declaredPlain) || !closeEnough(declaredPlain, statistics.plain_median_ms)) add("wrong-ratio", "reported plain median differs from all raw samples");
      if (!finite(declaredRatio) || !closeEnough(declaredRatio, statistics.ratio)) add("wrong-ratio", "reported ratio is not median(surface)/median(plain)");
      if (surfaceVector.length === TRIALS) surfaceVector.forEach((value, index) => {
        if (!finite(value) || !closeEnough(value, surfaceSamples[index])) add("wrong-ratio", `surface timing vector entry ${index} is stale`);
      });
      if (plainVector.length === TRIALS) plainVector.forEach((value, index) => {
        if (!finite(value) || !closeEnough(value, plainSamples[index])) add("wrong-ratio", `plain timing vector entry ${index} is stale`);
      });
      if (ratioVector.length === TRIALS) ratioVector.forEach((value, index) => {
        if (!finite(value) || !closeEnough(value, statistics.paired_ratios[index])) add("wrong-ratio", `paired ratio vector entry ${index} is stale`);
      });
      if (differenceVector.length === TRIALS) differenceVector.forEach((value, index) => {
        if (!finite(value) || !closeEnough(value, statistics.paired_differences_ms[index])) add("wrong-ratio", `paired difference vector entry ${index} is stale`);
      });
      const computedConfidence = statistics.bootstrap;
      for (const [field, label] of [
        ["ratio_lower95", "ratio lower bound"],
        ["ratio_upper95", "ratio upper bound"],
        ["difference_lower95", "difference lower bound"],
        ["difference_upper95", "difference upper bound"],
      ]) {
        if (!isObject(confidence) || !closeEnough(confidence[field], computedConfidence[field])) {
          add("wrong-confidence", `reported paired-bootstrap ${label} differs from all raw samples`);
        }
      }
      for (const field of ["ratio_bootstrap_sha256", "difference_bootstrap_sha256"]) {
        if (!isObject(confidence) || confidence[field] !== computedConfidence[field]) {
          add("wrong-confidence", `reported paired-bootstrap ${field} differs from all raw samples`);
        }
      }
    } catch (error) {
      add("missing-sample", `statistics could not be computed: ${error.message}`);
    }
  }

  const ratio = finite(row.ratio) ? row.ratio : statistics?.ratio;
  const computedVerdict = classifyVerdict(ratio, confidence);
  if (!["win", "loss", "inconclusive"].includes(row.verdict)) {
    add("missing-verdict", "pair verdict is missing or invalid");
  } else if (row.verdict !== computedVerdict) {
    add("wrong-verdict", `declared ${row.verdict} but strict comparator computes ${computedVerdict}`);
  }
  if (computedVerdict === "loss") add("loss", "strict lower-is-better comparator does not prove a surface win");
  if (computedVerdict === "inconclusive") add("inconclusive", "strict paired confidence interval does not prove a surface win");
  if (computedVerdict === "unavailable") add("unavailable", "strict paired comparator is unavailable");
  const valid = findings.length === 0 && computedVerdict === "win";
  return {
    schema: receipt?.schema ?? null,
    version: receipt?.version ?? null,
    status: valid ? "pass" : "fail",
    valid,
    verdict: computedVerdict,
    findings,
    pair: row,
    statistics,
  };
}

export const validateReceipt = validatePerformanceSurfaceReceipt;

export function validateManifestPair(pairSpec, evidenceSpec = null, measurementSpec = null) {
  const issues = [];
  if (!isObject(pairSpec) || pairSpec.id !== PAIR_ID) issues.push("manifest pair id is stale");
  if (pairSpec?.surface !== SURFACE_LABEL || pairSpec?.plain !== PLAIN_LABEL) issues.push("manifest pair labels are stale");
  if (pairSpec?.surface_program !== SURFACE_PROGRAM || pairSpec?.plain_program !== PLAIN_PROGRAM) issues.push("manifest program identities are stale");
  if (pairSpec?.surface_source !== SURFACE_SOURCE || pairSpec?.plain_source !== PLAIN_SOURCE) issues.push("manifest source spellings are stale");
  if (pairSpec?.input_range !== INPUT_RANGE || pairSpec?.expected_output !== EXPECTED_OUTPUT) issues.push("manifest range or output oracle is stale");
  if (!isObject(evidenceSpec) || evidenceSpec.target !== TARGET || evidenceSpec.metric !== METRIC) issues.push("manifest target or metric is stale");
  if (evidenceSpec?.warmups !== WARMUPS || evidenceSpec?.trials !== TRIALS) issues.push("manifest warmup/trial counts are stale");
  if (evidenceSpec?.output_oracle !== EXPECTED_OUTPUT) issues.push("manifest output oracle is stale");
  if (!isObject(measurementSpec) ||
      canonical(measurementSpec.inputs) !== canonical(PAIR_MEASUREMENT.inputs) ||
      measurementSpec.tier !== PAIR_MEASUREMENT.tier ||
      measurementSpec.target !== PAIR_MEASUREMENT.target ||
      measurementSpec.artifact_binding !== PAIR_MEASUREMENT.artifact_binding ||
      measurementSpec.environment_binding !== PAIR_MEASUREMENT.environment_binding ||
      measurementSpec.sampling !== PAIR_MEASUREMENT.sampling ||
      measurementSpec.output_validation !== PAIR_MEASUREMENT.output_validation) {
    issues.push("manifest pair measurement contract does not bind shared inputs, tier, artifact, environment, sampling, and exact output validation");
  }
  return issues;
}

function readTextCommand(command, args, cwd) {
  try {
    return execFileSync(command, args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return null;
  }
}

function cpuAffinity() {
  const configured = process.env.JET_CPU_AFFINITY?.trim();
  if (configured) return { value: configured, source: "JET_CPU_AFFINITY" };
  try {
    const taskset = readTextCommand("taskset", ["-pc", String(process.pid)], repoDir);
    const match = taskset?.match(/:\s*(.+)$/);
    if (match?.[1]) return { value: match[1].trim(), source: "taskset" };
  } catch {
    // Fall through to Linux's process status.
  }
  try {
    const status = readFileSync(`/proc/${process.pid}/status`, "utf8");
    const match = status.match(/^Cpus_allowed_list:\s*(.+)$/m);
    if (match?.[1]) return { value: match[1].trim(), source: "/proc/status" };
  } catch {
    // Host identity remains incomplete and validation fails closed.
  }
  return { value: null, source: null };
}

export function collectIdentity(options = {}) {
  const artifactPath = path.resolve(options.artifactPath ?? path.join(repoDir, "target/debug/jet"));
  let artifact = null;
  if (existsSync(artifactPath)) {
    const bytes = readFileSync(artifactPath);
    const stat = statSync(artifactPath);
    artifact = {
      path: path.relative(repoDir, artifactPath).split(path.sep).join("/"),
      sha256: sha256(bytes),
      md5: md5(bytes),
      bytes: bytes.length,
      mtime_ms: stat.mtimeMs,
    };
  }
  const affinity = cpuAffinity();
  const toolchain = {
    rustc: readTextCommand(process.env.RUSTC ?? "rustc", ["--version"], repoDir),
    cargo: readTextCommand(process.env.CARGO ?? "cargo", ["--version"], repoDir),
    node: readTextCommand(process.execPath, ["--version"], repoDir),
  };
  return {
    artifact,
    revision: readTextCommand("git", ["rev-parse", "HEAD"], repoDir),
    toolchain,
    host: {
      hostname: os.hostname(),
      platform: os.platform(),
      release: os.release(),
      arch: os.arch(),
      cpu_count: os.cpus().length,
      cpu_model: os.cpus()[0]?.model ?? null,
      cpu_affinity: affinity.value,
      cpu_affinity_source: affinity.source,
    },
  };
}

function runArm({ binary, source, expectedOutput, scratchRoot, label, timeoutMs = DEFAULT_TIMEOUT_MS, context = null }) {
  let runRoot = null;
  const started = performance.now();
  let result = null;
  let error = null;
  try {
    runRoot = mkdtempSync(path.join(scratchRoot, `${label}-`));
    writeFileSync(path.join(runRoot, "main.jet"), source, "utf8");
    result = spawnSync(binary, ["run", "--interpret", "main.jet"], {
      cwd: runRoot,
      env: { ...process.env, JET_TEST_SCRATCH_DIR: scratchRoot },
      timeout: timeoutMs,
      encoding: null,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (caught) {
    error = caught;
  }
  const elapsed = performance.now() - started;
  const stdoutBuffer = Buffer.isBuffer(result?.stdout) ? result.stdout : Buffer.from(result?.stdout ?? "");
  const stderrBuffer = Buffer.isBuffer(result?.stderr) ? result.stderr : Buffer.from(result?.stderr ?? "");
  const status = result?.status ?? null;
  const signal = result?.signal ?? null;
  const errorCode = error?.code ?? result?.error?.code ?? null;
  const timedOut = errorCode === "ETIMEDOUT";
  let directoryEntries = [];
  if (runRoot) {
    try { directoryEntries = readdirSync(runRoot); } catch { directoryEntries = []; }
  }
  const contaminated = directoryEntries.some((entry) => entry !== "main.jet");
  const outputMatches = status === 0 && signal == null && !timedOut && stdoutBuffer.equals(Buffer.from(expectedOutput, "utf8"));
  const valid = outputMatches && !error && status === 0 && signal == null && !timedOut && !contaminated;
  const sample = {
    label,
    source_file: "main.jet",
    source_sha256: sha256(source),
    pair_context_sha256: context?.context_sha256 ?? null,
    artifact_sha256: context?.artifact_sha256 ?? null,
    environment_sha256: context?.environment_sha256 ?? null,
    tier: context?.tier ?? PAIR_MEASUREMENT.tier,
    target: context?.target ?? TARGET,
    input_range: context?.inputs?.range ?? INPUT_RANGE,
    output_oracle: context?.output_validation?.oracle ?? expectedOutput,
    wall_time_ms: valid ? roundMs(elapsed) : null,
    elapsed_ms: roundMs(elapsed),
    status,
    exit_code: status,
    signal,
    timed_out: timedOut,
    error_code: errorCode,
    stdout: stdoutBuffer.toString("utf8"),
    stderr: stderrBuffer.toString("utf8"),
    stdout_bytes_base64: stdoutBuffer.toString("base64"),
    stderr_bytes_base64: stderrBuffer.toString("base64"),
    stdout_sha256: sha256(stdoutBuffer),
    stderr_sha256: sha256(stderrBuffer),
    output_verified: outputMatches,
    output_matches: outputMatches,
    contaminated,
    directory_entries: directoryEntries,
  };
  if (runRoot) rmSync(runRoot, { recursive: true, force: true });
  return sample;
}

function emptySample(label, source, context = null) {
  return {
    label,
    source_file: "main.jet",
    source_sha256: sha256(source),
    pair_context_sha256: context?.context_sha256 ?? null,
    artifact_sha256: context?.artifact_sha256 ?? null,
    environment_sha256: context?.environment_sha256 ?? null,
    tier: context?.tier ?? PAIR_MEASUREMENT.tier,
    target: context?.target ?? TARGET,
    input_range: context?.inputs?.range ?? INPUT_RANGE,
    output_oracle: context?.output_validation?.oracle ?? EXPECTED_OUTPUT,
    wall_time_ms: null,
    elapsed_ms: null,
    status: null,
    exit_code: null,
    signal: null,
    timed_out: false,
    error_code: "preflight",
    stdout: "",
    stderr: "",
    stdout_bytes_base64: "",
    stderr_bytes_base64: "",
    stdout_sha256: sha256(Buffer.alloc(0)),
    stderr_sha256: sha256(Buffer.alloc(0)),
    output_verified: false,
    output_matches: false,
    contaminated: false,
    directory_entries: [],
  };
}

function makeTrial(index, order, surface, plain) {
  const surfaceTime = sampleTime(surface);
  const plainTime = sampleTime(plain);
  return {
    index,
    order,
    paired: true,
    surface,
    plain,
    surface_wall_time_ms: surfaceTime,
    plain_wall_time_ms: plainTime,
    ratio: finite(surfaceTime) && finite(plainTime) && plainTime > 0 ? surfaceTime / plainTime : null,
    difference_ms: finite(surfaceTime) && finite(plainTime) ? surfaceTime - plainTime : null,
  };
}

function measurementPairRow({ identity, sources, context, warmupOrders, warmupRows, trialOrders, trialRows, statistics, preflightIssues }) {
  const surfaceWarmup = warmupRows.map((row) => sampleTime(row.surface));
  const plainWarmup = warmupRows.map((row) => sampleTime(row.plain));
  const surfaceTrials = trialRows.map((row) => row.surface_wall_time_ms);
  const plainTrials = trialRows.map((row) => row.plain_wall_time_ms);
  const allSamples = [...warmupRows, ...trialRows];
  const allOutputsMatch = allSamples.every((row) =>
    sampleOutputMatches(row.surface, EXPECTED_OUTPUT) && sampleOutputMatches(row.plain, EXPECTED_OUTPUT));
  const allSamplesValid = allSamples.every((row) =>
    sampleIssues(row.surface, EXPECTED_OUTPUT, "surface").length === 0 &&
    sampleIssues(row.plain, EXPECTED_OUTPUT, "plain").length === 0);
  const confidence = statistics?.bootstrap ?? null;
  const ratio = statistics?.ratio ?? null;
  const verdict = classifyVerdict(ratio, confidence);
  const validTiming = surfaceWarmup.length === WARMUPS && plainWarmup.length === WARMUPS &&
    surfaceWarmup.every((value) => finite(value) && value > 0) &&
    plainWarmup.every((value) => finite(value) && value > 0) &&
    surfaceTrials.length === TRIALS && plainTrials.length === TRIALS &&
    surfaceTrials.every((value) => finite(value) && value > 0) &&
    plainTrials.every((value) => finite(value) && value > 0);
  return {
    id: PAIR_ID,
    canonical: SURFACE_LABEL,
    alternate: PLAIN_LABEL,
    surface: SURFACE_LABEL,
    plain: PLAIN_LABEL,
    workload: WORKLOAD,
    surface_program: SURFACE_PROGRAM,
    plain_program: PLAIN_PROGRAM,
    expected_output: EXPECTED_OUTPUT,
    surface_source: sources.surface,
    plain_source: sources.plain,
    surface_source_sha256: sha256(sources.surface),
    plain_source_sha256: sha256(sources.plain),
    artifact: identity.artifact?.path ?? "target/debug/jet",
    artifact_sha256: identity.artifact?.sha256 ?? null,
    revision: identity.revision ?? null,
    measurement_context: context,
    measurement_context_sha256: context?.context_sha256 ?? null,
    environment_sha256: context?.environment_sha256 ?? null,
    tier: context?.tier ?? PAIR_MEASUREMENT.tier,
    target: context?.target ?? TARGET,
    input_range: context?.inputs?.range ?? INPUT_RANGE,
    timing_status: validTiming && allSamplesValid && allOutputsMatch && preflightIssues.length === 0 ? "measured" : "unavailable",
    surface_output_matches: allSamples.every((row) => sampleOutputMatches(row.surface, EXPECTED_OUTPUT)),
    plain_output_matches: allSamples.every((row) => sampleOutputMatches(row.plain, EXPECTED_OUTPUT)),
    warmup_orders: warmupOrders,
    trial_orders: trialOrders,
    warmups: warmupRows,
    trials: trialRows,
    trial_pairs: trialRows,
    surface_warmup_ms: surfaceWarmup,
    plain_warmup_ms: plainWarmup,
    surface_trials_ms: surfaceTrials,
    plain_trials_ms: plainTrials,
    paired_ratios: statistics?.paired_ratios ?? trialRows.map((row) => row.ratio),
    paired_differences_ms: statistics?.paired_differences_ms ?? trialRows.map((row) => row.difference_ms),
    surface_wall_time_ms: statistics?.surface_median_ms ?? null,
    plain_wall_time_ms: statistics?.plain_median_ms ?? null,
    ratio,
    statistics: statistics ? {
      surface_median_ms: statistics.surface_median_ms,
      plain_median_ms: statistics.plain_median_ms,
      ratio: statistics.ratio,
      paired_ratios: statistics.paired_ratios,
      paired_differences_ms: statistics.paired_differences_ms,
    } : null,
    confidence,
    bootstrap: confidence,
    verdict,
    verdict_reason: verdictReason(verdict, confidence),
    preflight_issues: preflightIssues,
    timing_references: [],
  };
}

export function measurePair(options = {}) {
  const root = path.resolve(options.repoDir ?? repoDir);
  const artifactPath = path.resolve(options.artifactPath ?? path.join(root, "target/debug/jet"));
  const scratchRoot = path.resolve(options.scratchRoot ?? process.env.JET_TEST_SCRATCH_DIR ?? path.join(os.homedir(), ".cache", "jet-dev", "perf-surface-pair"), "performance-surface");
  mkdirSync(scratchRoot, { recursive: true });
  const sources = { surface: SURFACE_SOURCE, plain: PLAIN_SOURCE };
  const identity = options.identity ?? collectIdentity({ artifactPath });
  const manifestIssues = options.manifest
    ? validateManifestPair(
      options.manifest.performance_surface_pairs?.pairs?.find((pair) => pair.id === PAIR_ID),
      options.manifest.performance_surface_pairs?.evidence,
      options.manifest.performance_surface_pairs?.pair_measurement,
    )
    : [];
  const preflightIssues = [...manifestIssues];
  if (!identity.artifact) preflightIssues.push("compiler artifact is missing");
  if (!identity.revision) preflightIssues.push("compiler revision is missing");
  if (!identity.toolchain?.rustc || !identity.toolchain?.node) preflightIssues.push("toolchain identity is missing");
  if (!identity.host?.cpu_affinity) preflightIssues.push("CPU affinity is missing");
  const warmupOrders = options.warmupOrders ?? balancedOrder(WARMUPS, ORDER_SEED);
  const trialOrders = options.trialOrders ?? balancedOrder(TRIALS, ORDER_SEED + 1);
  const context = makeMeasurementContext(identity, warmupOrders, trialOrders);
  const executable = identity.artifact?.path ? path.resolve(root, identity.artifact.path) : artifactPath;
  const run = options.runArm ?? runArm;
  const canRun = preflightIssues.length === 0 && existsSync(executable);
  const warmupRows = [];
  const trialRows = [];
  const invoke = (source, label) => canRun
    ? run({
      binary: executable,
      source,
      expectedOutput: EXPECTED_OUTPUT,
      scratchRoot,
      label,
      timeoutMs: options.timeoutMs ?? DEFAULT_TIMEOUT_MS,
      context,
    })
    : emptySample(label, source, context);
  try {
    if (canRun) {
      // Keep the warmups balanced and in the same deterministic arm order contract as trials.
      for (let index = 0; index < WARMUPS; index += 1) {
        const order = warmupOrders[index];
        const surface = order === "surface_first" ? invoke(sources.surface, `warmup-${index}-surface`) : null;
        const plain = order === "surface_first" ? invoke(sources.plain, `warmup-${index}-plain`) : null;
        const plainFirst = order === "plain_first" ? invoke(sources.plain, `warmup-${index}-plain`) : null;
        const surfaceSecond = order === "plain_first" ? invoke(sources.surface, `warmup-${index}-surface`) : null;
        warmupRows.push({ index, order, surface: surface ?? surfaceSecond, plain: plain ?? plainFirst });
      }
      for (let index = 0; index < TRIALS; index += 1) {
        const order = trialOrders[index];
        const surface = order === "surface_first" ? invoke(sources.surface, `trial-${index}-surface`) : null;
        const plain = order === "surface_first" ? invoke(sources.plain, `trial-${index}-plain`) : null;
        const plainFirst = order === "plain_first" ? invoke(sources.plain, `trial-${index}-plain`) : null;
        const surfaceSecond = order === "plain_first" ? invoke(sources.surface, `trial-${index}-surface`) : null;
        trialRows.push(makeTrial(index, order, surface ?? surfaceSecond, plain ?? plainFirst));
      }
    }
  } catch (error) {
    preflightIssues.push(`measurement runner failed: ${error.message}`);
  }
  if (!canRun) {
    for (let index = 0; index < WARMUPS; index += 1) {
      warmupRows.push({
        index,
        order: warmupOrders[index],
        surface: emptySample(`warmup-${index}-surface`, sources.surface, context),
        plain: emptySample(`warmup-${index}-plain`, sources.plain, context),
      });
    }
    for (let index = 0; index < TRIALS; index += 1) {
      trialRows.push(makeTrial(
        index,
        trialOrders[index],
        emptySample(`trial-${index}-surface`, sources.surface, context),
        emptySample(`trial-${index}-plain`, sources.plain, context),
      ));
    }
  }
  const surfaceTrials = trialRows.map((row) => row.surface_wall_time_ms).filter((value) => finite(value) && value > 0);
  const plainTrials = trialRows.map((row) => row.plain_wall_time_ms).filter((value) => finite(value) && value > 0);
  let statistics = null;
  if (surfaceTrials.length === TRIALS && plainTrials.length === TRIALS) {
    try { statistics = computeStatistics(surfaceTrials, plainTrials); } catch (error) { preflightIssues.push(`statistics failed: ${error.message}`); }
  }
  const artifactAfter = identity.artifact
    ? (existsSync(artifactPath) ? collectIdentity({ artifactPath }).artifact : null)
    : null;
  if (!identity.artifact || !artifactAfter) {
    preflightIssues.push("compiler artifact disappeared during measurement");
  } else if (identity.artifact.sha256 !== artifactAfter.sha256) {
    preflightIssues.push("compiler artifact changed during measurement");
  }
  const pair = measurementPairRow({
    identity,
    sources,
    context,
    warmupOrders,
    warmupRows,
    trialOrders,
    trialRows,
    statistics,
    preflightIssues,
  });
  const generatedAt = options.generatedAt ?? new Date().toISOString();
  const receipt = {
    schema: PERFORMANCE_SURFACE_SCHEMA,
    version: PERFORMANCE_SURFACE_VERSION,
    protocol: "paired-surface-v2",
    generated_command: options.generatedCommand ?? `Tools/agent/jet-env node Tools/gauntlet/harness/performance-surface.mjs`,
    generated_at: generatedAt,
    target: TARGET,
    tier: PAIR_MEASUREMENT.tier,
    artifact: identity.artifact?.path ?? "target/debug/jet",
    artifact_sha256: identity.artifact?.sha256 ?? null,
    artifact_md5: identity.artifact?.md5 ?? null,
    artifact_bytes: identity.artifact?.bytes ?? null,
    artifact_mtime_ms: identity.artifact?.mtime_ms ?? null,
    artifact_sha256_after: artifactAfter?.sha256 ?? null,
    revision: identity.revision ?? null,
    build_line: options.buildLine ?? process.env.JET_BUILD_LINE ?? null,
    toolchain: identity.toolchain ?? null,
    host: identity.host ?? null,
    environment_sha256: context.environment_sha256,
    measurement_context: context,
    input: { iterations: ITERATIONS, range: INPUT_RANGE, expected_sum: EXPECTED_SUM },
    setup: SETUP,
    warmups: WARMUPS,
    trials: TRIALS,
    metric: METRIC,
    output_oracle: EXPECTED_OUTPUT,
    order: {
      strategy: "balanced_seeded_fisher_yates",
      labels: ORDER_LABELS,
      seed: ORDER_SEED,
      warmup_seed: ORDER_SEED,
      trial_seed: ORDER_SEED + 1,
      warmups: warmupOrders,
      trials: trialOrders,
    },
    bootstrap: {
      method: "paired_index_bootstrap",
      statistic: "median(surface_wall_time_ms) / median(plain_wall_time_ms)",
      difference_statistic: "median(surface_wall_time_ms - plain_wall_time_ms)",
      confidence: BOOTSTRAP_LEVEL,
      resamples: BOOTSTRAP_RESAMPLES,
      seed: BOOTSTRAP_SEED,
      quantile: "nearest_rank_one_based",
      lower_rank: BOOTSTRAP_LOWER_RANK,
      upper_rank: BOOTSTRAP_UPPER_RANK,
    },
    source_digests: { surface_sha256: sha256(SURFACE_SOURCE), plain_sha256: sha256(PLAIN_SOURCE) },
    historical_receipts: options.historicalReceipts ?? [],
    prior_canonical_receipt: options.priorCanonicalReceipt ?? null,
    pairs: [pair],
    status: pair.verdict === "win" && preflightIssues.length === 0 ? "pass" : "fail",
    verdict: pair.verdict,
    verdict_reason: verdictReason(pair.verdict, pair.confidence),
    preflight_issues: preflightIssues,
  };
  const validation = validatePerformanceSurfaceReceipt(receipt, {
    pairSpec: options.manifest?.performance_surface_pairs?.pairs?.find((candidate) => candidate.id === PAIR_ID),
  });
  receipt.validation = { status: validation.status, valid: validation.valid, findings: validation.findings };
  return { receipt, validation, pair, warmupOrders, trialOrders };
}

function compactTimestamp(iso) {
  return iso.replace(/[-:]/g, "").replace(/\.\d{3}Z$/, "Z");
}

function uniquePath(candidate) {
  if (!existsSync(candidate)) return candidate;
  const extension = path.extname(candidate);
  const stem = candidate.slice(0, -extension.length);
  for (let index = 1; ; index += 1) {
    const next = `${stem}-${index}${extension}`;
    if (!existsSync(next)) return next;
  }
}

function prepareHistory({ canonicalPath, generatedAt, repo }) {
  const history = [];
  let prior = null;
  let issue = null;
  if (existsSync(canonicalPath)) {
    try {
      const bytes = readFileSync(canonicalPath);
      prior = JSON.parse(bytes.toString("utf8"));
      const snapshot = uniquePath(path.join(repo, "Tools", "gauntlet", `performance-surface-evidence-${compactTimestamp(generatedAt)}.json`));
      writeFileSync(snapshot, bytes, { flag: "wx" });
      history.push(path.relative(repo, snapshot).split(path.sep).join("/"));
    } catch (error) {
      issue = `prior canonical receipt could not be preserved: ${error.message}`;
    }
  }
  const priorLinks = Array.isArray(prior?.historical_receipts) ? prior.historical_receipts : HISTORICAL_RECEIPTS;
  for (const link of [...priorLinks, ...HISTORICAL_RECEIPTS]) {
    if (!history.includes(link)) history.push(link);
  }
  const digests = {};
  for (const link of history) {
    const absolute = path.resolve(repo, link);
    if (!existsSync(absolute)) {
      issue = issue ?? `historical receipt is missing: ${link}`;
      continue;
    }
    digests[link] = sha256(readFileSync(absolute));
  }
  return { prior, priorCanonicalReceipt: history[0] ?? null, historicalReceipts: history, historicalReceiptSha256: digests, issue };
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") return { help: true };
    const take = (name) => {
      if (arg === name) return argv[++index];
      if (arg.startsWith(`${name}=`)) return arg.slice(name.length + 1);
      return null;
    };
    const artifact = take("--artifact");
    if (artifact !== null) { options.artifactPath = artifact; continue; }
    const scratch = take("--scratch");
    if (scratch !== null) { options.scratchRoot = scratch; continue; }
    const output = take("--output");
    if (output !== null) { options.outputPath = output; continue; }
    const canonicalPath = take("--canonical");
    if (canonicalPath !== null) { options.canonicalPath = canonicalPath; continue; }
    const timeout = take("--timeout-ms");
    if (timeout !== null) { options.timeoutMs = Number(timeout); continue; }
    throw new Error(`unknown argument ${arg}`);
  }
  return options;
}

const HELP = `Usage: node Tools/gauntlet/harness/performance-surface.mjs [options]

Measure the exact compound_assign_vs_binary_assign interpreter pair.
Options: --artifact PATH, --scratch PATH, --output PATH, --canonical PATH, --timeout-ms N`;

export async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  if (options.help) {
    process.stdout.write(`${HELP}\n`);
    return 0;
  }
  const root = path.resolve(options.repoDir ?? repoDir);
  const canonicalPath = path.resolve(options.canonicalPath ?? path.join(root, "Tools/gauntlet/performance-surface-evidence.json"));
  const generatedAt = new Date().toISOString();
  const history = prepareHistory({ canonicalPath, generatedAt, repo: root });
  const manifestPath = path.join(root, "Tools/gauntlet/measurement-manifest.json");
  let manifest = null;
  try { manifest = JSON.parse(readFileSync(manifestPath, "utf8")); } catch { /* validation records missing manifest identity */ }
  const result = measurePair({
    ...options,
    repoDir: root,
    manifest,
    generatedAt,
    historicalReceipts: history.historicalReceipts,
    priorCanonicalReceipt: history.priorCanonicalReceipt,
    generatedCommand: process.argv.map((value) => JSON.stringify(value)).join(" "),
  });
  if (history.issue) result.receipt.preflight_issues.push(history.issue);
  result.receipt.historical_receipts = history.historicalReceipts;
  result.receipt.historical_receipt_sha256 = history.historicalReceiptSha256;
  result.receipt.prior_canonical_receipt = history.priorCanonicalReceipt;
  result.receipt.validation = validatePerformanceSurfaceReceipt(result.receipt, { pairSpec: manifest?.performance_surface_pairs?.pairs?.find((pair) => pair.id === PAIR_ID) });
  const outputPath = path.resolve(options.outputPath ?? path.join(root, "Tools", "gauntlet", `performance-surface-evidence-${compactTimestamp(generatedAt)}.json`));
  const text = `${JSON.stringify(result.receipt, null, 2)}\n`;
  writeFileSync(uniquePath(outputPath), text, "utf8");
  writeFileSync(canonicalPath, text, "utf8");
  process.stdout.write(`performance surface measured: ${PAIR_ID} verdict=${result.receipt.verdict} validation=${result.receipt.validation.status}\n`);
  return result.receipt.validation.valid ? 0 : 1;
}

export const performanceSurfaceInternals = Object.freeze({
  balancedOrder,
  computeStatistics,
  classifyVerdict,
  collectIdentity,
  makeTrial,
  measurePair,
  pairedBootstrap,
  measurementContextDigest,
  environmentDigest,
  validateManifestPair,
  validatePerformanceSurfaceReceipt,
  PAIR_MEASUREMENT,
});

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((code) => { process.exitCode = code; }).catch((error) => {
    console.error(`performance surface: ${error.message}`);
    process.exitCode = 1;
  });
}
