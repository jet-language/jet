#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../..");
const defaultManifestPath = path.join(repoDir, "gauntlet/measurement-manifest.json");
const gateSchema = "jet.gauntlet.integrated-gate.v1";
const statusContract = "gauntlet-status-v1";
const reportContract = "gauntlet-report-v1";
const supportedTiers = Object.freeze(["aot", "run"]);
const supportedBackends = Object.freeze(["aot_rust", "interpreter", "cranelift", "web"]);
const resultStatuses = new Set(["ok", "measured", "pass"]);
const knownFailureKinds = new Set([
  "missing-input", "malformed-input", "manifest", "missing-cell", "unexpected-cell",
  "cell-mismatch", "missing-entry", "entry-mismatch", "missing-peer", "unexpected-peer",
  "missing-metric", "unexpected-metric", "missing-tier", "unexpected-tier", "wrong-status",
  "wrong-verdict", "wrong-ratio", "loss", "unavailable", "inconclusive", "wrong-output",
  "missing-evidence", "missing-timestamp", "stale", "future-evidence", "identity-missing",
  "identity-mismatch", "policy-mismatch", "coverage", "publication", "tier-not-covered",
  "tier-count", "baseline", "conformance", "summary-mismatch",
]);

const HELP = `Usage: node gauntlet/harness/integrated-gate.mjs [options]

Read-only integrated evidence gate. It performs no workload, build, or mutation.

Options:
  --manifest PATH       Measurement manifest (default: gauntlet/measurement-manifest.json)
  --now VALUE           Freshness reference, as RFC3339 or Unix milliseconds
  --commit SHA          Expected candidate commit identity
  --json                Print the deterministic JSON gate report
  --check               Check the gate and print a compact human report
  --help                Show this help

With no output option, --check is the default. --json implies --check and uses
exit status 0 only when every required cell, peer, tier, metric, and evidence
source passes. No command in this tool runs a benchmark or writes a file.`;

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const asArray = (value) => Array.isArray(value) ? value : [];
const finite = (value) => typeof value === "number" && Number.isFinite(value);
const nonEmpty = (value) => typeof value === "string" && value.trim().length > 0;
const hasValue = (value) => {
  if (value === null || value === undefined) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  if (isObject(value)) return Object.keys(value).length > 0;
  return true;
};
const canonical = (value) => {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (isObject(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
};

function parseArgs(argv) {
  const options = { manifest: defaultManifestPath, json: false, check: false, help: false, now: null, commit: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      options.help = true;
      continue;
    }
    if (arg === "--json") {
      options.json = true;
      options.check = true;
      continue;
    }
    if (arg === "--check") {
      options.check = true;
      continue;
    }
    const take = (name) => {
      if (arg === name) {
        if (index + 1 >= argv.length) throw new Error(`${name} requires a value`);
        index += 1;
        return argv[index];
      }
      if (arg.startsWith(`${name}=`)) return arg.slice(name.length + 1);
      return null;
    };
    const manifest = take("--manifest");
    if (manifest !== null) {
      options.manifest = manifest;
      continue;
    }
    const now = take("--now");
    if (now !== null) {
      options.now = now;
      continue;
    }
    const commit = take("--commit");
    if (commit !== null) {
      options.commit = commit;
      continue;
    }
    throw new Error(`unknown option: ${arg}`);
  }
  if (!options.help && !options.json && !options.check) options.check = true;
  return options;
}

function usageError(message) {
  const error = new Error(message);
  error.code = "USAGE";
  return error;
}

function parseNow(value) {
  if (value == null) return Date.now();
  if (typeof value === "number" && Number.isFinite(value)) return value < 1e12 ? value * 1000 : value;
  const text = String(value).trim();
  if (/^\d+(?:\.\d+)?$/.test(text)) {
    const number = Number(text);
    return Number.isFinite(number) ? (number < 1e12 ? number * 1000 : number) : null;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
}

function parseTimestamp(value) {
  if (typeof value === "number" && Number.isFinite(value)) return value < 1e12 ? value * 1000 : value;
  if (typeof value !== "string" || value.trim() === "") return null;
  const text = value.trim();
  if (/^\d+(?:\.\d+)?$/.test(text)) {
    const number = Number(text);
    return Number.isFinite(number) ? (number < 1e12 ? number * 1000 : number) : null;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
}

function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

function relativePath(value) {
  if (value == null) return null;
  const text = String(value).replaceAll(path.sep, "/");
  if (!path.isAbsolute(text)) return text;
  const relative = path.relative(repoDir, path.resolve(text));
  if (relative && !relative.startsWith("..") && !path.isAbsolute(relative)) return relative.split(path.sep).join("/");
  return text;
}

function resolvePath(value, base = repoDir) {
  if (typeof value !== "string" || value.trim() === "") return null;
  return path.resolve(base, value);
}

function firstValue(...values) {
  return values.find((value) => value !== undefined && value !== null && value !== "") ?? null;
}

function nestedValue(object, candidates) {
  for (const candidate of candidates) {
    const parts = Array.isArray(candidate) ? candidate : String(candidate).split(".");
    let current = object;
    for (const part of parts) {
      if (!isObject(current) || !Object.hasOwn(current, part)) {
        current = undefined;
        break;
      }
      current = current[part];
    }
    if (current !== undefined && current !== null && current !== "") return current;
  }
  return null;
}

function commitValue(value) {
  if (typeof value !== "string") return null;
  const text = value.trim();
  return /^[0-9a-f]{40}$/i.test(text) ? text.toLowerCase() : null;
}

function isRustPeer(peer) {
  return peer === "rust" || peer.startsWith("rust-");
}

function expectedPairVerdict(peer, ratio, policy) {
  if (!finite(ratio)) return "unmeasured";
  const rust = isRustPeer(peer);
  const rule = rust ? policy?.rust : policy?.non_rust;
  if (!rule || rule.win !== "<1") return "unmeasured";
  if (ratio < 1) return "win";
  if (rust && rule.parity === "<=1.05" && ratio <= 1.05) return "parity";
  return "loss";
}
const verdictRank = Object.freeze({ loss: 0, unmeasured: 1, not_applicable: 2, parity: 3, win: 4 });
function worstVerdict(values) {
  const known = values.filter((value) => Object.hasOwn(verdictRank, value));
  return known.length === 0 ? "unmeasured" : known.reduce((worst, value) => verdictRank[value] < verdictRank[worst] ? value : worst, known[0]);
}


function normalizedStatus(record) {
  if (!isObject(record)) return "missing";
  return typeof record.status === "string" ? record.status : (record.verdict ?? "missing");
}

function metricRecord(peerRow, metric) {
  if (!isObject(peerRow)) return null;
  const metrics = isObject(peerRow.metrics) ? peerRow.metrics : peerRow.metric_comparisons;
  return isObject(metrics?.[metric]) ? metrics[metric] : null;
}

function tierRecord(metric, tier) {
  if (!isObject(metric)) return null;
  if (isObject(metric.tiers?.[tier])) return metric.tiers[tier];
  if (isObject(metric[tier])) return metric[tier];
  return null;
}

function metricApplicability(entry, peer, metric) {
  const byPeer = entry?.metric_applicability?.[peer];
  const fact = isObject(byPeer) && Object.hasOwn(byPeer, metric)
    ? byPeer[metric]
    : isObject(byPeer) && Object.hasOwn(byPeer, "*")
      ? byPeer["*"]
      : entry?.non_applicable?.[peer];
  if (fact === undefined) return { status: "required" };
  if (!isObject(fact) || fact.status !== "not_applicable") return { status: "invalid", reason: "metric applicability must declare not_applicable" };
  return { status: "not_applicable", ...fact };
}

function statusPeers(cell) {
  return asArray(cell?.peers).map((row) => ({ row, peer: firstValue(row?.peer, row?.language) })).filter((item) => nonEmpty(item.peer));
}

function statusMetricNames(peerRow) {
  const metrics = isObject(peerRow?.metrics) ? peerRow.metrics : peerRow?.metric_comparisons;
  return isObject(metrics) ? Object.keys(metrics).sort() : [];
}

function sourceStamp(record, stat) {
  const embedded = firstValue(
    record?.measured_iso,
    record?.measured_at,
    record?.generated,
    record?.generated_at,
    record?.timestamp,
    record?.created_at,
  );
  const parsed = parseTimestamp(embedded);
  if (parsed !== null) return { value: parsed, field: "embedded" };
  if (finite(stat?.mtimeMs)) return { value: stat.mtimeMs, field: "filesystem_mtime" };
  return { value: null, field: null };
}

const surfaceMeasuredStatuses = new Set(["measured", "complete", "verified", "passed", "pass", "ok"]);

function pathValue(object, pathText) {
  if (!nonEmpty(pathText)) return null;
  let current = object;
  for (const part of String(pathText).split(".")) {
    if ((!isObject(current) && !Array.isArray(current)) || !Object.hasOwn(current, part)) return null;
    current = current[part];
  }
  return current;
}

function finiteFrom(...values) {
  return values.find(finite) ?? null;
}

function closeEnough(left, right) {
  return finite(left) && finite(right)
    && Math.abs(left - right) <= Math.max(1e-9, Math.abs(left) * 1e-9, Math.abs(right) * 1e-9);
}

function surfaceEvidenceRows(evidence, evidencePath) {
  const selected = pathValue(evidence, evidencePath);
  if (Array.isArray(selected)) return selected;
  if (isObject(selected) && Array.isArray(selected.rows)) return selected.rows;
  return [];
}

function surfaceFieldValue(record, fields) {
  for (const field of fields) {
    const value = nestedValue(record, [field]);
    if (finite(value)) return value;
  }
  return null;
}

function surfaceMeasuredValues(pair) {
  const measured = isObject(pair?.measured) ? pair.measured : pair;
  return {
    surface: finiteFrom(measured?.surface_wall_time_ms, measured?.candidate_wall_time_ms, measured?.surface_ms),
    plain: finiteFrom(measured?.plain_wall_time_ms, measured?.baseline_wall_time_ms, measured?.plain_ms),
    ratio: finiteFrom(measured?.ratio),
  };
}

function validatePerformanceSurfacePairs(spec) {
  const issues = [];
  if (!isObject(spec)) return ["measurement manifest is missing performance_surface_pairs"];
  if (spec.schema !== "jet.gauntlet.performance-surface-pairs.v1" || spec.version !== 1) {
    issues.push("performance_surface_pairs must declare jet.gauntlet.performance-surface-pairs.v1 version 1");
  }
  if (spec.decision !== "D-PERF-SURFACE-WIN1=A") issues.push("performance_surface_pairs must cite ratified D-PERF-SURFACE-WIN1=A");
  const evidence = spec.evidence;
  if (!isObject(evidence) || !nonEmpty(evidence.file) || !nonEmpty(evidence.path)) {
    issues.push("performance_surface_pairs.evidence.file and evidence.path are required");
  }
  if (!nonEmpty(evidence?.tier) || !nonEmpty(evidence?.metric) || !nonEmpty(evidence?.output_oracle)) {
    issues.push("performance_surface_pairs evidence tier, metric, and output_oracle are required");
  }
  if (!nonEmpty(evidence?.target) || !nonEmpty(evidence?.input) || !nonEmpty(evidence?.setup)
    || !Number.isInteger(evidence?.warmups) || evidence.warmups < 0
    || !Number.isInteger(evidence?.trials) || evidence.trials <= 0) {
    issues.push("performance_surface_pairs evidence must pin target, input, setup, warmups, and positive trials");
  }
  const comparator = spec.comparator;
  if (!isObject(comparator)
    || comparator.direction !== "lower_is_better"
    || comparator.surface_over_plain !== "<1"
    || comparator.equality !== "loss"
    || comparator.missing !== "failure") {
    issues.push("performance_surface_pairs comparator must require surface/plain <1 and fail closed");
  }
  const pairs = asArray(spec.pairs);
  if (pairs.length === 0) issues.push("performance_surface_pairs.pairs must be non-empty");
  const ids = new Set();
  for (const pair of pairs) {
    if (!isObject(pair)) {
      issues.push("performance surface pair rows must be objects");
      continue;
    }
    if (!nonEmpty(pair.id) || ids.has(pair.id)) issues.push(`performance surface pair id ${pair.id ?? "missing"} is missing or duplicated`);
    if (nonEmpty(pair.id)) ids.add(pair.id);
    for (const field of ["surface", "plain", "workload", "surface_program", "plain_program", "evidence_key"]) {
      if (!nonEmpty(pair[field])) issues.push(`performance surface pair ${pair.id ?? "missing"} is missing ${field}`);
    }
    if (pair.surface_program === pair.plain_program) issues.push(`performance surface pair ${pair.id ?? "missing"} reuses one program for both spellings`);
    if (pair.verdict !== undefined && !["win", "loss", "inconclusive"].includes(pair.verdict)) {
      issues.push(`performance surface pair ${pair.id ?? "missing"} has unknown verdict ${pair.verdict}`);
    }
    if (pair.verdict === "inconclusive" && !nonEmpty(pair.verdict_reason)) {
      issues.push(`performance surface pair ${pair.id ?? "missing"} inconclusive verdict needs a reason`);
    }
    const measured = surfaceMeasuredValues(pair);
    if (!finite(measured.surface) || measured.surface <= 0) issues.push(`performance surface pair ${pair.id ?? "missing"} has no positive surface measurement`);
    if (!finite(measured.plain) || measured.plain <= 0) issues.push(`performance surface pair ${pair.id ?? "missing"} has no positive plain measurement`);
    if (!finite(measured.ratio) || measured.ratio <= 0) issues.push(`performance surface pair ${pair.id ?? "missing"} has no positive ratio`);
    if (finite(measured.surface) && finite(measured.plain) && finite(measured.ratio)) {
      const calculated = measured.surface / measured.plain;
      if (!closeEnough(calculated, measured.ratio)) issues.push(`performance surface pair ${pair.id} ratio is not surface/plain`);
      if (calculated >= 1 && !nonEmpty(pair.bug_card)) issues.push(`performance surface pair ${pair.id} is slower without a bug card`);
    }
  }
  return issues;
}

function evaluatePerformanceSurfacePairs(spec, evidence, options = {}) {
  const findings = [];
  const pairs = [];
  const evidenceFile = options.evidence_file ?? spec?.evidence?.file ?? null;
  const add = (kind, pair, cause) => {
    findings.push({
      kind,
      cell: pair?.id ?? null,
      entry: null,
      peer: null,
      tier: spec?.evidence?.tier ?? null,
      metric: spec?.evidence?.metric ?? "wall_time_ms",
      evidence_file: evidenceFile,
      cause,
    });
  };
  for (const issue of validatePerformanceSurfacePairs(spec)) add("malformed-input", null, issue);
  const configuredPairs = asArray(spec?.pairs);
  const rows = surfaceEvidenceRows(evidence, spec?.evidence?.path);
  const byId = new Map(rows.filter((row) => isObject(row) && nonEmpty(row.id)).map((row) => [row.id, row]));
  for (const pair of configuredPairs) {
    if (!isObject(pair) || !nonEmpty(pair.id)) continue;
    const evidenceRow = byId.get(pair.evidence_key ?? pair.id);
    if (!evidenceRow) {
      add("missing-evidence", pair, `performance surface pair ${pair.id} has no evidence row`);
      continue;
    }
    if (nonEmpty(evidenceRow.canonical) && evidenceRow.canonical !== pair.surface) {
      add("identity-mismatch", pair, `performance surface pair ${pair.id} surface label differs from evidence`);
    }
    if (nonEmpty(evidenceRow.alternate) && evidenceRow.alternate !== pair.plain) {
      add("identity-mismatch", pair, `performance surface pair ${pair.id} plain label differs from evidence`);
    }
    for (const field of ["surface_program", "plain_program", "workload"]) {
      if (nonEmpty(evidenceRow[field]) && evidenceRow[field] !== pair[field]) {
        add("identity-mismatch", pair, `performance surface pair ${pair.id} ${field} differs from evidence`);
      }
    }
    const status = firstValue(evidenceRow.timing_status, evidenceRow.status, evidenceRow.verdict);
    if (!surfaceMeasuredStatuses.has(status)) {
      add(status === "unavailable" ? "unavailable" : "inconclusive", pair, `performance surface pair ${pair.id} evidence status is ${status ?? "missing"}`);
    }
    const surfaceFields = [
      pair.surface_field,
      "surface_wall_time_ms",
      "candidate_wall_time_ms",
      "canonical_wall_time_ms",
    ].filter(nonEmpty);
    const plainFields = [
      pair.plain_field,
      "plain_wall_time_ms",
      "baseline_wall_time_ms",
      "alternate_wall_time_ms",
    ].filter(nonEmpty);
    const surface = surfaceFieldValue(evidenceRow, surfaceFields);
    const plain = surfaceFieldValue(evidenceRow, plainFields);
    if (!finite(surface) || surface <= 0 || !finite(plain) || plain <= 0) {
      add("unavailable", pair, `performance surface pair ${pair.id} evidence lacks positive surface and plain measurements`);
      continue;
    }
    const ratio = surface / plain;
    const declared = surfaceMeasuredValues(pair);
    if (finite(declared.surface) && !closeEnough(surface, declared.surface)) {
      add("identity-mismatch", pair, `performance surface pair ${pair.id} surface measurement differs from its checked-in table`);
    }
    if (finite(declared.plain) && !closeEnough(plain, declared.plain)) {
      add("identity-mismatch", pair, `performance surface pair ${pair.id} plain measurement differs from its checked-in table`);
    }
    if (finite(declared.ratio) && !closeEnough(ratio, declared.ratio)) {
      add("wrong-ratio", pair, `performance surface pair ${pair.id} reported ratio ${declared.ratio} is not surface/plain ${ratio}`);
    }
    const reportedRatio = finiteFrom(evidenceRow.ratio);
    if (finite(reportedRatio) && !closeEnough(ratio, reportedRatio)) {
      add("wrong-ratio", pair, `performance surface pair ${pair.id} evidence ratio ${reportedRatio} is not surface/plain ${ratio}`);
    }
    const outputFlags = [
      evidenceRow.output_verified,
      evidenceRow.output_matches,
      evidenceRow.surface_output_matches,
      evidenceRow.candidate_output_matches,
      evidenceRow.plain_output_matches,
    ];
    if (outputFlags.some((value) => value === false)) {
      add("wrong-output", pair, `performance surface pair ${pair.id} output verification failed`);
    }
    const expectedOutput = firstValue(pair.output, pair.expected_output);
    const expectedSurfaceOutput = isObject(expectedOutput) ? firstValue(expectedOutput.surface, expectedOutput.candidate) : expectedOutput;
    const expectedPlainOutput = isObject(expectedOutput) ? firstValue(expectedOutput.plain, expectedOutput.baseline) : expectedOutput;
    for (const [value, expected, label] of [
      [evidenceRow.surface_output, expectedSurfaceOutput, "surface"],
      [evidenceRow.candidate_output, expectedSurfaceOutput, "surface"],
      [evidenceRow.plain_output, expectedPlainOutput, "plain"],
      [evidenceRow.baseline_output, expectedPlainOutput, "plain"],
    ]) {
      if (value !== undefined && expected !== null && value !== expected) add("wrong-output", pair, `performance surface pair ${pair.id} ${label} output differs from the oracle`);
    }
    const computedVerdict = ratio < 1 ? "win" : "loss";
    const verdict = pair.verdict ?? computedVerdict;
    if (verdict !== "inconclusive" && verdict !== computedVerdict) {
      add("wrong-verdict", pair, `performance surface pair ${pair.id} declares ${verdict} for computed ${computedVerdict}`);
    }
    if (verdict === "inconclusive") {
      add("inconclusive", pair, `performance surface pair ${pair.id} is unresolved: ${pair.verdict_reason}`);
    } else if (verdict === "loss") {
      add("loss", pair, `performance surface ${pair.surface} is slower than plain ${pair.plain}: surface/plain ratio ${ratio} must be <1`);
      if (!nonEmpty(pair.bug_card)) add("malformed-input", pair, `performance surface pair ${pair.id} is slower without a bug card`);
    }
    pairs.push({
      id: pair.id,
      surface: pair.surface,
      plain: pair.plain,
      workload: pair.workload,
      surface_program: pair.surface_program,
      plain_program: pair.plain_program,
      tier: spec?.evidence?.tier ?? null,
      metric: spec?.evidence?.metric ?? "wall_time_ms",
      surface_wall_time_ms: surface,
      verdict_reason: pair.verdict_reason ?? null,
      plain_wall_time_ms: plain,
      ratio,
      verdict,
      bug_card: pair.bug_card ?? null,
      output_verified: !outputFlags.some((value) => value === false),
      evidence_file: evidenceFile,
    });
  }
  pairs.sort((left, right) => left.id.localeCompare(right.id));
  return {
    schema: spec?.schema ?? null,
    version: spec?.version ?? null,
    status: findings.length === 0 ? "pass" : "fail",
    pairs,
    findings,
  };
}

function validateManifest(manifest) {
  const issues = [];
  if (!isObject(manifest)) return ["measurement manifest must be a JSON object"];
  if (manifest.version !== 1) issues.push("measurement manifest version must be 1");
  if (!isObject(manifest.report_contract)) issues.push("measurement manifest is missing report_contract");
  const contract = manifest.report_contract;
  if (contract?.id !== reportContract) issues.push(`report_contract.id must be ${reportContract}`);
  if (contract?.scope !== "full_matrix") issues.push("report_contract.scope must be full_matrix");
  if (canonical(contract?.required_jet_tiers) !== canonical(["aot", "run"])) issues.push("report_contract.required_jet_tiers must be [aot, run]");
  if (canonical(contract?.ratio_verdicts?.rust) !== canonical({ win: "<1", parity: "<=1.05", loss: ">1.05" })) issues.push("Rust comparator policy is not the strict <=1.05 policy");
  if (canonical(contract?.ratio_verdicts?.non_rust) !== canonical({ win: "<1", parity: null, loss: ">=1" })) issues.push("non-Rust comparator policy is not the strict <1 policy");
  for (const issue of validatePerformanceSurfacePairs(manifest.performance_surface_pairs)) issues.push(issue);
  const gate = manifest.integrated_gate;
  if (!isObject(gate)) {
    issues.push("measurement manifest is missing integrated_gate");
    return issues;
  }
  if (gate.schema !== gateSchema || gate.version !== 1) issues.push(`integrated_gate must declare ${gateSchema} version 1`);
  const inputs = gate.inputs;
  for (const key of ["matrix", "status", "conformance_inventory", "conformance_summary", "tier_census", "compiler_speed", "entries_dir"]) {
    if (!nonEmpty(inputs?.[key])) issues.push(`integrated_gate.inputs.${key} is required`);
  }
  const freshness = gate.freshness;
  if (!finite(freshness?.max_age_seconds) || freshness.max_age_seconds <= 0) issues.push("integrated_gate.freshness.max_age_seconds must be positive");
  if (!finite(freshness?.future_skew_seconds) || freshness.future_skew_seconds < 0) issues.push("integrated_gate.freshness.future_skew_seconds must be nonnegative");
  const metrics = gate.required_metrics_by_mode;
  if (!isObject(metrics)) issues.push("integrated_gate.required_metrics_by_mode is required");
  for (const mode of Object.keys(contract?.tier_policy_by_mode ?? {})) {
    const list = metrics?.[mode];
    if (!Array.isArray(list) || list.length === 0 || new Set(list).size !== list.length) issues.push(`required metric list for ${mode} is missing or duplicated`);
  }
  if (!Array.isArray(gate.tier_census?.backends) || canonical([...gate.tier_census.backends].sort()) !== canonical([...supportedBackends].sort())) issues.push("tier census backend denominator is incomplete");
  if (!Array.isArray(gate.compiler_speed?.required_fields) || gate.compiler_speed.required_fields.length === 0) issues.push("compiler-speed identity field denominator is missing");
  return issues;
}

async function readFileValue(filePath, parseJson = true) {
  if (!filePath) return { value: null, stat: null, text: null, error: "path is not configured" };
  try {
    const [text, stat] = await Promise.all([fs.readFile(filePath, "utf8"), fs.stat(filePath)]);
    if (!parseJson) return { value: text, stat, text, error: null };
    try {
      return { value: JSON.parse(text), stat, text, error: null };
    } catch (error) {
      return { value: null, stat, text, error: `invalid JSON: ${error.message}` };
    }
  } catch (error) {
    return { value: null, stat: null, text: null, error: error.code === "ENOENT" ? "file does not exist" : error.message };
  }
}

async function loadInputs(options = {}) {
  const manifestPath = resolvePath(options.manifest ?? defaultManifestPath, repoDir);
  const manifestResult = await readFileValue(manifestPath, true);
  const manifest = manifestResult.value;
  const gate = isObject(manifest?.integrated_gate) ? manifest.integrated_gate : {};
  const configured = isObject(gate.inputs) ? gate.inputs : {};
  const surfaceSpec = isObject(manifest?.performance_surface_pairs) ? manifest.performance_surface_pairs : null;
  const pathFor = (key) => resolvePath(configured[key], repoDir);
  const surfaceEvidencePath = resolvePath(surfaceSpec?.evidence?.file, repoDir);
  const paths = {
    manifest: manifestPath,
    matrix: pathFor("matrix"),
    status: pathFor("status"),
    conformance_inventory: pathFor("conformance_inventory"),
    conformance_summary: pathFor("conformance_summary"),
    tier_census: pathFor("tier_census"),
    compiler_speed: pathFor("compiler_speed"),
    entries_dir: pathFor("entries_dir"),
    performance_surface_evidence: surfaceEvidencePath,
  };
  const [matrix, status, conformanceInventory, conformanceSummary, tierCensus, compilerSpeed, performanceSurfaceEvidence] = await Promise.all([
    readFileValue(paths.matrix, true),
    readFileValue(paths.status, true),
    readFileValue(paths.conformance_inventory, true),
    readFileValue(paths.conformance_summary, false),
    readFileValue(paths.tier_census, true),
    readFileValue(paths.compiler_speed, true),
    surfaceSpec
      ? readFileValue(paths.performance_surface_evidence, true)
      : Promise.resolve({ value: null, stat: null, text: null, error: null }),
  ]);
  const entries = new Map();
  const entryRows = asArray(manifest?.entries);
  await Promise.all(entryRows.map(async (row) => {
    if (!nonEmpty(row?.name) || !paths.entries_dir) return;
    const entryPath = path.join(paths.entries_dir, row.name, "entry.json");
    entries.set(row.name, { path: entryPath, ...(await readFileValue(entryPath, true)) });
  }));
  return {
    manifest,
    manifestResult,
    paths,
    matrix: matrix.value,
    status: status.value,
    conformanceInventory: conformanceInventory.value,
    conformanceSummary: conformanceSummary.value,
    tierCensus: tierCensus.value,
    compilerSpeed: compilerSpeed.value,
    performanceSurfaceEvidence: performanceSurfaceEvidence.value,
    files: { matrix, status, conformanceInventory, conformanceSummary, tierCensus, compilerSpeed, performanceSurfaceEvidence },
    entries,
    loadErrors: [
      ["manifest", manifestResult], ["matrix", matrix], ["status", status],
      ["conformance_inventory", conformanceInventory], ["conformance_summary", conformanceSummary],
      ["tier_census", tierCensus], ["compiler_speed", compilerSpeed],
      ...(surfaceSpec ? [["performance_surface_evidence", performanceSurfaceEvidence]] : []),
      ...[...entries].map(([name, result]) => [`entry:${name}`, result]),
    ].filter(([, result]) => result.error).map(([label, result]) => ({ label, path: result.path ?? paths[label] ?? paths.manifest, error: result.error })),
  };
}

function evaluateGate(inputs, options = {}) {
  const findings = [];
  const seen = new Set();
  const manifestPath = relativePath(inputs?.paths?.manifest) ?? "gauntlet/measurement-manifest.json";
  const add = (kind, fields = {}) => {
    const item = {
      kind: knownFailureKinds.has(kind) ? kind : "malformed-input",
      cell: fields.cell ?? null,
      entry: fields.entry ?? null,
      peer: fields.peer ?? null,
      tier: fields.tier ?? null,
      metric: fields.metric ?? null,
      evidence_file: relativePath(fields.evidence_file ?? manifestPath),
      cause: String(fields.cause ?? kind),
    };
    const key = canonical(item);
    if (!seen.has(key)) {
      seen.add(key);
      findings.push(item);
    }
    return item;
  };
  const manifest = inputs?.manifest;
  for (const issue of validateManifest(manifest)) add("manifest", { evidence_file: manifestPath, cause: issue });
  for (const loadError of asArray(inputs?.loadErrors)) {
    add("missing-input", { evidence_file: loadError.path ?? manifestPath, cause: `${loadError.label}: ${loadError.error}` });
  }
  const performanceSurfaceResult = evaluatePerformanceSurfacePairs(
    manifest?.performance_surface_pairs,
    inputs?.performanceSurfaceEvidence,
    { evidence_file: inputs?.paths?.performance_surface_evidence },
  );
  for (const finding of performanceSurfaceResult.findings) add(finding.kind, finding);
  const contract = manifest?.report_contract ?? {};
  const gate = manifest?.integrated_gate ?? {};
  const policy = contract.ratio_verdicts;
  const now = parseNow(options.now);
  const freshness = gate.freshness ?? {};
  const maxAgeMs = finite(freshness.max_age_seconds) ? freshness.max_age_seconds * 1000 : 0;
  const futureSkewMs = finite(freshness.future_skew_seconds) ? freshness.future_skew_seconds * 1000 : 0;
  const checkFresh = (label, record, stat, file, required = true, fields = {}) => {
    const stamp = sourceStamp(record, stat);
    if (stamp.value === null) {
      if (required) add("missing-timestamp", { ...fields, evidence_file: file, cause: `${label} has no evidence timestamp` });
      return null;
    }
    if (now !== null && stamp.value - now > futureSkewMs) add("future-evidence", { ...fields, evidence_file: file, cause: `${label} timestamp is in the future (${new Date(stamp.value).toISOString()})` });
    if (now !== null && maxAgeMs > 0 && now - stamp.value > maxAgeMs) add("stale", { ...fields, evidence_file: file, cause: `${label} evidence is older than ${freshness.max_age_seconds} seconds` });
    return stamp;
  };
  if (isObject(manifest?.performance_surface_pairs)) {
    checkFresh(
      "performance surface evidence",
      inputs?.performanceSurfaceEvidence,
      inputs?.files?.performanceSurfaceEvidence?.stat,
      inputs?.paths?.performance_surface_evidence,
      true,
      { metric: manifest.performance_surface_pairs.evidence?.metric ?? "wall_time_ms" },
    );
  }

  const checks = {
    gauntlet: { status: "fail", cells: [], metrics: [] },
    performance_surface_pairs: performanceSurfaceResult,
    conformance: { status: "fail" },
    tier_census: { status: "fail" },
    compiler_speed: { status: "fail" },
    identity: { status: "fail", commit: null, toolchain: null },
  };
  const matrix = inputs?.matrix;
  const status = inputs?.status;
  const entryRows = asArray(manifest?.entries);
  const entryMetadata = new Map();
  for (const row of entryRows) {
    const result = inputs?.entries?.get(row?.name);
    if (result?.value) entryMetadata.set(row.name, result.value);
  }
  const expectedMetricList = (mode) => asArray(gate.required_metrics_by_mode?.[mode]);
  const requiredTiers = (mode) => {
    const modeTiers = asArray(contract.tier_policy_by_mode?.[mode]);
    return supportedTiers.filter((tier) => modeTiers.includes(tier) && asArray(contract.required_jet_tiers).includes(tier));
  };
  const aotOnly = new Set(asArray(contract.aot_only_metrics));
  const gauntletStart = findings.length;
  const expectedCells = asArray(matrix?.cells).map((cell) => cell?.id).filter(nonEmpty).sort();
  if (new Set(expectedCells).size !== expectedCells.length) add("coverage", { evidence_file: inputs?.paths?.matrix, cause: "matrix contains duplicate cell ids" });
  const entryForCell = new Map();
  for (const row of entryRows) {
    const entry = entryMetadata.get(row?.name);
    for (const cell of asArray(entry?.cells)) {
      if (nonEmpty(cell)) {
        if (entryForCell.has(cell)) add("missing-entry", { cell, entry: row.name, evidence_file: inputs?.entries?.get(row.name)?.path, cause: `matrix cell is claimed by both ${entryForCell.get(cell)} and ${row.name}` });
        else entryForCell.set(cell, row.name);
      }
    }
  }
  const matrixIds = new Set(expectedCells);
  for (const [declaredCell, declaredEntry] of entryForCell) if (!matrixIds.has(declaredCell)) add("unexpected-cell", { cell: declaredCell, entry: declaredEntry, evidence_file: inputs?.entries?.get(declaredEntry)?.path, cause: "entry claims a cell outside the canonical matrix" });
  const statusCells = new Map();
  for (const cell of asArray(status?.cells)) {
    if (nonEmpty(cell?.id)) {
      if (statusCells.has(cell.id)) add("coverage", { cell: cell.id, evidence_file: inputs?.paths?.status, cause: "status contains a duplicate cell row" });
      statusCells.set(cell.id, cell);
    }
  }
  for (const cell of expectedCells) if (!statusCells.has(cell)) add("missing-cell", { cell, evidence_file: inputs?.paths?.status, cause: "required matrix cell is absent from gauntlet status" });
  for (const id of [...statusCells.keys()].sort()) if (!matrixIds.has(id)) add("unexpected-cell", { cell: id, evidence_file: inputs?.paths?.status, cause: "status contains a cell outside the canonical matrix" });
  if (matrix?.version !== 1) add("coverage", { evidence_file: inputs?.paths?.matrix, cause: "matrix version is not 1" });
  if (manifest?.corpus?.matrix_cell_count !== expectedCells.length) add("coverage", { evidence_file: manifestPath, cause: "manifest matrix_cell_count does not match matrix cells" });

  const cellResults = [];
  for (const cellId of expectedCells) {
    const cellStart = findings.length;
    const expectedEntry = entryForCell.get(cellId) ?? null;
    const metadata = expectedEntry ? entryMetadata.get(expectedEntry) : null;
    const statusCell = statusCells.get(cellId);
    const mode = metadata?.mode ?? statusCell?.mode ?? null;
    const cellResult = { id: cellId, entry: expectedEntry, mode, verdict: "unmeasured", metrics: [] };
    if (!nonEmpty(mode) || !Object.hasOwn(gate.required_metrics_by_mode ?? {}, mode)) add("cell-mismatch", { cell: cellId, entry: expectedEntry, evidence_file: inputs?.paths?.status ?? inputs?.paths?.matrix, cause: `cell mode ${mode ?? "missing"} has no canonical metric denominator` });
    if (!expectedEntry) add("missing-entry", { cell: cellId, evidence_file: inputs?.paths?.matrix, cause: "no declared gauntlet entry covers this matrix cell" });
    if (!statusCell) {
      cellResults.push(cellResult);
      continue;
    }
    if (statusCell.entry && expectedEntry && statusCell.entry !== expectedEntry) add("entry-mismatch", { cell: cellId, entry: statusCell.entry, evidence_file: inputs?.paths?.status, cause: `status names ${statusCell.entry}, canonical entry is ${expectedEntry}` });
    if (statusCell.mode && mode && statusCell.mode !== mode) add("cell-mismatch", { cell: cellId, entry: expectedEntry, evidence_file: inputs?.paths?.status, cause: `status mode ${statusCell.mode} does not match entry mode ${mode}` });
    const expectedPeers = asArray(metadata?.languages).filter((peer) => nonEmpty(peer) && peer !== "jet").sort();
    const actualPeers = statusPeers(statusCell);
    const peerMap = new Map();
    for (const item of actualPeers) {
      if (peerMap.has(item.peer)) add("coverage", { cell: cellId, entry: expectedEntry, peer: item.peer, evidence_file: inputs?.paths?.status, cause: "status contains a duplicate peer row" });
      peerMap.set(item.peer, item.row);
    }
    for (const peer of expectedPeers) if (!peerMap.has(peer)) add("missing-peer", { cell: cellId, entry: expectedEntry, peer, evidence_file: inputs?.paths?.status, cause: "declared peer is absent from the status cell" });
    for (const peer of [...peerMap.keys()].sort()) if (!expectedPeers.includes(peer)) add("unexpected-peer", { cell: cellId, entry: expectedEntry, peer, evidence_file: inputs?.paths?.status, cause: "status peer is not declared by the entry" });

    const metricResults = new Map();
    for (const metric of expectedMetricList(mode).slice().sort()) {
      const rows = [];
      for (const peer of expectedPeers) {
        const row = peerMap.get(peer);
        const applicability = metricApplicability(metadata, peer, metric);
        const metricResult = { metric, peer, applicability: applicability.status, verdict: "fail", tiers: [] };
        if (applicability.status === "invalid") {
          add("malformed-input", { cell: cellId, entry: expectedEntry, peer, metric, evidence_file: inputs?.entries?.get(expectedEntry)?.path, cause: applicability.reason });
          rows.push(metricResult);
          continue;
        }
        const record = metricRecord(row, metric);
        if (!record) {
          add("missing-metric", { cell: cellId, entry: expectedEntry, peer, metric, evidence_file: inputs?.paths?.status, cause: "required metric row is absent" });
          rows.push(metricResult);
          continue;
        }
        if (applicability.status === "not_applicable") {
          const reason = firstValue(record.reason, record.applicability?.reason, applicability.reason);
          if (normalizedStatus(record) !== "not_applicable" || !nonEmpty(reason)) add("wrong-status", { cell: cellId, entry: expectedEntry, peer, metric, evidence_file: inputs?.paths?.status, cause: "not_applicable metric lacks an explicit structural reason" });
          metricResult.verdict = normalizedStatus(record) === "not_applicable" && nonEmpty(reason) ? "not_applicable" : "fail";
          metricResult.reason = reason ?? null;
          rows.push(metricResult);
          continue;
        }
        const tiers = requiredTiers(mode).filter((tier) => !(aotOnly.has(metric) && tier !== "aot"));
        const knownTierNames = Object.keys(record.tiers ?? {}).sort();
        const allowedTiers = asArray(contract.tier_policy_by_mode?.[mode]);
        for (const tier of knownTierNames) if (!allowedTiers.includes(tier)) add("unexpected-tier", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: inputs?.paths?.status, cause: "status contains a tier outside the metric policy" });
        let metricWorstVerdict = "win";
        for (const tier of tiers) {
          const evidence = inputs?.paths?.status;
          const tierValue = tierRecord(record, tier);
          const tierResult = {
            peer, tier, metric, status: normalizedStatus(tierValue), jet: finite(tierValue?.jet) ? tierValue.jet : null,
            peer_value: finite(tierValue?.peer) ? tierValue.peer : null, ratio: finite(tierValue?.ratio) ? tierValue.ratio : null,
            verdict: "unmeasured", evidence_file: relativePath(tierValue?.source_file ?? record.source_file ?? evidence),
            measured_iso: firstValue(tierValue?.measured_iso, tierValue?.measured_at, record.measured_iso, record.measured_at) ?? null,
            cause: null,
          };
          metricResult.tiers.push(tierResult);
          if (!tierValue) {
            tierResult.cause = "required tier row is absent";
            add("missing-tier", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: evidence, cause: tierResult.cause });
            metricWorstVerdict = "unmeasured";
            continue;
          }
          const sourceFile = tierValue.source_file ?? record.source_file;
          if (!nonEmpty(sourceFile)) add("missing-evidence", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: evidence, cause: "measured tier has no exact evidence file" });
          else if (!awaitableExists(inputs, sourceFile)) add("missing-evidence", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile, cause: "exact evidence file does not exist" });
          const stamp = checkFresh(`gauntlet ${cellId}/${peer}/${tier}/${metric}`, tierValue, null, sourceFile ?? evidence, true, { cell: cellId, entry: expectedEntry, peer, tier, metric });
          tierResult.measured_iso = tierResult.measured_iso ?? (stamp?.value === null ? null : new Date(stamp.value).toISOString());
          const verification = firstValue(tierValue.output_verification, tierValue.verification, tierValue.output_verified, tierValue.output_ok, record.output_verification, record.output_verified);
          const verificationState = isObject(verification) ? firstValue(verification.status, verification.kind, verification.verdict) : verification;
          const outputVerified = isObject(verification) ? firstValue(verification.byte_identical, verification.byte_exact, verification.verified) : verification;
          if (outputVerified === false || verificationState === false || (typeof verificationState === "string" && !["ok", "pass", "verified", "byte_exact_stdout", "service_probe_sequence"].includes(verificationState))) {
            add("wrong-output", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: "tier output verification is absent, failed, or not byte-exact" });
          }
          const statusValue = normalizedStatus(tierValue);
          if (!resultStatuses.has(statusValue)) {
            tierResult.cause = firstValue(tierValue.reason, `tier status is ${statusValue}`);
            const kind = statusValue === "inconclusive" ? "inconclusive" : statusValue === "unavailable" ? "unavailable" : "wrong-status";
            add(kind, { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: tierResult.cause });
            metricWorstVerdict = "unmeasured";
            continue;
          }
          if (!finite(tierValue.jet) || !finite(tierValue.peer) || !finite(tierValue.ratio) || tierValue.peer <= 0 || tierValue.jet < 0) {
            tierResult.cause = "measured tier must contain finite nonnegative Jet and positive peer values plus a finite ratio";
            add("unavailable", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: tierResult.cause });
            metricWorstVerdict = "unmeasured";
            continue;
          }
          const calculated = tierValue.jet / tierValue.peer;
          if (Math.abs(calculated - tierValue.ratio) > Math.max(1e-9, Math.abs(calculated) * 1e-9)) add("wrong-ratio", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: `reported ratio ${tierValue.ratio} does not equal Jet/peer ${calculated}` });
          const verdict = expectedPairVerdict(peer, tierValue.ratio, policy);
          tierResult.verdict = verdict;
          if (tierValue.verdict !== undefined && tierValue.verdict !== verdict) add("wrong-verdict", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: `reported verdict ${tierValue.verdict} does not satisfy the canonical ${verdict} comparator` });
          if (verdict === "loss") {
            tierResult.cause = `Jet/peer ratio ${tierValue.ratio} violates ${isRustPeer(peer) ? "Rust <=1.05 parity" : "non-Rust <1.00 win"}`;
            add("loss", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: tierResult.cause });
            metricWorstVerdict = "loss";
          } else if (verdict === "unmeasured") {
            tierResult.cause = "comparator policy is unavailable or invalid";
            add("policy-mismatch", { cell: cellId, entry: expectedEntry, peer, tier, metric, evidence_file: sourceFile ?? evidence, cause: tierResult.cause });
            metricWorstVerdict = "unmeasured";
          } else if (verdict === "parity" && metricWorstVerdict === "win") metricWorstVerdict = "parity";
        }
        metricResult.verdict = metricWorstVerdict;
        rows.push(metricResult);
      }
      const actualMetricNames = new Set(expectedPeers.flatMap((peer) => statusMetricNames(peerMap.get(peer))));
      for (const extra of [...actualMetricNames].filter((name) => !expectedMetricList(mode).includes(name)).sort()) add("unexpected-metric", { cell: cellId, entry: expectedEntry, metric: extra, evidence_file: inputs?.paths?.status, cause: "status metric is outside the canonical mode metric denominator" });
      const allNotApplicable = rows.length > 0 && rows.every((item) => item.verdict === "not_applicable");
      const hasLoss = rows.some((item) => item.verdict === "loss");
      const hasUnmeasured = rows.some((item) => item.verdict === "fail" || item.verdict === "unmeasured");
      const hasParity = rows.some((item) => item.verdict === "parity");
      const aggregate = hasUnmeasured ? "unmeasured" : (hasLoss ? "loss" : (allNotApplicable ? "not_applicable" : (hasParity ? "parity" : "win")));
      metricResults.set(metric, { metric, verdict: aggregate, peers: rows });
    }
    cellResult.metrics = [...metricResults.values()];
    const primaryMetric = statusCell.primary_metric ?? contract.primary_metrics?.[mode];
    const primaryResult = cellResult.metrics.find((metric) => metric.metric === primaryMetric);
    const metricVerdicts = cellResult.metrics.map((metric) => metric.verdict);
    const cellVerdict = primaryResult
      ? primaryResult.verdict
      : (metricVerdicts.length === 0 || metricVerdicts.every((verdict) => verdict === "not_applicable")
        ? "unmeasured"
        : (metricVerdicts.includes("unmeasured") ? "unmeasured" : worstVerdict(metricVerdicts)));
    cellResult.reported_verdict = statusCell.verdict ?? null;
    if (statusCell.verdict === undefined) add("wrong-verdict", { cell: cellId, entry: expectedEntry, evidence_file: inputs?.paths?.status, cause: "status cell has no aggregate verdict" });
    else if (statusCell.verdict !== cellVerdict) add("wrong-verdict", { cell: cellId, entry: expectedEntry, evidence_file: inputs?.paths?.status, cause: `reported cell verdict ${statusCell.verdict} does not equal computed ${cellVerdict}` });
    if (statusCell.verdict && !["win", "parity", "loss", "unmeasured"].includes(statusCell.verdict)) add("wrong-verdict", { cell: cellId, entry: expectedEntry, evidence_file: inputs?.paths?.status, cause: `unknown cell verdict ${statusCell.verdict}` });
    cellResult.verdict = cellVerdict;
    if (findings.slice(cellStart).some((finding) => finding.cell === cellId) && cellResult.verdict === "win") cellResult.verdict = "unmeasured";
    cellResults.push(cellResult);
  }
  cellResults.sort((left, right) => left.id.localeCompare(right.id));
  checks.gauntlet.cells = cellResults;
  checks.gauntlet.metrics = cellResults.flatMap((cell) => cell.metrics).sort((left, right) => `${left.metric}\u0000${left.peers?.[0]?.peer ?? ""}`.localeCompare(`${right.metric}\u0000${right.peers?.[0]?.peer ?? ""}`));
  const computedCellSummary = {
    cells: cellResults.length,
    win: cellResults.filter((cell) => cell.verdict === "win").length,
    parity: cellResults.filter((cell) => cell.verdict === "parity").length,
    loss: cellResults.filter((cell) => cell.verdict === "loss").length,
    unmeasured: cellResults.filter((cell) => cell.verdict === "unmeasured").length,
  };
  for (const key of Object.keys(computedCellSummary)) if (status?.summary?.[key] !== computedCellSummary[key]) add("coverage", { evidence_file: inputs?.paths?.status, cause: `status summary ${key}=${status?.summary?.[key] ?? "missing"} does not match computed ${computedCellSummary[key]}` });
  if (canonical(status?.policy?.tier_policy_by_mode) !== canonical(contract.tier_policy_by_mode)) add("policy-mismatch", { evidence_file: inputs?.paths?.status, cause: "status tier policy differs from canonical manifest" });
  if (!nonEmpty(status?.provenance?.matrix_sha256)) add("identity-missing", { evidence_file: inputs?.paths?.status, cause: "status has no matrix provenance digest" });
  if (!nonEmpty(status?.provenance?.measurement_manifest_sha256)) add("identity-missing", { evidence_file: inputs?.paths?.status, cause: "status has no measurement-manifest provenance digest" });
  const statusStamp = checkFresh("gauntlet status", status, inputs?.files?.status?.stat, inputs?.paths?.status, true);
  if (status?.contract !== statusContract) add("malformed-input", { evidence_file: inputs?.paths?.status, cause: `status contract must be ${statusContract}` });
  if (status?.source?.report_contract !== reportContract) add("policy-mismatch", { evidence_file: inputs?.paths?.status, cause: "status source report contract does not match measurement manifest" });
  if (canonical(status?.policy?.primary_metric_by_mode) !== canonical(contract.primary_metric_by_mode)) add("policy-mismatch", { evidence_file: inputs?.paths?.status, cause: "status primary metric policy differs from canonical manifest" });
  if (canonical({ rust: status?.policy?.verdict_policy?.rust, non_rust: status?.policy?.verdict_policy?.non_rust }) !== canonical(policy)) add("policy-mismatch", { evidence_file: inputs?.paths?.status, cause: "status comparator policy differs from canonical manifest" });
  if (status?.publication?.status !== "complete" || status?.publication?.complete !== true) add("publication", { evidence_file: inputs?.paths?.status, cause: "gauntlet status publication is not complete" });
  if (status?.provenance?.matrix_sha256 && inputs?.files?.matrix?.text !== null && inputs?.files?.matrix?.text !== undefined && status.provenance.matrix_sha256 !== sha256(inputs.files.matrix.text)) add("identity-mismatch", { evidence_file: inputs?.paths?.status, cause: "status matrix provenance does not match matrix evidence" });
  if (status?.provenance?.measurement_manifest_sha256 && inputs?.manifestResult?.text !== null && inputs?.manifestResult?.text !== undefined && status.provenance.measurement_manifest_sha256 !== sha256(inputs.manifestResult.text)) add("identity-mismatch", { evidence_file: inputs?.paths?.status, cause: "status measurement-manifest provenance does not match manifest evidence" });
  checks.gauntlet.status = findings.length === gauntletStart ? "pass" : "fail";
  const conformanceStart = findings.length;

  const conformanceFile = inputs?.paths?.conformance_inventory;
  const summaryFile = inputs?.paths?.conformance_summary;
  const inventory = inputs?.conformanceInventory;
  const summary = typeof inputs?.conformanceSummary === "string" ? inputs.conformanceSummary : "";
  checkFresh("conformance inventory", inventory, inputs?.files?.conformanceInventory?.stat, conformanceFile, true);
  checkFresh("conformance summary", null, inputs?.files?.conformanceSummary?.stat, summaryFile, true);
  if (!Array.isArray(inventory) || inventory.length === 0 || inventory.some((item) => !nonEmpty(item))) add("conformance", { evidence_file: conformanceFile, cause: "conformance inventory must be a non-empty array of public Core names" });
  else if (new Set(inventory).size !== inventory.length) add("conformance", { evidence_file: conformanceFile, cause: "conformance inventory contains duplicate public Core names" });
  const denominator = summary.match(/core conformance denominator:\s*(\d+) public function\(s\);\s*(\d+) program\(s\);\s*(\d+) carve-out\(s\);\s*(\d+) uncovered row\(s\)/i);
  if (!denominator) add("summary-mismatch", { evidence_file: summaryFile, cause: "paired conformance summary has no denominator line" });
  else {
    const [, publicCount, programs, carveouts, uncovered] = denominator.map(Number);
    if (Array.isArray(inventory) && publicCount !== inventory.length) add("summary-mismatch", { evidence_file: summaryFile, cause: `summary denominator ${publicCount} differs from inventory length ${inventory.length}` });
    if (publicCount !== programs + carveouts) add("summary-mismatch", { evidence_file: summaryFile, cause: "program and carve-out counts do not cover the public denominator" });
    if (uncovered !== 0) add("conformance", { evidence_file: summaryFile, cause: `conformance denominator reports ${uncovered} uncovered row(s)` });
  }
  if (/^\s*error:/mi.test(summary)) add("conformance", { evidence_file: summaryFile, cause: "conformance summary contains an error line" });
  checks.conformance.status = findings.length === conformanceStart ? "pass" : "fail";
  const conformanceBoundary = findings.length;

  const censusStart = findings.length;
  const census = inputs?.tierCensus;
  const censusFile = inputs?.paths?.tier_census;
  if (!isObject(census) || census.schema_version !== 1) add("malformed-input", { evidence_file: censusFile, cause: "tier census schema_version must be 1" });
  const censusCounts = census?.counts ?? {};
  for (const [surface, countKey, rowsKey] of [["tir", "tir_variants", "tir"], ["core_call", "core_records", "core_calls"]]) {
    if (!Number.isInteger(censusCounts[countKey]) || censusCounts[countKey] <= 0) add("tier-count", { evidence_file: censusFile, cause: `tier census ${countKey} is missing or invalid` });
    if (!Array.isArray(census?.[rowsKey]) || census[rowsKey].length !== censusCounts[countKey]) add("tier-count", { evidence_file: censusFile, cause: `${rowsKey} rows do not match ${countKey}` });
  }
  if (census?.timing_status !== "complete" && census?.timing_status !== "verified" && census?.timing_status !== "passed") add("inconclusive", { evidence_file: censusFile, cause: `tier census timing_status is ${census?.timing_status ?? "missing"}, not complete` });
  const statusCounts = census?.status_counts;
  for (const surface of ["tir", "core_call"]) {
    for (const backend of supportedBackends) {
      const row = statusCounts?.[surface]?.[backend];
      if (!isObject(row)) {
        add("missing-tier", { tier: backend, metric: surface, evidence_file: censusFile, cause: "tier census backend row is absent" });
        continue;
      }
      for (const state of ["covered", "refused", "absent", "dynamic"]) {
        if (!Number.isInteger(row[state]) || row[state] < 0) add("malformed-input", { tier: backend, metric: surface, evidence_file: censusFile, cause: `tier census ${state} count is invalid` });
      }
      for (const state of ["refused", "absent", "dynamic"]) if (row[state] > 0) add("tier-not-covered", { tier: backend, metric: surface, evidence_file: censusFile, cause: `${surface}/${backend} has ${row[state]} ${state} row(s)` });
    }
  }
  if (Number.isInteger(census?.dynamic_probe_cells?.total) && census.dynamic_probe_cells.total > 0) add("inconclusive", { evidence_file: censusFile, cause: `tier census has ${census.dynamic_probe_cells.total} unresolved dynamic probe cell(s)` });
  checks.tier_census.status = findings.length === censusStart ? "pass" : "fail";
  const compilerStart = findings.length;

  const compiler = inputs?.compilerSpeed;
  const compilerFile = inputs?.paths?.compiler_speed;
  const compilerConfig = gate.compiler_speed ?? {};
  if (!isObject(compiler) || compiler.schema !== compilerConfig.schema || compiler.version !== compilerConfig.version) add("baseline", { evidence_file: compilerFile, cause: `compiler-speed baseline must be ${compilerConfig.schema} version ${compilerConfig.version}` });
  for (const field of asArray(compilerConfig.required_fields)) {
    if (!hasValue(nestedValue(compiler, [field, `machine.${field}`]))) add("identity-missing", { metric: field, evidence_file: compilerFile, cause: `compiler-speed baseline is missing required identity field ${field}` });
  }
  const runs = asArray(compiler?.runs);
  if (runs.length === 0) add("baseline", { evidence_file: compilerFile, cause: "compiler-speed baseline has no production run rows" });
  for (const [index, row] of runs.entries()) {
    for (const field of ["program", "state", "stage"]) if (!nonEmpty(row?.[field])) add("baseline", { metric: `runs[${index}].${field}`, evidence_file: compilerFile, cause: "compiler-speed run row is incomplete" });
    for (const field of ["latency_ns", "memory_bytes"]) if (!finite(row?.[field]) || row[field] <= 0) add("baseline", { metric: `runs[${index}].${field}`, evidence_file: compilerFile, cause: "compiler-speed run metric must be positive" });
  }
  const peerRows = asArray(compiler?.peer_rows ?? compiler?.peers?.rows ?? compiler?.peer?.rows);
  if (peerRows.length === 0) add("missing-peer", { evidence_file: compilerFile, cause: "compiler-speed baseline has no matched peer rows" });
  const peerGroups = new Map();
  for (const [index, row] of peerRows.entries()) {
    const language = firstValue(row?.language, row?.peer);
    if (!nonEmpty(language) || !nonEmpty(row?.program) || !nonEmpty(row?.state) || !nonEmpty(row?.metric)) add("baseline", { metric: `peer_rows[${index}]`, evidence_file: compilerFile, cause: "compiler-speed peer row lacks language/program/state/metric" });
    if (!finite(row?.value) || row.value <= 0) add("baseline", { metric: `peer_rows[${index}].value`, evidence_file: compilerFile, cause: "compiler-speed peer value must be positive" });
    for (const field of ["workload_sha256", "source_sha256", "expected_sha256", "manifest_sha256", "toolchain_sha256"]) if (!/^[0-9a-f]{64}$/i.test(String(row?.[field] ?? ""))) add("identity-missing", { metric: field, evidence_file: compilerFile, cause: `compiler-speed peer row ${index} has no full ${field}` });
    const key = `${row?.program ?? ""}\u0000${row?.state ?? ""}\u0000${row?.metric ?? ""}`;
    if (!peerGroups.has(key)) peerGroups.set(key, []);
    peerGroups.get(key).push(row);
  }
  for (const [key, rows] of peerGroups) {
    const jet = rows.find((row) => row.language === "jet" || row.peer === "jet");
    if (!jet) {
      add("missing-peer", { metric: key, evidence_file: compilerFile, cause: "compiler-speed peer group has no Jet row" });
      continue;
    }
    for (const row of rows.filter((candidate) => candidate !== jet)) {
      const language = firstValue(row.language, row.peer);
      const ratio = jet.value / row.value;
      const verdict = expectedPairVerdict(language, ratio, policy);
      if (verdict === "loss") add("loss", { peer: language, metric: key, evidence_file: compilerFile, cause: `compiler-speed Jet/peer ratio ${ratio} violates the strict comparator` });
      if (row.ratio !== undefined && (!finite(row.ratio) || Math.abs(row.ratio - ratio) > Math.max(1e-9, Math.abs(ratio) * 1e-9))) add("wrong-ratio", { peer: language, metric: key, evidence_file: compilerFile, cause: "compiler-speed peer ratio does not equal Jet/peer" });
    }
  }
  if (compiler?.status && compiler.status !== "pass" && compiler.status !== "verified") add("baseline", { evidence_file: compilerFile, cause: `compiler-speed baseline status is ${compiler.status}` });
  if (compiler?.parity && compiler.parity !== "pass" && compiler.parity !== "verified") add("baseline", { evidence_file: compilerFile, cause: `compiler-speed parity status is ${compiler.parity}` });
  checks.compiler_speed.status = findings.length === compilerStart ? "pass" : "fail";
  const identityStart = findings.length;

  const identitySources = [
    { label: "status", file: inputs?.paths?.status, value: nestedValue(status, ["provenance.jet_commit", "provenance.commit", "source.commit", "jet_commit", "commit", "commit_sha"]) },
    { label: "compiler_speed", file: compilerFile, value: nestedValue(compiler, ["jet_commit", "commit", "commit_sha", "candidate_commit"]) },
  ];
  const commits = identitySources.map((source) => ({ ...source, commit: commitValue(source.value) })).filter((source) => source.value !== null);
  for (const source of commits) if (!source.commit) add("identity-mismatch", { evidence_file: source.file, cause: `${source.label} commit identity is not a 40-character SHA-1` });
  const validCommits = commits.filter((source) => source.commit);
  const expectedCommit = commitValue(options.commit ?? gate.identity?.expected_commit);
  if (validCommits.length === 0) add("identity-missing", { evidence_file: inputs?.paths?.status ?? compilerFile, cause: "no evidence source records the candidate commit identity" });
  else {
    const uniqueCommits = [...new Set(validCommits.map((source) => source.commit))];
    if (uniqueCommits.length > 1) add("identity-mismatch", { evidence_file: inputs?.paths?.status ?? compilerFile, cause: `evidence sources disagree on candidate commit (${uniqueCommits.join(", ")})` });
    if (expectedCommit && uniqueCommits[0] !== expectedCommit) add("identity-mismatch", { evidence_file: inputs?.paths?.status ?? compilerFile, cause: `evidence commit ${uniqueCommits[0]} does not match expected ${expectedCommit}` });
    checks.identity.commit = uniqueCommits[0];
  }
  const toolchainObject = status?.provenance?.toolchains;
  const toolchainHash = firstValue(
    nestedValue(status, ["provenance.toolchain_sha256", "toolchain_sha256"]),
    nestedValue(compiler, ["toolchain_sha256", "machine.toolchain_sha256"]),
  );
  if (isObject(toolchainObject)) {
    const names = Object.keys(toolchainObject).sort();
    if (names.length === 0) add("identity-missing", { evidence_file: inputs?.paths?.status, cause: "toolchain identity map is empty" });
    for (const name of names) {
      const tool = toolchainObject[name];
      if (!isObject(tool) || (tool.status && tool.status !== "ok") || !nonEmpty(firstValue(tool.version, tool.sha256, tool.identity))) add("unavailable", { evidence_file: inputs?.paths?.status, cause: `toolchain ${name} has no successful version identity` });
    }
    checks.identity.toolchain = names;
  } else if (!nonEmpty(toolchainHash)) add("identity-missing", { evidence_file: compilerFile ?? inputs?.paths?.status, cause: "no toolchain identity map or digest is present" });
  else checks.identity.toolchain = toolchainHash;
  checks.identity.status = findings.length === identityStart ? "pass" : "fail";

  const findingKinds = findings.reduce((counts, finding) => {
    counts[finding.kind] = (counts[finding.kind] ?? 0) + 1;
    return counts;
  }, {});
  findings.sort((left, right) => canonical(left).localeCompare(canonical(right)));
  const cellFailures = cellResults.filter((cell) => !["win", "parity"].includes(cell.verdict)).length;
  const metricRows = cellResults.flatMap((cell) => cell.metrics.flatMap((metric) => metric.peers ?? []));
  const worstCase = findings.some((finding) => finding.kind === "loss") ? "loss"
    : findings.some((finding) => ["unavailable", "inconclusive", "stale", "future-evidence", "wrong-output"].includes(finding.kind)) ? "unmeasured"
      : findings.length > 0 ? "failure" : (cellResults.some((cell) => cell.metrics.some((metric) => metric.verdict === "parity")) ? "parity" : "win");
  return {
    schema: gateSchema,
    version: 1,
    verdict: findings.length === 0 ? "pass" : "fail",
    worst_case: worstCase,
    inputs: Object.fromEntries(Object.entries(inputs?.paths ?? {}).map(([key, value]) => [key, relativePath(value)])),
    identity: checks.identity,
    checks: {
      gauntlet: { status: checks.gauntlet.status, cells: checks.gauntlet.cells },
      performance_surface_pairs: checks.performance_surface_pairs,
      conformance: checks.conformance,
      tier_census: checks.tier_census,
      compiler_speed: checks.compiler_speed,
    },
    summary: {
      cells: cellResults.length,
      cells_pass: cellResults.length - cellFailures,
      cells_fail: cellFailures,
      metric_rows: metricRows.length,
      performance_surface_pairs: checks.performance_surface_pairs.pairs.length,
      performance_surface_pairs_pass: checks.performance_surface_pairs.pairs.filter((pair) => pair.verdict === "win").length,
      performance_surface_pairs_fail: checks.performance_surface_pairs.pairs.filter((pair) => pair.verdict !== "win").length,
      findings: findings.length,
      finding_kinds: Object.fromEntries(Object.entries(findingKinds).sort(([left], [right]) => left.localeCompare(right))),
    },
    findings,
  };
}

