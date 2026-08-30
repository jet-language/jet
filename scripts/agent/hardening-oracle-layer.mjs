#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { validateManifest } from "./hardening-manifest.mjs";

/*
 * Layer-1 wrong-answer machinery (#2337).
 *
 * This module is deliberately a library, not a test runner. The bounded rig
 * owns process execution; this file owns the value contract, independent
 * relations, stable batching, and reconstructible records.
 */

export const SCHEMA_VERSION = 1;
export const MUTATOR_VERSION = "value-mutator-1";
export const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);
export const DEFAULT_BATCH_SIZE = 32;
export const MAX_BATCH_SIZE = 512;
export const DEFAULT_CORPUS_LIMIT = 4096;
export const MAX_SOURCE_BYTES = 512 * 1024;
export const DEFAULT_TIMEOUT_MS = 30_000;
export const MAX_TIMEOUT_MS = 10 * 60 * 1000;
export const MAX_CAPTURE_BYTES = 256 * 1024;
export const MUTATION_ARMS = Object.freeze([
  "boundary-min",
  "boundary-max",
  "negative",
  "empty",
  "unicode",
]);

const ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const DEFAULT_JET_ENV = join(ROOT, "scripts/agent/jet-env");
const MANIFEST_SCHEMA = "jet.hardening.surface.v1";

function canonicalValue(value) {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(canonicalValue);
  return Object.fromEntries(
    Object.keys(value).sort().map((key) => [key, canonicalValue(value[key])]),
  );
}

export function canonicalJson(value) {
  return JSON.stringify(canonicalValue(value));
}

export function sha256(value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(String(value), "utf8");
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function equalValue(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function exactRelation(expected, actual) {
  return { ok: equalValue(expected, actual), expected, actual };
}

function decimalAdd(left, right) {
  const parse = (value) => {
    const text = String(value);
    const match = text.match(/^(-?)(\d+)(?:\.(\d+))?$/);
    if (!match) throw new Error(`invalid decimal witness: ${text}`);
    const scale = match[3]?.length || 0;
    return { scale, units: BigInt(`${match[1] === "-" ? "-" : ""}${match[2]}${match[3] || ""}`) };
  };
  const a = parse(left);
  const b = parse(right);
  const scale = Math.max(a.scale, b.scale);
  const units = a.units * 10n ** BigInt(scale - a.scale) + b.units * 10n ** BigInt(scale - b.scale);
  const negative = units < 0n;
  const digits = (negative ? -units : units).toString().padStart(scale + 1, "0");
  if (scale === 0) return `${negative ? "-" : ""}${digits}`;
  return `${negative ? "-" : ""}${digits.slice(0, -scale)}.${digits.slice(-scale)}`.replace(/\.0+$/, "").replace(/(\.\d*?)0+$/, "$1");
}

function shapeRelation(expected, actual) {
  const ok = Boolean(actual)
    && actual.length === expected.length
    && actual.hyphen === expected.hyphen
    && actual.version === expected.version;
  return { ok, expected, actual };
}

function adapter({
  id,
  oracle,
  independence_class,
  provenance = "hardening-layer1-v1",
  input,
  reference,
  wrong,
  relation = exactRelation,
  normalization = [],
  normalizer = null,
  trustworthy = true,
}) {
  return Object.freeze({
    id,
    oracle,
    version: "1",
    independence_class,
    provenance,
    input: Object.freeze(clone(input)),
    reference,
    wrong,
    relation,
    normalization: Object.freeze([...normalization]),
    normalizer,
    trustworthy,
  });
}

/*
 * These are intentionally small, named witnesses. Values come from laws,
 * published vectors, or isolated host fixtures; none calls Jet or wraps the
 * implementation under test.
 */
const ADAPTERS = Object.freeze({
  numeric: adapter({
    id: "numeric",
    oracle: "numeric-algebra-laws",
    independence_class: "algebraic-law",
    input: { a: 7, b: 3, c: 2 },
    reference: ({ a, b, c }) => a * b + c,
    wrong: ({ a, b, c }) => a * b + c + 1,
  }),
  float: adapter({
    id: "float",
    oracle: "ieee754-known-value",
    independence_class: "published-vector",
    input: { expression: "0.1 + 0.2" },
    reference: () => 0.30000000000000004,
    wrong: () => 0.3,
  }),
  text_unicode: adapter({
    id: "text_unicode",
    oracle: "unicode-scalar-and-nfc-law",
    independence_class: "published-law",
    input: { value: "e\u0301" },
    reference: ({ value }) => ({ scalars: Array.from(value).length, nfc: value.normalize("NFC") }),
    wrong: () => ({ scalars: 1, nfc: "e" }),
  }),
  bytes_encoding_serde: adapter({
    id: "bytes_encoding_serde",
    oracle: "rfc-4648-base64-vector",
    independence_class: "published-vector",
    input: { value: "Hello" },
    reference: () => "SGVsbG8=",
    wrong: () => "SGVsbG8",
  }),
  regex: adapter({
    id: "regex",
    oracle: "regex-common-subset-law",
    independence_class: "published-law",
    input: { pattern: "[Jj]et", value: "Jet jet go" },
    reference: () => ["Jet", "jet"],
    wrong: () => ["Jet"],
  }),
  time: adapter({
    id: "time",
    oracle: "rfc-3339-epoch-vector",
    independence_class: "published-vector",
    provenance: "#2288-rfc3339-vector;tzdata=2026c",
    input: { value: "2024-03-01T12:00:00Z" },
    reference: () => 1709294400,
    wrong: () => 1709294401,
  }),
  crypto: adapter({
    id: "crypto",
    oracle: "sha256-empty-known-answer",
    independence_class: "published-vector",
    input: { algorithm: "SHA-256", value: "" },
    reference: () => "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    wrong: () => "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b856",
  }),
  rng_uuid: adapter({
    id: "rng_uuid",
    oracle: "uuid-shape-laws",
    independence_class: "algebraic-law",
    input: { length: 36, hyphen: true, version: 4 },
    reference: ({ length, hyphen, version }) => ({ length, hyphen, version }),
    wrong: () => ({ length: 35, hyphen: false, version: 4 }),
    relation: shapeRelation,
    normalization: ["uuid.random_bytes"],
  }),
  host_io: adapter({
    id: "host_io",
    oracle: "isolated-fixture-exact-bytes",
    independence_class: "peer-process",
    input: { fixture: "uppercase", value: "alpha\n" },
    reference: () => "ALPHA\n",
    wrong: () => "alpha\n",
  }),
  protocol: adapter({
    id: "protocol",
    oracle: "local-protocol-state-law",
    independence_class: "state-model",
    input: { messages: ["SYN", "ACK", "DATA", "FIN"] },
    reference: ({ messages }) => messages,
    wrong: ({ messages }) => [messages[0], messages[2], messages[3]],
  }),
  concurrency: adapter({
    id: "concurrency",
    oracle: "deterministic-schedule-model",
    independence_class: "state-model",
    input: { trace: ["A1", "B1", "A2", "B2"] },
    reference: ({ trace }) => trace,
    wrong: ({ trace }) => [trace[0], trace[1], trace[3], trace[2]],
  }),
  memory: adapter({
    id: "memory",
    oracle: "ownership-state-model",
    independence_class: "state-model",
    input: { operations: ["alloc", "write:42", "read", "free"] },
    reference: () => ({ read: 42, freed: true }),
    wrong: () => ({ read: 0, freed: true }),
  }),
  compiler_reflection: adapter({
    id: "compiler_reflection",
    oracle: "structured-compiler-golden",
    independence_class: "blessed-golden",
    input: { source: "f(1)", shape: "call" },
    reference: () => ({ kind: "call", args: 1, return_type: "Int" }),
    wrong: () => ({ kind: "call", args: 1 }),
  }),
});

const EXTENDED_ADAPTERS = Object.freeze({
  collections: adapter({
    id: "collections",
    oracle: "collection-algebra-laws",
    independence_class: "algebraic-law",
    input: { value: [[1, 2], [3]], index: 1 },
    reference: ({ value, index }) => ({ length: value.length, selected: value[index] }),
    wrong: ({ value }) => ({ length: value.length, selected: value[0] }),
    normalization: ["collections.order-preserving"],
  }),
  numeric_decimal: adapter({
    id: "numeric_decimal",
    oracle: "numeric-decimal-laws",
    independence_class: "algebraic-law",
    input: { left: "7.25", right: "3.50" },
    reference: ({ left, right }) => decimalAdd(left, right),
    wrong: () => "10.76",
    normalization: ["decimal.exact"],
  }),
  json_codable: adapter({
    id: "json_codable",
    oracle: "json-codable-roundtrip-laws",
    independence_class: "published-law",
    input: { value: { id: 7, nested: { enabled: true }, values: [1, 2] } },
    reference: ({ value }) => value,
    wrong: ({ value }) => ({ ...value, nested: {} }),
    normalization: ["json.canonical"],
  }),
});

const ALL_ADAPTERS = Object.freeze({ ...ADAPTERS, ...EXTENDED_ADAPTERS });
const LEGACY_ADAPTER_IDS = Object.freeze(Object.keys(ADAPTERS));

const DOMAIN_ALIASES = Object.freeze({
  collection: "collections",
  list: "collections",
  map: "collections",
  set: "collections",
  sequence: "collections",
  decimal: "numeric_decimal",
  numeric_decimal: "numeric_decimal",
  math: "numeric",
  json: "json_codable",
  codable: "json_codable",
  serde_json: "json_codable",
  text: "text_unicode",
  unicode: "text_unicode",
  bytes: "bytes_encoding_serde",
  encoding: "bytes_encoding_serde",
  serde: "bytes_encoding_serde",
  uuid: "rng_uuid",
  rng: "rng_uuid",
  files: "host_io",
  process: "host_io",
  env: "host_io",
  network: "protocol",
  db: "protocol",
  tasks: "concurrency",
  views: "memory",
  compiler: "compiler_reflection",
  reflection: "compiler_reflection",
});

export function canonicalDomain(domain) {
  const value = DOMAIN_ALIASES[domain] || domain;
  if (!Object.hasOwn(ALL_ADAPTERS, value)) throw new Error(`unknown hardening domain: ${domain}`);
  return value;
}

export function oracleAdapter(domain) {
  return ALL_ADAPTERS[canonicalDomain(domain)];
}

export function oracleCatalog() {
  return Object.values(ALL_ADAPTERS).map((item) => ({
    domain: item.id,
    oracle: item.oracle,
    version: item.version,
    independence_class: item.independence_class,
    provenance: item.provenance,
    normalization: [...item.normalization],
    trustworthy: item.trustworthy,
  }));
}

export function checkAdapter(domain, candidate) {
  const item = oracleAdapter(domain);
  const expected = item.reference(item.input);
  const actual = typeof candidate === "function" ? candidate(item.input) : candidate;
  const relation = item.relation(expected, actual);
  return {
    domain: item.id,
    oracle: item.oracle,
    independence_class: item.independence_class,
    provenance: item.provenance,
    ...relation,
  };
}

export function checkAllAdapters() {
  return LEGACY_ADAPTER_IDS.map((id) => {
    const item = ALL_ADAPTERS[id];
    const result = checkAdapter(item.id, item.wrong);
    if (result.ok) throw new Error(`planted wrong answer survived ${item.id}`);
    return { ...result, ok: true };
  });
}

export function checkAllDomainAdapters() {
  return Object.values(ALL_ADAPTERS).map((item) => {
    const result = checkAdapter(item.id, item.wrong);
    if (result.ok) throw new Error(`planted wrong answer survived ${item.id}`);
    return { ...result, ok: true };
  });
}

function requireString(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value;
}

function requireTierList(value, label) {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`${label} must name at least one tier`);
  }
  const tiers = [...new Set(value)];
  if (tiers.length !== value.length || tiers.some((tier) => !TIERS.includes(tier))) {
    throw new Error(`${label} contains an invalid or duplicate tier`);
  }
  return tiers;
}

