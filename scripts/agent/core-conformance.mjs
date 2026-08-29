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
  let quote = false;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  for (let i = start; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") lineComment = false;
      continue;
    }
    if (blockComment) {
      if (c === "*" && n === "/") {
        blockComment = false;
        i += 1;
      }
      continue;
    }
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = false;
      continue;
    }
    if (c === '"') {
      quote = true;
      continue;
    }
    if (c === "/" && n === "/") {
      lineComment = true;
      i += 1;
      continue;
    }
    if (c === "/" && n === "*") {
      blockComment = true;
      i += 1;
      continue;
    }
    if (c === opening) depth += 1;
    else if (c === closing && --depth === 0) return i;
  }
  throw new Error(`unbalanced ${opening} at ${start}`);
}

function quoted(text) {
  return Array.from(text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g), (m) => m[1]);
}

function moduleItems() {
  const source = readFileSync(MODULE_ITEMS, "utf8");
  const start = source.indexOf("pub fn core_module_items");
  const end = source.indexOf("/// Ratified nominal types", start);
  if (start < 0 || end < 0) throw new Error("core_module_items source anchors disappeared");
  const body = source.slice(start, end);
  const out = new Map();
  const arms = /^\s*((?:"[^"]+"\s*(?:\|\s*)?)+)=>\s*&\[/gm;
  for (const arm of body.matchAll(arms)) {
    const modules = quoted(arm[1]);
    const opening = body.indexOf("[", arm.index + arm[0].length - 1);
    const close = matching(body, opening, "[", "]");
    const names = quoted(body.slice(opening + 1, close));
    for (const module of modules) {
      if (!out.has(module)) out.set(module, new Set());
      for (const name of names) out.get(module).add(name);
    }
  }

  // core.mem is intentionally a typed gate table instead of a literal match
  // arm. Resolve its string constants from the same source that owns the gate.
  const memSource = readFileSync(MEM_SURFACE, "utf8");
  const memStart = memSource.indexOf("pub const CORE_MEM_GATE_TIERS");
  const memEnd = memSource.indexOf("];", memStart);
  if (memStart < 0 || memEnd < 0) throw new Error("CORE_MEM_GATE_TIERS source anchor disappeared");
  const constants = new Map(
    Array.from(memSource.matchAll(/pub const ([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"]+)"/g),
      (m) => [m[1], m[2]],
    ),
  );
  const mem = new Set();
  for (const name of memSource.slice(memStart, memEnd).matchAll(/\b[A-Z][A-Z0-9_]*\b/g)) {
    if (constants.has(name[0])) mem.add(constants.get(name[0]));
  }
  if (mem.size === 0) throw new Error("CORE_MEM_GATE_TIERS resolved no item names");
  out.set("core.mem", mem);
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
  for (const raw of readFileSync(EXCLUSIONS, "utf8").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const [key, reason = ""] = line.split("\t", 2);
    if (rows.has(key)) throw new Error(`duplicate conformance carve-out: ${key}`);
    rows.set(key, reason.trim());
  }
  return rows;
}

function withoutLineComment(line) {
  let quote = false;
  let escaped = false;
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    const n = line[i + 1];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = false;
    } else if (c === '"') {
      quote = true;
    } else if (c === "/" && n === "/") {
      return line.slice(0, i);
    }
  }
  return line;
}

function lineConsumesValue(line, value) {
  const codeLine = withoutLineComment(line);
  const consumer = /\b(?:print|eprint|assert)\s*\(/.exec(codeLine);
  if (!consumer) return false;
  const valueUse = new RegExp(`\\b${value}\\b`);
  let quote = false;
  let escaped = false;
  let code = "";
  for (let i = consumer.index + consumer[0].length; i < codeLine.length; i += 1) {
    const c = codeLine[i];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = false;
      else if (c === "{") {
        const close = codeLine.indexOf("}", i + 1);
        if (close >= 0 && valueUse.test(codeLine.slice(i + 1, close))) return true;
        if (close >= 0) i = close;
      }
    } else if (c === '"') {
      quote = true;
    } else {
      code += c;
    }
  }
  return valueUse.test(code);
}

function sourceErrors(key, source) {
  const errors = [];
  const marker = source.match(/^\s*\/\/\s*core-conformance:\s*([^\s]+)\s*$/m);
  if (!marker) errors.push("missing // core-conformance: module.function marker");
  else if (marker[1] !== key) errors.push(`marker names ${marker[1]}`);

  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const name = key.slice(dot + 1);
  const alias = source.match(new RegExp(`^\\s*use\\s+${module.replaceAll(".", "\\.")}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)`, "m"));
  if (!alias) {
    errors.push(`missing use ${module} as <alias>`);
    return errors;
  }
  const call = new RegExp(`\\b${alias[1]}\\.${name}\\s*\\(`, "g");
  const calls = Array.from(source.matchAll(call));
  if (calls.length !== 1) {
    errors.push(`expected one ${module}.${name} call, found ${calls.length}`);
    return errors;
  }

  const callStart = calls[0].index;
  const lineStart = source.lastIndexOf("\n", callStart) + 1;
  const beforeCall = source.slice(lineStart, callStart);
  const binding = beforeCall.match(/\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*$/);
  if (binding) {
    const value = binding[1];
    if (value.startsWith("_")) errors.push(`result is bound to discard name ${value}`);
    const callEnd = calls[0].index + calls[0][0].length;
    const after = source.slice(callEnd);
    const consumed = after.split(/\r?\n/).some((line) => lineConsumesValue(line, value));
    if (!consumed) errors.push(`bound result ${value} is never consumed by print/eprint/assert`);
  } else {
    const line = source.slice(lineStart, source.indexOf("\n", callStart) < 0 ? source.length : source.indexOf("\n", callStart));
    if (!/\b(?:print|eprint|assert)\s*\(/.test(line.slice(0, callStart - lineStart))) {
      errors.push("direct result is not consumed by print/eprint/assert");
    }
  }
  return errors;
}

function audit() {
  const expected = inventory();
  const expectedSet = new Set(expected);
  const files = new Map();
  const errors = [];
  for (const path of walk(CORPUS)) {
    const key = keyForPath(path);
    if (!expectedSet.has(key)) {
      errors.push(`${key}: file is not a public Core function`);
      continue;
    }
    if (files.has(key)) {
      errors.push(`${key}: duplicate files ${files.get(key)} and ${path}`);
      continue;
    }
    files.set(key, path);
    const source = readFileSync(path, "utf8");
    errors.push(...sourceErrors(key, source).map((error) => `${key}: ${error}`));
  }
  const exclusions = parseExclusions();
  for (const [key, reason] of exclusions) {
    if (!expectedSet.has(key)) errors.push(`${key}: carve-out is not a public Core function`);
    if (!reason) errors.push(`${key}: carve-out has no reason`);
    if (files.has(key)) errors.push(`${key}: has both a program and a carve-out`);
  }
  const missing = expected.filter((key) => !files.has(key) && !exclusions.has(key));
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
  const fixtures = [
    `// core-conformance: core.crypto.uuid.v4
use core.crypto.uuid as uuid
fn run() {
    _result :: uuid.v4()
    print("ok")
}
`,
    `// core-conformance: core.crypto.uuid.v4
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print("ok")
}
`,
    `// core-conformance: core.crypto.uuid.v4
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print("ok") // result
}
`,
    `// core-conformance: core.crypto.uuid.v4
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print("result")
}
`,
  ];
  const errors = fixtures.map((source) => sourceErrors("core.crypto.uuid.v4", source));
  if (!errors[0].some((error) => error.includes("discard name"))) {
    throw new Error("discard binding was accepted");
  }
  if (!errors.every((fixture) => fixture.some((error) => error.includes("never consumed")))) {
    throw new Error("unused result was accepted");
  }
  console.log("core conformance hostile fixtures: rejected bind-and-discard result and unused binding");
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
