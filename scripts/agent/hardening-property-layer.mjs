#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalJson,
  compareCaseObservations,
  executeCase,
  makeResultBundle,
  serializeBundles,
  sha256,
} from "./hardening-oracle-layer.mjs";

/**
 * Layer 2 (#2341).  This is a library, not a second runner.  It only creates
 * deterministic law records and cases; process ownership stays with the
 * existing hardening rig and its tier executor.
 */

export const PROPERTY_SCHEMA = "jet.hardening.property.v1";
export const PROPERTY_SCHEMA_VERSION = 1;
export const PROPERTY_MUTATOR_VERSION = "property-law-1";
export const PROPERTY_DEFAULT_SEED = "2341";
export const PROPERTY_DEFAULT_MAX_CASES = 128;
export const PROPERTY_MAX_CASES = 4096;
export const PROPERTY_DEFAULT_BATCH_SIZE = 32;
export const PROPERTY_MAX_BATCH_SIZE = 512;

const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(SCRIPT_DIR, "../..");
const STATE_PACKS = new Set([
  "iterator-view",
  "host-isolation",
  "protocol-db",
  "task-cancellation",
  "association",
  "freeze",
  "copy",
]);

function clone(value) {
  if (value === undefined) return undefined;
  return typeof structuredClone === "function" ? structuredClone(value) : JSON.parse(JSON.stringify(value));
}

function freezeDeep(value) {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) return value;
  for (const child of Object.values(value)) freezeDeep(child);
  return Object.freeze(value);
}

function stableText(value) {
  const preserve = (candidate) => {
    if (typeof candidate === "number") {
      if (Number.isNaN(candidate)) return { "$number": "NaN" };
      if (candidate === Number.POSITIVE_INFINITY) return { "$number": "+Infinity" };
      if (candidate === Number.NEGATIVE_INFINITY) return { "$number": "-Infinity" };
      if (Object.is(candidate, -0)) return { "$number": "-0" };
      return candidate;
    }
    if (candidate === null || typeof candidate !== "object") return candidate;
    if (Array.isArray(candidate)) return candidate.map(preserve);
    return Object.fromEntries(Object.entries(candidate).map(([key, child]) => [key, preserve(child)]));
  };
  return canonicalJson(preserve(value));
}

function digest(value) {
  return sha256(stableText(value));
}

function hashSeed(seed) {
  const bytes = createHash("sha256").update(String(seed), "utf8").digest();
  return bytes.readUInt32LE(0) || 1;
}

function nextRandom(state) {
  let value = state.value >>> 0;
  value ^= value << 13;
  value ^= value >>> 17;
  value ^= value << 5;
  state.value = value >>> 0;
  return state.value;
}

function boundedSeed(seed, count) {
  const state = { value: hashSeed(seed) };
  const rows = [];
  for (let index = 0; index < count; index += 1) rows.push(nextRandom(state));
  return rows;
}

function matchingDelimited(text, start, opening, closing) {
  let depth = 0;
  let quote = false;
  let escaped = false;
  for (let index = start; index < text.length; index += 1) {
    const character = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quote = false;
      continue;
    }
    if (character === '"') quote = true;
    else if (character === opening) depth += 1;
    else if (character === closing && --depth === 0) return index;
  }
  throw new Error(`unclosed registry ${opening}${closing}`);
}

function splitRegistryArgs(text) {
  const parts = [];
  let start = 0;
  const depth = { "(": 0, "[": 0, "{": 0 };
  const closing = { ")": "(", "]": "[", "}": "{" };
  let quote = false;
  let escaped = false;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quote = false;
      continue;
    }
    if (character === '"') quote = true;
    else if (Object.hasOwn(depth, character)) depth[character] += 1;
    else if (Object.hasOwn(closing, character)) depth[closing[character]] -= 1;
    else if (character === "," && Object.values(depth).every((value) => value === 0)) {
      parts.push(text.slice(start, index).trim());
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts;
}

function rustStrings(text) {
  return [...text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g)].map((match) => {
    try { return JSON.parse(`"${match[1]}"`); } catch { return match[1]; }
  });
}

function registryKey(row) {
  const stableId = row?.stable_id;
  if (typeof stableId !== "string") return { kind: null, owner: null, member: null };
  const match = stableId.match(/^(module|receiver):(.+)\.([^\.]+)$/);
  if (!match) return { kind: null, owner: null, member: null };
  return { kind: match[1], owner: match[2], member: match[3] };
}

function registryDomain(module) {
  const parts = module.split(".");
  return parts[0] === "core" ? parts[1] || "core" : parts[0];
}

function parseCoreRegistrySurfaces(source) {
  const events = [];
  const pattern = /CoreCallRecord::(new|receiver)\s*\(/g;
  for (const match of source.matchAll(pattern)) {
    const open = source.indexOf("(", match.index);
    const close = matchingDelimited(source, open, "(", ")");
    events.push({ kind: match[1], start: match.index, close, args: splitRegistryArgs(source.slice(open + 1, close)) });
  }
  const rows = [];
  for (const [index, event] of events.entries()) {
    const block = source.slice(event.start, events[index + 1]?.start ?? source.length);
    const module = event.kind === "receiver" ? null : rustStrings(event.args[0] || "")[0];
    const owners = event.kind === "receiver" ? rustStrings(event.args[0] || "") : [module];
    const member = rustStrings(event.args[1] || "")[0];
    if (!member || (event.kind === "new" && !module)) continue;
    for (const owner of owners) {
      const stable_id = event.kind === "receiver" ? `receiver:${owner}.${member}` : `module:${owner}.${member}`;
      const tags = sortedUnique([
        ...(owner ? owner.split(".") : []),
        owner,
        ...(owner ? owner.split(".").filter((part) => part.endsWith("s")).map((part) => part.slice(0, -1)) : []),
      ]);
      rows.push({
        stable_id,
        kind: event.kind === "receiver" ? "receiver_method" : "module_call",
        owner,
        module: owner,
        member,
        domain: owner ? registryDomain(owner) : "core",
        tags,
        surface_tags: tags,
        property_tags: tags,
        applicable_tiers: event.kind === "receiver" ? ["interpreter"] : [...TIERS],
        status: "covered",
        value_consuming: true,
        exclusion: null,
        registry_arity: (event.args[event.kind === "receiver" ? 2 : 4] || "").match(/true|false/g)?.length ?? 0,
        registry_source: "crates/jet-foundation/src/Syntax/core_calls.rs",
        registry_direct_aot: !/\.without_direct_aot\s*\(\s*\)/.test(block),
        registry_direct_jit: !/\.without_direct_jit\s*\(\s*\)/.test(block),
      });
    }
  }
  return rows;
}

export function coreRegistrySurfaces({ root = DEFAULT_ROOT, source = null } = {}) {
  const registrySource = source ?? readFileSync(join(root, "crates/jet-foundation/src/Syntax/core_calls.rs"), "utf8");
  return parseCoreRegistrySurfaces(String(registrySource));
}

function exact(expected, actual) {
  return { ok: stableText(expected) === stableText(actual), expected, actual };
}

function sortedUnique(values) {
  return [...new Set(values.filter((value) => typeof value === "string" && value.length > 0))]
    .sort();
}

function asString(value) {
  return JSON.stringify(String(value));
}

function jetLiteral(value) {
  if (typeof value === "string") return asString(value);
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") {
    if (Number.isNaN(value)) return "Float.NAN";
    if (value === Number.POSITIVE_INFINITY) return "Float.INFINITY";
    if (value === Number.NEGATIVE_INFINITY) return "-Float.INFINITY";
    if (Object.is(value, -0)) return "-0.0";
    return String(value);
  }
  if (value === null) return "none";
  if (Array.isArray(value)) {
    if (value.length === 0) return "[Int]{}";
    return `[Int]{${value.map(jetLiteral).join(", ")}}`;
  }
  return asString(stableText(value));
}

function seededInputFor(stableId, entropy) {
  const value = entropy >>> 0;
  const signed = (value % 2001) - 1000;
  const text = ["", "Jet", "e\u0301", "𝄞"][value % 4];
  switch (stableId) {
    case "property.numeric.add-identity": return { value: signed, identity: 0 };
    case "property.numeric.order": return { left: signed, right: signed + (value % 11) - 5 };
    case "property.float.classification": return { value: (value % 2001) / 100 };
    case "property.float.bounded-error": return { left: (value % 1000) / 10, right: ((value >>> 8) % 1000) / 10 };
    case "property.collections.order-membership": {
      const values = [value % 7, (value >>> 5) % 7, (value >>> 10) % 7];
      return { values, needle: values[value % values.length] };
    }
    case "property.unicode.codepoint": return { value: text };
    case "property.parsing.roundtrip": return { value: ["0", " 1 ", "[1, 2]", "{}"][value % 4] };
    case "property.encoding.bytes-roundtrip": return { value: ["", "A", "\u0000\u00ff", "é"][value % 4] };
    case "property.serde.roundtrip": return { value: { id: value % 17, values: [value % 3, (value >>> 4) % 3], ok: value % 2 === 0 } };
    case "property.time.calendar-composition": return { day: 19000 + (value % 1000), delta: (value % 7) - 3 };
    case "property.crypto.known-transformation": return { value: text };
    case "property.rng.seeded-range-determinism": return { seed: value, bound: 1 + (value % 1000) };
    case "property.iterator-view.transition": return { state: ["empty", "ready", "exhausted", "reborrow"][value % 4], value: value % 17 };
    case "property.host-isolation.transition": return { state: ["file", "path", "env", "process"][value % 4], value: text || "JET_CASE=1" };
    case "property.protocol-db.transition": return { state: ["open", "transaction", "closed"][value % 3], value: ["close", "commit", "rollback"][value % 3] };
    case "property.task-cancellation.transition": return { state: ["running", "queued", "failed"][value % 3], value: ["complete", "cancel", "join"][value % 3] };
    case "property.association.transition": return { state: ["free", "associated", "consumed"][value % 3], value: ["associate", "use", "consume"][value % 3] };
    case "property.freeze.transition": return { value: { n: value % 17, nested: { ok: value % 2 === 0 } } };
    case "property.copy.transition": return { value: { n: value % 17, child: { n: (value >>> 4) % 17 } } };
    default: return null;
  }
}

function sourceBody(law, partition, index, inputOverride = undefined) {
  const input = inputOverride === undefined ? partition.input : inputOverride;
  switch (law.stable_id) {
    case "property.numeric.add-identity":
      return [
        `    left :: ${jetLiteral(input.value)}`,
        `    zero :: ${jetLiteral(input.identity)}`,
        "    value :: left + zero",
        "    print(value)",
      ];
    case "property.numeric.order":
      return [
        `    left :: ${jetLiteral(input.left)}`,
        `    right :: ${jetLiteral(input.right)}`,
        "    value :: left <= right",
        "    print(value)",
      ];
    case "property.float.classification":
      return [
        `    value :: ${jetLiteral(input.value)}`,
        "    result :: value == value",
        "    print(result)",
      ];
    case "property.float.bounded-error":
      return [
        `    left :: ${jetLiteral(input.left)}`,
        `    right :: ${jetLiteral(input.right)}`,
        "    value :: left + right",
        "    print(value)",
      ];
    case "property.collections.order-membership":
      return [
        `    values :: ${jetLiteral(input.values)}`,
        `    needle :: ${jetLiteral(input.needle)}`,
        "    print(values.len())",
        "    print(values.contains(needle))",
      ];
    case "property.unicode.codepoint":
    case "property.parsing.roundtrip":
    case "property.encoding.bytes-roundtrip":
    case "property.serde.roundtrip":
      return [
        `    value :: ${jetLiteral(input.value)}`,
        "    print(value)",
      ];
    case "property.time.calendar-composition":
      return [
        `    value :: ${jetLiteral(input.day)}`,
        `    delta :: ${jetLiteral(input.delta)}`,
        "    result :: value + delta",
        "    print(result)",
      ];
    case "property.crypto.known-transformation":
      return [
        `    value :: ${jetLiteral(input.value)}`,
        "    print(value)",
      ];
    case "property.rng.seeded-range-determinism":
      return [
        `    seed :: ${jetLiteral(input.seed)}`,
        `    bound :: ${jetLiteral(input.bound)}`,
        "    print(seed)",
        "    print(bound)",
      ];
    default:
      return [
        `    value :: ${jetLiteral(input.value ?? input.state ?? index)}`,
        "    print(value)",
      ];
  }
}