function awaitableExists(_inputs, sourceFile) {
  const absolute = resolvePath(sourceFile, repoDir);
  return absolute !== null && existsSync(absolute);
}

function humanReport(report) {
  const summary = report.summary;
  const lines = [
    `integrated gate: ${report.verdict} (worst=${report.worst_case})`,
    `cells: ${summary.cells_pass}/${summary.cells} pass; metric rows: ${summary.metric_rows}; findings: ${summary.findings}`,
  ];
  for (const [name, check] of Object.entries(report.checks)) lines.push(`${name}: ${check.status}`);
  for (const finding of report.findings.slice(0, 40)) {
    const location = [finding.cell, finding.peer, finding.tier, finding.metric].filter(Boolean).join("/");
    lines.push(`- ${finding.kind}${location ? ` ${location}` : ""}: ${finding.cause} [${finding.evidence_file}]`);
  }
  if (report.findings.length > 40) lines.push(`- ... ${report.findings.length - 40} additional findings in --json output`);
  return `${lines.join("\n")}\n`;
}

async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  if (options.help) {
    process.stdout.write(`${HELP}\n`);
    return 0;
  }
  const inputs = await loadInputs(options);
  const report = evaluateGate(inputs, options);
  if (options.json) process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  else if (options.check) process.stdout.write(humanReport(report));
  return report.verdict === "pass" ? 0 : 1;
}

export {
  evaluateGate,
  evaluatePerformanceSurfacePairs,
  evaluatePerformanceSurfacePairs as evaluateSurfacePairs,
  expectedPairVerdict,
  loadInputs,
  nestedValue,
  parseArgs,
  parseTimestamp,
  validateManifest,
  validatePerformanceSurfacePairs,
};

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((code) => {
    process.exitCode = code;
  }).catch((error) => {
    if (error.code === "USAGE") console.error(`integrated gate: ${error.message}\n\n${HELP}`);
    else console.error(`integrated gate: ${error.message}`);
    process.exitCode = error.code === "USAGE" ? 2 : 1;
  });
}
