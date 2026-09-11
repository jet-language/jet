#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  readdirSync,
  readFileSync,
  statSync,
} from "node:fs";
import { dirname, extname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const SCHEMA = "jet.test-economics.report.v1";
export const SCHEMA_VERSION = 1;
export const RUNNER_SOURCES = Object.freeze([
  "scripts/agent/hardening-rig.mjs",
  "scripts/agent/hardening-oracle-layer.mjs",
  "scripts/agent/time-suites.sh",
  "gauntlet/harness/run.mjs",
]);
export const HARDENING_REQUIREMENTS = Object.freeze({
  clean_days: 14,
  valid_cases: 10_000_000,
  mutations_per_callable: 100,
  fresh_context_lanes: 8,
});
const CANDIDATE_FIELDS = Object.freeze([
  "commit",
  "binary_sha256",
  "registry_snapshot_hash",
  "config_hash",
]);
const TIER_NAMES = new Set(["aot", "jet_run", "interpreter", "run", "dev", "web"]);
const STATUS_NAMES = new Set(["pass", "detected", "fail", "unavailable", "skipped", "inconclusive"]);
const DEFAULT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

function clone(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
}

export function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

export function digest(value) {
  return `sha256:${createHash("sha256").update(Buffer.from(canonicalJson(value), "utf8")).digest("hex")}`;
}

function string(value) {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function list(value) {
  if (Array.isArray(value)) return [...new Set(value.filter((item) => string(item)).map(String))].sort();
  if (typeof value === "string" && value.length > 0) return [value];
  return [];
}

function first(value, keys) {
  for (const key of keys) {
    const candidate = value?.[key];
    if (candidate !== undefined && candidate !== null) return candidate;
  }
  return null;
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`cannot read JSON ${path}: ${error.message}`);
  }
}
function fileHash(path) {
  if (!path || !existsSync(path)) return null;
  try {
    if (!statSync(path).isFile()) return null;
    return `sha256:${createHash("sha256").update(readFileSync(path)).digest("hex")}`;
  } catch {
    return null;
  }
}


function pathFor(root, value) {
  if (!value) return null;
  return isAbsolute(value) ? value : resolve(root, value);
}

function relPath(root, value) {
  const path = relative(root, value).split("\\").join("/");
  return path || ".";
}

function walkFiles(root, out = []) {
  if (!root || !existsSync(root)) return out;
  let stat;
  try { stat = statSync(root); } catch { return out; }
  if (stat.isFile()) {
    out.push(root);
    return out;
  }
  if (!stat.isDirectory()) return out;
  for (const name of readdirSync(root).sort()) {
    walkFiles(join(root, name), out);
  }
  return out;
}

function candidateIdentity(value) {
  const source = value?.candidate && typeof value.candidate === "object" ? value.candidate : value;
  const identity = {
    commit: string(first(source, ["commit", "jet_commit", "source_commit"])),
    binary_sha256: string(first(source, ["binary_sha256", "binary_hash", "binary"])),
    registry_snapshot_hash: string(first(source, ["registry_snapshot_hash", "registry_sha256", "manifest_sha256", "registry_hash"])),
    config_hash: string(first(source, ["config_hash", "config_sha256", "rig_config_sha256"])),
  };
  return CANDIDATE_FIELDS.every((key) => identity[key]) ? identity : null;
}

export function candidateKey(candidate) {
  if (!candidate) return null;
  return CANDIDATE_FIELDS.map((key) => `${key}=${candidate[key]}`).join("|");
}

function candidateFromBundle(value) {
  if (value?.bundle && typeof value.bundle === "object") return value.bundle;
  return value;
}

function normalizeCandidate(value) {
  return candidateIdentity(candidateFromBundle(value));
}

function sameCandidate(left, right) {
  return Boolean(left && right && CANDIDATE_FIELDS.every((key) => left[key] === right[key]));
}

function normalizeCost(value) {
  const source = value?.cost && typeof value.cost === "object" ? value.cost : value || {};
  let wall_ms = first(source, ["wall_ms", "wall_time_ms", "duration_ms", "elapsed_ms", "time_ms", "runtime_ms"]);
  if (wall_ms === null) {
    const seconds = first(source, ["wall_seconds", "duration_seconds", "elapsed_seconds", "runtime_seconds"]);
    if (typeof seconds === "number") wall_ms = seconds * 1000;
  }
  let memory_bytes = first(source, ["memory_bytes", "peak_memory_bytes", "rss_bytes"]);
  if (memory_bytes === null) {
    const kb = first(source, ["memory_kb", "peak_rss_kb", "rss_kb"]);
    if (typeof kb === "number") memory_bytes = kb * 1024;
  }
  if (memory_bytes === null) {
    const mb = first(source, ["memory_mb", "peak_memory_mb", "rss_mb"]);
    if (typeof mb === "number") memory_bytes = mb * 1024 * 1024;
  }
  const valid = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
  return {
    wall_ms: valid(wall_ms) ? wall_ms : null,
    memory_bytes: valid(memory_bytes) ? memory_bytes : null,
    measured: valid(wall_ms) && valid(memory_bytes),
  };
}

function nestedObservation(value) {
  if (!value || typeof value !== "object") return value;
  if (Array.isArray(value.tier_results) && value.tier_results.length > 0) return value.tier_results[0];
  if (Array.isArray(value.observations) && value.observations.length > 0) return value.observations[0];
  return value;
}

function rawEvidence(value) {
  const source = nestedObservation(candidateFromBundle(value));
  const stdout = first(source, ["stdout_bytes", "raw_stdout", "stdout"]);
  const stderr = first(source, ["stderr_bytes", "raw_stderr", "stderr"]);
  const hasExit = Object.prototype.hasOwnProperty.call(source || {}, "exit");
  const hasRaw = stdout !== null && stdout !== undefined && stderr !== null && stderr !== undefined;
  return {
    available: Boolean(hasRaw && hasExit),
    stdout: stdout ?? null,
    stderr: stderr ?? null,
    exit: hasExit ? source.exit : null,
    signal: first(source, ["signal"]),
    timeout: source?.timeout === true,
    command: string(first(value, ["tier_command", "command"])) || string(first(source, ["tier_command", "command"])),
    tier: string(first(value, ["tier"])) || string(first(source, ["tier"])),
    source_sha256: string(first(value, ["source_sha256"])) || string(first(source, ["source_sha256"])),
    run_id: string(first(value, ["run_id", "id"])) || string(first(source, ["run_id", "id"])),
  };
}

function relationMismatch(value) {
  const expected = first(value, ["expected_relation", "expected"]);
  const actual = first(value, ["actual_relation", "actual"]);
  return expected !== null && actual !== null && canonicalJson(expected) !== canonicalJson(actual);
}

