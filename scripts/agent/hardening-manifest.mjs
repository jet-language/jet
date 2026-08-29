#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalJson, sha256 } from "./hardening-repro.mjs";

export const SURFACE_SCHEMA = "jet.hardening.surface.v1";
export const SURFACE_SCHEMA_VERSION = 1;
export const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);

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

function parseReceiverRows(callsSource) {
  const out = [];
  for (const call of calls(callsSource, "CoreCallRecord::receiver(")) {
    const types = quoted(call.args[0] || "");
    const member = quoted(call.args[1] || "")[0];
    if (!member || types.length === 0) continue;
    for (const type of types) out.push({ stable_id: `receiver:${type}.${member}`, type, member });
  }
  return out;
}

function parsePlainCallRows(callsSource) {
  const out = [];
  for (const call of calls(callsSource, "CoreCallRecord::new(")) {
    const module = quoted(call.args[0] || "")[0];
    const member = quoted(call.args[1] || "")[0];
    if (module?.startsWith("core.") && member) out.push(`${module}.${member}`);
  }
  return [...new Set(out)].sort();
}

function parsePairLiterals(source) {
  const clean = withoutComments(source);
  const out = [];
  const pattern = /\(\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*\)/g;
  for (const match of clean.matchAll(pattern)) out.push([decodeRustString(match[1]), decodeRustString(match[2])]);
  return out;
}

