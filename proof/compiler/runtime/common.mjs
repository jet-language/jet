#!/usr/bin/env node

/**
 * Shared implementation-bound checker for the Jet-owned Prelude/Core and
 * runtime stages.  This module indexes the dispatcher and the real Prelude
 * and Foundation bodies; it does not execute a compiler or claim semantic
 * preservation from a route name.
 */

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonical,
  canonicalJson,
  digestBytes,
  digestText,
  identityDigest,
  replay,
  validateClaim,
  validateManifest,
} from "../../../scripts/agent/compiler-proof.mjs";
import { buildLedger } from "../../../scripts/agent/check-core-surface-ledger.mjs";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
export const CORE_PATH = "crates/jet-codegen/src/Prelude/Core.jet";
export const CORE_RS_PATH = "crates/jet-codegen/src/Prelude/Core.rs";
export const LEDGER_PATH = "scripts/agent/check-core-surface-ledger.mjs";
export const OBLIGATIONS_PATH = "proof/compiler/obligations.json";
export const MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
export const RUNNER_PATH = "scripts/agent/jet-env";
export const IMPLEMENTATION_ROOTS = Object.freeze([
  "crates/jet-codegen/src/Prelude",
  "crates/jet-foundation/src",
  "crates/jet-rt/src",
]);
export const REQUIRED_POLICIES = Object.freeze([
  "validation",
  "defaults",
  "bounds",
  "encoding",
  "numerics",
  "lifecycle",
  "effects",
  "cancellation",
  "cleanup",
]);
export const OBSERVATION_FIELDS = Object.freeze([
  "result",
  "typed_failure",
  "mutation",
  "input_consumption",
  "ordered_effects",
  "cleanup",
  "resource_premises",
  "schedule",
  "allowed_schedules",
]);
export const STATUS_SET = new Set([
  "blocked",
  "matched",
  "mismatch",
  "unavailable",
  "invalid_oracle",
  "timeout",
  "cancelled",
  "failed",
]);
export const DISPOSITION_SET = new Set(["mapped_unproved", "unsupported", "unmapped", "proved"]);

export const STAGE_PROOF_NEGATIVE_CONTROLS = Object.freeze([
  Object.freeze({
    id: "runtime-forged-outer-stage-binding",
    category: "proof-binding",
    source_refs: Object.freeze([MANIFEST_PATH, OBLIGATIONS_PATH, CORE_PATH]),
    mutation: "attach identity-direct or unrelated-theorem evidence to a row with claimant-supplied stage_binding hashes",
    expected: {
      status: "mismatch",
      reason: "canonical stage proposition/formal goal and checked implementation correspondence must reject unrelated evidence",
      universal_proof: false,
    },
  }),
]);

export class CheckError extends Error {
  constructor(code, message, details = undefined) {
    super(message);
    this.name = "CheckError";
    this.code = code;
    this.details = details;
  }
}

export function fail(code, message, details = undefined) {
  throw new CheckError(code, message, details);
}

export function projectPath(value, label) {
  if (typeof value !== "string" || value.length === 0) fail("invalid_oracle", `${label} must be a non-empty path`);
  const normalized = value.replaceAll("\\", "/");
  if (normalized.startsWith("/") || normalized.split("/").includes("..")) {
    fail("invalid_oracle", `${label} must stay within the project`);
  }
  const absolute = resolve(ROOT, normalized);
  if (absolute !== ROOT && !absolute.startsWith(`${ROOT}/`)) fail("invalid_oracle", `${label} escapes the project`);
  return normalized;
}

export function readJson(path, label) {
  const project = projectPath(path, label);
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail("unavailable", `${label} is missing: ${project}`);
  try {
    return JSON.parse(readFileSync(absolute, "utf8"));
  } catch (error) {
    fail("invalid_oracle", `${label} is not valid JSON: ${project}`, { error: String(error) });
  }
}

export function readText(path, label) {
  const project = projectPath(path, label);
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail("unavailable", `${label} is missing: ${project}`);
  try {
    return readFileSync(absolute, "utf8");
  } catch (error) {
    fail("unavailable", `${label} cannot be read: ${project}`, { error: String(error) });
  }
}

export function object(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail("invalid_oracle", `${label} must be an object`);
  return value;
}

export function array(value, label) {
  if (!Array.isArray(value)) fail("invalid_oracle", `${label} must be an array`);
  return value;
}

export function text(value, label) {
  if (typeof value !== "string" || value.length === 0) fail("invalid_oracle", `${label} must be non-empty text`);
  return value;
}

export function bool(value, label) {
  if (typeof value !== "boolean") fail("invalid_oracle", `${label} must be boolean`);
  return value;
}

export function integer(value, label) {
  if (!Number.isInteger(value) || value < 0) fail("invalid_oracle", `${label} must be a non-negative integer`);
  return value;
}

export function deepEqual(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

export function fileDigest(path) {
  const project = projectPath(path, "file path");
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute) || !statSync(absolute).isFile()) fail("unavailable", `file is missing: ${project}`);
  return digestBytes(readFileSync(absolute));
}
let CARGO_DIGEST;
function cargoDigest() {
  if (CARGO_DIGEST === undefined) CARGO_DIGEST = fileDigest("Cargo.toml");
  return CARGO_DIGEST;
}

function walk(root) {
  const absolute = resolve(ROOT, projectPath(root, "source root"));
  if (!existsSync(absolute)) fail("unavailable", `source root is missing: ${root}`);
  if (statSync(absolute).isFile()) return [projectPath(root, "source root")];
  const result = [];
  for (const entry of readdirSync(absolute, { withFileTypes: true })) {
    if (entry.name === ".git" || entry.name === "target") continue;
    const child = `${root}/${entry.name}`;
    if (entry.isDirectory()) result.push(...walk(child));
    else if (entry.isFile()) result.push(projectPath(child, "source path"));
  }
  return result.sort();
}

/** Digest a file or a complete source tree with path names included. */
export function treeDigest(path) {
  const files = walk(path);
  const hash = createHash("sha256");
  for (const file of files) {
    hash.update(file);
    hash.update("\0");
    hash.update(readFileSync(resolve(ROOT, file)));
    hash.update("\0");
  }
  return {
    path: projectPath(path, "dependency path"),
    files: files.length,
    sha256: `sha256-${hash.digest("hex")}`,
  };
}

export function checkDependencies(contract, witnessesPath) {
  const dependencies = array(contract.dependencies, "contract.dependencies");
  const expectedPaths = new Set();
  const records = [];
  for (const [index, dependency] of dependencies.entries()) {
    object(dependency, `contract.dependencies[${index}]`);
    const path = projectPath(dependency.path, `contract.dependencies[${index}].path`);
    if (expectedPaths.has(path)) fail("invalid_oracle", `duplicate dependency path: ${path}`);
    expectedPaths.add(path);
    const expected = text(dependency.sha256, `contract.dependencies[${index}].sha256`);
    if (!/^sha256-[0-9a-f]{64}$/u.test(expected)) fail("invalid_oracle", `invalid dependency digest: ${path}`);
    const current = treeDigest(path);
    records.push({
      path,
      kind: text(dependency.kind, `contract.dependencies[${index}].kind`),
      expected_sha256: expected,
      current_sha256: current.sha256,
      files: current.files,
      status: current.sha256 === expected ? "unchanged" : "changed",
    });
  }
  const witnessPath = projectPath(witnessesPath, "witnesses path");
  const witnessDigest = fileDigest(witnessPath);
  const recordedWitness = text(contract.witnesses_sha256, "contract.witnesses_sha256");
  if (!/^sha256-[0-9a-f]{64}$/u.test(recordedWitness)) fail("invalid_oracle", "contract.witnesses_sha256 is not a sha256 digest");
  records.push({
    path: witnessPath,
    kind: "witness-configuration",
    expected_sha256: recordedWitness,
    current_sha256: witnessDigest,
    files: 1,
    status: witnessDigest === recordedWitness ? "unchanged" : "changed",
  });
  return {
    valid: records.every((record) => record.status === "unchanged"),
    records,
    changed: records.filter((record) => record.status === "changed").map((record) => record.path),
  };
}

function lineForOffset(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function quotedStrings(value) {
  return [...value.matchAll(/"((?:\\.|[^"\\])*)"/gu)].map((match) => match[1]);
}

function dispatcherRoute(expression, family) {
  if (family !== "receiver") {
    const match = expression.match(/CoreCallRecord::new\s*\(\s*"[^"]*"\s*,\s*"[^"]*"\s*,\s*"([^"]+)"/u);
    return match?.[1] ?? null;
  }
  if (!expression.includes("receiver_with_symbol")) return null;
  const match = expression.match(/receiver_with_symbol\s*\((.*?)(?:\)\s*\.|\)\s*$)/u);
  const values = quotedStrings(match?.[1] ?? expression);
  return values.at(-1) ?? null;
}