function normalizeStatus(value) {
  const explicit = String(first(value, ["status", "outcome", "result"]) ?? "").toLowerCase();
  if (STATUS_NAMES.has(explicit)) return explicit;
  if (["passed", "ok", "green", "covered", "killed"].includes(explicit)) return explicit === "killed" ? "detected" : "pass";
  if (["failed", "red", "error", "broken"].includes(explicit)) return "fail";
  if (["blocked", "unknown", "unavailable", "skipped", "stale"].includes(explicit)) return "unavailable";
  if (relationMismatch(value)) return "detected";
  const exit = first(value, ["exit", "exit_code"]);
  if (typeof exit === "number") return exit === 0 ? "pass" : "fail";
  return "unavailable";
}

function oracleInfo(value) {
  const source = value?.oracle && typeof value.oracle === "object" ? value.oracle : value;
  return {
    name: string(first(source, ["name", "oracle_name"])),
    version: string(first(source, ["version", "oracle_version"])),
    independence_class: string(first(source, ["independence_class", "independence", "oracle_class"])),
    input_digest: string(first(source, ["input_digest", "oracle_input_digest"])),
    valid: source?.valid !== false && source?.broken !== true,
    broken: source?.valid === false || source?.broken === true || explicitBroken(source),
  };
}

function explicitBroken(value) {
  const status = String(first(value, ["status", "state"]) ?? "").toLowerCase();
  return ["broken", "invalid", "unavailable", "error"].includes(status);
}

export function normalizeEvidence(value, fallback = {}) {
  const source = value?.bundle && typeof value.bundle === "object" ? value.bundle : value;
  const candidate = normalizeCandidate(source) || normalizeCandidate(fallback);
  const raw = rawEvidence(source);
  const status = normalizeStatus(source);
  const oracle = oracleInfo(source);
  const cost = normalizeCost(source);
  const harnessError = first(source, ["harness_error", "harnessError", "error_kind"]);
  const availability = first(source, ["availability", "availability_state", "availabilityStatus"]);
  const flake = first(source, ["flake", "flaky", "flake_rate", "retry_count"]);
  const valueConsuming = source?.value_consuming !== false && source?.discarded_output !== true && source?.bound_and_discarded !== true;
  return {
    status,
    candidate,
    candidate_key: candidateKey(candidate),
    raw,
    oracle,
    cost,
    harness_error: harnessError ? String(harnessError) : null,
    valid_inputs: first(source, ["valid_inputs", "valid_input_count", "valid_cases", "valid_case_count"]),
    availability: availability === null ? null : String(availability),
    flake,
    value_consuming: valueConsuming,
    classification: string(first(source, ["classification", "defect_class", "kind"])),
    source: string(first(source, ["source", "path", "fixture"])),
  };
}

function evidenceError(evidence, expectedCandidate = null) {
  const errors = [];
  if (!evidence.candidate) errors.push("wrong-identity: candidate identity is incomplete");
  if (expectedCandidate && !sameCandidate(evidence.candidate, expectedCandidate)) errors.push("wrong-identity: candidate does not match frozen candidate");
  if (evidence.flake === true || evidence.flake === "true") errors.push("flaky: evidence is marked flaky");
  if (evidence.availability && ["unavailable", "skipped", "refused", "unknown"].includes(String(evidence.availability).toLowerCase())) errors.push(`availability: evidence is ${evidence.availability}`);
  if (!evidence.raw.available) errors.push("discarded-output: raw stdout/stderr/exit evidence is incomplete");
  if (evidence.valid_inputs !== null && evidence.valid_inputs !== undefined && Number(evidence.valid_inputs) <= 0) errors.push("zero-valid: record contains no valid inputs");
  if (!evidence.value_consuming) errors.push("discarded-output: output is bound and discarded");
  if (evidence.oracle.broken || !evidence.oracle.independence_class) errors.push("broken-oracle: oracle is invalid or not independent");
  if (evidence.harness_error) errors.push("harness-error: harness error was reported");
  if (!evidence.raw.run_id) errors.push("run-identity: run_id is missing");
  if (!evidence.raw.command) errors.push("run-identity: exact tier command is missing");
  if (!evidence.raw.source_sha256) errors.push("run-identity: source hash is missing");
  return errors;
}

function testId(value, index) {
  return string(first(value, ["id", "test_id", "name", "finding_id", "stable_surface_id"])) || `test-${index + 1}`;
}

function testEvidenceValues(value) {
  if (Array.isArray(value?.evidence)) return value.evidence;
  if (Array.isArray(value?.records)) return value.records;
  if (value?.bundle && typeof value.bundle === "object") return [value.bundle];
  if (value && (value.stdout_bytes !== undefined || value.raw_stdout !== undefined || value.exit !== undefined || value.tier_results)) return [value];
  return [];
}

export function normalizeTest(value, index = 0) {
  const id = testId(value, index);
  const stableId = string(first(value, ["stable_surface_id", "surface_id"]));
  const obligations = list(first(value, ["obligations", "required_obligations"]))
    .concat(stableId ? [`surface:${stableId}`] : [])
    .filter((item, position, all) => all.indexOf(item) === position);
  const defectShapes = list(first(value, ["defect_shapes", "detected_defect_shapes", "defects"]))
    .concat(first(value, ["classification", "defect_class"]) ? [String(first(value, ["classification", "defect_class"]))] : [])
    .concat(stableId ? [stableId] : [])
    .filter((item, position, all) => all.indexOf(item) === position);
  const boundaries = list(first(value, ["boundaries", "integration_boundaries", "boundary"]));
  const evidence = testEvidenceValues(value).map((item) => normalizeEvidence(item, value));
  const detectedDefectShapes = value?.detected === true || value?.mutation_detected === true || evidence.some((item) => item.status === "detected") ? defectShapes : [];
  const declaredOracle = oracleInfo(value);
  const oracle = declaredOracle.independence_class ? declaredOracle : evidence.find((item) => item.oracle.independence_class)?.oracle || declaredOracle;
  const cost = normalizeCost(value);
  const historical = list(first(value, ["historical_defects", "historical_defect_ids"]));
  return {
    id,
    path: string(first(value, ["path", "test_path", "source_path", "file"])),
    kind: string(first(value, ["kind", "layer", "type"])) || "behavioral",
    obligations: obligations.sort(),
    detected_defect_shapes: detectedDefectShapes.sort(),
    defect_shapes: defectShapes.sort(),
    boundaries: boundaries.sort(),
    historical_defects: historical.sort(),
    oracle,
    mandatory: value?.mandatory === true,
    value_consuming: value?.value_consuming !== false && value?.bound_and_discarded !== true,
    evidence,
    cost: cost.measured ? cost : evidence.find((item) => item.cost.measured)?.cost || cost,
    source_record: string(first(value, ["source_record", "record_path"])),
  };
}

function manifestRows(manifest) {
  if (Array.isArray(manifest?.rows)) return manifest.rows;
  if (Array.isArray(manifest?.denominator?.rows)) return manifest.denominator.rows;
  return [];
}

