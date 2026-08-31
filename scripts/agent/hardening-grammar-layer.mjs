#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalJson, makeResultBundle, serializeBundles, sha256 } from "./hardening-oracle-layer.mjs";

/**
 * Layer 3 (#2342).  The manifest is derived from compiler source and the
 * generated programs are deliberately small.  Execution is delegated to the
 * existing runner's parser/sema/TIR/tier commands.
 */

export const GRAMMAR_SCHEMA = "jet.hardening.grammar.v1";
export const GRAMMAR_SCHEMA_VERSION = 1;
export const GRAMMAR_MUTATOR_VERSION = "grammar-generator-1";
export const GRAMMAR_DEFAULT_SEED = "2342";
export const GRAMMAR_DEFAULT_MAX_CASES = 128;
export const GRAMMAR_MAX_CASES = 1024;

const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);
const SOURCE_LIMIT = 512 * 1024;
const CONSTRUCT_ID = /^(?:family|syntax|parser|sema):[A-Za-z_][A-Za-z0-9_-]*$/;
const DIAGNOSTIC_CODE = /^(?:E|L|JT)(?:\d{4}|(?:-[A-Z][A-Z0-9]*){2,})$/;
const BUILTIN_REGISTERED_DIAGNOSTICS = new Set([
  "E0003", "E0104", "E0110", "E0121", "E0124", "E0212", "E0306", "E0505",
  "E0740", "E0904", "E0906", "E0989", "E3410",
]);

const DEFAULT_ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));

function registeredDiagnostics(root = DEFAULT_ROOT) {
  const codes = new Set(BUILTIN_REGISTERED_DIAGNOSTICS);
  const path = join(root, "docs/spec/diagnostics.md");
  if (!existsSync(path)) return codes;
  try {
    for (const match of readFileSync(path, "utf8").matchAll(/\b(?:E|L|JT)(?:\d{4}|(?:-[A-Z][A-Z0-9]*){2,})\b/g)) codes.add(match[0]);
  } catch {
    // Fixture-only manifests still have the small built-in registry above.
  }
  return codes;
}

const REGISTERED_DIAGNOSTICS = registeredDiagnostics();

export function diagnosticRegistryHash() {
  return digest([...REGISTERED_DIAGNOSTICS].sort((left, right) => left.localeCompare(right)));
}

function clone(value) {
  if (value === undefined) return undefined;
  return JSON.parse(JSON.stringify(value));
}

function freezeDeep(value) {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) return value;
  for (const child of Object.values(value)) freezeDeep(child);
  return Object.freeze(value);
}

function stable(value) {
  return canonicalJson(value);
}

function sourceHash(source) {
  return sha256(source);
}

function sortedUnique(values) {
  return [...new Set(values.filter((value) => typeof value === "string" && value.length > 0))]
    .sort((left, right) => left.localeCompare(right));
}

function digest(value) {
  return sha256(stable(value));
}

function boundedMaxCases(value) {
  const result = value ?? GRAMMAR_DEFAULT_MAX_CASES;
  if (!Number.isInteger(result) || result < 1 || result > GRAMMAR_MAX_CASES) throw new Error(`grammar maxCases must be an integer from 1 through ${GRAMMAR_MAX_CASES}`);
  return result;
}

function hashSeed(seed) {
  let value = 2166136261;
  for (const char of String(seed)) value = Math.imul(value ^ char.charCodeAt(0), 16777619);
  return value >>> 0 || 1;
}

function nextValue(state) {
  state.value = (Math.imul(state.value, 1664525) + 1013904223) >>> 0;
  return state.value;
}

function tierList(value) {
  const tiers = value == null ? TIERS : [...new Set(value)];
  if (value != null && tiers.length !== value.length) throw new Error("grammar applicable tiers are duplicated");
  if (tiers.length === 0 || tiers.some((tier) => !TIERS.includes(tier))) throw new Error("grammar applicable tiers are invalid");
  return tiers;
}

/*
 * These are construct families, not a second grammar.  Their templates use
 * only the current canonical surface and always consume a value at print.
 */
