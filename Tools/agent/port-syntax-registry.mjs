#!/usr/bin/env node
// One-time syntax registry port and temporary Rust-reference comparison.
// The Jet files are canonical tool inputs; this script is not a build generator.
// While the reference exists: node Tools/agent/port-syntax-registry.mjs --check
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const SYNTAX_DIR = "Compiler/JetFoundation/Data/Syntax";
const fail = (message) => { throw new Error(`syntax registry port: ${message}`); };

// Keep strings and comments opaque while translating expression punctuation.
function translate(text, transform) {
  return text.split(/("(?:\\.|[^"\\])*"|\/\/[^\n]*|\/\*[\s\S]*?\*\/)/g)
    .map((part, index) => index % 2 ? part : transform(part)).join("");
}
function statementEnd(source, start) {
  let quote = false, escape = false, line = false, block = 0;
  for (let i = start; i < source.length; i++) {
    const c = source[i], n = source[i + 1];
    if (line) { if (c === "\n") line = false; continue; }
    if (block) {
      if (c === "/" && n === "*") { block++; i++; }
      else if (c === "*" && n === "/") { block--; i++; }
      continue;
    }
    if (quote) {
      if (escape) escape = false;
      else if (c === "\\") escape = true;
      else if (c === '"') quote = false;
      continue;
    }
    if (c === '"') quote = true;
    else if (c === "/" && n === "/") { line = true; i++; }
    else if (c === "/" && n === "*") { block = 1; i++; }
    else if (c === ";") return i;
  }
  fail("unterminated constant initializer");
}
function attachedComments(source, start) {
  const lines = source.slice(0, start).split("\n");
  lines.pop();
  const out = [];
  while (lines.length) {
    const line = lines.pop();
    if (/^\s*#\[/.test(line)) continue;
    if (!/^\s*\/\//.test(line)) break;
    out.unshift(line);
  }
  return out.join("\n");
}
export function rustConstants(source) {
  const rows = [];
  for (const match of source.matchAll(/^(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*([^=]+)=/gm)) {
    const end = statementEnd(source, match.index + match[0].length);
    const trailing = source.slice(end + 1).match(/^[\t ]*(\/\/[^\n]*)/)?.[1] || "";
    rows.push({ name: match[1], type: match[2].trim(), value: source.slice(match.index + match[0].length, end).trim(), comments: attachedComments(source, match.index), trailing });
  }
  return rows;
}
function jetType(type) {
  return type.replace(/&(?:'static\s+)?str/g, "String").replace(/&(?=\[)/g, "")
    .replace(/\bu64\b/g, "U64").replace(/\bu8\b/g, "U8").replace(/\busize\b/g, "Int");
}
function jetValue(row, constants) {
  if (row.type === "&str") {
    const literal = /^"(?:\\.|[^"\\])*"$/;
    if (literal.test(row.value)) return row.value;
    if (/^concat!\(/.test(row.value)) {
      const parts = [...row.value.matchAll(/"(?:\\.|[^"\\])*"/g)].map(m => JSON.parse(m[0]));
      return JSON.stringify(parts.join(""));
    }
    const name = row.value.replace(/^super::/, "");
    const target = constants.get(name);
    if (!target || target === row) fail(`unresolved string constant ${row.name}: ${row.value}`);
    return jetValue(target, constants);
  }
  const type = jetType(row.type);
  let value = translate(row.value, part => part.replace(/\b(?:super|crate)::/g, "").replace(/::/g, ".").replace(/\bSome\(/g, "Val("));
  value = value.replace(/retired_ffi_effect!\(\s*("(?:\\.|[^"\\])*")\s*,\s*("(?:\\.|[^"\\])*")\s*,\s*("(?:\\.|[^"\\])*")\s*\)/g,
    (_, id, retired, canonical) => `Retirement{id: ${id}, retired: ${retired}, canonical: ${canonical}, kind: RetirementKind.Semantic, decision: "D-AUTHORITY-ROOTS1=A", since: "2026-08-06", code: Val("E0119")}`);
  if (/^&\[/.test(row.value)) {
    value = value.replace(/^&\[/, `${type}{`).replace(/\]$/, "}");
  }
  return value;
}
export function projectRegistry(root = ROOT) {
  const sourceDir = resolve(root, "crates/jet-foundation/src/Syntax");
  const paths = ["crates/jet-foundation/src/Syntax.rs", ...readdirSync(sourceDir).filter(name => name.endsWith(".rs")).sort().map(name => `crates/jet-foundation/src/Syntax/${name}`)];
  const modules = paths.map(path => ({ path, rows: rustConstants(readFileSync(resolve(root, path), "utf8")) }));
  const constants = new Map();
  for (const { rows } of modules) for (const row of rows) {
    if (constants.has(row.name)) fail(`duplicate constant ${row.name}`);
    constants.set(row.name, row);
  }
  const files = new Map();
  for (const { path, rows } of modules) {
    const name = path.endsWith("/Syntax.rs") ? "Spellings.jet" : path.split("/").pop().replace(/\.rs$/, ".jet");
    const header = ["// Canonical owner-controlled syntax registry (I7).", "// Mechanically ported by Tools/agent/port-syntax-registry.mjs; edit this Jet home.", "// The reference comparison is temporary and is not a generator dependency."];
    if (name === "Spellings.jet") header.push("", "struct LexicalEntry {", "    spelling: String", "    meaning: String", "    decision: String", "}");
    const body = rows.map(row => [row.comments, `${row.name} :: ${jetValue(row, constants)}${row.trailing ? ` ${row.trailing}` : ""}`].filter(Boolean).join("\n")).join("\n\n");
    files.set(`${SYNTAX_DIR}/${name}`, `${header.join("\n")}\n\n${body}\n`);
  }
  const lexer = readFileSync(resolve(root, "crates/jet-lexer/src/Lexer/mod.rs"), "utf8");
  const keywords = [...lexer.matchAll(/s if s == Syntax::(\w+) => Some\(TokKind::(Kw\w+)\),/g)];
  if (!keywords.length) fail("missing reference lexer keyword mapping");
  const keywordRows = keywords.map(([, name, kind]) => {
    if (!constants.has(name)) fail(`unresolved keyword ${name}`);
    const tag = `K_${kind.slice(2).replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase()}`;
    return `    LexerKeyword{constant: "${name}", kind: "${kind}", tag: "${tag}"},`;
  });
  files.set("Compiler/JetLexer/Data/Lexer/Keywords.jet", [
    "// Canonical lexer keyword mapping; spellings live in the Syntax registry.",
    "// Mechanically ported by Tools/agent/port-syntax-registry.mjs.",
    "struct LexerKeyword {", "    constant: String", "    kind: String", "    tag: String", "}", "",
    "LEXER_KEYWORDS :: [LexerKeyword]{", ...keywordRows, "}", "",
  ].join("\n"));
  return { files, constants };
}
function main(args) {
  if (args.length !== 1 || !["--check", "--write"].includes(args[0])) fail("usage: port-syntax-registry.mjs --check|--write");
  if (!existsSync(resolve(ROOT, "crates/jet-foundation/src/Syntax.rs"))) {
    if (args[0] === "--write") fail("Rust reference was removed; edit the canonical Jet registry");
    console.log("syntax registry: Rust reference removed; temporary comparison retired");
    return;
  }
  const { files, constants } = projectRegistry();
  for (const [path, content] of files) {
    if (args[0] === "--write") {
      mkdirSync(dirname(resolve(ROOT, path)), { recursive: true });
      writeFileSync(resolve(ROOT, path), content);
    } else if (!existsSync(resolve(ROOT, path)) || readFileSync(resolve(ROOT, path), "utf8") !== content) fail(`Jet/reference projection differs: ${path}`);
  }
  console.log(`syntax registry ${args[0].slice(2)}: ${constants.size} constants, ${files.size} Jet files`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main(process.argv.slice(2));