export function normalizeDenominator(manifest, additional = {}) {
  const rows = manifestRows(manifest);
  const obligations = [];
  const excluded = [];
  const gaps = [];
  const records = [];
  for (const row of rows) {
    const id = string(first(row, ["stable_id", "id", "construct_id"]));
    if (!id) {
      gaps.push({ id: null, reason: "denominator row has no stable identity" });
      continue;
    }
    const tiers = list(first(row, ["applicable_tiers", "tiers", "modes"]));
    const status = String(row.status || "unknown");
    const reason = string(first(row, ["exclusion_reason", "reason"]));
    const isExcluded = status === "excluded" || row.excluded === true;
    const rowRecord = {
      id,
      kind: string(row.kind) || "unknown",
      status,
      tiers,
      value_consuming: row.value_consuming,
      exclusion_reason: reason,
      seed: string(row.seed),
    };
    records.push(rowRecord);
    if (isExcluded) {
      if (!reason) gaps.push({ id, reason: "excluded row has no ratified reason" });
      excluded.push({ id, reason: reason || "missing exclusion reason" });
      continue;
    }
    if (tiers.length === 0) {
      gaps.push({ id, reason: `row status ${status} has no applicable mode` });
      continue;
    }
    if (["missing", "unrouted", "invalid", "refused", "stale", "unknown"].includes(status)) {
      gaps.push({ id, reason: `row is ${status}` });
    }
    for (const tier of tiers) obligations.push(`capability:${id}:${tier}`);
  }
  for (const item of list(additional.mandatory_obligations)) obligations.push(item);
  const uniqueObligations = [...new Set(obligations)].sort();
  return {
    schema: manifest?.schema || null,
    source_snapshot_hash: string(manifest?.source_snapshot?.hash),
    source_manifest_hash: string(additional.manifest_hash),
    rows: records.sort((left, right) => left.id.localeCompare(right.id)),
    obligations: uniqueObligations,
    excluded: excluded.sort((left, right) => left.id.localeCompare(right.id)),
    gaps: gaps.sort((left, right) => String(left.id).localeCompare(String(right.id)) || left.reason.localeCompare(right.reason)),
    all_excluded: rows.length > 0 && uniqueObligations.length === 0,
    counts: {
      rows: rows.length,
      obligations: uniqueObligations.length,
      excluded: excluded.length,
      gaps: gaps.length,
    },
  };
}

function findBundleObjects(value, out = [], seen = new Set()) {
  if (!value || typeof value !== "object" || seen.has(value)) return out;
  seen.add(value);
  if ((value.stable_surface_id || value.mutant_id || value.mutation_id || value.finding_id) && (value.stdout_bytes !== undefined || value.tier_observations || value.bundle_identity)) out.push(value);
  if (Array.isArray(value)) {
    for (const item of value) findBundleObjects(item, out, seen);
  } else {
    for (const child of Object.values(value)) findBundleObjects(child, out, seen);
  }
  return out;
}

function defaultPortfolio(root, hardeningRoot, defectsPath) {
  const records = [];
  const loaded = [];
  for (const path of walkFiles(hardeningRoot).filter((item) => extname(item) === ".json")) {
    try { loaded.push({ path, value: readJson(path) }); } catch { /* unreadable records are reported separately */ }
  }
  const seen = new Set();
  for (const { path, value } of loaded) {
    const bundles = [];
    if (value?.findings && Array.isArray(value.findings)) bundles.push(...value.findings.map((finding) => finding.bundle).filter(Boolean));
    bundles.push(...findBundleObjects(value));
    for (const bundle of bundles) {
      const bundleShape = string(bundle.stable_surface_id) || string(bundle.mutant_id) || string(bundle.mutation_id) || string(bundle.classification) || "unknown";
      const identity = string(bundle.bundle_identity) || digest({
        source_sha256: bundle.source_sha256,
        stable_surface_id: bundle.stable_surface_id,
        mutant_id: bundle.mutant_id,
        mutation_id: bundle.mutation_id,
        tier: bundle.tier,
        run_id: bundle.run_id,
      });
      if (seen.has(identity)) continue;
      seen.add(identity);
      const finding = value.findings?.find((item) => item.bundle === bundle);
      const shape = bundleShape;
      const tiers = list(bundle.applicable_tiers);
      records.push({
        id: string(finding?.finding_id) || `hardening:${shape}:${bundle.tier || "unknown"}`,
        path: relPath(root, path),
        kind: String(bundle.mutation_arm || "hardening"),
        obligations: tiers.map((tier) => `surface:${shape}:${tier}`),
        defect_shapes: [shape, string(bundle.classification)].filter(Boolean),
        boundaries: [string(bundle.tier)].filter(Boolean),
        evidence: [bundle],
        source_record: relPath(root, path),
      });
    }
  }
  if (defectsPath && existsSync(defectsPath)) {
    const defects = readJson(defectsPath);
    for (const defect of Array.isArray(defects) ? defects : []) {
      const id = string(defect.id);
      if (!id) continue;
      records.push({
        id: `historical:${id}`,
        path: string(defect.repro),
        kind: "historical-regression",
        obligations: [`historical:${id}`],
        defect_shapes: [string(defect.kind), id].filter(Boolean),
        boundaries: list(defect.probesAffected),
        historical_defects: [id],
        evidence: [],
        source_record: relPath(root, defectsPath),
      });
    }
  }
  return records;
}

function loadPortfolio(options, root) {
  if (options.portfolio && existsSync(options.portfolio)) {
    const value = readJson(options.portfolio);
    const records = Array.isArray(value) ? value : value.tests || value.records || [];
    return { records, source: relPath(root, options.portfolio) };
  }
  return {
    records: defaultPortfolio(root, options.hardeningRoot, options.defects),
    source: "derived:hardening-records+historical-defects",
  };
}

function historicalDefectObligations(defectsPath) {
  if (!defectsPath || !existsSync(defectsPath)) return [];
  let defects;
  try { defects = readJson(defectsPath); } catch { return []; }
  return (Array.isArray(defects) ? defects : [])
    .map((defect) => string(defect.id))
    .filter(Boolean)
    .map((id) => `historical:${id}`);
}

export function selectCandidate(tests, requested = null) {
  const counts = new Map();
  for (const test of tests) {
    for (const evidence of test.evidence) {
      if (!evidence.candidate) continue;
      const key = candidateKey(evidence.candidate);
      counts.set(key, (counts.get(key) || 0) + 1);
    }
  }
  const keys = [...counts.keys()].sort();
  if (requested) {
    const key = typeof requested === "string" ? requested : candidateKey(requested);
    const match = tests.flatMap((test) => test.evidence).find((evidence) => candidateKey(evidence.candidate) === key);
    return {
      candidate: match?.candidate || null,
      key,
      candidates: keys.map((candidateKeyValue) => ({ key: candidateKeyValue, evidence: counts.get(candidateKeyValue) })),
      error: match ? null : "requested candidate identity has no evidence",
    };
  }
  if (keys.length === 1) {
    const candidate = tests.flatMap((test) => test.evidence).find((evidence) => candidateKey(evidence.candidate) === keys[0])?.candidate;
    return { candidate, key: keys[0], candidates: [{ key: keys[0], evidence: counts.get(keys[0]) }], error: null };
  }
  if (keys.length === 0) return { candidate: null, key: null, candidates: [], error: "no complete frozen candidate identity" };
  return {
    candidate: null,
    key: null,
    candidates: keys.map((candidateKeyValue) => ({ key: candidateKeyValue, evidence: counts.get(candidateKeyValue) })),
    error: "candidate identity is ambiguous; pass --candidate with the frozen identity",
  };
}

