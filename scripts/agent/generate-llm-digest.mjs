#!/usr/bin/env node
// Generate the one-file Jet surface digest used as agent context.
//
// Usage:
//   node scripts/agent/generate-llm-digest.mjs
//   node scripts/agent/generate-llm-digest.mjs --check
//   node scripts/agent/generate-llm-digest.mjs --stdout
//
// This is a stdlib-only projection of existing compiler registries. It does
// not create a second syntax, marker, diagnostic, or Core authority.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const OUTPUT = join(ROOT, "llms.text");
const PATHS = {
  syntax: [
    "crates/jet-foundation/src/Syntax.rs",
    "crates/jet-foundation/src/Syntax/core_surface.rs",
    "crates/jet-foundation/src/Syntax/effects_surface.rs",
    "crates/jet-foundation/src/Syntax/math_layout.rs",
    "crates/jet-foundation/src/Syntax/package_files.rs",
    "crates/jet-foundation/src/Syntax/predicates.rs",
  ],
  markers: "crates/jet-codegen/src/Prelude/Markers.jet",
  diagnostics: "crates/jet-codegen/src/Prelude/Diagnostics.jet",
  coreModules: "crates/jet-foundation/src/Syntax/predicates.rs",
  coreItems: "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
  canonicalExample: "examples/canon.jet",
};

function read(path) {
  return readFileSync(join(ROOT, path), "utf8").replace(/\r\n/g, "\n");
}

function closeBracket(source, open, opening = "[", closing = "]") {
  let depth = 0;
  let quote = false;
  let escaped = false;
  let lineComment = false;
  for (let index = open; index < source.length; index += 1) {
    const character = source[index];
    const next = source[index + 1];
    if (lineComment) {
      if (character === "\n") lineComment = false;
      continue;
    }
    if (quote) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quote = false;
      continue;
    }
    if (character === '"') {
      quote = true;
      continue;
    }
    if (character === "/" && next === "/") {
      lineComment = true;
      index += 1;
      continue;
    }
    if (character === opening) depth += 1;
    if (character === closing) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  throw new Error(`unclosed ${opening} block`);
}

function stripLineComments(source) {
  return source
    .split("\n")
    .map((line) => {
      let quote = false;
      let escaped = false;
      for (let index = 0; index < line.length - 1; index += 1) {
        const character = line[index];
        if (quote) {
          if (escaped) escaped = false;
          else if (character === "\\") escaped = true;
          else if (character === '"') quote = false;
          continue;
        }
        if (character === '"') {
          quote = true;
          continue;
        }
        if (character === "/" && line[index + 1] === "/") return line.slice(0, index);
      }
      return line;
    })
    .join("\n");
}

function rustString(value) {
  try {
    return JSON.parse(value);
  } catch {
    return value.slice(1, -1).replace(/\\n/g, "\n").replace(/\\t/g, "\t").replace(/\\r/g, "\r").replace(/\\(.)/g, "$1");
  }
}

function stringConstants() {
  const constants = new Map();
  for (const path of PATHS.syntax) {
    const source = read(path);
    for (const match of source.matchAll(/\b(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*([^;]+);/g)) {
      const expression = match[2].trim();
      constants.set(match[1], expression.startsWith('"') ? rustString(expression) : expression.split("::").at(-1));
    }
  }
  return constants;
}

function resolveConstant(name, constants, seen = new Set()) {
  if (!constants.has(name) || seen.has(name)) return undefined;
  const value = constants.get(name);
  if (typeof value !== "string") return value;
  if (value.startsWith('"')) return rustString(value);
  if (value === name) return undefined;
  seen.add(name);
  return constants.has(value) ? resolveConstant(value, constants, seen) : value;
}

function registryList(source, name, constants) {
  const start = source.indexOf(`pub const ${name}:`);
  if (start < 0) throw new Error(`missing syntax registry ${name}`);
  const equals = source.indexOf("=", start);
  const open = source.indexOf("[", equals);
  if (open < 0) throw new Error(`${name} has no list body`);
  const close = closeBracket(source, open);
  const body = stripLineComments(source.slice(open + 1, close));
  const values = [];
  for (const match of body.matchAll(/"((?:\\.|[^"])*)"|\b([A-Za-z_][A-Za-z0-9_]*)\b/g)) {
    const value = match[1] === undefined ? resolveConstant(match[2], constants) : rustString(`"${match[1]}"`);
    if (value !== undefined) values.push(value);
  }
  return [...new Set(values)];
}

