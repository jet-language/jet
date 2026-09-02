#!/usr/bin/env node

import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/*
 * Registry-driven Core conformance corpus (#2286).
 *
 * module_items.rs is the denominator. The corpus is deliberately not guessed
 * from fixed_sigs.rs or core_calls.rs: an exported operation without a second
 * route is still a public operation that needs a witness or a named carve-out.
 * Recipes below are only small, known-good seeds. Hard or effectful operations
 * are hand-authored under tests/conformance/corpus and remain visible as
 * uncovered until someone supplies their real arguments and authority.
 * Generic core rows may be called through a typed receiver (`mem.Ptr<T>.from_addr`)
 * even when the denominator row is published under `core.mem`. The source
 * matcher accepts that generic-receiver shape for every row; sema remains the
 * authority for whether the receiver and method are valid.
 */

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const MODULE_ITEMS = join(ROOT, "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs");
const MEM_SURFACE = join(ROOT, "crates/jet-foundation/src/Syntax/core_surface.rs");
const CORPUS = join(ROOT, "tests/conformance/corpus");
const EXCLUSIONS = join(ROOT, "tests/conformance/exclusions.tsv");

// `module_items.rs` also publishes these math fields. `zero` is deliberately
// absent: sema treats `core.math.zero()` as a callable operation.
const VALUE_NAMES = new Set(["pi", "e", "tau", "infinity", "nan"]);

// Seed only witnesses whose call shape is known. The denominator check, not
// this map, decides whether the rest of Core is covered.
const RECIPES = new Map([
  ["core.math.sqrt", `// core-conformance: core.math.sqrt
use core.math as math

fn run() {
    result :: math.sqrt(4.0)
    print(result)
}
`],
  ["core.math.round", `// core-conformance: core.math.round
use core.math as math

fn run() {
    result :: math.round(2.5)
    print(result)
}
`],
  ["core.math.zero", `// core-conformance: core.math.zero
use core.math as math

fn run() {
    result :: math.zero()
    print(result)
}
`],
  ["core.math.gcd", `// core-conformance: core.math.gcd
use core.math as math

fn run() {
    result :: math.gcd(18, 12)
    print(result)
}
`],
  ["core.math.is_even", `// core-conformance: core.math.is_even
use core.math as math

fn run() {
    result :: math.is_even(18)
    print(result)
}
`],
  ["core.text.lower", `// core-conformance: core.text.lower
use core.text as text

fn run() {
    result :: text.lower("JET")
    print(result)
}
`],
  ["core.text.trim", `// core-conformance: core.text.trim
use core.text as text

fn run() {
    result :: text.trim("  jet  ")
    print(result)
}
`],
  ["core.text.byte_count", `// core-conformance: core.text.byte_count
use core.text as text

fn run() {
    result :: text.byte_count("jet")
    print(result)
}
`],
  ["core.text.fmt.grouped", `// core-conformance: core.text.fmt.grouped
use core.text.fmt as fmt

fn run() {
    result :: fmt.grouped(Float{1234.5678}, 2)
    print(result)
}
`],
  ["core.args.spec", `// core-conformance: core.args.spec
use core.args as args

fn run() {
    result :: args.spec()
    print(result.completion("bash").contains("--help"))
}
`],
  ["core.crypto.uuid.v4", `// core-conformance: core.crypto.uuid.v4
use core.crypto.uuid as uuid

fn run() {
    result :: uuid.v4()
    print(result.len())
}
`],
  ["core.time.parse_rfc3339", `// core-conformance: core.time.parse_rfc3339
use core.time as time

fn run() {
    result :: time.parse_rfc3339("2024-03-01T12:00:00Z") ?? panic("parse")
    print(result.to_timestamp())
}
`],
]);
// Calls whose checked result is exactly Unit must still be observed without
// asking Display to render Unit.  Keep this list aligned with the sema return
// projections; the generator applies one observer shape to every such seed.
const UNIT_RESULT_KEYS = new Set([
  "core.term.binwrite",
  "core.term.progress",
  "core.net.set_timeout",
  "core.net.unix_write_all_bytes",
  "core.net.tcp_write_text",
  "core.net.set_write_timeout",
  "core.net.tcp_write",
  "core.net.unix_write",
  "core.net.tcp_reply",
  "core.net.unix_close",
  "core.net.set_nodelay",
  "core.net.set_ttl",
  "core.net.tcp_close",
  "core.net.tcp_shutdown",
  "core.net.tcp_write_all_bytes",
  "core.net.set_read_timeout",
  "core.net.unix_shutdown",
  "core.net.udp_set_timeout",
  "core.ui.mount",
  "core.files.write",
  "core.files.write_atomic",
  "core.files.remove_dir",
  "core.files.set_mode",
  "core.files.remove_all",
  "core.files.remove",
  "core.files.rename",
  "core.files.write_at",
  "core.files.symlink",
  "core.files.hard_link",
  "core.files.write_bytes",
  "core.mem.volatile_write",
  "core.http.server.static_files",
  "core.http.server.cors",
  "core.http.server.request_id",
  "core.http.server.serve_once_listener",
  "core.perf.reset_fidelity",
  "core.perf.override_fidelity",
  "core.tasks.yield_now",
]);