function contributionKeys(test) {
  const values = [];
  for (const obligation of test.obligations) values.push(`obligation:${obligation}`);
  for (const shape of test.detected_defect_shapes) values.push(`defect:${shape}`);
  for (const boundary of test.boundaries) values.push(`boundary:${boundary}`);
  if (test.oracle.independence_class) values.push(`oracle:${test.oracle.independence_class}`);
  for (const defect of test.historical_defects) values.push(`historical:${defect}`);
  return [...new Set(values)].sort();
}

function validEvidence(test, candidate) {
  const errors = [];
  const matching = test.evidence.filter((evidence) => !candidate || sameCandidate(evidence.candidate, candidate));
  if (matching.length === 0) return { evidence: null, errors: ["wrong-identity: no evidence for frozen candidate"] };
  const ordered = [...matching].sort((left, right) => String(left.raw.run_id).localeCompare(String(right.raw.run_id)));
  for (const evidence of ordered) errors.push(...evidenceError(evidence, candidate));
  const usable = ordered.find((evidence) => ["pass", "detected"].includes(evidence.status) && evidenceError(evidence, candidate).length === 0);
  if (!usable && errors.length === 0) errors.push("unavailable: no passing or detecting evidence");
  return { evidence: usable || null, errors: [...new Set(errors)] };
}

function costOf(test, evidence) {
  if (test.cost?.measured) return test.cost;
  return evidence?.cost?.measured ? evidence.cost : { wall_ms: null, memory_bytes: null, measured: false };
}

function sumCosts(tests) {
  const costs = tests.map((test) => test.selected_cost || test.cost).filter(Boolean);
  if (costs.length === 0) return { measured: false, wall_ms: null, memory_bytes: null, samples: 0 };
  const complete = costs.every((cost) => cost.measured);
  return {
    measured: complete,
    wall_ms: complete ? costs.reduce((sum, cost) => sum + cost.wall_ms, 0) : null,
    memory_bytes: complete ? costs.reduce((sum, cost) => sum + cost.memory_bytes, 0) : null,
    samples: costs.length,
  };
}

function costRank(test) {
  const cost = test.selected_cost || test.cost || {};
  return cost.measured ? cost.wall_ms + cost.memory_bytes / (1024 * 1024) : Number.POSITIVE_INFINITY;
}

export function selectTests({ denominator, tests, candidate }) {
  const prepared = tests.map((test) => {
    const evidenceResult = validEvidence(test, candidate);
    const selectedCost = evidenceResult.evidence ? costOf(test, evidenceResult.evidence) : { wall_ms: null, memory_bytes: null, measured: false };
    return {
      ...test,
      selected_evidence: evidenceResult.evidence,
      selected_cost: selectedCost,
      evidence_errors: evidenceResult.errors,
      valid: Boolean(evidenceResult.evidence),
      contribution_keys: contributionKeys(test),
    };
  });
  const required = new Set(denominator.obligations.map((item) => `obligation:${item}`));
  for (const test of prepared.filter((item) => item.valid && item.mandatory)) required.add(`test:${test.id}`);
  const independentClasses = [...new Set(prepared.filter((item) => item.valid && item.oracle.independence_class).map((item) => item.oracle.independence_class))].sort();
  for (const oracleClass of independentClasses) required.add(`oracle:${oracleClass}`);
  for (const test of prepared.filter((item) => item.valid)) {
    for (const shape of test.detected_defect_shapes) required.add(`defect:${shape}`);
    for (const defect of test.historical_defects) required.add(`historical:${defect}`);
  }
  const uncovered = new Set(required);
  const selected = [];
  while (uncovered.size > 0) {
    const choices = prepared.filter((test) => test.valid && !selected.includes(test)).map((test) => {
      const gain = [...uncovered].filter((item) => test.contribution_keys.includes(item) || (item === `test:${test.id}`)).length;
      return { test, gain };
    }).filter((choice) => choice.gain > 0);
    if (choices.length === 0) break;
    choices.sort((left, right) => right.gain - left.gain || costRank(left.test) - costRank(right.test) || left.test.id.localeCompare(right.test.id));
    const chosen = choices[0].test;
    selected.push(chosen);
    for (const item of chosen.contribution_keys) uncovered.delete(item);
    uncovered.delete(`test:${chosen.id}`);
  }
  const selectedIds = new Set(selected.map((test) => test.id));
  const selectedContribution = new Set(selected.flatMap((test) => test.contribution_keys));
  const retained = selected.map((test) => test.id).sort();
  const consolidated = [];
  const blocked = [];
  const unavailable = [];
  for (const test of prepared.filter((item) => !selectedIds.has(item.id)).sort((left, right) => left.id.localeCompare(right.id))) {
    if (!test.valid) {
      unavailable.push({
        id: test.id,
        status: "unavailable",
        obligation: test.obligations,
        defect_shapes: test.detected_defect_shapes,
        measured_cost: test.selected_cost,
        replacement_evidence: [],
        reason: test.evidence_errors.length ? test.evidence_errors.join("; ") : "no valid same-candidate evidence",
      });
      continue;
    }
    const uncoveredContribution = test.contribution_keys.filter((item) => !selectedContribution.has(item));
    const replacements = selected.filter((replacement) => test.contribution_keys.every((item) => replacement.contribution_keys.includes(item))).map((replacement) => replacement.id).sort();
    if (uncoveredContribution.length > 0 || replacements.length === 0) {
      blocked.push({
        id: test.id,
        status: "sole-detector",
        obligation: test.obligations,
        defect_shapes: test.detected_defect_shapes,
        measured_cost: test.selected_cost,
        replacement_evidence: replacements,
        reason: `unique contribution is not covered: ${uncoveredContribution.join(", ") || "no independent replacement"}`,
      });
      continue;
    }
    consolidated.push({
      id: test.id,
      status: "consolidated",
      obligation: test.obligations,
      defect_shapes: test.detected_defect_shapes,
      measured_cost: test.selected_cost,
      replacement_evidence: replacements,
      reason: "every obligation, defect shape, boundary, and oracle contribution is detected by the retained evidence",
    });
  }
  const soleDetectorProof = [...required].sort().map((requirement) => ({
    requirement,
    detectors: selected.filter((test) => test.contribution_keys.includes(requirement) || requirement === `test:${test.id}`).map((test) => test.id).sort(),
  }));
  const missing = [...uncovered].sort();
  const before = sumCosts(prepared.filter((test) => test.valid));
  const after = sumCosts(selected);
  const reduction = before.measured && after.measured ? {
    wall_ms: before.wall_ms - after.wall_ms,
    memory_bytes: before.memory_bytes - after.memory_bytes,
    wall_percent: before.wall_ms === 0 ? 0 : (before.wall_ms - after.wall_ms) / before.wall_ms * 100,
    memory_percent: before.memory_bytes === 0 ? 0 : (before.memory_bytes - after.memory_bytes) / before.memory_bytes * 100,
  } : null;
  return {
    prepared,
    retained,
    selected,
    consolidated,
    unavailable,
    blocked,
    required: [...required].sort(),
    missing,
    sole_detector_proof: soleDetectorProof,
    costs: { before, after, reduction },
    deterministic_key: digest({ candidate: candidateKey(candidate), retained, consolidated: consolidated.map((item) => item.id), required: [...required].sort() }),
  };
}

