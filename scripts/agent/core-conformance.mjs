#!/usr/bin/env node

import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
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
    result :: time.parse_rfc3339("2024-03-01T12:00:00Z") ?? return Err("parse")
    print(result.to_timestamp())
}
`],
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

function rustStringExpressions(text, constants) {
  const clean = withoutComments(text);
  const names = new Set(quoted(clean));
  for (const match of clean.matchAll(/\b(?:Syntax::)?([A-Z][A-Z0-9_]*)\b/g)) {
    if (constants.has(match[1])) names.add(constants.get(match[1]));
  }
  for (const match of clean.matchAll(/\bSyntax::([A-Z][A-Z0-9_]*)\b/g)) {
    if (!constants.has(match[1])) {
      throw new Error(`unresolved Syntax string constant: ${match[1]}`);
    }
  }
  return Array.from(names);
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
  const arms = /^\s*((?:"[^"]+"\s*(?:\|\s*)?)+)=>\s*&\[/gm;
  for (const arm of body.matchAll(arms)) {
    const modules = rustStringExpressions(arm[1], constants);
    const opening = body.indexOf("[", arm.index + arm[0].length - 1);
    const close = matching(body, opening, "[", "]");
    const names = rustStringExpressions(body.slice(opening + 1, close), constants);
    for (const module of modules) {
      if (!out.has(module)) out.set(module, new Set());
      for (const name of names) out.get(module).add(name);
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
  const mem = new Set(rustStringExpressions(memSource.slice(opening + 1, close), constants));
  if (mem.size === 0) throw new Error("CORE_MEM_GATE_TIERS resolved no item names");
  out.set("core.mem", mem);
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
  return Array.from(new Set(rows)).sort();
}

function walk(dir) {
  if (!existsSync(dir)) return [];
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(path));
    else if (entry.isFile() && entry.name.endsWith(".jet")) files.push(path);
  }
  return files.sort();
}

function keyForPath(path) {
  const rel = relative(CORPUS, path).replaceAll("\\", "/");
  const parts = rel.split("/");
  const name = parts.pop().replace(/\.jet$/, "");
  return `${parts.join(".")}.${name}`;
}

function parseExclusions() {
  if (!existsSync(EXCLUSIONS)) return new Map();
  const rows = new Map();
  for (const [index, raw] of readFileSync(EXCLUSIONS, "utf8").split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const fields = raw.split("\t");
    if (fields.length !== 2 || !fields[0].trim()) {
      throw new Error(`malformed conformance carve-out at line ${index + 1}: expected key<TAB>reason`);
    }
    const key = fields[0].trim();
    if (rows.has(key)) throw new Error(`duplicate conformance carve-out: ${key}`);
    rows.set(key, fields[1].trim());
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
  const valueUse = new RegExp(`\\b${escapedRegExp(value)}\\b`);
  return valueUse.test(codeOnly(text)) || stringInterpolatesValue(text, value);
}

function observerCalls(code) {
  const observers = [];
  const pattern = /(?<![A-Za-z0-9_.])(?:print|eprint|assert)\s*\(/g;
  for (const match of code.matchAll(pattern)) {
    const open = match.index + match[0].lastIndexOf("(");
    try {
      observers.push({ open, close: matching(code, open, "(", ")") });
    } catch {
      // An unbalanced observer cannot prove result consumption.
    }
  }
  return observers;
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
  const call = new RegExp(
    `(?<![A-Za-z0-9_.])${escapedRegExp(alias)}\\s*\\.\\s*${escapedRegExp(name)}\\s*\\(`,
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
  const binding = beforeCall.match(/(?:^|[{};])\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*$/);
  const observers = observerCalls(code);
  if (binding) {
    const value = binding[1];
    if (value.startsWith("_")) errors.push(`result is bound to discard name ${value}`);
    const consumed = observers.some(
      ({ open, close }) => open > callClose && expressionConsumesValue(source.slice(open + 1, close), value),
    );
    if (!consumed) errors.push(`bound result ${value} is never consumed by print/eprint/assert`);
  } else {
    const consumed = observers.some(({ open, close }) => open < callStart && callStart < close);
    if (!consumed) errors.push("direct result is not consumed by print/eprint/assert");
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
  for (const [key, reason] of Array.from(exclusions).sort(([left], [right]) => left.localeCompare(right))) {
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

function generate() {
  const expected = new Set(inventory());
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
  console.log(`core conformance generator: emitted ${generated} seed program(s) from ${RECIPES.size} explicit recipe(s)`);
  return audit();
}

function hostileFixtures() {
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
  ];
  for (const [label, source, expected] of cases) {
    const errors = sourceErrors(key, source);
    if (!errors.some((error) => error.includes(expected))) {
      throw new Error(`${label} fixture was accepted: ${errors.join("; ")}`);
    }
  }

  const interpolated = valid.replace("print(result)", 'print("value={result}")');
  if (sourceErrors(key, interpolated).length !== 0) {
    throw new Error("interpolated observer fixture was rejected");
  }

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

  console.log("core conformance hostile fixtures: rejected bind-and-discard result, observerless/direct calls, comment/string ghosts, malformed marker/calls, and denominator rows");
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