function law({
  stable_id,
  pack,
  family,
  domains,
  surface_tags,
  surface_set = surface_tags,
  precondition,
  precondition_fn = null,
  generated_partitions,
  observable_sink,
  shrink_rule,
  relation,
  seed,
  partitions,
  evaluate,
  wrong,
  wrong_relation = "the planted relation must disagree with the named law",
  type_constraints,
  fixture = null,
  generate_input = null,
  surface_predicate = null,
}) {
  const seeded = seededInputFor(stable_id, hashSeed(seed));
  const allPartitions = seeded === null
    ? partitions
    : [...partitions, { id: "seeded", input: seeded }];
  const inputGenerator = generate_input || ((input, context) => (
    context.partition.id === "seeded"
      ? seededInputFor(stable_id, context.entropy)
      : input
  ));
  const resolvedFixture = fixture || (family === "state"
    ? {
        kind: "case-scratch",
        isolation: "one fixture root per law case",
        deterministic: true,
        setup: "create declared local state before tier execution",
        cleanup: "remove only the case-owned fixture root after observation",
      }
    : null);
  const record = {
    stable_id,
    pack,
    family,
    domains: sortedUnique(domains),
    surface_tags: sortedUnique(surface_tags),
    surface_set: sortedUnique(surface_set),
    precondition,
    generated_partitions: sortedUnique([
      ...generated_partitions,
      ...(seeded === null ? [] : ["seeded"]),
    ]),
    applicable_tiers: [...TIERS],
    observable_sink: { ...observable_sink },
    shrink_rule,
    relation,
    wrong_relation,
    seed,
    type_constraints: sortedUnique(type_constraints),
  };
  if (resolvedFixture) record.fixture = clone(resolvedFixture);
  record.input_generator = "deterministic-xorshift32";
  const surfacePredicate = surface_predicate || ((row) => {
    const tags = rowTags(row);
    return domains.some((domain) => tags.has(domain.toLowerCase()))
      || surface_tags.some((tag) => tags.has(tag.toLowerCase()))
      || surface_set.some((tag) => tags.has(tag.toLowerCase()));
  });
  return freezeDeep({
    ...record,
    partitions: freezeDeep(allPartitions.map((item) => freezeDeep({ ...item }))),
    evaluate,
    wrong,
    precondition_fn,
    generate_input: inputGenerator,
    surface_predicate: surfacePredicate,
    fixture: resolvedFixture ? freezeDeep(clone(resolvedFixture)) : null,
    record: freezeDeep(record),
  });
}

const COLLECTION_MEMBERS = new Set([
  "count", "table", "rows", "series", "values", "schema", "lazy", "plan", "collect",
  "filter", "sort_by", "lazy_filter", "lazy_sort_by", "csv", "json", "csv_reader", "json_reader",
  "map_new", "map_get", "map_merge", "map_show", "list_new", "list_merge", "list_show",
  "from_list", "to_list",
]);
const SERDE_MEMBERS = new Set([
  "parse", "parse_bytes", "parse_with", "decode", "encode", "events", "to_string", "to_string_pretty",
  "to_bytes", "to_bytes_canonical", "canonical", "root", "attribute", "content",
]);
const SERDE_MODULES = new Set([
  "core.encoding.base32", "core.encoding.base64", "core.encoding.cbor", "core.encoding.csv",
  "core.encoding.hex", "core.encoding.json", "core.encoding.jsonl", "core.encoding.toml",
  "core.encoding.xml", "core.encoding.yaml",
]);
const PARSING_MEMBERS = new Set(["check", "decode", "lex", "parse", "parse_with", "source_map"]);
const ITERATOR_MEMBERS = new Set([
  "lazy", "plan", "collect", "filter", "sort_by", "lazy_filter", "lazy_sort_by", "rows", "values",
]);
const COPY_MEMBERS = new Set(["copy", "table", "series", "rows", "values", "collect", "to_list"]);

function rowCall(row) {
  const key = registryKey(row);
  return {
    kind: row?.kind || key.kind,
    owner: row?.owner || row?.module || key.owner,
    member: row?.member || key.member,
  };
}

function rowTypeConstraints(row) {
  if (Array.isArray(row?.registry_type_constraints)) return row.registry_type_constraints;
  if (Array.isArray(row?.type_constraints)) return row.type_constraints;
  return [];
}

function matchesTypes(row, expected) {
  const actual = rowTypeConstraints(row);
  return actual.length === 0 || actual.some((type) => expected.includes(type));
}

function collectionSurface(row) {
  const call = rowCall(row);
  return call.kind === "module_call"
    && ["core.data", "core.sync", "core.compute"].includes(call.owner)
    && COLLECTION_MEMBERS.has(call.member)
    && matchesTypes(row, [
      "List<T>", "List<Float>", "Table<T>", "Series<T>", "LazyFrame<T>", "DataStream<T>",
      "FileReader", "SyncList", "SyncMap", "Tensor",
    ]);
}

function serdeSurface(row) {
  const call = rowCall(row);
  return call.kind === "module_call"
    && SERDE_MODULES.has(call.owner)
    && SERDE_MEMBERS.has(call.member)
    && matchesTypes(row, ["DataTree", "Codable", "List<U8>", "String"]);
}

function parsingSurface(row) {
  const call = rowCall(row);
  const compiler = call.kind === "module_call"
    && call.owner === "core.compiler"
    && PARSING_MEMBERS.has(call.member);
  const encoding = call.kind === "module_call"
    && call.owner?.startsWith("core.encoding.")
    && PARSING_MEMBERS.has(call.member);
  const uuid = call.kind === "module_call"
    && call.owner === "core.crypto.uuid"
    && call.member === "parse";
  return (compiler || encoding || uuid)
    && matchesTypes(row, ["DataTree", "Codable", "List<U8>", "String", "Syntax"]);
}

function randomSurface(row) {
  const call = rowCall(row);
  return call.kind === "module_call"
    && ["core.math.random", "core.crypto.random"].includes(call.owner)
    && matchesTypes(row, ["Int", "Float", "Rng", "List<U8>"]);
}

function iteratorSurface(row) {
  const call = rowCall(row);
  return ((call.kind === "module_call" && call.owner === "core.data" && ITERATOR_MEMBERS.has(call.member))
    || (call.kind === "module_call" && call.owner === "core.tasks" && call.member === "interval"))
    && matchesTypes(row, [
      "List<T>", "Table<T>", "Series<T>", "LazyFrame<T>", "DataStream<T>", "Duration", "Receiver<Int>",
    ]);
}

function copySurface(row) {
  const call = rowCall(row);
  return (call.kind === "module_call" && call.owner === "core.math" && call.member === "copy")
    || (call.kind === "module_call" && call.owner === "core.data" && COPY_MEMBERS.has(call.member))
    || (call.kind === "module_call" && call.owner === "core.compute" && call.member === "to_list")
    ? matchesTypes(row, [
      "Int", "Float", "List<T>", "List<Float>", "Table<T>", "Series<T>", "LazyFrame<T>", "Tensor",
    ])
    : false;
}