export function parseSuiteInventory(path) {
  if (!path || !existsSync(path)) return { path: path || null, sections: [], targets: [], missing: true };
  const sections = [];
  const targets = [];
  let section = null;
  for (const rawLine of readFileSync(path, "utf8").split(/\r?\n/)) {
    const line = rawLine.trimEnd();
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    const sectionMatch = line.match(/^([A-Za-z0-9_-]+):\s*$/);
    if (sectionMatch) {
      section = sectionMatch[1];
      sections.push(section);
      continue;
    }
    const target = line.trim();
    if (section && target) targets.push({ section, target });
  }
  return {
    path,
    sections: [...new Set(sections)].sort(),
    targets: targets.sort((left, right) => left.target.localeCompare(right.target)),
    missing: false,
  };
}

export function parseSuiteBudgets(path) {
  if (!path || !existsSync(path)) return { path: path || null, rows: [], missing: true };
  const rows = [];
  for (const rawLine of readFileSync(path, "utf8").split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const match = line.match(/^(\S+)\s+(\d+(?:\.\d+)?)\s+(.+)$/);
    if (!match) continue;
    const reason = match[3];
    const measured = reason.match(/measured\s+(\d+(?:\.\d+)?)s/i);
    rows.push({
      target: match[1],
      budget_seconds: Number(match[2]),
      budget_wall_ms: Number(match[2]) * 1000,
      measured_wall_ms: measured ? Number(measured[1]) * 1000 : null,
      reason,
    });
  }
  return { path, rows: rows.sort((left, right) => left.target.localeCompare(right.target)), missing: false };
}

function performanceEvidence(root, resultRoot) {
  const files = walkFiles(resultRoot).filter((path) => extname(path) === ".json");
  const rows = [];
  for (const path of files) {
    let value;
    try { value = readJson(path); } catch { continue; }
    if (value?.contract !== "gauntlet-report-v1") continue;
    const entries = Array.isArray(value.entries) ? value.entries : [];
    for (const entry of entries) {
      const name = string(entry?.entry?.name) || string(entry?.name);
      const metrics = [];
      for (const row of Array.isArray(entry.rows) ? entry.rows : []) {
        for (const [metric, metricValue] of Object.entries(row.metrics || {})) {
          const tier = string(row.tier) || "aot";
          const sample = metricValue?.tiers?.[tier] || metricValue;
          const stats = sample?.stats?.jet || sample?.stats || {};
          const wall = first(stats, ["median", "value", "runtime_wall_seconds"]);
          const memory = first(stats, ["rss", "memory_kb"]);
          if (typeof wall === "number" || typeof memory === "number") metrics.push({ metric, tier, wall_ms: typeof wall === "number" ? wall * 1000 : null, memory_bytes: typeof memory === "number" ? memory * 1024 : null });
        }
      }
      rows.push({ id: name || relPath(root, path), path: relPath(root, path), run_id: value.run_id || null, measured: metrics.length > 0, metrics });
    }
  }
  return rows.sort((left, right) => left.id.localeCompare(right.id));
}

function hardeningStatus(hardeningRoot) {
  const resultPath = join(hardeningRoot, "result.json");
  if (!existsSync(resultPath)) return { status: "unavailable", path: null };
  let result;
  try { result = readJson(resultPath); } catch (error) { return { status: "unavailable", path: relPath(DEFAULT_ROOT, resultPath), error: error.message }; }
  return {
    status: String(result.status || "unknown").toLowerCase(),
    path: relPath(DEFAULT_ROOT, resultPath),
    reason: string(result.refusal?.reason) || string(result.error) || null,
    candidate: normalizeCandidate(result),
    run_id: result.run_id || null,
    valid_cases: result.oracle?.valid_cases ?? null,
    mutation_score: result.mutation?.score ?? null,
    mutation_count: result.mutation?.valid_mutations ?? result.valid_mutations ?? result.mutations_per_callable ?? result.quota?.mutations_per_callable ?? null,
    fresh_context_lanes: result.fresh_context_lanes ?? result.fresh_lanes ?? result.quota?.fresh_context_lanes ?? null,
    clean_days: result.clean_days ?? null,
  };
}
export function qualify({ denominator, selection, candidateSelection, controls = null, hardening = null }) {
  const errors = [];
  if (hardening?.candidate && candidateSelection.candidate && !sameCandidate(hardening.candidate, candidateSelection.candidate)) errors.push("wrong-identity: hardening result is not for frozen candidate");
  if (hardening) {
    if (!["pass", "qualified", "green", "ok"].includes(hardening.status)) {
      errors.push(`hardening gate: status is ${hardening.status}`);
    } else {
      if (!hardening.candidate) errors.push("hardening gate: candidate identity is unavailable");
      if (typeof hardening.clean_days !== "number" || hardening.clean_days < HARDENING_REQUIREMENTS.clean_days) errors.push("hardening gate: clean-day threshold is unavailable or below 14");
      if (typeof hardening.valid_cases !== "number" || hardening.valid_cases < HARDENING_REQUIREMENTS.valid_cases) errors.push("hardening gate: valid-case threshold is unavailable or below 10000000");
      if (typeof hardening.mutation_count !== "number" || hardening.mutation_count < HARDENING_REQUIREMENTS.mutations_per_callable) errors.push("hardening gate: mutation threshold is unavailable or below 100 per callable");
      if (typeof hardening.fresh_context_lanes !== "number" || hardening.fresh_context_lanes < HARDENING_REQUIREMENTS.fresh_context_lanes) errors.push("hardening gate: fresh-context lane quota is unavailable or below 8");
    }
  }
  if (!denominator.schema) errors.push("denominator: canonical hardening manifest is unavailable");
  if (denominator.counts.rows === 0) errors.push("denominator: canonical hardening manifest has no rows");
  if (candidateSelection.error) errors.push(`candidate: ${candidateSelection.error}`);
  if (!candidateSelection.candidate) errors.push("candidate: no frozen same-candidate evidence");
  if (denominator.all_excluded) errors.push("all-excluded: denominator contains no mandatory capability or mode");
  if (denominator.gaps.length > 0) errors.push(`mandatory denominator gaps: ${denominator.gaps.length}`);
  if (selection.prepared.filter((test) => test.valid).length === 0) errors.push("zero-valid: no test has valid same-candidate raw evidence");
  for (const test of selection.prepared.filter((item) => !item.valid && item.evidence_errors.length > 0)) {
    errors.push(`${test.id}: ${test.evidence_errors.join("; ")}`);
  }
  if (selection.missing.length > 0) errors.push(`uncovered obligations or defect shapes: ${selection.missing.length}`);
  for (const item of selection.blocked) errors.push(`sole detector cannot be removed: ${item.id}`);
  for (const test of selection.prepared) {
    if (test.selected_evidence && evidenceError(test.selected_evidence, candidateSelection.candidate).length > 0) errors.push(`${test.id}: invalid selected evidence`);
  }
  if (controls && controls.failed > 0) errors.push(`false-green controls failed to reject: ${controls.failed}`);
  return {
    status: errors.length === 0 ? "PASS" : "BLOCKED",
    errors,
    same_candidate: Boolean(candidateSelection.candidate && selection.prepared.some((test) => test.valid)),
  };
}