function syntaxRegistry() {
  const constants = stringConstants();
  const source = read("crates/jet-foundation/src/Syntax/package_files.rs");
  const reservedSource = read("crates/jet-foundation/src/Syntax/math_layout.rs");
  const coreSource = read(PATHS.coreModules);
  return {
    keywords: registryList(source, "JET_KEYWORD_LIST", constants),
    types: registryList(source, "JET_TYPE_LIST", constants),
    reserved: registryList(reservedSource, "FIRST_PARTY_RESERVED", constants),
    coreModules: registryList(coreSource, "KNOWN_CORE_MODULES", constants),
    constants,
  };
}

function markerRows() {
  return read(PATHS.markers)
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("marker "))
    .map((line) => {
      const name = line.match(/^marker\s+([A-Za-z][A-Za-z0-9_]*)/)?.[1];
      if (!name) throw new Error(`bad marker row: ${line}`);
      return {
        name,
        status: line.includes("@retired:") ? "retired" : "active",
        declaration: line,
      };
    });
}

function unescapeDiagnostic(value) {
  let source = value;
  if (source.startsWith("`` ") && source.endsWith(" ``")) source = source.slice(3, -3);
  let output = "";
  let escaped = false;
  for (const character of source) {
    if (escaped) {
      output += { n: "\n", r: "\r", t: "\t" }[character] ?? character;
      escaped = false;
    } else if (character === "\\") escaped = true;
    else output += character;
  }
  return escaped ? `${output}\\` : output;
}

function oneLine(value) {
  return unescapeDiagnostic(value).replace(/\s+/g, " ").trim();
}

function diagnosticRows() {
  return read(PATHS.diagnostics)
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("//"))
    .map((line) => {
      const fields = line.split("\t");
      if (fields.length !== 12 || fields[0] !== "diagnostic") throw new Error(`bad diagnostic row: ${line}`);
      return {
        code: oneLine(fields[1]),
        stage: oneLine(fields[2]),
        severity: fields[3],
        moment: fields[4],
        status: fields[5],
        meaning: oneLine(fields[6]),
        what: oneLine(fields[7]),
        why: oneLine(fields[8]),
        fix: oneLine(fields[9]),
        detail: oneLine(fields[10]),
        structuredFix: oneLine(fields[11]),
      };
    });
}

function coreModuleItems(syntax) {
  const source = read(PATHS.coreItems);
  const start = source.indexOf("pub fn core_module_items");
  const matchStart = source.indexOf("match module", start);
  if (start < 0 || matchStart < 0) throw new Error("Core item registry not found");
  const matchOpen = source.indexOf("{", matchStart);
  const matchClose = closeBracket(source, matchOpen, "{", "}");
  const body = source.slice(matchOpen + 1, matchClose);
  const result = new Map();
  const arm = /((?:"(?:\\.|[^"])+"\s*(?:\|\s*)?)+|Syntax::[A-Z_]+)\s*=>\s*&\s*\[/g;
  for (const match of body.matchAll(arm)) {
    const names = [];
    for (const literal of match[1].matchAll(/"((?:\\.|[^"])*)"|Syntax::([A-Z_]+)/g)) {
      const name = literal[1] === undefined ? syntax.constants.get(literal[2]) : rustString(`"${literal[1]}"`);
      if (name) names.push(name);
    }
    const open = body.indexOf("[", match.index + match[0].indexOf("["));
    const close = closeBracket(body, open);
    const values = [];
    for (const literal of body.slice(open + 1, close).matchAll(/"((?:\\.|[^"])*)"|\b([A-Za-z_][A-Za-z0-9_]*)\b/g)) {
      const value = literal[1] === undefined ? syntax.constants.get(literal[2]) : rustString(`"${literal[1]}"`);
      if (value) values.push(value);
    }
    for (const name of names) result.set(name, [...new Set(values)]);
  }

  const langNames = new Set(["Site", "Track"]);
  const policy = read("crates/jet-foundation/src/Policy.rs");
  const variantsStart = policy.indexOf("fn canonical_rule_arg_variants");
  const variantsEnd = policy.indexOf("/// Generated from the active/retired", variantsStart);
  const variants = policy.slice(variantsStart, variantsEnd);
  for (const match of variants.matchAll(/"([A-Za-z][A-Za-z0-9_]*)"\s*=>/g)) langNames.add(match[1]);
  result.set("core.lang", [...langNames].sort());

  const mem = read("crates/jet-foundation/src/Syntax/core_surface.rs");
  const memStart = mem.indexOf("pub const CORE_MEM_GATE_TIERS");
  if (memStart >= 0) {
    const equals = mem.indexOf("=", memStart);
    const open = mem.indexOf("[", equals);
    const close = closeBracket(mem, open);
    const values = [];
    for (const match of mem.slice(open + 1, close).matchAll(/\(\s*([A-Z][A-Z0-9_]*)\s*,/g)) {
      const value = syntax.constants.get(match[1]);
      if (value) values.push(value);
    }
    result.set("core.mem", [...new Set(values)]);
  }

  return new Map(syntax.coreModules.map((module) => [module, result.get(module) ?? []]));
}