function receiverTypes(expression) {
  const match = expression.match(/receiver(?:_with_symbol|_with_coverage)?\s*\(\s*&?\[([^\]]*)\]/u);
  if (!match) return [];
  return [...match[1].matchAll(/(?:"([^"]+)"|([A-Za-z_][A-Za-z0-9_:]*))/gu)].map((value) => value[1] ?? value[2]);
}

function routeOptions(expression) {
  const options = [];
  const jit = expression.match(/with_jit_symbol\s*\(\s*"([^"]+)"/u);
  if (jit) options.push({ kind: "jit_symbol", value: jit[1] });
  const interpreter = expression.match(/with_interpreter_route\s*\(\s*CoreCallInterpreterRoute::([A-Za-z_]+)/u);
  if (interpreter) options.push({ kind: "interpreter_route", value: interpreter[1] });
  const pure = expression.match(/with_pure_route\s*\(\s*CoreCallPureRoute::([A-Za-z_]+)/u);
  if (pure) options.push({ kind: "pure_route", value: pure[1] });
  for (const target of ["aot", "jit"]) {
    if (expression.includes(`without_direct_${target}`)) options.push({ kind: `without_direct_${target}`, value: true });
  }
  return options;
}

export function parseDispatcher(source) {
  const rows = [];
  const lines = source.split("\n");
  for (const [index, line] of lines.entries()) {
    const match = line.match(/^dispatcher_row\s+(plain|adapter|receiver)\s+(.+?)\s+\|\s*(.*)$/u);
    if (!match) continue;
    const [, family, left, expression] = match;
    const leftText = left.trim();
    const receiver = family === "receiver";
    let module;
    let member;
    if (receiver) {
      const receiverMember = expression.match(
        /receiver(?:_with_symbol|_with_coverage)?\s*\(\s*&?\[[^\]]*\]\s*,\s*(?:"([^"]+)"|([A-Z_][A-Z0-9_]*))/u,
      );
      member = receiverMember?.[1] ?? receiverMember?.[2] ?? leftText.replace(/^[-\s]+/u, "");
      module = "receiver";
    } else {
      const split = leftText.match(/^(.+)\s+([A-Za-z_][A-Za-z0-9_]*)$/u);
      if (!split) fail("invalid_oracle", `cannot parse Core dispatcher row at line ${index + 1}`);
      module = split[1];
      member = split[2];
    }
    const route = dispatcherRoute(expression, family);
    const types = receiver ? receiverTypes(expression) : [];
    const subject = receiver ? `-.${member}` : `${module}.${member}`;
    rows.push({
      id: receiver ? `core-call:receiver:${types.join(",")}::${member}` : `core-call:${family}:${module}.${member}`,
      family: "core",
      kind: family === "adapter" ? "adapter" : "core_call",
      subject,
      rule: family === "adapter" ? "core.adapter.correspondence" : "core.call.observation",
      owner: family === "adapter" ? "#2941" : "#2940",
      source: CORE_PATH,
      source_line: index + 1,
      dispatcher: { family, module, member, receiver_types: types, route_symbol: route, options: routeOptions(expression) },
    });
  }
  if (rows.length === 0) fail("invalid_oracle", "Core dispatcher contains no dispatcher rows");
  const ids = new Set();
  for (const row of rows) {
    if (ids.has(row.id)) fail("invalid_oracle", `duplicate Core dispatcher row: ${row.id}`);
    ids.add(row.id);
  }
  return rows;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}
function implementationDefinition(source, needle) {
  if (!needle) return [];
  const escaped = escapeRegExp(needle);
  const prefix = `(?:pub(?:\\([^)]*\\))?\\s+)?(?:(?:unsafe|async)\\s+)*(?:extern\\s+"[^"]+"\\s+)?`;
  const regex = new RegExp(`${prefix}fn\\s+${escaped}(?:\\s*<[^>]*>)?\\s*\\(`, "gu");
  return [...source.matchAll(regex)].map((match) => ({ offset: match.index, line: lineForOffset(source, match.index), symbol: needle }));
}

function blockEnd(source, openingBrace) {
  let depth = 1;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  for (let index = openingBrace + 1; index < source.length; index += 1) {
    const character = source[index];
    const next = source[index + 1];
    if (lineComment) {
      if (character === "\n") lineComment = false;
      continue;
    }
    if (blockComment) {
      if (character === "*" && next === "/") {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (quote !== null) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }
    if (character === "/" && next === "/") {
      lineComment = true;
      index += 1;
      continue;
    }
    if (character === "/" && next === "*") {
      blockComment = true;
      index += 1;
      continue;
    }
    if (character === "\"" || character === "'" || character === "`") {
      quote = character;
      continue;
    }
    if (character === "{") depth += 1;
    else if (character === "}") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return source.length;
}

function methodDefinition(source, method, receiverTypes = []) {
  if (!method || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(method)) return [];
  const escaped = escapeRegExp(method);
  const prefix = `(?:pub(?:\\([^)]*\\))?\\s+)?(?:(?:unsafe|async)\\s+)*(?:extern\\s+"[^"]+"\\s+)?`;
  const regex = new RegExp(`${prefix}fn\\s+${escaped}(?:\\s*<[^>]*>)?\\s*\\(`, "gu");
  const candidates = [...source.matchAll(regex)];
  if (receiverTypes.length === 0) {
    return candidates.map((match) => ({ offset: match.index, line: lineForOffset(source, match.index), symbol: method }));
  }
  const names = receiverTypes.map((type) => type.split("::").at(-1)).filter(Boolean);
  const implRanges = [];
  for (const impl of source.matchAll(/\bimpl\b[^{]*\{/gu)) {
    const header = impl[0];
    if (!names.some((name) => new RegExp(`(?:^|[^A-Za-z0-9_])${escapeRegExp(name)}(?:$|[^A-Za-z0-9_])`, "u").test(header))) continue;
    const openingBrace = impl.index + impl[0].lastIndexOf("{");
    implRanges.push([impl.index, blockEnd(source, openingBrace)]);
  }
  return candidates
    .filter((match) => implRanges.some(([start, end]) => match.index > start && match.index < end))
    .map((match) => ({ offset: match.index, line: lineForOffset(source, match.index), symbol: method }));
}

export function buildImplementationIndex() {
  const files = [...new Set(IMPLEMENTATION_ROOTS.flatMap((root) => walk(root)))].sort();
  const contents = new Map(files.map((path) => [path, readFileSync(resolve(ROOT, path), "utf8")]));
  return { files, contents, digests: new Map() };
}

function bodySourceFiles(files) {
  return files.filter((path) => !path.endsWith("/Core.jet") && !path.endsWith("/core_calls.rs"));
}

function sourceDigest(path, contents, digests) {
  if (!digests.has(path)) digests.set(path, digestBytes(Buffer.from(contents.get(path), "utf8")));
  return digests.get(path);
}

function bodyDigest(matches, contents, digests) {
  const normalized = matches.map((match) => ({
    path: match.path,
    line: match.line,
    symbol: match.symbol,
    source_sha256: sourceDigest(match.path, contents, digests),
  }));
  return digestText(canonicalJson(normalized));
}

export function bindImplementation(row, index) {
  const { files, contents } = index;
  const digests = index.digests ?? new Map();
  const bodyFiles = bodySourceFiles(files);
  const route = row.dispatcher.route_symbol;
  const matches = [];
  for (const path of bodyFiles) {
    const source = contents.get(path);
    const found = route
      ? implementationDefinition(source, route.split("::").at(-1))
      : methodDefinition(source, row.dispatcher.member, row.dispatcher.receiver_types);
    for (const match of found) matches.push({ path, ...match });
  }
  const unique = [];
  const seen = new Set();
  for (const match of matches) {
    const key = `${match.path}:${match.line}:${match.symbol}`;
    if (!seen.has(key)) {
      seen.add(key);
      unique.push(match);
    }
  }
  unique.sort((left, right) => left.path.localeCompare(right.path) || left.line - right.line);
  const routeText = row.dispatcher.route_symbol ?? "";
  const foreignRoute = /^(?:std|libc|alloc|core|windows|unix|wasm|extern)(?:::|$)/u.test(routeText)
    || /(?:cuda|metal|vulkan|webgpu|browser|sqlite|openssl|raylib|wgpu)/iu.test(routeText);
  const implementationKind = unique.length > 0
    ? (row.dispatcher.receiver_types.length > 0 ? "jet_owned_receiver_body" : "jet_owned_core_body")
    : (foreignRoute ? "foreign_edge" : "unresolved_edge");
  const paths = unique.length > 0 ? [...new Set(unique.map((match) => match.path))] : (foreignRoute ? [CORE_PATH] : []);
  return {
    kind: implementationKind,
    route_symbol: route,
    receiver_types: row.dispatcher.receiver_types,
    source_paths: paths,
    body_sha256: bodyDigest(unique, contents, digests),
    source_sha256: digestText(canonicalJson(paths.map((path) => ({
      path,
      sha256: sourceDigest(path, contents, digests),
    })))),
    body_found: unique.length > 0,
    foreign_edge: foreignRoute,
    dependency_scope: "Prelude and Foundation source trees",
  };
}

const ASSUMPTION_DEFINITIONS = Object.freeze([
  {
    id: "foreign.compiler-abi",
    kind: "compiler",
    edge: "Core dispatcher to AOT/JIT/interpreter lowering and ABI adapters",
    contract: "The named compiler backend preserves Jet value representation, call arity, ownership, and typed failure transport at this edge.",
  },
  {
    id: "foreign.os",
    kind: "os",
    edge: "filesystem, process, environment, terminal, network, and clock host calls",
    contract: "The operating system returns the documented result or a permitted resource/input failure; Jet validation and error translation remain in the wrapper.",
  },
  {
    id: "foreign.library",
    kind: "library",
    edge: "crypto, database, encoding, compression, regex, and service library calls",
    contract: "The foreign library obeys its named encoding, bounds, error, and lifetime contract; a foreign success status does not prove the Jet wrapper.",
  },
  {
    id: "foreign.browser",
    kind: "browser",
    edge: "DOM, browser storage, fetch, canvas, and WebGPU host calls",
    contract: "The browser exposes the declared API and event/callback lifetime; Jet-owned validation, cancellation, and cleanup are checked separately.",
  },
  {
    id: "foreign.device",
    kind: "device",
    edge: "CUDA, Metal, Vulkan, WebGPU, Raylib, and embedded-device calls",
    contract: "The target device and driver honor the declared width, bounds, synchronization, and resource limits.",
  },
  {
    id: "foreign.allocator",
    kind: "allocator",
    edge: "allocation, stack, buffer, stream, task, and resource handles",
    contract: "The host allocator/resource provider satisfies the finite capacity and release contract; Jet memory-safe premises forbid undefined behavior on failure.",
  },
]);
export { ASSUMPTION_DEFINITIONS };

function boundaryEntry(row, stage, stageProof = null, contract, replayProof = false) {
  const paths = stage === "prelude"
    ? ["crates/jet-codegen/src/Prelude/Core.rs", "crates/jet-codegen/src/Prelude/Core"]
    : ["crates/jet-foundation/src/Task.rs", "crates/jet-foundation/src"];
  const sourceRecords = paths.map((path) => ({ path, source_sha256: fileDigest(path) }));
  const implementation = {
    kind: "jet_owned_boundary",
    route_symbol: null,
    receiver_types: [],
    source_paths: paths,
    source_spans: paths.map((path) => ({ path, line: 1, symbol: null, role: "boundary-authority" })),
    source_sha256: digestText(canonicalJson(sourceRecords)),
    body_sha256: digestText(canonicalJson(sourceRecords)),
    body_found: paths.every((path) => existsSync(resolve(ROOT, path))),
    foreign_edge: false,
    dependency_scope: "Prelude and Foundation source trees",
  };
  const synthetic = {
    ...row,
    source_line: row.source_line ?? 1,
    dispatcher: {
      family: "boundary",
      module: row.id.replace(/^boundary:/u, ""),
      member: row.subject,
      receiver_types: [],
      route_symbol: null,
      options: [],
    },
  };
  return entryForProof(synthetic, implementation, stage, stageProof, contract, replayProof);
}
function assumptionsFor(row, implementation) {
  const module = `${row.dispatcher.module}.${row.dispatcher.member}`.toLowerCase();
  const route = `${implementation.route_symbol ?? ""}`.toLowerCase();
  const result = ["foreign.compiler-abi"];
  const add = (id) => { if (!result.includes(id)) result.push(id); };
  if (/(file|fs|sys|process|term|net|http|socket|time|watcher|env|signal|pipe|path)/u.test(module)) add("foreign.os");
  if (/(crypto|db|data|encoding|json|csv|xml|cbor|email|regex|gzip|hash|plugin|mod)/u.test(module) || /(openssl|sqlite|argon|crypto|zstd|regex)/u.test(route)) add("foreign.library");
  if (/(web|browser|dom|canvas|ui|plot|virtual|store|table|fetch)/u.test(module) || /(browser|dom|webgpu|wasm)/u.test(route)) add("foreign.browser");
  if (/(compute|device|gpu|raylib|hardware|simd)/u.test(module) || /(cuda|metal|vulkan|webgpu|raylib)/u.test(route)) add("foreign.device");
  if (/(mem|alloc|stream|task|job|parallel|compute|list|array|buffer)/u.test(module) || /(alloc|malloc|stack|buffer|stream)/u.test(route)) add("foreign.allocator");
  return result;
}

const POLICY_TEXT = Object.freeze({
  validation: "Jet checks input shape, authority, and preconditions before crossing a host edge; a failed check is a typed Jet failure.",
  defaults: "Defaults are selected by the Jet-owned body and are recorded as part of the call contract, never inferred from a foreign success value.",
  bounds: "Widths, indices, lengths, and resource limits are checked before conversion or host access; overflow is a permitted typed failure, not truncation.",
  encoding: "Jet owns text/bytes and structured-wire encoding at the boundary; foreign codecs are assumptions after the validated edge.",
  numerics: "Jet owns numeric domain, precision, rounding, and checked conversion policy; host arithmetic is an explicit suffix.",
  lifecycle: "Handles, receivers, tasks, and callbacks have a declared state transition and cannot be used after terminal cleanup.",
  effects: "The body declares and preserves its effect/capability requirements; adapter routing does not grant an undeclared effect.",
  cancellation: "Cancellation is an observable terminal transition with no post-cancel callback or result commitment.",
  cleanup: "Every acquired resource and callback registration has exactly one cleanup path, including typed failure and cancellation.",
});

const MEMORY_PREMISES = Object.freeze([
  {
    id: "memory-safe-call",
    source: "crates/jet-foundation/src/MemSentry.rs",
    statement: "Vetted memory internals may use unsafe operations only behind checked pointer, allocation, alignment, lifetime, and failure premises; a safe-call wrapper does not erase those premises.",
  },
  {
    id: "memory-checked-allocation",
    source: "crates/jet-codegen/src/Prelude/Core/FixedAllocator.rs",
    statement: "Allocator capacity, layout, and release outcomes are explicit resource premises; exhaustion is a typed resource failure, not undefined behavior.",
  },
  {
    id: "memory-receiver-lifetime",
    source: "crates/jet-codegen/src/Prelude/Core.rs",
    statement: "Receiver and callback handles cannot be used after their terminal lifecycle transition; safe calls retain the owner and cleanup premises.",
  },
]);

export { MEMORY_PREMISES };

export function policyBindings(row, stage) {
  const bindings = {};
  for (const policy of REQUIRED_POLICIES) {
    bindings[policy] = {
      owner: "Jet",
      stage,
      status: "unverified",
      rule: POLICY_TEXT[policy],
      source_obligation: row.id,
      evidence: "implementation-bound source certificate; no execution evidence is claimed by this stage",
    };
  }
  return bindings;
}

function tiersFor(row) {
  const options = row.dispatcher.options;
  const noAot = options.some((option) => option.kind === "without_direct_aot");
  const noJit = options.some((option) => option.kind === "without_direct_jit");
  return {
    aot: { applicable: !noAot, route: noAot ? "ambient_or_adapter" : "shared-prelude" },
    jet_run: { applicable: !noJit, route: noJit ? "ambient_or_adapter" : "shared-prelude" },
    interpreter: { applicable: true, route: options.some((option) => option.kind === "interpreter_route") ? "declared-interpreter-route" : "shared-prelude" },
    web: { applicable: /(web|browser|dom|canvas|compute|device)/iu.test(row.subject), route: "target-dependent" },
  };
}

export function buildEntry(row, implementation, stage, stageProof = null) {
  const assumptions = assumptionsFor(row, implementation);
  const policies = policyBindings(row, stage);
  const tiers = tiersFor(row);
  const memorySafePremises = MEMORY_PREMISES.map((premise) => ({
    ...premise,
    status: "unverified",
    source_obligation: row.id,
    evidence: "memory-safe premise is bound to the source route; this stage does not claim execution or universal proof",
  }));
  const premiseRecord = { policies, memory_safe_premises: memorySafePremises };
  const certificate = {
    ...implementation,
    memory_safe_premises: memorySafePremises,
    premise_sha256: digestText(canonicalJson(premiseRecord)),
    target_sha256: digestText(canonicalJson(tiers)),
    configuration_sha256: cargoDigest(),
  };
  const proof = stageProof?.verified === true ? {
    claim_id: stageProof.claim_id,
    identity_sha256: stageProof.identity_sha256,
    formal_goal: stageProof.formal_goal,
    proposition_sha256: stageProof.proposition_sha256,
    model_sha256: stageProof.semantic_binding?.model_sha256 ?? stageProof.model_sha256,
    implementation_sha256: stageProof.semantic_binding?.implementation_sha256 ?? stageProof.implementation_sha256,
    artifact: stageProof.artifact,
    binding: stageProof.binding,
    semantic_binding: stageProof.semantic_binding,
    proof_object: {
      ...stageProof.proof_object,
      model_sha256: stageProof.semantic_binding?.model_sha256 ?? stageProof.proof_object?.model_sha256,
      implementation_sha256: stageProof.semantic_binding?.implementation_sha256 ?? stageProof.proof_object?.implementation_sha256,
      semantic_binding: stageProof.semantic_binding,
    },
    proof_replay: stageProof.proof_replay,
    applies_to_stage_semantics: true,
    qualification: "obligation-specific compiler-proof certificate is validated and exactly bound to this source, body, premise, target, configuration, and candidate identity",
    universal_proof: false,
  } : null;
  const disposition = proof ? "proved" : implementation.body_found ? "mapped_unproved" : "unsupported";
  return {
    id: row.id,
    subject: row.subject,
    family: row.family,
    kind: row.kind,
    owner: row.owner,
    source: { path: row.source, line: row.source_line },
    dispatcher: canonical(row.dispatcher),
    implementation: certificate,
    ...(proof ? { proof } : {}),
    disposition,
    disposition_reason: proof
      ? "A validated obligation-specific certificate discharges this row; universal Core/runtime qualification remains separate."
      : implementation.body_found
        ? "Jet-owned body is located and identity-bound; no bounded observation was executed, so this remains unproved."
        : implementation.foreign_edge
          ? "The route crosses a named foreign edge; the foreign contract is an assumption and no Jet-owned body proof is inferred."
          : "No implementation body was located in the authoritative Prelude/Foundation roots; proof is blocked rather than replaced by a placeholder.",
    policies,
    tiers,
    foreign_assumptions: assumptions,
    observations: Object.fromEntries(OBSERVATION_FIELDS.map((field) => [field, {
      status: "unverified",
      reason: "no compiler/runtime observation is executed by the source certificate stage",
    }])),
  };
}


function entryForProof(row, implementation, stage, stageProof, contract, replayProof = false) {
  const base = buildEntry(row, implementation, stage);
  if (!stageProof?.structurally_valid) return base;
  if (!base.implementation.body_found || base.implementation.foreign_edge) {
    stageProof.status = "invalid";
    stageProof.reason = "obligation-specific proof cannot discharge a missing or foreign implementation body";
    return {
      ...base,
      proof_rejection: {
        claim_id: stageProof.claim_id,
        status: "invalid",
        reason: stageProof.reason,
      },
    };
  }
  const binding = validateStageProofBinding(stageProof, contract, row, base.implementation);
  if (!binding.verified) {
    stageProof.status = "invalid";
    stageProof.reason = binding.reason;
    return {
      ...base,
      proof_rejection: {
        claim_id: stageProof.claim_id,
        status: "invalid",
        reason: stageProof.reason,
      },
    };
  }
  if (!replayProof) return base;
  const semantic = binding.semantic_binding;
  const candidate = stageProof.candidate;
  const replayAuthority = {
    authority_key: semantic.authority_key,
    obligation_id: row.id,
  };
  let replayResult;
  try {
    replayResult = replay(contract.identity.manifest_path, candidate.id, replayAuthority);
    if (replayResult.status !== "proved" || replayResult.claim_id !== candidate.id) {
      throw new Error("pinned compiler-proof replay did not prove the selected stage claim");
    }
    if (replayResult.identity_sha256 !== stageProof.identity_sha256) {
      throw new Error("pinned compiler-proof replay identity does not match the validated stage claim");
    }
    const expectedComponents = {
      claim: {
        id: candidate.id,
        proposition: candidate.proposition,
        proposition_sha256: candidate.proposition_sha256,
      },
      model: candidate.model,
      source: candidate.source,
      implementation: candidate.implementation,
      correspondence: candidate.correspondence,
      candidate: candidate.candidate,
      checker: candidate.checker,
      assumptions: candidate.assumptions,
      proof: { artifact: candidate.proof.artifact },
    };
    if (canonicalJson(replayResult.components) !== canonicalJson(expectedComponents)) {
      throw new Error("pinned compiler-proof replay components do not match the selected stage claim");
    }
  } catch (error) {
    stageProof.status = "invalid";
    stageProof.reason = String(error);
    return {
      ...base,
      proof_rejection: {
        claim_id: stageProof.claim_id,
        status: "invalid",
        reason: stageProof.reason,
      },
    };
  }
  const proofReplay = {
    schema: "jet.compiler-proof-replay.v1",
    status: "verified",
    result: replayResult.status,
    exit_code: replayResult.replay?.exit_code,
    stdout: replayResult.replay
      ? { text: replayResult.replay.stdout, sha256: replayResult.replay.stdout_sha256 }
      : undefined,
    stderr: replayResult.replay
      ? { text: replayResult.replay.stderr, sha256: replayResult.replay.stderr_sha256 }
      : undefined,
    verifier: "compiler-proof.replay",
    execution: "pinned Lean replay executed and component identity matched",
  };
  stageProof.verified = true;
  stageProof.status = "validated_proved";
  stageProof.proof_replay = proofReplay;
  stageProof.proof_object = {
    ...stageProof.proof_object,
    status: "verified",
    semantic_binding: semantic,
  };
  return buildEntry(row, implementation, stage, {
    ...stageProof,
    verified: true,
    semantic_binding: semantic,
    proof_replay: proofReplay,
    proof_object: stageProof.proof_object,
  });
}
export function validateContract(contract, stage, checkerPath) {
  object(contract, `${stage} contract`);
  if (contract.schema !== `jet.${stage}-contract.v1`) fail("invalid_oracle", `${stage} contract schema is invalid`);
  if (contract.schema_version !== 1) fail("invalid_oracle", `${stage} contract schema_version is not 1`);
  if (contract.stage !== stage) fail("invalid_oracle", `${stage} contract stage does not match`);
  text(contract.contract_id, `${stage} contract.contract_id`);
  object(contract.claim, `${stage} contract.claim`);
  text(contract.claim.kind, `${stage} contract.claim.kind`);
  text(contract.claim.statement, `${stage} contract.claim.statement`);
  object(contract.coverage, `${stage} contract.coverage`);
  object(contract.execution, `${stage} contract.execution`);
  text(contract.coverage.relation_path, `${stage} contract.coverage.relation_path`);
  if (contract.coverage.relation_path !== OBLIGATIONS_PATH) fail("invalid_oracle", `${stage} coverage relation must be ${OBLIGATIONS_PATH}`);
  if (contract.coverage.owner !== "#2940") fail("invalid_oracle", `${stage} coverage owner must be #2940`);
  array(contract.coverage.mapping_statuses, `${stage} contract.coverage.mapping_statuses`);
  for (const status of contract.coverage.mapping_statuses) if (!DISPOSITION_SET.has(status)) fail("invalid_oracle", `unknown coverage status ${status}`);
  object(contract.inventory_join, `${stage} contract.inventory_join`);
  array(contract.inventory_join.authorities, `${stage} contract.inventory_join.authorities`);
  if (!contract.inventory_join.authorities.includes("#2335") || !contract.inventory_join.authorities.includes("#2898")) {
    fail("invalid_oracle", `${stage} contract must join #2335 and #2898 inventories`);
  }
  for (const authority of ["#2335", "#2898"]) {
    object(contract.inventory_join[authority], `${stage} contract.inventory_join.${authority}`);
    const source = projectPath(contract.inventory_join[authority].source, `${stage} inventory source`);
    if (!existsSync(resolve(ROOT, source))) fail("unavailable", `${stage} inventory source is missing: ${source}`);
    array(contract.inventory_join[authority].selection, `${stage} inventory selection`);
    array(contract.inventory_join[authority].dimensions, `${stage} inventory dimensions`);
  }
  text(contract.inventory_join.join_rule, `${stage} inventory join rule`);
  const dimensions = array(contract.obligation_dimensions, `${stage} contract.obligation_dimensions`);
  for (const policy of REQUIRED_POLICIES) if (!dimensions.includes(policy)) fail("invalid_oracle", `${stage} obligation dimensions omit ${policy}`);
  for (const dimension of ["receiver", "field", "task", "resource", "error"]) {
    if (!dimensions.includes(dimension)) fail("invalid_oracle", `${stage} obligation dimensions omit ${dimension}`);
  }
  const memoryPremises = array(contract.memory_premises, `${stage} contract.memory_premises`);
  const requiredPremiseIds = new Set(MEMORY_PREMISES.map((premise) => premise.id));
  const seenPremiseIds = new Set();
  for (const premise of memoryPremises) {
    object(premise, `${stage} memory premise`);
    text(premise.id, `${stage} memory premise id`);
    if (seenPremiseIds.has(premise.id)) fail("invalid_oracle", `${stage} duplicate memory premise ${premise.id}`);
    seenPremiseIds.add(premise.id);
    if (!requiredPremiseIds.has(premise.id)) fail("invalid_oracle", `${stage} unknown memory premise ${premise.id}`);
    const source = projectPath(premise.source, `${stage} memory premise source`);
    if (!existsSync(resolve(ROOT, source))) fail("unavailable", `${stage} memory premise source is missing: ${source}`);
    text(premise.statement, `${stage} memory premise statement`);
  }
  for (const id of requiredPremiseIds) if (!seenPremiseIds.has(id)) fail("invalid_oracle", `${stage} contract omits memory premise ${id}`);
  text(contract.identity.schema, `${stage} contract.identity.schema`);
  text(contract.identity.manifest_path, `${stage} contract.identity.manifest_path`);
  if (stage === "runtime" || stage === "prelude") {
    text(contract.identity.authority_key, `${stage} contract.identity.authority_key`);
    text(contract.identity.required_authority, `${stage} contract.identity.required_authority`);
    const expectedAuthorityKey = `compiler-proof.${stage}`;
    if (contract.identity.authority_key !== expectedAuthorityKey) fail("invalid_oracle", `${stage} contract authority key is not ${expectedAuthorityKey}`);
    if (contract.identity.required_authority !== "manifest.approved_authorities[identity.authority_key].goals[obligation_id]") {
      fail("invalid_oracle", `${stage} contract required authority selector is invalid`);
    }
  }
  text(contract.witnesses_sha256, `${stage} contract.witnesses_sha256`);
  array(contract.binding_methods, `${stage} contract.binding_methods`);
  const methods = new Set(contract.binding_methods.map((method) => method.method));
  for (const method of ["verified_construction", "direct_verification", "translation_certificate"]) {
    if (!methods.has(method)) fail("invalid_oracle", `${stage} contract omits binding method ${method}`);
  }
  const assumptions = array(contract.foreign_assumptions, `${stage} contract.foreign_assumptions`);
  const assumptionIds = new Set(ASSUMPTION_DEFINITIONS.map((entry) => entry.id));
  for (const assumption of assumptions) {
    object(assumption, `${stage} contract foreign assumption`);
    if (!assumptionIds.has(assumption.id)) fail("invalid_oracle", `unknown foreign assumption ${String(assumption.id)}`);
    text(assumption.edge, `${stage} foreign assumption edge`);
    text(assumption.contract, `${stage} foreign assumption contract`);
  }
  const checker = projectPath(checkerPath, `${stage} checker path`);
  if (!existsSync(resolve(ROOT, checker))) fail("unavailable", `${stage} checker is missing: ${checker}`);
  return contract;
}

export function validateWitnesses(witnesses, stage) {
  object(witnesses, `${stage} witnesses`);
  if (witnesses.schema !== "jet.core-runtime-witnesses.v1") fail("invalid_oracle", `${stage} witness schema is invalid`);
  if (witnesses.schema_version !== 1) fail("invalid_oracle", `${stage} witness schema_version is not 1`);
  if (witnesses.stage !== stage) fail("invalid_oracle", `${stage} witness stage does not match`);
  const identityFields = array(witnesses.identity_fields, `${stage} witnesses.identity_fields`);
  for (const field of ["program_sha256", "source_sha256", "body_sha256", "premise_sha256", "target_sha256", "configuration_sha256"]) {
    if (!identityFields.includes(field)) fail("invalid_oracle", `${stage} witnesses omit identity field ${field}`);
  }
  object(witnesses.runner, `${stage} witnesses.runner`);
  text(witnesses.runner.launcher, `${stage} witness runner.launcher`);
  text(witnesses.runner.unknown_policy, `${stage} witness runner.unknown_policy`);
  object(witnesses.coverage, `${stage} witnesses.coverage`);
  text(witnesses.coverage.relation, `${stage} witness coverage relation`);
  text(witnesses.coverage.owner, `${stage} witness coverage owner`);
  if (witnesses.coverage.owner !== "#2940") fail("invalid_oracle", `${stage} witness coverage owner must be #2940`);
  array(witnesses.observation_fields, `${stage} witnesses.observation_fields`);
  for (const field of OBSERVATION_FIELDS) if (!witnesses.observation_fields.includes(field)) fail("invalid_oracle", `${stage} witnesses omit observation field ${field}`);
  const controls = array(witnesses.negative_controls, `${stage} witnesses.negative_controls`);
  const categories = new Set();
  const ids = new Set();
  const required = ["wrong-default", "width-bounds-conversion", "cleanup-count", "callback-lifetime", "cancellation", "error-interpretation", "encoding-numeric-boundary"];
  for (const [index, control] of controls.entries()) {
    object(control, `${stage} negative control ${index}`);
    text(control.id, `${stage} negative control id`);
    text(control.category, `${stage} negative control category`);
    if (ids.has(control.id)) fail("invalid_oracle", `duplicate negative control ${control.id}`);
    ids.add(control.id);
    categories.add(control.category);
    array(control.obligations, `${stage} negative control obligations`);
    array(control.source_refs, `${stage} negative control source_refs`);
    object(control.mutation, `${stage} negative control mutation`);
    object(control.expected, `${stage} negative control expected`);
    if (control.expected.status !== "mismatch") fail("invalid_oracle", `${control.id} must expect mismatch`);
    if (control.expected.universal_proof !== false) fail("invalid_oracle", `${control.id} cannot claim universal proof`);
    for (const path of control.source_refs) {
      const project = projectPath(path, `${control.id} source ref`);
      if (!existsSync(resolve(ROOT, project))) fail("unavailable", `${control.id} source ref is missing: ${project}`);
    }
  }
  for (const category of required) if (!categories.has(category)) fail("invalid_oracle", `${stage} witnesses omit ${category}`);
  return { controls, categories: [...categories].sort() };
}

export function checkObligationCoverage(obligations, entries) {
  object(obligations, "obligation relation");
  if (obligations.schema !== "jet.compiler-obligations.v1") fail("invalid_oracle", "obligation relation schema is invalid");
  const expected = obligations.rows.filter((row) => row.owner === "#2940");
  const entryById = new Map(entries.map((entry) => [entry.id, entry]));
  const seen = new Set();
  const rows = expected.map((row) => {
    if (seen.has(row.id)) fail("invalid_oracle", `duplicate obligation id ${row.id}`);
    seen.add(row.id);
    const entry = entryById.get(row.id);
    if (!entry) return {
      ...row,
      disposition: "unmapped",
      disposition_reason: "No stage binding was generated for the current obligation.",
      implementation_bound: false,
    };
    if (!DISPOSITION_SET.has(entry.disposition)) fail("invalid_oracle", `invalid disposition for ${row.id}`);
    return {
      ...row,
      disposition: entry.disposition,
      disposition_reason: entry.disposition_reason,
      implementation_bound: entry.implementation.body_found,
      ...(entry.proof ? { proof: entry.proof } : {}),
      implementation: {
        kind: entry.implementation.kind,
        source_paths: entry.implementation.source_paths,
        source_sha256: entry.implementation.source_sha256,
        body_sha256: entry.implementation.body_sha256,
      },
    };
  });
  const dispositionCounts = Object.fromEntries([...DISPOSITION_SET].map((status) => [status, 0]));
  for (const row of rows) dispositionCounts[row.disposition] += 1;
  return {
    relation_path: OBLIGATIONS_PATH,
    relation_sha256: fileDigest(OBLIGATIONS_PATH),
    denominator: rows.length,
    rows,
    disposition_counts: dispositionCounts,
    qualification: "blocked",
    universal_proof: false,
    complete: rows.length === expected.length && rows.every((row) => row.disposition !== "unmapped"),
  };
}

function approvedAuthority(manifest, contract) {
  const key = contract.identity?.authority_key;
  const registry = manifest.approved_authorities;
  if (!key || !registry || typeof registry !== "object") return null;
  const authority = registry[key];
  if (!authority || typeof authority !== "object") return null;
  return { ...authority, authority_key: key };
}

function approvedGoal(stageProof, row) {
  const authority = stageProof.approved_authority;
  if (!authority) return { verified: false, reason: "approved semantic authority is unavailable" };
  const goals = authority.goals;
  if (!goals || typeof goals !== "object") return { verified: false, reason: "approved semantic authority has no per-obligation goal map" };
  const goal = goals[row.id];
  if (!goal || typeof goal !== "object") return { verified: false, reason: `no approved formal goal exists for obligation ${row.id}` };
  if (goal.claim_id !== undefined && goal.claim_id !== stageProof.candidate?.id) {
    return { verified: false, reason: "approved formal goal claim_id does not match stage claim" };
  }
  if (typeof goal.proposition !== "string" || typeof goal.formal_goal !== "string") {
    return { verified: false, reason: "approved formal goal has no proposition/formal_goal" };
  }
  if (!goal.model || typeof goal.model.path !== "string" || typeof goal.model.sha256 !== "string") {
    return { verified: false, reason: "approved formal goal has no trusted model reference" };
  }
  if (!goal.implementation || typeof goal.implementation !== "object"
    || typeof goal.implementation.id !== "string"
    || typeof goal.implementation.representation !== "string"
    || typeof goal.implementation.path !== "string"
    || typeof goal.implementation.sha256 !== "string") {
    return { verified: false, reason: "approved formal goal has no complete implementation reference" };
  }
  if (!goal.correspondence || typeof goal.correspondence !== "object"
    || typeof goal.correspondence.method !== "string"
    || typeof goal.correspondence.route !== "string"
    || typeof goal.correspondence.checked_declaration !== "string") {
    return { verified: false, reason: "approved formal goal has no complete correspondence reference" };
  }
  const authorityRefs = {
    authority_key: authority.authority_key,
    contract_path: authority.contract_path,
    contract_sha256: authority.contract_sha256,
    obligations_path: authority.obligations_path,
    obligations_sha256: authority.obligations_sha256,
  };
  return {
    verified: true,
    authority,
    goal,
    expected: {
      authority: authorityRefs,
      obligation: {
        id: row.id,
        family: row.family,
        kind: row.kind,
        subject: row.subject,
        rule: row.rule,
        source: row.source,
        source_line: row.source_line,
      },
      claim_id: goal.claim_id,
      model: goal.model,
      proposition: goal.proposition,
      proposition_sha256: digestText(goal.proposition),
      formal_goal: goal.formal_goal,
      formal_goal_sha256: digestText(goal.formal_goal),
      implementation: goal.implementation,
      implementation_sha256: goal.implementation.sha256,
      correspondence: goal.correspondence,
      correspondence_sha256: digestText(canonicalJson(goal.correspondence)),
      candidate: goal.candidate,
    },
  };
}

function authorityProof(contract, row, certificate, stageProof) {
  const resolved = approvedGoal(stageProof, row);
  if (!resolved.verified) return resolved;
  return {
    ...resolved,
    expected: {
      ...resolved.expected,
      runtime_implementation: canonicalRuntimeImplementation(certificate),
      runtime_implementation_sha256: digestText(canonicalJson(canonicalRuntimeImplementation(certificate))),
    },
  };
}

function validateStageProofBinding(stageProof, contract, row, certificate) {
  const candidate = stageProof.candidate;
  if (!candidate || typeof candidate !== "object") return { verified: false, reason: "stage proof candidate is unavailable" };
  const binding = stageProof.binding;
  const resolved = authorityProof(contract, row, certificate, stageProof);
  if (!resolved.verified) return resolved;
  const { authority, expected } = resolved;
  if (candidate.proposition !== expected.proposition) return { verified: false, reason: "stage proof proposition does not match approved authority" };
  if (candidate.proof?.formal_goal !== expected.formal_goal) return { verified: false, reason: "stage proof formal goal does not match approved authority" };
  if (candidate.model.path !== expected.model.path || candidate.model.sha256 !== expected.model.sha256) {
    return { verified: false, reason: "stage proof model does not match approved authority" };
  }
  if (candidate.implementation.path !== expected.implementation.path || candidate.implementation.sha256 !== expected.implementation.sha256) {
    return { verified: false, reason: "stage proof implementation does not match approved authority" };
  }
  if (authority.correspondence && canonicalJson(candidate.correspondence) !== canonicalJson(authority.correspondence)) {
    return { verified: false, reason: "stage proof correspondence does not match approved authority" };
  }
  if (authority.candidate && canonicalJson(candidate.candidate) !== canonicalJson(authority.candidate)) {
    return { verified: false, reason: "stage proof candidate does not match approved authority" };
  }
  const expectedBinding = {
    authority_key: authority.authority_key,
    stage: contract.stage,
    obligation_id: row.id,
    obligation_rule: row.rule,
    obligation_subject: row.subject,
    source_path: row.source,
    source_line: row.source_line,
    source_sha256: certificate.source_sha256,
    body_sha256: certificate.body_sha256,
    premise_sha256: certificate.premise_sha256,
    target_sha256: certificate.target_sha256,
    configuration_sha256: certificate.configuration_sha256,
    proposition_sha256: expected.proposition_sha256,
    formal_goal_sha256: expected.formal_goal_sha256,
    model_sha256: expected.model.sha256,
    implementation_sha256: expected.implementation.sha256,
    correspondence_sha256: expected.correspondence_sha256,
    claim_model_sha256: stageProof.claim_model_sha256,
    claim_implementation_sha256: stageProof.claim_implementation_sha256,
    candidate_identity_sha256: stageProof.candidate_identity_sha256,
  };
  for (const [field, value] of Object.entries(expectedBinding)) {
    if (binding[field] !== value) return { verified: false, reason: `stage proof binding mismatch: ${field}` };
  }
  return {
    verified: true,
    semantic_binding: {
      schema: "jet.core-runtime-semantic-binding.v1",
      authority_key: authority.authority_key,
      authority: {
        authority_key: authority.authority_key,
        obligation_id: row.id,
        contract_path: authority.contract_path,
        contract_sha256: authority.contract_sha256,
        obligations_path: authority.obligations_path,
        obligations_sha256: authority.obligations_sha256,
        model: expected.model,
        goal: {
          claim_id: expected.claim_id,
          model: expected.model,
          proposition: expected.proposition,
          formal_goal: expected.formal_goal,
          implementation: expected.implementation,
          correspondence: expected.correspondence,
          ...(expected.candidate ? { candidate: expected.candidate } : {}),
        },
      },
      obligation: expected.obligation,
      expected_proposition: expected.proposition,
      expected_proposition_sha256: expected.proposition_sha256,
      expected_formal_goal: expected.formal_goal,
      expected_formal_goal_sha256: expected.formal_goal_sha256,
      model_sha256: expected.model.sha256,
      implementation_sha256: expected.implementation_sha256,
      proposition: expected.proposition,
      proposition_sha256: expected.proposition_sha256,
      formal_goal: expected.formal_goal,
      formal_goal_sha256: expected.formal_goal_sha256,
      model: expected.model,
      implementation: expected.implementation,
      correspondence: expected.correspondence,
      correspondence_sha256: expected.correspondence_sha256,
      ...(expected.candidate ? { candidate: expected.candidate } : {}),
      runtime_implementation: expected.runtime_implementation,
      runtime_implementation_sha256: expected.runtime_implementation_sha256,
    },
  };
}
export function checkProofIdentity(contract, { replayProof = false } = {}) {
  const manifest = readJson(contract.identity.manifest_path, "pinned compiler-proof manifest");
  let summary;
  try {
    summary = validateManifest(manifest);
  } catch (error) {
    fail("invalid_oracle", "pinned compiler-proof manifest failed validation", { error: String(error) });
  }
  const claimId = text(contract.identity.claim_id, "contract.identity.claim_id");
  const claim = manifest.claims.find((candidate) => candidate.id === claimId);
  if (!claim) fail("unavailable", `pinned compiler-proof claim is missing: ${claimId}`);
  let certificate;
  try {
    certificate = validateClaim(claim, summary.pin, { checkFiles: true, allowNonProof: true });
  } catch (error) {
    fail("invalid_oracle", "pinned compiler-proof certificate could not be consumed", { error: String(error) });
  }

  const authority = approvedAuthority(manifest, contract);
  const stageClaims = [];
  for (const candidate of manifest.claims) {
    const binding = candidate.stage_binding;
    if (!binding || binding.stage !== contract.stage) continue;
    const record = {
      claim_id: candidate.id,
      stage: contract.stage,
      result: candidate.result,
      binding,
      candidate,
      approved_authority: authority,
      verified: false,
      status: "invalid",
    };
    try {
      if (candidate.id === claim.id) throw new Error("identity-direct claim cannot discharge a stage obligation");
      if (binding.schema !== "jet.core-runtime-proof-binding.v1") throw new Error("stage proof binding schema is invalid");
      if (typeof binding.obligation_id !== "string" || binding.obligation_id.length === 0) throw new Error("stage proof obligation_id is missing");
      if (typeof binding.obligation_rule !== "string" || binding.obligation_rule.length === 0) throw new Error("stage proof obligation_rule is missing");
      if (!authority) throw new Error("approved semantic authority is unavailable");
      if (binding.authority_key !== authority.authority_key) throw new Error("stage proof authority key is not approved");
      const approvedGoalRecord = authority.goals?.[binding.obligation_id];
      if (!approvedGoalRecord || typeof approvedGoalRecord !== "object") {
        throw new Error(`no approved formal goal exists for obligation ${binding.obligation_id}`);
      }
      if (approvedGoalRecord.claim_id !== undefined && approvedGoalRecord.claim_id !== candidate.id) {
        throw new Error("approved formal goal claim_id does not match stage claim");
      }
      for (const field of [
        "authority_key",
        "source_path",
        "source_sha256",
        "body_sha256",
        "premise_sha256",
        "target_sha256",
        "configuration_sha256",
        "proposition_sha256",
        "formal_goal_sha256",
        "model_sha256",
        "implementation_sha256",
        "correspondence_sha256",
        "claim_model_sha256",
        "claim_implementation_sha256",
        "candidate_identity_sha256",
      ]) {
        if (typeof binding[field] !== "string" || binding[field].length === 0) throw new Error(`stage proof binding field is missing: ${field}`);
      }
      const proofCertificate = validateClaim(candidate, summary.pin, {
        checkFiles: true,
        allowNonProof: candidate.result !== "proved",
      });
      if (candidate.proposition !== approvedGoalRecord.proposition) throw new Error("stage proof proposition does not match approved formal goal");
      if (candidate.proof.formal_goal !== approvedGoalRecord.formal_goal) throw new Error("stage proof formal goal does not match approved formal goal");
      if (candidate.model.path !== approvedGoalRecord.model?.path || candidate.model.sha256 !== approvedGoalRecord.model?.sha256) {
        throw new Error("stage proof model does not match approved formal goal");
      }
      if (candidate.implementation.path !== approvedGoalRecord.implementation?.path
        || candidate.implementation.sha256 !== approvedGoalRecord.implementation?.sha256) {
        throw new Error("stage proof implementation does not match approved formal goal");
      }
      if (canonicalJson(candidate.correspondence) !== canonicalJson(approvedGoalRecord.correspondence)) {
        throw new Error("stage proof correspondence does not match approved formal goal");
      }
      if (approvedGoalRecord.candidate && canonicalJson(candidate.candidate) !== canonicalJson(approvedGoalRecord.candidate)) {
        throw new Error("stage proof candidate does not match approved formal goal");
      }
      if (canonicalJson(candidate.candidate) !== canonicalJson(claim.candidate)) {
        throw new Error("stage proof candidate does not match the pinned compiler candidate");
      }
      const candidateIdentity = digestText(canonicalJson(candidate.candidate));
      if (binding.candidate_identity_sha256 !== candidateIdentity) throw new Error("stage proof candidate identity does not match its candidate record");
      if (binding.proposition_sha256 !== candidate.proposition_sha256) throw new Error("stage proof proposition binding does not match the claim");
      if (binding.formal_goal_sha256 !== digestText(candidate.proof.formal_goal)) throw new Error("stage proof formal-goal binding does not match the claim");
      if (binding.claim_model_sha256 !== candidate.model.sha256) throw new Error("stage proof claim-model binding does not match the claim");
      if (binding.claim_implementation_sha256 !== candidate.implementation.sha256) throw new Error("stage proof claim-implementation binding does not match the claim");
      if (binding.correspondence_sha256 !== digestText(canonicalJson(candidate.correspondence))) throw new Error("stage proof correspondence binding does not match the claim");
      const replayResult = null;
      const replayVerified = false;
      const authorityModelSha256 = approvedGoalRecord.model.sha256;
      record.authority_key = authority.authority_key;
      record.identity_sha256 = proofCertificate.identity_sha256;
      record.proposition = candidate.proposition;
      record.proposition_sha256 = candidate.proposition_sha256;
      record.formal_goal = candidate.proof.formal_goal;
      record.formal_goal_sha256 = digestText(candidate.proof.formal_goal);
      record.correspondence = candidate.correspondence;
      record.model = approvedGoalRecord.model;
      record.model_sha256 = authorityModelSha256;
      record.implementation_sha256 = approvedGoalRecord.implementation.sha256;
      record.claim_model_sha256 = candidate.model.sha256;
      record.claim_implementation_sha256 = candidate.implementation.sha256;
      record.candidate_identity_sha256 = candidateIdentity;
      record.proof_object = {
        replay_ref: { path: contract.identity.manifest_path, claim_id: candidate.id },
        schema: "jet.compiler-proof-object.v1",
        status: replayVerified ? "verified" : "unverified",
        claim_id: candidate.id,
        proposition: candidate.proposition,
        proposition_sha256: candidate.proposition_sha256,
        formal_goal_sha256: binding.formal_goal_sha256,
        model: candidate.model,
        source: candidate.source,
        implementation: candidate.implementation,
        correspondence: candidate.correspondence,
        candidate: candidate.candidate,
        checker: candidate.checker,
        assumptions: candidate.assumptions,
        artifact: candidate.proof.artifact,
        identity_sha256: proofCertificate.identity_sha256,
      };
      record.proof_replay = {
        schema: "jet.compiler-proof-replay.v1",
        status: replayVerified ? "verified" : "unverified",
        result: replayResult?.status ?? candidate.result,
        exit_code: replayResult?.replay?.exit_code ?? candidate.raw_replay.exit_code,
        stdout: replayResult?.replay
          ? { text: replayResult.replay.stdout, sha256: replayResult.replay.stdout_sha256 }
          : candidate.raw_replay.stdout,
        stderr: replayResult?.replay
          ? { text: replayResult.replay.stderr, sha256: replayResult.replay.stderr_sha256 }
          : candidate.raw_replay.stderr,
        verifier: replayVerified ? "compiler-proof.replay" : "compiler-proof.validateClaim",
        execution: replayVerified
          ? "pinned Lean replay executed and component identity matched"
          : "row-bound replay is performed by entryForProof when --replay is requested",
      };
      record.status = candidate.result !== "proved"
        ? "validated_nonproof"
        : replayVerified
          ? "validated_proved"
          : "pending_replay";
      record.structurally_valid = candidate.result === "proved";
      record.verified = replayVerified;
    } catch (error) {
      record.reason = String(error);
    }
    stageClaims.push(record);
  }
  const claimsByObligation = new Map();
  for (const stageClaim of stageClaims) {
    const obligationId = stageClaim.binding.obligation_id;
    const prior = claimsByObligation.get(obligationId);
    if (prior) {
      prior.verified = false;
      prior.status = "invalid";
      prior.reason = "multiple stage proof claims bind the same obligation";
      stageClaim.verified = false;
      stageClaim.status = "invalid";
      stageClaim.reason = "multiple stage proof claims bind the same obligation";
    } else {
      claimsByObligation.set(obligationId, stageClaim);
    }
  }
  return {
    schema: manifest.schema,
    schema_version: manifest.schema_version,
    manifest_path: contract.identity.manifest_path,
    toolchain_pin_sha256: claim.checker.toolchain_pin_sha256,
    claim_id: claim.id,
    authority_key: contract.identity.authority_key ?? null,
    authority_status: authority ? "available" : "unavailable",
    approved_authority: authority,
    identity_sha256: certificate.identity_sha256 ?? claim.identity_sha256 ?? identityDigest(claim),
    result: claim.result,
    certificate: {
      artifact: claim.proof.artifact,
      formal_goal: claim.proof.formal_goal,
      checker: claim.checker,
      validation: "compiler-proof.validateClaim consumed the recorded model, implementation, proof artifact, correspondence, candidate, and replay identities",
      status: claim.result === "proved" ? "validated_proved_claim" : "validated_nonproof_claim",
      applies_to_stage_semantics: false,
    },
    stage_claims: {
      count: stageClaims.length,
      items: stageClaims,
      invalid_count: stageClaims.filter((stageClaim) => stageClaim.status === "invalid").length,
      verified_count: stageClaims.filter((stageClaim) => stageClaim.verified).length,
    },
    negative_controls: STAGE_PROOF_NEGATIVE_CONTROLS.map((control) => ({
      ...control,
      observed_status: "unverified",
      qualification: "negative control is declared; no proof replay or execution is performed by this source stage",
    })),
    qualification: "validated pinned candidate certificate plus any exact obligation-specific certificates; unbound identity claims do not qualify stage obligations",
  };
}

export function checkLedger(ledger) {
  object(ledger, "Core surface ledger");
  if (ledger.schemaVersion !== 2) fail("invalid_oracle", "Core surface ledger schemaVersion is not 2");
  if (!Array.isArray(ledger.rows) || ledger.rows.length === 0) fail("invalid_oracle", "Core surface ledger has no rows");
  const stableLedger = { ...ledger };
  delete stableLedger.generatedOn;
  return {
    schema_version: ledger.schemaVersion,
    source_of_truth: LEDGER_PATH,
    row_count: ledger.rows.length,
    digest: digestText(canonicalJson(stableLedger)),
  };
}

export function stageResult({ stage, contract, witnesses, execute = false, replayProof = false }) {
  validateContract(contract, stage, `proof/compiler/${stage}/check.mjs`);
  const witnessInfo = validateWitnesses(witnesses, stage);
  const coreSource = readText(CORE_PATH, "Core dispatcher source");
  const rows = parseDispatcher(coreSource);
  const obligations = readJson(OBLIGATIONS_PATH, "compiler obligations");
  const identity = checkProofIdentity(contract, { replayProof });
  const stageProofs = new Map(
    identity.stage_claims.items
      .filter((stageClaim) => stageClaim.structurally_valid === true && stageClaim.status !== "invalid")
      .map((stageClaim) => [stageClaim.binding.obligation_id, stageClaim]),
  );
  const index = buildImplementationIndex();
  const coreEntries = rows
    .filter((row) => row.owner === "#2940")
    .map((row) => entryForProof(row, bindImplementation(row, index), stage, stageProofs.get(row.id), contract, replayProof));
  const boundaryEntries = obligations.rows
    .filter((row) => row.owner === "#2940" && row.family === "runtime" && row.kind === "boundary")
    .map((row) => boundaryEntry(row, stage, stageProofs.get(row.id), contract, replayProof));
  const entries = [...coreEntries, ...boundaryEntries];
  identity.stage_claims.verified_count = identity.stage_claims.items.filter((stageClaim) => stageClaim.verified === true).length;
  identity.stage_claims.invalid_count = identity.stage_claims.items.filter((stageClaim) => stageClaim.status === "invalid").length;
  const coverage = checkObligationCoverage(obligations, entries);
  const ledger = checkLedger(buildLedger());
  const dependencyClosure = checkDependencies(contract, `proof/compiler/${stage}/witnesses.json`);
  const allEntriesHaveSource = entries.every((entry) => entry.implementation.source_paths.length > 0);
  const allPoliciesPresent = entries.every((entry) => REQUIRED_POLICIES.every((policy) => entry.policies[policy]?.owner === "Jet"));
  const status = dependencyClosure.valid
    && identity.stage_claims.invalid_count === 0
    && coverage.complete
    && allEntriesHaveSource
    && allPoliciesPresent
    ? "blocked"
    : "unavailable";
  const globalAssumptions = contract.foreign_assumptions.map((assumption) => ({
    ...assumption,
    status: "assumed",
    universal_proof: false,
  }));
  return {
    schema: `jet.${stage}-contract-check.v1`,
    schema_version: 1,
    status,
    stage,
    contract: {
      id: contract.contract_id,
      schema: contract.schema,
      schema_version: contract.schema_version,
      claim: contract.claim.kind,
      certificates: contract.certificates,
    },
    identity,
    binding: {
      checker: `proof/compiler/${stage}/check.mjs`,
      driver: "scripts/agent/compiler-proof.mjs",
      authority_count: contract.dependencies.length,
      implementation_file_count: index.files.length,
      obligations_sha256: coverage.relation_sha256,
      ledger_sha256: ledger.digest,
      inventory_join: contract.inventory_join,
      obligation_dimensions: contract.obligation_dimensions,
      memory_premises: contract.memory_premises,
    },
    obligations: coverage,
    entries: {
      count: entries.length,
      items: entries,
      implementation_bound_count: entries.filter((entry) => entry.implementation.body_found).length,
      foreign_edge_count: entries.filter((entry) => entry.implementation.foreign_edge).length,
      proved_count: entries.filter((entry) => entry.disposition === "proved").length,
      unproved_count: entries.filter((entry) => entry.disposition === "mapped_unproved").length,
      proof_rejection_count: entries.filter((entry) => entry.proof_rejection).length,
    },
    witnesses: {
      count: witnessInfo.controls.length,
      negative_count: witnessInfo.controls.length,
      categories: witnessInfo.categories,
      reports: witnessInfo.controls.map((control) => ({
        id: control.id,
        category: control.category,
        expected_status: control.expected.status,
        observed_status: "unverified",
        qualification: "negative witness is retained; execution is required before mismatch evidence",
        universal_proof: false,
      })),
    },
    assumptions: { count: globalAssumptions.length, items: globalAssumptions },
    dependency_closure: {
      valid: dependencyClosure.valid,
      changed: dependencyClosure.changed,
      inputs: dependencyClosure.records,
      invalidation_policy: "body, premise, target, configuration, witness, or authority digest drift invalidates this stage; no stale binding is reused",
    },
    compiler_execution: {
      status: execute ? "unverified" : "unverified",
      requested: execute,
      runner: contract.execution.runner,
      compiler: contract.execution.compiler,
      timeout_ms: contract.execution.timeout_ms,
    },
    proof_boundary: {
      proven: contract.boundary.proven,
      assumed: contract.boundary.assumed,
      tested: contract.boundary.tested,
      blocked: contract.boundary.blocked,
      qualification: "blocked",
      universal_proof: false,
      reason: dependencyClosure.valid
        ? "implementation-bound source certificates and negative witness declarations are present; semantic observations remain unexecuted"
        : "dependency closure changed; all downstream qualification is invalidated",
    },
  };
}

export function errorPayload(stage, error) {
  const code = error instanceof CheckError ? error.code : "invalid_oracle";
  const status = STATUS_SET.has(code) ? code : "invalid_oracle";
  return {
    schema: `jet.${stage}-contract-check.v1`,
    schema_version: 1,
    status,
    stage,
    error: {
      code,
      message: error?.message ?? String(error),
      ...(error?.details === undefined ? {} : { details: error.details }),
    },
    proof_boundary: { qualification: "blocked", universal_proof: false },
  };
}

export function printHuman(result) {
  process.stdout.write(`${result.stage ?? "compiler stage"}: ${result.status}\n`);
  if (result.entries) process.stdout.write(`entries: ${result.entries.count}; implementation-bound=${result.entries.implementation_bound_count}\n`);
  if (result.obligations) process.stdout.write(`obligations: ${result.obligations.denominator}; qualification=${result.obligations.qualification}\n`);
  if (result.dependency_closure) process.stdout.write(`dependencies: ${result.dependency_closure.valid ? "unchanged" : "invalidated"}\n`);
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}

export async function runStage(stage, argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const execute = false;
  const replayProof = argv.includes("--replay");
  const allowed = new Set(["--json", "--skip-observation", "--skip-execution", "--replay"]);
  if (argv.some((arg) => !allowed.has(arg))) {
    const result = errorPayload(stage, new CheckError("invalid_oracle", `usage: ${stage}/check.mjs [--json] [--skip-observation] [--replay]`));
    if (json) process.stdout.write(`${canonicalJson(result)}\n`); else printHuman(result);
    return 64;
  }
  try {
    const contract = readJson(`proof/compiler/${stage}/contract.json`, `${stage} contract`);
    const witnesses = readJson(`proof/compiler/${stage}/witnesses.json`, `${stage} witnesses`);
    const result = stageResult({ stage, contract, witnesses, execute, replayProof });
    if (json) process.stdout.write(`${canonicalJson(result)}\n`); else printHuman(result);
    return 0;
  } catch (error) {
    const result = errorPayload(stage, error);
    if (json) process.stdout.write(`${canonicalJson(result)}\n`); else printHuman(result);
    if (result.status === "unavailable") return 3;
    if (result.status === "timeout") return 4;
    return 1;
  }
}