function positiveControlFixture() {
  const candidate = {
    commit: "control-commit",
    binary_sha256: "sha256:control-binary",
    registry_snapshot_hash: "sha256:control-registry",
    config_hash: "sha256:control-config",
  };
  const evidence = (overrides = {}) => ({
    run_id: "control-run",
    commit: candidate.commit,
    binary_sha256: candidate.binary_sha256,
    registry_snapshot_hash: candidate.registry_snapshot_hash,
    config_hash: candidate.config_hash,
    tier: "aot",
    tier_command: "scripts/agent/jet-env jet run {source}",
    source_sha256: "sha256:control-source",
    stdout_bytes: "base64:b2s=",
    stderr_bytes: "base64:",
    exit: 0,
    status: "detected",
    expected_relation: "expected",
    actual_relation: "wrong",
    oracle: { name: "control", version: "1", independence_class: "independent-control", input_digest: "sha256:control-input" },
    valid_inputs: 1,
    wall_ms: 10,
    memory_bytes: 1024,
    ...overrides,
  });
  const denominator = normalizeDenominator({
    schema: "jet.hardening.surface.v1",
    source_snapshot: { hash: "sha256:control-source-snapshot" },
    rows: [{
      stable_id: "module:control.case",
      kind: "module_call",
      status: "covered",
      applicable_tiers: ["aot", "interpreter"],
    }],
  }, { mandatory_obligations: ["historical:DEF-SEED"] });
  const tests = [
    normalizeTest({
      id: "expensive-duplicate",
      obligations: ["capability:module:control.case:aot"],
      defect_shapes: ["wrong-answer"],
      evidence: [evidence({ wall_ms: 80, memory_bytes: 200 })],
    }),
    normalizeTest({
      id: "cheap-duplicate",
      obligations: ["capability:module:control.case:aot"],
      defect_shapes: ["wrong-answer"],
      evidence: [evidence({ wall_ms: 10, memory_bytes: 50 })],
    }),
    normalizeTest({
      id: "unique-mode-and-history",
      obligations: ["capability:module:control.case:interpreter", "historical:DEF-SEED"],
      defect_shapes: ["tier-mismatch"],
      historical_defects: ["DEF-SEED"],
      evidence: [evidence({ tier: "interpreter", wall_ms: 10, memory_bytes: 50 })],
    }),
  ];
  return { candidate, denominator, tests, evidence };
}

export function runFalseGreenControls() {
  const base = positiveControlFixture();
  const controls = [];
  const execute = (name, denominator, tests, candidate = base.candidate) => {
    const candidateSelection = selectCandidate(tests, candidateKey(candidate));
    if (!candidateSelection.candidate) {
      candidateSelection.candidate = candidate;
      candidateSelection.error = null;
    }
    const selection = selectTests({ denominator, tests, candidate: candidateSelection.candidate });
    const result = qualify({ denominator, selection, candidateSelection });
    const rejected = result.status !== "PASS";
    controls.push({ name, status: rejected ? "rejected" : "accepted", errors: result.errors });
  };
  execute("zero-valid", base.denominator, [normalizeTest({ id: "zero-valid", obligations: ["capability:module:control.case:aot"], defect_shapes: ["wrong-answer"], evidence: [base.evidence({ valid_inputs: 0 })] })]);
  execute("all-excluded", normalizeDenominator({ schema: "jet.hardening.surface.v1", rows: [{ stable_id: "module:excluded", status: "excluded", applicable_tiers: [], exclusion_reason: "owner-ratified" }] }), base.tests);
  execute("discarded-output", base.denominator, [normalizeTest({ id: "discarded", obligations: ["capability:module:control.case:aot"], defect_shapes: ["wrong-answer"], evidence: [base.evidence({ value_consuming: false, stdout_bytes: null })] })]);
  execute("broken-oracle", base.denominator, [normalizeTest({ id: "broken", obligations: ["capability:module:control.case:aot"], defect_shapes: ["wrong-answer"], evidence: [base.evidence({ oracle: { valid: false, independence_class: "independent-control" } })] })]);
  execute("wrong-identity", base.denominator, [normalizeTest({ id: "wrong-id", obligations: ["capability:module:control.case:aot"], defect_shapes: ["wrong-answer"], evidence: [base.evidence({ commit: "other-commit" })] })]);
  execute("harness-error", base.denominator, [normalizeTest({ id: "harness-error", obligations: ["capability:module:control.case:aot"], defect_shapes: ["wrong-answer"], evidence: [base.evidence({ harness_error: "child exited but harness returned pass" })] })]);
  return { total: controls.length, failed: controls.filter((control) => control.status !== "rejected").length, passed: controls.filter((control) => control.status === "rejected").length, controls };
}