function numericLaws() {
  return [
    law({
      stable_id: "property.numeric.add-identity",
      pack: "numeric",
      family: "pure",
      domains: ["numeric", "numeric_decimal"],
      surface_tags: ["numeric", "math", "decimal"],
      precondition: "left and identity are finite numeric values; identity == 0",
      generated_partitions: ["zero", "positive-boundary", "negative-boundary", "large-integer"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(value)", type_aware: true },
      shrink_rule: "shrink absolute value toward zero while preserving identity partition",
      relation: "add(left, identity) == left",
      seed: "property-numeric-add-identity-001",
      type_constraints: ["Int", "Float"],
      partitions: [
        { id: "zero", input: { value: 0, identity: 0 } },
        { id: "positive-boundary", input: { value: 1, identity: 0 } },
        { id: "negative-boundary", input: { value: -1, identity: 0 } },
        { id: "large-integer", input: { value: 2147483647, identity: 0 } },
      ],
      evaluate: ({ value, identity }) => value + identity,
      wrong: ({ value }) => value + 1,
    }),
    law({
      stable_id: "property.numeric.order",
      pack: "numeric",
      family: "pure",
      domains: ["numeric", "numeric_decimal"],
      surface_tags: ["numeric", "math", "order"],
      precondition: "left and right are finite comparable numeric values",
      generated_partitions: ["equal", "ascending", "descending", "signed-boundary"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(value)", type_aware: true },
      shrink_rule: "shrink both operands toward the nearest equal-order boundary",
      relation: "left <= right matches the ordered numeric relation",
      seed: "property-numeric-order-001",
      type_constraints: ["Int", "Float"],
      partitions: [
        { id: "equal", input: { left: 0, right: 0 } },
        { id: "ascending", input: { left: -1, right: 1 } },
        { id: "descending", input: { left: 1, right: -1 } },
        { id: "signed-boundary", input: { left: -2147483648, right: 2147483647 } },
      ],
      evaluate: ({ left, right }) => left <= right,
      wrong: ({ left, right }) => left < right,
    }),
  ];
}

function pureLaws() {
  return [
    ...numericLaws(),
    law({
      stable_id: "property.float.classification",
      pack: "float",
      family: "pure",
      domains: ["float"],
      surface_tags: ["float", "math", "classification"],
      precondition: "value is an IEEE-754 value; NaN classification is compared by class, not payload",
      generated_partitions: ["zero", "negative-zero", "finite", "infinity", "nan"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(is_nan(value), is_infinite(value))", type_aware: true },
      shrink_rule: "shrink finite magnitude toward zero; preserve signed-zero and non-finite class",
      relation: "classification(value) is stable under observation",
      seed: "property-float-classification-001",
      type_constraints: ["Float"],
      partitions: [
        { id: "zero", input: { value: 0 } },
        { id: "negative-zero", input: { value: -0 } },
        { id: "finite", input: { value: 0.1 + 0.2 } },
        { id: "infinity", input: { value: Number.POSITIVE_INFINITY } },
        { id: "nan", input: { value: Number.NaN } },
      ],
      evaluate: ({ value }) => ({ nan: Number.isNaN(value), infinite: !Number.isFinite(value) && !Number.isNaN(value), negative_zero: Object.is(value, -0) }),
      wrong: () => ({ nan: false, infinite: false, negative_zero: true }),
    }),
    law({
      stable_id: "property.float.bounded-error",
      pack: "float",
      family: "pure",
      domains: ["float"],
      surface_tags: ["float", "math", "ulp"],
      precondition: "operands are finite and expected error is bounded by 2 ulps",
      generated_partitions: ["small", "cancellation", "large"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(value)", type_aware: true },
      shrink_rule: "shrink exponent and mantissa while retaining the error boundary",
      relation: "abs(actual - expected) <= 2 * ulp(expected)",
      seed: "property-float-error-001",
      type_constraints: ["Float"],
      partitions: [
        { id: "small", input: { left: 0.1, right: 0.2 } },
        { id: "cancellation", input: { left: 10000000000000000, right: -10000000000000000 } },
        { id: "large", input: { left: 1e150, right: 1e150 } },
      ],
      evaluate: ({ left, right }) => left + right,
      wrong: ({ left, right }) => left + right + Number.EPSILON,
    }),
    law({
      stable_id: "property.collections.order-membership",
      pack: "collections",
      family: "pure",
      domains: ["collections", "collection", "list", "map", "set"],
      surface_tags: ["collections", "list", "map", "set", "order", "membership"],
      precondition: "collection is finite; membership key is representable by the collection element type",
      generated_partitions: ["empty", "singleton", "duplicate", "ordered"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(values.len()); print(values.contains(needle))", type_aware: true },
      shrink_rule: "remove elements from the tail while retaining first membership witness",
      relation: "length and insertion order are stable; membership agrees with the element relation",
      seed: "property-collections-order-001",
      surface_set: ["core.data", "core.sync", "core.compute"],
      type_constraints: ["List<T>", "Table<T>", "Series<T>", "SyncList", "SyncMap"],
      surface_predicate: collectionSurface,
      generate_input: (input, context) => {
        const entropy = context.entropy >>> 0;
        const values = [entropy % 7, (entropy >>> 5) % 7, (entropy >>> 10) % 7];
        return { values, needle: values[entropy % values.length] };
      },
      partitions: [
        { id: "empty", input: { values: [], needle: 0 } },
        { id: "singleton", input: { values: [1], needle: 1 } },
        { id: "duplicate", input: { values: [1, 1, 2], needle: 2 } },
        { id: "ordered", input: { values: [3, 1, 2], needle: 1 } },
      ],
      evaluate: ({ values, needle }) => ({ length: values.length, values: [...values], member: values.includes(needle), ordered: true }),
      wrong: ({ values, needle }) => ({ length: values.length, values: [...values].reverse(), member: values.includes(needle), ordered: false }),
    }),
    law({
      stable_id: "property.unicode.codepoint",
      pack: "unicode",
      family: "pure",
      domains: ["text_unicode", "text", "unicode"],
      surface_tags: ["unicode", "text", "codepoint"],
      precondition: "input is valid UTF-8 and contains no unpaired surrogate",
      generated_partitions: ["ascii", "combining-mark", "supplementary-plane", "empty"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(value.len()); print(value)", type_aware: true },
      shrink_rule: "remove complete Unicode scalar values, never split UTF-8 sequences",
      relation: "UTF-8 roundtrip preserves scalar sequence and normalization is named",
      seed: "property-unicode-codepoint-001",
      type_constraints: ["String"],
      partitions: [
        { id: "ascii", input: { value: "Jet" } },
        { id: "combining-mark", input: { value: "e\u0301" } },
        { id: "supplementary-plane", input: { value: "𝄞" } },
        { id: "empty", input: { value: "" } },
      ],
      evaluate: ({ value }) => ({ scalars: Array.from(value).length, value, nfc: value.normalize("NFC") }),
      wrong: ({ value }) => ({ scalars: value.length, value, nfc: `${value}\u0000` }),
    }),
    law({
      stable_id: "property.parsing.roundtrip",
      pack: "parsing",
      family: "pure",
      domains: ["parsing", "compiler_reflection"],
      surface_tags: ["parse", "format", "compiler"],
      precondition: "value belongs to the documented parser/formatter grammar",
      generated_partitions: ["minimal", "whitespace", "nested", "boundary"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(format(parse(value)))", type_aware: true },
      shrink_rule: "remove optional whitespace and nested nodes while retaining the parse/format witness",
      relation: "parse(format(value)) == value for canonical values",
      seed: "property-parsing-roundtrip-001",
      surface_set: [
        "core.compiler", "core.crypto.uuid", "core.encoding.cbor", "core.encoding.csv",
        "core.encoding.json", "core.encoding.jsonl", "core.encoding.toml", "core.encoding.xml",
      ],
      type_constraints: ["String", "Syntax"],
      surface_predicate: parsingSurface,
      partitions: [
        { id: "minimal", input: { value: "1" } },
        { id: "whitespace", input: { value: " 1 " } },
        { id: "nested", input: { value: "[1, 2]" } },
        { id: "boundary", input: { value: "{}" } },
      ],
      evaluate: ({ value }) => value.trim(),
      wrong: ({ value }) => `${value.trim()}!`,
    }),
    law({
      stable_id: "property.encoding.bytes-roundtrip",
      pack: "encoding",
      family: "pure",
      domains: ["bytes_encoding_serde", "bytes", "encoding"],
      surface_tags: ["bytes", "encoding", "base64", "hex"],
      precondition: "byte sequence is finite and encoding alphabet is valid",
      generated_partitions: ["empty", "one-byte", "binary", "unicode-utf8"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(decode(encode(bytes)))", type_aware: true },
      shrink_rule: "drop trailing bytes while preserving the first non-ASCII byte",
      relation: "decode(encode(bytes)) == bytes exactly",
      seed: "property-encoding-roundtrip-001",
      type_constraints: ["Bytes"],
      partitions: [
        { id: "empty", input: { value: "" } },
        { id: "one-byte", input: { value: "A" } },
        { id: "binary", input: { value: "\u0000\u00ff" } },
        { id: "unicode-utf8", input: { value: "é" } },
      ],
      evaluate: ({ value }) => Buffer.from(value, "utf8").toString("base64"),
      wrong: ({ value }) => `${Buffer.from(value, "utf8").toString("base64")}!`,
    }),
    law({
      stable_id: "property.serde.roundtrip",
      pack: "serde",
      family: "pure",
      domains: ["json_codable", "json", "codable", "serde_json"],
      surface_tags: ["serde", "json", "codable", "roundtrip"],
      precondition: "value is in the supported Codable subset and has no volatile fields",
      generated_partitions: ["scalar", "nested", "empty-collection", "mixed"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(decode(encode(value)))", type_aware: true },
      shrink_rule: "remove optional object fields and tail collection elements",
      relation: "decode(encode(value)) == value under canonical key ordering",
      seed: "property-serde-roundtrip-001",
      surface_set: [
        "core.encoding.base32", "core.encoding.base64", "core.encoding.cbor", "core.encoding.csv",
        "core.encoding.hex", "core.encoding.json", "core.encoding.jsonl", "core.encoding.toml",
        "core.encoding.xml", "core.encoding.yaml",
      ],
      type_constraints: ["DataTree", "Codable"],
      surface_predicate: serdeSurface,
      generate_input: (input, context) => {
        const entropy = context.entropy >>> 0;
        return {
          value: {
            id: entropy % 17,
            values: [entropy % 3, (entropy >>> 4) % 3],
            ok: entropy % 2 === 0,
          },
        };
      },
      partitions: [
        { id: "scalar", input: { value: { id: 1 } } },
        { id: "nested", input: { value: { id: 1, nested: { enabled: true } } } },
        { id: "empty-collection", input: { value: { values: [] } } },
        { id: "mixed", input: { value: { id: 2, values: [1, 2], ok: false } } },
      ],
      evaluate: ({ value }) => clone(value),
      wrong: ({ value }) => ({ ...clone(value), _wrong: true }),
    }),
    law({
      stable_id: "property.time.calendar-composition",
      pack: "time",
      family: "pure",
      domains: ["time"],
      surface_tags: ["time", "date", "calendar", "duration"],
      precondition: "calendar date is valid in the pinned tzdata version",
      generated_partitions: ["ordinary-day", "month-end", "leap-day", "dst-boundary"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(result.format_rfc3339())", type_aware: true },
      shrink_rule: "reduce duration while retaining month-end, leap, or DST boundary",
      relation: "compose(decompose(value)) == value and calendar addition is associative where defined",
      seed: "property-time-calendar-001",
      type_constraints: ["Date", "DateTime", "Duration", "Period"],
      partitions: [
        { id: "ordinary-day", input: { day: 19723, delta: 1 } },
        { id: "month-end", input: { day: 19752, delta: 1 } },
        { id: "leap-day", input: { day: 20517, delta: 1 } },
        { id: "dst-boundary", input: { day: 20159, delta: 3600 } },
      ],
      evaluate: ({ day, delta }) => day + delta,
      wrong: ({ day, delta }) => day + delta + 1,
    }),
    law({
      stable_id: "property.crypto.known-transformation",
      pack: "crypto",
      family: "pure",
      domains: ["crypto"],
      surface_tags: ["crypto", "hash", "known-answer"],
      precondition: "algorithm and byte input are supported by the documented vector",
      generated_partitions: ["empty", "ascii", "binary", "known-vector"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(digest(value))", type_aware: true },
      shrink_rule: "shrink input toward the shortest vector that still differs",
      relation: "known transformation matches its published vector exactly",
      seed: "property-crypto-known-answer-001",
      type_constraints: ["Bytes", "Digest"],
      partitions: [
        { id: "empty", input: { value: "" } },
        { id: "ascii", input: { value: "Jet" } },
        { id: "binary", input: { value: "\u0000\u00ff" } },
        { id: "known-vector", input: { value: "abc" } },
      ],
      evaluate: ({ value }) => createHash("sha256").update(value, "utf8").digest("hex"),
      wrong: ({ value }) => createHash("sha256").update(value, "utf8").digest("hex").replace(/.$/, "0"),
    }),
    law({
      stable_id: "property.rng.seeded-range-determinism",
      pack: "seeded-random",
      family: "pure",
      domains: ["rng_uuid", "rng", "uuid"],
      surface_tags: ["rng", "random", "seed", "range"],
      precondition: "seed is explicit and bound is a positive integer",
      generated_partitions: ["zero-seed", "small-bound", "large-bound", "repeat-seed"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(first == second); print(first >= 0 && first < bound)", type_aware: true },
      shrink_rule: "shrink bound toward one and seed toward zero while preserving repeatability",
      relation: "same explicit seed gives same sequence; every value is in [0, bound)",
      seed: "property-rng-seeded-001",
      surface_set: ["core.math.random", "core.crypto.random"],
      type_constraints: ["Int", "Rng", "List<U8>"],
      surface_predicate: randomSurface,
      generate_input: (input, context) => {
        const entropy = context.entropy >>> 0;
        return { seed: entropy, bound: 1 + (entropy % 1000) };
      },
      partitions: [
        { id: "zero-seed", input: { seed: 0, bound: 1 } },
        { id: "small-bound", input: { seed: 7, bound: 3 } },
        { id: "large-bound", input: { seed: 99, bound: 2147483647 } },
        { id: "repeat-seed", input: { seed: 42, bound: 100 } },
      ],
      evaluate: ({ seed, bound }) => {
        const values = boundedSeed(seed, 2).map((value) => value % bound);
        return { values, in_range: values.every((value) => value >= 0 && value < bound) };
      },
      wrong: ({ seed, bound }) => ({ values: [seed % bound, (seed + 1) % bound], in_range: false }),
    }),
  ];
}

function stateLaws() {
  return [
    law({
      stable_id: "property.iterator-view.transition",
      pack: "iterator-view",
      family: "state",
      domains: ["memory", "views", "collections"],
      surface_tags: ["iterator", "view", "memory"],
      precondition: "iterator/view source remains owned and finite for the transition",
      generated_partitions: ["empty", "single-step", "exhaustion", "reborrow"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(next_value); print(done)", type_aware: true },
      shrink_rule: "remove steps after the first transition that differs",
      relation: "next/peek/exhaustion transitions preserve the source order and ownership state",
      seed: "property-iterator-view-001",
      surface_set: ["core.data", "core.tasks"],
      type_constraints: ["Iter<T>", "Table<T>", "LazyFrame<T>", "DataStream<T>", "Receiver<T>"],
      surface_predicate: iteratorSurface,
      generate_input: (input, context) => {
        const entropy = context.entropy >>> 0;
        return { state: ["empty", "ready", "exhausted", "reborrow"][entropy % 4], value: entropy % 17 };
      },
      partitions: [
        { id: "empty", input: { state: "empty", value: 0 } },
        { id: "single-step", input: { state: "ready", value: 1 } },
        { id: "exhaustion", input: { state: "exhausted", value: 2 } },
        { id: "reborrow", input: { state: "reborrow", value: 3 } },
      ],
      evaluate: ({ state, value }) => ({ state, value, transition: state === "ready" ? "yield" : "done" }),
      wrong: ({ state, value }) => ({ state, value, transition: "yield" }),
    }),
    law({
      stable_id: "property.host-isolation.transition",
      pack: "host-isolation",
      family: "state",
      domains: ["host_io", "files", "path", "env", "process"],
      surface_tags: ["file", "path", "env", "process", "isolation"],
      precondition: "fixture path is inside the per-case scratch root and process environment is explicit",
      generated_partitions: ["file-write-read", "path-normalize", "env-overlay", "process-exit"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(bytes); print(exit)", type_aware: true },
      shrink_rule: "retain the shortest fixture transition that changes bytes, path, env, or exit",
      relation: "isolated fixture state is reproducible and cannot escape its case root",
      seed: "property-host-isolation-001",
      type_constraints: ["Path", "Bytes", "Env", "Process"],
      partitions: [
        { id: "file-write-read", input: { state: "file", value: "alpha" } },
        { id: "path-normalize", input: { state: "path", value: "a/../b" } },
        { id: "env-overlay", input: { state: "env", value: "JET_CASE=1" } },
        { id: "process-exit", input: { state: "process", value: 0 } },
      ],
      evaluate: ({ state, value }) => ({ state, value, isolated: true }),
      wrong: ({ state, value }) => ({ state, value, isolated: false }),
    }),
    law({
      stable_id: "property.protocol-db.transition",
      pack: "protocol-db",
      family: "state",
      domains: ["protocol", "network", "db"],
      surface_tags: ["protocol", "network", "database", "transaction"],
      precondition: "local peer/DB fixture is available and every message/transaction is explicit",
      generated_partitions: ["open-close", "commit", "rollback", "invalid-order"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(state); print(result)", type_aware: true },
      shrink_rule: "drop suffix messages after the first invalid state transition",
      relation: "state-machine transitions accept only the documented message/transaction order",
      seed: "property-protocol-db-001",
      type_constraints: ["Protocol", "Db", "Transaction"],
      partitions: [
        { id: "open-close", input: { state: "open", value: "close" } },
        { id: "commit", input: { state: "transaction", value: "commit" } },
        { id: "rollback", input: { state: "transaction", value: "rollback" } },
        { id: "invalid-order", input: { state: "closed", value: "commit" } },
      ],
      evaluate: ({ state, value }) => ({ state, next: value, accepted: !(state === "closed" && value === "commit") }),
      wrong: ({ state, value }) => ({ state, next: value, accepted: false }),
    }),
    law({
      stable_id: "property.task-cancellation.transition",
      pack: "task-cancellation",
      family: "state",
      domains: ["concurrency", "tasks"],
      surface_tags: ["task", "cancellation", "join", "association"],
      precondition: "task fixture has a deterministic completion/cancellation point",
      generated_partitions: ["complete", "cancel-before-run", "cancel-after-run", "join-failure"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(status); print(joined)", type_aware: true },
      shrink_rule: "remove task steps after cancellation is observed",
      relation: "cancellation is terminal, associated join observes exactly one outcome",
      seed: "property-task-cancellation-001",
      type_constraints: ["Task<T>", "TaskFailure"],
      partitions: [
        { id: "complete", input: { state: "running", value: "complete" } },
        { id: "cancel-before-run", input: { state: "queued", value: "cancel" } },
        { id: "cancel-after-run", input: { state: "running", value: "cancel" } },
        { id: "join-failure", input: { state: "failed", value: "join" } },
      ],
      evaluate: ({ state, value }) => ({ state, outcome: value, terminal: value === "cancel" || value === "complete" || state === "failed" }),
      wrong: ({ state, value }) => ({ state, outcome: value, terminal: false }),
    }),
    law({
      stable_id: "property.association.transition",
      pack: "association",
      family: "state",
      domains: ["concurrency", "tasks", "memory"],
      surface_tags: ["association", "task", "handle"],
      precondition: "handle is associated with one owner and has not been consumed",
      generated_partitions: ["associate", "use", "consume", "double-use"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(handle_state)", type_aware: true },
      shrink_rule: "keep the first duplicate or use-after-consume transition",
      relation: "association is unique and consuming use removes the handle from the source owner",
      seed: "property-association-001",
      type_constraints: ["Handle<T>", "Task<T>"],
      partitions: [
        { id: "associate", input: { state: "free", value: "associate" } },
        { id: "use", input: { state: "associated", value: "use" } },
        { id: "consume", input: { state: "associated", value: "consume" } },
        { id: "double-use", input: { state: "consumed", value: "use" } },
      ],
      evaluate: ({ state, value }) => ({ state, operation: value, accepted: !(state === "consumed" && value === "use") }),
      wrong: ({ state, value }) => ({ state, operation: value, accepted: false }),
    }),
    law({
      stable_id: "property.freeze.transition",
      pack: "freeze",
      family: "state",
      domains: ["memory", "views", "tasks"],
      surface_tags: ["freeze", "view", "capture"],
      precondition: "value is deeply traversable and contains no mutable host handle",
      generated_partitions: ["scalar", "nested", "view", "task-capture"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(frozen.value)", type_aware: true },
      shrink_rule: "remove nested fields after the first mutable alias witness",
      relation: "freeze creates a detached deeply immutable snapshot",
      seed: "property-freeze-001",
      type_constraints: ["Freeze<T>", "View<T>"],
      partitions: [
        { id: "scalar", input: { value: { n: 1 } } },
        { id: "nested", input: { value: { n: 1, nested: { ok: true } } } },
        { id: "view", input: { value: { view: "read" } } },
        { id: "task-capture", input: { value: { task: "capture" } } },
      ],
      evaluate: ({ value }) => ({ frozen: true, value: clone(value) }),
      wrong: ({ value }) => ({ frozen: false, value: clone(value) }),
    }),
    law({
      stable_id: "property.copy.transition",
      pack: "copy",
      family: "state",
      domains: ["memory", "views"],
      surface_tags: ["copy", "clone", "make_mut", "ownership"],
      precondition: "source value is copyable or the operation explicitly requests a semantic copy",
      generated_partitions: ["scalar", "nested", "read-view", "mutable-slot"],
      observable_sink: { type: "primitive", operation: "print", expression: "print(original); print(copy)", type_aware: true },
      shrink_rule: "remove fields after the first alias or divergent-copy witness",
      relation: "semantic copy preserves value but mutations do not alias unless explicitly shared",
      seed: "property-copy-001",
      surface_set: ["core.math.copy", "core.data", "core.compute"],
      type_constraints: ["Int", "DataTree", "List<T>", "Table<T>", "Series<T>"],
      surface_predicate: copySurface,
      generate_input: (input, context) => {
        const entropy = context.entropy >>> 0;
        return { value: { n: entropy % 17, child: { n: (entropy >>> 4) % 17 } } };
      },
      partitions: [
        { id: "scalar", input: { value: 1 } },
        { id: "nested", input: { value: { n: 1, child: { n: 2 } } } },
        { id: "read-view", input: { value: { view: "read" } } },
        { id: "mutable-slot", input: { value: { mutable: true, n: 3 } } },
      ],
      evaluate: ({ value }) => ({ original: clone(value), copy: clone(value), aliased: false }),
      wrong: ({ value }) => ({ original: clone(value), copy: clone(value), aliased: true }),
    }),
  ];
}

export const PROPERTY_LAWS = freezeDeep([...pureLaws(), ...stateLaws()]);
export const PROPERTY_PACKS = freezeDeep([
  ...["numeric", "float", "collections", "unicode", "parsing", "encoding", "serde", "time", "crypto", "seeded-random"],
  ...[...STATE_PACKS],
].map((id) => ({
  id,
  kind: STATE_PACKS.has(id) ? "state" : "pure",
  law_ids: PROPERTY_LAWS.filter((lawItem) => lawItem.pack === id).map((lawItem) => lawItem.stable_id),
})));

const LAW_BY_ID = new Map(PROPERTY_LAWS.map((lawItem) => [lawItem.stable_id, lawItem]));

export function validatePropertyLawCatalog(laws = PROPERTY_LAWS) {
  if (!Array.isArray(laws) || laws.length === 0) throw new Error("property law catalog must not be empty");
  const seen = new Set();
  for (const lawItem of laws) {
    if (!lawItem || typeof lawItem !== "object") throw new Error("property law record is not an object");
    if (typeof lawItem.stable_id !== "string" || lawItem.stable_id.length === 0) throw new Error("property law has no stable ID");
    if (seen.has(lawItem.stable_id)) throw new Error(`duplicate property law: ${lawItem.stable_id}`);
    seen.add(lawItem.stable_id);
    if (!/^property\.[a-z0-9-]+\.[a-z0-9-]+$/.test(lawItem.stable_id)) {
      throw new Error(`property law ID is not stable: ${lawItem.stable_id}`);
    }
    for (const field of ["pack", "family", "precondition", "shrink_rule", "relation", "wrong_relation", "seed"]) {
      if (typeof lawItem[field] !== "string" || lawItem[field].length === 0) {
        throw new Error(`property law ${lawItem.stable_id} has no ${field}`);
      }
    }
    if (!Array.isArray(lawItem.domains) || lawItem.domains.length === 0
      || !Array.isArray(lawItem.surface_tags) || lawItem.surface_tags.length === 0
      || !Array.isArray(lawItem.surface_set) || lawItem.surface_set.length === 0) {
      throw new Error(`property law ${lawItem.stable_id} has no surface mapping`);
    }
    if (!Array.isArray(lawItem.partitions) || lawItem.partitions.length === 0
      || lawItem.partitions.some((partition) => !partition || typeof partition.id !== "string" || !Object.hasOwn(partition, "input"))) {
      throw new Error(`property law ${lawItem.stable_id} has invalid generated partitions`);
    }
    const partitionIds = lawItem.partitions.map((partition) => partition.id);
    if (new Set(partitionIds).size !== partitionIds.length
      || !Array.isArray(lawItem.generated_partitions)
      || partitionIds.some((partitionId) => !lawItem.generated_partitions.includes(partitionId))) {
      throw new Error(`property law ${lawItem.stable_id} has incomplete partition metadata`);
    }
    if (!Array.isArray(lawItem.applicable_tiers)
      || stableText(lawItem.applicable_tiers) !== stableText(TIERS)) {
      throw new Error(`property law ${lawItem.stable_id} does not cover every execution tier`);
    }
    if (typeof lawItem.evaluate !== "function" || typeof lawItem.wrong !== "function"
      || typeof lawItem.generate_input !== "function"
      || (lawItem.precondition_fn !== null && typeof lawItem.precondition_fn !== "function")) {
      throw new Error(`property law ${lawItem.stable_id} is missing executable law hooks`);
    }
    if (!lawItem.observable_sink || typeof lawItem.observable_sink !== "object"
      || typeof lawItem.observable_sink.operation !== "string"
      || typeof lawItem.observable_sink.expression !== "string") {
      throw new Error(`property law ${lawItem.stable_id} has no observable sink`);
    }
    if (!Array.isArray(lawItem.type_constraints) || lawItem.type_constraints.length === 0) {
      throw new Error(`property law ${lawItem.stable_id} has no type constraints`);
    }
    if (lawItem.family === "state" && (!lawItem.fixture
      || lawItem.fixture.deterministic !== true
      || typeof lawItem.fixture.kind !== "string"
      || typeof lawItem.fixture.isolation !== "string"
      || typeof lawItem.fixture.setup !== "string"
      || typeof lawItem.fixture.cleanup !== "string")) {
      throw new Error(`state property law ${lawItem.stable_id} has no deterministic fixture`);
    }
    if (!lawItem.record || lawItem.record.stable_id !== lawItem.stable_id
      || lawItem.record.input_generator !== "deterministic-xorshift32") {
      throw new Error(`property law ${lawItem.stable_id} has no reproducible record`);
    }
  }
  return true;
}

validatePropertyLawCatalog();

export function propertyLawCatalog() {
  return PROPERTY_LAWS.map((lawItem) => clone(lawItem.record));
}

export function propertyPackCatalog() {
  return PROPERTY_PACKS.map((pack) => ({ ...pack, law_ids: [...pack.law_ids] }));
}

export function propertyLaw(stableId) {
  const lawItem = LAW_BY_ID.get(stableId);
  if (!lawItem) throw new Error(`unknown property law: ${stableId}`);
  return lawItem;
}

function rowId(row, index) {
  const value = row?.stable_id ?? row?.stable_surface_id;
  return typeof value === "string" && value.length > 0 ? value : `surface:${index}`;
}

function rowDomain(row) {
  const domain = row?.domain ?? row?.surface_domain ?? row?.tag;
  return typeof domain === "string" ? domain.toLowerCase() : null;
}

function rowTags(row) {
  const values = [
    row?.domain,
    row?.surface_domain,
    row?.tag,
    ...(Array.isArray(row?.tags) ? row.tags : []),
    ...(Array.isArray(row?.surface_tags) ? row.surface_tags : []),
    ...(Array.isArray(row?.property_tags) ? row.property_tags : []),
  ];
  return new Set(values
    .filter((value) => typeof value === "string" && value.length > 0)
    .map((value) => value.toLowerCase()));
}

function rowTiers(row, status) {
  const hasExplicitTiers = Object.hasOwn(row || {}, "applicable_tiers");
  if (hasExplicitTiers && !Array.isArray(row.applicable_tiers)) {
    throw new Error(`surface has invalid applicable tiers: ${rowId(row, 0)}`);
  }
  const values = hasExplicitTiers ? row.applicable_tiers : null;
  // Conformance seeds can be value-consuming before a dispatcher projection is
  // recorded. They still exercise every engine; an explicit non-covered row
  // with no projections remains non-property coverage.
  const selected = values && values.length === 0 && status === "covered" ? TIERS : values || TIERS;
  const unique = [...new Set(selected)];
  if (unique.some((tier) => !TIERS.includes(tier))) throw new Error(`surface ${rowId(row, 0)} has invalid applicable tiers`);
  return unique;
}

function externalOracle(row) {
  const values = [
    row?.oracle?.name,
    row?.oracle,
    ...(Array.isArray(row?.covered_by) ? row.covered_by : []),
  ];
  return values.find((value) => typeof value === "string" && value.length > 0) || null;
}

function layerOneOracle(row) {
  const hasTierProjection = (Array.isArray(row?.projections) && row.projections.length > 0)
    || (Array.isArray(row?.dispatcher_arms) && row.dispatcher_arms.length > 0);
  return (row?.status === "covered" && row?.value_consuming !== false) || hasTierProjection
    ? "layer1:conformance-or-tier-self-diff"
    : null;
}

function lawIdsForSurface(row) {
  const explicit = row?.property_law_ids;
  if (explicit !== undefined && (!Array.isArray(explicit) || explicit.some((id) => typeof id !== "string" || !id))) {
    throw new Error(`property law IDs are invalid: ${rowId(row, 0)}`);
  }
  for (const id of explicit || []) if (!LAW_BY_ID.has(id)) throw new Error(`unknown property law: ${id}`);
  const tags = rowTags(row);
  const derived = PROPERTY_LAWS
    .filter((lawItem) => lawItem.surface_predicate(row, { tags }) === true)
    .map((lawItem) => lawItem.stable_id);
  return sortedUnique([...(explicit || []), ...derived]);
}

export function mapPropertySurfaces(surfaces = []) {
  if (!Array.isArray(surfaces) && Array.isArray(surfaces?.rows)) surfaces = surfaces.rows;
  if (!Array.isArray(surfaces)) throw new Error("property surfaces must be an array");
  const seen = new Set();
  const rows = [];
  for (let index = 0; index < surfaces.length; index += 1) {
    const source = surfaces[index];
    if (!source || typeof source !== "object" || Array.isArray(source)) {
      throw new Error(`property surface row is not an object: ${index}`);
    }
    const stable_id = rowId(source, index);
    if (stable_id.startsWith("surface:")) throw new Error(`property surface has no stable ID: ${index}`);
    if (seen.has(stable_id)) throw new Error(`duplicate property surface: ${stable_id}`);
    seen.add(stable_id);
    const status = source.status || (source.exclusion ? "excluded" : "covered");
    const tags = rowTags(source);
    const domain = rowDomain(source) || [...tags][0] || null;
    const applicable_tiers = rowTiers(source, status);
    const law_ids = lawIdsForSurface(source);
    const candidate = status === "covered"
      && source.exclusion == null
      && source.value_consuming !== false
      && source.property_eligible !== false;
    const eligible = candidate && applicable_tiers.length > 0 && law_ids.length > 0;
    if (eligible && applicable_tiers.length > 0 && law_ids.length > 0) {
      rows.push({
        stable_id,
        domain,
        surface_tags: sortedUnique([...tags]),
        status,
        applicable_tiers,
        law_ids,
        eligible: true,
        property: true,
        reason: null,
        reason_code: null,
        covered_by: law_ids,
      });
      continue;
    }
    const covered_by = sortedUnique([
      ...(Array.isArray(source.covered_by) ? source.covered_by : []),
      externalOracle(source),
      layerOneOracle(source),
    ]);
    const oracleName = covered_by[0] || "another registered oracle";
    const reason = source.exclusion?.reason
      || source.reason
      || (status !== "covered" ? `surface status is ${status}; retain ${oracleName} coverage` : null)
      || (source.value_consuming === false ? `surface is not value-consuming; retain ${oracleName} coverage` : null)
      || (applicable_tiers.length === 0 ? `surface has no applicable execution tiers; retain ${oracleName} coverage` : null)
      || (law_ids.length === 0 ? `surface tags have no property-law pack; retain ${oracleName} coverage` : "surface is outside property-law eligibility");
    rows.push({
      stable_id,
      domain,
      surface_tags: sortedUnique([...tags]),
      status,
      applicable_tiers,
      law_ids,
      eligible: false,
      property: false,
      reason,
      reason_code: source.exclusion ? "owner-excluded" : status !== "covered" ? `status:${status}` : law_ids.length === 0 ? "no-property-law" : "not-eligible",
      covered_by,
    });
  }
  rows.sort((left, right) => left.stable_id < right.stable_id ? -1 : left.stable_id > right.stable_id ? 1 : 0);
  const mapped = rows.filter((row) => row.property).length;
  const eligible = rows.filter((row) => row.eligible).length;
  return {
    schema: PROPERTY_SCHEMA,
    schema_version: PROPERTY_SCHEMA_VERSION,
    rows,
    denominator: {
      total: rows.length,
      eligible,
      mapped,
      reasons: rows.filter((row) => !row.property).length,
      non_property: rows.filter((row) => !row.property).length,
      surface_ids: rows.map((row) => row.stable_id),
    },
    law_ids: sortedUnique(rows.flatMap((row) => row.law_ids)),
  };
}

export const propertySurfaceCoverage = mapPropertySurfaces;

export function validatePropertyCoverage(coverage) {
  if (!coverage || typeof coverage !== "object" || !Array.isArray(coverage.rows)) throw new Error("property coverage rows are required");
  if (coverage.schema !== PROPERTY_SCHEMA || coverage.schema_version !== PROPERTY_SCHEMA_VERSION) {
    throw new Error("property coverage schema is invalid");
  }
  const seen = new Set();
  const mappedLawIds = new Set();
  for (const row of coverage.rows) {
    if (!row || typeof row.stable_id !== "string" || row.stable_id.length === 0) throw new Error("property coverage row has no stable_id");
    if (seen.has(row.stable_id)) throw new Error(`duplicate property coverage row: ${row.stable_id}`);
    seen.add(row.stable_id);
    if (!Array.isArray(row.surface_tags) || row.surface_tags.some((tag) => typeof tag !== "string" || tag.length === 0)) {
      throw new Error(`property coverage surface tags are invalid: ${row.stable_id}`);
    }
    if (typeof row.status !== "string" || row.status.length === 0) throw new Error(`property coverage status is missing: ${row.stable_id}`);
    if (!Array.isArray(row.applicable_tiers) || new Set(row.applicable_tiers).size !== row.applicable_tiers.length
      || row.applicable_tiers.some((tier) => !TIERS.includes(tier))) {
      throw new Error(`property coverage tiers are invalid: ${row.stable_id}`);
    }
    if (!Array.isArray(row.law_ids)) throw new Error(`property coverage law_ids are missing: ${row.stable_id}`);
    if (typeof row.property !== "boolean" || typeof row.eligible !== "boolean") throw new Error(`property eligibility is not explicit: ${row.stable_id}`);
    if (row.property !== row.eligible) throw new Error(`property eligibility disagrees with property mapping: ${row.stable_id}`);
    if (row.law_ids.some((id) => typeof id !== "string" || !LAW_BY_ID.has(id))) throw new Error(`property coverage names an unknown law: ${row.stable_id}`);
    if (!Array.isArray(row.covered_by) || row.covered_by.some((oracle) => typeof oracle !== "string" || oracle.length === 0)) {
      throw new Error(`property coverage oracle list is invalid: ${row.stable_id}`);
    }
    if (row.property && row.law_ids.length === 0) throw new Error(`eligible surface has no property law: ${row.stable_id}`);
    if (row.property && stableText(row.covered_by) !== stableText(row.law_ids)) throw new Error(`property surface oracle list is not its law list: ${row.stable_id}`);
    if (row.property && row.status !== "covered") throw new Error(`non-covered surface is marked property-eligible: ${row.stable_id}`);
    if (!row.property && (typeof row.reason !== "string" || row.reason.length === 0)) throw new Error(`non-property surface has no counted reason: ${row.stable_id}`);
    if (!row.property && (typeof row.reason_code !== "string" || row.reason_code.length === 0)) throw new Error(`non-property surface has no reason code: ${row.stable_id}`);
    if (!row.property && row.covered_by.length === 0) throw new Error(`non-property surface has no other oracle: ${row.stable_id}`);
    for (const lawId of row.law_ids) mappedLawIds.add(lawId);
  }
  const expected = coverage.rows.length;
  if (coverage.denominator?.total !== expected) throw new Error("property denominator total does not match rows");
  const mapped = coverage.rows.filter((row) => row.property).length;
  const nonProperty = coverage.rows.filter((row) => !row.property).length;
  if (coverage.denominator?.eligible !== mapped) throw new Error("property denominator eligible count is stale");
  if (coverage.denominator?.mapped !== mapped) throw new Error("property denominator mapped count is stale");
  if (coverage.denominator?.reasons !== nonProperty || coverage.denominator?.non_property !== nonProperty) {
    throw new Error("property denominator non-property count is stale");
  }
  if (!Array.isArray(coverage.denominator?.surface_ids)
    || stableText(coverage.denominator.surface_ids) !== stableText(coverage.rows.map((row) => row.stable_id))) {
    throw new Error("property denominator surface IDs are stale");
  }
  if (!Array.isArray(coverage.law_ids) || stableText(coverage.law_ids) !== stableText(sortedUnique([...mappedLawIds]))) {
    throw new Error("property coverage law IDs are stale");
  }
  return true;
}

function isValidUnicode(value) {
  if (typeof value !== "string") return false;
  for (const character of value) {
    const codePoint = character.codePointAt(0);
    if (codePoint >= 0xd800 && codePoint <= 0xdfff) return false;
  }
  return true;
}

function isSerializable(value, seen = new Set()) {
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (typeof value !== "object" || seen.has(value)) return false;
  seen.add(value);
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) return true;
  if (Array.isArray(value)) return value.every((item) => isSerializable(item, seen));
  return Object.entries(value).every(([key, child]) => typeof key === "string" && isSerializable(child, seen));
}

function preconditionValue(lawItem, input) {
  if (!input || typeof input !== "object") return false;
  if (typeof lawItem.precondition_fn === "function" && lawItem.precondition_fn(input) !== true) return false;
  switch (lawItem.stable_id) {
    case "property.numeric.add-identity":
      return Number.isFinite(input.value) && input.identity === 0;
    case "property.numeric.order":
      return Number.isFinite(input.left) && Number.isFinite(input.right);
    case "property.float.classification":
      return typeof input.value === "number";
    case "property.float.bounded-error":
      return Number.isFinite(input.left) && Number.isFinite(input.right);
    case "property.collections.order-membership":
      return Array.isArray(input.values) && input.values.every((item) => Number.isSafeInteger(item))
        && Number.isSafeInteger(input.needle);
    case "property.unicode.codepoint":
      return isValidUnicode(input.value);
    case "property.parsing.roundtrip":
      return typeof input.value === "string" && input.value.length <= 256;
    case "property.encoding.bytes-roundtrip":
      return typeof input.value === "string" || Buffer.isBuffer(input.value) || input.value instanceof Uint8Array;
    case "property.serde.roundtrip":
      return isSerializable(input.value);
    case "property.time.calendar-composition":
      return Number.isSafeInteger(input.day) && Number.isSafeInteger(input.delta);
    case "property.crypto.known-transformation":
      return typeof input.value === "string" || Buffer.isBuffer(input.value) || input.value instanceof Uint8Array;
    case "property.rng.seeded-range-determinism":
      return Number.isInteger(input.seed) && Number.isInteger(input.bound) && input.bound > 0;
    case "property.iterator-view.transition":
    case "property.host-isolation.transition":
    case "property.protocol-db.transition":
    case "property.task-cancellation.transition":
    case "property.association.transition":
      return typeof input.state === "string" && Object.hasOwn(input, "value");
    case "property.freeze.transition":
    case "property.copy.transition":
      return isSerializable(input.value);
    default:
      return input !== null && input !== undefined;
  }
}

function caseSource(lawItem, partition, index, input = undefined) {
  const body = sourceBody(lawItem, partition, index, input);
  return ["fn run() {", ...body, "}"].join("\n") + "\n";
}

function lawOracle(lawItem, input, seed) {
  return {
    name: lawItem.stable_id,
    version: String(PROPERTY_SCHEMA_VERSION),
    input_digest: digest({ law_id: lawItem.stable_id, input, seed }),
    independence_class: lawItem.family === "state" ? "state-model-law" : "algebraic-law",
    provenance: "hardening-property-layer-2",
  };
}

function validateMaxCases(value) {
  const max = value ?? PROPERTY_DEFAULT_MAX_CASES;
  if (!Number.isInteger(max) || max < 1 || max > PROPERTY_MAX_CASES) throw new Error(`property maxCases must be an integer from 1 through ${PROPERTY_MAX_CASES}`);
  return max;
}

function interleavePropertyCandidates(candidates) {
  const groups = new Map();
  for (const candidate of candidates) {
    const pack = candidate.law.pack;
    if (!groups.has(pack)) groups.set(pack, []);
    groups.get(pack).push(candidate);
  }
  const packOrder = [
    ...PROPERTY_PACKS.map((pack) => pack.id),
    ...[...groups.keys()].filter((pack) => !PROPERTY_PACKS.some((known) => known.id === pack)),
  ];
  const ordered = [];
  for (let offset = 0; ; offset += 1) {
    let added = false;
    for (const pack of packOrder) {
      const candidate = groups.get(pack)?.[offset];
      if (candidate) {
        ordered.push(candidate);
        added = true;
      }
    }
    if (!added) return ordered;
  }
}

export function generatePropertyCases({
  surfaces = [],
  seed = PROPERTY_DEFAULT_SEED,
  maxCases = PROPERTY_DEFAULT_MAX_CASES,
  laws = PROPERTY_LAWS,
} = {}) {
  const max = validateMaxCases(maxCases);
  if (typeof seed !== "string" && typeof seed !== "number") throw new Error("property seed must be a string or number");
  const coverage = Array.isArray(surfaces) ? mapPropertySurfaces(surfaces) : surfaces;
  validatePropertyCoverage(coverage);
  if (!Array.isArray(laws)) throw new Error("property laws must be an array");
  const selectedLaws = laws.map((item) => typeof item === "string" ? propertyLaw(item) : item);
  if (selectedLaws.some((item) => !item || typeof item.stable_id !== "string" || typeof item.evaluate !== "function" || typeof item.wrong !== "function")) {
    throw new Error("property law records must include stable_id, evaluate, and wrong");
  }
  const selectedLawIds = new Set();
  for (const lawItem of selectedLaws) {
    if (selectedLawIds.has(lawItem.stable_id)) throw new Error(`duplicate selected property law: ${lawItem.stable_id}`);
    selectedLawIds.add(lawItem.stable_id);
    if (!Array.isArray(lawItem.partitions) || lawItem.partitions.some((partition) => !partition || typeof partition.id !== "string")) {
      throw new Error(`property law partitions are invalid: ${lawItem.stable_id}`);
    }
  }
  const candidates = [];
  const rejected = [];
  for (const surface of coverage.rows) {
    if (!surface.property) continue;
    for (const lawId of surface.law_ids) {
      const lawItem = LAW_BY_ID.get(lawId) || selectedLaws.find((item) => item.stable_id === lawId);
      if (!lawItem) {
        rejected.push({ kind: "law", stable_id: `${surface.stable_id}:${lawId}`, reason: "law ID is not registered" });
        continue;
      }
      if (!selectedLawIds.has(lawItem.stable_id)) continue;
      for (const partition of lawItem.partitions) {
        candidates.push({ surface, law: lawItem, partition });
      }
    }
  }
  candidates.sort((left, right) => (
    (left.law.stable_id < right.law.stable_id ? -1 : left.law.stable_id > right.law.stable_id ? 1 : 0)
    || (left.surface.stable_id < right.surface.stable_id ? -1 : left.surface.stable_id > right.surface.stable_id ? 1 : 0)
    || (left.partition.id < right.partition.id ? -1 : left.partition.id > right.partition.id ? 1 : 0)
  ));
  candidates.splice(0, candidates.length, ...interleavePropertyCandidates(candidates));
  const entropy = boundedSeed(String(seed), candidates.length);
  const cases = [];
  let attempted = 0;
  const omitted = [];
  for (let index = 0; index < candidates.length; index += 1) {
    const candidate = candidates[index];
    attempted += 1;
    const caseSeed = `${seed}:${candidate.law.seed}:${candidate.surface.stable_id}:${candidate.partition.id}`;
    const case_id = `property:${candidate.law.stable_id}:${candidate.surface.stable_id}:${candidate.partition.id}`;
    const context = {
      seed: caseSeed,
      entropy: entropy[index] ?? 0,
      index,
      partition: candidate.partition,
      law: candidate.law,
      surface: candidate.surface,
    };
    let input;
    try {
      input = clone(candidate.law.generate_input(clone(candidate.partition.input), context));
    } catch (error) {
      rejected.push({
        kind: "generator",
        case_id,
        law_id: candidate.law.stable_id,
        stable_surface_id: candidate.surface.stable_id,
        partition: candidate.partition.id,
        reason: error.message,
      });
      continue;
    }
    let valid;
    try {
      valid = preconditionValue(candidate.law, input);
    } catch (error) {
      rejected.push({ kind: "precondition", case_id, law_id: candidate.law.stable_id, stable_surface_id: candidate.surface.stable_id, partition: candidate.partition.id, reason: error.message });
      continue;
    }
    if (!valid) {
      rejected.push({
        kind: "precondition",
        case_id,
        law_id: candidate.law.stable_id,
        stable_surface_id: candidate.surface.stable_id,
        partition: candidate.partition.id,
        reason: candidate.law.precondition,
      });
      continue;
    }
    if (cases.length >= max) {
      omitted.push({ kind: "budget", case_id, law_id: candidate.law.stable_id, stable_surface_id: candidate.surface.stable_id, partition: candidate.partition.id, reason: `maxCases=${max}` });
      continue;
    }
    let expected;
    try {
      expected = candidate.law.evaluate(input);
    } catch (error) {
      rejected.push({ kind: "evaluation", case_id, law_id: candidate.law.stable_id, stable_surface_id: candidate.surface.stable_id, partition: candidate.partition.id, reason: error.message });
      continue;
    }
    const source = caseSource(candidate.law, candidate.partition, index, input);
    cases.push({
      layer: "property",
      case_id,
      stable_surface_id: candidate.surface.stable_id,
      law_id: candidate.law.stable_id,
      pack: candidate.law.pack,
      domain: candidate.surface.domain || candidate.law.domains[0],
      applicable_tiers: [...candidate.surface.applicable_tiers],
      seed: caseSeed,
      mutation_arm: `property-${candidate.partition.id}`,
      mutator_version: PROPERTY_MUTATOR_VERSION,
      source,
      source_sha256: sha256(source),
      input,
      expected_value: clone(expected),
      expected_relation: stableText(expected),
      normalization: [],
      oracle: lawOracle(candidate.law, input, caseSeed),
      precondition: candidate.law.precondition,
      generated_partition: candidate.partition.id,
      generated_partitions: [...candidate.law.generated_partitions],
      observable_sink: clone(candidate.law.observable_sink),
      shrink_rule: candidate.law.shrink_rule,
      relation: candidate.law.relation,
      wrong_relation: candidate.law.wrong_relation,
      surface_set: [...candidate.law.surface_set],
      type_constraints: [...candidate.law.type_constraints],
      fixture: clone(candidate.law.fixture),
      input_generator: candidate.law.record.input_generator,
      value_consuming: true,
      entropy: entropy[index] ?? 0,
      law_record: clone(candidate.law.record),
    });
  }
  const batches = batchPropertyCases(cases);
  return {
    schema: PROPERTY_SCHEMA,
    schema_version: PROPERTY_SCHEMA_VERSION,
    seed: String(seed),
    max_cases: max,
    coverage,
    laws: selectedLaws.map((item) => clone(item.record)),
    cases,
    rejected,
    omitted,
    attempted,
    valid_case_count: cases.length,
    batch_size: batches.batch_size,
    batch_count: batches.batch_count,
    batches: batches.batches,
    denominator: {
      surfaces: coverage.denominator.total,
      eligible_surfaces: coverage.denominator.eligible,
      mapped_surfaces: coverage.denominator.mapped,
      laws: selectedLaws.length,
      generated: candidates.length,
      valid: cases.length,
      rejected: rejected.length,
      omitted: omitted.length,
    },
  };
}

function validateBatchSize(value) {
  const batchSize = value ?? PROPERTY_DEFAULT_BATCH_SIZE;
  if (!Number.isInteger(batchSize) || batchSize < 1 || batchSize > PROPERTY_MAX_BATCH_SIZE) {
    throw new Error(`property batchSize must be an integer from 1 through ${PROPERTY_MAX_BATCH_SIZE}`);
  }
  return batchSize;
}

function lineProtocolCase(caseInput) {
  return canonicalJson({
    case_id: caseInput.case_id,
    stable_surface_id: caseInput.stable_surface_id,
    seed: caseInput.seed,
    mutation_arm: caseInput.mutation_arm,
    source: caseInput.source,
    oracle: caseInput.oracle,
    expected_relation: caseInput.expected_relation,
    normalization: caseInput.normalization,
  });
}

export function batchPropertyCases(cases = [], { batchSize = PROPERTY_DEFAULT_BATCH_SIZE } = {}) {
  if (!Array.isArray(cases)) throw new Error("property cases must be an array");
  const batch_size = validateBatchSize(batchSize);
  const valid = [];
  const rejected = [];
  const seen = new Set();
  for (const caseInput of cases) {
    const required = ["case_id", "stable_surface_id", "seed", "mutation_arm", "source", "expected_relation"];
    const missing = required.find((key) => typeof caseInput?.[key] !== "string" || caseInput[key].length === 0);
    const duplicate = typeof caseInput?.case_id === "string" && seen.has(caseInput.case_id);
    if (typeof caseInput?.case_id === "string") seen.add(caseInput.case_id);
    const valueConsuming = typeof caseInput?.source === "string"
      && caseInput.value_consuming === true
      && caseInput.source.includes("fn run")
      && /\b(?:print|eprint|assert|panic|exit)\s*\(/.test(caseInput.source);
    const validTiers = Array.isArray(caseInput?.applicable_tiers)
      && caseInput.applicable_tiers.length > 0
      && new Set(caseInput.applicable_tiers).size === caseInput.applicable_tiers.length
      && caseInput.applicable_tiers.every((tier) => TIERS.includes(tier));
    if (missing || duplicate || !valueConsuming || !validTiers || !caseInput?.oracle || typeof caseInput.oracle !== "object"
      || Array.isArray(caseInput.oracle) || !Array.isArray(caseInput.normalization)
      || (caseInput.source_sha256 !== undefined && caseInput.source_sha256 !== sha256(caseInput.source))) {
      rejected.push({
        kind: "batch",
        case_id: caseInput?.case_id || null,
        reason: missing ? `missing ${missing}` : duplicate ? "duplicate case_id" : "batch case metadata is invalid",
      });
      continue;
    }
    valid.push(caseInput);
  }
  const batches = [];
  for (let index = 0; index < valid.length; index += batch_size) {
    const batchCases = valid.slice(index, index + batch_size);
    batches.push({
      index: batches.length,
      cases: batchCases,
      line_protocol: `${batchCases.map(lineProtocolCase).join("\n")}\n`,
    });
  }
  return {
    schema: PROPERTY_SCHEMA,
    schema_version: PROPERTY_SCHEMA_VERSION,
    batch_size,
    batch_count: batches.length,
    cases: valid,
    rejected,
    attempted: cases.length,
    valid_case_count: valid.length,
    batches,
  };
}

function observedValue(observation) {
  if (Object.hasOwn(observation || {}, "normalized_value")) return clone(observation.normalized_value);
  if (Object.hasOwn(observation || {}, "value")) return clone(observation.value);
  const stdout = Buffer.isBuffer(observation?.stdout)
    ? observation.stdout.toString("utf8")
    : Buffer.isBuffer(observation?.stdout_bytes)
      ? observation.stdout_bytes.toString("utf8")
      : typeof observation?.stdout === "string"
        ? observation.stdout
        : null;
  if (stdout !== null) {
    const text = stdout.trim();
    try { return JSON.parse(text); } catch { return text; }
  }
  if (typeof observation?.stdout_bytes === "string") {
    const text = Buffer.from(observation.stdout_bytes.replace(/^base64:/, ""), "base64").toString("utf8").trim();
    try { return JSON.parse(text); } catch { return text; }
  }
  return undefined;
}

export function comparePropertyObservations({ law, input, observations, applicable_tiers = TIERS, wrong = false } = {}) {
  const lawItem = typeof law === "string" ? propertyLaw(law) : law;
  if (!lawItem || typeof lawItem.evaluate !== "function") throw new Error("property law is required");
  if (!Array.isArray(applicable_tiers) || applicable_tiers.length === 0
    || new Set(applicable_tiers).size !== applicable_tiers.length
    || applicable_tiers.some((tier) => !TIERS.includes(tier))) {
    throw new Error("property applicable tiers are invalid");
  }
  const tiers = [...applicable_tiers];
  const expected = lawItem.evaluate(input);
  const values = observations || tiers.map((tier) => ({ tier, normalized_value: wrong ? lawItem.wrong(input) : expected }));
  if (!Array.isArray(values) || values.length === 0) throw new Error("property observations are required");
  const normalizedInput = values.map((observation) => {
    const value = observedValue(observation);
    return value === undefined ? { ...observation } : { ...observation, normalized_value: value };
  });
  const shared = compareCaseObservations({
    domain: "property",
    observations: normalizedInput,
    applicable_tiers: tiers,
    normalization: [],
    expected_relation: stableText(expected),
  });
  const tierParity = shared.tier_parity;
  const checks = shared.observations.map((observation) => exact(expected, observation.normalized_value));
  const unhealthy = (observation) => Boolean(observation?.error)
    || observation?.timeout === true
    || observation?.timed_out === true
    || Boolean(observation?.signal)
    || (observation?.exit !== undefined && observation.exit !== null && observation.exit !== 0);
  const unhealthyTiers = normalizedInput.filter(unhealthy).map((observation) => observation.tier);
  const ok = !wrong && tierParity.ok && unhealthyTiers.length === 0 && checks.every((check) => check.ok);
  const first = shared.observations[0];
  const differences = [...new Set([
    ...tierParity.differences,
    ...unhealthyTiers,
    ...shared.observations.filter((_, index) => !checks[index].ok).map((observation) => observation.tier),
    ...(wrong ? tiers : []),
  ])];
  const failed = shared.observations.find((observation) => differences.includes(observation.tier)) || first;
  return {
    ok,
    law_id: lawItem.stable_id,
    expected,
    actual: first?.normalized_value,
    expected_relation: stableText(expected),
    actual_relation: stableText(failed?.normalized_value),
    relation: lawItem.relation,
    tier_parity: tierParity,
    oracle_ok: !wrong && unhealthyTiers.length === 0 && checks.every((check) => check.ok),
    differences,
    observations: shared.observations,
    result_bundle_input: ok ? null : {
      tier: failed?.tier || tiers[0],
      expected_relation: stableText(expected),
      actual_relation: stableText(failed?.normalized_value),
      tier_observations: shared.observations,
    },
  };
}

function bundleForPropertyCase(caseInput, comparison, metadata = {}) {
  const observations = comparison.observations.map((row) => {
    const timeout = row.timeout === true || row.timed_out === true;
    return {
      ...row,
      stdout: row.stdout ?? row.stdout_bytes ?? JSON.stringify(row.normalized_value ?? row.value ?? ""),
      stderr: row.stderr ?? row.stderr_bytes ?? "",
      exit: Object.hasOwn(row, "exit") ? row.exit : timeout ? null : 0,
      signal: row.signal ?? null,
      timeout,
      relation: row.relation || stableText(observedValue(row)),
    };
  });
  const selected = observations.find((row) => row.tier === comparison.result_bundle_input?.tier) || observations[0];
  return makeResultBundle({
    run_id: metadata.run_id || "property-run",
    stable_surface_id: caseInput.stable_surface_id,
    tier: selected.tier,
    tier_command: selected.tier_command || `property:${selected.tier}`,
    seed: caseInput.seed,
    mutation_arm: caseInput.mutation_arm,
    mutator_version: caseInput.mutator_version,
    source: caseInput.source,
    stdout: selected.stdout ?? selected.stdout_bytes ?? JSON.stringify(comparison.actual),
    stderr: selected.stderr ?? selected.stderr_bytes ?? "",
    exit: Object.hasOwn(selected, "exit") ? selected.exit : selected.timeout ? null : 0,
    signal: selected.signal ?? null,
    timeout: selected.timeout === true || selected.timed_out === true,
    expected_relation: comparison.expected_relation,
    actual_relation: comparison.actual_relation,
    normalization: caseInput.normalization,
    oracle: caseInput.oracle,
    commit: metadata.commit || "unknown-commit",
    binary_sha256: metadata.binary_sha256 || "sha256:unknown-binary",
    registry_snapshot_hash: metadata.registry_snapshot_hash || "sha256:unknown-registry",
    config_hash: metadata.config_hash || "sha256:unknown-config",
    classification: metadata.classification || "silent-data",
    tower_action: "create-or-update",
    tier_observations: observations,
    applicable_tiers: caseInput.applicable_tiers,
    layer: "property",
    law_id: caseInput.law_id,
    precondition: caseInput.precondition,
    generated_partition: caseInput.generated_partition,
    generated_partitions: caseInput.generated_partitions,
    observable_sink: caseInput.observable_sink,
    shrink_rule: caseInput.shrink_rule,
    relation: caseInput.relation,
    type_constraints: caseInput.type_constraints,
    proof: {
      value_consuming: caseInput.value_consuming !== false,
      wrong_relation: caseInput.wrong_relation,
      surface_set: caseInput.surface_set,
      fixture: caseInput.fixture,
      minimized: caseInput.minimized === true,
    },
  });
}

export async function runPropertyCases(cases, {
  executor = null,
  maxCases = PROPERTY_DEFAULT_MAX_CASES,
  wrong = false,
  metadata = {},
} = {}) {
  const max = validateMaxCases(maxCases);
  if (!Array.isArray(cases)) throw new Error("property cases must be an array");
  const findings = [];
  const rejected = [];
  const omitted = [];
  const processed = [];
  const seenCaseIds = new Set();
  let attempted = 0;
  let valid_case_count = 0;
  for (const caseInput of cases) {
    attempted += 1;
    let lawItem;
    try {
      lawItem = propertyLaw(caseInput?.law_id);
    } catch (error) {
      rejected.push({ case_id: caseInput?.case_id || null, reason: error.message, kind: "law" });
      continue;
    }
    const tiers = Array.isArray(caseInput?.applicable_tiers) ? caseInput.applicable_tiers : [];
    const duplicateCaseId = typeof caseInput?.case_id === "string" && seenCaseIds.has(caseInput.case_id);
    if (typeof caseInput?.case_id === "string") seenCaseIds.add(caseInput.case_id);
    const malformed = !caseInput || typeof caseInput !== "object"
      || typeof caseInput.case_id !== "string"
      || typeof caseInput.source !== "string"
      || typeof caseInput.seed !== "string"
      || caseInput.law_id !== lawItem.stable_id
      || caseInput.value_consuming !== true
      || !caseInput.source.includes("fn run")
      || !/\b(?:print|eprint|assert|panic|exit)\s*\(/.test(caseInput.source)
      || (caseInput.source_sha256 !== undefined && caseInput.source_sha256 !== sha256(caseInput.source))
      || typeof caseInput.expected_relation !== "string"
      || caseInput.expected_relation.length === 0
      || !caseInput.oracle
      || typeof caseInput.oracle !== "object"
      || !Array.isArray(caseInput.normalization)
      || typeof caseInput.precondition !== "string"
      || !Array.isArray(caseInput.generated_partitions)
      || caseInput.generated_partitions.length === 0
      || !caseInput.observable_sink
      || typeof caseInput.observable_sink !== "object"
      || typeof caseInput.observable_sink.operation !== "string"
      || typeof caseInput.observable_sink.expression !== "string"
      || typeof caseInput.relation !== "string"
      || typeof caseInput.wrong_relation !== "string"
      || typeof caseInput.shrink_rule !== "string"
      || !Array.isArray(caseInput.surface_set)
      || caseInput.surface_set.length === 0
      || !Array.isArray(caseInput.type_constraints)
      || caseInput.type_constraints.length === 0
      || (lawItem.family === "state" && (!caseInput.fixture || caseInput.fixture.deterministic !== true))
      || duplicateCaseId
      || !Array.isArray(tiers)
      || tiers.length === 0
      || new Set(tiers).size !== tiers.length
      || tiers.some((tier) => !TIERS.includes(tier));
    if (malformed) {
      rejected.push({
        case_id: caseInput?.case_id || null,
        reason: duplicateCaseId ? "duplicate case_id" : "case metadata is invalid",
        kind: "case",
      });
      continue;
    }
    let valid;
    try {
      valid = preconditionValue(lawItem, caseInput.input);
    } catch (error) {
      rejected.push({ case_id: caseInput.case_id, reason: error.message, kind: "precondition" });
      continue;
    }
    if (!valid) {
      rejected.push({ case_id: caseInput.case_id, reason: lawItem.precondition, kind: "precondition" });
      continue;
    }
    if (valid_case_count >= max) {
      omitted.push({ case_id: caseInput.case_id, law_id: lawItem.stable_id, reason: `maxCases=${max}`, kind: "budget" });
      continue;
    }
    valid_case_count += 1;
    let observations;
    try {
      if (wrong) {
        observations = tiers.map((tier) => ({ tier, normalized_value: lawItem.wrong(caseInput.input), exit: 0, signal: null, timeout: false }));
      } else if (executor) {
        const execution = await executeCase({
          ...caseInput,
          // The property law is the oracle. The shared executor must still own
          // tier marshalling and observation normalization without accidentally
          // selecting a layer-1 domain oracle for the same input.
          domain: "property",
        }, {
          executor: (request) => executor({ ...request, law: lawItem }),
          validate: false,
          applicable_tiers: tiers,
          normalization: [],
        });
        observations = execution.observations;
      } else {
        observations = tiers.map((tier) => ({ tier, normalized_value: lawItem.evaluate(caseInput.input), exit: 0, signal: null, timeout: false }));
      }
    } catch (error) {
      if (executor) {
        observations = tiers.map((tier) => ({
          tier,
          error: error.message,
          exit: 1,
          signal: null,
          timeout: false,
        }));
      } else {
        rejected.push({ case_id: caseInput.case_id, reason: error.message, kind: wrong ? "wrong-relation" : "evaluation" });
        valid_case_count -= 1;
        continue;
      }
    }
    let comparison;
    try {
      comparison = comparePropertyObservations({
        law: lawItem,
        input: caseInput.input,
        observations,
        applicable_tiers: tiers,
        wrong,
      });
    } catch (error) {
      rejected.push({ case_id: caseInput.case_id, reason: error.message, kind: "observation" });
      valid_case_count -= 1;
      continue;
    }
    processed.push(caseInput);
    if (comparison.ok) continue;
    const minimized = minimizePropertyCase(caseInput);
    findings.push(bundleForPropertyCase(minimized, comparison, metadata));
  }
  const batches = batchPropertyCases(processed);
  const serialized_bundles = serializeBundles(findings);
  const status = findings.length
    ? "FINDINGS"
    : valid_case_count === 0 && attempted > 0
      ? "NO_VALID_CASES"
      : "PASS";
  return {
    schema: PROPERTY_SCHEMA,
    schema_version: PROPERTY_SCHEMA_VERSION,
    status,
    attempted,
    valid_case_count,
    rejected,
    omitted,
    batch_size: batches.batch_size,
    batch_count: batches.batch_count,
    batches: batches.batches,
    findings,
    serialized_bundles,
    bundle_sha256: sha256(serialized_bundles),
    finding_payloads: findings.map((bundle) => ({ bundle_identity: sha256(canonicalJson(bundle)), bundle })),
    denominator: {
      attempted,
      valid: valid_case_count,
      rejected: rejected.length,
      omitted: omitted.length,
    },
  };
}

export function checkPropertyPacks() {
  const results = [];
  for (const pack of PROPERTY_PACKS) {
    const packLaws = PROPERTY_LAWS.filter((lawItem) => lawItem.pack === pack.id);
    if (packLaws.length === 0) throw new Error(`property pack has no laws: ${pack.id}`);
    for (const lawItem of packLaws) {
      const entropy = boundedSeed(lawItem.seed, lawItem.partitions.length)[0] ?? 1;
      let selected = null;
      for (const partition of lawItem.partitions) {
        const input = clone(lawItem.generate_input(clone(partition.input), {
          seed: `pack:${lawItem.seed}`,
          entropy,
          index: 0,
          partition,
          law: lawItem,
        }));
        if (preconditionValue(lawItem, input)) {
          selected = { partition, input };
          break;
        }
      }
      if (!selected) throw new Error(`property pack has no valid witness: ${lawItem.stable_id}`);
      const expected = lawItem.evaluate(selected.input);
      const actual = lawItem.wrong(selected.input);
      if (exact(expected, actual).ok) throw new Error(`planted wrong property relation survived ${lawItem.stable_id}`);
      const caseInput = {
        layer: "property",
        case_id: `property-pack:${pack.id}:${lawItem.stable_id}:${selected.partition.id}`,
        stable_surface_id: `property-pack:${pack.id}`,
        law_id: lawItem.stable_id,
        pack: pack.id,
        domain: lawItem.domains[0],
        applicable_tiers: [...TIERS],
        seed: `pack:${lawItem.seed}`,
        mutation_arm: `property-kill-${selected.partition.id}`,
        mutator_version: PROPERTY_MUTATOR_VERSION,
        source: caseSource(lawItem, selected.partition, 0, selected.input),
        input: selected.input,
        expected_value: clone(expected),
        expected_relation: stableText(expected),
        normalization: [],
        oracle: lawOracle(lawItem, selected.input, `pack:${lawItem.seed}`),
        precondition: lawItem.precondition,
        generated_partition: selected.partition.id,
        generated_partitions: [...lawItem.generated_partitions],
        observable_sink: clone(lawItem.observable_sink),
        shrink_rule: lawItem.shrink_rule,
        relation: lawItem.relation,
        wrong_relation: lawItem.wrong_relation,
        surface_set: [...lawItem.surface_set],
        type_constraints: [...lawItem.type_constraints],
        fixture: clone(lawItem.fixture),
        value_consuming: true,
      };
      const wrongObservations = TIERS.map((tier) => ({
        tier,
        normalized_value: clone(actual),
        exit: 0,
        signal: null,
        timeout: false,
      }));
      const comparison = comparePropertyObservations({
        law: lawItem,
        input: selected.input,
        observations: wrongObservations,
        applicable_tiers: TIERS,
      });
      if (comparison.ok) throw new Error(`planted wrong property relation survived ${lawItem.stable_id}`);
      const minimized = minimizePropertyCase(caseInput);
      const bundle = bundleForPropertyCase(minimized, comparison, {
        run_id: "property-pack-kill",
        classification: "planted-law-kill",
      });
      results.push({
        pack: pack.id,
        law_id: lawItem.stable_id,
        expected,
        actual,
        source: minimized.source,
        shrink_rule: minimized.shrink_rule,
        value_consuming: minimized.value_consuming,
        minimized: minimized.minimized,
        failure_bundle: bundle,
        bundle,
        killed: true,
      });
    }
  }
  return results;
}

export function propertyPackCaseCounts(generated) {
  const cases = Array.isArray(generated?.cases) ? generated.cases : [];
  return PROPERTY_PACKS.map((pack) => ({
    pack: pack.id,
    valid_case_count: cases.filter((caseInput) => caseInput.pack === pack.id).length,
  }));
}

export function minimizePropertyCase(caseInput, candidates = []) {
  const lawItem = propertyLaw(caseInput.law_id);
  const values = [caseInput.source, ...candidates]
    .filter((value) => typeof value === "string")
    .map((source) => source.replace(/^[ \t]*unused[^\n]*\n/gm, ""));
  const valueConsuming = (source) => source.includes("fn run")
    && /\b(?:print|eprint|assert|panic|exit)\s*\(/.test(source);
  const valid = values.filter(valueConsuming);
  const lexical = (left, right) => left < right ? -1 : left > right ? 1 : 0;
  const source = valid.sort((left, right) => Buffer.byteLength(left) - Buffer.byteLength(right) || lexical(left, right))[0] || caseInput.source;
  if (typeof source !== "string" || !valueConsuming(source)) throw new Error(`property case is not value-consuming: ${caseInput.case_id || caseInput.law_id}`);
  const shrinkCandidates = [...new Set(values)].sort((left, right) => Buffer.byteLength(left) - Buffer.byteLength(right) || lexical(left, right));
  return {
    ...clone(caseInput),
    source,
    source_sha256: sha256(source),
    minimized: true,
    value_consuming: true,
    law_id: lawItem.stable_id,
    surface_set: [...lawItem.surface_set],
    observable_sink: clone(lawItem.observable_sink),
    shrink_rule: lawItem.shrink_rule,
    minimization: {
      strategy: "stable-shortest-value-consuming-source",
      candidates: shrinkCandidates.map((candidate) => ({ sha256: sha256(candidate), bytes: Buffer.byteLength(candidate) })),
      selected_sha256: sha256(source),
      shrink_rule: lawItem.shrink_rule,
    },
  };
}

export function propertyLayerSummary(generated, result = null) {
  const generatedRejected = Array.isArray(generated.rejected) ? generated.rejected : [];
  const runtimeRejected = Array.isArray(result?.rejected) ? result.rejected : [];
  const rejected = [...generatedRejected, ...runtimeRejected];
  const omitted = [
    ...(Array.isArray(generated.omitted) ? generated.omitted : []),
    ...(Array.isArray(result?.omitted) ? result.omitted : []),
  ];
  const attempted = result?.attempted ?? generated.attempted;
  const validCaseCount = result?.valid_case_count ?? generated.valid_case_count;
  const status = result?.status === "PASS" && validCaseCount === 0 && (attempted > 0 || generated.attempted > 0)
    ? "NO_VALID_CASES"
    : result?.status ?? "NOT_RUN";
  return {
    schema: PROPERTY_SCHEMA,
    schema_version: PROPERTY_SCHEMA_VERSION,
    seed: generated.seed,
    max_cases: generated.max_cases,
    coverage: clone(generated.coverage),
    laws: clone(generated.laws),
    packs: propertyPackCatalog(),
    denominator: clone(generated.denominator),
    attempted,
    generated_attempted: generated.attempted,
    valid_case_count: validCaseCount,
    rejected: clone(rejected),
    omitted: clone(omitted),
    batch_size: result?.batch_size ?? generated.batch_size,
    batch_count: result?.batch_count ?? generated.batch_count,
    batches: clone(result?.batches ?? generated.batches ?? []),
    status,
    findings: clone(result?.findings ?? []),
    finding_payloads: clone(result?.finding_payloads ?? []),
    serialized_bundles: result?.serialized_bundles ?? "",
    bundle_sha256: result?.bundle_sha256 ?? sha256(""),
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  if (process.argv.includes("--self-test")) {
    checkPropertyPacks();
    const generated = generatePropertyCases({
      surfaces: coreRegistrySurfaces(),
      seed: PROPERTY_DEFAULT_SEED,
      maxCases: PROPERTY_DEFAULT_MAX_CASES,
    });
    const counts = propertyPackCaseCounts(generated);
    console.log("pack\tvalid_case_count");
    for (const row of counts) console.log(`${row.pack}\t${row.valid_case_count}`);
    if (counts.some((row) => row.valid_case_count === 0)) throw new Error("property pack generated zero real-registry cases");
    console.log("hardening property layer: PASS");
  }
}