export const CONSTRUCT_FAMILIES = freezeDeep([
  {
    id: "expressions",
    syntax_tags: ["expression", "literal", "call", "method", "binary", "type"],
    type_constraints: ["Int", "String", "Bool"],
    valid_templates: [
      "fn add(left: Int, right: Int) Int -> left + right\nfn run() {\n    value :: add(1, 2)\n    print(value)\n}\n",
      "fn run() {\n    value :: if true -> 1 else -> 0\n    print(value)\n}\n",
      "fn choose(flag: Bool) Int -> if flag -> 1 else -> 0\nfn run() {\n    value :: choose(true)\n    print(value)\n}\n",
      "fn length(text: String) Int -> text.len()\nfn run() {\n    value :: length(\"jet\")\n    print(value)\n}\n",
      "fn run() {\n    value :: \"jet\"\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "remove-expression-operand", violated_property: "binary expressions require both operands" },
      { id: "close-call-early", violated_property: "calls require a closing delimiter" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "statements",
    syntax_tags: ["statement", "binding", "assignment", "return"],
    type_constraints: ["Int", "Unit"],
    valid_templates: [
      "fn run() {\n    value := 1\n    value += 1\n    print(value)\n}\n",
      "fn value() Int -> {\n    return 3\n}\nfn run() {\n    value :: value()\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "drop-binding-value", violated_property: "bindings require a value" },
      { id: "close-statement-call", violated_property: "statement calls require a closing delimiter" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "control-flow",
    syntax_tags: ["if", "loop", "match", "when"],
    type_constraints: ["Bool", "Int"],
    valid_templates: [
      "fn run() {\n    value :: if true -> 1 else -> 0\n    print(value)\n}\n",
      "fn classify(value: Int) Int -> {\n    if value > 0 -> 1 else -> 0\n}\nfn run() {\n    value :: classify(1)\n    print(value)\n}\n",
      "fn run() {\n    sum := 0\n    loop i in 0..<3 -> sum += i\n    value :: sum\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "remove-condition", violated_property: "conditional branches require a condition" },
      { id: "unclosed-branch", violated_property: "branches require a closing delimiter" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "patterns",
    syntax_tags: ["pattern", "destructure", "dispatch"],
    type_constraints: ["Int", "String", "Tuple"],
    valid_templates: [
      "fn run() {\n    text :: \"one\"\n    value :: if text == {\n        \"one\" -> 1\n        else -> 0\n    }\n    print(value)\n}\n",
      "fn run() {\n    value :: if 7 == {\n        7 | 8 -> 1\n        else -> 0\n    }\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "bind-invalid-pattern", violated_property: "pattern bindings require a valid pattern" },
      { id: "missing-arm", violated_property: "pattern dispatch requires a matching arm" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "generics",
    syntax_tags: ["generic", "type-argument", "fixed-size"],
    type_constraints: ["T", "Int", "List<T>"],
    valid_templates: [
      "fn id<T>(value: T) T -> value\nfn run() {\n    value :: id<Int>(1)\n    print(value)\n}\n",
      "struct Pair<T> {\n    first: T\n    second: T\n}\nfn make_pair<T>(first: T, second: T) Pair<T> -> {first: ~first, second: ~second}\nfn run() {\n    pair :: Pair<Int>{make_pair(1, 2)}\n    value :: pair.first\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "drop-type-argument", violated_property: "generic calls require valid type arguments" },
      { id: "wrong-arity", violated_property: "calls require the declared argument count" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "traits",
    syntax_tags: ["trait", "impl", "bound", "derive"],
    type_constraints: ["Shape", "Square", "Int"],
    valid_templates: [
      "trait Shape {\n    fn area(self) Int\n    fn name(self) String\n}\nstruct Square {\n    side: Int\n}\nimpl Square.Shape {\n    fn area(self) Int -> self.side * self.side\n    fn name(self) String -> \"square\"\n}\nfn run() {\n    square :: Square{side: 2}\n    value :: square.area()\n    print(value)\n}\n",
      "trait Shape {\n    fn area(self) Int\n    fn name(self) String\n}\nstruct Square {\n    side: Int\n}\nimpl Square.Shape {\n    fn area(self) Int -> self.side * self.side\n    fn name(self) String -> \"square\"\n}\nfn run() {\n    square :: Square{side: 2}\n    value :: square.name()\n    print(value)\n    area :: square.area()\n    print(area)\n}\n",
    ],
    near_valid_mutations: [
      { id: "remove-bound", violated_property: "trait implementations require a complete bound" },
      { id: "unimplemented-method", violated_property: "trait implementations require all declared methods" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "effects",
    syntax_tags: ["effect", "ability", "capability", "handler"],
    type_constraints: ["-[]>", "Int"],
    valid_templates: [
      "fn effect_value() Int -[]> {\n    return 1\n}\nfn run() {\n    value :: effect_value()\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "remove-effect", violated_property: "effect annotations require a complete effect set" },
      { id: "unhandled-effect", violated_property: "effectful calls require a handler" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "views",
    syntax_tags: ["view", "read-view", "mutable-place", "borrow"],
    type_constraints: ["View<Int>", "[Int]", "from"],
    valid_templates: [
      "fn first(values: [Int]) View<Int> from values -> values[0..1]\nfn run() {\n    values :: [7, 8]\n    value :: first(values)[0]\n    print(value)\n}\n",
    ],
    near_valid_mutations: [
      { id: "write-through-view", violated_property: "read views cannot be written through" },
      { id: "use-after-move", violated_property: "moved values cannot be used again" },
    ],
    applicable_tiers: [...TIERS],
  },
  {
    id: "comptime",
    syntax_tags: ["comptime", "static", "compile-time"],
    type_constraints: ["Int", "compile-time binding"],
    valid_templates: ["@value :: 1\nfn run() {\n    value :: @value\n    print(value)\n}\n"],
    near_valid_mutations: [
      { id: "runtime-comptime-call", violated_property: "comptime expressions require compile-time inputs" },
      { id: "non-constant-expression", violated_property: "comptime bindings require constant expressions" },
    ],
    applicable_tiers: ["aot", "jet_run", "interpreter"],
  },
  {
    id: "nested-places",
    syntax_tags: ["index", "field-place", "nested-place", "assignment"],
    type_constraints: ["[Point]", "Int", "indexed field place"],
    valid_templates: ["struct Point {\n    x: Int\n}\nfn run() {\n    points := [Point{x: 1}]\n    points[0].x = 2\n    value :: points[0].x\n    print(value)\n}\n"],
    near_valid_mutations: [
      { id: "index-wrong-type", violated_property: "indexed places require an integer index" },
      { id: "drop-place-base", violated_property: "nested places require a valid base" },
    ],
    applicable_tiers: [...TIERS],
  },
]);

const FAMILY_BY_ID = new Map(CONSTRUCT_FAMILIES.map((family) => [family.id, family]));

function sourceText(pathOrText) {
  if (typeof pathOrText !== "string") return "";
  if (existsSync(pathOrText)) {
    try { return readFileSync(pathOrText, "utf8"); } catch { return ""; }
  }
  if (pathOrText.includes("\n") || pathOrText.includes("{") || pathOrText.includes("pub ")) return pathOrText;
  try { return readFileSync(pathOrText, "utf8"); } catch { return ""; }
}

function commentsBefore(source, offset) {
  const prefix = source.slice(0, offset);
  const lines = prefix.split("\n");
  if (lines.at(-1) === "") lines.pop();
  const comments = [];
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index].trim();
    if (!line) break;
    if (!line.startsWith("///") && !line.startsWith("//")) break;
    comments.unshift(line.replace(/^\/\/\/?\s?/, ""));
  }
  return comments.join(" ");
}

function commentsAfter(source, offset) {
  const lineEnd = source.indexOf("\n", offset);
  const line = source.slice(offset, lineEnd < 0 ? source.length : lineEnd);
  const comment = line.indexOf("//");
  return comment < 0 ? "" : line.slice(comment + 2).trim();
}

function decisionIds(text) {
  return sortedUnique([...String(text || "").matchAll(/\b(?:D-[A-Z0-9-]+|S\d+(?:-[A-Z0-9-]+)?)\b/g)]
    .map((match) => match[0]));
}

function surfaceIsRatified(comment) {
  const text = String(comment || "");
  if (/\b(?:rejected|retired|legacy|foreign-only)\b/i.test(text)) return false;
  return /\bratified\b/i.test(text) || decisionIds(text).length > 0;
}

function decodeString(value) {
  try { return JSON.parse(`"${value}"`); } catch { return value; }
}

function maskRustCommentsAndStrings(source) {
  const blank = (value) => value.replace(/[^\n]/g, " ");
  return String(source)
    .replace(/r#*"[\s\S]*?"#*/g, blank)
    .replace(/\/\*[\s\S]*?\*\//g, blank)
    .replace(/\/\/[^\n]*/g, blank)
    .replace(/"(?:\\.|[^"\\])*"/g, blank)
    .replace(/'(?:\\.|[^'\\])*'/g, blank);
}

function syntaxEntries(source, sourceName = "Syntax.rs") {
  const entries = [];
  const seen = new Set();
  const pattern = /(?:^|\n)[ \t]*(?:(?:pub(?:\([^)]*\))?|crate|super)\s+)?(?:const|static)\s+([A-Z][A-Z0-9_]*)\s*:\s*([^=;]+?)\s*=\s*([\s\S]*?);/g;
  const code = maskRustCommentsAndStrings(source);
  for (const match of code.matchAll(pattern)) {
    const name = match[1];
    const type = match[2].trim();
    if (!/\bstr\b/.test(type)) continue;
    if (seen.has(name)) continue;
    seen.add(name);
    const declarationEnd = code.indexOf(";", match.index + match[0].length - 1);
    const declaration = source.slice(match.index, declarationEnd < 0 ? source.length : declarationEnd);
    const literals = [...declaration.matchAll(/"((?:\\.|[^"\\])*)"/g)].map((item) => decodeString(item[1]));
    const declarationOffset = source.lastIndexOf("\n", match.index) + 1;
    const comment = [commentsBefore(source, declarationOffset), commentsAfter(source, declarationOffset)]
      .filter(Boolean)
      .join(" ");
    const decisions = decisionIds(comment);
    const ratified = surfaceIsRatified(comment);
    const internal = /^(?:INTERNAL|FOREIGN|RETIRED|LEGACY|PRIVATE)_/.test(name);
    const aggregate = /[[(]/.test(type);
    entries.push({
      construct_id: `syntax:${name}`,
      family: inferFamily(`${name} ${literals.join(" ")}`, comment),
      name,
      spelling: literals[0] || name,
      spellings: literals,
      surface_kind: aggregate ? "catalog" : "scalar",
      comment,
      decision_ids: decisions,
      ratified,
      source: sourceName,
      source_kind: "syntax",
      line: source.slice(0, declarationOffset).split("\n").length,
      internal,
    });
  }
  return entries;
}

function functionEntries(source, prefix, sourceName) {
  const entries = [];
  const seen = new Set();
  const pattern = /(?<![A-Za-z0-9_])(?:(?:pub(?:\([^)]*\))?|crate|super)\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/g;
  const code = maskRustCommentsAndStrings(source);
  for (const match of code.matchAll(pattern)) {
    const name = match[1];
    const line = source.slice(0, match.index).split("\n").length;
    const rowKey = `${name}:${line}`;
    if (seen.has(rowKey)) continue;
    seen.add(rowKey);
    const lower = name.toLowerCase();
    if (lower === "run" || lower.startsWith("test") || lower.startsWith("assert")) continue;
    const isProduction = prefix === "parser"
      ? /(?:parse|expr|stmt|item|pattern|type|postfix|primary|collection|control|block)/.test(lower)
      : /(?:check|infer|resolve|type|validate|bind|lower|effect|pattern|place|generic|trait|ownership|statement|expr)/.test(lower);
    const ownedModule = prefix === "parser"
      ? sourceName === "parser" || /(?:^|[/\\])Parser(?:[/\\])/.test(String(sourceName))
      : sourceName === "sema" || /(?:^|[/\\])Sema(?:[/\\])/.test(String(sourceName));
    if (!isProduction && !ownedModule) continue;
    const sourceToken = `${String(sourceName || prefix).replace(/[^A-Za-z0-9_-]+/g, "_")}-${line}-${match.index}`;
    entries.push({
      construct_id: `${prefix}:${name}-${sourceToken}`,
      family: inferFamily(name, sourceName),
      name,
      spelling: name,
      comment: `${prefix} production ${name}`,
      decision_ids: [],
      ratified: true,
      source: sourceName,
      source_kind: prefix,
      line,
      production: isProduction,
    });
  }
  return entries;
}

function inferFamily(primary, secondary = "") {
  const classify = (text) => {
    const value = text.toLowerCase().replace(/[^a-z0-9]+/g, " ");
    const has = (...words) => words.some((word) => new RegExp(`(?:^| )${word}(?: |$)`).test(value));
    if (has("generic", "typearg", "fixedsize")) return "generics";
    if (has("trait", "bound", "derive", "impl", "struct", "enum")) return "traits";
    if (has("effect", "ability", "capability", "handler")) return "effects";
    if (has("index", "field", "assign", "nested")) return "nested-places";
    if (has("view", "borrow", "ownership", "place")) return "views";
    if (has("comptime", "compile", "static")) return "comptime";
    if (has("pattern", "dispatch", "match", "destructure")) return "patterns";
    if (has("if", "loop", "when", "control", "branch")) return "control-flow";
    if (has("statement", "stmt", "binding", "return", "defer")) return "statements";
    return null;
  };
  return classify(primary) || classify(secondary) || "expressions";
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function surfaceInTemplate(spelling, template) {
  if (typeof spelling !== "string" || spelling.length === 0) return false;
  if (/^[A-Za-z0-9_]+$/.test(spelling)) {
    return new RegExp(`(?:^|[^A-Za-z0-9_])${escapeRegExp(spelling)}(?:$|[^A-Za-z0-9_])`).test(template);
  }
  return template.includes(spelling);
}

function templateCoverage(row, family) {
  if (!family) return [];
  if (row.source === "construct-family" || row.source_kind === "parser" || row.source_kind === "sema") {
    return family.valid_templates.map((_, index) => index);
  }
  const spellings = sortedUnique([row.spelling, ...(row.spellings || [])]);
  return family.valid_templates.flatMap((template, index) => (
    spellings.some((spelling) => surfaceInTemplate(spelling, template)) ? [index] : []
  ));
}

function sourceFileDigest(root, paths) {
  return paths.map((path) => ({
    path: sourceLabel(root, path, path),
    sha256: sourceHash(sourceText(path)),
  }));
}

function parserFiles(root) {
  const directory = join(root, "crates/jet-parser/src");
  const out = [];
  const walk = (path) => {
    if (!existsSync(path)) return;
    for (const name of readdirSync(path, { withFileTypes: true })) {
      const child = join(path, name.name);
      if (name.isDirectory()) walk(child);
      else if (name.name.endsWith(".rs")) out.push(child);
    }
  };
  walk(directory);
  return out.sort();
}

function semaFiles(root) {
  const directory = join(root, "crates/jet-sema/src");
  const out = [];
  const walk = (path) => {
    if (!existsSync(path)) return;
    for (const name of readdirSync(path, { withFileTypes: true })) {
      const child = join(path, name.name);
      if (name.isDirectory()) walk(child);
      else if (name.name.endsWith(".rs")) out.push(child);
    }
  };
  walk(directory);
  return out.sort();
}

function syntaxFiles(root) {
  const main = join(root, "crates/jet-foundation/src/Syntax.rs");
  const directory = join(root, "crates/jet-foundation/src/Syntax");
  const out = existsSync(main) ? [main] : [];
  const walk = (path) => {
    if (!existsSync(path)) return;
    for (const name of readdirSync(path, { withFileTypes: true })) {
      const child = join(path, name.name);
      if (name.isDirectory()) walk(child);
      else if (name.name.endsWith(".rs")) out.push(child);
    }
  };
  walk(directory);
  return [...new Set(out)].sort();
}

function sourceLabel(root, path, fallback) {
  if (typeof path !== "string" || !existsSync(path)) return fallback;
  const relativePath = path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path;
  return relativePath || fallback;
}

function sourceIds(rows, predicate = () => true) {
  return sortedUnique(rows.filter(predicate).map((row) => row.construct_id));
}

function coverageIds(snapshot) {
  const sourceIdsByKind = snapshot?.source_ids;
  if (!sourceIdsByKind || typeof sourceIdsByKind !== "object") return null;
  return Object.values(sourceIdsByKind).flatMap((ids) => Array.isArray(ids) ? ids : []);
}

export function deriveConstructManifest({
  root = DEFAULT_ROOT,
  syntaxSource = undefined,
  parserSources = undefined,
  semaSources = undefined,
  includeStaticFamilies = true,
  countedReasons = [],
} = {}) {
  const rows = [];
  const requestedSyntaxPaths = syntaxSource === undefined
    ? []
    : Array.isArray(syntaxSource) ? syntaxSource : [syntaxSource];
  const canonicalSyntaxRoot = join(root, "crates/jet-foundation/src/Syntax.rs");
  const syntaxPaths = syntaxSource === undefined
    || (requestedSyntaxPaths.length === 1 && resolve(requestedSyntaxPaths[0]) === resolve(canonicalSyntaxRoot))
    ? syntaxFiles(root)
    : requestedSyntaxPaths;
  const syntaxRows = [];
  for (const path of syntaxPaths) {
    const text = sourceText(path);
    syntaxRows.push(...syntaxEntries(text, sourceLabel(root, path, "Syntax.rs")));
  }
  rows.push(...syntaxRows);
  const parserPaths = parserSources == null
    ? parserFiles(root)
    : Array.isArray(parserSources) ? parserSources : [parserSources];
  const parserRows = [];
  for (const path of parserPaths) {
    const text = sourceText(path);
    parserRows.push(...functionEntries(text, "parser", sourceLabel(root, path, "parser")));
  }
  rows.push(...parserRows);
  const semaPaths = semaSources == null
    ? semaFiles(root)
    : Array.isArray(semaSources) ? semaSources : [semaSources];
  const semaRows = [];
  for (const path of semaPaths) {
    const text = sourceText(path);
    semaRows.push(...functionEntries(text, "sema", sourceLabel(root, path, "sema")));
  }
  rows.push(...semaRows);
  if (includeStaticFamilies) {
    for (const family of CONSTRUCT_FAMILIES) rows.push({
      construct_id: `family:${family.id}`,
      family: family.id,
      name: family.id,
      spelling: family.syntax_tags[0],
      comment: "ratified construct family",
      ratified: true,
      source: "construct-family",
    });
  }
  const deduped = new Map();
  for (const row of rows) if (!deduped.has(row.construct_id)) deduped.set(row.construct_id, row);
  const manifestRows = [...deduped.values()].sort((left, right) => left.construct_id.localeCompare(right.construct_id)).map((row) => {
    const family = FAMILY_BY_ID.get(row.family);
    const internal = /^(?:INTERNAL|FOREIGN|RETIRED|LEGACY|PRIVATE)_/.test(row.name);
    const coveredTemplates = templateCoverage(row, family);
    const production = ["parser", "sema"].includes(row.source_kind) ? row.production === true : true;
    const executable = Boolean(family)
      && (row.source === "construct-family" || row.ratified === true)
      && production
      && !internal
      && row.surface_kind !== "catalog"
      && coveredTemplates.length > 0;
    const sourceDescription = row.source_kind === "syntax"
      ? `${row.construct_id} (${row.spelling || row.name})`
      : `${row.construct_id} production family ${row.family}`;
    return {
      construct_id: row.construct_id,
      family: row.family,
      name: row.name,
      spelling: row.spelling,
      ...(row.spellings?.length ? { spellings: [...row.spellings] } : {}),
      ...(row.decision_ids?.length ? { decision_ids: [...row.decision_ids] } : {}),
      source: row.source,
      source_kind: row.source_kind || (row.source === "construct-family" ? "family" : "unknown"),
      source_line: row.line || null,
      ...(row.production === undefined ? {} : { production: row.production }),
      surface_kind: row.surface_kind || (row.source === "construct-family" ? "family" : "production"),
      ratified: row.ratified === true,
      valid_templates: executable ? [...family.valid_templates] : [],
      near_valid_mutations: executable ? [...family.near_valid_mutations] : [],
      type_constraints: executable ? [...family.type_constraints] : [],
      applicable_tiers: executable ? [...family.applicable_tiers] : [],
      value_consuming: executable,
      observable_sink: executable
        ? { type: "primitive", operation: "print", expression: "print(value)", type_aware: true }
        : null,
      template_coverage: executable ? coveredTemplates : [],
      reason: executable ? null : `no bounded value-consuming template exercises ${sourceDescription}`,
      owner_visible: true,
      owner_decision: row.decision_ids?.[0] || null,
    };
  });
  const normalizedReasons = (Array.isArray(countedReasons) ? countedReasons : [countedReasons])
    .filter((item) => item && typeof item === "object")
    .map((item) => ({
      construct_id: item.construct_id,
      reason: item.reason,
      owner_visible: item.owner_visible === true,
      owner_decision: item.owner_decision || null,
    }))
    .sort((left, right) => String(left.construct_id).localeCompare(String(right.construct_id)));
  const sourceIdsByKind = {
    syntax: sourceIds(syntaxRows, (row) => row.ratified === true),
    parser: sourceIds(parserRows),
    sema: sourceIds(semaRows),
  };
  const requiredFamilies = CONSTRUCT_FAMILIES.map((family) => family.id);
  const families = CONSTRUCT_FAMILIES.map((family) => family.id);
  const executable = manifestRows.filter((row) => row.value_consuming).length;
  const reasons = manifestRows.filter((row) => !row.value_consuming).length + normalizedReasons.length;
  const denominatorIds = sortedUnique([
    ...manifestRows.map((row) => row.construct_id),
    ...normalizedReasons.map((row) => row.construct_id),
  ]);
  const syntax = syntaxPaths.map((path) => sourceText(path));
  const syntaxSnapshot = syntax.length === 1
    ? sourceHash(syntax[0])
    : digest(syntax.map((text, index) => ({ path: syntaxPaths[index], sha256: sourceHash(text) })));
  return {
    schema: GRAMMAR_SCHEMA,
    schema_version: GRAMMAR_SCHEMA_VERSION,
    source_snapshot: {
      root: String(root),
      syntax_sha256: syntaxSnapshot,
      syntax_files: syntaxPaths.map((path) => sourceLabel(root, path, "Syntax.rs")),
      parser_files: parserPaths.length,
      sema_files: semaPaths.length,
      parser_file_digests: sourceFileDigest(root, parserPaths),
      sema_file_digests: sourceFileDigest(root, semaPaths),
      source_ids: sourceIdsByKind,
      required_families: requiredFamilies,
    },
    coverage: {
      source_ids: sourceIdsByKind,
      counted_reasons: normalizedReasons,
    },
    families,
    rows: manifestRows,
    denominator: {
      total: denominatorIds.length,
      executable,
      counted_reasons: reasons,
      construct_ids: denominatorIds,
      source_ids: sourceIdsByKind,
    },
  };
}

export const constructManifest = deriveConstructManifest;

export function constructManifestHash(manifest) {
  validateConstructManifest(manifest);
  const sourceSnapshot = { ...manifest.source_snapshot };
  delete sourceSnapshot.root;
  return digest({
    schema: manifest.schema,
    schema_version: manifest.schema_version,
    source_snapshot: sourceSnapshot,
    denominator: manifest.denominator,
  });
}

export function validateConstructManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || !Array.isArray(manifest.rows)) throw new Error("construct manifest rows are required");
  if (manifest.schema !== GRAMMAR_SCHEMA || manifest.schema_version !== GRAMMAR_SCHEMA_VERSION) {
    throw new Error("construct manifest schema is unsupported");
  }
  if (!manifest.source_snapshot || typeof manifest.source_snapshot !== "object") throw new Error("construct manifest source snapshot is required");
  const expected = coverageIds(manifest.source_snapshot);
  if (!expected) throw new Error("construct manifest source coverage is required");
  const sourceIdGroups = manifest.source_snapshot.source_ids;
  if (!sourceIdGroups || typeof sourceIdGroups !== "object" || Array.isArray(sourceIdGroups)) {
    throw new Error("construct manifest source id groups are required");
  }
  if (new Set(expected).size !== expected.length) throw new Error("construct manifest source ids are duplicated");
  for (const [kind, ids] of Object.entries(sourceIdGroups)) {
    if (!Array.isArray(ids) || ids.some((constructId) => typeof constructId !== "string" || !CONSTRUCT_ID.test(constructId))) {
      throw new Error(`construct manifest source ids are invalid: ${kind}`);
    }
    if (stable(ids) !== stable(sortedUnique(ids))) throw new Error(`construct manifest source ids are not ordered: ${kind}`);
  }
  if (manifest.coverage?.source_ids !== undefined
    && stable(manifest.coverage.source_ids) !== stable(sourceIdGroups)) {
    throw new Error("construct manifest coverage source ids are stale");
  }
  const countedReasons = manifest.coverage?.counted_reasons
    ?? (Array.isArray(manifest.denominator?.counted_reasons) ? manifest.denominator.counted_reasons : []);
  if (!Array.isArray(countedReasons)) throw new Error("construct manifest counted reasons are required");
  const countedIds = countedReasons.map((reason) => reason?.construct_id);
  if (stable(countedIds) !== stable(sortedUnique(countedIds))) throw new Error("construct manifest counted reasons are not ordered");
  const seen = new Set();
  const rowsById = new Map();
  const rowIds = manifest.rows.map((row) => row?.construct_id);
  if (rowIds.some((constructId) => typeof constructId !== "string")
    || stable(rowIds) !== stable(sortedUnique(rowIds))) throw new Error("construct manifest rows are not ordered");
  for (const row of manifest.rows) {
    if (typeof row.construct_id !== "string" || row.construct_id.length === 0) throw new Error("construct manifest row has no construct_id");
    if (!CONSTRUCT_ID.test(row.construct_id)) throw new Error(`construct manifest identity is invalid: ${row.construct_id}`);
    if (seen.has(row.construct_id)) throw new Error(`duplicate construct identity: ${row.construct_id}`);
    seen.add(row.construct_id);
    rowsById.set(row.construct_id, row);
    if (typeof row.family !== "string" || row.family.length === 0) throw new Error(`construct row has no family: ${row.construct_id}`);
    if (typeof row.ratified !== "boolean") throw new Error(`construct row ratification is missing: ${row.construct_id}`);
    if (typeof row.source_kind !== "string" || !["syntax", "parser", "sema", "family", "production"].includes(row.source_kind)) {
      throw new Error(`construct row source kind is invalid: ${row.construct_id}`);
    }
    if (["parser", "sema"].includes(row.source_kind) && typeof row.production !== "boolean") {
      throw new Error(`production row classification is missing: ${row.construct_id}`);
    }
    if (row.source_kind === "syntax" && row.ratified && !sourceIdGroups.syntax?.includes(row.construct_id)) {
      throw new Error(`ratified syntax row is outside source coverage: ${row.construct_id}`);
    }
    if (["parser", "sema"].includes(row.source_kind) && !sourceIdGroups[row.source_kind]?.includes(row.construct_id)) {
      throw new Error(`production row is outside source coverage: ${row.construct_id}`);
    }
    if (row.surface_kind !== undefined && typeof row.surface_kind !== "string") {
      throw new Error(`construct row surface kind is invalid: ${row.construct_id}`);
    }
    if (typeof row.value_consuming !== "boolean") throw new Error(`construct row value contract is missing: ${row.construct_id}`);
    if (!Array.isArray(row.valid_templates) || row.valid_templates.some((template) => typeof template !== "string" || template.length === 0)
      || !Array.isArray(row.near_valid_mutations) || row.near_valid_mutations.some((mutation) => !mutation
        || typeof mutation !== "object"
        || typeof mutation.id !== "string" || mutation.id.length === 0
        || typeof mutation.violated_property !== "string" || mutation.violated_property.length === 0)
      || !Array.isArray(row.type_constraints) || row.type_constraints.some((constraint) => typeof constraint !== "string" || constraint.length === 0)) {
      throw new Error(`construct row is incomplete: ${row.construct_id}`);
    }
    if (!Array.isArray(row.template_coverage)) throw new Error(`construct row template coverage is missing: ${row.construct_id}`);
    if (row.value_consuming) {
      if (row.valid_templates.length === 0 || !row.observable_sink?.type_aware) throw new Error(`executable construct lacks value-consuming sink: ${row.construct_id}`);
      if (row.template_coverage.length === 0 || row.template_coverage.some((index) => !Number.isInteger(index) || index < 0 || index >= row.valid_templates.length)) {
        throw new Error(`executable construct template coverage is invalid: ${row.construct_id}`);
      }
      if (row.near_valid_mutations.length === 0) throw new Error(`executable construct lacks near-valid mutation: ${row.construct_id}`);
      if (!Array.isArray(row.applicable_tiers) || row.applicable_tiers.length === 0) throw new Error(`executable construct lacks tiers: ${row.construct_id}`);
      tierList(row.applicable_tiers);
      if (!row.observable_sink.expression || !row.valid_templates.some((template) => template.includes(row.observable_sink.expression))) {
        throw new Error(`executable construct sink is not in its template: ${row.construct_id}`);
      }
    } else if (typeof row.reason !== "string" || row.reason.length === 0 || row.owner_visible !== true) {
      throw new Error(`non-executable construct lacks owner-visible reason: ${row.construct_id}`);
    } else if (row.template_coverage.length !== 0) {
      throw new Error(`non-executable construct claims template coverage: ${row.construct_id}`);
    }
  }
  const countedById = new Map();
  for (const reason of countedReasons) {
    if (!reason || typeof reason !== "object" || typeof reason.construct_id !== "string") throw new Error("counted reason identity is missing");
    if (!expected.includes(reason.construct_id)) throw new Error(`counted reason is outside source coverage: ${reason.construct_id}`);
    if (rowsById.has(reason.construct_id)) throw new Error(`counted reason duplicates construct row: ${reason.construct_id}`);
    if (countedById.has(reason.construct_id)) throw new Error(`duplicate counted reason: ${reason.construct_id}`);
    if (typeof reason.reason !== "string" || reason.reason.length === 0 || reason.owner_visible !== true) {
      throw new Error(`counted reason is not owner-visible: ${reason.construct_id}`);
    }
    countedById.set(reason.construct_id, reason);
  }
  const uncovered = expected.filter((constructId) => !rowsById.has(constructId) && !countedById.has(constructId));
  if (uncovered.length) throw new Error(`construct manifest source coverage is missing: ${uncovered.join(", ")}`);
  const requiredFamilies = manifest.source_snapshot.required_families;
  if (!Array.isArray(requiredFamilies) || requiredFamilies.some((family) => typeof family !== "string" || !family)) {
    throw new Error("construct manifest required families are missing");
  }
  const canonicalFamilies = CONSTRUCT_FAMILIES.map((family) => family.id);
  if (stable(requiredFamilies) !== stable(canonicalFamilies)) throw new Error("construct manifest required families are stale");
  if (manifest.families !== undefined && stable(manifest.families) !== stable(canonicalFamilies)) {
    throw new Error("construct manifest families are stale");
  }
  const presentFamilies = new Set(manifest.rows.map((row) => row.family));
  const missingFamilies = requiredFamilies.filter((family) => !presentFamilies.has(family));
  if (missingFamilies.length) throw new Error(`construct manifest production families are missing: ${missingFamilies.join(", ")}`);
  const denominatorIds = [...manifest.rows.map((row) => row.construct_id), ...countedReasons.map((row) => row.construct_id)];
  if (new Set(denominatorIds).size !== denominatorIds.length) throw new Error("construct denominator is duplicated");
  if (manifest.denominator?.total !== denominatorIds.length
    || stable(manifest.denominator?.construct_ids) !== stable(denominatorIds)) throw new Error("construct denominator is stale");
  if (manifest.denominator?.executable !== manifest.rows.filter((row) => row.value_consuming).length) {
    throw new Error("construct denominator executable count is stale");
  }
  if (manifest.denominator?.counted_reasons !== manifest.rows.filter((row) => !row.value_consuming).length + countedReasons.length) {
    throw new Error("construct denominator reason count is stale");
  }
  if (manifest.denominator?.source_ids !== undefined
    && stable(manifest.denominator.source_ids) !== stable(sourceIdGroups)) {
    throw new Error("construct denominator source ids are stale");
  }
  return true;
}

function sourceFor(row, template, index) {
  const suffix = `// generated construct ${row.construct_id} case ${index}\n`;
  const source = template.endsWith("\n") ? `${template}${suffix}` : `${template}\n${suffix}`;
  if (Buffer.byteLength(source, "utf8") > SOURCE_LIMIT) throw new Error(`generated construct exceeds ${SOURCE_LIMIT} bytes: ${row.construct_id}`);
  return source;
}

function balancedSource(source) {
  const opening = new Set(["(", "[", "{"]);
  const closing = new Map([["}", "{"], ["]", "["], [")", "("]]);
  const stack = [];
  let quote = false;
  let escaped = false;
  let lineComment = false;
  let blockComment = false;
  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];
    if (lineComment) {
      if (char === "\n") lineComment = false;
      continue;
    }
    if (blockComment) {
      if (char === "*" && next === "/") {
        blockComment = false;
        index += 1;
      }
      continue;
    }
    if (quote) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quote = false;
      continue;
    }
    if (char === "/" && next === "/") {
      lineComment = true;
      index += 1;
      continue;
    }
    if (char === "/" && next === "*") {
      blockComment = true;
      index += 1;
      continue;
    }
    if (char === '"') {
      quote = true;
      continue;
    }
    if (opening.has(char)) stack.push(char);
    else if (closing.has(char) && stack.pop() !== closing.get(char)) return false;
  }
  return !quote && !blockComment && stack.length === 0;
}

function hasRunFunction(source) {
  return /(?:^|\n)[ \t]*fn\s+run\s*\(/.test(source);
}

function hasObservableSink(source, sink) {
  return sink?.type_aware === true
    && sink.type === "primitive"
    && sink.operation === "print"
    && typeof sink.expression === "string"
    && sink.expression.length > 0
    && source.includes(sink.expression);
}

function nearValidSource(source, mutation) {
  const fallback = (replacement) => replacement === source ? `${source}\nfn invalid(\n` : replacement;
  const removeLastBrace = () => {
    const offset = source.lastIndexOf("}");
    return offset < 0 ? fallback(source) : `${source.slice(0, offset)}${source.slice(offset + 1)}`;
  };
  switch (mutation) {
    case "remove-expression-operand": return fallback(source.replace("1 + 2", "1 +"));
    case "close-call-early": return fallback(source.replace("print(value)", "print(value"));
    case "close-statement-call": return fallback(source.replace("print(value)", "print(value"));
    case "drop-binding-value": return fallback(source.replace(/value\s*(?:::\s*|:=\s*)1/, "value ::"));
    case "duplicate-return": return fallback(source.replace("print(value)", "print(value"));
    case "remove-condition": return fallback(source.replace("if true", "if"));
    case "unclosed-branch": return removeLastBrace();
    case "bind-invalid-pattern": return fallback(source.replace("print(value)", "print(value"));
    case "missing-arm": return removeLastBrace();
    case "drop-type-argument": return fallback(source.replace("id<Int>(1)", "id<>(1)").replace("id(1)", "id<>(1)"));
    case "wrong-arity": return fallback(source.replace("id<Int>(1)", "id<Int>(1, 2)").replace("id(1)", "id(1, 2)"));
    case "remove-bound": return fallback(source.replace("impl Square.Shape", "impl Square.Shape { fn area(self) Int ->"));
    case "unimplemented-method": return `${source}\nstruct Empty {}\nimpl Empty.Shape {}\n`;
    case "remove-effect": return fallback(source.replace("-[IO]>", "-[IO>").replace("-[]>", "-[>"));
    case "unhandled-effect": return fallback(source.replace("print(value)", "print(read_file())"));
    case "write-through-view": return fallback(source.replace("print(value)", "values[0] = 9\n    print(value)"));
    case "use-after-move": return fallback(source.replace("print(value)", "values = [9, 10]\n    print(value)"));
    case "runtime-comptime-call": return fallback(source.replace("print(value)", "@if input() -> print(value)\n    print(value)"));
    case "non-constant-expression": return fallback(source.replace("@value :: 1", "@value :: input()"));
    case "index-wrong-type": return fallback(source.replace(/\b(values|points)\[0\]/, "$1[\"x\"]"));
    case "drop-place-base": return fallback(source.replace("points[0]", "points[\"x\"]").replace("values[0]", "values[\"x\"]"));
    default: return fallback(source);
  }
}

function nearValidArm(mutation) {
  const id = typeof mutation === "string" ? mutation : mutation?.id;
  return CONSTRUCT_FAMILIES.flatMap((family) => family.near_valid_mutations)
    .find((arm) => arm.id === id) || null;
}

export function diagnosticFor(mutation) {
  const arm = nearValidArm(mutation);
  return arm
    ? { violated_property: arm.violated_property, class: "near-valid-generated-program" }
    : { violated_property: null, class: "near-valid-generated-program" };
}

function shuffled(values, state) {
  const result = [...values];
  for (let index = result.length - 1; index > 0; index -= 1) {
    const swap = nextValue(state) % (index + 1);
    [result[index], result[swap]] = [result[swap], result[index]];
  }
  return result;
}

function candidateOrder(candidates, state) {
  const familyRows = new Map(candidates
    .filter((row) => row.source === "construct-family")
    .map((row) => [row.family, row]));
  const guaranteed = CONSTRUCT_FAMILIES
    .map((family) => familyRows.get(family.id))
    .filter(Boolean);
  const rest = candidates.filter((row) => !guaranteed.includes(row));
  return [...guaranteed, ...shuffled(rest, state)];
}

export function generateTypedPrograms(manifest, {
  seed = GRAMMAR_DEFAULT_SEED,
  maxCases = GRAMMAR_DEFAULT_MAX_CASES,
  includeNearValid = false,
} = {}) {
  const max = boundedMaxCases(maxCases);
  validateConstructManifest(manifest);
  const state = { value: hashSeed(seed) };
  const candidates = manifest.rows.filter((row) => row.value_consuming && row.valid_templates.length > 0);
  const orderedCandidates = candidateOrder(candidates, state);
  const rejected = manifest.rows.filter((row) => !row.value_consuming).map((row) => ({
    construct_id: row.construct_id,
    reason: row.reason,
  }));
  rejected.push(...(manifest.coverage?.counted_reasons || []).map((row) => ({
    construct_id: row.construct_id,
    reason: row.reason,
  })));
  const programs = [];
  const manifest_sha256 = constructManifestHash(manifest);
  for (let index = 0; programs.length < max && orderedCandidates.length > 0; index += 1) {
    const row = orderedCandidates[index % orderedCandidates.length];
    const family = FAMILY_BY_ID.get(row.family);
    const coveredTemplates = row.template_coverage.length ? row.template_coverage : row.valid_templates.map((_, templateIndex) => templateIndex);
    const templateIndex = coveredTemplates[nextValue(state) % coveredTemplates.length];
    const template = row.valid_templates[templateIndex];
    const source = sourceFor(row, template, index);
    const entropy = nextValue(state);
    const base = {
      layer: "grammar",
      case_id: `grammar:${row.construct_id}:${index}`,
      construct_id: row.construct_id,
      construct_family: row.family,
      seed: `${seed}:${index}`,
      mutation_arm: "grammar-valid",
      mutator_version: GRAMMAR_MUTATOR_VERSION,
      source,
      source_sha256: sourceHash(source),
      applicable_tiers: [...row.applicable_tiers],
      typed: true,
      bounded: true,
      value_consuming: true,
      observable_sink: clone(row.observable_sink),
      type_constraints: [...row.type_constraints],
      precondition: `template ${templateIndex} covers ${row.construct_id} and satisfies its type constraints`,
      generated_partitions: [...(family?.syntax_tags || [])],
      shrink_rule: "remove complete statements while preserving construct identity and print sink",
      relation: "parser, sema, TIR, and all applicable tiers agree on the consumed value",
      oracle: {
        name: `grammar:${row.construct_id}`,
        version: String(GRAMMAR_SCHEMA_VERSION),
        input_digest: digest({ construct_id: row.construct_id, seed, index }),
        independence_class: "tier-self-diff",
        provenance: "hardening-grammar-layer-3",
      },
      manifest_sha256,
      generation_index: index,
      entropy,
    };
    validateGeneratedProgram(base);
    programs.push(base);
    if (includeNearValid && programs.length < max) {
      for (const mutation of row.near_valid_mutations.slice(0, 1)) {
        const mutated = nearValidSource(source, mutation.id);
        const nearValidProgram = {
          ...base,
          case_id: `${base.case_id}:near:${mutation.id}`,
          mutation_arm: `grammar-near-valid:${mutation.id}`,
          base_source: source,
          base_source_sha256: sourceHash(source),
          source: mutated,
          source_sha256: sourceHash(mutated),
          value_consuming: false,
          violated_property: mutation.violated_property,
          base_observable_sink: clone(base.observable_sink),
          observable_sink: null,
          relation: "the compiler reports a registered diagnostic for the violated property",
        };
        validateGeneratedProgram(nearValidProgram, { nearValid: true });
        programs.push(nearValidProgram);
        break;
      }
    }
  }
  return {
    schema: GRAMMAR_SCHEMA,
    schema_version: GRAMMAR_SCHEMA_VERSION,
    seed: String(seed),
    max_cases: max,
    manifest,
    programs,
    rejected,
    attempted: programs.length,
    valid_case_count: programs.filter((program) => program.value_consuming).length,
    denominator: {
      constructs: manifest.denominator.total,
      executable_constructs: orderedCandidates.length,
      counted_reasons: rejected.length,
      generated: programs.length,
      valid: programs.filter((program) => program.value_consuming).length,
    },
    manifest_sha256,
    programs_sha256: digest(programs.map((program) => ({
      case_id: program.case_id,
      construct_id: program.construct_id,
      mutation_arm: program.mutation_arm,
      source_sha256: program.source_sha256,
      seed: program.seed,
    }))),
  };
}

export const generateGrammarPrograms = generateTypedPrograms;

export function validateGeneratedProgram(program, { nearValid = false } = {}) {
  if (!program || typeof program !== "object") throw new Error("generated program is required");
  if (typeof program.construct_id !== "string" || !CONSTRUCT_ID.test(program.construct_id)) throw new Error("generated program construct_id is invalid");
  if (typeof program.source !== "string" || program.source.length === 0 || Buffer.byteLength(program.source) > SOURCE_LIMIT) throw new Error(`generated program source is invalid: ${program.construct_id}`);
  if (program.typed !== true || program.bounded !== true) throw new Error(`generated program is not typed and bounded: ${program.construct_id}`);
  if (typeof program.value_consuming !== "boolean") throw new Error(`generated program value contract is missing: ${program.construct_id}`);
  if (!Array.isArray(program.type_constraints) || program.type_constraints.length === 0) throw new Error(`generated program type constraints are missing: ${program.construct_id}`);
  if (program.source_sha256 !== undefined && program.source_sha256 !== sourceHash(program.source)) throw new Error(`generated program source hash is stale: ${program.construct_id}`);
  if (!Array.isArray(program.applicable_tiers) || program.applicable_tiers.length === 0) throw new Error(`generated program tiers are missing: ${program.construct_id}`);
  tierList(program.applicable_tiers);
  if (nearValid) {
    if (program.value_consuming !== false) throw new Error(`near-valid program is marked value-consuming: ${program.construct_id}`);
    if (typeof program.base_source !== "string" || !balancedSource(program.base_source) || !hasRunFunction(program.base_source)) {
      throw new Error(`near-valid program lacks a valid base program: ${program.construct_id}`);
    }
    if (program.base_source_sha256 !== undefined && program.base_source_sha256 !== sourceHash(program.base_source)) {
      throw new Error(`near-valid base source hash is stale: ${program.construct_id}`);
    }
    if (program.source === program.base_source || !program.base_observable_sink?.type_aware
      || !hasObservableSink(program.base_source, program.base_observable_sink)) {
      throw new Error(`near-valid program lost its executable observer: ${program.construct_id}`);
    }
    if (typeof program.mutation_arm !== "string" || !program.mutation_arm.startsWith("grammar-near-valid:")) {
      throw new Error(`near-valid program mutation identity is missing: ${program.construct_id}`);
    }
    if (typeof program.violated_property !== "string" || program.violated_property.length === 0) {
      throw new Error(`near-valid program lacks violated property: ${program.construct_id}`);
    }
  } else {
    if (program.value_consuming !== true || !balancedSource(program.source) || !hasRunFunction(program.source) || !hasObservableSink(program.source, program.observable_sink)) {
      throw new Error(`generated program is observerless or not a valid bounded program: ${program.construct_id}`);
    }
  }
  return true;
}

function compilerError(result) {
  if (!result) return null;
  if (result.error) return String(result.error);
  if (result.stderr) return String(result.stderr);
  if (result.parse?.error) return String(result.parse.error);
  if (result.check?.error) return String(result.check.error);
  return null;
}

const DIAGNOSTIC_TEXT = /\b(?:E|L|JT)(?:\d{4}|(?:-[A-Z][A-Z0-9]*){2,})\b/g;

function diagnosticCodes(value) {
  const codes = new Set();
  const visited = new Set();
  const diagnosticKey = (key) => /^(?:code|diagnostic|diagnostics|error|errors|message|stderr|stdout|what|why|fix|reason)$/i.test(key)
    || /(?:_code|_bytes)$/.test(key);
  const visit = (candidate) => {
    if (candidate == null) return;
    if (typeof candidate === "string") {
      for (const match of candidate.matchAll(DIAGNOSTIC_TEXT)) codes.add(match[0]);
      return;
    }
    if (Buffer.isBuffer(candidate) || candidate instanceof Uint8Array) {
      visit(Buffer.from(candidate).toString("utf8"));
      return;
    }
    if (typeof candidate !== "object" || visited.has(candidate)) return;
    visited.add(candidate);
    for (const [key, child] of Object.entries(candidate)) {
      if (diagnosticKey(key) || (child && typeof child === "object")) visit(child);
    }
  };
  visit(value);
  return [...codes].sort();
}

function stageAccepted(stage) {
  if (diagnosticCodes(stage).length > 0) return false;
  return stage?.accepted === true
    || stage?.ok === true
    || stage?.status === "accepted"
    || (stage?.parse && stage.parse.error == null && stage.parse.ok === true)
    || (stage?.check && stage.check.error == null && stage.check.ok === true)
    || false;
}

function internalFailure(value, seen = new Set(), field = null) {
  if (value == null) return false;
  if (typeof value === "number") {
    return value === 101 && (field === null || /^(?:exit|exit_code|status|status_code)$/i.test(field));
  }
  if (typeof value === "string") {
    return /(?:internal compiler error|\bICE\b|compiler panic|\brustc\b[^\n]*(?:reject|error)|generated Rust[^\n]*(?:reject|error))/i.test(value);
  }
  if (typeof value !== "object" || seen.has(value)) return false;
  seen.add(value);
  return Object.entries(value).some(([key, child]) => {
    if (/^(?:exit|exit_code|status|status_code)$/i.test(key) && Number(child) === 101) return true;
    return internalFailure(child, seen, key);
  });
}

function rustcRejected(rust, tiers) {
  const explicit = (value) => value && (value.accepted === false || value.ok === false);
  if (explicit(rust)) return true;
  return Object.values(tiers || {}).some((observation) => {
    if (explicit(observation?.rust)) return true;
    const text = [observation?.error, observation?.stderr, observation?.stderr_bytes]
      .filter((value) => value !== undefined && value !== null)
      .map((value) => Buffer.isBuffer(value) ? value.toString("utf8") : String(value))
      .join(" ");
    return /\brustc\b|generated Rust|rustc rejected/i.test(text);
  });
}

function diagnosticObservation(code, stage, observed) {
  return code
    ? { code, stage, registered: REGISTERED_DIAGNOSTICS.has(code), observed: true }
    : { code: null, stage: null, registered: false, observed: false };
}

function sameCodes(left, right) {
  return stable(left) === stable(right);
}

export function classifyGrammarObservation({
  parser,
  sema,
  tir,
  tiers = {},
  rust = null,
  program = null,
} = {}) {
  const errors = [];
  if (program && !program.value_consuming) {
    const applicable = Array.isArray(program.applicable_tiers) ? program.applicable_tiers : Object.keys(tiers);
    const stageRows = [["parser", parser], ["sema", sema]];
    const diagnosticRows = stageRows.filter(([, observation]) => observation)
      .map(([stage, observation]) => ({ stage, observation, codes: diagnosticCodes(observation) }));
    const tierRows = applicable.map((tier) => ({
      stage: tier,
      observation: tiers[tier],
      codes: diagnosticCodes(tiers[tier]),
    }));
    const rows = [...diagnosticRows, ...tierRows];
    const allCodes = sortedUnique(rows.flatMap((row) => row.codes));
    const observedCode = allCodes[0] || null;
    const observedStage = rows.find((row) => row.codes.includes(observedCode))?.stage || null;
    const observed = diagnosticObservation(observedCode, observedStage, rows.length > 0);
    if (typeof program.violated_property !== "string" || program.violated_property.length === 0) {
      errors.push("near-valid program has no violated property");
    }
    if (!parser) errors.push("near-valid parser observation is missing");
    if (parser && stageAccepted(parser) && !sema) errors.push("near-valid sema observation is missing");
    if (!rows.some((row) => row.codes.length > 0)) errors.push("near-valid program produced no diagnostic");
    if (allCodes.some((code) => !DIAGNOSTIC_CODE.test(code) || !REGISTERED_DIAGNOSTICS.has(code))) {
      errors.push("near-valid program reported an unregistered diagnostic");
    }
    if (allCodes.length > 1 || rows.filter((row) => row.codes.length > 0)
      .some((row) => !sameCodes(row.codes, [observedCode]))) {
      errors.push("near-valid diagnostic codes disagree");
    }
    if (program.expected_diagnostic?.code !== undefined && program.expected_diagnostic.code !== observedCode) {
      errors.push(`near-valid expected ${program.expected_diagnostic.code} but compiler emitted ${observedCode || "none"}`);
    }
    if (rows.some((row) => internalFailure(row.observation))) {
      errors.push("near-valid compiler produced an internal failure");
    }
    if (rustcRejected(rust, tiers)) {
      errors.push("generated Rust rejection after sema acceptance is internal I2");
    }
    if (Object.keys(tiers).length > 0) {
      for (const row of tierRows) {
        if (!row.observation) errors.push(`missing near-valid tier observation: ${row.stage}`);
        else if (!row.codes.length) errors.push(`tier ${row.stage} produced no diagnostic`);
      }
    }
    return {
      status: errors.length ? "RED" : "USER_INVALID",
      classification: errors.some((error) => error.includes("internal"))
        ? "internal-I2"
        : errors.length ? "grammar-mismatch" : "registered-diagnostic",
      errors,
      diagnostics: allCodes,
      violated_property: program.violated_property,
      observed_diagnostic: observed,
    };
  }
  const parserAccepted = stageAccepted(parser);
  const semaAccepted = stageAccepted(sema);
  const tirConstructed = tir?.constructed === true || tir?.constructed?.ok === true;
  const tirEvaluated = tir?.evaluated === true || tir?.evaluated?.ok === true;
  if (!parserAccepted) errors.push(`parser rejected accepted construct: ${compilerError(parser) || "no parser acceptance"}`);
  if (!semaAccepted) errors.push(`sema rejected accepted construct: ${compilerError(sema) || "no sema acceptance"}`);
  if (!tirConstructed || !tirEvaluated) errors.push("TIR construction/evaluation is missing");
  if (diagnosticCodes(parser).length > 0 || diagnosticCodes(sema).length > 0) {
    errors.push("accepted compiler stage reported a diagnostic");
  }
  const tierRows = Object.entries(tiers);
  const applicable = Array.isArray(program?.applicable_tiers) ? program.applicable_tiers : tierRows.map(([tier]) => tier);
  if (program && (!Array.isArray(program.applicable_tiers) || program.applicable_tiers.length === 0)) {
    errors.push("generated program has no applicable tiers");
  }
  for (const tier of applicable) {
    const observation = tiers[tier];
    if (!observation) errors.push(`missing applicable tier observation: ${tier}`);
    else if (observation.timeout === true || observation.timed_out === true || observation.signal || observation.exit !== 0) {
      errors.push(`tier ${tier} did not execute successfully`);
    }
  }
  const rustRejected = rustcRejected(rust, tiers);
  if (semaAccepted && rustRejected) {
    errors.push("generated Rust rejection after sema acceptance is internal I2");
  }
  if (internalFailure({ parser, sema, tir, tiers, rust })) {
    errors.push("compiler produced an internal failure");
  }
  const values = applicable
    .map((tier) => tiers[tier])
    .filter(Boolean)
    .map((observation) => observation.normalized_value ?? observation.value ?? observation.stdout ?? "");
  if (values.length > 1 && values.some((value) => stable(value) !== stable(values[0]))) errors.push("applicable tier values disagree");
  if (program && (!program.value_consuming || !program.observable_sink)) errors.push("observerless generation is not permitted");
  const classification = errors.some((error) => error.includes("I2"))
    ? "internal-I2"
    : errors.some((error) => error.includes("tier values disagree"))
      ? "optimizer-only-meaning-change"
      : errors.length ? "grammar-mismatch" : "agreement";
  return {
    status: errors.length ? "RED" : "PASS",
    classification,
    errors,
    parser: parserAccepted,
    sema: semaAccepted,
    tir: Boolean(tirConstructed && tirEvaluated),
    tiers: Object.fromEntries(applicable.map((tier) => [
      tier,
      tiers[tier]?.normalized_value ?? tiers[tier]?.value ?? tiers[tier]?.stdout ?? "",
    ])),
  };
}

export const checkGrammarAgreement = classifyGrammarObservation;

export function minimizeGrammarProgram(program, candidates = [], options = {}) {
  if (!Array.isArray(candidates)) {
    options = candidates || {};
    candidates = options.candidates || [];
  }
  validateGeneratedProgram(program, { nearValid: !program.value_consuming });
  const observableMismatch = options.observable_mismatch
    ?? options.observableMismatch
    ?? program.observable_mismatch;
  const trace = [];
  const identityMarker = `generated construct ${program.construct_id}`;
  const predicate = typeof options.predicate === "function" ? options.predicate : null;
  const viable = [program, ...candidates]
    .map((candidate) => ({
      candidate,
      source: typeof candidate === "string" ? candidate : candidate?.source,
      construct_id: typeof candidate === "object" ? candidate?.construct_id : null,
      mismatch: typeof candidate === "object" ? candidate?.observable_mismatch : undefined,
    }))
    .filter((item) => {
      if (typeof item.source !== "string" || !item.source.includes("fn run")) return false;
      if (item.construct_id !== null && item.construct_id !== undefined && item.construct_id !== program.construct_id) return false;
      if (item.construct_id === null && !item.source.includes(identityMarker)) return false;
      if (program.value_consuming && (!balancedSource(item.source) || !hasObservableSink(item.source, program.observable_sink))) return false;
      if (!program.value_consuming && typeof item.candidate === "object" && item.candidate?.base_source
        && !item.candidate.base_source.includes(identityMarker)) return false;
      if (observableMismatch !== undefined
        && (item.mismatch === undefined || stable(item.mismatch) !== stable(observableMismatch))) return false;
      if (predicate && !predicate(item.candidate, program)) return false;
      trace.push({ source_sha256: sourceHash(item.source), kept: true });
      return true;
    })
    .sort((left, right) => Buffer.byteLength(left.source) - Buffer.byteLength(right.source) || left.source.localeCompare(right.source));
  const source = (viable.length ? viable : [{ source: program.source }])[0].source;
  const minimized = {
    ...clone(program),
    source,
    source_sha256: sourceHash(source),
    minimized: true,
    shrink_trace: trace,
    construct_id: program.construct_id,
    observable_sink: clone(program.observable_sink),
  };
  if (observableMismatch !== undefined) minimized.observable_mismatch = clone(observableMismatch);
  return minimized;
}

function serializableObservation(observation, { applicable = true, reason = null } = {}) {
  if (!observation) return {
    applicable,
    observed: false,
    ...(reason ? { reason } : {}),
  };
  const copy = (value) => {
    if (Buffer.isBuffer(value) || value instanceof Uint8Array) return Buffer.from(value).toString("utf8");
    if (Array.isArray(value)) return value.map(copy);
    if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, copy(child)]));
    return value;
  };
  return {
    applicable,
    observed: true,
    ...copy(observation),
    diagnostic_codes: diagnosticCodes(observation),
  };
}

function tirObservation(stage, observations) {
  if (stage?.tir && typeof stage.tir === "object") return serializableObservation(stage.tir);
  const tierTirs = observations
    .map((observation) => observation?.tir)
    .filter((value) => value && typeof value === "object");
  if (tierTirs.length === 0) return serializableObservation(null, { reason: "TIR observation is missing" });
  return {
    applicable: true,
    observed: true,
    constructed: tierTirs.some((value) => value.constructed === true || value.constructed?.ok === true),
    evaluated: tierTirs.some((value) => value.evaluated === true || value.evaluated?.ok === true),
    source: "tier-observation",
  };
}

function programObservation(program, stageInput, stages, observations, comparison) {
  const tierMap = Object.fromEntries(observations.map((observation) => [observation.tier, observation]));
  const parserAccepted = stageAccepted(stageInput.parser);
  const semaApplicable = program.value_consuming || (parserAccepted && stageInput.sema != null);
  const tirApplicable = program.value_consuming || (semaApplicable && stageAccepted(stageInput.sema));
  return {
    case_id: program.case_id,
    construct_id: program.construct_id,
    generation_index: program.generation_index,
    mutation_arm: program.mutation_arm,
    value_consuming: program.value_consuming,
    ...(program.violated_property ? { violated_property: program.violated_property } : {}),
    applicable_tiers: [...program.applicable_tiers],
    stages: {
      parser: serializableObservation(stageInput.parser),
      sema: serializableObservation(stageInput.sema, {
        applicable: semaApplicable,
        reason: semaApplicable ? null : "parser rejected before sema",
      }),
      tir: tirApplicable
        ? tirObservation(stageInput, observations)
        : serializableObservation(null, { applicable: false, reason: "sema rejected before TIR" }),
      ...(stages?.rust ? { rust: serializableObservation(stages.rust) } : {}),
    },
    tiers: Object.fromEntries(program.applicable_tiers.map((tier) => [
      tier,
      serializableObservation(tierMap[tier]),
    ])),
    status: comparison.status,
    classification: comparison.classification,
    ...(comparison.observed_diagnostic ? { observed_diagnostic: clone(comparison.observed_diagnostic) } : {}),
    ...(comparison.observable_mismatch ? { observable_mismatch: clone(comparison.observable_mismatch) } : {}),
  };
}

function bundleForGrammar(program, comparison, metadata = {}) {
  const minimizationInput = comparison.observable_mismatch
    ? { ...program, observable_mismatch: comparison.observable_mismatch }
    : program;
  const minimized = comparison.minimized_program
    || minimizeGrammarProgram(minimizationInput, [], { observable_mismatch: comparison.observable_mismatch });
  const observations = (comparison.observations || []).map((row) => ({
    ...row,
    stdout: row.stdout ?? row.stdout_bytes ?? JSON.stringify(row.normalized_value ?? row.value ?? ""),
    stderr: row.stderr ?? row.stderr_bytes ?? "",
    exit: row.exit === undefined ? 0 : row.exit,
    signal: row.signal ?? null,
    timeout: row.timeout === true || row.timed_out === true,
    relation: row.relation || stable(row.normalized_value ?? row.value ?? row.stdout ?? ""),
  }));
  const observation = observations.find((row) => row.tier === comparison.result_bundle_input?.tier) || observations[0];
  const source = minimized.value_consuming ? minimized.source : minimized.base_source || minimized.source;
  return makeResultBundle({
    run_id: metadata.run_id || "grammar-run",
    stable_surface_id: program.construct_id,
    tier: observation?.tier || program.applicable_tiers[0],
    tier_command: observation?.tier_command || `grammar:${observation?.tier || program.applicable_tiers[0]}`,
    seed: minimized.seed,
    mutation_arm: minimized.mutation_arm,
    mutator_version: minimized.mutator_version,
    source,
    ...(minimized.value_consuming || !minimized.source || minimized.source === source ? {} : { mutated_source: minimized.source }),
    stdout: observation?.stdout ?? observation?.stdout_bytes ?? "",
    stderr: observation?.stderr ?? observation?.stderr_bytes ?? "",
    exit: observation?.exit === undefined ? 0 : observation.exit,
    signal: observation?.signal ?? null,
    timeout: observation?.timeout === true || observation?.timed_out === true,
    expected_relation: comparison.expected_relation || "grammar-agreement",
    actual_relation: comparison.actual_relation || stable(observation?.normalized_value ?? ""),
    normalization: [],
    oracle: minimized.oracle,
    commit: metadata.commit || "unknown-commit",
    binary_sha256: metadata.binary_sha256 || "sha256:unknown-binary",
    registry_snapshot_hash: metadata.registry_snapshot_hash || "sha256:unknown-registry",
    config_hash: metadata.config_hash || "sha256:unknown-config",
    classification: comparison.classification || metadata.classification || "grammar-mismatch",
    tower_action: "create-or-update",
    tier_observations: observations,
    applicable_tiers: program.applicable_tiers,
    type_constraints: program.type_constraints,
    proof: {
      stages: clone(comparison.stages || null),
      diagnostics: clone(comparison.diagnostics || null),
      observable_mismatch: clone(comparison.observable_mismatch || null),
      minimized: true,
    },
    layer: "grammar",
    construct_id: minimized.construct_id,
    precondition: minimized.precondition,
    generated_partitions: minimized.generated_partitions,
    observable_sink: minimized.observable_sink || minimized.base_observable_sink,
    shrink_rule: minimized.shrink_rule,
    relation: minimized.relation,
  });
}

function grammarStageInput(observations, stage = null) {
  const sources = [];
  if (stage && typeof stage === "object") sources.push(stage);
  for (const observation of observations) {
    sources.push(observation);
    if (observation.grammar_stages && typeof observation.grammar_stages === "object") {
      sources.push(observation.grammar_stages);
    }
  }
  const first = (key) => sources.find((source) => source[key] !== undefined)?.[key];
  const explicitTiers = sources.find((source) => source.tiers && typeof source.tiers === "object")?.tiers;
  const observedTirs = sources
    .map((source) => source.tir)
    .filter((value) => value && typeof value === "object");
  const tir = stage?.tir !== undefined
    ? stage.tir
    : observedTirs.length > 0
      ? {
          constructed: observedTirs.some((value) => value.constructed === true || value.constructed?.ok === true),
          evaluated: observedTirs.some((value) => value.evaluated === true || value.evaluated?.ok === true),
        }
      : undefined;
  return {
    parser: first("parser"),
    sema: first("sema"),
    tir,
    rust: first("rust"),
    tiers: explicitTiers,
  };
}

async function executeGrammarStages(stageExecutor, program) {
  if (!stageExecutor) return null;
  try {
    const stages = await stageExecutor(program);
    if (!stages || typeof stages !== "object") return { error: "stage executor returned no observation" };
    return stages;
  } catch (error) {
    return { error: error instanceof Error ? error.message : String(error) };
  }
}

async function executeGrammarTier(executor, program, tier) {
  try {
    const observation = await executor({ ...program, tier });
    if (!observation || typeof observation !== "object") {
      return { tier, exit: 1, signal: null, timeout: false, timed_out: false, error: "executor returned no observation" };
    }
    return {
      ...observation,
      tier: observation.tier || tier,
      exit: observation.exit === undefined ? 0 : observation.exit,
      signal: observation.signal === undefined ? null : observation.signal,
      timeout: observation.timeout === true || observation.timed_out === true,
    };
  } catch (error) {
    return {
      tier,
      exit: 1,
      signal: null,
      timeout: false,
      timed_out: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

async function executeGrammarTiers(executor, program) {
  if (!executor) {
    return program.applicable_tiers.map((tier) => ({
      tier,
      exit: 1,
      signal: null,
      timeout: false,
      error: "grammar executor is required for tier observation",
    }));
  }
  const observations = [];
  for (const tier of program.applicable_tiers) {
    observations.push(await executeGrammarTier(executor, program, tier));
  }
  return observations;
}

function observationValue(observation) {
  return observation?.normalized_value ?? observation?.value ?? observation?.stdout;
}

function observationUnhealthy(observation) {
  return !observation
    || observation.error
    || observation.timeout === true
    || observation.timed_out === true
    || observation.signal
    || (observation.exit !== undefined && observation.exit !== null && observation.exit !== 0);
}

function coverageForPrograms(programResults, expectedPrograms = programResults) {
  const cells = [];
  const count = (stage) => programResults.filter((program) => program.stages[stage]?.applicable).length;
  const observed = (stage) => programResults.filter((program) => program.stages[stage]?.applicable && program.stages[stage].observed).length;
  for (const program of programResults) {
    for (const [stage, result] of Object.entries(program.stages)) {
      if (result.applicable && !result.observed) cells.push(`${program.case_id}:${stage}`);
    }
    for (const tier of program.applicable_tiers) {
      if (!program.tiers[tier]?.observed) cells.push(`${program.case_id}:${tier}`);
    }
  }
  const complete = (program) => !Object.entries(program.stages).some(([, result]) => result.applicable && !result.observed)
    && program.applicable_tiers.every((tier) => program.tiers[tier]?.observed);
  return {
    expected_programs: expectedPrograms.length,
    observed_programs: programResults.filter(complete).length,
    expected_valid_programs: expectedPrograms.filter((program) => program.value_consuming).length,
    observed_valid_programs: programResults.filter((program) => program.value_consuming && complete(program)).length,
    expected_near_valid_programs: expectedPrograms.filter((program) => !program.value_consuming).length,
    observed_near_valid_programs: programResults.filter((program) => !program.value_consuming && complete(program)).length,
    stages: Object.fromEntries(["parser", "sema", "tir"].map((stage) => [stage, {
      applicable: count(stage),
      observed: observed(stage),
    }])),
    tiers: Object.fromEntries(TIERS.map((tier) => {
      const applicable = programResults.filter((program) => program.applicable_tiers.includes(tier));
      return [tier, {
        applicable: applicable.length,
        observed: applicable.filter((program) => program.tiers[tier]?.observed).length,
      }];
    })),
    unobserved_cells: cells,
    complete: cells.length === 0 && programResults.length === expectedPrograms.length,
  };
}

export async function runGrammarPrograms(programs, {
  executor = null,
  stageExecutor = null,
  maxCases = GRAMMAR_DEFAULT_MAX_CASES,
  metadata = {},
} = {}) {
  const max = boundedMaxCases(maxCases);
  if (!Array.isArray(programs)) throw new Error("grammar programs must be an array");
  const findings = [];
  const rejected = [];
  const program_results = [];
  let attempted = 0;
  let valid_case_count = 0;
  const selected = programs.slice(0, max);
  for (const program of selected) {
    attempted += 1;
    validateGeneratedProgram(program, { nearValid: !program.value_consuming });
    const stages = await executeGrammarStages(stageExecutor, program);
    const observations = await executeGrammarTiers(executor, program);
    const tierMap = Object.fromEntries(observations.map((observation) => [observation.tier, observation]));
    const stageInput = grammarStageInput(observations, stages);
    const tiers = { ...(stageInput.tiers || {}), ...tierMap };
    if (!program.value_consuming) {
      const diagnosticResult = classifyGrammarObservation({
        ...stageInput,
        tiers,
        program,
      });
      const observedDiagnostic = diagnosticResult.observed_diagnostic || diagnosticObservation(null, null, false);
      const observedDiagnostics = {
        parser: diagnosticCodes(stageInput.parser),
        sema: diagnosticCodes(stageInput.sema),
        tiers: Object.fromEntries(program.applicable_tiers.map((tier) => [tier, diagnosticCodes(tierMap[tier])])),
      };
      const observableMismatch = {
        construct_id: program.construct_id,
        violated_property: program.violated_property,
        expected_diagnostic: program.expected_diagnostic?.code || null,
        observed_diagnostic: observedDiagnostic,
        observed_diagnostics: observedDiagnostics,
        errors: diagnosticResult.errors,
      };
      const comparison = {
        status: diagnosticResult.status,
        expected_relation: `violated-property:${program.violated_property}`,
        actual_relation: `diagnostic:${observedDiagnostic.code || "none"}`,
        classification: diagnosticResult.classification,
        result_bundle_input: { tier: observations[0]?.tier || program.applicable_tiers[0] },
        observations,
        stages,
        diagnostics: diagnosticResult,
        observed_diagnostic: observedDiagnostic,
        observable_mismatch: observableMismatch,
      };
      program_results.push(programObservation(program, stageInput, stages, observations, comparison));
      if (diagnosticResult.status === "RED") {
        findings.push(bundleForGrammar({
          ...program,
          observable_sink: program.base_observable_sink || program.observable_sink,
        }, comparison, metadata));
      } else {
        rejected.push({
          case_id: program.case_id,
          construct_id: program.construct_id,
          reason: program.violated_property,
          violated_property: program.violated_property,
          observed_diagnostic: observedDiagnostic,
          observed_diagnostics: observedDiagnostics,
        });
      }
      continue;
    }
    valid_case_count += 1;
    const stageResult = classifyGrammarObservation({
      ...stageInput,
      tiers,
      program,
    });
    const applicableObservations = program.applicable_tiers.map((tier) => tierMap[tier]);
    const values = applicableObservations.map(observationValue);
    const expectedValue = values[0];
    const expected_relation = expectedValue === undefined ? "unobserved" : stable(expectedValue);
    const differing = applicableObservations
      .map((observation, index) => ({ observation, index }))
      .filter(({ observation, index }) => index === 0
        ? observationUnhealthy(observation)
        : expectedValue === undefined
          || observationValue(observation) === undefined
          || stable(observationValue(observation)) !== expected_relation)
      .map(({ observation }) => observation?.tier)
      .filter(Boolean);
    const unhealthy = applicableObservations.filter(observationUnhealthy).map((observation) => observation?.tier).filter(Boolean);
    const actualValue = values.find((value, index) => index > 0 && (expectedValue === undefined
      || value === undefined
      || stable(value) !== expected_relation)) ?? expectedValue;
    const actual_relation = actualValue === undefined ? "unobserved" : stable(actualValue);
    const stageDifferences = [];
    if (!stageInput.parser || !stageAccepted(stageInput.parser)) stageDifferences.push("parser");
    if (diagnosticCodes(stageInput.parser).length > 0) stageDifferences.push("parser");
    if (!stageInput.sema || !stageAccepted(stageInput.sema)) stageDifferences.push("sema");
    if (diagnosticCodes(stageInput.sema).length > 0) stageDifferences.push("sema");
    const tirConstructed = stageInput.tir?.constructed === true || stageInput.tir?.constructed?.ok === true;
    const tirEvaluated = stageInput.tir?.evaluated === true || stageInput.tir?.evaluated?.ok === true;
    if (!stageInput.tir || !tirConstructed || !tirEvaluated) stageDifferences.push("tir");
    if (rustcRejected(stageInput.rust, tiers)) stageDifferences.push("rust");
    const differences = [...new Set([
      ...unhealthy,
      ...differing,
      ...stageDifferences,
    ])];
    const comparison = {
      status: stageResult.status,
      expected_relation,
      actual_relation,
      classification: stageResult.classification || (unhealthy.length ? "grammar-execution-failure" : "grammar-mismatch"),
      result_bundle_input: { tier: differences[0], expected_relation, actual_relation },
      observations,
      stages,
      diagnostics: stageResult,
      observable_mismatch: {
        construct_id: program.construct_id,
        expected_relation,
        actual_relation,
        differences,
        errors: stageResult.errors,
      },
    };
    program_results.push(programObservation(program, stageInput, stages, observations, comparison));
    if (differences.length > 0 || stageResult.errors.length > 0) {
      findings.push(bundleForGrammar(program, comparison, metadata));
    }
  }
  const coverage = coverageForPrograms(program_results, selected);
  const serialized_bundles = serializeBundles(findings);
  return {
    schema: GRAMMAR_SCHEMA,
    schema_version: GRAMMAR_SCHEMA_VERSION,
    status: findings.length ? "FINDINGS" : "PASS",
    attempted,
    valid_case_count,
    near_valid_case_count: attempted - valid_case_count,
    omitted: programs.slice(max).map((program) => ({ case_id: program.case_id, reason: `maxCases=${max}` })),
    rejected,
    program_results,
    observations: program_results,
    coverage,
    findings,
    serialized_bundles,
    bundle_sha256: sha256(serialized_bundles),
    stage_checked: Boolean(stageExecutor),
  };
}

export function checkGrammarNegativeControls() {
  const manifest = deriveConstructManifest({
    syntaxSource: "/// ratified D-GRAMMAR-CONTROL\npub const KW_FN: &str = \"fn\";",
    parserSources: [],
    semaSources: [],
    includeStaticFamilies: true,
  });
  const generated = generateTypedPrograms(manifest, { maxCases: CONSTRUCT_FAMILIES.length });
  const controls = [];

  const mustFail = (name, action) => {
    try {
      action();
    } catch {
      controls.push(name);
      return;
    }
    throw new Error(`${name} control survived`);
  };

  mustFail("missing-production", () => {
    validateConstructManifest({
      ...manifest,
      rows: manifest.rows.filter((row) => row.construct_id !== "syntax:KW_FN"),
    });
  });
  mustFail("observerless", () => validateGeneratedProgram({
    ...generated.programs[0],
    value_consuming: false,
    observable_sink: null,
  }, { nearValid: false }));

  const accepted = generated.programs.find((program) => program.value_consuming);
  const unlowered = classifyGrammarObservation({
    parser: { accepted: true },
    sema: { accepted: true },
    tir: { constructed: false, evaluated: false },
    tiers: Object.fromEntries(accepted.applicable_tiers.map((tier) => [tier, { value: 1, exit: 0 }])),
    rust: { accepted: false },
    program: accepted,
  });
  if (!unlowered.errors.some((error) => error.includes("TIR"))) throw new Error("admitted-but-unlowered control survived");
  controls.push("admitted-but-unlowered");

  const optimizerOnly = classifyGrammarObservation({
    parser: { accepted: true },
    sema: { accepted: true },
    tir: { constructed: true, evaluated: true },
    tiers: {
      aot: { value: 1, exit: 0 },
      jet_run: { value: 2, exit: 0 },
      interpreter: { value: 1, exit: 0 },
    },
    rust: { accepted: true },
    program: accepted,
  });
  if (optimizerOnly.classification !== "optimizer-only-meaning-change") throw new Error("optimizer-only meaning-change control survived");
  controls.push("optimizer-only-meaning-change");

  const expected = ["missing-production", "observerless", "admitted-but-unlowered", "optimizer-only-meaning-change"];
  if (stable(controls) !== stable(expected)) throw new Error("grammar negative controls are incomplete");
  return controls;
}

if (import.meta.url === `file://${process.argv[1]}` && process.argv.includes("--self-test")) {
  checkGrammarNegativeControls();
  console.log("hardening grammar layer: PASS");
}