export function buildReport(options = {}) {
  const root = resolve(options.root || DEFAULT_ROOT);
  const manifestPath = pathFor(root, options.manifest || ".jet/hardening-manifest.json");
  const hardeningRoot = pathFor(root, options.hardeningRoot || process.env.JET_HARDENING_CACHE || "~/.cache/jet-hardening/v1".replace(/^~/, process.env.HOME || ""));
  const defectsPath = pathFor(root, options.defects || "tools/agent-eval/domain-foundations/_defects/defects.json");
  const suitesPath = pathFor(root, options.suites || "tests/suites.txt");
  const budgetsPath = pathFor(root, options.budgets || "tests/suite_budgets.txt");
  const manifest = manifestPath && existsSync(manifestPath) ? readJson(manifestPath) : null;
  const manifestHash = manifest ? digest(manifest) : null;
  const denominator = normalizeDenominator(manifest, { manifest_hash: manifestHash, mandatory_obligations: historicalDefectObligations(defectsPath) });
  const portfolio = loadPortfolio({ ...options, hardeningRoot, defects: defectsPath }, root);
  const tests = portfolio.records.map((value, index) => normalizeTest(value, index));
  const candidateSelection = selectCandidate(tests, options.candidate || null);
  const selection = selectTests({ denominator, tests, candidate: candidateSelection.candidate });
  const controls = options.controls === false ? null : runFalseGreenControls();
  const hardening = hardeningStatus(hardeningRoot);
  const qualification = qualify({ denominator, selection, candidateSelection, controls, hardening });
  const performance = options.gauntlet === false ? [] : performanceEvidence(root, pathFor(root, options.gauntletRoot || "gauntlet/results"));
  const suiteInventory = parseSuiteInventory(suitesPath);
  const suiteBudgets = parseSuiteBudgets(budgetsPath);
  const report = {
    schema: SCHEMA,
    schema_version: SCHEMA_VERSION,
    generated_by: "tools/agent-eval/test-economics/test-economics.mjs",
    policy: {
      selection_order: ["mandatory obligations", "unique defect shapes", "independent oracle classes", "measured cost", "stable id"],
      no_count_as_value: true,
      no_hidden_denominator: true,
      same_candidate_required: true,
      runner_sources: [...RUNNER_SOURCES],
      hardening_requirements: clone(HARDENING_REQUIREMENTS),
      release_requirements: {
        same_candidate: true,
        preserve_mandatory_modes: true,
        preserve_unique_defect_shapes: true,
        unavailable_is_not_pass: true,
        raw_evidence_required: true,
      },
    },
    inputs: {
      manifest: manifestPath ? relPath(root, manifestPath) : null,
      manifest_sha256: manifestHash,
      defects_sha256: fileHash(defectsPath),
      suites_sha256: fileHash(suitesPath),
      budgets_sha256: fileHash(budgetsPath),
      hardening_result_sha256: fileHash(join(hardeningRoot, "result.json")),
      portfolio: portfolio.source,
      hardening_root: hardeningRoot ? relPath(root, hardeningRoot) : null,
      defects: defectsPath ? relPath(root, defectsPath) : null,
      suites: suitesPath ? relPath(root, suitesPath) : null,
      budgets: budgetsPath ? relPath(root, budgetsPath) : null,
      gauntlet: options.gauntlet === false ? null : relPath(root, pathFor(root, options.gauntletRoot || "gauntlet/results")),
    },
    candidate: {
      selected: candidateSelection.candidate,
      key: candidateSelection.key,
      candidates: candidateSelection.candidates,
      error: candidateSelection.error,
    },
    denominator,
    suite_inventory: suiteInventory,
    suite_budgets: suiteBudgets,
    hardening,
    performance,
    portfolio: {
      total: tests.length,
      valid_same_candidate: selection.prepared.filter((test) => test.valid).length,
      retained: selection.retained,
      consolidated: selection.consolidated,
      removed: [],
      unavailable: selection.unavailable,
      blocked_sole_detectors: selection.blocked,
      records: selection.prepared.map((test) => ({
        id: test.id,
        path: test.path,
        kind: test.kind,
        obligations: test.obligations,
        detected_defect_shapes: test.detected_defect_shapes,
        boundaries: test.boundaries,
        historical_defects: test.historical_defects,
        measured_cost: test.selected_cost,
        evidence_states: test.evidence.map((evidence) => ({
          status: evidence.status,
          availability: evidence.availability,
          flake: evidence.flake,
          valid_inputs: evidence.valid_inputs,
          run_id: evidence.raw.run_id,
          candidate: evidence.candidate,
          cost: evidence.cost,
        })),
        evidence: test.selected_evidence ? {
          status: test.selected_evidence.status,
          run_id: test.selected_evidence.raw.run_id,
          tier: test.selected_evidence.raw.tier,
          command: test.selected_evidence.raw.command,
          source_sha256: test.selected_evidence.raw.source_sha256,
          candidate: test.selected_evidence.candidate,
          oracle: test.selected_evidence.oracle,
          availability: test.selected_evidence.availability,
          flake: test.selected_evidence.flake,
          valid_inputs: test.selected_evidence.valid_inputs,
          harness_error: test.selected_evidence.harness_error,
          raw: test.selected_evidence.raw,
        } : null,
        evidence_errors: test.evidence_errors,
        selection: selection.retained.includes(test.id) ? "retained" : selection.consolidated.some((item) => item.id === test.id) ? "consolidated" : selection.unavailable.some((item) => item.id === test.id) ? "unavailable" : selection.blocked.some((item) => item.id === test.id) ? "sole-detector" : "omitted",
      })),
    },
    selection: {
      retained: selection.retained,
      required: selection.required,
      missing: selection.missing,
      deterministic_key: selection.deterministic_key,
      sole_detector_proof: selection.sole_detector_proof,
      costs: selection.costs,
    },
    controls,
    qualification,
  };
  report.report_sha256 = digest(report);
  return report;
}

function human(report) {
  const lines = [
    "Test economics",
    `qualification: ${report.qualification.status}`,
    `candidate: ${report.candidate.key || "none"}`,
    `denominator: ${report.denominator.counts.rows} rows, ${report.denominator.counts.obligations} mandatory mode obligations, ${report.denominator.counts.gaps} gaps`,
    `portfolio: ${report.portfolio.total} records; retained ${report.portfolio.retained.length}; consolidated ${report.portfolio.consolidated.length}; unavailable ${report.portfolio.unavailable.length}; sole detectors ${report.portfolio.blocked_sole_detectors.length}`,
    `cost: ${report.selection.costs.reduction ? `${report.selection.costs.reduction.wall_percent.toFixed(2)}% wall / ${report.selection.costs.reduction.memory_percent.toFixed(2)}% memory reduction` : "unmeasured"}`,
    `controls: ${report.controls ? `${report.controls.passed}/${report.controls.total} false-green controls rejected` : "not run"}`,
  ];
  if (report.hardening?.status === "skipped" || report.hardening?.status === "unavailable") {
    lines.push(`hardening: ${report.hardening.status}${report.hardening.reason ? ` (${report.hardening.reason})` : ""}; evidence path ${report.hardening.path || "missing"}`);
  }
  if (report.denominator.counts.gaps > 0) {
    const reasons = new Map();
    for (const gap of report.denominator.gaps) reasons.set(gap.reason, (reasons.get(gap.reason) || 0) + 1);
    const summary = [...reasons.entries()].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
      .slice(0, 5).map(([reason, count]) => `${count} ${reason}`).join(", ");
    lines.push(`denominator gaps: ${summary}${reasons.size > 5 ? `, plus ${reasons.size - 5} other reasons` : ""}`);
  }
  const identityMissing = report.portfolio.unavailable.filter((item) => item.reason?.startsWith("wrong-identity"));
  if (identityMissing.length > 0) {
    const historical = identityMissing.filter((item) => item.id.startsWith("historical:"));
    lines.push(`identity: frozen candidate comes from matching hardening/oracle evidence; ${identityMissing.length} records lack raw evidence for it (${historical.length} historical-defect records); no suite-vs-analyzer identity disagreement is established`);
  }
  for (const error of report.qualification.errors.slice(0, 12)) lines.push(`block: ${error}`);
  return `${lines.join("\n")}\n`;
}