function matching(text, start, opening, closing) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockDepth = 0;
  for (let i = start; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") lineComment = false;
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && n === "*") {
        blockDepth += 1;
        i += 1;
      } else if (c === "*" && n === "/") {
        blockDepth -= 1;
        i += 1;
      }
      continue;
    }
    if (quote === "triple") {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"' && text.slice(i, i + 3) === '"""') {
        quote = null;
        i += 2;
      }
      continue;
    }
    if (quote === "regular") {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = null;
      continue;
    }
    if (c === '"' && text.slice(i, i + 3) === '"""') {
      quote = "triple";
      i += 2;
      continue;
    }
    if (c === '"') {
      quote = "regular";
      continue;
    }
    if (c === "/" && n === "/") {
      lineComment = true;
      i += 1;
      continue;
    }
    if (c === "/" && n === "*") {
      blockDepth = 1;
      i += 1;
      continue;
    }
    if (c === opening) depth += 1;
    else if (c === closing && --depth === 0) return i;
  }
  throw new Error(`unbalanced ${opening} at ${start}`);
}

// Keep source offsets and line structure while removing syntax that must not
// participate in registry or witness discovery.
function withoutComments(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockDepth = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") {
        lineComment = false;
        out += c;
      } else {
        out += c === "\r" ? c : " ";
      }
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && n === "*") {
        blockDepth += 1;
        out += "  ";
        i += 1;
      } else if (c === "*" && n === "/") {
        blockDepth -= 1;
        out += "  ";
        i += 1;
      } else {
        out += c === "\n" || c === "\r" ? c : " ";
      }
      continue;
    }
    if (quote === "triple") {
      out += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"' && text.slice(i, i + 3) === '"""') {
        out += '""';
        i += 2;
        quote = null;
      }
      continue;
    }
    if (quote === "regular") {
      out += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = null;
      continue;
    }
    if (c === '"' && text.slice(i, i + 3) === '"""') {
      out += '"""';
      i += 2;
      quote = "triple";
      continue;
    }
    if (c === '"') {
      out += c;
      quote = "regular";
      continue;
    }
    if (c === "/" && n === "/") {
      out += "  ";
      i += 1;
      lineComment = true;
      continue;
    }
    if (c === "/" && n === "*") {
      out += "  ";
      i += 1;
      blockDepth = 1;
      continue;
    }
    out += c;
  }
  return out;
}

// The complement of withoutComments: strings are blanked too, so names and
// calls in comments or string literals can never become witness evidence.
function codeOnly(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockDepth = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") {
        lineComment = false;
        out += c;
      } else {
        out += c === "\r" ? c : " ";
      }
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && n === "*") {
        blockDepth += 1;
        out += "  ";
        i += 1;
      } else if (c === "*" && n === "/") {
        blockDepth -= 1;
        out += "  ";
        i += 1;
      } else {
        out += c === "\n" || c === "\r" ? c : " ";
      }
      continue;
    }
    if (quote === "triple") {
      if (c === '"' && text.slice(i, i + 3) === '"""') {
        out += "   ";
        i += 2;
        quote = null;
      } else {
        out += c === "\n" || c === "\r" ? c : " ";
        if (escaped) escaped = false;
        else if (c === "\\") escaped = true;
      }
      continue;
    }
    if (quote === "regular") {
      out += c === "\n" || c === "\r" ? c : " ";
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = null;
      continue;
    }
    if (c === '"' && text.slice(i, i + 3) === '"""') {
      out += "   ";
      i += 2;
      quote = "triple";
      continue;
    }
    if (c === '"') {
      out += " ";
      quote = "regular";
      continue;
    }
    if (c === "/" && n === "/") {
      out += "  ";
      i += 1;
      lineComment = true;
      continue;
    }
    if (c === "/" && n === "*") {
      out += "  ";
      i += 1;
      blockDepth = 1;
      continue;
    }
    out += c;
  }
  return out;
}

function decodeRustString(value) {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

function rustStringConstants(source) {
  const constants = new Map();
  const clean = withoutComments(source);
  const pattern = /pub\s+const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g;
  for (const match of clean.matchAll(pattern)) {
    constants.set(match[1], decodeRustString(match[2]));
  }
  return constants;
}

function rustStringExpressions(text, constants, { preserveDuplicates = false } = {}) {
  const clean = withoutComments(text);
  const values = quoted(clean);
  const names = preserveDuplicates ? values : new Set(values);
  for (const match of clean.matchAll(/\b(?:Syntax::)?([A-Z][A-Z0-9_]*)\b/g)) {
    if (!constants.has(match[1])) continue;
    const value = constants.get(match[1]);
    if (preserveDuplicates) values.push(value);
    else names.add(value);
  }
  for (const match of clean.matchAll(/\bSyntax::([A-Z][A-Z0-9_]*)\b/g)) {
    if (!constants.has(match[1])) {
      throw new Error(`unresolved Syntax string constant: ${match[1]}`);
    }
  }
  return preserveDuplicates ? values : Array.from(names);
}

function quoted(text) {
  return Array.from(text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g), (m) => decodeRustString(m[1]));
}

function escapedRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function moduleItems() {
  const source = readFileSync(MODULE_ITEMS, "utf8");
  const start = source.indexOf("pub fn core_module_items");
  const end = source.indexOf("/// Ratified nominal types", start);
  if (start < 0 || end < 0) throw new Error("core_module_items source anchors disappeared");
  const body = withoutComments(source.slice(start, end));
  const constants = rustStringConstants(readFileSync(MEM_SURFACE, "utf8"));
  const out = new Map();
  const duplicateRows = new Set();
  const arms = /^\s*((?:"[^"]+"\s*(?:\|\s*)?)+)=>\s*&\[/gm;
  for (const arm of body.matchAll(arms)) {
    const modules = rustStringExpressions(arm[1], constants);
    const opening = body.indexOf("[", arm.index + arm[0].length - 1);
    const close = matching(body, opening, "[", "]");
    const names = rustStringExpressions(body.slice(opening + 1, close), constants, { preserveDuplicates: true });
    for (const module of modules) {
      if (!out.has(module)) out.set(module, new Set());
      for (const name of names) {
        const values = out.get(module);
        if (module !== "core.mem" && values.has(name)) duplicateRows.add(`${module}.${name}`);
        values.add(name);
      }
    }
  }

  // This branch is registry-owned too, although its names are generated from
  // policy declarations rather than written as a literal array. The current
  // declarations are type-like and therefore contribute no function rows.
  if (body.includes('module == "core.compiler.lang"')) {
    out.set("core.compiler.lang", new Set());
  }

  // core.mem is intentionally a typed gate table instead of a literal match
  // arm. Resolve its string constants from the same source that owns the gate.
  const memSource = withoutComments(readFileSync(MEM_SURFACE, "utf8"));
  const memStart = memSource.indexOf("pub const CORE_MEM_GATE_TIERS");
  const table = memSource.indexOf("= &[", memStart);
  if (memStart < 0 || table < 0) throw new Error("CORE_MEM_GATE_TIERS source anchor disappeared");
  const opening = memSource.indexOf("[", table);
  const close = matching(memSource, opening, "[", "]");
  const memValues = rustStringExpressions(memSource.slice(opening + 1, close), constants, { preserveDuplicates: true });
  const mem = new Set(memValues);
  if (mem.size !== memValues.length) {
    throw new Error("CORE_MEM_GATE_TIERS contains duplicate item names");
  }
  if (mem.size === 0) throw new Error("CORE_MEM_GATE_TIERS resolved no item names");
  out.set("core.mem", mem);
  if (duplicateRows.size) {
    throw new Error(
      `core_module_items contains duplicate denominator row(s): ${Array.from(duplicateRows).sort().join(", ")}`,
    );
  }
  if (out.size === 0) throw new Error("core_module_items yielded no modules");
  return out;
}

function inventory() {
  const modules = moduleItems();
  const moduleNames = new Set(modules.keys());
  const rows = [];
  for (const [module, names] of modules) {
    for (const name of names) {
      if (/^[A-Z]/.test(name)) continue;
      if (VALUE_NAMES.has(name) || moduleNames.has(`${module}.${name}`)) continue;
      rows.push(`${module}.${name}`);
    }
  }
  return rows.sort();
}

function walk(dir) {
  if (!existsSync(dir)) return [];
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(path));
    else if (entry.isFile() && entry.name.endsWith(".jet") && entry.name !== "package.jet") files.push(path);
  }
  return files.sort();
}

function keyForPath(path) {
  const rel = relative(CORPUS, path).replaceAll("\\", "/");
  const parts = rel.split("/");
  const name = parts.pop().replace(/\.jet$/, "");
  return `${parts.join(".")}.${name}`;
}

function parseExclusions(source = existsSync(EXCLUSIONS) ? readFileSync(EXCLUSIONS, "utf8") : "") {
  const rows = new Map();
  for (const [index, raw] of source.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const fields = raw.split("\t").map((field) => field.trim());
    if (fields.length !== 4 || fields.some((field) => !field)) {
      throw new Error(
        `malformed conformance carve-out at line ${index + 1}: expected key<TAB>reason<TAB>owner<TAB>decision`,
      );
    }
    const [key, reason] = fields;
    if (rows.has(key)) throw new Error(`duplicate conformance carve-out: ${key}`);
    rows.set(key, reason);
  }
  return rows;
}

function stringInterpolatesValue(text, value) {
  const clean = withoutComments(text);
  let quote = null;
  let escaped = false;
  for (let i = 0; i < clean.length; i += 1) {
    const c = clean[i];
    if (!quote) {
      if (c === '"' && clean.slice(i, i + 3) === '"""') {
        quote = "triple";
        i += 2;
      } else if (c === '"') {
        quote = "regular";
      }
      continue;
    }
    if (escaped) {
      escaped = false;
      continue;
    }
    if (c === "\\") {
      escaped = true;
      continue;
    }
    if (quote === "triple" && c === '"' && clean.slice(i, i + 3) === '"""') {
      quote = null;
      i += 2;
      continue;
    }
    if (quote === "regular" && c === '"') {
      quote = null;
      continue;
    }
    if (c !== "{") continue;
    if (clean[i + 1] === "{") {
      i += 1;
      continue;
    }
    let close;
    try {
      close = matching(clean, i, "{", "}");
    } catch {
      return false;
    }
    if (expressionConsumesValue(clean.slice(i + 1, close), value)) return true;
    i = close;
  }
  return false;
}

