#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalJson, sha256 } from "./hardening-repro.mjs";

export const SURFACE_SCHEMA = "jet.hardening.surface.v1";
export const SURFACE_SCHEMA_VERSION = 1;
export const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);
export const DEFAULT_MANIFEST_PATH = ".jet/hardening-manifest.json";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(SCRIPT_DIR, "../..");
const VALUE_NAMES = new Set(["pi", "e", "tau", "infinity", "nan"]);
const KIND_ORDER = Object.freeze(["module_call", "receiver_method", "field", "nominal_type"]);
const KIND_PREFIX = Object.freeze({
  module_call: "module:",
  receiver_method: "receiver:",
  field: "field:",
  nominal_type: "type:",
});
const PATHS = Object.freeze({
  moduleItems: "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
  moduleTypes: "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
  fields: "crates/jet-sema/src/Sema/CheckerCoreLib/core_types.rs",
  calls: "crates/jet-foundation/src/Syntax/core_calls.rs",
  fixedSigs: "crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs",
  surface: "crates/jet-foundation/src/Syntax/core_surface.rs",
  conformance: "scripts/agent/core-conformance.mjs",
  exclusions: "tests/conformance/exclusions.tsv",
});
const ROUTE_FILES = Object.freeze({
  aot: ["crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs", "crates/jet-codegen/src/Codegen/TIR/emit/helpers.rs"],
  jet_run: ["crates/jet-jit/src/jit/lower_ctx.rs", "crates/jet-jit/src/jit/runtime_host.rs", "crates/jet-jit/src/jit/types_meta.rs"],
  interpreter: ["crates/jet-jit/src/ambient_interp.rs", "crates/jet-jit/src/enc_stream/mod.rs", "crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs", "crates/jet-comptime/src/Comptime/CorePureParity.rs"],
});
const VALID_STATUSES = new Set(["covered", "missing", "unrouted", "excluded"]);
const SHA256_PATTERN = /^sha256:[0-9a-f]{64}$/;

function compareStable(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function uniqueSorted(values) {
  return [...new Set(values)].sort(compareStable);
}

function lineNumber(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function rowEvidence(path, source, offset, seam, stableId) {
  return `${path}:${lineNumber(source, offset)}:${seam}:${stableId}`;
}

function validDigest(value) {
  return typeof value === "string" && SHA256_PATTERN.test(value);
}

export function manifestContentDigest(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw new Error("manifest is not an object");
  }
  const content = { ...manifest };
  delete content.content_digest;
  return sha256(canonicalJson(content));
}

function sourceBytes(value) {
  return Buffer.isBuffer(value) || value instanceof Uint8Array
    ? Buffer.from(value)
    : Buffer.from(String(value), "utf8");
}

function fail(message) {
  throw new Error(message);
}

function decodeRustString(value) {
  try { return JSON.parse(`"${value}"`); } catch { return value; }
}

function quoted(text) {
  return Array.from(text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g), (match) => decodeRustString(match[1]));
}

function matching(text, start, opening, closing) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  for (let index = start; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"') { quote = '"'; continue; }
    if (char === opening) depth += 1;
    else if (char === closing && --depth === 0) return index;
  }
  fail(`unbalanced ${opening} at ${start}`);
}

function withoutComments(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let line = false;
  let block = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    const next = text[index + 1];
    if (line) {
      if (char === "\n") { line = false; out += char; } else out += char === "\r" ? char : " ";
      continue;
    }
    if (block) {
      if (char === "/" && next === "*") { block += 1; out += "  "; index += 1; }
      else if (char === "*" && next === "/") { block -= 1; out += "  "; index += 1; }
      else out += char === "\n" || char === "\r" ? char : " ";
      continue;
    }
    if (quote) {
      out += char;
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"') { quote = '"'; out += char; continue; }
    if (char === "/" && next === "/") { line = true; out += "  "; index += 1; continue; }
    if (char === "/" && next === "*") { block = 1; out += "  "; index += 1; continue; }
    out += char;
  }
  return out;
}

function codeOnly(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let line = false;
  let block = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    const next = text[index + 1];
    if (line) {
      if (char === "\n") { line = false; out += char; } else out += char === "\r" ? char : " ";
      continue;
    }
    if (block) {
      if (char === "/" && next === "*") { block += 1; out += "  "; index += 1; }
      else if (char === "*" && next === "/") { block -= 1; out += "  "; index += 1; }
      else out += char === "\n" || char === "\r" ? char : " ";
      continue;
    }
    if (quote) {
      out += char === "\n" || char === "\r" ? char : " ";
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"') { quote = '"'; out += " "; continue; }
    if (char === "/" && next === "/") { line = true; out += "  "; index += 1; continue; }
    if (char === "/" && next === "*") { block = 1; out += "  "; index += 1; continue; }
    out += char;
  }
  return out;
}