function section(title, body) {
  return `## ${title}\n\n${body.trim()}\n`;
}

function list(values) {
  return values.map((value) => `- ${value}`).join("\n");
}

function generate() {
  const syntax = syntaxRegistry();
  const markers = markerRows();
  const diagnostics = diagnosticRows();
  const core = coreModuleItems(syntax);
  const canonical = read(PATHS.canonicalExample).trim();
  const markerText = [
    "status\tname\tregistered declaration",
    ...markers.map((row) => `${row.status}\t${row.name}\t${row.declaration}`),
  ].join("\n");
  const diagnosticText = [
    "code\tstatus\tstage\tseverity\tmoment\tmeaning\twhat\twhy\tfix\tdetail\tstructured-fix",
    ...diagnostics.map((row) => [
      row.code,
      row.status,
      row.stage,
      row.severity,
      row.moment,
      row.meaning,
      row.what,
      row.why,
      row.fix,
      row.detail,
      row.structuredFix,
    ].join("\t")),
  ].join("\n");
  const coreText = [
    "module\titems",
    ...[...core].map(([module, items]) => `${module}\t${items.join(", ") || "(no indexed item)"}`),
  ].join("\n");

  const out = [
    "# Jet LLM surface digest",
    "",
    "Generated. Current compiler registries own markers, diagnostics, syntax names, and Core items.",
    "Regenerate with `node scripts/agent/generate-llm-digest.mjs`; CI compares the bytes.",
    "Use active rows only. Retired rows teach replacement; they are not valid current source.",
    "Write one current program. Do not invent aliases, legacy spellings, or library namespaces.",
    "",
    section("First program", [
      "A source file ends with one `fn run()` entry. `print` is a built-in.",
      "",
      "```jet",
      "fn run() {",
      '    greeting :: "Hello, Jet"',
      "    print(greeting)",
      "}",
      "```",
      "",
      "No semicolons. Comments start with `//`. Strings use double quotes and interpolate `{name}`.",
    ].join("\n")),
    section("Core source rules", [
      "Bindings: `name :: value` is immutable; `name := value` is mutable; `name = value` reassigns a mutable binding.",
      "Functions: `fn name(parameter: Type) => Return { ... }`; expression bodies use `:: expression`.",
      "Visibility: declarations are private by default; prefix an item with `pub` for package use.",
      "Types: `Int`, `Float`, `Bool`, `String`, `Char`; lists use `[T]`; optional values use `T?`; failures use `T ? E`.",
      "Errors: handle `T?` or `T ? E` with `?? fallback`, `?`, or a pattern test. Use `Ok(value)`, `Err(error)`, `Val(value)`, and `None`.",
      "Control: `if condition { ... } else { ... }`; collecting loops use `loop name, source { ... }`; exit with `break` and advance with `next`.",
      "Construction: use `Type.{ field: value }`; list literals use `[T].{ value1, value2 }`.",
      "Calls and member access use `name(args)` and `value.member(args)`. Core imports use `use core.module as alias`.",
      "Ownership is safe by default. `&T` writes, `^T` moves, and `~value` copies. Expert unsafe code needs `#Unsafe(\"reason\")`.",
    ].join("\n")),
    section("Canonical compiling example", [
      "Read this as working source syntax. It is the checked executable showcase in `examples/canon.jet`.",
      "",
      "```jet",
      canonical,
      "```",
    ].join("\n")),
    section("Keywords", list(syntax.keywords)),
    section("Built-in type names", list(syntax.types)),
    section("Reserved first-party names", list(syntax.reserved)),
    section("Markers", [
      "User marker spelling is `#Name(arguments)`; rows below are registry declarations.",
      "",
      "```text",
      markerText,
      "```",
    ].join("\n")),
    section("Core module index", [
      "Use a module alias, then call an indexed item: `use core.io as io`; `io.print(\"hi\")`.",
      "",
      "```text",
      coreText,
      "```",
    ].join("\n")),
    section("Diagnostics", [
      "Diagnostic rows use current registry meaning. Match code first; follow `fix`. Rows marked retired or reserved are not current syntax.",
      "",
      "```text",
      diagnosticText,
      "```",
    ].join("\n")),
  ].join("\n");
  return `${out.trimEnd()}\n`;
}

const output = generate();
const args = new Set(process.argv.slice(2));
if (args.has("--stdout")) {
  process.stdout.write(output);
} else if (args.has("--check")) {
  const actual = readFileSync(OUTPUT, "utf8").replace(/\r\n/g, "\n");
  if (actual !== output) {
    console.error(`${OUTPUT} is stale; run generator to refresh it`);
    process.exitCode = 1;
  }
} else {
  writeFileSync(OUTPUT, output);
}