function validateStableId(stableId, kind) {
  const prefixes = {
    module_call: "module:",
    receiver_method: "receiver:",
    field: "field:",
    type: "type:",
  };
  if (!Object.hasOwn(prefixes, kind)) throw new Error(`unknown surface kind: ${kind}`);
  if (!stableId.startsWith(prefixes[kind]) || stableId.length <= prefixes[kind].length) {
    throw new Error(`${kind} has malformed stable_id: ${stableId}`);
  }
}

function validateExclusion(exclusion, stableId) {
  if (!exclusion || typeof exclusion !== "object") {
    throw new Error(`exclusion for ${stableId} must name an owner decision and reason`);
  }
  requireString(exclusion.reason, `exclusion reason for ${stableId}`);
  requireString(exclusion.owner_decision, `exclusion owner_decision for ${stableId}`);
  return {
    reason: exclusion.reason,
    owner_decision: exclusion.owner_decision,
  };
}

function normalizeSurfaceRow(row, seen) {
  if (!row || typeof row !== "object" || Array.isArray(row)) {
    throw new Error("surface manifest row must be an object");
  }
  const stableId = requireString(row.stable_id, "surface stable_id");
  if (seen.has(stableId)) throw new Error(`duplicate surface stable_id: ${stableId}`);
  seen.add(stableId);
  const kind = requireString(row.kind, `kind for ${stableId}`);
  validateStableId(stableId, kind);
  const applicableTiers = requireTierList(row.applicable_tiers, `tiers for ${stableId}`);
  const exclusion = row.exclusion == null ? null : validateExclusion(row.exclusion, stableId);
  if (exclusion) {
    return {
      stable_id: stableId,
      kind,
      applicable_tiers: applicableTiers,
      exclusion,
    };
  }
  requireString(row.domain, `domain for ${stableId}`);
  requireString(row.seed, `seed for ${stableId}`);
  if (row.value_consuming !== true) {
    throw new Error(`executable surface ${stableId} is not value-consuming`);
  }
  return {
    stable_id: stableId,
    kind,
    domain: canonicalDomain(row.domain),
    applicable_tiers: applicableTiers,
    seed: row.seed,
    value_consuming: true,
    normalization: Array.isArray(row.normalization) ? [...row.normalization].sort() : [],
    observable_sink: row.observable_sink || "source-defined value-consuming sink",
    mutator: row.mutator || MUTATOR_VERSION,
    batch_size: row.batch_size || DEFAULT_BATCH_SIZE,
  };
}

function requireManifestArtifact(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw new Error("hardening manifest artifact must be an object");
  }
  if (manifest.schema !== MANIFEST_SCHEMA) {
    throw new Error(`hardening manifest schema must be ${MANIFEST_SCHEMA}`);
  }
  if (manifest.schema_version !== 1) {
    throw new Error("hardening manifest schema_version must be 1");
  }
  if (!manifest.source_snapshot || typeof manifest.source_snapshot !== "object") {
    throw new Error("hardening manifest source_snapshot is missing");
  }
  requireString(manifest.source_snapshot.hash, "hardening manifest source snapshot hash");
  if (!manifest.denominator || typeof manifest.denominator !== "object") {
    throw new Error("hardening manifest denominator is missing");
  }
  if (!manifest.denominator.source_ids || typeof manifest.denominator.source_ids !== "object") {
    throw new Error("hardening manifest denominator source_ids is missing");
  }
  if (!Array.isArray(manifest.rows)) throw new Error("hardening manifest rows are missing");
  const validation = validateManifest(manifest);
  if (!validation.ok) throw new Error(validation.errors.join("; "));
  const denominator = {};
  for (const [kind, values] of Object.entries(manifest.denominator.source_ids)) {
    if (!Array.isArray(values)) throw new Error(`hardening manifest denominator ${kind} is not an array`);
    denominator[kind] = [...values];
  }
  const denominatorIds = new Set(Object.values(denominator).flat());
  if (denominatorIds.size !== manifest.rows.length) {
    throw new Error("hardening manifest denominator does not cover rows exactly once");
  }
  for (const row of manifest.rows) {
    if (!denominator[row.kind]?.includes(row.stable_id)) {
      throw new Error(`hardening manifest row is outside denominator: ${row.stable_id}`);
    }
    if (!["covered", "excluded", "missing", "unrouted", "invalid", "invalid-exclusion"].includes(row.status)) {
      throw new Error(`hardening manifest row has unknown status: ${row.stable_id}`);
    }
    if (row.status === "covered") {
      requireString(row.domain, `domain for ${row.stable_id}`);
      requireString(row.seed, `seed for ${row.stable_id}`);
      if (row.value_consuming !== true || row.sink?.type_aware !== true) {
        throw new Error(`covered manifest row is not value-consuming: ${row.stable_id}`);
      }
    }
  }
  return manifest;
}

export function readManifestArtifact(path) {
  requireString(path, "hardening manifest path");
  if (!existsSync(path)) throw new Error(`hardening manifest is missing: ${path}`);
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`hardening manifest is unreadable: ${path}: ${error.message}`);
  }
  return requireManifestArtifact(manifest);
}

export const readHardeningManifest = readManifestArtifact;

function oracleForRow(row) {
  if (!row.domain) return null;
  try {
    const item = oracleAdapter(row.domain);
    if (!item.trustworthy) return null;
    return {
      kind: "external-oracle",
      name: item.oracle,
      version: item.version,
      input_digest: sha256(canonicalJson({ domain: row.domain, seed: row.seed })),
      independence_class: item.independence_class,
      provenance: item.provenance,
      normalization: [...item.normalization],
    };
  } catch {
    return null;
  }
}

function selfDiffOracle(row) {
  return {
    kind: "tier-self-diff",
    name: "aot-vs-jet_run-vs-interpreter",
    version: "1",
    input_digest: sha256(canonicalJson({ domain: row.domain || null, seed: row.seed || null })),
    independence_class: "tier-self-diff",
    provenance: "hardening-layer1-tier-execution-v1",
    normalization: Array.isArray(row.normalization) ? [...row.normalization].sort() : [],
  };
}

function catalogRow(row) {
  const executable = row.status === "covered";
  const external = executable ? oracleForRow(row) : null;
  const oracle = executable ? external || selfDiffOracle(row) : null;
  const rejection = executable
    ? null
    : {
        status: row.status,
        reason: row.exclusion?.reason || row.errors?.join("; ") || `manifest row is ${row.status}`,
        owner_decision: row.exclusion?.decision || row.exclusion?.owner || null,
      };
  return {
    stable_surface_id: row.stable_id,
    stable_id: row.stable_id,
    kind: row.kind,
    owner: row.owner || null,
    member: row.member || null,
    domain: row.domain || null,
    seed: row.seed || null,
    applicable_tiers: [...(row.applicable_tiers || [])],
    projections: clone(row.projections || []),
    dispatcher_arms: [...(row.dispatcher_arms || [])],
    membership_sources: [...(row.membership_sources || [])].sort(),
    value_consuming: row.value_consuming === true,
    sink: row.sink ? clone(row.sink) : null,
    status: row.status,
    executable,
    valid: executable,
    rejection,
    tier_self_diff: Boolean(oracle?.kind === "tier-self-diff" || executable),
    external_oracle: Boolean(external),
    oracle,
  };
}

function catalogFromManifest(manifest) {
  requireManifestArtifact(manifest);
  const rows = manifest.rows.map(catalogRow).sort((left, right) => left.stable_id.localeCompare(right.stable_id));
  const sourceIds = clone(manifest.denominator.source_ids);
  const counts = Object.fromEntries(Object.entries(sourceIds).map(([kind, ids]) => [kind, ids.length]));
  const statusCounts = Object.fromEntries(["covered", "excluded", "missing", "unrouted", "invalid", "invalid-exclusion"]
    .map((status) => [status, rows.filter((row) => row.status === status).length]));
  return {
    schema: SCHEMA_VERSION,
    source_schema: MANIFEST_SCHEMA,
    source_snapshot_hash: manifest.source_snapshot.hash,
    manifest_hash: sha256(canonicalJson(manifest)),
    generated_by: "hardening-oracle-layer",
    denominator: {
      source_ids: sourceIds,
      counts,
      total: Object.values(counts).reduce((sum, count) => sum + count, 0),
    },
    counts: {
      ...statusCounts,
      executable: statusCounts.covered,
      valid: statusCounts.covered,
      unclassified: 0,
    },
    rows,
    exclusions: statusCounts.excluded,
    executable: statusCounts.covered,
  };
}

function legacyCatalog(surfaceRows, sourceSnapshotHash) {
  if (!Array.isArray(surfaceRows)) throw new Error("surface manifest must contain rows");
  requireString(sourceSnapshotHash, "source snapshot hash");
  const seen = new Set();
  const normalized = surfaceRows
    .map((row) => normalizeSurfaceRow(row, seen))
    .sort((left, right) => left.stable_id.localeCompare(right.stable_id));
  const rows = normalized.map((row) => ({
    ...row,
    stable_surface_id: row.stable_id,
    status: row.exclusion ? "excluded" : "covered",
    executable: !row.exclusion,
    valid: !row.exclusion,
    rejection: row.exclusion ? { status: "excluded", reason: row.exclusion.reason, owner_decision: row.exclusion.owner_decision } : null,
    tier_self_diff: !row.exclusion,
    external_oracle: !row.exclusion,
    oracle: row.exclusion ? null : oracleForRow(row) || selfDiffOracle(row),
  }));
  const source_ids = Object.fromEntries(["module_call", "receiver_method", "field", "nominal_type"].map((kind) => [
    kind,
    rows.filter((row) => row.kind === kind).map((row) => row.stable_id),
  ]));
  return {
    schema: SCHEMA_VERSION,
    source_schema: "legacy-surface-rows",
    source_snapshot_hash: sourceSnapshotHash,
    manifest_hash: sha256(canonicalJson({ sourceSnapshotHash, rows })),
    generated_by: "hardening-oracle-layer",
    denominator: {
      source_ids,
      counts: Object.fromEntries(Object.entries(source_ids).map(([kind, ids]) => [kind, ids.length])),
      total: rows.length,
    },
    counts: { covered: rows.filter((row) => row.executable).length, excluded: rows.filter((row) => row.exclusion).length, executable: rows.filter((row) => row.executable).length, valid: rows.filter((row) => row.executable).length, unclassified: 0 },
    rows,
    exclusions: rows.filter((row) => row.exclusion).length,
    executable: rows.filter((row) => row.executable).length,
  };
}