function rustStringConstants(source) {
  const out = new Map();
  const clean = withoutComments(source);
  for (const match of clean.matchAll(/pub\s+const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
    out.set(match[1], decodeRustString(match[2]));
  }
  return out;
}

function rustStrings(text, constants) {
  const out = new Set(quoted(text));
  for (const match of text.matchAll(/\b(?:Syntax::)?([A-Z][A-Z0-9_]*)\b/g)) {
    if (constants.has(match[1])) out.add(constants.get(match[1]));
  }
  return [...out];
}

function splitTopLevel(text) {
  const parts = [];
  let start = 0;
  let round = 0;
  let square = 0;
  let curly = 0;
  let quote = false;
  let escaped = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quote = false;
      continue;
    }
    if (char === '"') { quote = true; continue; }
    if (char === "(") round += 1;
    else if (char === ")") round -= 1;
    else if (char === "[") square += 1;
    else if (char === "]") square -= 1;
    else if (char === "{") curly += 1;
    else if (char === "}") curly -= 1;
    else if (char === "," && round === 0 && square === 0 && curly === 0) {
      parts.push(text.slice(start, index).trim());
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts;
}

function calls(source, needle) {
  const clean = withoutComments(source);
  const out = [];
  let cursor = 0;
  while (true) {
    const start = clean.indexOf(needle, cursor);
    if (start < 0) break;
    const open = start + needle.length - 1;
    const close = matching(clean, open, "(", ")");
    out.push({ start, args: splitTopLevel(clean.slice(open + 1, close)) });
    cursor = close + 1;
  }
  return out;
}

function moduleItemsFromSource(moduleItemsSource, surfaceSource) {
  const start = moduleItemsSource.indexOf("pub fn core_module_items");
  const end = moduleItemsSource.indexOf("/// Ratified nominal types", start);
  if (start < 0 || end < 0) fail("core module item source anchors disappeared");
  const body = withoutComments(moduleItemsSource.slice(start, end));
  const constants = rustStringConstants(surfaceSource);
  const modules = new Map();
  const arms = /^\s*((?:"[^"]+"\s*(?:\|\s*)?)+)=>\s*&\[/gm;
  for (const arm of body.matchAll(arms)) {
    const opening = body.indexOf("[", arm.index + arm[0].length - 1);
    const close = matching(body, opening, "[", "]");
    const names = rustStrings(body.slice(opening + 1, close), constants);
    for (const module of rustStrings(arm[1], constants)) {
      if (!modules.has(module)) modules.set(module, new Set());
      for (const name of names) modules.get(module).add(name);
    }
  }
  if (body.includes('module == "core.compiler.lang"')) modules.set("core.compiler.lang", new Set());
  const memStart = surfaceSource.indexOf("pub const CORE_MEM_GATE_TIERS");
  const memTable = surfaceSource.indexOf("= &[", memStart);
  if (memStart < 0 || memTable < 0) fail("CORE_MEM_GATE_TIERS source anchor disappeared");
  const memClean = withoutComments(surfaceSource);
  const memOpening = memClean.indexOf("[", memClean.indexOf("= &[", memClean.indexOf("pub const CORE_MEM_GATE_TIERS")));
  const memClose = matching(memClean, memOpening, "[", "]");
  const memNames = rustStrings(memClean.slice(memOpening + 1, memClose), rustStringConstants(surfaceSource));
  if (memNames.length === 0) fail("CORE_MEM_GATE_TIERS yielded no names");
  modules.set("core.mem", new Set(memNames));
  return modules;
}

function explicitTypes(moduleItemsSource) {
  const start = moduleItemsSource.indexOf("pub(crate) fn core_module_type_item");
  if (start < 0) return new Map();
  const body = withoutComments(moduleItemsSource.slice(start));
  const out = new Map();
  const pair = /\(\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*\)/g;
  for (const match of body.matchAll(pair)) {
    const modules = quoted(match[1]);
    const names = quoted(match[2]);
    for (const module of modules) {
      if (!out.has(module)) out.set(module, new Set());
      for (const name of names) out.get(module).add(name);
    }
  }
  return out;
}

function moduleItemEvidence(source, module, member, stableId) {
  const clean = withoutComments(source);
  const moduleIndex = clean.indexOf(`"${module}"`);
  const memberIndex = clean.indexOf(`"${member}"`, Math.max(0, moduleIndex));
  const offset = memberIndex >= 0 ? memberIndex : moduleIndex >= 0 ? moduleIndex : 0;
  return [rowEvidence(PATHS.moduleItems, source, offset, "core-module-registry", stableId)];
}

function typeMembershipEvidence(source, stableId) {
  const untagged = untaggedId(stableId);
  const dot = untagged.lastIndexOf(".");
  const module = dot < 0 ? "" : untagged.slice(0, dot);
  const type = dot < 0 ? untagged : untagged.slice(dot + 1);
  return moduleItemEvidence(source, module, type, stableId);
}

function parseCoreCallRegistry(callsSource, path = PATHS.calls) {
  const events = [
    ...calls(callsSource, "CoreCallRecord::new(").map((call) => ({ ...call, kind: "module_call" })),
    ...calls(callsSource, "CoreCallRecord::receiver(").map((call) => ({ ...call, kind: "receiver_method" })),
  ].sort((left, right) => left.start - right.start);
  const modules = new Map();
  const receivers = new Map();
  for (const [index, event] of events.entries()) {
    const block = codeOnly(callsSource.slice(event.start, events[index + 1]?.start ?? callsSource.length));
    if (event.kind === "module_call") {
      const module = quoted(event.args[0] || "")[0];
      const member = quoted(event.args[1] || "")[0];
      if (!module || !member || !module.startsWith("core.")) continue;
      const stable_id = `module:${module}.${member}`;
      modules.set(stable_id, {
        stable_id,
        module,
        member,
        aot_direct: !/\.without_direct_aot\s*\(\s*\)/.test(block),
        jit_direct: !/\.without_direct_jit\s*\(\s*\)/.test(block),
        interpreter_explicit: /\.with_(?:pure_route|interpreter_route)\s*\(/.test(block),
        evidence: [rowEvidence(path, callsSource, event.start, "CoreCallRecord::new", stable_id)],
      });
      continue;
    }
    const types = quoted(event.args[0] || "");
    const member = quoted(event.args[1] || "")[0];
    if (!member || types.length === 0) continue;
    for (const type of types) {
      const stable_id = `receiver:${type}.${member}`;
      receivers.set(stable_id, {
        stable_id,
        type,
        member,
        evidence: [rowEvidence(path, callsSource, event.start, "CoreCallRecord::receiver", stable_id)],
      });
    }
  }
  return { modules, receivers };
}

function parseReceiverRows(callsSource) {
  return [...parseCoreCallRegistry(callsSource).receivers.values()]
    .sort((left, right) => compareStable(left.stable_id, right.stable_id));
}

function parsePlainCallRows(callsSource) {
  return [...parseCoreCallRegistry(callsSource).modules.values()]
    .map((row) => row.stable_id)
    .sort(compareStable);
}

function parsePairLiterals(source) {
  const clean = withoutComments(source);
  const out = [];
  const pattern = /\(\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*\)/g;
  for (const match of clean.matchAll(pattern)) {
    out.push({
      first: decodeRustString(match[1]),
      second: decodeRustString(match[2]),
      start: match.index,
    });
  }
  return out;
}

function parseFieldRows(typesSource, surfaceSource) {
  const clean = withoutComments(typesSource);
  const evidence = new Map();
  const add = (type, field, offset = 0, path = PATHS.fields, seam = "core-field-registry") => {
    if (!/^[A-Z][A-Za-z0-9_]*$/.test(type) || !/^[a-z_][A-Za-z0-9_]*$/.test(field)) return;
    const stable = `${type}.${field}`;
    if (!evidence.has(stable)) evidence.set(stable, []);
    evidence.get(stable).push(rowEvidence(
      path,
      path === PATHS.fields ? typesSource : surfaceSource,
      offset,
      seam,
      `field:${stable}`,
    ));
  };
  const pair = /\(\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*\)\s*=>/g;
  for (const match of clean.matchAll(pair)) {
    for (const type of quoted(match[1])) for (const field of quoted(match[2])) add(type, field, match.index);
  }
  for (const fieldMatch of clean.matchAll(/match\s+field\s*\{/g)) {
    const opening = clean.indexOf("{", fieldMatch.index + fieldMatch[0].length - 1);
    let close;
    try { close = matching(clean, opening, "{", "}"); } catch { continue; }
    const context = clean.slice(Math.max(0, fieldMatch.index - 2200), fieldMatch.index);
    const types = new Set();
    for (const match of context.matchAll(/(?:type_name|name)\s*==\s*"([A-Z][A-Za-z0-9_]*)"/g)) types.add(match[1]);
    for (const match of context.matchAll(/matches!\(\s*(?:type_name|name)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\)/g)) {
      for (const type of quoted(match[1])) types.add(type);
    }
    const body = clean.slice(opening + 1, close);
    const fields = [];
    for (const match of body.matchAll(/"([a-z_][A-Za-z0-9_]*)"\s*(?:\|\s*)*(?==>)/g)) {
      fields.push({ name: match[1], offset: opening + 1 + match.index });
    }
    for (const type of types) for (const field of fields) add(type, field.name, field.offset);
  }
  const constructable = clean.indexOf("pub(crate) fn core_constructable_fields");
  if (constructable >= 0) {
    const body = clean.slice(constructable);
    for (const match of body.matchAll(/"([A-Z][A-Za-z0-9_]*)"\s*=>\s*Some\s*\(\s*vec!\s*\[/g)) {
      const opening = body.indexOf("[", match.index + match[0].length - 1);
      let close;
      try { close = matching(body, opening, "[", "]"); } catch { continue; }
      for (const field of body.slice(opening + 1, close).matchAll(/\(\s*"([a-z_][A-Za-z0-9_]*)"/g)) {
        add(match[1], field[1], constructable + opening + 1 + field.index, PATHS.fields, "core-constructable-field");
      }
    }
  }
  // A field table can use Syntax string constants for a reserved type name.
  const constants = rustStringConstants(surfaceSource);
  for (const [name, value] of constants) {
    if (!/^[A-Z][A-Za-z0-9_]*$/.test(value)) continue;
    const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    for (const match of clean.matchAll(new RegExp(`Syntax::${escaped}\\s*[,)]`, "g"))) {
      const context = clean.slice(Math.max(0, match.index - 400), match.index + 400);
      const fields = quoted(context).filter((field) => /^[a-z_][A-Za-z0-9_]*$/.test(field));
      for (const field of fields) add(value, field);
    }
  }
  return [...evidence.keys()].sort(compareStable).map((stable) => {
    const dot = stable.lastIndexOf(".");
    return {
      stable_id: `field:${stable}`,
      type: stable.slice(0, dot),
      field: stable.slice(dot + 1),
      evidence: uniqueSorted(evidence.get(stable)),
    };
  });
}

function coreConformanceInventory(root, fallback) {
  const script = join(root, "scripts/agent/core-conformance.mjs");
  if (!existsSync(script)) return fallback;
  const result = spawnSync(process.execPath, [script, "--inventory"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.status !== 0) fail(`core conformance inventory failed: ${result.stderr.trim()}`);
  try {
    const parsed = JSON.parse(result.stdout);
    if (!Array.isArray(parsed)) fail("core conformance inventory is not an array");
    return parsed;
  } catch (error) {
    fail(`core conformance inventory is unreadable: ${error.message}`);
  }
}

function walk(root) {
  if (!existsSync(root)) return [];
  const out = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else if (entry.isFile() && entry.name.endsWith(".jet")) out.push(path);
  }
  return out.sort();
}

function seedKey(corpusRoot, path) {
  const rel = relative(corpusRoot, path).replaceAll("\\", "/");
  const parts = rel.split("/");
  const name = parts.pop().replace(/\.jet$/, "");
  return parts.length ? `${parts.join(".")}.${name}` : name;
}

function observerCalls(code) {
  const out = [];
  for (const match of code.matchAll(/(?<![A-Za-z0-9_.])(?:print|eprint|assert)\s*\(/g)) {
    const open = match.index + match[0].lastIndexOf("(");
    try { out.push({ operation: match[0].trim().slice(0, -1), open, close: matching(code, open, "(", ")") }); } catch { /* invalid sink */ }
  }
  return out;
}

function likelyOpaque(module, member) {
  return /(?:^|\.)(open|connect|listen|accept|bind|stdin|stdout|stderr|reader|writer|session|socket|request|response|handle|lock|watch|spawn|transaction|cursor|stream|server|client)$/.test(`${module}.${member}`);
}

function seedInspection(key, source) {
  const errors = [];
  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const member = key.slice(dot + 1);
  const expectedMarker = `// core-conformance: ${key}`;
  if (source.split(/\r?\n/, 1)[0] !== expectedMarker) errors.push(`missing exact marker ${expectedMarker}`);
  const code = codeOnly(source);
  const aliasMatches = [...code.matchAll(new RegExp(`^\\s*use\\s+${module.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)`, "gm"))];
  if (aliasMatches.length !== 1) return { errors: [...errors, `expected one use for ${module}`], sink: null };
  const alias = aliasMatches[0][1];
  const callMatches = [...code.matchAll(new RegExp(`(?<![A-Za-z0-9_.])${alias}\\s*\\.\\s*${member}\\s*(?:<[^{}]*>\\s*)?\\(`, "g"))];
  if (callMatches.length !== 1) return { errors: [...errors, `expected one ${key} call`], sink: null };
  const callStart = callMatches[0].index;
  const callOpen = callStart + callMatches[0][0].lastIndexOf("(");
  let callClose;
  try { callClose = matching(code, callOpen, "(", ")"); } catch { return { errors: [...errors, `unbalanced ${key} call`], sink: null }; }
  const lineStart = code.lastIndexOf("\n", callStart) + 1;
  const binding = code.slice(lineStart, callStart).match(/(?:^|[{};])\s*(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*$/)?.[1] || null;
  const observers = observerCalls(code);
  if (!binding) {
    const observer = observers.find(({ open, close }) => open < callStart && callStart < close);
    if (!observer) errors.push("direct result has no observable sink");
    if (likelyOpaque(module, member)) errors.push("opaque result needs a follow-up operation before observation");
    return {
      errors,
      sink: observer ? { kind: "call-result", operation: observer.operation, type_aware: true, observed_type: "return-value" } : null,
    };
  }
  if (binding.startsWith("_")) errors.push(`result is bound to discard name ${binding}`);
  const use = new RegExp(`(?<![A-Za-z0-9_])${binding}(?![A-Za-z0-9_])`);
  const observer = observers.find(({ open, close }) => open > callClose && use.test(code.slice(open + 1, close)));
  if (!observer) {
    errors.push(`bound result ${binding} is never consumed by print/eprint/assert`);
    return { errors, sink: null };
  }
  const expression = code.slice(observer.open + 1, observer.close);
  const followUp = expression.match(new RegExp(`${binding}\\s*\\.\\s*([A-Za-z_][A-Za-z0-9_]*)`))?.[1] || null;
  if (likelyOpaque(module, member) && !followUp) errors.push("opaque result needs a follow-up operation before observation");
  return {
    errors,
    sink: {
      kind: followUp ? "follow-up" : "primitive",
      operation: observer.operation,
      binding,
      follow_up: followUp,
      type_aware: true,
      observed_type: followUp ? "derived-primitive" : "declared-return-value",
    },
  };
}

function parseExclusions(root) {
  const path = join(root, PATHS.exclusions);
  if (!existsSync(path)) return new Map();
  const out = new Map();
  for (const [index, raw] of readFileSync(path, "utf8").split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const fields = raw.split("\t");
    if (fields.length < 2 || !fields[0].trim() || !fields[1].trim()) fail(`malformed exclusion at line ${index + 1}`);
    const key = fields[0].trim();
    if (out.has(key)) fail(`duplicate exclusion: ${key}`);
    out.set(key, { reason: fields[1].trim(), owner: fields[2]?.trim() || null, decision: fields[3]?.trim() || null });
  }
  return out;
}

function normalizeId(kind, value) {
  if (typeof value === "string") {
    if (value.startsWith(`${KIND_PREFIX[kind]}`)) return value;
    return `${KIND_PREFIX[kind]}${value}`;
  }
  if (value && typeof value.stable_id === "string") return normalizeId(kind, value.stable_id);
  if (kind === "receiver_method" && value?.type && value?.member) return `receiver:${value.type}.${value.member}`;
  if (kind === "field" && value?.type && value?.field) return `field:${value.type}.${value.field}`;
  fail(`cannot derive ${kind} stable id`);
}

function untaggedId(id) {
  return id.replace(/^(module|receiver|field|type):/, "");
}

function asSet(values, kind) {
  return new Set((values || []).map((value) => normalizeId(kind, value)));
}

function routeIdentity(id) {
  return id.startsWith("module:") ? id.slice("module:".length) : id;
}

function routeSourceEvidence(root, paths, pattern, seam, stableId) {
  for (const path of paths) {
    const absolute = join(root, path);
    if (!existsSync(absolute)) continue;
    const source = readFileSync(absolute, "utf8");
    const match = codeOnly(source).match(pattern);
    if (match) return [rowEvidence(path, source, match.index, seam, stableId)];
  }
  return [];
}

function routeFactsFromSources(
  root,
  moduleIds,
  receiverRows,
  fieldRows,
  typeRows,
  plainRows,
  registry = null,
) {
  const expectedModules = new Set(moduleIds.map(routeIdentity));
  const actual = Object.fromEntries(TIERS.map((tier) => [tier, []]));
  const add = (tier, stableId, route, evidence, seam) => {
    const proof = uniqueSorted(evidence.filter(Boolean));
    if (proof.length === 0) return;
    const rows = actual[tier];
    const existing = rows.find((row) => row.stable_id === stableId && row.route === route);
    if (existing) {
      existing.evidence = uniqueSorted([...existing.evidence, ...proof]);
      return;
    }
    rows.push({ stable_id: stableId, route, seam, evidence: proof });
  };

  const callsSource = registry?.source || (existsSync(join(root, PATHS.calls)) ? readFileSync(join(root, PATHS.calls), "utf8") : "");
  const callRegistry = registry?.rows || parseCoreCallRegistry(callsSource);
  const moduleRegistry = callRegistry.modules || new Map();
  const receiverRegistry = callRegistry.receivers || new Map();

  const genericLookups = {
    aot: routeSourceEvidence(root, ROUTE_FILES.aot, /(?:crate::)?Syntax::core_call\s*\(\s*module\s*,\s*method\s*\)/, "aot-core-call-dispatch", "__route__"),
    jet_run: routeSourceEvidence(root, ROUTE_FILES.jet_run, /jet_foundation::Syntax::core_call\s*\(\s*module\s*,\s*method\s*\)/, "jet-run-core-call-dispatch", "__route__"),
    interpreter: routeSourceEvidence(root, ROUTE_FILES.interpreter, /jet_foundation::Syntax::core_call\s*\(\s*module\s*,\s*method\s*\)/, "interpreter-core-call-dispatch", "__route__"),
  };
  const routeSupports = {
    aot: (entry) => entry?.aot_direct !== false,
    jet_run: (entry) => entry?.jit_direct !== false,
    interpreter: (entry) => entry?.interpreter_explicit === true,
  };

  for (const row of moduleRegistry.values()) {
    if (!expectedModules.has(row.stable_id.slice("module:".length))) continue;
    for (const tier of TIERS) {
      if (!routeSupports[tier](row)) continue;
      const lookup = genericLookups[tier];
      if (lookup.length === 0) continue;
      add(
        tier,
        row.stable_id,
        `${tier}:canonical-core-call-lookup`,
        [...row.evidence, ...lookup.map((item) => item.replace("__route__", row.stable_id))],
        "core-call-registry",
      );
    }
  }

  // Hand-written literal arms remain valid, but only when the exact pair is
  // present in the registry. The row-specific registry proof prevents an
  // unrelated pair in a route file from becoming blanket coverage.
  for (const tier of TIERS) {
    for (const path of ROUTE_FILES[tier]) {
      const absolute = join(root, path);
      if (!existsSync(absolute)) continue;
      const source = readFileSync(absolute, "utf8");
      for (const pair of parsePairLiterals(source)) {
        const stableId = `module:${pair.first}.${pair.second}`;
        const entry = moduleRegistry.get(stableId);
        if (!expectedModules.has(`${pair.first}.${pair.second}`) || !entry) continue;
        add(
          tier,
          stableId,
          `${tier}:literal-dispatch`,
          [
            ...entry.evidence,
            rowEvidence(path, source, pair.start, "literal-dispatch", stableId),
          ],
          "core-call-registry",
        );
      }
    }
  }

  const receiverLookup = routeSourceEvidence(
    root,
    ROUTE_FILES.interpreter,
    /core_receiver_method\s*\(/,
    "interpreter-receiver-dispatch",
    "__route__",
  );
  if (receiverLookup.length > 0) {
    for (const receiver of receiverRows) {
      const entry = receiverRegistry.get(receiver.stable_id);
      if (!entry) continue;
      add(
        "interpreter",
        receiver.stable_id,
        "interpreter:canonical-receiver-lookup",
        [...(receiver.evidence || entry.evidence || []), ...receiverLookup.map((item) => item.replace("__route__", receiver.stable_id))],
        "core-receiver-registry",
      );
    }
  }

  const shapePatterns = {
    aot: /core_struct_field_rust_name\s*\(/,
    jet_run: /core_struct_field_(?:type|names|index|layout)\s*\(/,
    interpreter: /TExprKind::Field|struct_field_types/,
  };
  for (const tier of TIERS) {
    const shapeEvidence = routeSourceEvidence(root, ROUTE_FILES[tier], shapePatterns[tier], `${tier}-core-shape-dispatch`, "__route__");
    if (shapeEvidence.length === 0) continue;
    for (const row of fieldRows) {
      add(
        tier,
        row.stable_id,
        `${tier}:canonical-field-shape`,
        [...(row.evidence || []), ...shapeEvidence.map((item) => item.replace("__route__", row.stable_id))],
        "core-field-registry",
      );
    }
    for (const row of typeRows) {
      add(
        tier,
        row.stable_id,
        `${tier}:canonical-type-shape`,
        [...(row.evidence || []), ...shapeEvidence.map((item) => item.replace("__route__", row.stable_id))],
        "core-type-registry",
      );
    }
  }
  for (const rows of Object.values(actual)) {
    rows.sort((left, right) => compareStable(left.stable_id, right.stable_id) || compareStable(left.route, right.route));
  }
  return actual;
}

function sourceFiles(root) {
  const files = new Set(Object.values(PATHS).map((path) => path));
  for (const paths of Object.values(ROUTE_FILES)) for (const path of paths) files.add(path);
  const corpus = join(root, "tests/conformance/corpus");
  for (const path of walk(corpus)) files.add(relative(root, path).replaceAll("\\", "/"));
  return [...files].sort(compareStable);
}

export function sourceSnapshotFromContents(entries) {
  const files = Object.entries(entries).map(([path, contents]) => ({
    path,
    bytes: sourceBytes(contents).length,
    sha256: sha256(contents),
  })).sort((left, right) => compareStable(left.path, right.path));
  return { algorithm: "sha256", files, hash: sha256(canonicalJson(files)) };
}

export function sourceSnapshot(root = DEFAULT_ROOT, files = sourceFiles(root)) {
  const entries = {};
  for (const path of files) {
    const absolute = join(root, path);
    if (!existsSync(absolute)) fail(`manifest source is unreadable: ${path}`);
    entries[path] = readFileSync(absolute);
  }
  return sourceSnapshotFromContents(entries);
}

function defaultSurface(root) {
  const moduleItemsSource = readFileSync(join(root, PATHS.moduleItems), "utf8");
  const surfaceSource = readFileSync(join(root, PATHS.surface), "utf8");
  const modules = moduleItemsFromSource(moduleItemsSource, surfaceSource);
  const fallbackCalls = [];
  const moduleNames = new Set(modules.keys());
  for (const [module, names] of modules) for (const name of names) {
    if (!/^[A-Z]/.test(name) && !VALUE_NAMES.has(name) && !moduleNames.has(`${module}.${name}`)) fallbackCalls.push(`${module}.${name}`);
  }
  const moduleCalls = coreConformanceInventory(root, [...new Set(fallbackCalls)].sort()).map((value) => `module:${value}`);
  const explicit = explicitTypes(moduleItemsSource);
  const typeSet = new Set();
  for (const [module, names] of modules) for (const name of names) if (/^[A-Z]/.test(name)) typeSet.add(`type:${module}.${name}`);
  for (const [module, names] of explicit) for (const name of names) typeSet.add(`type:${module}.${name}`);
  const callsSource = readFileSync(join(root, PATHS.calls), "utf8");
  const registry = { source: callsSource, rows: parseCoreCallRegistry(callsSource) };
  const receiverRows = parseReceiverRows(callsSource);
  const fieldRows = parseFieldRows(readFileSync(join(root, PATHS.fields), "utf8"), surfaceSource);
  const plainRows = parsePlainCallRows(callsSource);
  const typeRows = [...typeSet].sort(compareStable).map((stable_id) => ({
    stable_id,
    evidence: typeMembershipEvidence(moduleItemsSource, stable_id),
  }));
  const routes = routeFactsFromSources(root, moduleCalls, receiverRows, fieldRows, typeRows, plainRows, registry);
  const seeds = new Map();
  const corpusRoot = join(root, "tests/conformance/corpus");
  for (const path of walk(corpusRoot)) {
    const key = seedKey(corpusRoot, path);
    const stableId = `module:${key}`;
    if (!moduleCalls.includes(stableId)) continue;
    if (seeds.has(stableId)) fail(`duplicate conformance seed: ${stableId}`);
    const source = readFileSync(path, "utf8");
    seeds.set(stableId, { path: relative(root, path).replaceAll("\\", "/"), source, ...seedInspection(key, source) });
  }
  const exclusions = parseExclusions(root);
  return {
    moduleCalls,
    receivers: receiverRows,
    fields: fieldRows,
    types: [...typeSet].sort().map((stable_id) => ({ stable_id })),
    routes,
    seeds,
    exclusions,
    snapshot: sourceSnapshot(root),
    membershipSources: {
      module_call: [PATHS.moduleItems, PATHS.conformance],
      receiver_method: [PATHS.calls],
      field: [PATHS.fields],
      nominal_type: [PATHS.moduleTypes],
    },
    membershipEvidence: {
      module_call: Object.fromEntries(moduleCalls.map((stable_id) => [
        stable_id,
        moduleItemEvidence(moduleItemsSource, untaggedId(stable_id).slice(0, untaggedId(stable_id).lastIndexOf(".")), untaggedId(stable_id).slice(untaggedId(stable_id).lastIndexOf(".") + 1), stable_id),
      ])),
      receiver_method: Object.fromEntries(receiverRows.map((row) => [row.stable_id, row.evidence || []])),
      field: Object.fromEntries(fieldRows.map((row) => [row.stable_id, row.evidence || []])),
      nominal_type: Object.fromEntries(typeRows.map((row) => [row.stable_id, row.evidence || []])),
    },
  };
}

function normalizeRoutes(routes = {}) {
  const out = Object.fromEntries(TIERS.map((tier) => [tier, []]));
  for (const tier of TIERS) {
    for (const value of routes[tier] || []) {
      if (typeof value === "string") {
        out[tier].push({ stable_id: value.includes(":") ? value : `module:${value}`, route: `${tier}:fixture`, seam: null, evidence: [] });
      } else {
        out[tier].push({
          stable_id: value.stable_id,
          route: value.route || `${tier}:fixture`,
          seam: value.seam || null,
          evidence: uniqueSorted([...(value.evidence || [])]),
        });
      }
    }
    out[tier].sort((left, right) => compareStable(left.stable_id, right.stable_id) || compareStable(left.route, right.route));
  }
  return out;
}

function normalizedExclusions(exclusions = new Map()) {
  const out = new Map();
  const entries = exclusions instanceof Map ? exclusions.entries() : Object.entries(exclusions);
  for (const [key, value] of entries) {
    const stable = key.includes(":") ? key : `module:${key}`;
    out.set(stable, typeof value === "string" ? { reason: value, owner: null, decision: null } : { ...value });
  }
  return out;
}

function rowIdentity(kind, value) {
  const stable_id = normalizeId(kind, value);
  const untagged = untaggedId(stable_id);
  const dot = untagged.lastIndexOf(".");
  const owner = dot < 0 ? untagged : untagged.slice(0, dot);
  const member = dot < 0 ? untagged : untagged.slice(dot + 1);
  return { stable_id, owner, member };
}

function rowDomain(row) {
  const value = row.owner || row.member;
  const bits = value.split(".");
  return bits.length > 1 ? bits[1] : "core";
}

function buildRow(kind, value, surface, routes, exclusions) {
  const identity = rowIdentity(kind, value);
  const routeRows = [];
  for (const tier of TIERS) {
    for (const route of routes[tier]) {
      if (route.stable_id !== identity.stable_id) continue;
      routeRows.push({ tier, route: route.route, seam: route.seam, evidence: route.evidence });
    }
  }
  const applicable = [...new Set(routeRows.map((route) => route.tier))].sort((left, right) => TIERS.indexOf(left) - TIERS.indexOf(right));
  const exclusion = exclusions.get(identity.stable_id) || exclusions.get(untaggedId(identity.stable_id)) || null;
  const seed = surface.seeds?.get(identity.stable_id) || surface.seeds?.[identity.stable_id] || null;
  const executable = Boolean(seed);
  const invalid = seed && seed.errors?.length > 0;
  let status = exclusion ? "excluded" : invalid ? "invalid" : executable ? "covered" : applicable.length ? "missing" : "unrouted";
  if (exclusion && (!exclusion.reason || !exclusion.owner || !exclusion.decision)) status = "invalid-exclusion";
  const row = {
    stable_id: identity.stable_id,
    kind,
    owner: identity.owner,
    member: identity.member,
    domain: rowDomain(identity),
    applicable_tiers: applicable,
    projections: routeRows.sort((left, right) => TIERS.indexOf(left.tier) - TIERS.indexOf(right.tier) || compareStable(left.route, right.route)),
    dispatcher_arms: routeRows.map((route) => route.route),
    membership_sources: [...(surface.membershipSources?.[kind] || [])].sort(compareStable),
    membership_evidence: uniqueSorted(surface.membershipEvidence?.[kind]?.[identity.stable_id] || value.evidence || []),
    seed: seed?.path || null,
    value_consuming: exclusion || !executable ? null : !invalid,
    sink: exclusion || !executable ? null : seed.sink,
    status,
    exclusion: exclusion ? { ...exclusion } : null,
  };
  if (invalid) row.errors = [...seed.errors];
  return row;
}

export function buildManifest({ root = DEFAULT_ROOT, surface = null } = {}) {
  const actual = surface || defaultSurface(root);
  const routes = normalizeRoutes(actual.routes);
  const exclusions = normalizedExclusions(actual.exclusions);
  const members = {
    module_call: [...(actual.moduleCalls || [])].map((value) => normalizeId("module_call", value)),
    receiver_method: [...(actual.receivers || [])].map((value) => normalizeId("receiver_method", value)),
    field: [...(actual.fields || [])].map((value) => normalizeId("field", value)),
    nominal_type: [...(actual.types || [])].map((value) => normalizeId("nominal_type", value)),
  };
  for (const kind of KIND_ORDER) members[kind] = [...new Set(members[kind])].sort();
  const rows = KIND_ORDER.flatMap((kind) => members[kind].map((value) => buildRow(kind, value, actual, routes, exclusions)))
    .sort((left, right) => compareStable(left.stable_id, right.stable_id));
  const manifest = {
    schema: SURFACE_SCHEMA,
    schema_version: SURFACE_SCHEMA_VERSION,
    source_snapshot: actual.snapshot || sourceSnapshot(root),
    denominator: {
      source_ids: members,
      counts: {
        ...Object.fromEntries(KIND_ORDER.map((kind) => [kind, members[kind].length])),
        exclusions: exclusions.size,
      },
    },
    actual_routes: routes,
    exclusions: [...exclusions.entries()].sort(([left], [right]) => compareStable(left, right)).map(([stable_id, value]) => ({ stable_id, ...value })),
    rows,
  };
  manifest.content_digest = manifestContentDigest(manifest);
  const validation = validateManifest(manifest);
  if (!validation.ok) fail(validation.errors.join("\n"));
  return manifest;
}

function expectedRouteMap(manifest) {
  const out = Object.fromEntries(TIERS.map((tier) => [tier, new Map()]));
  for (const tier of TIERS) for (const route of Array.isArray(manifest.actual_routes?.[tier]) ? manifest.actual_routes[tier] : []) {
    if (!isRecord(route) || typeof route.stable_id !== "string" || typeof route.route !== "string") continue;
    const key = `${route.stable_id}\u0000${route.route}`;
    if (out[tier].has(key)) return { error: `duplicate actual route fact: ${tier}:${route.stable_id}:${route.route}` };
    out[tier].set(key, route);
  }
  return out;
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validateSourceSnapshot(snapshot, errors) {
  if (!isRecord(snapshot)) {
    errors.push("manifest source snapshot is required");
    return;
  }
  if (snapshot.algorithm !== "sha256") errors.push("manifest source snapshot algorithm must be sha256");
  if (!Array.isArray(snapshot.files)) {
    errors.push("manifest source snapshot files are required");
    return;
  }
  let previousPath = null;
  const paths = new Set();
  for (const file of snapshot.files) {
    if (!isRecord(file)) {
      errors.push("manifest source snapshot has an invalid file");
      continue;
    }
    if (typeof file.path !== "string" || file.path.length === 0) errors.push("manifest source snapshot file path is required");
    else {
      if (paths.has(file.path)) errors.push(`duplicate manifest source snapshot file: ${file.path}`);
      paths.add(file.path);
      if (previousPath !== null && compareStable(previousPath, file.path) > 0) errors.push("manifest source snapshot files are not sorted");
      previousPath = file.path;
    }
    if (!Number.isInteger(file.bytes) || file.bytes < 0) errors.push(`invalid manifest source snapshot byte count: ${file.path || "?"}`);
    if (!validDigest(file.sha256)) errors.push(`invalid manifest source snapshot digest: ${file.path || "?"}`);
  }
  if (!validDigest(snapshot.hash)) errors.push("manifest source snapshot hash is missing or invalid");
  else if (snapshot.hash !== sha256(canonicalJson(snapshot.files))) errors.push("manifest source snapshot hash does not match files");
}

export function validateManifest(manifest, { expectedIds = null, currentSnapshotHash = null } = {}) {
  const errors = [];
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) return { ok: false, errors: ["manifest is not an object"] };
  const expectedContentDigest = manifestContentDigest(manifest);
  if (manifest.schema !== SURFACE_SCHEMA) errors.push(`manifest schema must be ${SURFACE_SCHEMA}`);
  if (manifest.schema_version !== SURFACE_SCHEMA_VERSION) errors.push(`manifest schema_version must be ${SURFACE_SCHEMA_VERSION}`);
  if (!validDigest(manifest.content_digest)) errors.push("manifest content digest is missing or invalid");
  else if (manifest.content_digest !== expectedContentDigest) errors.push("manifest content digest does not match manifest");
  validateSourceSnapshot(manifest.source_snapshot, errors);

  const rows = Array.isArray(manifest.rows) ? manifest.rows : [];
  if (!Array.isArray(manifest.rows)) errors.push("manifest rows are required");
  const seen = new Set();
  const rowsById = new Map();
  let previousRowId = null;
  for (const row of rows) {
    if (!isRecord(row) || typeof row.stable_id !== "string" || row.stable_id.length === 0) {
      errors.push("manifest row has no stable_id");
      continue;
    }
    const stableId = row.stable_id;
    if (seen.has(stableId)) errors.push(`duplicate stable_id: ${stableId}`);
    seen.add(stableId);
    rowsById.set(stableId, row);
    if (previousRowId !== null && compareStable(previousRowId, stableId) > 0) errors.push("manifest rows are not sorted");
    previousRowId = stableId;

    const prefix = KIND_PREFIX[row.kind];
    if (!prefix || !stableId.startsWith(prefix) || stableId.length === prefix.length) errors.push(`stable_id kind mismatch: ${stableId}`);
    for (const field of ["kind", "owner", "member", "domain", "status"]) {
      if (typeof row[field] !== "string" || row[field].length === 0) errors.push(`manifest row ${field} is required: ${stableId}`);
    }
    if (prefix) {
      const untagged = stableId.slice(prefix.length);
      const dot = untagged.lastIndexOf(".");
      const owner = dot < 0 ? untagged : untagged.slice(0, dot);
      const member = dot < 0 ? untagged : untagged.slice(dot + 1);
      if (row.owner !== owner || row.member !== member) errors.push(`manifest row identity mismatch: ${stableId}`);
      if (typeof row.owner === "string" && typeof row.member === "string" && row.domain !== rowDomain(row)) errors.push(`manifest row domain mismatch: ${stableId}`);
    }

    const tiers = row.applicable_tiers;
    if (!Array.isArray(tiers)) errors.push(`manifest row applicable_tiers is required: ${stableId}`);
    else {
      if ([...new Set(tiers)].length !== tiers.length) errors.push(`duplicate tier projection: ${stableId}`);
      for (const tier of tiers) if (!TIERS.includes(tier)) errors.push(`unknown tier projection ${tier}: ${stableId}`);
      if (canonicalJson(tiers) !== canonicalJson([...tiers].sort((left, right) => TIERS.indexOf(left) - TIERS.indexOf(right)))) errors.push(`manifest tiers are not sorted: ${stableId}`);
    }

    const projections = row.projections;
    const projectionKeys = new Set();
    if (!Array.isArray(projections)) errors.push(`manifest row projections are required: ${stableId}`);
    else {
      for (const projection of projections) {
        if (!isRecord(projection)) {
          errors.push(`manifest row has an invalid projection: ${stableId}`);
          continue;
        }
        if (!TIERS.includes(projection.tier)) errors.push(`unknown projection tier ${projection.tier}: ${stableId}`);
        if (typeof projection.route !== "string" || projection.route.length === 0) errors.push(`projection route is required: ${stableId}`);
        const key = `${projection.tier}\u0000${projection.route}`;
        if (projectionKeys.has(key)) errors.push(`duplicate row projection: ${stableId}:${projection.tier}:${projection.route}`);
        projectionKeys.add(key);
        if (!Object.prototype.hasOwnProperty.call(projection, "seam") || (projection.seam !== null && (typeof projection.seam !== "string" || projection.seam.length === 0))) {
          errors.push(`projection seam is invalid: ${stableId}`);
        }
        if (!Array.isArray(projection.evidence) || projection.evidence.some((item) => typeof item !== "string" || item.length === 0)) errors.push(`projection evidence is invalid: ${stableId}`);
      }
      const ordered = [...projections].sort((left, right) => TIERS.indexOf(left?.tier) - TIERS.indexOf(right?.tier) || compareStable(left?.route || "", right?.route || ""));
      if (canonicalJson(projections) !== canonicalJson(ordered)) errors.push(`manifest row projections are not sorted: ${stableId}`);
    }
    const projectionTiers = Array.isArray(projections) ? projections.filter(isRecord).map((projection) => projection.tier) : [];
    if (Array.isArray(tiers) && canonicalJson([...tiers].sort()) !== canonicalJson([...new Set(projectionTiers)].sort())) errors.push(`applicable tier projection mismatch: ${stableId}`);
    if (!Array.isArray(row.dispatcher_arms)) errors.push(`manifest dispatcher arms are required: ${stableId}`);
    else if (Array.isArray(projections) && canonicalJson(row.dispatcher_arms) !== canonicalJson(projections.filter(isRecord).map((projection) => projection.route))) errors.push(`dispatcher arms mismatch: ${stableId}`);

    for (const field of ["membership_sources", "membership_evidence"]) {
      if (!Array.isArray(row[field]) || row[field].some((item) => typeof item !== "string" || item.length === 0)) errors.push(`manifest row ${field} is invalid: ${stableId}`);
      else if (canonicalJson(row[field]) !== canonicalJson(uniqueSorted(row[field]))) errors.push(`manifest row ${field} is not sorted: ${stableId}`);
    }
    if (row.seed !== null && (typeof row.seed !== "string" || row.seed.length === 0)) errors.push(`manifest row seed is invalid: ${stableId}`);
    if (row.value_consuming !== null && typeof row.value_consuming !== "boolean") errors.push(`manifest row value_consuming is invalid: ${stableId}`);
    if (row.sink !== null && !isRecord(row.sink)) errors.push(`manifest row sink is invalid: ${stableId}`);
    if (row.exclusion !== null && !isRecord(row.exclusion)) errors.push(`manifest row exclusion is invalid: ${stableId}`);
    if (!VALID_STATUSES.has(row.status)) errors.push(`unknown manifest row status ${row.status}: ${stableId}`);
    if (row.status === "covered") {
      if (row.value_consuming !== true || !row.seed || !row.sink || row.sink.type_aware !== true || typeof row.sink.operation !== "string" || row.sink.operation.length === 0) errors.push(`executable row has no type-aware observable sink: ${stableId}`);
    }
    if (row.status === "excluded" && (!row.exclusion?.reason || !row.exclusion?.owner || !row.exclusion?.decision)) errors.push(`exclusion is not owner-ratified: ${stableId}`);
    if (row.status === "missing" && (row.value_consuming !== null || row.sink !== null)) errors.push(`missing row has executable proof: ${stableId}`);
    if (row.status === "unrouted" && (tiers?.length !== 0 || row.value_consuming !== null || row.sink !== null)) errors.push(`unrouted row has coverage: ${stableId}`);
  }

  const denominator = isRecord(manifest.denominator) ? manifest.denominator : null;
  if (!denominator) errors.push("manifest denominator is required");
  const sourceIds = denominator && isRecord(denominator.source_ids) ? denominator.source_ids : null;
  const counts = denominator && isRecord(denominator.counts) ? denominator.counts : null;
  if (!sourceIds) errors.push("manifest denominator source_ids are required");
  if (!counts) errors.push("manifest denominator counts are required");
  const memberIds = new Set();
  const denominatorIds = new Set();
  for (const kind of KIND_ORDER) {
    const source = sourceIds && Array.isArray(sourceIds[kind]) ? sourceIds[kind] : null;
    if (!source) {
      errors.push(`manifest denominator is missing ${kind}`);
      continue;
    }
    const sourceSet = new Set(source);
    if (sourceSet.size !== source.length) errors.push(`denominator contains duplicate ${kind}`);
    let previousId = null;
    for (const id of source) {
      if (typeof id !== "string" || id.length === 0 || !id.startsWith(KIND_PREFIX[kind])) errors.push(`denominator has invalid ${kind}: ${id}`);
      else {
        memberIds.add(id);
        if (denominatorIds.has(id)) errors.push(`duplicate denominator identity: ${id}`);
        denominatorIds.add(id);
        if (previousId !== null && compareStable(previousId, id) > 0) errors.push(`denominator ${kind} is not sorted`);
        previousId = id;
      }
    }
    if (counts && counts[kind] !== source.length) errors.push(`denominator count mismatch: ${kind}`);
    const rowSet = new Set(rows.filter((row) => row?.kind === kind).map((row) => row?.stable_id));
    for (const id of sourceSet) if (!rowSet.has(id)) errors.push(`source membership missing from manifest: ${id}`);
    for (const id of rowSet) if (!sourceSet.has(id)) errors.push(`manifest row is not source membership: ${id}`);
  }
  if (counts && counts.exclusions === undefined) errors.push("manifest exclusion count is required");

  const exclusions = manifest.exclusions;
  const exclusionsById = new Map();
  if (!Array.isArray(exclusions)) errors.push("manifest exclusions are required");
  else {
    let previousExclusion = null;
    for (const exclusion of exclusions) {
      if (!isRecord(exclusion) || typeof exclusion.stable_id !== "string" || exclusion.stable_id.length === 0) {
        errors.push("manifest exclusion has no stable_id");
        continue;
      }
      const stableId = exclusion.stable_id;
      if (exclusionsById.has(stableId)) errors.push(`duplicate exclusion identity: ${stableId}`);
      exclusionsById.set(stableId, exclusion);
      if (previousExclusion !== null && compareStable(previousExclusion, stableId) > 0) errors.push("manifest exclusions are not sorted");
      previousExclusion = stableId;
      const kind = KIND_ORDER.find((candidate) => stableId.startsWith(KIND_PREFIX[candidate]));
      if (!kind) errors.push(`exclusion has invalid stable_id: ${stableId}`);
      if (!memberIds.has(stableId)) errors.push(`exclusion is not source membership: ${stableId}`);
      for (const field of ["reason", "owner", "decision"]) {
        if (typeof exclusion[field] !== "string" || exclusion[field].length === 0) errors.push(`exclusion ${field} is required: ${stableId}`);
      }
    }
    if (counts && counts.exclusions !== exclusions.length) errors.push("denominator exclusion count mismatch");
  }
  for (const row of rows.filter(isRecord)) {
    const recorded = exclusionsById.get(row.stable_id);
    if (row.exclusion === null) {
      if (recorded) errors.push(`manifest exclusion is missing from row: ${row.stable_id}`);
      if (row.status === "excluded") errors.push(`excluded row has no exclusion record: ${row.stable_id}`);
    } else if (!recorded) {
      errors.push(`row exclusion is not persisted: ${row.stable_id}`);
    } else {
      const recordedDetails = { ...recorded };
      delete recordedDetails.stable_id;
      if (canonicalJson(row.exclusion) !== canonicalJson(recordedDetails) || row.status !== "excluded") errors.push(`row exclusion mismatch: ${row.stable_id}`);
    }
  }
  for (const stableId of exclusionsById.keys()) if (!rowsById.has(stableId)) errors.push(`exclusion has no manifest row: ${stableId}`);

  const actualRoutes = manifest.actual_routes;
  if (!isRecord(actualRoutes)) errors.push("manifest actual_routes are required");
  else {
    for (const key of Object.keys(actualRoutes)) if (!TIERS.includes(key)) errors.push(`unknown route tier: ${key}`);
    for (const tier of TIERS) {
      const routes = actualRoutes[tier];
      if (!Array.isArray(routes)) {
        errors.push(`manifest routes are missing ${tier}`);
        continue;
      }
      let previousRoute = null;
      const routeKeys = new Set();
      for (const route of routes) {
        if (!isRecord(route) || typeof route.stable_id !== "string" || typeof route.route !== "string" || route.route.length === 0) {
          errors.push(`manifest route is invalid: ${tier}`);
          continue;
        }
        const key = `${route.stable_id}\u0000${route.route}`;
        if (routeKeys.has(key)) errors.push(`duplicate actual route fact: ${tier}:${route.stable_id}:${route.route}`);
        routeKeys.add(key);
        if (!KIND_ORDER.some((kind) => route.stable_id.startsWith(KIND_PREFIX[kind]))) errors.push(`route has invalid stable_id: ${tier}:${route.stable_id}`);
        if (!Object.prototype.hasOwnProperty.call(route, "seam") || (route.seam !== null && (typeof route.seam !== "string" || route.seam.length === 0))) errors.push(`route seam is invalid: ${tier}:${route.stable_id}`);
        if (!Array.isArray(route.evidence) || route.evidence.some((item) => typeof item !== "string" || item.length === 0)) errors.push(`route evidence is invalid: ${tier}:${route.stable_id}`);
        const identity = `${route.stable_id}\u0000${route.route}`;
        if (previousRoute !== null && compareStable(previousRoute, identity) > 0) errors.push(`manifest routes are not sorted: ${tier}`);
        previousRoute = identity;
      }
    }
  }
  const routeMap = expectedRouteMap(manifest);
  if (routeMap.error) errors.push(routeMap.error);
  for (const tier of TIERS) for (const route of (Array.isArray(manifest.actual_routes?.[tier]) ? manifest.actual_routes[tier] : []).filter(isRecord)) {
    if (!seen.has(route.stable_id)) errors.push(`dispatcher arm has no public row: ${tier}:${route.stable_id}`);
    else {
      const row = rowsById.get(route.stable_id);
      const projection = Array.isArray(row?.projections)
        ? row.projections.find((candidate) => isRecord(candidate) && candidate.tier === tier && candidate.route === route.route)
        : null;
      if (!projection) errors.push(`dispatcher arm missing projection: ${tier}:${route.stable_id}:${route.route}`);
      else if (canonicalJson({ seam: projection.seam, evidence: projection.evidence }) !== canonicalJson({ seam: route.seam, evidence: route.evidence })) errors.push(`dispatcher arm proof mismatch: ${tier}:${route.stable_id}:${route.route}`);
    }
  }
  for (const row of rows.filter(isRecord)) for (const projection of (Array.isArray(row.projections) ? row.projections : []).filter(isRecord)) {
    const route = routeMap[projection.tier]?.get(`${row.stable_id}\u0000${projection.route}`) || null;
    if (!route) errors.push(`fake coverage or missing dispatcher arm: ${projection.tier}:${row.stable_id}:${projection.route}`);
  }
  const constructorRows = rows.filter((row) => row?.member === "new");
  const constructorIds = new Set();
  for (const row of constructorRows) {
    const key = `${row.kind}:${row.owner}`;
    if (constructorIds.has(key)) errors.push(`duplicate constructor: ${key}`);
    constructorIds.add(key);
    if (row.kind === "receiver_method" && !rows.some((candidate) => candidate?.kind === "nominal_type" && candidate.member === row.owner)) {
      // Receiver type names are often unqualified in the registry; this check
      // only rejects an actually unowned constructor, not a missing module path.
      if (!row.owner) errors.push(`constructor has no owner: ${row.stable_id}`);
    }
  }
  if (expectedIds) {
    const expected = new Set(expectedIds);
    for (const id of expected) if (!seen.has(id)) errors.push(`expected membership missing: ${id}`);
    for (const id of seen) if (!expected.has(id)) errors.push(`unexpected membership: ${id}`);
  }
  if (currentSnapshotHash !== null && currentSnapshotHash !== undefined && manifest.source_snapshot?.hash !== currentSnapshotHash) errors.push("manifest source snapshot is stale");
  return { ok: errors.length === 0, errors: [...new Set(errors)].sort() };
}

export function manifestIsStale(manifest, root = DEFAULT_ROOT) {
  return manifest?.source_snapshot?.hash !== sourceSnapshot(root).hash;
}

export function readManifest(path, { root = DEFAULT_ROOT } = {}) {
  if (typeof path !== "string" || path.length === 0) fail("manifest path is required");
  if (!existsSync(path)) fail(`unreadable manifest: ${path}`);
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`unreadable manifest ${path}: ${error.message}`);
  }
  const validation = validateManifest(manifest, { currentSnapshotHash: sourceSnapshot(root).hash });
  if (!validation.ok) fail(validation.errors.join("\n"));
  return manifest;
}

function hostileFixtures() {
  const surface = {
    moduleCalls: ["core.test.call"],
    receivers: [{ type: "Widget", member: "read" }],
    fields: [{ type: "Widget", field: "value" }],
    types: ["core.test.Widget"],
    routes: {
      aot: [{ stable_id: "module:core.test.call", route: "aot:fixture" }],
      jet_run: [{ stable_id: "receiver:Widget.read", route: "jet_run:fixture" }],
      interpreter: [{ stable_id: "field:Widget.value", route: "interpreter:fixture" }],
    },
    seeds: new Map([[
      "module:core.test.call",
      { path: "tests/conformance/corpus/core/test/call.jet", errors: [], sink: { type_aware: true, operation: "print", kind: "primitive" } },
    ]]),
    exclusions: new Map(),
    snapshot: sourceSnapshotFromContents({ "fixture.rs": "one" }),
    membershipSources: Object.fromEntries(KIND_ORDER.map((kind) => [kind, ["fixture.rs"]])),
  };
  const manifest = buildManifest({ surface });
  if (!validateManifest(manifest).ok) fail("valid manifest fixture rejected");
  const rehash = (value) => { value.content_digest = manifestContentDigest(value); return value; };
  const tamperedContent = JSON.parse(JSON.stringify(manifest));
  tamperedContent.rows[0].domain = "tampered";
  const tamperedValidation = validateManifest(tamperedContent);
  if (tamperedValidation.ok || !tamperedValidation.errors.includes("manifest content digest does not match manifest")) fail("manifest content tampering accepted");
  const missingReceiver = JSON.parse(JSON.stringify(manifest));
  missingReceiver.rows = missingReceiver.rows.filter((row) => row.stable_id !== "receiver:Widget.read");
  missingReceiver.denominator.source_ids.receiver_method = ["receiver:Widget.read"];
  rehash(missingReceiver);
  if (validateManifest(missingReceiver).ok) fail("missing receiver accepted");
  const missingField = JSON.parse(JSON.stringify(manifest));
  missingField.rows = missingField.rows.filter((row) => row.stable_id !== "field:Widget.value");
  rehash(missingField);
  if (validateManifest(missingField).ok) fail("missing field accepted");
  const fakeCoverage = JSON.parse(JSON.stringify(manifest));
  fakeCoverage.rows.find((row) => row.stable_id === "module:core.test.call").projections.push({ tier: "interpreter", route: "fake:coverage", evidence: [] });
  fakeCoverage.rows.find((row) => row.stable_id === "module:core.test.call").applicable_tiers.push("interpreter");
  rehash(fakeCoverage);
  if (validateManifest(fakeCoverage).ok) fail("fake coverage accepted");
  const fakeRow = JSON.parse(JSON.stringify(manifest));
  const fake = JSON.parse(JSON.stringify(fakeRow.rows[0]));
  fake.stable_id = "field:Widget.fake";
  fake.member = "fake";
  fakeRow.rows.push(fake);
  rehash(fakeRow);
  if (validateManifest(fakeRow).ok) fail("fake row accepted");
  const fakeRoute = JSON.parse(JSON.stringify(manifest));
  fakeRoute.actual_routes.aot.push({ stable_id: "module:core.test.call", route: "aot:fake", seam: null, evidence: [] });
  rehash(fakeRoute);
  if (validateManifest(fakeRoute).ok) fail("fake route accepted");
  const duplicate = JSON.parse(JSON.stringify(manifest));
  duplicate.rows.push(duplicate.rows[0]);
  rehash(duplicate);
  if (validateManifest(duplicate).ok) fail("duplicate constructor/row accepted");
  const duplicateIdentity = JSON.parse(JSON.stringify(manifest));
  duplicateIdentity.denominator.source_ids.module_call.push("module:core.test.call");
  rehash(duplicateIdentity);
  if (validateManifest(duplicateIdentity).ok) fail("duplicate denominator identity accepted");
  const observerless = JSON.parse(JSON.stringify(manifest));
  const observed = observerless.rows.find((row) => row.stable_id === "module:core.test.call");
  observed.status = "covered";
  observed.value_consuming = false;
  observed.sink = null;
  rehash(observerless);
  if (validateManifest(observerless).ok) fail("observerless value accepted");
  const malformedExclusion = JSON.parse(JSON.stringify(manifest));
  malformedExclusion.exclusions.push({ stable_id: "module:core.test.call", reason: "", owner: "owner", decision: "D-2335" });
  malformedExclusion.denominator.counts.exclusions = 1;
  rehash(malformedExclusion);
  if (validateManifest(malformedExclusion).ok) fail("malformed exclusion accepted");
  const invalidExclusion = JSON.parse(JSON.stringify(manifest));
  const excluded = invalidExclusion.rows.find((row) => row.stable_id === "module:core.test.call");
  excluded.status = "excluded";
  excluded.exclusion = { reason: "reason", owner: null, decision: "D-2335" };
  excluded.value_consuming = null;
  excluded.sink = null;
  rehash(invalidExclusion);
  if (validateManifest(invalidExclusion).ok) fail("invalid exclusion accepted");
  const stale = JSON.parse(JSON.stringify(manifest));
  stale.source_snapshot.hash = sha256("changed");
  rehash(stale);
  if (validateManifest(stale, { currentSnapshotHash: manifest.source_snapshot.hash }).ok) fail("stale snapshot accepted");
  console.log("hardening manifest hostile fixtures: PASS");
  return 0;
}

function main(args) {
  if (args.includes("--hostile-fixtures")) return hostileFixtures();
  const checkAt = args.indexOf("--check");
  if (checkAt >= 0) {
    const path = args[checkAt + 1];
    if (!path || path.startsWith("--")) fail("--check requires a path");
    const manifest = readManifest(path);
    const counts = Object.fromEntries(["covered", "missing", "unrouted", "excluded", "invalid", "invalid-exclusion"].map((status) => [status, manifest.rows.filter((row) => row.status === status).length]));
    console.log(`hardening manifest: ${manifest.rows.length} tagged rows; ${counts.covered} covered; ${counts.missing} missing; ${counts.unrouted} unrouted; ${counts.excluded} excluded; ${counts.invalid + counts["invalid-exclusion"]} invalid`);
    console.log(`hardening manifest: source ${manifest.source_snapshot.hash}; VALID`);
    return 0;
  }
  const manifest = buildManifest();
  const validation = validateManifest(manifest);
  if (!validation.ok) { for (const error of validation.errors) console.error(`error: ${error}`); return 1; }
  if (args.includes("--generate")) { process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`); return 0; }
  const writeAt = args.indexOf("--write");
  if (writeAt >= 0) {
    const path = args[writeAt + 1];
    if (!path) fail("--write requires a path");
    mkdirSync(dirname(resolve(path)), { recursive: true });
    writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`hardening manifest: wrote ${path}`);
  }
  const counts = Object.fromEntries(["covered", "missing", "unrouted", "excluded", "invalid", "invalid-exclusion"].map((status) => [status, manifest.rows.filter((row) => row.status === status).length]));
  console.log(`hardening manifest: ${manifest.rows.length} tagged rows; ${counts.covered} covered; ${counts.missing} missing; ${counts.unrouted} unrouted; ${counts.excluded} excluded; ${counts.invalid + counts["invalid-exclusion"]} invalid`);
  console.log(`hardening manifest: source ${manifest.source_snapshot.hash}; VALID`);
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try { process.exitCode = main(process.argv.slice(2)); }
  catch (error) { console.error(`error: ${error.message}`); process.exitCode = 1; }
}
