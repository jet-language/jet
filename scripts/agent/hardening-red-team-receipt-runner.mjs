#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  fsyncSync,
  lstatSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { isAbsolute, join, parse, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { bundleIdentity, makeResultBundle } from "./hardening-oracle-layer.mjs";

export const RECEIPT_DIR_ENV = "JET_HARDENING_RED_TEAM_RECEIPT_DIR";
export const RED_TEAM_SCHEMA_VERSION = 1;
export const RED_TEAM_PACKET_SCHEMA = "jet.hardening.red-team.context.v1";
export const RED_TEAM_LANE_SCHEMA = "jet.hardening.red-team.lane.v1";
export const RED_TEAM_MODEL = "gpt-5.6-luna";
export const RED_TEAM_REASONING = "max";

const SHA256_PATTERN = /^sha256:[0-9a-f]{64}$/;
const COMMIT_PATTERN = /^[0-9a-f]{40}$/i;
const LANE_ID_PATTERN = /^lane-[1-8]$/;
const SAFE_LANE_NAME_PATTERN = /^[A-Za-z0-9._-]+$/;
const CONTEXT_ID_PATTERN = /^[A-Za-z0-9_.:/-]{1,160}$/;
const CONSUMED_MARKER_SCHEMA = "jet.hardening.red-team.receipt-consumed.v1";
const CONSUMED_MARKER_PATTERN = /^\.consumed-lane-[1-8]\.json$/;
const SOURCE_KEYS = ["id", "source", "source_sha256", "value_consuming", "observer"];
const CLEANUP_KEYS = ["active_agents", "active_processes", "scratch_paths", "alternate_targets", "unbounded_logs", "complete"];
const ATTEMPT_KEYS = ["id", "program_id", "valid"];
const VALID_CASE_KEYS = ["id", "attempt_id"];
const LIST_KEYS = Object.freeze({
  attempts: ATTEMPT_KEYS,
  valid_cases: VALID_CASE_KEYS,
  duplicates: ["id"],
  false_positives: ["id"],
});
const FINDING_ID_PATTERN = /^[A-Za-z0-9_.:/-]{1,160}$/;
const FINDING_REQUIRED_KEYS = ["finding_id", "severity", "silent_wrong_data", "reproducer_id", "bundle", "bundle_identity"];
const FINDING_OPTIONAL_KEYS = ["priority", "p0", "classification", "default_jet_run_divergence", "load_bearing"];
const ORACLE_KEYS = ["name", "version", "input_digest", "independence_class", "provenance"];
const TIER_OBSERVATION_KEYS = [
  "tier",
  "stdout_bytes",
  "stderr_bytes",
  "exit",
  "signal",
  "timeout",
  "relation",
  "normalized_value",
  "stdout_truncated",
  "stderr_truncated",
];
const TIER_PARITY_KEYS = ["ok", "baseline", "differences"];
const BUNDLE_REQUIRED_KEYS = [
  "schema_version",
  "run_id",
  "stable_surface_id",
  "tier",
  "tier_command",
  "seed",
  "mutation_arm",
  "mutator_version",
  "source",
  "source_sha256",
  "stdout_bytes",
  "stderr_bytes",
  "exit",
  "signal",
  "timeout",
  "expected_relation",
  "actual_relation",
  "normalization",
  "oracle",
  "commit",
  "binary_sha256",
  "registry_snapshot_hash",
  "config_hash",
  "classification",
  "tower_action",
  "tier_observations",
  "applicable_tiers",
  "tier_parity",
];
const BUNDLE_OPTIONAL_KEYS = [
  "layer",
  "law_id",
  "construct_id",
  "mutant_id",
  "seam",
  "expected_layer",
  "precondition",
  "generated_partition",
  "shrink_rule",
  "relation",
  "generated_partitions",
  "type_constraints",
  "observable_sink",
  "ast_mutation",
  "proof",
  "mutated_source",
  "mutated_source_sha256",
];
const LANE_FILE_PATTERN = /^lane-[1-8]\.json$/;
const COUNT_KEYS = [
  "source_programs",
  "attempts",
  "valid_cases",
  "duplicates",
  "false_positives",
  "minimized_reproducers",
  "unique_findings",
];
const TARGET_KEYS = [
  "root",
  "commit",
  "binary_path",
  "binary_sha256",
  "platform",
  "arch",
];
const RECEIPT_TARGET_KEYS = [...TARGET_KEYS, "registry_snapshot", "public_surface_snapshot"];
const PACKET_REQUIRED_KEYS = [
  "schema",
  "schema_version",
  "session_id",
  "manifest_sha256",
  "lane_id",
  "wave",
  "attack_surface",
  "brief",
  "mission",
  "target",
  "public_surface_snapshot",
  "agent_policy",
  "rig_config_sha256",
  "oracle_versions",
  "quota",
  "resource_limits",
  "visibility",
  "forbidden_inputs",
  "context_digest",
];
const RECEIPT_KEYS = [
  "schema",
  "schema_version",
  "session_id",
  "packet_digest",
  "lane_id",
  "wave",
  "attack_surface",
  "brief",
  "context_id",
  "agent_id",
  "model",
  "reasoning_effort",
  "fresh_context",
  "known_defects_visible",
  "target",
  "started_at",
  "finished_at",
  "complete",
  "stopped_early",
  "semantic_change",
  "registry_changed",
  "cleanup",
  "source_programs",
  "attempts",
  "valid_cases",
  "duplicates",
  "false_positives",
  "minimized_reproducers",
  "unique_findings",
  "counts",
];

export class ReceiptRunnerError extends Error {
  constructor(message, code = "E_RECEIPT_RUNNER") {
    super(message);
    this.name = "ReceiptRunnerError";
    this.code = code;
  }
}

function fail(message, code = "E_RECEIPT_RUNNER") {
  throw new ReceiptRunnerError(message, code);
}

function requiredString(value, label) {
  if (typeof value !== "string" || !value.trim()) fail(`${label} must be a non-empty string`, "E_SCHEMA");
  return value;
}

function requiredObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`, "E_SCHEMA");
  return value;
}

function validDigest(value) {
  return typeof value === "string" && SHA256_PATTERN.test(value);
}

function digest(value) {
  return `sha256:${createHash("sha256").update(Buffer.from(String(value), "utf8")).digest("hex")}`;
}

function canonicalValue(value) {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(canonicalValue);
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalValue(value[key])]));
}

export function canonicalJson(value) {
  return JSON.stringify(canonicalValue(value));
}

function assertExactKeys(value, requiredKeys, label, optionalKeys = []) {
  const expected = new Set([...requiredKeys, ...optionalKeys]);
  for (const key of requiredKeys) {
    if (!Object.hasOwn(value, key)) fail(`${label} is missing ${key}`, "E_SCHEMA");
  }
  for (const key of Object.keys(value)) {
    if (!expected.has(key)) fail(`${label} has unexpected field ${key}`, "E_SCHEMA");
  }
}

function assertRelativePath(value, label) {
  requiredString(value, label);
  if (isAbsolute(value) || value.split(/[\\/]/).includes("..")) fail(`${label} must stay inside the frozen checkout`, "E_SCHEMA");
}

function validateSnapshot(snapshot, label) {
  requiredObject(snapshot, label);
  assertExactKeys(snapshot, ["path", "sha256"], label, ["source_snapshot_hash"]);
  assertRelativePath(snapshot.path, `${label}.path`);
  if (!validDigest(snapshot.sha256)) fail(`${label}.sha256 is invalid`, "E_SCHEMA");
  if (Object.hasOwn(snapshot, "source_snapshot_hash") && !validDigest(snapshot.source_snapshot_hash)) {
    fail(`${label}.source_snapshot_hash is invalid`, "E_SCHEMA");
  }
  return Object.hasOwn(snapshot, "source_snapshot_hash")
    ? { path: snapshot.path, sha256: snapshot.sha256, source_snapshot_hash: snapshot.source_snapshot_hash }
    : { path: snapshot.path, sha256: snapshot.sha256 };
}

function validateBaseTarget(target, label) {
  requiredObject(target, label);
  assertExactKeys(target, TARGET_KEYS, label);
  requiredString(target.root, `${label}.root`);
  if (!COMMIT_PATTERN.test(target.commit || "")) fail(`${label}.commit is not a full commit hash`, "E_SCHEMA");
  assertRelativePath(target.binary_path, `${label}.binary_path`);
  if (!validDigest(target.binary_sha256)) fail(`${label}.binary_sha256 is invalid`, "E_SCHEMA");
  requiredString(target.platform, `${label}.platform`);
  requiredString(target.arch, `${label}.arch`);
}

function validatePacket(packet) {
  requiredObject(packet, "context packet");
  assertExactKeys(packet, PACKET_REQUIRED_KEYS, "context packet");
  if (packet.schema !== RED_TEAM_PACKET_SCHEMA || packet.schema_version !== RED_TEAM_SCHEMA_VERSION) {
    fail("invalid red-team context packet schema", "E_PACKET");
  }
  requiredString(packet.session_id, "context packet session_id");
  if (!validDigest(packet.manifest_sha256)) fail("context packet manifest digest is invalid", "E_PACKET");
  if (!LANE_ID_PATTERN.test(packet.lane_id || "") || !SAFE_LANE_NAME_PATTERN.test(packet.lane_id || "")) {
    fail("context packet lane_id is invalid", "E_PACKET");
  }
  if (!Number.isInteger(packet.wave) || packet.wave < 1 || packet.wave > 4) fail("context packet wave is invalid", "E_PACKET");
  requiredString(packet.attack_surface, "context packet attack_surface");
  requiredString(packet.brief, "context packet brief");
  validateBaseTarget(packet.target, "context packet target");
  validateSnapshot(packet.public_surface_snapshot, "context packet public_surface_snapshot");
  requiredObject(packet.agent_policy, "context packet agent_policy");
  if (String(packet.agent_policy.model || "").toLowerCase() !== RED_TEAM_MODEL
    || packet.agent_policy.reasoning_effort !== RED_TEAM_REASONING
    || packet.agent_policy.fresh_context !== true) {
    fail("context packet agent policy is not fresh Luna-max", "E_PACKET");
  }
  if (!validDigest(packet.rig_config_sha256)) fail("context packet rig config digest is invalid", "E_PACKET");
  requiredObject(packet.oracle_versions, "context packet oracle_versions");
  requiredObject(packet.quota, "context packet quota");
  if (packet.quota.lanes !== 8 || packet.quota.waves !== 4 || packet.quota.lanes_per_wave !== 2
    || packet.quota.full_quota_required !== true) fail("context packet quota is invalid", "E_PACKET");
  requiredObject(packet.resource_limits, "context packet resource_limits");
  requiredObject(packet.visibility, "context packet visibility");
  if (packet.visibility.current_defect_cards !== "hidden" || packet.visibility.known_findings !== "omitted") {
    fail("context packet exposes current defect cards", "E_PACKET");
  }
  if (!Array.isArray(packet.forbidden_inputs)) fail("context packet forbidden_inputs must be an array", "E_PACKET");
  for (const forbidden of ["defect_cards", "known_defects", "known_findings", "tower_cards"]) {
    if (Object.hasOwn(packet, forbidden)) fail(`context packet contains forbidden ${forbidden}`, "E_PACKET");
  }
  if (!validDigest(packet.context_digest) || packet.context_digest !== contextPacketDigest(packet)) {
    fail("context packet digest changed", "E_PACKET");
  }
  return packet;
}

function contextPacketDigest(packet) {
  const unsigned = { ...packet };
  delete unsigned.context_digest;
  return digest(canonicalJson(unsigned));
}

function stripComments(source) {
  return String(source)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/[^\n]*/g, "");
}

function isValueConsumingSource(source) {
  if (typeof source !== "string" || !source.trim()) return false;
  const code = stripComments(source);
  return [...code.matchAll(/\b(?:print|write|assert|panic)\s*\(([^)\n]*)\)/g)].some((match) => {
    const argument = match[1].trim();
    if (!argument || /^(?:true|false|null|[-+]?\d+(?:\.\d+)?|"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')$/.test(argument)) return false;
    return /[A-Za-z_][A-Za-z0-9_.]*|\(|\[|[+\-*\/%<>=!&|]/.test(argument);
  });
}

function exactRecord(value, keys, label, optionalKeys = []) {
  requiredObject(value, label);
  assertExactKeys(value, keys, label, optionalKeys);
  return Object.fromEntries([...keys, ...optionalKeys]
    .filter((key) => Object.hasOwn(value, key))
    .map((key) => [key, value[key]]));
}

function validateSourceRecord(value, index, label) {
  const output = exactRecord(value, SOURCE_KEYS, `${label}[${index}]`);
  const source = requiredString(output.source, `${label}[${index}].source`);
  if (output.value_consuming !== true || !isValueConsumingSource(source)) {
    fail(`${label}[${index}] is not a value-consuming program`, "E_LANE");
  }
  const observer = requiredString(output.observer, `${label}[${index}].observer`);
  if (observer !== "value observer" && !stripComments(source).includes(observer)) {
    fail(`${label}[${index}] observer is not present in source`, "E_LANE");
  }
  const sourceSha = digest(source);
  if (output.source_sha256 !== sourceSha) fail(`${label}[${index}] source digest does not match`, "E_LANE");
  requiredString(output.id, `${label}[${index}].id`);
  return output;
}

function validateListRecord(value, index, label, keys) {
  const output = exactRecord(value, keys, `${label}[${index}]`);
  requiredString(output.id, `${label}[${index}].id`);
  if (label === "attempts") {
    requiredString(output.program_id, `${label}[${index}].program_id`);
    if (typeof output.valid !== "boolean") fail(`${label}[${index}].valid must be boolean`, "E_LANE");
  }
  if (label === "valid_cases") requiredString(output.attempt_id, `${label}[${index}].attempt_id`);
  return output;
}
function ensureUnique(ids, label) {
  const seen = new Set();
  for (const id of ids) {
    if (seen.has(id)) fail(`${label} repeats id ${id}`, "E_LANE");
    seen.add(id);
  }
}

function validateCounts(counts, receipt) {
  requiredObject(counts, "lane receipt counts");
  assertExactKeys(counts, COUNT_KEYS, "lane receipt counts");
  for (const key of COUNT_KEYS) {
    if (!Number.isInteger(counts[key]) || counts[key] < 0 || counts[key] !== receipt[key].length) {
      fail(`lane receipt count ${key} is not an evidence count`, "E_LANE");
    }
  }
  return Object.fromEntries(COUNT_KEYS.map((key) => [key, counts[key]]));
}
function validateCleanup(cleanup) {
  requiredObject(cleanup, "lane receipt cleanup");
  assertExactKeys(cleanup, CLEANUP_KEYS, "lane receipt cleanup");
  if (cleanup.active_agents !== 0 || cleanup.active_processes !== 0) {
    fail("lane receipt cleanup still has active work", "E_CLEANUP");
  }
  if (!Array.isArray(cleanup.scratch_paths) || cleanup.scratch_paths.length !== 0
    || !Array.isArray(cleanup.alternate_targets) || cleanup.alternate_targets.length !== 0) {
    fail("lane receipt cleanup retains scratch or alternate targets", "E_CLEANUP");
  }
  if (cleanup.unbounded_logs !== false || cleanup.complete !== true) {
    fail("lane receipt cleanup is incomplete", "E_CLEANUP");
  }
  return {
    active_agents: 0,
    active_processes: 0,
    scratch_paths: [],
    alternate_targets: [],
    unbounded_logs: false,
    complete: true,
  };
}


function validateTierObservation(value, index, label) {
  const output = exactRecord(value, TIER_OBSERVATION_KEYS, `${label}[${index}]`);
  requiredString(output.tier, `${label}[${index}].tier`);
  requiredString(output.stdout_bytes, `${label}[${index}].stdout_bytes`);
  requiredString(output.stderr_bytes, `${label}[${index}].stderr_bytes`);
  if (output.exit !== null && !Number.isInteger(output.exit)) fail(`${label}[${index}].exit is invalid`, "E_LANE");
  if (output.signal !== null && typeof output.signal !== "string") fail(`${label}[${index}].signal is invalid`, "E_LANE");
  if (typeof output.timeout !== "boolean" || typeof output.relation !== "string"
    || typeof output.stdout_truncated !== "boolean" || typeof output.stderr_truncated !== "boolean") {
    fail(`${label}[${index}] has an invalid observation field`, "E_LANE");
  }
  return output;
}

function validateTierParity(value, label) {
  if (value === null) return null;
  const output = exactRecord(value, TIER_PARITY_KEYS, label);
  if (typeof output.ok !== "boolean") fail(`${label}.ok is invalid`, "E_LANE");
  requiredString(output.baseline, `${label}.baseline`);
  if (!Array.isArray(output.differences) || output.differences.some((item) => typeof item !== "string")) {
    fail(`${label}.differences is invalid`, "E_LANE");
  }
  return output;
}

function validateBundle(value, label) {
  const bundle = exactRecord(value, BUNDLE_REQUIRED_KEYS, label, BUNDLE_OPTIONAL_KEYS);
  const oracle = exactRecord(bundle.oracle, ORACLE_KEYS, `${label}.oracle`);
  for (const key of ORACLE_KEYS) requiredString(oracle[key], `${label}.oracle.${key}`);
  if (!Array.isArray(bundle.tier_observations)) fail(`${label}.tier_observations must be an array`, "E_LANE");
  bundle.tier_observations = bundle.tier_observations.map((item, index) => (
    validateTierObservation(item, index, `${label}.tier_observations`)
  ));
  bundle.tier_parity = validateTierParity(bundle.tier_parity, `${label}.tier_parity`);
  let normalized;
  try {
    normalized = makeResultBundle(bundle);
  } catch (error) {
    fail(`${label} is not a valid result bundle: ${error.message}`, "E_LANE");
  }
  if (canonicalJson(normalized) !== canonicalJson(bundle)) {
    fail(`${label} is not canonical`, "E_LANE");
  }
  return normalized;
}

function validateFinding(value, index, packet, registryHash, reproducerSet) {
  const label = `unique_findings[${index}]`;
  const finding = exactRecord(value, FINDING_REQUIRED_KEYS, label, FINDING_OPTIONAL_KEYS);
  requiredString(finding.finding_id, `${label}.finding_id`);
  if (!FINDING_ID_PATTERN.test(finding.finding_id)) fail(`${label}.finding_id is invalid`, "E_LANE");
  if (!/^P[0-3]$/.test(finding.severity) || typeof finding.silent_wrong_data !== "boolean") {
    fail(`${label} severity fields are invalid`, "E_LANE");
  }
  requiredString(finding.reproducer_id, `${label}.reproducer_id`);
  if (!reproducerSet.has(finding.reproducer_id)) fail(`${label} has no minimized reproducer`, "E_LANE");
  for (const key of ["priority", "classification"]) {
    if (Object.hasOwn(finding, key)) requiredString(finding[key], `${label}.${key}`);
  }
  for (const key of ["p0", "default_jet_run_divergence", "load_bearing"]) {
    if (Object.hasOwn(finding, key) && typeof finding[key] !== "boolean") fail(`${label}.${key} is invalid`, "E_LANE");
  }
  const bundle = validateBundle(finding.bundle, `${label}.bundle`);
  if (bundle.commit !== packet.target.commit || bundle.binary_sha256 !== packet.target.binary_sha256) {
    fail(`${label} targets a different frozen binary`, "E_LANE");
  }
  if (bundle.registry_snapshot_hash !== registryHash) fail(`${label} targets a different registry snapshot`, "E_LANE");
  const identity = bundleIdentity(bundle);
  if (finding.bundle_identity !== identity) fail(`${label}.bundle_identity does not match bundle`, "E_LANE");
  return { ...finding, bundle };
}
function validateLaneReceipt(receipt, packet) {
  requiredObject(receipt, "lane receipt");
  if (Object.hasOwn(receipt, "status")) {
    fail("lane receipt status is not part of the terminal lane schema", "E_LANE");
  }
  assertExactKeys(receipt, RECEIPT_KEYS, "lane receipt");
  if (receipt.schema !== RED_TEAM_LANE_SCHEMA || receipt.schema_version !== RED_TEAM_SCHEMA_VERSION) {
    fail("invalid red-team lane receipt schema", "E_LANE");
  }
  if (receipt.session_id !== packet.session_id) fail("lane receipt belongs to another session", "E_LANE");
  if (receipt.packet_digest !== packet.context_digest) fail("lane receipt did not use its independent context packet", "E_LANE");
  if (receipt.lane_id !== packet.lane_id || !LANE_ID_PATTERN.test(receipt.lane_id || "")) {
    fail("lane receipt lane_id does not match packet", "E_LANE");
  }
  if (receipt.wave !== packet.wave || receipt.attack_surface !== packet.attack_surface || receipt.brief !== packet.brief) {
    fail(`lane ${packet.lane_id} does not attest its frozen attack slice`, "E_LANE");
  }
  if (!CONTEXT_ID_PATTERN.test(receipt.context_id || "")) fail("lane receipt context_id is invalid", "E_LANE");
  requiredString(receipt.agent_id, "lane receipt agent_id");
  if (String(receipt.model || "").toLowerCase() !== RED_TEAM_MODEL || receipt.reasoning_effort !== RED_TEAM_REASONING
    || receipt.fresh_context !== true || receipt.known_defects_visible !== false) {
    fail(`lane ${packet.lane_id} is not a fresh hidden-card Luna-max receipt`, "E_LANE");
  }
  for (const key of ["started_at", "finished_at"]) {
    requiredString(receipt[key], `lane receipt ${key}`);
    if (Number.isNaN(Date.parse(receipt[key]))) fail(`lane receipt ${key} is not an ISO timestamp`, "E_LANE");
  }
  if (Date.parse(receipt.finished_at) < Date.parse(receipt.started_at)) fail("lane receipt finished before it started", "E_LANE");
  for (const key of ["complete", "stopped_early", "semantic_change", "registry_changed"]) {
    if (typeof receipt[key] !== "boolean") fail(`lane receipt ${key} must be boolean`, "E_LANE");
  }
  if (receipt.complete !== true || receipt.stopped_early === true) fail(`lane ${packet.lane_id} stopped before completing`, "E_LANE");
  if (receipt.semantic_change || receipt.registry_changed) fail(`lane ${packet.lane_id} is stale`, "E_LANE");

  requiredObject(receipt.target, "lane receipt target");
  assertExactKeys(receipt.target, RECEIPT_TARGET_KEYS, "lane receipt target");
  const baseTarget = Object.fromEntries(TARGET_KEYS.map((key) => [key, receipt.target[key]]));
  validateBaseTarget(baseTarget, "lane receipt target");
  for (const key of TARGET_KEYS) {
    if (receipt.target[key] !== packet.target[key]) fail(`lane ${packet.lane_id} target does not match packet`, "E_LANE");
  }
  const registrySnapshot = validateSnapshot(receipt.target.registry_snapshot, "lane receipt target.registry_snapshot");
  const publicSurfaceSnapshot = validateSnapshot(receipt.target.public_surface_snapshot, "lane receipt target.public_surface_snapshot");
  if (canonicalJson(publicSurfaceSnapshot) !== canonicalJson(packet.public_surface_snapshot)) {
    fail(`lane ${packet.lane_id} public surface does not match packet`, "E_LANE");
  }
  const target = { ...baseTarget, registry_snapshot: registrySnapshot, public_surface_snapshot: publicSurfaceSnapshot };

  for (const key of ["source_programs", "attempts", "valid_cases", "duplicates", "false_positives", "minimized_reproducers", "unique_findings"]) {
    if (!Array.isArray(receipt[key])) fail(`lane receipt ${key} must be an array`, "E_LANE");
  }
  if (!receipt.source_programs.length || !receipt.attempts.length || !receipt.valid_cases.length) {
    fail(`lane ${packet.lane_id} has no value-consuming quota evidence`, "E_LANE");
  }
  const sourcePrograms = receipt.source_programs.map((value, index) => validateSourceRecord(value, index, "source_programs"));
  ensureUnique(sourcePrograms.map((value) => value.id), `lane ${packet.lane_id} source programs`);
  const attempts = receipt.attempts.map((value, index) => validateListRecord(value, index, "attempts", LIST_KEYS.attempts));
  ensureUnique(attempts.map((value) => value.id), `lane ${packet.lane_id} attempts`);
  const sourceSet = new Set(sourcePrograms.map((value) => value.id));
  for (const [index, attempt] of attempts.entries()) {
    if (!sourceSet.has(attempt.program_id)) fail(`lane ${packet.lane_id} attempt ${index} has an unknown source`, "E_LANE");
  }
  const attemptSet = new Set(attempts.map((value) => value.id));
  const validCases = receipt.valid_cases.map((value, index) => validateListRecord(value, index, "valid_cases", LIST_KEYS.valid_cases));
  ensureUnique(validCases.map((value) => value.id), `lane ${packet.lane_id} valid cases`);
  for (const [index, validCase] of validCases.entries()) {
    if (!attemptSet.has(validCase.attempt_id)) fail(`lane ${packet.lane_id} valid case ${index} has an unknown attempt`, "E_LANE");
  }
  const duplicates = receipt.duplicates.map((value, index) => validateListRecord(value, index, "duplicates", LIST_KEYS.duplicates));
  const falsePositives = receipt.false_positives.map((value, index) => validateListRecord(value, index, "false_positives", LIST_KEYS.false_positives));
  ensureUnique(duplicates.map((value) => value.id), `lane ${packet.lane_id} duplicates`);
  ensureUnique(falsePositives.map((value) => value.id), `lane ${packet.lane_id} false positives`);
  const reproducers = receipt.minimized_reproducers.map((value, index) => validateSourceRecord(value, index, "minimized_reproducers"));
  ensureUnique(reproducers.map((value) => value.id), `lane ${packet.lane_id} reproducers`);
  const reproducerSet = new Set(reproducers.map((value) => value.id));
  const uniqueFindings = receipt.unique_findings.map((value, index) => (
    validateFinding(value, index, packet, registrySnapshot.sha256, reproducerSet)
  ));
  const findingIds = new Set();
  const findingBundles = new Set();
  for (const finding of uniqueFindings) {
    if (findingIds.has(finding.finding_id) || findingBundles.has(finding.bundle_identity)) {
      fail(`lane ${packet.lane_id} repeats a unique finding`, "E_LANE");
    }
    findingIds.add(finding.finding_id);
    findingBundles.add(finding.bundle_identity);
  }
  const normalized = {
    ...receipt,
    target,
    cleanup: validateCleanup(receipt.cleanup),
    source_programs: sourcePrograms,
    attempts,
    valid_cases: validCases,
    duplicates,
    false_positives: falsePositives,
    minimized_reproducers: reproducers,
    unique_findings: uniqueFindings,
  };
  normalized.counts = validateCounts(receipt.counts, normalized);
  return Object.fromEntries(RECEIPT_KEYS.map((key) => [key, normalized[key]]));
}

function assertNoDuplicateJsonKeys(text) {
  let index = 0;
  let depth = 0;
  const skipWhitespace = () => {
    while (index < text.length && /\s/.test(text[index])) index += 1;
  };
  const stringToken = () => {
    const start = index;
    if (text[index] !== '"') fail("JSON contains an invalid string", "E_JSON");
    index += 1;
    let escaped = false;
    while (index < text.length) {
      const char = text[index++];
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === '"') {
        return text.slice(start, index);
      }
    }
    fail("JSON contains an unterminated string", "E_JSON");
  };
  const value = () => {
    skipWhitespace();
    const char = text[index];
    if (char === '"') {
      stringToken();
      return;
    }
    if (char === "{") {
      object();
      return;
    }
    if (char === "[") {
      array();
      return;
    }
    const start = index;
    while (index < text.length && !/[\s,\]}]/.test(text[index])) index += 1;
    if (index === start) fail("JSON contains an invalid value", "E_JSON");
  };
  const object = () => {
    if (++depth > 512) fail("JSON nesting is too deep", "E_JSON");
    index += 1;
    skipWhitespace();
    const keys = new Set();
    if (text[index] === "}") {
      index += 1;
      depth -= 1;
      return;
    }
    while (index < text.length) {
      skipWhitespace();
      const token = stringToken();
      let key;
      try {
        key = JSON.parse(token);
      } catch {
        fail("JSON contains an invalid object key", "E_JSON");
      }
      if (keys.has(key)) fail(`JSON contains duplicate object field ${key}`, "E_JSON");
      keys.add(key);
      skipWhitespace();
      if (text[index++] !== ":") fail("JSON object is missing a colon", "E_JSON");
      value();
      skipWhitespace();
      if (text[index] === "}") {
        index += 1;
        depth -= 1;
        return;
      }
      if (text[index++] !== ",") fail("JSON object is missing a comma", "E_JSON");
    }
    fail("JSON object is unterminated", "E_JSON");
  };
  const array = () => {
    if (++depth > 512) fail("JSON nesting is too deep", "E_JSON");
    index += 1;
    skipWhitespace();
    if (text[index] === "]") {
      index += 1;
      depth -= 1;
      return;
    }
    while (index < text.length) {
      value();
      skipWhitespace();
      if (text[index] === "]") {
        index += 1;
        depth -= 1;
        return;
      }
      if (text[index++] !== ",") fail("JSON array is missing a comma", "E_JSON");
    }
    fail("JSON array is unterminated", "E_JSON");
  };
  value();
  skipWhitespace();
  if (index !== text.length) fail("JSON contains trailing data", "E_JSON");
}

function parseJson(text, label) {
  if (typeof text !== "string" || !text.trim()) fail(`${label} is empty`, "E_JSON");
  try {
    assertNoDuplicateJsonKeys(text);
    return JSON.parse(text);
  } catch (error) {
    if (error instanceof ReceiptRunnerError) throw error;
    fail(`${label} is not valid JSON: ${error.message}`, "E_JSON");
  }
}

function assertNoSymlinkComponents(path) {
  const absolute = resolve(path);
  const root = parse(absolute).root;
  let current = root;
  for (const component of absolute.slice(root.length).split(sep).filter(Boolean)) {
    current = join(current, component);
    try {
      if (lstatSync(current).isSymbolicLink()) fail(`receipt path contains symlink component: ${current}`, "E_PATH");
    } catch (error) {
      if (error instanceof ReceiptRunnerError) throw error;
      if (error.code === "ENOENT") break;
      throw error;
    }
  }
}

function receiptDirectory(configured, cwd) {
  if (typeof configured !== "string" || !configured.trim()) {
    fail(`${RECEIPT_DIR_ENV} must name an explicit receipt directory`, "E_CONFIG");
  }
  const requested = resolve(cwd, configured);
  assertNoSymlinkComponents(requested);
  let stat;
  try {
    stat = lstatSync(requested);
  } catch (error) {
    fail(`receipt directory is unavailable: ${requested}`, "E_PATH");
  }
  if (stat.isSymbolicLink() || !stat.isDirectory()) fail(`receipt directory is not a regular directory: ${requested}`, "E_PATH");
  const canonical = realpathSync(requested);
  assertNoSymlinkComponents(canonical);
  return canonical;
}

function selectReceiptFile(directory, laneId) {
  const expectedName = `${laneId}.json`;
  const entries = readdirSync(directory, { withFileTypes: true });
  const names = new Set();
  for (const entry of entries) {
    const candidate = join(directory, entry.name);
    const stat = lstatSync(candidate);
    if (stat.isSymbolicLink()) fail(`receipt directory contains symlink: ${entry.name}`, "E_PATH");
    if (!stat.isFile()) fail(`receipt directory contains non-file data: ${entry.name}`, "E_PATH");
    if (!LANE_FILE_PATTERN.test(entry.name)) fail(`receipt directory contains unexpected receipt file: ${entry.name}`, "E_PATH");
    if (names.has(entry.name)) fail(`receipt directory contains duplicate receipt file: ${entry.name}`, "E_PATH");
    names.add(entry.name);
  }
  if (!names.has(expectedName)) fail(`missing pre-produced lane receipt: ${expectedName}`, "E_RECEIPT");
  const candidate = resolve(directory, expectedName);
  const rel = relative(directory, candidate);
  if (!rel || rel.startsWith(`..${sep}`) || rel === ".." || isAbsolute(rel)) fail("lane receipt path escapes receipt directory", "E_PATH");
  assertNoSymlinkComponents(candidate);
  return candidate;
}

function readReceiptFile(path) {
  const noFollow = constants.O_NOFOLLOW || 0;
  let fd;
  try {
    fd = openSync(path, constants.O_RDONLY | noFollow);
    const stat = fstatSync(fd);
    if (!stat.isFile()) fail(`lane receipt is not a regular file: ${path}`, "E_PATH");
    return readFileSync(fd, "utf8");
  } catch (error) {
    if (error instanceof ReceiptRunnerError) throw error;
    fail(`cannot read lane receipt ${path}: ${error.message}`, "E_RECEIPT");
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

export function loadLaneReceipt(packet, { receipt_dir = undefined, cwd = process.cwd() } = {}) {
  validatePacket(packet);
  const directory = receiptDirectory(receipt_dir ?? process.env[RECEIPT_DIR_ENV], cwd);
  const path = selectReceiptFile(directory, packet.lane_id);
  const receipt = parseJson(readReceiptFile(path), `lane receipt ${path}`);
  return validateLaneReceipt(receipt, packet);
}

export function runReceiptRunner(packetText, options = {}) {
  const packet = parseJson(packetText, "context packet");
  return loadLaneReceipt(packet, options);
}

export function main() {
  try {
    const receipt = runReceiptRunner(readFileSync(0, "utf8"));
    process.stdout.write(`${canonicalJson(receipt)}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`hardening-red-team-receipt-runner: ${error.message}\n`);
    return 1;
  }
}

const invokedPath = process.argv[1] && resolve(process.argv[1]);
const modulePath = resolve(fileURLToPath(import.meta.url));
if (invokedPath === modulePath) process.exitCode = main();
