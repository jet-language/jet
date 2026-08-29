#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
export const MUTATION_ARMS = Object.freeze([
  "boundary-min",
  "boundary-max",
  "negative",
  "empty",
  "unicode",
]);

const ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));

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

const DOMAIN_ALIASES = Object.freeze({
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
  if (!Object.hasOwn(ADAPTERS, value)) throw new Error(`unknown hardening domain: ${domain}`);
  return value;
}

export function oracleAdapter(domain) {
  return ADAPTERS[canonicalDomain(domain)];
}

export function oracleCatalog() {
  return Object.values(ADAPTERS).map((item) => ({
    domain: item.id,
    oracle: item.oracle,
    version: item.version,
    independence_class: item.independence_class,
    provenance: item.provenance,
    normalization: [...item.normalization],
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
  return Object.values(ADAPTERS).map((item) => {
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

export function buildOracleCatalog(surfaceRows, sourceSnapshotHash) {
  if (!Array.isArray(surfaceRows)) throw new Error("surface manifest must contain rows");
  requireString(sourceSnapshotHash, "source snapshot hash");
  const seen = new Set();
  const normalized = surfaceRows
    .map((row) => normalizeSurfaceRow(row, seen))
    .sort((left, right) => left.stable_id.localeCompare(right.stable_id));
  const rows = normalized.map((row) => {
    if (row.exclusion) {
      return {
        ...row,
        tier_self_diff: false,
        oracle: null,
      };
    }
    const item = oracleAdapter(row.domain);
    return {
      ...row,
      tier_self_diff: true,
      oracle: {
        name: item.oracle,
        version: item.version,
        input_digest: sha256(canonicalJson({ domain: row.domain, seed: row.seed })),
        independence_class: item.independence_class,
        provenance: item.provenance,
      },
    };
  });
  return {
    schema: SCHEMA_VERSION,
    source_snapshot_hash: sourceSnapshotHash,
    generated_by: "hardening-oracle-layer",
    rows,
    exclusions: rows.filter((row) => row.exclusion).length,
    executable: rows.filter((row) => !row.exclusion).length,
  };
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
  const rows = Array.isArray(value) ? value : value && value.rows;
  if (!Array.isArray(rows)) throw new Error("surface manifest must be an array or {rows: []}");
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

function observerFingerprint(source) {
  return sha256(sourceSkeleton(source));
}

function identifierCount(masked, name) {
  return (masked.match(new RegExp(`\\b${name.replace(/[.*+?^${}()|[\\]\\]/g, "\\$&")}\\b`, "g")) || []).length;
}

function inspectSource(source) {
  const scanned = scanSource(source);
  const errors = [];
  if (!/\b(?:print|assert|panic|exit)\s*\(/.test(scanned.masked)) {
    errors.push("observable sink is missing");
  }
  const bindings = [];
  const bindingPattern = /^\s*([A-Za-z_]\w*)\s*(?:::|:=)\s*(.+)$/gm;
  let match;
  while ((match = bindingPattern.exec(scanned.masked)) !== null) {
    const name = match[1];
    bindings.push(name);
    if (name === "_" || name.startsWith("_")) {
      errors.push(`bind-and-discard binding: ${name}`);
    } else if (identifierCount(scanned.masked, name) < 2) {
      errors.push(`bind-and-discard result: ${name}`);
    }
  }
  const directCall = /^\s*(?:[A-Za-z_]\w*\.)+[A-Za-z_]\w*\s*\([^\n]*\)\s*$/gm;
  if (directCall.test(scanned.masked)) errors.push("direct call result is not consumed");
  return {
    ...scanned,
    skeleton: sourceSkeleton(source),
    observer_fingerprint: observerFingerprint(source),
    bindings,
    errors,
  };
}

function isNondeterministicSource(source) {
  return /\b(?:uuid\.v4|random|rand|now|readline|stdin)\s*\(/.test(scanSource(source).masked);
}

export function validateMutationCase({
  source,
  mutated_source,
  skeleton,
  normalization = [],
  nondeterministic = undefined,
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
  }
  return {
    skeleton: before.skeleton,
    observer_fingerprint: before.observer_fingerprint,
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
}) {
  requireString(seed, "mutation seed");
  canonicalDomain(domain);
  if (!MUTATION_ARMS.includes(mutation_arm)) throw new Error(`unknown mutation arm: ${mutation_arm}`);
  const before = validateMutationCase({ source, normalization, nondeterministic });
  const { literals } = scanSource(source);
  const candidates = literals.filter((literal) => !literalIsInObserver(source, literal) && !literalIsInType(source, literal));
  const target = candidates[0] || literals[0];
  if (!target) throw new Error("mutation source has no typed value literal");
  const replacement = mutationValue(target, mutation_arm);
  if (replacement === target.raw) throw new Error(`mutation arm ${mutation_arm} does not change its input`);
  const mutated_source = `${source.slice(0, target.start)}${replacement}${source.slice(target.end)}`;
  validateMutationCase({
    source: mutated_source,
    skeleton: before.skeleton,
    normalization,
    nondeterministic,
  });
  return {
    seed,
    domain: canonicalDomain(domain),
    mutation_arm,
    mutator_version: MUTATOR_VERSION,
    source: mutated_source,
    skeleton: before.skeleton,
    observer_fingerprint: before.observer_fingerprint,
    target_kind: target.kind,
  };
}

function caseKey(item) {
  return `${item.stable_surface_id}\u0000${item.seed}\u0000${item.mutation_arm}`;
}

function sourceSeed(item) {
  requireString(item.stable_surface_id, "seed stable_surface_id");
  requireString(item.seed, `seed for ${item.stable_surface_id}`);
  requireString(item.source, `source for ${item.stable_surface_id}`);
  requireString(item.domain, `domain for ${item.stable_surface_id}`);
  return item;
}

export function batchMutations(seeds, { batchSize = DEFAULT_BATCH_SIZE, arms = MUTATION_ARMS } = {}) {
  if (!Array.isArray(seeds)) throw new Error("mutation seeds must be an array");
  if (!Number.isInteger(batchSize) || batchSize < 1 || batchSize > 512) {
    throw new Error("batch size must be an integer from 1 through 512");
  }
  if (!Array.isArray(arms) || arms.length === 0 || arms.some((arm) => !MUTATION_ARMS.includes(arm))) {
    throw new Error("mutation arms must be known, non-empty, and bounded");
  }
  const cases = [];
  const seen = new Set();
  [...seeds]
    .map(sourceSeed)
    .sort((left, right) => `${left.stable_surface_id}\u0000${left.seed}`.localeCompare(`${right.stable_surface_id}\u0000${right.seed}`))
    .forEach((seed) => {
      for (const mutation_arm of [...arms].sort()) {
        const key = `${seed.stable_surface_id}\u0000${seed.seed}\u0000${mutation_arm}`;
        if (seen.has(key)) throw new Error(`duplicate mutation case: ${key}`);
        seen.add(key);
        const mutation = mutateValueSource(seed.source, {
          domain: seed.domain,
          seed: seed.seed,
          mutation_arm,
          normalization: seed.normalization || [],
          nondeterministic: seed.nondeterministic,
        });
        const oracle = oracleAdapter(seed.domain);
        cases.push({
          case_id: sha256(key).slice("sha256:".length, "sha256:".length + 16),
          stable_surface_id: seed.stable_surface_id,
          seed: seed.seed,
          domain: mutation.domain,
          mutation_arm,
          mutator_version: MUTATOR_VERSION,
          source: mutation.source,
          skeleton: mutation.skeleton,
          observer_fingerprint: mutation.observer_fingerprint,
          normalization: [...(seed.normalization || [])].sort(),
          oracle: {
            name: oracle.oracle,
            version: oracle.version,
            input_digest: sha256(canonicalJson({ seed: seed.seed, domain: mutation.domain })),
            independence_class: oracle.independence_class,
            provenance: oracle.provenance,
          },
          expected_relation: `oracle:${oracle.oracle}`,
        });
      }
    });
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
  return { mutator_version: MUTATOR_VERSION, batch_size: batchSize, cases, batches };
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

function normalizeTierObservations(observations) {
  if (observations === undefined) return [];
  if (!Array.isArray(observations)) throw new Error("tier_observations must be an array");
  const seen = new Set();
  return [...observations]
    .map((observation) => {
      if (!observation || !TIERS.includes(observation.tier)) throw new Error("tier observation has an invalid tier");
      if (seen.has(observation.tier)) throw new Error(`duplicate tier observation: ${observation.tier}`);
      seen.add(observation.tier);
      if (!Number.isInteger(observation.exit)) throw new Error(`tier observation ${observation.tier} has no exit`);
      return {
        tier: observation.tier,
        stdout_bytes: encodeBytes(observation.stdout ?? observation.stdout_bytes, `${observation.tier} stdout`),
        stderr_bytes: encodeBytes(observation.stderr ?? observation.stderr_bytes, `${observation.tier} stderr`),
        exit: observation.exit,
        signal: observation.signal ?? null,
        timeout: observation.timeout === true,
        relation: requireString(observation.relation, `${observation.tier} relation`),
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
    return observation.stdout_bytes !== baseline.stdout_bytes
      || observation.stderr_bytes !== baseline.stderr_bytes
      || observation.exit !== baseline.exit
      || observation.signal !== baseline.signal
      || observation.timeout !== baseline.timeout
      || observation.relation !== baseline.relation;
  }).map((observation) => observation.tier);
  return { ok: differences.length === 0, baseline: baseline.tier, differences };
}

export function makeResultBundle(input) {
  if (!input || typeof input !== "object") throw new Error("result bundle input must be an object");
  const normalization = input.normalization;
  if (!Array.isArray(normalization) || normalization.some((item) => typeof item !== "string" || !item)) {
    throw new Error("result bundle normalization must be a named list");
  }
  const source = requiredBundleString(input, "source");
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
  if (!Number.isInteger(bundle.exit)) throw new Error("result bundle exit must be an integer");
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

export function serializeBundles(bundles) {
  if (!Array.isArray(bundles)) throw new Error("result bundles must be an array");
  return [...bundles]
    .map((bundle) => makeResultBundle(bundle))
    .sort((left, right) => `${left.stable_surface_id}\u0000${left.seed}\u0000${left.mutation_arm}\u0000${left.tier}`
      .localeCompare(`${right.stable_surface_id}\u0000${right.seed}\u0000${right.mutation_arm}\u0000${right.tier}`))
    .map(canonicalJson)
    .join("\n") + (bundles.length ? "\n" : "");
}

export const REGRESSION_SEEDS = Object.freeze([
  {
    stable_surface_id: "regression:semantic-equality",
    domain: "numeric",
    seed: "regression-semantic-equality-001",
    source: `fn run() {
    left :: [1, 2]
    right :: [1, 2]
    print(left == right)
}
`,
    input: { a: 7, b: 3, c: 2 },
    expected: "equal=true",
    wrong: "equal=false",
  },
  {
    stable_surface_id: "regression:indexed-place",
    domain: "memory",
    seed: "regression-indexed-place-001",
    source: `fn run() {
    values := [1, 2, 3]
    values[1] = 9
    print(values[1])
}
`,
    input: { operations: ["index", "write:9", "read"] },
    expected: "index[1]=9",
    wrong: "index[1]=2",
  },
  {
    stable_surface_id: "regression:packed-int",
    domain: "numeric",
    seed: "regression-packed-int-001",
    source: `fn run() {
    print(9223372036854775807)
}
`,
    input: { a: 9223372036854775806, b: 1, c: 0 },
    expected: "i64.max=9223372036854775807",
    wrong: "i64.max=-1",
  },
  {
    stable_surface_id: "regression:release-emission",
    domain: "compiler_reflection",
    seed: "regression-release-emission-001",
    source: `fn run() {
    value :: 3
    print(value + 4)
}
`,
    input: { source: "value + 4", shape: "call" },
    expected: "release-output=7",
    wrong: "release-output=0",
  },
  {
    stable_surface_id: "regression:stdin-transport",
    domain: "host_io",
    seed: "regression-stdin-transport-001",
    source: `use core.term as io

fn run() {
    line :: io.readline() ?? return Err("read")
    print(line)
}
`,
    input: { fixture: "stdin", value: "alpha\n" },
    expected: "stdin-bytes=alpha\\n",
    wrong: "stdin-bytes=",
    normalization: ["stdin.fixture"],
  },
]);

function regressionOracle(seed) {
  // Regression fixtures name the seam-specific relation directly. The domain
  // adapter still supplies the independent proof family for the bundle.
  const item = oracleAdapter(seed.domain);
  return {
    name: item.oracle,
    version: item.version,
    input_digest: sha256(canonicalJson(seed.input)),
    independence_class: item.independence_class,
    provenance: item.provenance,
  };
}

export function regressionFindingBundles({
  commit = "unknown-commit",
  binary_sha256 = "sha256:unknown-binary",
  registry_snapshot_hash = "sha256:unknown-registry",
  config_hash = "sha256:unknown-config",
} = {}) {
  return REGRESSION_SEEDS.map((seed) => {
    if (seed.expected === seed.wrong) throw new Error(`regression wrong answer survived ${seed.stable_surface_id}`);
    const tier_observations = TIERS.map((tier) => ({
      tier,
      stdout: `${seed.wrong}\n`,
      stderr: "",
      exit: 0,
      relation: seed.wrong,
    }));
    return makeResultBundle({
      run_id: `regression-${seed.seed}`,
      stable_surface_id: seed.stable_surface_id,
      tier: "jet_run",
      tier_command: "scripts/agent/jet-env jet run <regression.jet>",
      seed: seed.seed,
      mutation_arm: "planted-wrong-answer",
      source: seed.source,
      stdout: `${seed.wrong}\n`,
      stderr: "",
      exit: 0,
      expected_relation: seed.expected,
      actual_relation: seed.wrong,
      normalization: seed.normalization || [],
      oracle: regressionOracle(seed),
      applicable_tiers: TIERS,
      commit,
      binary_sha256,
      registry_snapshot_hash,
      config_hash,
      classification: "P0",
      tower_action: "create-or-update",
      tier_observations,
    });
  });
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