function usage() {
  return [
    "usage: node tools/agent-eval/test-economics/test-economics.mjs [options]",
    "",
    "Options:",
    "  --json                 emit the complete inspectable report",
    "  --portfolio PATH       portfolio records; defaults to current hardening records + defects",
    "  --manifest PATH        canonical hardening manifest",
    "  --hardening-root PATH  existing hardening result directory",
    "  --defects PATH         historical defect records",
    "  --suites PATH           canonical suite partition",
    "  --budgets PATH          committed suite budgets",
    "  --gauntlet-root PATH   existing gauntlet results",
    "  --candidate KEY        frozen candidate key (commit=...|binary_sha256=...|registry_snapshot_hash=...|config_hash=...)",
    "  --no-controls          do not run false-green controls",
    "  --self-test            run positive selection and all false-green controls",
    "  --help                 show this help",
  ].join("\n") + "\n";
}

function parseArgs(argv) {
  const options = { json: false, selfTest: false, controls: true };
  const pathOptions = new Map([
    ["--portfolio", "portfolio"],
    ["--manifest", "manifest"],
    ["--hardening-root", "hardeningRoot"],
    ["--defects", "defects"],
    ["--suites", "suites"],
    ["--budgets", "budgets"],
    ["--gauntlet-root", "gauntletRoot"],
    ["--candidate", "candidate"],
  ]);
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--json") options.json = true;
    else if (token === "--self-test") options.selfTest = true;
    else if (token === "--no-controls") options.controls = false;
    else if (token === "--help" || token === "-h") options.help = true;
    else if (pathOptions.has(token)) {
      const value = argv[++index];
      if (!value) throw new Error(`${token} requires a value`);
      options[pathOptions.get(token)] = value;
    } else throw new Error(`unknown option: ${token}`);
  }
  return options;
}

export function runSelfTest() {
  const base = positiveControlFixture();
  const candidateSelection = selectCandidate(base.tests, candidateKey(base.candidate));
  const selection = selectTests({ denominator: base.denominator, tests: base.tests, candidate: candidateSelection.candidate });
  const qualification = qualify({ denominator: base.denominator, selection, candidateSelection });
  if (qualification.status !== "PASS") throw new Error(`positive selector fixture did not qualify: ${qualification.errors.join("; ")}`);
  const retained = new Set(selection.retained);
  if (!retained.has("cheap-duplicate") || !retained.has("unique-mode-and-history") || retained.has("expensive-duplicate")) {
    throw new Error(`selector did not retain the unique detector and cheaper duplicate: ${selection.retained.join(", ")}`);
  }
  const consolidated = selection.consolidated.find((item) => item.id === "expensive-duplicate");
  if (!consolidated || consolidated.replacement_evidence.join(",") !== "cheap-duplicate") {
    throw new Error("selector did not record the cheaper duplicate as replacement evidence");
  }
  const proof = selection.sole_detector_proof;
  if (proof.some((item) => item.detectors.length === 0)) throw new Error("selector left a mandatory or unique contribution without a detector");
  if (selection.costs.reduction?.wall_percent !== 80 || selection.costs.reduction?.memory_percent !== (200 / 300) * 100) {
    throw new Error(`selector measured reduction changed: ${JSON.stringify(selection.costs.reduction)}`);
  }
  const reversed = selectTests({
    denominator: base.denominator,
    tests: [...base.tests].reverse(),
    candidate: candidateSelection.candidate,
  });
  if (selection.deterministic_key !== reversed.deterministic_key) throw new Error("selector is not deterministic for the same candidate");
  const nearMissDenominator = normalizeDenominator({
    schema: "jet.hardening.surface.v1",
    rows: [{
      stable_id: "module:near-miss.case",
      kind: "module_call",
      status: "covered",
      applicable_tiers: ["aot"],
    }],
  });
  const nearMissTests = [
    normalizeTest({
      id: "near-miss-result-oracle",
      obligations: ["capability:module:near-miss.case:aot"],
      defect_shapes: ["wrong-answer"],
      evidence: [base.evidence({
        oracle: { name: "result", version: "1", independence_class: "independent-result", input_digest: "sha256:result-input" },
      })],
    }),
    normalizeTest({
      id: "near-miss-diagnostic-oracle",
      obligations: ["capability:module:near-miss.case:aot"],
      defect_shapes: ["wrong-answer"],
      evidence: [base.evidence({
        oracle: { name: "diagnostic", version: "1", independence_class: "independent-diagnostic", input_digest: "sha256:diagnostic-input" },
      })],
    }),
  ];
  const nearMissCandidate = selectCandidate(nearMissTests, candidateKey(base.candidate));
  const nearMissSelection = selectTests({
    denominator: nearMissDenominator,
    tests: nearMissTests,
    candidate: nearMissCandidate.candidate,
  });
  if (nearMissSelection.retained.length !== 2 || nearMissSelection.consolidated.length !== 0 || nearMissSelection.blocked.length !== 0) {
    throw new Error("selector removed an independent oracle that shares a defect shape");
  }
  const controls = runFalseGreenControls();
  if (controls.failed !== 0 || controls.passed !== controls.total) throw new Error("false-green control did not fail closed");
  return {
    status: "PASS",
    positive: qualification,
    selection: {
      retained: selection.retained,
      consolidated: selection.consolidated,
      sole_detector_proof: proof,
      costs: selection.costs,
    },
    near_miss: {
      retained: nearMissSelection.retained,
      consolidated: nearMissSelection.consolidated,
      blocked: nearMissSelection.blocked,
      costs: nearMissSelection.costs,
    },
    controls,
  };
}

export function main(argv = process.argv.slice(2)) {
  try {
    const options = parseArgs(argv);
    if (options.help) {
      process.stdout.write(usage());
      return 0;
    }
    if (options.selfTest) {
      const result = runSelfTest();
      process.stdout.write(options.json ? `${JSON.stringify(result, null, 2)}\n` : `TEST ECONOMICS CONTROLS PASS (${result.controls.passed}/${result.controls.total})\n`);
      return 0;
    }
    const report = buildReport(options);
    process.stdout.write(options.json ? `${JSON.stringify(report, null, 2)}\n` : human(report));
    return report.qualification.status === "PASS" ? 0 : 1;
  } catch (error) {
    process.stderr.write(`test economics: ${error.stack || error.message}\n`);
    return 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  process.exitCode = main();
}
