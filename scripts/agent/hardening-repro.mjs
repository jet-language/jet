#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";

export const REPRO_SCHEMA = "jet.hardening.repro.v1";
export const REPRO_SCHEMA_VERSION = 1;

function canonical(value) {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(canonical);
  return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
}

export function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

export function sha256(value) {
  const bytes = Buffer.isBuffer(value) || value instanceof Uint8Array
    ? value
    : Buffer.from(String(value), "utf8");
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function requiredString(value, name) {
  if (typeof value !== "string" || value.length === 0) throw new Error(`repro ${name} is required`);
  return value;
}

function rawBytes(value, name) {
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) return Buffer.from(value);
  if (typeof value === "string") return Buffer.from(value, "utf8");
  throw new Error(`repro ${name} must be a string or byte array`);
}

function base64(bytes) {
  return Buffer.from(bytes).toString("base64");
}

function decodeBase64(value, name) {
  if (typeof value !== "string" || !/^[A-Za-z0-9+/]*={0,2}$/.test(value) || value.length % 4 === 1) {
    throw new Error(`repro ${name} is not valid base64`);
  }
  const bytes = Buffer.from(value, "base64");
  if (bytes.toString("base64") !== value) throw new Error(`repro ${name} is not canonical base64`);
  return bytes;
}

function commands(value) {
  const rows = Array.isArray(value)
    ? value
    : Object.entries(value || {}).map(([tier, command]) => ({ tier, command }));
  const out = rows.map((row) => ({
    tier: requiredString(row?.tier, "tier command tier"),
    command: requiredString(row?.command, "tier command"),
  }));
  out.sort((left, right) => left.tier.localeCompare(right.tier) || left.command.localeCompare(right.command));
  const seen = new Set();
  for (const row of out) {
    if (seen.has(row.tier)) throw new Error(`repro has duplicate tier command: ${row.tier}`);
    seen.add(row.tier);
  }
  if (out.length === 0) throw new Error("repro tier commands are required");
  return out;
}

function oracle(value) {
  if (!value || typeof value !== "object") throw new Error("repro oracle is required");
  return {
    name: requiredString(value.name, "oracle name"),
    version: requiredString(String(value.version ?? ""), "oracle version"),
    input_digest: requiredString(value.input_digest, "oracle input digest"),
    independence_class: requiredString(value.independence_class, "oracle independence class"),
  };
}

function copyRelation(value, name) {
  if (value === undefined || value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value ?? null;
  }
  try {
    return JSON.parse(JSON.stringify(value));
  } catch {
    throw new Error(`repro ${name} is not JSON-serializable`);
  }
}

export function makeReproBundle(input = {}) {
  const source = requiredString(input.source, "source");
  const stdout = rawBytes(input.stdout_bytes ?? input.stdout ?? "", "stdout_bytes");
  const stderr = rawBytes(input.stderr_bytes ?? input.stderr ?? "", "stderr_bytes");
  const bundle = {
    schema: REPRO_SCHEMA,
    schema_version: REPRO_SCHEMA_VERSION,
    run_id: requiredString(input.run_id, "run_id"),
    started: requiredString(input.started, "started"),
    finished: requiredString(input.finished, "finished"),
    commit: requiredString(input.commit ?? input.jet_commit, "commit"),
    binary_sha256: requiredString(input.binary_sha256, "binary_sha256"),
    host: requiredString(input.host, "host"),
    target: requiredString(input.target, "target"),
    registry_snapshot_hash: requiredString(input.registry_snapshot_hash, "registry snapshot hash"),
    config_hash: requiredString(input.config_hash, "config hash"),
    stable_surface_id: requiredString(input.stable_surface_id, "stable surface id"),
    tier_commands: commands(input.tier_commands),
    seed: requiredString(input.seed, "seed"),
    mutation_arm: requiredString(input.mutation_arm, "mutation arm"),
    mutator_version: requiredString(input.mutator_version, "mutator version"),
    source,
    source_sha256: sha256(source),
    stdout_encoding: "base64",
    stdout_bytes: base64(stdout),
    stderr_encoding: "base64",
    stderr_bytes: base64(stderr),
    exit: input.exit === undefined ? null : input.exit,
    signal: input.signal === undefined ? null : input.signal,
    timeout: Boolean(input.timeout),
    expected_relation: copyRelation(input.expected_relation, "expected relation"),
    actual_relation: copyRelation(input.actual_relation, "actual relation"),
    normalization: Array.isArray(input.normalization) ? [...input.normalization] : [],
    oracle: oracle(input.oracle),
    classification: requiredString(input.classification, "classification"),
    tower_action: input.tower_action && typeof input.tower_action === "object"
      ? JSON.parse(JSON.stringify(input.tower_action))
      : (() => { throw new Error("repro Tower action is required"); })(),
  };
  const result = validateReproBundle(bundle);
  if (!result.ok) throw new Error(result.errors.join("\n"));
  return bundle;
}