export function buildOracleCatalog(manifestOrRows, sourceSnapshotHash) {
  if (typeof manifestOrRows === "string") return catalogFromManifest(readManifestArtifact(resolve(manifestOrRows)));
  if (manifestOrRows && typeof manifestOrRows === "object" && !Array.isArray(manifestOrRows)) {
    return catalogFromManifest(manifestOrRows);
  }
  // Kept for callers of the original library API. New callers must pass the
  // validated producer artifact so denominator membership cannot be invented.
  return legacyCatalog(manifestOrRows, sourceSnapshotHash);
}

export function readSurfaceManifest(path) {
  requireString(path, "surface manifest path");
  if (!existsSync(path)) throw new Error(`surface manifest is missing: ${path}`);
  const raw = readFileSync(path, "utf8");
  let value;
  try {
    value = JSON.parse(raw);
  } catch (error) {
    throw new Error(`surface manifest is unreadable: ${path}: ${error.message}`);
  }
  if (value && !Array.isArray(value) && value.schema === MANIFEST_SCHEMA) {
    const manifest = requireManifestArtifact(value);
    return { manifest, rows: manifest.rows, source_snapshot_hash: manifest.source_snapshot.hash };
  }
  const rows = Array.isArray(value) ? value : value && value.rows;
  if (!Array.isArray(rows)) throw new Error("surface manifest must be an array or validated hardening manifest");
  return { rows, source_snapshot_hash: sha256(raw) };
}

function isIdentifierChar(char) {
  return Boolean(char) && /[A-Za-z0-9_]/.test(char);
}