function parseFieldRows(typesSource, surfaceSource) {
  const clean = withoutComments(typesSource);
  const out = new Set();
  const add = (type, field) => {
    if (/^[A-Z][A-Za-z0-9_]*$/.test(type) && /^[a-z_][A-Za-z0-9_]*$/.test(field)) out.add(`${type}.${field}`);
  };
  const pair = /\(\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*\)\s*=>/g;
  for (const match of clean.matchAll(pair)) {
    for (const type of quoted(match[1])) for (const field of quoted(match[2])) add(type, field);
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
    for (const match of body.matchAll(/"([a-z_][A-Za-z0-9_]*)"\s*(?:\|\s*)*(?==>)/g)) fields.push(match[1]);
    for (const type of types) for (const field of fields) add(type, field);
  }
  const constructable = clean.indexOf("pub(crate) fn core_constructable_fields");
  if (constructable >= 0) {
    const body = clean.slice(constructable);
    for (const match of body.matchAll(/"([A-Z][A-Za-z0-9_]*)"\s*=>\s*Some\s*\(\s*vec!\s*\[/g)) {
      const opening = body.indexOf("[", match.index + match[0].length - 1);
      let close;
      try { close = matching(body, opening, "[", "]"); } catch { continue; }
      for (const field of body.slice(opening + 1, close).matchAll(/\(\s*"([a-z_][A-Za-z0-9_]*)"/g)) add(match[1], field[1]);
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
  return [...out].sort().map((stable) => {
    const dot = stable.lastIndexOf(".");
    return { stable_id: `field:${stable}`, type: stable.slice(0, dot), field: stable.slice(dot + 1) };
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

function routeFactsFromSources(root, moduleIds, receiverRows, fieldRows, typeRows, plainRows) {
  const expectedModules = new Set(moduleIds.map(routeIdentity));
  const actual = Object.fromEntries(TIERS.map((tier) => [tier, []]));
  const add = (tier, stableId, route, evidence) => {
    const rows = actual[tier];
    if (!rows.some((row) => row.stable_id === stableId && row.route === route)) rows.push({ stable_id: stableId, route, evidence: [...evidence].sort() });
  };
  for (const tier of TIERS) {
    for (const path of ROUTE_FILES[tier]) {
      const absolute = join(root, path);
      if (!existsSync(absolute)) continue;
      const source = readFileSync(absolute, "utf8");
      for (const [module, member] of parsePairLiterals(source)) {
        const untagged = `${module}.${member}`;
        if (expectedModules.has(untagged)) add(tier, `module:${untagged}`, `${tier}:literal-dispatch`, [path]);
        for (const receiver of receiverRows) {
          if (receiver.member === member && receiver.type === module) add(tier, receiver.stable_id, `${tier}:receiver-dispatch`, [path]);
        }
      }
    }
  }
  const aotSources = ROUTE_FILES.aot.map((path) => existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : "").join("\n");
  const jitSources = ROUTE_FILES.jet_run.map((path) => existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : "").join("\n");
  if (/Syntax::core_call\(module\s*,\s*method\)/.test(aotSources)) {
    for (const row of plainRows) if (expectedModules.has(row)) add("aot", `module:${row}`, "aot:canonical-core-call-lookup", ROUTE_FILES.aot);
  }
  if (/Syntax::core_call\(module\s*,\s*method\)/.test(jitSources)) {
    for (const row of plainRows) if (expectedModules.has(row)) add("jet_run", `module:${row}`, "jet_run:canonical-core-call-lookup", ROUTE_FILES.jet_run);
  }
  const interpreter = actual.interpreter;
  if (ROUTE_FILES.interpreter.some((path) => existsSync(join(root, path)))) {
    const source = ROUTE_FILES.interpreter.map((path) => existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : "").join("\n");
    if (/core_receiver_method|evaluate_method/.test(source)) {
      for (const receiver of receiverRows) {
        if (!interpreter.some((row) => row.stable_id === receiver.stable_id)) add("interpreter", receiver.stable_id, "interpreter:receiver-dispatch", ROUTE_FILES.interpreter);
      }
    }
  }
  const shapePaths = {
    aot: ROUTE_FILES.aot,
    jet_run: ROUTE_FILES.jet_run,
    interpreter: ROUTE_FILES.interpreter,
  };
  for (const tier of TIERS) {
    const evidence = shapePaths[tier].filter((path) => existsSync(join(root, path)) && /core_struct_field(?:_type|_rust_name)/.test(readFileSync(join(root, path), "utf8")));
    if (evidence.length) {
      for (const row of fieldRows) add(tier, row.stable_id, `${tier}:canonical-field-shape`, evidence);
      for (const row of typeRows) add(tier, row.stable_id, `${tier}:canonical-type-shape`, evidence);
    }
  }
  for (const rows of Object.values(actual)) rows.sort((left, right) => left.stable_id.localeCompare(right.stable_id) || left.route.localeCompare(right.route));
  return actual;
}

function sourceFiles(root) {
  const files = new Set(Object.values(PATHS).map((path) => path));
  for (const paths of Object.values(ROUTE_FILES)) for (const path of paths) files.add(path);
  const corpus = join(root, "tests/conformance/corpus");
  for (const path of walk(corpus)) files.add(relative(root, path).replaceAll("\\", "/"));
  return [...files].sort();
}

export function sourceSnapshotFromContents(entries) {
  const files = Object.entries(entries).map(([path, contents]) => ({ path, bytes: Buffer.byteLength(String(contents)), sha256: sha256(contents) }))
    .sort((left, right) => left.path.localeCompare(right.path));
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
  const receiverRows = parseReceiverRows(readFileSync(join(root, PATHS.calls), "utf8"));
  const fieldRows = parseFieldRows(readFileSync(join(root, PATHS.fields), "utf8"), surfaceSource);
  const plainRows = parsePlainCallRows(readFileSync(join(root, PATHS.calls), "utf8"));
  const routes = routeFactsFromSources(root, moduleCalls, receiverRows, fieldRows, [...typeSet].map((stable_id) => ({ stable_id })), plainRows);
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
  };
}

function normalizeRoutes(routes = {}) {
  const out = Object.fromEntries(TIERS.map((tier) => [tier, []]));
  for (const tier of TIERS) {
    for (const value of routes[tier] || []) {
      if (typeof value === "string") out[tier].push({ stable_id: value.includes(":") ? value : `module:${value}`, route: `${tier}:fixture`, evidence: [] });
      else out[tier].push({ stable_id: value.stable_id, route: value.route || `${tier}:fixture`, evidence: [...(value.evidence || [])].sort() });
    }
    out[tier].sort((left, right) => left.stable_id.localeCompare(right.stable_id) || left.route.localeCompare(right.route));
  }
  return out;
}

function normalizedExclusions(exclusions = new Map()) {
  const out = new Map();
  const entries = exclusions instanceof Map ? exclusions.entries() : Object.entries(exclusions);
  for (const [key, value] of entries) {
    const stable = key.includes(":") ? key : `module:${key}`;
    out.set(stable, typeof value === "string" ? { reason: value, owner: null, decision: null } : value);
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
  for (const tier of TIERS) for (const route of routes[tier]) if (route.stable_id === identity.stable_id) routeRows.push({ tier, route: route.route, evidence: route.evidence });
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
    projections: routeRows.sort((left, right) => TIERS.indexOf(left.tier) - TIERS.indexOf(right.tier) || left.route.localeCompare(right.route)),
    dispatcher_arms: routeRows.map((route) => route.route),
    membership_sources: [...(surface.membershipSources?.[kind] || [])].sort(),
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
    .sort((left, right) => left.stable_id.localeCompare(right.stable_id));
  const manifest = {
    schema: SURFACE_SCHEMA,
    schema_version: SURFACE_SCHEMA_VERSION,
    source_snapshot: actual.snapshot || sourceSnapshot(root),
    denominator: {
      source_ids: members,
      counts: Object.fromEntries(KIND_ORDER.map((kind) => [kind, members[kind].length])),
    },
    actual_routes: routes,
    exclusions: [...exclusions.entries()].sort(([left], [right]) => left.localeCompare(right)).map(([stable_id, value]) => ({ stable_id, ...value })),
    rows,
  };
  const validation = validateManifest(manifest);
  if (!validation.ok) fail(validation.errors.join("\n"));
  return manifest;
}

function expectedRouteMap(manifest) {
  const out = Object.fromEntries(TIERS.map((tier) => [tier, new Map()]));
  for (const tier of TIERS) for (const route of manifest.actual_routes?.[tier] || []) {
    const key = `${route.stable_id}\u0000${route.route}`;
    if (out[tier].has(key)) return { error: `duplicate actual route fact: ${tier}:${route.stable_id}:${route.route}` };
    out[tier].set(key, route);
  }
  return out;
}

export function validateManifest(manifest, { expectedIds = null, currentSnapshotHash = null } = {}) {
  const errors = [];
  if (!manifest || typeof manifest !== "object") return { ok: false, errors: ["manifest is not an object"] };
  if (manifest.schema !== SURFACE_SCHEMA) errors.push(`manifest schema must be ${SURFACE_SCHEMA}`);
  if (manifest.schema_version !== SURFACE_SCHEMA_VERSION) errors.push(`manifest schema_version must be ${SURFACE_SCHEMA_VERSION}`);
  const rows = Array.isArray(manifest.rows) ? manifest.rows : [];
  const seen = new Set();
  for (const row of rows) {
    if (!row || typeof row.stable_id !== "string") { errors.push("manifest row has no stable_id"); continue; }
    if (seen.has(row.stable_id)) errors.push(`duplicate stable_id: ${row.stable_id}`);
    seen.add(row.stable_id);
    const prefix = KIND_PREFIX[row.kind];
    if (!prefix || !row.stable_id.startsWith(prefix)) errors.push(`stable_id kind mismatch: ${row.stable_id}`);
    const tiers = row.applicable_tiers || [];
    if ([...new Set(tiers)].length !== tiers.length) errors.push(`duplicate tier projection: ${row.stable_id}`);
    for (const tier of tiers) if (!TIERS.includes(tier)) errors.push(`unknown tier projection ${tier}: ${row.stable_id}`);
    const projectionTiers = (row.projections || []).map((projection) => projection.tier);
    if (canonicalJson([...tiers].sort()) !== canonicalJson([...new Set(projectionTiers)].sort())) errors.push(`applicable tier projection mismatch: ${row.stable_id}`);
    if (row.status === "covered") {
      if (row.value_consuming !== true || !row.sink || row.sink.type_aware !== true || typeof row.sink.operation !== "string") errors.push(`executable row has no type-aware observable sink: ${row.stable_id}`);
    }
    if (row.status === "excluded" && (!row.exclusion?.reason || !row.exclusion?.owner || !row.exclusion?.decision)) errors.push(`exclusion is not owner-ratified: ${row.stable_id}`);
    if (row.status === "invalid" && (!Array.isArray(row.errors) || row.errors.length === 0)) errors.push(`invalid row has no failure: ${row.stable_id}`);
  }
  const denominator = manifest.denominator?.source_ids || {};
  for (const kind of KIND_ORDER) {
    const source = Array.isArray(denominator[kind]) ? denominator[kind] : [];
    const sourceSet = new Set(source);
    if (sourceSet.size !== source.length) errors.push(`denominator contains duplicate ${kind}`);
    const rowSet = new Set(rows.filter((row) => row.kind === kind).map((row) => row.stable_id));
    for (const id of sourceSet) if (!rowSet.has(id)) errors.push(`source membership missing from manifest: ${id}`);
    for (const id of rowSet) if (!sourceSet.has(id)) errors.push(`manifest row is not source membership: ${id}`);
  }
  const routeMap = expectedRouteMap(manifest);
  if (routeMap.error) errors.push(routeMap.error);
  for (const tier of TIERS) for (const route of manifest.actual_routes?.[tier] || []) {
    if (!seen.has(route.stable_id)) errors.push(`dispatcher arm has no public row: ${tier}:${route.stable_id}`);
    else {
      const row = rows.find((candidate) => candidate.stable_id === route.stable_id);
      if (!row?.projections?.some((projection) => projection.tier === tier && projection.route === route.route)) errors.push(`dispatcher arm missing projection: ${tier}:${route.stable_id}:${route.route}`);
    }
  }
  for (const row of rows) for (const projection of row.projections || []) {
    const route = manifest.actual_routes?.[projection.tier]?.find((candidate) => candidate.stable_id === row.stable_id && candidate.route === projection.route);
    if (!route) errors.push(`fake coverage or missing dispatcher arm: ${projection.tier}:${row.stable_id}:${projection.route}`);
  }
  const constructorRows = rows.filter((row) => row.member === "new");
  const constructorIds = new Set();
  for (const row of constructorRows) {
    const key = `${row.kind}:${row.owner}`;
    if (constructorIds.has(key)) errors.push(`duplicate constructor: ${key}`);
    constructorIds.add(key);
    if (row.kind === "receiver_method" && !rows.some((candidate) => candidate.kind === "nominal_type" && candidate.member === row.owner)) {
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
  if (currentSnapshotHash && manifest.source_snapshot?.hash !== currentSnapshotHash) errors.push("manifest source snapshot is stale");
  return { ok: errors.length === 0, errors: [...new Set(errors)].sort() };
}

export function manifestIsStale(manifest, root = DEFAULT_ROOT) {
  return manifest?.source_snapshot?.hash !== sourceSnapshot(root).hash;
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
  const missingReceiver = JSON.parse(JSON.stringify(manifest));
  missingReceiver.rows = missingReceiver.rows.filter((row) => row.stable_id !== "receiver:Widget.read");
  missingReceiver.denominator.source_ids.receiver_method = ["receiver:Widget.read"];
  if (validateManifest(missingReceiver).ok) fail("missing receiver accepted");
  const missingField = JSON.parse(JSON.stringify(manifest));
  missingField.rows = missingField.rows.filter((row) => row.stable_id !== "field:Widget.value");
  if (validateManifest(missingField).ok) fail("missing field accepted");
  const fakeCoverage = JSON.parse(JSON.stringify(manifest));
  fakeCoverage.rows.find((row) => row.stable_id === "module:core.test.call").projections.push({ tier: "interpreter", route: "fake:coverage", evidence: [] });
  fakeCoverage.rows.find((row) => row.stable_id === "module:core.test.call").applicable_tiers.push("interpreter");
  if (validateManifest(fakeCoverage).ok) fail("fake coverage accepted");
  const duplicate = JSON.parse(JSON.stringify(manifest));
  duplicate.rows.push(duplicate.rows[0]);
  if (validateManifest(duplicate).ok) fail("duplicate constructor/row accepted");
  const observerless = JSON.parse(JSON.stringify(manifest));
  const observed = observerless.rows.find((row) => row.stable_id === "module:core.test.call");
  observed.status = "covered";
  observed.value_consuming = false;
  observed.sink = null;
  if (validateManifest(observerless).ok) fail("observerless value accepted");
  const stale = JSON.parse(JSON.stringify(manifest));
  stale.source_snapshot.hash = sha256("changed");
  if (validateManifest(stale, { currentSnapshotHash: manifest.source_snapshot.hash }).ok) fail("stale snapshot accepted");
  console.log("hardening manifest hostile fixtures: PASS");
  return 0;
}

function main(args) {
  if (args.includes("--hostile-fixtures")) return hostileFixtures();
  const manifest = buildManifest();
  const validation = validateManifest(manifest);
  if (!validation.ok) { for (const error of validation.errors) console.error(`error: ${error}`); return 1; }
  if (args.includes("--generate")) { process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`); return 0; }
  const writeAt = args.indexOf("--write");
  if (writeAt >= 0) {
    const path = args[writeAt + 1];
    if (!path) fail("--write requires a path");
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