export function validateReproBundle(bundle, { currentRegistrySnapshotHash } = {}) {
  const errors = [];
  if (!bundle || typeof bundle !== "object") return { ok: false, stale: false, errors: ["repro bundle is not an object"] };
  if (bundle.schema !== REPRO_SCHEMA) errors.push(`repro schema must be ${REPRO_SCHEMA}`);
  if (bundle.schema_version !== REPRO_SCHEMA_VERSION) errors.push(`repro schema_version must be ${REPRO_SCHEMA_VERSION}`);
  for (const field of [
    "run_id", "started", "finished", "commit", "binary_sha256", "host", "target",
    "registry_snapshot_hash", "config_hash", "stable_surface_id", "seed", "mutation_arm",
    "mutator_version", "source", "source_sha256", "classification",
  ]) {
    if (typeof bundle[field] !== "string" || bundle[field].length === 0) {
      errors.push(`repro ${field} is missing or not a string`);
    }
  }
  for (const field of ["expected_relation", "actual_relation"]) {
    if (!Object.prototype.hasOwnProperty.call(bundle, field)) errors.push(`repro ${field} is missing`);
  }
  if (bundle.source_sha256 !== sha256(bundle.source || "")) errors.push("repro source hash does not match source");
  for (const field of ["stdout_bytes", "stderr_bytes"]) {
    try { decodeBase64(bundle[field], field); } catch (error) { errors.push(error.message); }
  }
  try { commands(bundle.tier_commands); } catch (error) { errors.push(error.message); }
  try { oracle(bundle.oracle); } catch (error) { errors.push(error.message); }
  if (!Array.isArray(bundle.normalization)) errors.push("repro normalization must be an array");
  if (!Object.prototype.hasOwnProperty.call(bundle, "tower_action") || !bundle.tower_action || typeof bundle.tower_action !== "object") {
    errors.push("repro Tower action is required");
  }
  if (bundle.exit !== null && (!Number.isInteger(bundle.exit) || bundle.exit < 0)) errors.push("repro exit must be a non-negative integer or null");
  if (bundle.signal !== null && typeof bundle.signal !== "string") errors.push("repro signal must be a string or null");
  const stale = currentRegistrySnapshotHash !== undefined
    && bundle.registry_snapshot_hash !== currentRegistrySnapshotHash;
  if (stale) errors.push("repro registry snapshot is stale");
  return { ok: errors.length === 0, stale, errors };
}

export function reconstructReproBundle(bundle, tier = null) {
  const result = validateReproBundle(bundle);
  if (!result.ok) throw new Error(result.errors.join("\n"));
  const selected = tier === null
    ? bundle.tier_commands
    : bundle.tier_commands.filter((row) => row.tier === tier);
  if (tier !== null && selected.length !== 1) throw new Error(`repro has no unique command for tier ${tier}`);
  return {
    source: bundle.source,
    source_sha256: bundle.source_sha256,
    tier_commands: selected.map((row) => ({ ...row })),
    stdout: decodeBase64(bundle.stdout_bytes, "stdout_bytes"),
    stderr: decodeBase64(bundle.stderr_bytes, "stderr_bytes"),
    exit: bundle.exit,
    signal: bundle.signal,
    timeout: bundle.timeout,
  };
}

export function replayCommand(bundle, tier, sourcePath) {
  const reconstructed = reconstructReproBundle(bundle, tier);
  if (!sourcePath) throw new Error("replay source path is required");
  return reconstructed.tier_commands[0].command.replaceAll("{source}", sourcePath);
}

export function readReproBundle(path, options = {}) {
  if (!existsSync(path)) throw new Error(`unreadable repro bundle: ${path}`);
  let bundle;
  try {
    bundle = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`unreadable repro bundle ${path}: ${error.message}`);
  }
  const result = validateReproBundle(bundle, options);
  if (!result.ok) throw new Error(result.errors.join("\n"));
  return bundle;
}

function hostileFixtures() {
  const valid = makeReproBundle({
    run_id: "run-1",
    started: "2026-08-29T00:00:00Z",
    finished: "2026-08-29T00:00:01Z",
    commit: "deadbeef",
    binary_sha256: sha256("jet"),
    host: "test-host",
    target: "x86_64-unknown-linux-gnu",
    registry_snapshot_hash: sha256("registry"),
    config_hash: sha256("config"),
    stable_surface_id: "module:core.math.sqrt",
    tier_commands: [{ tier: "jet_run", command: "jet run {source}" }],
    seed: "seed-1",
    mutation_arm: "boundary-min",
    mutator_version: "value-mutator-1",
    source: "print(1)\n",
    stdout_bytes: Buffer.from([0, 255, 10]),
    stderr_bytes: "",
    exit: 0,
    expected_relation: "one",
    actual_relation: "one",
    normalization: [],
    oracle: { name: "law", version: "1", input_digest: sha256("input"), independence_class: "law-only" },
    classification: "pass",
    tower_action: { card: 2335, action: "none" },
  });
  const reconstructed = reconstructReproBundle(valid, "jet_run");
  if (reconstructed.stdout[1] !== 255 || replayCommand(valid, "jet_run", "case.jet") !== "jet run case.jet") {
    throw new Error("repro reconstruction lost exact bytes or command");
  }
  const stale = validateReproBundle(valid, { currentRegistrySnapshotHash: sha256("new-registry") });
  if (stale.ok || !stale.stale) throw new Error("stale repro snapshot was accepted");
  const missing = validateReproBundle({ ...valid, binary_sha256: "" });
  if (missing.ok) throw new Error("invalid repro bundle was accepted");
  console.log("hardening repro hostile fixtures: PASS");
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const args = new Set(process.argv.slice(2));
  process.exitCode = args.has("--hostile-fixtures") ? hostileFixtures() : 0;
}