function isNumberStart(source, index) {
  const char = source[index];
  if (/\d/.test(char)) return !isIdentifierChar(source[index - 1]);
  if (char !== "-" || !/\d/.test(source[index + 1])) return false;
  const previous = source[index - 1];
  return !previous || /[\s([{,:=+\-*\/%!<>]/.test(previous);
}

function scanSource(source) {
  const masked = Array.from({ length: source.length }, () => " ");
  const literals = [];
  let i = 0;
  let lineComment = false;
  let blockDepth = 0;
  while (i < source.length) {
    const char = source[i];
    const next = source[i + 1];
    if (lineComment) {
      if (char === "\n") {
        masked[i] = char;
        lineComment = false;
      }
      i += 1;
      continue;
    }
    if (blockDepth > 0) {
      if (char === "/" && next === "*") {
        blockDepth += 1;
        i += 2;
      } else if (char === "*" && next === "/") {
        blockDepth -= 1;
        i += 2;
      } else {
        if (char === "\n" || char === "\r") masked[i] = char;
        i += 1;
      }
      continue;
    }
    if (char === "/" && next === "/") {
      lineComment = true;
      i += 2;
      continue;
    }
    if (char === "/" && next === "*") {
      blockDepth = 1;
      i += 2;
      continue;
    }
    if (char === '"') {
      const triple = source.slice(i, i + 3) === '"""';
      const start = i;
      i += triple ? 3 : 1;
      let escaped = false;
      let closed = false;
      while (i < source.length) {
        if (!triple && escaped) {
          escaped = false;
          i += 1;
          continue;
        }
        if (!triple && source[i] === "\\") {
          escaped = true;
          i += 1;
          continue;
        }
        if ((triple && source.slice(i, i + 3) === '"""') || (!triple && source[i] === '"')) {
          i += triple ? 3 : 1;
          closed = true;
          break;
        }
        i += 1;
      }
      if (!closed) throw new Error(`unterminated string literal at byte ${start}`);
      literals.push({ start, end: i, kind: "string", raw: source.slice(start, i), triple });
      continue;
    }
    if (isNumberStart(source, i)) {
      const match = source.slice(i).match(/-?(?:0[xX][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d[\d_]*)?)/);
      if (match) {
        const end = i + match[0].length;
        literals.push({
          start: i,
          end,
          kind: /[.eE]/.test(match[0]) ? "float" : "number",
          raw: match[0],
        });
        i = end;
        continue;
      }
    }
    masked[i] = char;
    i += 1;
  }
  return { masked: masked.join(""), literals };
}

function sourceSkeleton(source) {
  const { masked, literals } = scanSource(source);
  let output = masked;
  for (let i = literals.length - 1; i >= 0; i -= 1) {
    const literal = literals[i];
    output = `${output.slice(0, literal.start)}<${literal.kind}>${output.slice(literal.end)}`;
  }
  return output;
}

function matchingMasked(masked, start, opening, closing) {
  let depth = 0;
  for (let index = start; index < masked.length; index += 1) {
    if (masked[index] === opening) depth += 1;
    else if (masked[index] === closing && --depth === 0) return index;
  }
  return -1;
}

function observerSpans(source, scanned = scanSource(source)) {
  const out = [];
  for (const match of scanned.masked.matchAll(/\b(print|eprint|assert|panic|exit)\s*\(/g)) {
    const open = match.index + match[0].lastIndexOf("(");
    const close = matchingMasked(scanned.masked, open, "(", ")");
    if (close < 0) continue;
    out.push({ operation: match[1], open, close });
  }
  return out;
}

function observerFingerprint(source) {
  const scanned = scanSource(source);
  const observers = observerSpans(source, scanned).map(({ operation, open, close }) => ({
    operation,
    skeleton: sourceSkeleton(source.slice(source.lastIndexOf("\n", open) + 1, close + 1)),
  }));
  return sha256(canonicalJson(observers));
}


function inspectSource(source) {
  const scanned = scanSource(source);
  const errors = [];
  const observers = observerSpans(source, scanned);
  if (!observers.length) {
    errors.push("observable sink is missing");
  }
  const bindings = [];
  const bindingPattern = /\b([A-Za-z_]\w*)\s*(?:::|:=)\s*/g;
  let match;
  while ((match = bindingPattern.exec(scanned.masked)) !== null) {
    const name = match[1];
    const start = match.index;
    const expressionStart = bindingPattern.lastIndex;
    const lineEnd = scanned.masked.slice(expressionStart).search(/[\n;}]/);
    const end = lineEnd < 0 ? source.length : expressionStart + lineEnd;
    const binding = { name, start, expressionStart, end };
    bindings.push(binding);
    if (name === "_" || name.startsWith("_")) {
      errors.push(`bind-and-discard binding: ${name}`);
    }
  }
  const inspected = { ...scanned, observers, binding_ranges: bindings };
  for (const binding of bindings) {
    if (!binding.name.startsWith("_") && !bindingFeedsObserver(binding, inspected)) {
      errors.push(`bind-and-discard result: ${binding.name}`);
    }
  }
  const directCall = /^\s*(?:[A-Za-z_]\w*\.)+[A-Za-z_]\w*\s*\([^\n]*\)\s*$/gm;
  if (directCall.test(scanned.masked)) errors.push("direct call result is not consumed");
  return {
    ...scanned,
    skeleton: sourceSkeleton(source),
    observer_fingerprint: observerFingerprint(source),
    observers,
    bindings: bindings.map(({ name }) => name),
    binding_ranges: bindings,
    errors,
  };
}

function isNondeterministicSource(source) {
  return /\b(?:uuid\.v4|random|rand|now|readline|stdin)\s*\(/.test(scanSource(source).masked);
}

const COMPILER_VOLATILE_KEYS = new Set([
  "span", "spans", "source", "file", "path", "line", "column", "offset",
  "start", "end", "byte_start", "byte_end", "diagnostics", "message",
]);
const COMPILER_VALUE_KEYS = new Set(["value", "raw", "text", "lexeme", "literal", "token"]);

function structuralCompilerValue(value, key = null) {
  if (value === null || typeof value !== "object") {
    return key && COMPILER_VALUE_KEYS.has(key) ? `<${typeof value}>` : value;
  }
  if (Array.isArray(value)) return value.map((item) => structuralCompilerValue(item));
  return Object.fromEntries(Object.keys(value).sort().flatMap((name) => {
    if (COMPILER_VOLATILE_KEYS.has(name)) return [];
    return [[name, structuralCompilerValue(value[name], name)]];
  }));
}

function compilerJsonValue(result) {
  if (!result || typeof result !== "object") return result;
  if (result.json && typeof result.json === "object") return result.json;
  if (typeof result.stdout === "string" || Buffer.isBuffer(result.stdout)) {
    try { return JSON.parse(Buffer.from(result.stdout).toString("utf8")); } catch { return null; }
  }
  return result;
}

function compilerSkeleton(result) {
  if (!result) return null;
  if (typeof result.type_skeleton === "string") return result.type_skeleton;
  if (typeof result.skeleton === "string") return result.skeleton;
  const json = compilerJsonValue(result);
  return json && typeof json === "object" ? sha256(canonicalJson(structuralCompilerValue(json))) : null;
}

function compilerCheckError(result) {
  if (result === false) return "checker returned failure";
  if (!result || typeof result !== "object") return null;
  if (result.ok === false || result.exit !== undefined && result.exit !== 0 || result.signal || result.timeout) {
    return result.error || result.stderr || "compiler check failed";
  }
  const json = compilerJsonValue(result);
  if (json?.status === "error" || json?.ok === false || json?.compiler?.error) {
    return json.compiler?.error?.message || json.error?.message || json.message || "compiler check failed";
  }
  return null;
}

function callSynchronousChecker(checkSource, source, label) {
  if (typeof checkSource !== "function") return null;
  const result = checkSource(source, label);
  if (result && typeof result.then === "function") {
    throw new Error("asynchronous compiler checker requires validateMutationCaseExecutable");
  }
  const error = compilerCheckError(result);
  if (error) throw new Error(`${label} parse-invalid/type-invalid: ${error}`);
  return result;
}

function bindingFeedsObserver(binding, inspected, visited = new Set()) {
  if (visited.has(binding)) return false;
  visited.add(binding);
  const name = new RegExp("\\b" + binding.name + "\\b");
  if (inspected.observers.some(({ open, close }) => (
    open > binding.end && name.test(inspected.masked.slice(open, close + 1))
  ))) return true;
  return inspected.binding_ranges.some((candidate) => {
    if (candidate === binding || candidate.start <= binding.end) return false;
    const expression = inspected.masked.slice(candidate.expressionStart, candidate.end);
    return name.test(expression) && bindingFeedsObserver(candidate, inspected, visited);
  });
}

function literalFeedsObserver(source, literal, inspected) {
  if (inspected.observers.some(({ open, close }) => literal.start > open && literal.end <= close)) return true;
  const binding = inspected.binding_ranges.find(({ expressionStart, end }) => literal.start >= expressionStart && literal.end <= end);
  return Boolean(binding && bindingFeedsObserver(binding, inspected));
}

export function validateMutationCase({
  source,
  mutated_source,
  skeleton,
  normalization = [],
  nondeterministic = undefined,
  checkSource = null,
  type_skeleton = undefined,
}) {
  requireString(source, "mutation source");
  if (!Array.isArray(normalization) || normalization.some((item) => typeof item !== "string" || !item)) {
    throw new Error("normalization must be a list of named fields");
  }
  const before = inspectSource(source);
  if (before.errors.length) throw new Error(before.errors.join("; "));
  if (skeleton !== undefined && skeleton !== before.skeleton) {
    throw new Error("mutation skeleton changed");
  }
  const beforeCompiler = callSynchronousChecker(checkSource, source, "source");
  const beforeTypeSkeleton = compilerSkeleton(beforeCompiler) || type_skeleton || null;
  if (type_skeleton !== undefined && beforeTypeSkeleton !== type_skeleton) {
    throw new Error("mutation type skeleton changed");
  }
  const rawNondeterministic = isNondeterministicSource(source) || nondeterministic === true;
  if (rawNondeterministic && normalization.length === 0) {
    throw new Error("nondeterministic raw bytes require recorded normalization");
  }
  if (mutated_source !== undefined) {
    requireString(mutated_source, "mutated source");
    const after = inspectSource(mutated_source);
    if (after.errors.length) throw new Error(`mutated source rejected: ${after.errors.join("; ")}`);
    if (after.skeleton !== before.skeleton) throw new Error("mutation changed typed source skeleton");
    if (after.observer_fingerprint !== before.observer_fingerprint) {
      throw new Error("mutation changed observable sink");
    }
    if (!after.literals.some((literal) => literalFeedsObserver(mutated_source, literal, after))) {
      throw new Error("mutation destroyed value-consuming meaning");
    }
    const afterCompiler = callSynchronousChecker(checkSource, mutated_source, "mutated source");
    const afterTypeSkeleton = compilerSkeleton(afterCompiler);
    if (beforeTypeSkeleton && afterTypeSkeleton && beforeTypeSkeleton !== afterTypeSkeleton) {
      throw new Error("mutation changed typed source skeleton");
    }
  }
  return {
    skeleton: before.skeleton,
    observer_fingerprint: before.observer_fingerprint,
    type_skeleton: beforeTypeSkeleton,
    nondeterministic: rawNondeterministic,
    normalization: [...normalization].sort(),
  };
}

function mutationValue(literal, arm) {
  if (literal.kind === "number" || literal.kind === "float") {
    const floating = literal.kind === "float";
    const values = floating
      ? {
          "boundary-min": "0.0",
          "boundary-max": "1.7976931348623157e308",
          negative: "-1.0",
          empty: "0.0",
          unicode: "1.0",
        }
      : {
          "boundary-min": "-9223372036854775808",
          "boundary-max": "9223372036854775807",
          negative: "-1",
          empty: "0",
          unicode: "1",
        };
    return values[arm];
  }
  const values = {
    "boundary-min": "",
    "boundary-max": "x".repeat(64),
    negative: "-1",
    empty: "",
    unicode: "e\u0301",
  };
  const value = values[arm];
  if (literal.triple) return `"""${value.replaceAll('"""', '\\"\\"\\"')}"""`;
  return JSON.stringify(value);
}

function literalIsInObserver(source, literal) {
  const lineStart = source.lastIndexOf("\n", literal.start - 1) + 1;
  const prefix = source.slice(lineStart, literal.start);
  return /\b(?:print|assert)\s*\([^)]*$/.test(prefix);
}

function literalIsInType(source, literal) {
  const lineStart = source.lastIndexOf("\n", literal.start - 1) + 1;
  const prefix = source.slice(lineStart, literal.start);
  const previous = prefix.trimEnd().at(-1);
  const suffix = source.slice(literal.end);
  return previous === "#"
    || previous === "."
    || /^\s*\./.test(suffix)
    || /\b(?:Int|I8|I16|I32|I64|U8|U16|U32|U64|F32|F64)\s*\([^)]*$/.test(prefix);
}

export function mutateValueSource(source, {
  domain,
  seed,
  mutation_arm = "boundary-min",
  normalization = [],
  nondeterministic = undefined,
  checkSource = null,
  type_skeleton = undefined,
}) {
  requireString(seed, "mutation seed");
  const normalizedDomain = mutationDomain(domain);
  if (!MUTATION_ARMS.includes(mutation_arm)) throw new Error(`unknown mutation arm: ${mutation_arm}`);
  const before = validateMutationCase({ source, normalization, nondeterministic, checkSource, type_skeleton });
  const { literals } = scanSource(source);
  const inspected = inspectSource(source);
  const candidates = literals.filter((literal) => !literalIsInType(source, literal) && literalFeedsObserver(source, literal, inspected));
  const target = candidates.find((literal) => mutationValue(literal, mutation_arm) !== literal.raw);
  if (!target) throw new Error("mutation source has no typed value literal");
  const replacement = mutationValue(target, mutation_arm);
  const mutated_source = `${source.slice(0, target.start)}${replacement}${source.slice(target.end)}`;
  validateMutationCase({
    source: mutated_source,
    mutated_source: undefined,
    skeleton: before.skeleton,
    normalization,
    nondeterministic,
    checkSource,
    type_skeleton: before.type_skeleton,
  });
  validateMutationCase({
    source,
    mutated_source,
    skeleton: before.skeleton,
    normalization,
    nondeterministic,
    checkSource,
    type_skeleton: before.type_skeleton,
  });
  return {
    seed,
    domain: normalizedDomain,
    mutation_arm,
    mutator_version: MUTATOR_VERSION,
    source: mutated_source,
    skeleton: before.skeleton,
    observer_fingerprint: before.observer_fingerprint,
    type_skeleton: before.type_skeleton,
    target_kind: target.kind,
  };
}

function caseKey(item) {
  return `${item.stable_surface_id}\u0000${item.seed}\u0000${item.mutation_arm}`;
}

function mutationDomain(domain) {
  requireString(domain, "mutation domain");
  try { return canonicalDomain(domain); } catch { return domain; }
}

function sourceSeed(item) {
  requireString(item.stable_surface_id, "seed stable_surface_id");
  requireString(item.seed, `seed for ${item.stable_surface_id}`);
  requireString(item.source, `source for ${item.stable_surface_id}`);
  requireString(item.domain, `domain for ${item.stable_surface_id}`);
  if (Buffer.byteLength(item.source, "utf8") > MAX_SOURCE_BYTES) {
    throw new Error(`seed source exceeds ${MAX_SOURCE_BYTES} bytes: ${item.stable_surface_id}`);
  }
  return item;
}

function seedOracle(seed) {
  const row = { domain: mutationDomain(seed.domain), seed: seed.seed, normalization: seed.normalization || [] };
  return oracleForRow(row) || selfDiffOracle(row);
}

function rejectedMutation(seed, mutation_arm, error) {
  return {
    stable_surface_id: seed.stable_surface_id,
    seed: seed.seed,
    domain: mutationDomain(seed.domain),
    mutation_arm,
    valid: false,
    reason: error.message,
  };
}

export function batchMutations(seeds, {
  batchSize = DEFAULT_BATCH_SIZE,
  arms = MUTATION_ARMS,
  maxCases = DEFAULT_CORPUS_LIMIT,
  checkSource = null,
} = {}) {
  if (!Array.isArray(seeds)) throw new Error("mutation seeds must be an array");
  if (!Number.isInteger(batchSize) || batchSize < 1 || batchSize > MAX_BATCH_SIZE) {
    throw new Error(`batch size must be an integer from 1 through ${MAX_BATCH_SIZE}`);
  }
  if (!Number.isInteger(maxCases) || maxCases < 1 || maxCases > DEFAULT_CORPUS_LIMIT) {
    throw new Error(`corpus limit must be an integer from 1 through ${DEFAULT_CORPUS_LIMIT}`);
  }
  if (seeds.length * arms.length > maxCases * 2) {
    throw new Error(`mutation corpus exceeds bounded attempt limit: ${maxCases * 2}`);
  }
  if (!Array.isArray(arms) || arms.length === 0 || arms.some((arm) => !MUTATION_ARMS.includes(arm))) {
    throw new Error("mutation arms must be known, non-empty, and bounded");
  }
  const cases = [];
  const rejected = [];
  const seen = new Set();
  [...seeds]
    .map(sourceSeed)
    .sort((left, right) => `${left.stable_surface_id}\u0000${left.seed}`.localeCompare(`${right.stable_surface_id}\u0000${right.seed}`))
    .forEach((seed) => {
      for (const mutation_arm of [...arms].sort()) {
        const key = `${seed.stable_surface_id}\u0000${seed.seed}\u0000${mutation_arm}`;
        if (seen.has(key)) throw new Error(`duplicate mutation case: ${key}`);
        seen.add(key);
        let mutation;
        try {
          mutation = mutateValueSource(seed.source, {
            domain: seed.domain,
            seed: seed.seed,
            mutation_arm,
            normalization: seed.normalization || [],
            nondeterministic: seed.nondeterministic,
            checkSource,
            type_skeleton: seed.type_skeleton,
          });
        } catch (error) {
          rejected.push(rejectedMutation(seed, mutation_arm, error));
          return;
        }
        const oracle = seedOracle(seed);
        cases.push({
          case_id: sha256(key).slice("sha256:".length, "sha256:".length + 16),
          stable_surface_id: seed.stable_surface_id,
          seed: seed.seed,
          domain: mutationDomain(seed.domain),
          mutation_arm,
          mutator_version: MUTATOR_VERSION,
          source: mutation.source,
          skeleton: mutation.skeleton,
          observer_fingerprint: mutation.observer_fingerprint,
          type_skeleton: mutation.type_skeleton || null,
          normalization: [...(seed.normalization || [])].sort(),
          oracle: {
            name: oracle.name,
            version: oracle.version,
            input_digest: sha256(canonicalJson({ seed: seed.seed, domain: mutation.domain })),
            independence_class: oracle.independence_class,
            provenance: oracle.provenance,
          },
          expected_relation: oracle.kind === "tier-self-diff"
            ? "tier-self-diff:aot-vs-jet_run-vs-interpreter"
            : `oracle:${oracle.name}`,
          applicable_tiers: [...(seed.applicable_tiers || TIERS)],
          validation: checkSource ? "checked" : "deferred-to-executable-runner",
        });
      }
    });
  if (cases.length > maxCases) throw new Error(`mutation corpus exceeds ${maxCases} valid cases`);
  const batches = [];
  for (let index = 0; index < cases.length; index += batchSize) {
    const batch = cases.slice(index, index + batchSize);
    const protocol = batch.map((item) => canonicalJson({
      case_id: item.case_id,
      stable_surface_id: item.stable_surface_id,
      seed: item.seed,
      mutation_arm: item.mutation_arm,
      source: item.source,
      oracle: item.oracle,
      expected_relation: item.expected_relation,
      normalization: item.normalization,
    })).join("\n") + (batch.length ? "\n" : "");
    batches.push({ index: batches.length, cases: batch, line_protocol: protocol });
  }
  return {
    mutator_version: MUTATOR_VERSION,
    batch_size: batchSize,
    max_cases: maxCases,
    attempted: cases.length + rejected.length,
    valid_case_count: cases.length,
    cases,
    rejected,
    batches,
  };
}

const TIER_FLAGS = Object.freeze({
  aot: ["--release"],
  jet_run: [],
  interpreter: ["--interpret"],
});

function commandArgument(value) {
  return JSON.stringify(String(value));
}

function commandLabel(program, args) {
  return [program, ...args].map(commandArgument).join(" ");
}

function stableCommandLabel(program, args) {
  const label = relative(ROOT, program).startsWith("..") ? program : relative(ROOT, program);
  return [label, ...args].map(commandArgument).join(" ");
}

export function tierCommand(tier, sourcePath, { root = ROOT, jetEnv = join(root, "scripts/agent/jet-env") } = {}) {
  if (!TIERS.includes(tier)) throw new Error(`invalid execution tier: ${tier}`);
  requireString(sourcePath, "tier source path");
  requireString(jetEnv, "Jet environment command");
  const program = resolve(jetEnv);
  const args = ["jet", "run", ...TIER_FLAGS[tier], sourcePath];
  const stableProgram = relative(root, program).startsWith("..") ? program : relative(root, program);
  return {
    tier,
    program,
    args,
    command: commandLabel(program, args),
    tier_command: `${stableProgram} ${["jet", "run", ...TIER_FLAGS[tier], "{source}"].map(commandArgument).join(" ")}`,
  };
}

function compilerCommand(operation, sourcePath, { root = ROOT, jetEnv = join(root, "scripts/agent/jet-env") } = {}) {
  if (!["parse", "check"].includes(operation)) throw new Error(`invalid compiler operation: ${operation}`);
  const program = resolve(jetEnv);
  const args = ["jet", "inspect", "compiler", operation, sourcePath, "--json"];
  const stableProgram = relative(root, program).startsWith("..") ? program : relative(root, program);
  return {
    operation,
    program,
    args,
    command: commandLabel(program, args),
    tier_command: `${stableProgram} ${["jet", "inspect", "compiler", operation, "{source}", "--json"].map(commandArgument).join(" ")}`,
  };
}

function boundedTimeout(value) {
  const timeout = value === undefined ? DEFAULT_TIMEOUT_MS : value;
  if (!Number.isInteger(timeout) || timeout < 1 || timeout > MAX_TIMEOUT_MS) {
    throw new Error(`timeout must be an integer from 1 through ${MAX_TIMEOUT_MS}`);
  }
  return timeout;
}

function boundedCapture(value) {
  const limit = value === undefined ? MAX_CAPTURE_BYTES : value;
  if (!Number.isInteger(limit) || limit < 1 || limit > MAX_CAPTURE_BYTES) {
    throw new Error(`capture limit must be an integer from 1 through ${MAX_CAPTURE_BYTES}`);
  }
  return limit;
}

function inputBytes(value, label) {
  if (value === undefined || value === null) return Buffer.alloc(0);
  if (Buffer.isBuffer(value)) return value;
  if (value instanceof Uint8Array) return Buffer.from(value);
  if (typeof value === "string") return Buffer.from(value, "utf8");
  throw new Error(`${label} must be bytes or UTF-8 text`);
}

function appendCapture(current, chunk, limit) {
  const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
  if (current.bytes.length >= limit) return { ...current, truncated: true };
  const room = limit - current.bytes.length;
  return {
    bytes: Buffer.concat([current.bytes, bytes.subarray(0, room)]),
    truncated: current.truncated || bytes.length > room,
  };
}

function stopChild(child) {
  if (!child?.pid || child.pid <= 1 || child.pid === process.pid) return;
  try {
    if (process.platform !== "win32") process.kill(-child.pid, "SIGTERM");
    else child.kill("SIGTERM");
  } catch { /* process exited */ }
}

export async function executeCommand({
  program,
  args = [],
  cwd = ROOT,
  env = {},
  stdin = "",
  timeout_ms = DEFAULT_TIMEOUT_MS,
  capture_limit = MAX_CAPTURE_BYTES,
  label = "command",
} = {}) {
  requireString(program, "command program");
  if (!Array.isArray(args) || args.some((arg) => typeof arg !== "string")) throw new Error("command args must be strings");
  requireString(cwd, "command cwd");
  const input = inputBytes(stdin, "command stdin");
  const timeout = boundedTimeout(timeout_ms);
  const limit = boundedCapture(capture_limit);
  const command = commandLabel(program, args);
  const child = spawn(program, args, {
    cwd,
    detached: process.platform !== "win32",
    env: { ...process.env, ...env },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const record = {
    label,
    command,
    program,
    args: [...args],
    pid: child.pid || null,
    stdout: { bytes: Buffer.alloc(0), truncated: false },
    stderr: { bytes: Buffer.alloc(0), truncated: false },
    exit: null,
    signal: null,
    timeout: false,
    error: null,
  };
  child.stdout?.on("data", (chunk) => { record.stdout = appendCapture(record.stdout, chunk, limit); });
  child.stderr?.on("data", (chunk) => { record.stderr = appendCapture(record.stderr, chunk, limit); });
  const timer = setTimeout(() => {
    record.timeout = true;
    stopChild(child);
  }, timeout);
  const closed = await new Promise((resolvePromise) => {
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      resolvePromise(value);
    };
    child.once("error", (error) => finish({ error }));
    child.once("close", (exit, signal) => finish({ exit, signal }));
  });
  clearTimeout(timer);
  if (closed.error) record.error = closed.error.message;
  record.exit = closed.exit ?? null;
  record.signal = closed.signal ?? null;
  if (record.timeout) stopChild(child);
  return {
    ...record,
    stdout: record.stdout.bytes,
    stderr: record.stderr.bytes,
    stdout_truncated: record.stdout.truncated,
    stderr_truncated: record.stderr.truncated,
    stdout_bytes: record.stdout.bytes.length,
    stderr_bytes: record.stderr.bytes.length,
    stdout_sha256: sha256(record.stdout.bytes),
    stderr_sha256: sha256(record.stderr.bytes),
    ok: !record.error && !record.timeout && record.exit === 0 && !record.signal,
  };
}

async function withSourceFile(source, options, callback) {
  if (source === undefined) return callback(options.source_path, null);
  requireString(source, "Jet source");
  if (Buffer.byteLength(source, "utf8") > MAX_SOURCE_BYTES) {
    throw new Error(`Jet source exceeds ${MAX_SOURCE_BYTES} bytes`);
  }
  const scratchRoot = resolve(options.scratch_root || process.env.JET_TEST_SCRATCH || os.tmpdir());
  await mkdir(scratchRoot, { recursive: true });
  const directory = await mkdtemp(join(scratchRoot, "jet-oracle-layer1-"));
  const sourcePath = join(directory, "case.jet");
  await writeFile(sourcePath, source, "utf8");
  try {
    return await callback(sourcePath, directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

export async function executeTier({
  tier,
  source,
  source_path,
  root = ROOT,
  jet_env = join(root, "scripts/agent/jet-env"),
  cwd = root,
  env = {},
  stdin = "",
  timeout_ms = DEFAULT_TIMEOUT_MS,
  capture_limit = MAX_CAPTURE_BYTES,
} = {}) {
  if (source === undefined && !source_path) throw new Error("tier execution needs source or source_path");
  const sourceHash = source === undefined ? null : sha256(source);
  return withSourceFile(source, {
    source_path: source_path ? resolve(root, source_path) : null,
    scratch_root: process.env.JET_TEST_SCRATCH,
  }, async (path) => {
    if (!path) throw new Error("tier source path is missing");
    const command = tierCommand(tier, path, { root, jetEnv: jet_env });
    const result = await executeCommand({
      ...command,
      cwd,
      env: { NO_COLOR: "1", JETPACK_ENV: "1", ...env },
      stdin,
      timeout_ms,
      capture_limit,
      label: `tier:${tier}`,
    });
    return {
      ...result,
      tier,
      tier_command: command.tier_command,
      source_path: path,
      source_sha256: sourceHash || sha256(readFileSync(path)),
    };
  });
}

export function createTierExecutor(options = {}) {
  return (request) => executeTier({ ...options, ...request });
}

export const runTier = executeTier;

function parseCompilerResult(result, operation) {
  let value = null;
  try { value = JSON.parse(result.stdout.toString("utf8")); } catch { /* diagnostic is retained in raw stderr */ }
  const error = compilerCheckError({ ...result, json: value });
  return { ...result, operation, json: value, ok: !error && result.ok, error };
}

export async function checkJetSource(source, {
  root = ROOT,
  jet_env = join(root, "scripts/agent/jet-env"),
  cwd = root,
  env = {},
  timeout_ms = DEFAULT_TIMEOUT_MS,
  capture_limit = MAX_CAPTURE_BYTES,
  source_path,
} = {}) {
  if (source === undefined && !source_path) throw new Error("compiler check needs source or source_path");
  return withSourceFile(source, { source_path: source_path ? resolve(root, source_path) : null }, async (path) => {
    const parse = parseCompilerResult(await executeCommand({
      ...compilerCommand("parse", path, { root, jetEnv: jet_env }),
      cwd,
      env: { NO_COLOR: "1", ...env },
      timeout_ms,
      capture_limit,
      label: "compiler:parse",
    }), "parse");
    const check = parse.ok
      ? parseCompilerResult(await executeCommand({
          ...compilerCommand("check", path, { root, jetEnv: jet_env }),
          cwd,
          env: { NO_COLOR: "1", ...env },
          timeout_ms,
          capture_limit,
          label: "compiler:check",
        }), "check")
      : null;
    const error = parse.error || check?.error || null;
    return {
      ok: !error,
      error,
      parse,
      check,
      parse_skeleton: compilerSkeleton(parse),
      type_skeleton: compilerSkeleton(check),
    };
  });
}

export async function validateMutationCaseExecutable(input, options = {}) {
  const staticResult = validateMutationCase(input);
  const checker = options.checkSource || ((source) => checkJetSource(source, options));
  const before = await checker(input.source, "source");
  const beforeError = compilerCheckError(before);
  if (beforeError) throw new Error(`source parse-invalid/type-invalid: ${beforeError}`);
  const type_skeleton = compilerSkeleton(before) || staticResult.type_skeleton;
  if (input.type_skeleton !== undefined && type_skeleton !== input.type_skeleton) {
    throw new Error("mutation type skeleton changed");
  }
  if (input.mutated_source !== undefined) {
    const after = await checker(input.mutated_source, "mutated source");
    const afterError = compilerCheckError(after);
    if (afterError) throw new Error(`mutated source parse-invalid/type-invalid: ${afterError}`);
    const afterTypeSkeleton = compilerSkeleton(after);
    if (type_skeleton && afterTypeSkeleton && type_skeleton !== afterTypeSkeleton) {
      throw new Error("mutation changed typed source skeleton");
    }
  }
  return { ...staticResult, type_skeleton, compiler_checked: true };
}

export async function batchMutationsExecutable(seeds, options = {}) {
  const batch = batchMutations(seeds, options);
  const byKey = new Map(seeds.map((seed) => [`${seed.stable_surface_id}\u0000${seed.seed}`, seed]));
  const cases = [];
  const rejected = [...batch.rejected];
  for (const item of batch.cases) {
    const seed = byKey.get(`${item.stable_surface_id}\u0000${item.seed}`);
    try {
      const checked = await validateMutationCaseExecutable({
        source: seed.source,
        mutated_source: item.source,
        skeleton: seed.skeleton,
        normalization: item.normalization,
        nondeterministic: seed.nondeterministic,
        type_skeleton: seed.type_skeleton,
      }, options);
      cases.push({ ...item, type_skeleton: checked.type_skeleton, validation: "checked" });
    } catch (error) {
      rejected.push({
        stable_surface_id: item.stable_surface_id,
        seed: item.seed,
        domain: item.domain,
        mutation_arm: item.mutation_arm,
        valid: false,
        reason: error.message,
      });
    }
  }
  const batches = [];
  for (let index = 0; index < cases.length; index += batch.batch_size) {
    const current = cases.slice(index, index + batch.batch_size);
    const line_protocol = current.map((item) => canonicalJson({
      case_id: item.case_id,
      stable_surface_id: item.stable_surface_id,
      seed: item.seed,
      mutation_arm: item.mutation_arm,
      source: item.source,
      oracle: item.oracle,
      expected_relation: item.expected_relation,
      normalization: item.normalization,
    })).join("\n") + (current.length ? "\n" : "");
    batches.push({ index: batches.length, cases: current, line_protocol });
  }
  return {
    ...batch,
    cases,
    rejected,
    attempted: cases.length + rejected.length,
    valid_case_count: cases.length,
    batches,
  };
}

function encodeBytes(value, label) {
  if (typeof value === "string" && value.startsWith("base64:")) {
    const encoded = value.slice("base64:".length);
    if (!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded) || encoded.length % 4 === 1) {
      throw new Error(`${label} has invalid base64 bytes`);
    }
    return value;
  }
  let bytes;
  if (Buffer.isBuffer(value)) bytes = value;
  else if (value instanceof Uint8Array) bytes = Buffer.from(value);
  else if (typeof value === "string") bytes = Buffer.from(value, "utf8");
  else throw new Error(`${label} must be bytes or UTF-8 text`);
  return `base64:${bytes.toString("base64")}`;
}

function requiredBundleString(input, key) {
  return requireString(input[key], `result bundle ${key}`);
}

function bytesFrom(value, label) {
  if (Buffer.isBuffer(value)) return value;
  if (value instanceof Uint8Array) return Buffer.from(value);
  if (typeof value === "string" && value.startsWith("base64:")) {
    const encoded = value.slice("base64:".length);
    if (!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded) || encoded.length % 4 === 1) {
      throw new Error(`${label} has invalid base64 bytes`);
    }
    return Buffer.from(encoded, "base64");
  }
  if (typeof value === "string") return Buffer.from(value, "utf8");
  throw new Error(`${label} must be bytes or UTF-8 text`);
}

function parsePrintedValue(text) {
  const trimmed = text.endsWith("\n") ? text.slice(0, -1) : text;
  if (/^-?\d+$/.test(trimmed)) {
    if (trimmed.length < 16 && Number.isSafeInteger(Number(trimmed))) return Number(trimmed);
    return trimmed;
  }
  if (/^-?(?:\d+\.\d+|\d+[eE][+-]?\d+)$/.test(trimmed)) return Number(trimmed);
  if (trimmed === "true" || trimmed === "false") return trimmed === "true";
  if (trimmed === "null") return null;
  try { return JSON.parse(trimmed); } catch { return text; }
}

function normalizeObservedValue(domain, observation, normalization = []) {
  if (Object.hasOwn(observation, "normalized_value")) return clone(observation.normalized_value);
  if (Object.hasOwn(observation, "value")) return clone(observation.value);
  let text = bytesFrom(observation.stdout ?? observation.stdout_bytes ?? "", "observed stdout").toString("utf8");
  for (const rule of normalization) {
    if (rule === "line_endings" || rule === "text.line_endings") text = text.replaceAll("\r\n", "\n").replaceAll("\r", "\n");
    if (rule === "stdout.trailing_newline") text = text.endsWith("\n") ? text.slice(0, -1) : text;
  }
  const canonical = mutationDomain(domain);
  if (normalization.includes("uuid.random_bytes")) {
    const uuid = text.trim();
    const version = uuid[14] ? Number(uuid[14]) : null;
    return { length: uuid.length, hyphen: uuid.includes("-"), version };
  }
  if (["numeric", "float", "time"].includes(canonical)) return parsePrintedValue(text);
  if (canonical === "numeric_decimal") return text.endsWith("\n") ? text.slice(0, -1) : text;
  if (["collections", "json_codable", "regex", "protocol", "concurrency", "memory"].includes(canonical)) {
    return parsePrintedValue(text);
  }
  return text;
}

function relationText(value) {
  const result = canonicalJson(value);
  return result === undefined ? "null" : result;
}

function expectedCaseValue(caseInput) {
  if (Object.hasOwn(caseInput, "expected_value")) return clone(caseInput.expected_value);
  const adapterInput = caseInput.oracle_input ?? caseInput.input;
  if (adapterInput !== undefined) {
    try { return oracleAdapter(caseInput.domain).reference(adapterInput); } catch { /* self-diff */ }
  }
  const relation = caseInput.expected_relation;
  if (typeof relation === "string" && !relation.startsWith("oracle:") && !relation.startsWith("tier-self-diff:")) {
    try { return JSON.parse(relation); } catch { return relation; }
  }
  return undefined;
}

function sameBytes(left, right) {
  try { return bytesFrom(left ?? "", "observation bytes").equals(bytesFrom(right ?? "", "observation bytes")); } catch { return left === right; }
}

function normalizeTierObservations(observations) {
  if (observations === undefined) return [];
  if (!Array.isArray(observations)) throw new Error("tier_observations must be an array");
  const seen = new Set();
  return [...observations]
    .map((observation) => {
      if (!observation || !TIERS.includes(observation.tier)) throw new Error("tier observation has an invalid tier");
      if (seen.has(observation.tier)) throw new Error(`duplicate tier observation: ${observation.tier}`);
      seen.add(observation.tier);
      if (observation.exit !== null && !Number.isInteger(observation.exit)) throw new Error(`tier observation ${observation.tier} has no exit`);
      return {
        tier: observation.tier,
        stdout_bytes: encodeBytes(observation.stdout ?? observation.stdout_bytes, `${observation.tier} stdout`),
        stderr_bytes: encodeBytes(observation.stderr ?? observation.stderr_bytes, `${observation.tier} stderr`),
        exit: observation.exit ?? null,
        signal: observation.signal ?? null,
        timeout: observation.timeout === true,
        relation: requireString(observation.relation, `${observation.tier} relation`),
        normalized_value: Object.hasOwn(observation, "normalized_value") ? clone(observation.normalized_value) : null,
        stdout_truncated: observation.stdout_truncated === true,
        stderr_truncated: observation.stderr_truncated === true,
      };
    })
    .sort((left, right) => TIERS.indexOf(left.tier) - TIERS.indexOf(right.tier));
}

export function compareTierObservations(observations, applicableTiers, normalization = []) {
  const expected = requireTierList(applicableTiers, "applicable tiers");
  const actual = observations.map((observation) => observation.tier);
  if (canonicalJson(expected) !== canonicalJson(actual)) {
    throw new Error("tier observations do not cover applicable tiers exactly once");
  }
  const baseline = observations[0];
  const differences = observations.slice(1).filter((observation) => {
    if (normalization.length > 0) return observation.relation !== baseline.relation;
    return !sameBytes(observation.stdout ?? observation.stdout_bytes, baseline.stdout ?? baseline.stdout_bytes)
      || !sameBytes(observation.stderr ?? observation.stderr_bytes, baseline.stderr ?? baseline.stderr_bytes)
      || observation.exit !== baseline.exit
      || observation.signal !== baseline.signal
      || observation.timeout !== baseline.timeout
      || observation.relation !== baseline.relation;
  }).map((observation) => observation.tier);
  return { ok: differences.length === 0, baseline: baseline.tier, differences };
}

export function compareCaseObservations({
  domain,
  observations,
  applicable_tiers = TIERS,
  normalization = [],
  expected_value = undefined,
  expected_relation = undefined,
} = {}) {
  requireString(domain, "comparison domain");
  if (!Array.isArray(observations) || observations.length === 0) throw new Error("case observations are required");
  const tiers = requireTierList(applicable_tiers, "applicable tiers");
  const normalized = observations.map((observation) => ({
    ...observation,
    normalized_value: normalizeObservedValue(domain, observation, normalization),
  })).map((observation) => ({
    ...observation,
    relation: relationText(observation.normalized_value),
  }));
  const tierParity = compareTierObservations(normalized, tiers, normalization);
  const expected = expected_value === undefined ? undefined : clone(expected_value);
  const adapterItem = (() => { try { return oracleAdapter(domain); } catch { return null; } })();
  const oracleChecks = expected === undefined || !adapterItem
    ? normalized.map(() => true)
    : normalized.map((observation) => adapterItem.relation(expected, observation.normalized_value).ok);
  const oracleOk = oracleChecks.every(Boolean);
  const differences = normalized.filter((observation, index) => !oracleChecks[index]).map((observation) => observation.tier);
  const ok = tierParity.ok && oracleOk;
  const first = normalized[0];
  const actualRelation = first.relation;
  const expectedRelation = expected === undefined
    ? (expected_relation || "tier-self-diff:aot-vs-jet_run-vs-interpreter")
    : relationText(expected);
  return {
    ok,
    expected: expected === undefined ? expectedRelation : expected,
    actual: first.normalized_value,
    expected_relation: expectedRelation,
    actual_relation: actualRelation,
    tier_parity: tierParity,
    oracle_ok: oracleOk,
    oracle_differences: differences,
    differences: [...new Set([...tierParity.differences, ...differences])],
    observations: normalized,
    result_bundle_input: ok ? null : {
      tier: normalized.find((observation) => differences.includes(observation.tier))?.tier
        || normalized.find((observation) => tierParity.differences.includes(observation.tier))?.tier
        || first.tier,
      expected_relation: expectedRelation,
      actual_relation: normalized.find((observation) => differences.includes(observation.tier))?.relation
        || normalized.find((observation) => tierParity.differences.includes(observation.tier))?.relation
        || actualRelation,
      tier_observations: normalized,
    },
  };
}

export async function executeCase(caseInput, {
  executor = executeTier,
  validate = true,
  validation = {},
  applicable_tiers = caseInput?.applicable_tiers || TIERS,
  normalization = caseInput?.normalization || [],
  stdin = caseInput?.stdin || "",
  ...executionOptions
} = {}) {
  if (!caseInput || typeof caseInput !== "object") throw new Error("execution case is required");
  requireString(caseInput.source, "execution case source");
  requireString(caseInput.domain, "execution case domain");
  const tiers = requireTierList(applicable_tiers, "applicable tiers");
  if (validate) {
    await validateMutationCaseExecutable({
      source: caseInput.source,
      mutated_source: caseInput.mutated_source,
      skeleton: caseInput.skeleton,
      normalization,
      nondeterministic: caseInput.nondeterministic,
      type_skeleton: caseInput.type_skeleton,
    }, validation);
  }
  const run = typeof executor === "function" ? executor : executor.execute;
  if (typeof run !== "function") throw new Error("case executor must be a function");
  const observations = [];
  for (const tier of tiers) {
    observations.push(await run({
      ...executionOptions,
      ...caseInput,
      tier,
      source: caseInput.source,
      stdin,
    }));
  }
  const comparison = compareCaseObservations({
    domain: caseInput.domain,
    observations,
    applicable_tiers: tiers,
    normalization,
    expected_value: expectedCaseValue(caseInput),
    expected_relation: caseInput.expected_relation,
  });
  return {
    ...caseInput,
    ...comparison,
    observations: comparison.observations,
    tier_results: observations,
    applicable_tiers: tiers,
  };
}

export function wrongResultExecutor(executor, { tier = null, stdout, normalized_value } = {}) {
  if (typeof executor !== "function") throw new Error("wrong-result executor needs an executor");
  return async (request) => {
    const result = await executor(request);
    if (tier !== null && request.tier !== tier) return result;
    return {
      ...result,
      ...(stdout === undefined ? {} : { stdout: inputBytes(stdout, "planted stdout") }),
      ...(normalized_value === undefined ? {} : { normalized_value: clone(normalized_value) }),
    };
  };
}

export function makeResultBundle(input) {
  if (!input || typeof input !== "object") throw new Error("result bundle input must be an object");
  const normalization = input.normalization;
  if (!Array.isArray(normalization) || normalization.some((item) => typeof item !== "string" || !item)) {
    throw new Error("result bundle normalization must be a named list");
  }
  const source = requiredBundleString(input, "source");
  if (Buffer.byteLength(source, "utf8") > MAX_SOURCE_BYTES) throw new Error(`result bundle source exceeds ${MAX_SOURCE_BYTES} bytes`);
  const bundle = {
    schema_version: SCHEMA_VERSION,
    run_id: requiredBundleString(input, "run_id"),
    stable_surface_id: requiredBundleString(input, "stable_surface_id"),
    tier: requiredBundleString(input, "tier"),
    tier_command: requiredBundleString(input, "tier_command"),
    seed: requiredBundleString(input, "seed"),
    mutation_arm: requiredBundleString(input, "mutation_arm"),
    mutator_version: input.mutator_version || MUTATOR_VERSION,
    source,
    source_sha256: sha256(source),
    stdout_bytes: encodeBytes(input.stdout ?? input.stdout_bytes, "stdout"),
    stderr_bytes: encodeBytes(input.stderr ?? input.stderr_bytes, "stderr"),
    exit: input.exit,
    signal: input.signal ?? null,
    timeout: input.timeout === true,
    expected_relation: requiredBundleString(input, "expected_relation"),
    actual_relation: requiredBundleString(input, "actual_relation"),
    normalization: [...new Set(normalization)].sort(),
    oracle: clone(input.oracle),
    commit: requiredBundleString(input, "commit"),
    binary_sha256: requiredBundleString(input, "binary_sha256"),
    registry_snapshot_hash: requiredBundleString(input, "registry_snapshot_hash"),
    config_hash: requiredBundleString(input, "config_hash"),
    classification: requiredBundleString(input, "classification"),
    tower_action: requiredBundleString(input, "tower_action"),
    tier_observations: normalizeTierObservations(input.tier_observations),
    applicable_tiers: input.applicable_tiers == null
      ? null
      : requireTierList(input.applicable_tiers, "result bundle applicable_tiers"),
  };
  if (!TIERS.includes(bundle.tier)) throw new Error(`result bundle has invalid tier: ${bundle.tier}`);
  if (bundle.exit !== null && !Number.isInteger(bundle.exit)) throw new Error("result bundle exit must be an integer or null");
  if (bundle.exit === null && !bundle.signal && !bundle.timeout) throw new Error("result bundle null exit needs signal or timeout");
  if (!bundle.oracle || typeof bundle.oracle !== "object") throw new Error("result bundle oracle is missing");
  for (const key of ["name", "version", "input_digest", "independence_class", "provenance"]) {
    requireString(bundle.oracle[key], `result bundle oracle.${key}`);
  }
  let tierParity = null;
  if (bundle.applicable_tiers !== null) {
    tierParity = compareTierObservations(bundle.tier_observations, bundle.applicable_tiers, bundle.normalization);
  }
  bundle.tier_parity = tierParity;
  const scan = inspectSource(source);
  if (scan.errors.length) throw new Error(`result bundle source rejected: ${scan.errors.join("; ")}`);
  if (isNondeterministicSource(source) && bundle.normalization.length === 0) {
    throw new Error("result bundle drops nondeterministic bytes without normalization");
  }
  return bundle;
}

export function validateResultBundle(bundle) {
  makeResultBundle(bundle);
  return true;
}

function stableCommand(command) {
  return String(command)
    .replace(/(?:[A-Za-z]:)?[\\/][^\s"']+\.jet(?=(?:[\s"']|$))/g, "{source}")
    .replace(/(?:^|[\\/])(?:tmp|jet-oracle-layer1-[^\\/]+)(?:[\\/]|$)/g, (match) => match.startsWith("/") ? "/" : "");
}

const TRANSIENT_BUNDLE_KEYS = new Set([
  "pid", "source_path", "cwd", "started", "finished", "timestamp", "at",
]);

function removeTransient(value, key = null) {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map((item) => removeTransient(item));
  return Object.fromEntries(Object.keys(value).sort().flatMap((name) => {
    if (TRANSIENT_BUNDLE_KEYS.has(name)) return [];
    return [[name, removeTransient(value[name], name)]];
  }));
}

export function stableBundleInput(bundle) {
  const normalized = makeResultBundle(bundle);
  return removeTransient({
    ...normalized,
    run_id: "<run>",
    tier_command: stableCommand(normalized.tier_command),
  });
}

export function bundleIdentity(bundle) {
  return sha256(canonicalJson(stableBundleInput(bundle)));
}

export function serializeBundles(bundles) {
  if (!Array.isArray(bundles)) throw new Error("result bundles must be an array");
  const stable = new Map();
  for (const bundle of bundles) {
    const identity = bundleIdentity(bundle);
    if (!stable.has(identity)) stable.set(identity, stableBundleInput(bundle));
  }
  return [...stable.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([, bundle]) => canonicalJson(bundle))
    .join("\n") + (stable.size ? "\n" : "");
}

export const REGRESSION_SEEDS = Object.freeze([
  {
    stable_surface_id: "regression:semantic-equality",
    control: "nested-collections",
    domain: "collections",
    seed: "regression-semantic-equality-001",
    source: `fn run() {
    left :: [[1, 2], [3]]
    right :: [[1, 2], [3]]
    print(left == right)
}
`,
    expected_value: true,
    wrong_value: false,
    wrong_tier: "jet_run",
  },
  {
    stable_surface_id: "regression:indexed-place",
    control: "bool-matching",
    domain: "numeric",
    seed: "regression-indexed-place-001",
    source: `fn run() {
    flag :: true
    print(flag)
}
`,
    expected_value: true,
    wrong_value: false,
    wrong_tier: "jet_run",
  },
  {
    stable_surface_id: "regression:packed-int",
    control: "packed-integer-extrema",
    domain: "numeric",
    seed: "regression-packed-int-001",
    source: `fn run() {
    print(9223372036854775807)
}
`,
    expected_value: "9223372036854775807",
    wrong_value: "-1",
    wrong_tier: "jet_run",
  },
  {
    stable_surface_id: "regression:release-emission",
    control: "release-emission-totality",
    domain: "compiler_reflection",
    seed: "regression-release-emission-001",
    source: `fn run() {
    value :: 3
    print(value + 4)
}
`,
    expected_value: 7,
    wrong_value: 0,
    wrong_tier: "aot",
  },
  {
    stable_surface_id: "regression:stdin-transport",
    control: "stdin-transport",
    domain: "host_io",
    seed: "regression-stdin-transport-001",
    source: `use core.term as io

fn run() {
    line :: io.readline() ?? return Err("read")
    print(line)
}
`,
    expected_value: "alpha\n",
    wrong_value: "",
    wrong_tier: "jet_run",
    normalization: ["stdin.fixture"],
    stdin: "alpha\n",
  },
]);

function regressionOracle(seed) {
  const item = seedOracle(seed);
  return {
    name: item.name,
    version: item.version,
    input_digest: item.input_digest,
    independence_class: item.independence_class,
    provenance: item.provenance,
  };
}

function printedValue(value) {
  if (typeof value === "string") return `${value}${value.endsWith("\n") ? "" : "\n"}`;
  return `${JSON.stringify(value)}\n`;
}

function bundleInputForControl(seed, planted, metadata) {
  const input = planted.result_bundle_input;
  const selected = planted.tier_results.find((observation) => observation.tier === input.tier) || planted.tier_results[0];
  const selectedObservation = planted.observations.find((observation) => observation.tier === input.tier) || planted.observations[0];
  return {
    run_id: metadata.run_id || `control-${seed.seed}`,
    stable_surface_id: seed.stable_surface_id,
    tier: input.tier,
    tier_command: selected.tier_command || `scripts/agent/jet-env jet run {source}`,
    seed: seed.seed,
    mutation_arm: "planted-wrong-answer",
    mutator_version: MUTATOR_VERSION,
    source: seed.source,
    stdout: selected.stdout ?? selected.stdout_bytes,
    stderr: selected.stderr ?? selected.stderr_bytes,
    exit: selected.exit,
    signal: selected.signal ?? null,
    timeout: selected.timeout === true,
    expected_relation: input.expected_relation,
    actual_relation: selectedObservation.relation,
    normalization: seed.normalization || [],
    oracle: regressionOracle(seed),
    commit: metadata.commit,
    binary_sha256: metadata.binary_sha256,
    registry_snapshot_hash: metadata.registry_snapshot_hash,
    config_hash: metadata.config_hash,
    classification: "P0",
    tower_action: "create-or-update",
    applicable_tiers: planted.applicable_tiers,
    tier_observations: planted.observations,
  };
}

function regressionCase(seed) {
  return {
    ...seed,
    expected_relation: relationText(seed.expected_value),
    applicable_tiers: seed.applicable_tiers || TIERS,
  };
}

function staticRegressionControl(seed) {
  const caseInput = regressionCase(seed);
  const wrongTier = seed.wrong_tier || "jet_run";
  const tier_results = caseInput.applicable_tiers.map((tier) => ({
    tier,
    stdout: printedValue(tier === wrongTier ? caseInput.wrong_value : caseInput.expected_value),
    stderr: "",
    exit: 0,
    signal: null,
    timeout: false,
  }));
  const comparison = compareCaseObservations({
    domain: caseInput.domain,
    observations: tier_results,
    applicable_tiers: caseInput.applicable_tiers,
    normalization: caseInput.normalization || [],
    expected_value: caseInput.expected_value,
    expected_relation: caseInput.expected_relation,
  });
  if (comparison.ok) throw new Error(`planted wrong answer survived ${seed.stable_surface_id}`);
  return { ...caseInput, ...comparison, tier_results };
}

export function regressionFindingBundles({
  controls = REGRESSION_SEEDS,
  commit = "unknown-commit",
  binary_sha256 = "sha256:unknown-binary",
  registry_snapshot_hash = "sha256:unknown-registry",
  config_hash = "sha256:unknown-config",
  run_id = undefined,
} = {}) {
  if (!Array.isArray(controls) || controls.length === 0) throw new Error("regression controls are required");
  if (controls.length > DEFAULT_CORPUS_LIMIT) throw new Error(`regression controls exceed ${DEFAULT_CORPUS_LIMIT}`);
  const metadata = { commit, binary_sha256, registry_snapshot_hash, config_hash, run_id };
  return controls.map((seed) => makeResultBundle(
    bundleInputForControl(seed, staticRegressionControl(seed), metadata),
  ));
}

export async function runRegressionControls({
  executor = executeTier,
  validate = true,
  validation = {},
  controls = REGRESSION_SEEDS,
  commit = "unknown-commit",
  binary_sha256 = "sha256:unknown-binary",
  registry_snapshot_hash = "sha256:unknown-registry",
  config_hash = "sha256:unknown-config",
  run_id = undefined,
} = {}) {
  if (!Array.isArray(controls) || controls.length === 0) throw new Error("regression controls are required");
  if (controls.length > DEFAULT_CORPUS_LIMIT) throw new Error(`regression controls exceed ${DEFAULT_CORPUS_LIMIT}`);
  const metadata = { commit, binary_sha256, registry_snapshot_hash, config_hash, run_id };
  const findings = [];
  for (const seed of controls) {
    const caseInput = regressionCase(seed);
    const baseline = await executeCase(caseInput, { executor, validate, validation });
    if (!baseline.ok) throw new Error(`regression control baseline failed: ${seed.stable_surface_id}`);
    const plantedExecutor = wrongResultExecutor(executor, {
      tier: seed.wrong_tier || null,
      stdout: printedValue(caseInput.wrong_value),
    });
    const planted = await executeCase(caseInput, {
      executor: plantedExecutor,
      validate,
      validation,
    });
    if (planted.ok) throw new Error(`planted wrong answer survived ${seed.stable_surface_id}`);
    findings.push(makeResultBundle(bundleInputForControl(seed, planted, metadata)));
  }
  return findings;
}


function walkFiles(directory, suffix) {
  if (!existsSync(directory)) return [];
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...walkFiles(path, suffix));
    else if (entry.isFile() && path.endsWith(suffix)) files.push(path);
  }
  return files.sort((left, right) => left.localeCompare(right));
}

function walkJetFiles(directory) {
  return walkFiles(directory, ".jet");
}

function coreMarker(source) {
  const match = source.match(/^\s*\/\/\s*core-conformance:\s*(\S+)\s*$/m);
  return match ? `module:${match[1]}` : null;
}

function seedFromPath(path, root, conformanceRoot) {
  const source = readFileSync(path, "utf8");
  const relativePath = relative(root, path).split("\\").join("/");
  const stableSurfaceId = path.startsWith(conformanceRoot)
    ? coreMarker(source)
    : `fixture:${relativePath}`;
  return {
    stable_surface_id: stableSurfaceId || `fixture:${relativePath}`,
    seed: sha256(relativePath).slice("sha256:".length, "sha256:".length + 16),
    path: relativePath,
    source_kind: path.startsWith(conformanceRoot) ? "conformance" : "differential",
    source,
    domain: "compiler_reflection",
    normalization: [],
  };
}

function safeRelativePath(value, label) {
  requireString(value, label);
  const normalized = value.replaceAll("\\", "/");
  if (normalized.startsWith("/") || normalized.split("/").includes("..")) {
    throw new Error(`${label} escapes the differential fixture root: ${value}`);
  }
  return normalized;
}

export function readDifferentialManifest(path = join(ROOT, "tests/fuzz/sema/differential/manifest.tsv")) {
  if (!existsSync(path)) throw new Error(`differential fixture manifest is missing: ${path}`);
  const rows = [];
  const seen = new Set();
  for (const [index, raw] of readFileSync(path, "utf8").split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const fields = raw.split("\t");
    if (fields.length !== 4 || fields[0] === "source" && index === 0) {
      if (fields[0] === "source" && fields[1] === "output") continue;
      throw new Error(`malformed differential fixture manifest at line ${index + 1}`);
    }
    const source = safeRelativePath(fields[0].trim(), `source at line ${index + 1}`);
    const output = fields[1].trim() === "-"
      ? null
      : safeRelativePath(fields[1].trim(), `output at line ${index + 1}`);
    const relation = requireString(fields[2].trim(), `relation at line ${index + 1}`);
    const exception = fields[3].trim();
    if (seen.has(source)) throw new Error(`duplicate differential source: ${source}`);
    if (!output && !exception) throw new Error(`missing pairing exception for ${source}`);
    if (output && exception) throw new Error(`paired output has an exception for ${source}`);
    seen.add(source);
    rows.push({ source, output, relation, exception });
  }
  rows.sort((left, right) => left.source.localeCompare(right.source));
  const directory = dirname(path);
  for (const row of rows) {
    if (!existsSync(join(directory, row.source))) throw new Error(`differential source is missing: ${row.source}`);
    if (row.output && !existsSync(join(directory, row.output))) {
      throw new Error(`differential output is missing: ${row.output}`);
    }
  }
  const listedOutputs = new Set(rows.filter((row) => row.output).map((row) => row.output));
  const discoveredOutputs = new Set(walkFiles(directory, ".out").map((file) => relative(directory, file).replaceAll("\\", "/")));
  const orphanedOutputs = [...discoveredOutputs].filter((output) => !listedOutputs.has(output)).sort();
  if (orphanedOutputs.length) throw new Error(`unpaired differential outputs: ${orphanedOutputs.join(",")}`);
  return rows;
}

function differentialPaths(root, manifest) {
  const directory = join(root, "tests/fuzz/sema/differential");
  const discovered = new Set(walkJetFiles(directory).map((path) => relative(directory, path).replaceAll("\\", "/")));
  const listed = new Set(manifest.map((row) => row.source));
  const missing = [...discovered].filter((path) => !listed.has(path)).sort();
  const stale = [...listed].filter((path) => !discovered.has(path)).sort();
  if (missing.length || stale.length) {
    throw new Error(`differential fixture manifest drift: missing=${missing.join(",") || "none"}; stale=${stale.join(",") || "none"}`);
  }
  return manifest.map((row) => join(directory, row.source));
}

export function discoverCorpusSeeds(root = ROOT, { includeDifferential = true } = {}) {
  const conformanceRoot = join(root, "tests/conformance/corpus");
  const differentialRoot = join(root, "tests/fuzz/sema/differential");
  const paths = walkJetFiles(conformanceRoot);
  if (includeDifferential) paths.push(...differentialPaths(root, readDifferentialManifest(join(differentialRoot, "manifest.tsv"))));
  const seeds = [];
  const rejected = [];
  for (const path of paths.sort((left, right) => left.localeCompare(right))) {
    const seed = seedFromPath(path, root, conformanceRoot);
    try {
      if (seed.source_kind === "conformance" && !coreMarker(seed.source)) {
        throw new Error("conformance seed is missing its core-conformance stable ID");
      }
      const details = validateMutationCase({ source: seed.source, normalization: seed.normalization });
      seeds.push({ ...seed, skeleton: details.skeleton, observer_fingerprint: details.observer_fingerprint });
    } catch (error) {
      rejected.push({ path: seed.path, reason: error.message });
    }
  }
  return { seeds, rejected };
}

function printUsage() {
  console.error(`usage: ${process.argv[1]} --self-test|--adapters|--regressions|--seeds|--catalog FILE`);
}

function main(argv) {
  const command = argv[0] || "--self-test";
  if (command === "--self-test") {
    const adapters = checkAllAdapters();
    const findings = regressionFindingBundles();
    console.log(`hardening oracle layer: adapters=${adapters.length} planted_wrong_answers=${findings.length} rejected`);
    return 0;
  }
  if (command === "--adapters") {
    console.log(canonicalJson(oracleCatalog()));
    return 0;
  }
  if (command === "--regressions") {
    console.log(serializeBundles(regressionFindingBundles()));
    return 0;
  }
  if (command === "--seeds") {
    const result = discoverCorpusSeeds();
    console.log(canonicalJson({
      checked_in_seeds: result.seeds.length,
      rejected: result.rejected,
      seed_ids: result.seeds.map((seed) => seed.stable_surface_id),
    }));
    return result.rejected.length ? 1 : 0;
  }
  if (command === "--catalog") {
    const path = argv[1];
    if (!path) {
      printUsage();
      return 2;
    }
    const input = readSurfaceManifest(resolve(path));
    console.log(canonicalJson(buildOracleCatalog(input.rows, input.source_snapshot_hash)));
    return 0;
  }
  printUsage();
  return 2;
}

const invokedPath = process.argv[1] && resolve(process.argv[1]);
const modulePath = resolve(fileURLToPath(import.meta.url));
if (invokedPath === modulePath) process.exitCode = main(process.argv.slice(2));