function expressionConsumesValue(text, value) {
  const valueUse = new RegExp(
    `(?<![A-Za-z0-9_])${escapedRegExp(value)}(?![A-Za-z0-9_])`,
  );
  return valueUse.test(codeOnly(text)) || stringInterpolatesValue(text, value);
}

function observerCalls(code) {
  const observers = [];
  const pattern = /(?<![A-Za-z0-9_.])(?:print|eprint|assert)\s*\(/g;
  for (const match of code.matchAll(pattern)) {
    const operation = match[0].match(/^(print|eprint|assert)/)[1];
    const open = match.index + match[0].lastIndexOf("(");
    try {
      observers.push({ operation, open, close: matching(code, open, "(", ")") });
    } catch {
      // An unbalanced observer cannot prove result consumption.
    }
  }
  return observers;
}

// A registered result also counts as consumed when it reaches an observer
// through later bindings: `first :: reader.next()` followed by `print(first)`
// reads `reader`. Walking that chain to a fixpoint keeps genuinely unread
// bindings failing while accepting a program that really consumes the value.
function valueReachesObserver(code, source, observers, value, after) {
  const bindings = [];
  const pattern = /(?:^|[{};\n])\s*(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*([^\n;]*)/g;
  for (const match of code.matchAll(pattern)) {
    bindings.push({ name: match[1], init: match[2] });
  }
  const reached = new Set([value]);
  for (let pass = 0; pass <= bindings.length; pass += 1) {
    let grew = false;
    for (const held of Array.from(reached)) {
      const seen = observers.some(
        ({ open, close }) => open > after && expressionConsumesValue(source.slice(open + 1, close), held),
      );
      if (seen) return true;
      // Builder values are also consumed by instance methods used as bare
      // statements. This keeps `server.html(...); server.port(...)` honest
      // without treating an unobserved binding as covered.
      const methodUse = new RegExp(
        `(?<![A-Za-z0-9_.])${escapedRegExp(held)}\\s*\\.\\s*[A-Za-z_][A-Za-z0-9_]*\\s*\\(`,
        "g",
      );
      if (Array.from(code.matchAll(methodUse)).some(({ index }) => index > after)) return true;
      if (matchArmObserves(code, observers, held)) return true;
      for (const candidate of bindings) {
        if (reached.has(candidate.name)) continue;
        if (!expressionConsumesValue(candidate.init, held)) continue;
        reached.add(candidate.name);
        grew = true;
      }
    }
    if (!grew) break;
  }
  return false;
}

// Branching on a value and printing from the arms observes it. The arm binders
// are introduced by the pattern, not by a `::` binding, so the chain above
// cannot see them; this reads the match as one consumption site.
function matchArmObserves(code, observers, held) {
  const pattern = /(?<![A-Za-z0-9_.])if\s+([^\n{]*?)==\s*\{/g;
  for (const match of code.matchAll(pattern)) {
    if (!expressionConsumesValue(match[1], held)) continue;
    const open = match.index + match[0].length - 1;
    let close;
    try {
      close = matching(code, open, "{", "}");
    } catch {
      continue;
    }
    if (observers.some(({ open: obs }) => obs > open && obs < close)) return true;
  }
  return false;
}

function sourceErrors(key, source) {
  const errors = [];
  const expectedMarker = `// core-conformance: ${key}`;
  const firstLine = source.split(/\r?\n/, 1)[0];
  if (firstLine !== expectedMarker) {
    const marker = firstLine.match(/^\s*\/\/\s*core-conformance:\s*(.*?)\s*$/);
    if (marker) errors.push(`marker names ${marker[1] || "<empty>"}`);
    else errors.push(`file must start with ${expectedMarker}`);
  }
  const markerCount = source
    .split(/\r?\n/)
    .filter((line) => line.trim() === expectedMarker)
    .length;
  if (markerCount > 1) errors.push(`expected exactly one ${expectedMarker} marker, found ${markerCount}`);

  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const name = key.slice(dot + 1);
  const code = codeOnly(source);
  const unitRun = code.match(/\bfn\s+run\s*\(\s*\)\s*\{/);
  if (unitRun) {
    const opening = unitRun.index + unitRun[0].lastIndexOf("{");
    const closing = matching(code, opening, "{", "}");
    if (/\?\?\s*return\s+Err\s*\(/.test(code.slice(opening + 1, closing))) {
      errors.push("Unit run cannot use ?? return Err(...) propagation");
    }
  }
  const usePattern = new RegExp(
    `^\\s*use\\s+${escapedRegExp(module)}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)[ \\t]*(?:;[ \\t]*)?\\r?$`,
    "gm",
  );
  const aliases = Array.from(code.matchAll(usePattern));
  if (aliases.length !== 1) {
    errors.push(`expected one use ${module} as <alias>, found ${aliases.length}`);
    return errors;
  }
  const alias = aliases[0][1];
  // A registry row may name a method whose canonical surface is a generic
  // receiver (`alias.Type<T>.method(...)`) rather than `alias.method(...)`.
  // Match both shapes without hard-coding one type or method.
  const call = new RegExp(
    `(?<![A-Za-z0-9_.])${escapedRegExp(alias)}\\s*\\.\\s*(?:${escapedRegExp(name)}\\s*(?:<[^{}]*>\\s*)?|[A-Za-z_][A-Za-z0-9_]*\\s*<[^{}]*>\\s*\\.\\s*${escapedRegExp(name)}\\s*)\\(`,
    "g",
  );
  const calls = Array.from(code.matchAll(call));
  if (calls.length !== 1) {
    errors.push(`expected one ${module}.${name} call, found ${calls.length}`);
    return errors;
  }

  const callStart = calls[0].index;
  const callOpen = callStart + calls[0][0].lastIndexOf("(");
  let callClose;
  try {
    callClose = matching(code, callOpen, "(", ")");
  } catch {
    errors.push(`malformed ${module}.${name} call: unbalanced parentheses`);
    return errors;
  }

  const lineStart = code.lastIndexOf("\n", callStart) + 1;
  const beforeCall = code.slice(lineStart, callStart);
  const binding = beforeCall.match(/(?:^|[{};])\s*(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*$/);
  const observers = observerCalls(code);
  if (binding) {
    const value = binding[1];
    if (value.startsWith("_")) errors.push(`result is bound to discard name ${value}`);
    if (!valueReachesObserver(code, source, observers, value, callClose)) {
      errors.push(`bound result ${value} is never consumed by print/eprint/assert`);
    }
  } else {
    const directObserver = observers.some(({ open, close }) => open < callStart && callStart < close);
    const lineEnd = code.indexOf("\n", callClose);
    const tail = code.slice(callClose + 1, lineEnd < 0 ? code.length : lineEnd);
    const propagated = /\?\?\s*panic\s*\(/.test(tail);
    const observesSuccess = observers.some(({ open }) => open > callClose);
    // Unit-returning rows are consumed by the effect they perform. A later
    // observer in the same witness records that effect without manufacturing
    // a value or wrapping the call in a lambda.
    const observesUnitEffect = UNIT_RESULT_KEYS.has(key) && observesSuccess;
    if (!directObserver && !(propagated && observesSuccess) && !observesUnitEffect) {
      errors.push("direct result is not consumed by print/eprint/assert or explicit error propagation");
    }
  }
  return errors;
}

function auditEntries(expected, witnesses, exclusions) {
  const expectedRows = Array.from(expected).sort();
  const expectedSet = new Set(expectedRows);
  const errors = [];
  for (let i = 1; i < expectedRows.length; i += 1) {
    if (expectedRows[i] === expectedRows[i - 1]) {
      errors.push(`${expectedRows[i]}: denominator contains a duplicate row`);
    }
  }
  const files = new Map();
  for (const witness of witnesses) {
    const { key, path, source } = witness;
    if (!expectedSet.has(key)) {
      errors.push(`${key}: file is not a public Core function`);
      continue;
    }
    if (files.has(key)) {
      errors.push(`${key}: duplicate files ${files.get(key)} and ${path}`);
      continue;
    }
    files.set(key, path);
    errors.push(...sourceErrors(key, source).map((error) => `${key}: ${error}`));
  }
  for (const [key, reason] of Array.from(exclusions).sort(([left], [right]) => (
    left < right ? -1 : left > right ? 1 : 0
  ))) {
    if (!expectedSet.has(key)) errors.push(`${key}: carve-out is not a public Core function`);
    if (!reason) errors.push(`${key}: carve-out has no reason`);
    if (files.has(key)) errors.push(`${key}: has both a program and a carve-out`);
  }
  const missing = expectedRows.filter((key) => !files.has(key) && !exclusions.has(key));
  return {
    errors: errors.sort(),
    exclusions,
    files,
    missing,
  };
}

function audit() {
  const expected = inventory();
  const witnesses = walk(CORPUS).map((path) => ({
    key: keyForPath(path),
    path,
    source: readFileSync(path, "utf8"),
  }));
  const result = auditEntries(expected, witnesses, parseExclusions());
  const { errors, exclusions, files, missing } = result;
  console.log(`core conformance denominator: ${expected.length} public function(s); ${files.size} program(s); ${exclusions.size} carve-out(s); ${missing.length} uncovered row(s)`);
  for (const key of missing) console.log(`  ${key}`);
  for (const error of errors) console.error(`error: ${error}`);
  return missing.length || errors.length ? 1 : 0;
}

function unitObserver(expression, indent) {
  const body = expression
    .trim()
    .split(/\r?\n/)
    .map((line) => `${indent}    ${line.trim()}`)
    .join("\n");
  return `(() -> {\n${body}\n${indent}    true\n${indent}})()`;
}

function lineRange(source, index) {
  const start = source.lastIndexOf("\n", index - 1) + 1;
  const endAt = source.indexOf("\n", index);
  return { start, end: endAt < 0 ? source.length : endAt };
}

function canonicalizeUnitWitness(key, source) {
  if (!UNIT_RESULT_KEYS.has(key)) return source;
  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const name = key.slice(dot + 1);
  const code = codeOnly(source);
  const usePattern = new RegExp(
    `^\\s*use\\s+${escapedRegExp(module)}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)[ \\t]*(?:;[ \\t]*)?\\r?$`,
    "gm",
  );
  const aliases = Array.from(code.matchAll(usePattern));
  if (aliases.length !== 1) return source;
  const alias = aliases[0][1];
  const call = new RegExp(
    `(?<![A-Za-z0-9_.])${escapedRegExp(alias)}\\s*\\.\\s*${escapedRegExp(name)}\\s*(?:<[^{}]*>\\s*)?\\(`,
    "g",
  );
  const calls = Array.from(code.matchAll(call));
  if (calls.length !== 1) return source;
  const callStart = calls[0].index;
  const callOpen = callStart + calls[0][0].lastIndexOf("(");
  let callClose;
  try {
    callClose = matching(code, callOpen, "(", ")");
  } catch {
    return source;
  }
  const observers = observerCalls(code);
  const directObserver = observers.find(({ open, close }) => open < callStart && callStart < close);
  if (directObserver) {
    const argument = source.slice(directObserver.open + 1, directObserver.close);
    const argumentCode = code.slice(directObserver.open + 1, directObserver.close).trim();
    if (argumentCode.startsWith("(() ->")) {
      const staleNames = ["result", "changed"].filter(
        (value) => !new RegExp(`(?:^|[{};\\n])\\s*${escapedRegExp(value)}\\s*(?:::|:=)`).test(code),
      );
      if (staleNames.length === 0) return source;
      const staleLine = new RegExp(
        `^[ \\t]*print\\((${staleNames.map(escapedRegExp).join("|")})\\)[ \\t]*(?:\\r?\\n|$)`,
        "gm",
      );
      const tailStart = directObserver.close + 1;
      const tail = source.slice(tailStart).replace(staleLine, "");
      return `${source.slice(0, tailStart)}${tail}`;
    }
    const targetCode = code.slice(callStart, callClose + 1).trim();
    if (!argumentCode.startsWith(targetCode)) return source;
    const suffix = argumentCode.slice(targetCode.length).trim();
    const suffixIsPropagation = /^\?\?\s*panic\s*\(/.test(suffix);
    const suffixIsAdditionalArgument = /^,/.test(suffix);
    if (suffix && !suffixIsPropagation && !suffixIsAdditionalArgument) return source;
    const { start } = lineRange(source, directObserver.open);
    const indent = source.slice(start).match(/^\s*/)[0];
    const expression = suffixIsAdditionalArgument
      ? source.slice(callStart, callClose + 1)
      : argument;
    const marker = unitObserver(expression, indent);
    const rewrittenArgument = suffixIsAdditionalArgument
      ? `${marker}${argument.slice(argument.indexOf(source.slice(callClose + 1, directObserver.close)))}`
      : marker;
    return `${source.slice(0, directObserver.open + 1)}${rewrittenArgument}${source.slice(directObserver.close)}`;
  }

  const { start: callLineStart, end: callLineEnd } = lineRange(source, callStart);
  const callLine = source.slice(callLineStart, callLineEnd);
  const binding = callLine.match(/^(\s*)(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*(.*?)\s*$/);
  if (!binding) return source;
  const expression = binding[3].trim();
  const expressionCode = code.slice(callLineStart, callLineEnd).match(
    /^\s*@?[A-Za-z_][A-Za-z0-9_]*\s*(?:::|:=)\s*(.*?)\s*$/,
  )?.[1]?.trim();
  const targetCode = code.slice(callStart, callClose + 1).trim();
  if (!expressionCode || !expressionCode.startsWith(targetCode)) return source;
  const suffix = expressionCode.slice(targetCode.length).trim();
  if (suffix && !/^\?\?\s*panic\s*\(/.test(suffix)) return source;
  const value = binding[2];
  const observer = observers.find(
    ({ open, close }) =>
      open > callClose &&
      new RegExp(`(?<![A-Za-z0-9_])${escapedRegExp(value)}(?![A-Za-z0-9_])`).test(
        code.slice(open + 1, close),
      ),
  );
  if (!observer) return source;
  const observerArgument = source.slice(observer.open + 1, observer.close).trim();
  const valuePattern = new RegExp(`^${escapedRegExp(value)}(?:\\s*,\\s*(.*))?$`);
  const observerMatch = observerArgument.match(valuePattern);
  if (!observerMatch) return source;
  const indent = binding[1];
  const marker = unitObserver(expression, indent);
  const bindingReplacement = `${indent}print(${marker})`;
  const observerLine = lineRange(source, observer.open);
  const observerLineText = source.slice(observerLine.start, observerLine.end);
  const observerCallText = observerLineText.trim();
  const observerText = `${observer.operation}(${observerArgument})`;
  const removeObserverLine = observerCallText === observerText;
  const edits = [
    { start: callLineStart, end: callLineEnd, text: bindingReplacement },
  ];
  if (removeObserverLine) {
    edits.push({
      start: observerLine.start,
      end: observerLine.end < source.length ? observerLine.end + 1 : observerLine.end,
      text: "",
    });
  } else if (observerMatch[1]) {
    edits.push({
      start: observer.open + 1,
      end: observer.close,
      text: observerMatch[1],
    });
  }
  edits.sort((left, right) => right.start - left.start);
  let rewritten = source;
  for (const edit of edits) rewritten = `${rewritten.slice(0, edit.start)}${edit.text}${rewritten.slice(edit.end)}`;
  return rewritten;
}

function normalizeUnitObservers(expected) {
  for (const key of UNIT_RESULT_KEYS) {
    if (!expected.has(key)) throw new Error(`Unit observer key is not public Core: ${key}`);
  }
  let normalized = 0;
  for (const path of walk(CORPUS)) {
    const key = keyForPath(path);
    if (!UNIT_RESULT_KEYS.has(key)) continue;
    const source = readFileSync(path, "utf8");
    const rewritten = canonicalizeUnitWitness(key, source);
    if (rewritten === source) continue;
    writeFileSync(path, rewritten);
    normalized += 1;
  }
  return normalized;
}

function generate() {
  const expected = new Set(inventory());
  for (const key of UNIT_RESULT_KEYS) {
    if (!expected.has(key)) throw new Error(`Unit observer key is not public Core: ${key}`);
  }
  let generated = 0;
  for (const [key, source] of RECIPES) {
    if (!expected.has(key)) throw new Error(`recipe names non-public Core function: ${key}`);
    const dot = key.lastIndexOf(".");
    const path = join(CORPUS, key.slice(0, dot).replaceAll(".", "/"), `${key.slice(dot + 1)}.jet`);
    if (existsSync(path)) continue;
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, source);
    generated += 1;
  }
  const normalized = normalizeUnitObservers(expected);
  console.log(
    `core conformance generator: emitted ${generated} seed program(s) from ${RECIPES.size} explicit recipe(s); normalized ${normalized} Unit observer(s)`,
  );
  return audit();
}

function hostileFixtures() {
  const walkFixture = mkdtempSync(join(ROOT, "core-conformance-hostile-"));
  try {
    const visible = join(walkFixture, "visible", "ordinary.jet");
    const manifest = join(walkFixture, "visible", "package.jet");
    const hidden = join(walkFixture, "visible", ".jet", "receipts", ".package.jet");
    const hiddenFile = join(walkFixture, "visible", ".ghost.jet");
    mkdirSync(dirname(visible), { recursive: true });
    mkdirSync(dirname(hidden), { recursive: true });
    writeFileSync(visible, "");
    writeFileSync(manifest, "manifest");
    writeFileSync(hidden, "");
    writeFileSync(hiddenFile, "");
    const discovered = walk(walkFixture);
    if (discovered.length !== 1 || discovered[0] !== visible) {
      throw new Error("walk included hidden file/receipt or visible package manifest as a witness");
    }
    const ordinaryErrors = sourceErrors("core.fake.ordinary", "");
    if (!ordinaryErrors.includes("file must start with // core-conformance: core.fake.ordinary")) {
      throw new Error("ordinary witness without a marker was accepted");
    }
  } finally {
    rmSync(walkFixture, { recursive: true, force: true });
  }

  const key = "core.crypto.uuid.v4";
  const valid = `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print(result)
}
`;
  const cases = [
    [
      "bind-and-discard",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    _result :: uuid.v4()
    print("ok")
}
`,
      "discard name",
    ],
    [
      "observerless binding",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    sink(result)
}
`,
      "never consumed",
    ],
    [
      "direct unobserved call",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    uuid.v4()
}
`,
      "direct result is not consumed",
    ],
    [
      "comment call ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    // uuid.v4()
    print("ok")
}
`,
      "expected one core.crypto.uuid.v4 call, found 0",
    ],
    [
      "string call ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print("uuid.v4()")
}
`,
      "expected one core.crypto.uuid.v4 call, found 0",
    ],
    [
      "comment observer ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    /* print(result) */
}
`,
      "never consumed",
    ],
    [
      "string observer ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print("result")
}
`,
      "never consumed",
    ],
    [
      "malformed marker",
      `// core-conformance: ${key} extra
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4())
}
`,
      "marker names",
    ],
    [
      "malformed call shape",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4)
}
`,
      "expected one core.crypto.uuid.v4 call, found 0",
    ],
    [
      "unbalanced call",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4(}
}
`,
      "malformed core.crypto.uuid.v4 call",
    ],
    [
      "duplicate call",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4())
    print(uuid.v4())
}
`,
      "expected one core.crypto.uuid.v4 call, found 2",
    ],
    [
      "direct Unit return propagation",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    uuid.v4() ?? return Err("uuid")
    print("ok")
}
`,
      "Unit run cannot use ?? return Err(...) propagation",
    ],
    [
      "bound Unit return propagation",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4() ?? return Err("uuid")
    print(result)
}
`,
      "Unit run cannot use ?? return Err(...) propagation",
    ],
  ];
  for (const [label, source, expected] of cases) {
    const errors = sourceErrors(key, source);
    if (!errors.some((error) => error.includes(expected))) {
      throw new Error(`${label} fixture was accepted: ${errors.join("; ")}`);
    }
  }

  const directPanic = `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    uuid.v4() ?? panic("uuid")
    print("ok")
}
`;
  if (sourceErrors(key, directPanic).length !== 0) {
    throw new Error("direct panic propagation fixture was rejected");
  }

  const interpolated = valid.replace("print(result)", 'print("value={result}")');
  if (sourceErrors(key, interpolated).length !== 0) {
    throw new Error("interpolated observer fixture was rejected");
  }

  const comptime = valid.replace("result ::", "@result ::").replace("print(result)", "print(@result)");
  if (sourceErrors(key, comptime).length !== 0) {
    throw new Error("comptime binding fixture was rejected");
  }

  const generic = `// core-conformance: core.data.csv
use core.data as data
fn run() {
    result :: data.csv<Ticket>("rows")
    print(result)
}
`;
  if (sourceErrors("core.data.csv", generic).length !== 0) {
    throw new Error("generic call fixture was rejected");
  }

  const parsed = parseExclusions("core.fake.one\tplain reason\towner\tD-TEST\n");
  if (parsed.get("core.fake.one") !== "plain reason") {
    throw new Error("owner-ratified exclusion did not return its reason");
  }
  const malformedExclusion = (label, source) => {
    try {
      parseExclusions(source);
    } catch (error) {
      if (error.message.includes("expected key<TAB>reason<TAB>owner<TAB>decision")) return;
      throw error;
    }
    throw new Error(`${label} exclusion was accepted`);
  };
  malformedExclusion("reasonless", "core.fake.one\t\towner\tD-TEST\n");
  malformedExclusion("ownerless", "core.fake.one\tplain reason\t\tD-TEST\n");
  malformedExclusion("decisionless", "core.fake.one\tplain reason\towner\t\n");
  malformedExclusion("extra-field", "core.fake.one\tplain reason\towner\tD-TEST\textra\n");

  const expectedRows = ["core.fake.one", "core.fake.two"];
  const witness = (row, path) => ({ key: row, path, source: `// core-conformance: ${row}
use core.fake as fake
fn run() {
    print(fake.${row.slice(row.lastIndexOf(".") + 1)}())
}
` });
  const assertLedgerError = (label, result, expected) => {
    if (!result.errors.some((error) => error.includes(expected))) {
      throw new Error(`${label} ledger fixture was accepted: ${result.errors.join("; ")}`);
    }
  };
  const missing = auditEntries(expectedRows, [witness("core.fake.one", "core/fake/one.jet")], new Map());
  if (!missing.missing.includes("core.fake.two")) throw new Error("missing ledger row was accepted");
  const duplicateDenominator = auditEntries(["core.fake.one", "core.fake.one"], [], new Map());
  assertLedgerError("duplicate-denominator", duplicateDenominator, "denominator contains a duplicate row");
  const duplicate = auditEntries(
    expectedRows,
    [witness("core.fake.one", "core/fake/one.jet"), witness("core.fake.one", "other/one.jet")],
    new Map(),
  );
  assertLedgerError("duplicate", duplicate, "duplicate files");
  const nonPublic = auditEntries(
    expectedRows,
    [witness("core.fake.ghost", "core/fake/ghost.jet")],
    new Map(),
  );
  assertLedgerError("non-public", nonPublic, "not a public Core function");
  const reasonless = auditEntries(expectedRows, [], new Map([["core.fake.one", ""]]))
  assertLedgerError("reasonless", reasonless, "carve-out has no reason");
  const both = auditEntries(
    expectedRows,
    [witness("core.fake.one", "core/fake/one.jet")],
    new Map([["core.fake.one", "owner-approved test"]]),
  );
  assertLedgerError("witness-plus-exclusion", both, "both a program and a carve-out");

  console.log("core conformance hostile fixtures: rejected hidden/package witnesses, unmarked ordinary files, missing exclusion metadata, bind-and-discard result, observerless/direct calls, comment/string ghosts, malformed marker/calls, and denominator rows");
  return 0;
}

const command = process.argv[2] || "--check";
try {
  if (command === "--generate") process.exitCode = generate();
  else if (command === "--check") process.exitCode = audit();
  else if (command === "--hostile-fixtures") process.exitCode = hostileFixtures();
  else if (command === "--inventory") {
    console.log(JSON.stringify(inventory(), null, 2));
  } else {
    console.error(`usage: ${process.argv[1]} --generate|--check|--hostile-fixtures|--inventory`);
    process.exitCode = 2;
  }
} catch (error) {
  console.error(`error: ${error.message}`);
  process.exitCode = 2;
}
