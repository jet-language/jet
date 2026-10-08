#!/usr/bin/env node
// Project the bootstrap lexer keyword() mapping into borrowed Jet byte checks.
// Spellings remain Syntax constants; TokKind and existing K_* tags own the kinds.
// Usage: node Tools/agent/lexer-keyword-rows.mjs [--check|--write]
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const SCAN_PATH = "Compiler/JetLexer/Source/Lexer/Scan.jet";
const OPEN = "// BEGIN GENERATED LEXER KEYWORD SPANS";
const CLOSE = "// END GENERATED LEXER KEYWORD SPANS";
const fail = (message) => { throw new Error(`lexer keyword rows: ${message}`); };
const uncomment = (text) => text.replace(/\/\/[^\n]*/g, "");

export function keywordRows(mapping, syntax, tokens, tags) {
  const body = uncomment(mapping).match(/^fn keyword\(name: &str\) -> Option<TokKind> \{\s*match name \{([\s\S]*?)\n\s*\}\s*\n\}/m)?.[1];
  if (!body) fail("unrecognized keyword() function");
  const rows = [];
  const spellings = new Set();
  const kinds = new Set();
  const constants = new Map();
  for (const match of syntax.matchAll(/^\s*pub\s+const\s+(\w+)\s*:\s*&str\s*=\s*([^;]+);/gm)) {
    const values = constants.get(match[1]) ?? [];
    values.push(match[2].trim());
    constants.set(match[1], values);
  }
  const enumBody = uncomment(tokens).match(/^pub enum TokKind \{([\s\S]*?)^\}/m)?.[1];
  if (!enumBody) fail("missing TokKind enum");
  const enumKinds = [...enumBody.matchAll(/^\s*(Kw\w+),\s*$/gm)].map((match) => match[1]);
  const mentionedKinds = enumBody.match(/\bKw\w+\b/g) ?? [];
  if (!enumKinds.length || mentionedKinds.length !== enumKinds.length ||
      new Set(enumKinds).size !== enumKinds.length) fail("unparsed/ambiguous keyword token kinds");
  const tagNames = [...tags.matchAll(/^(K_\w+) ::/gm)].map((match) => match[1]);
  let fallback = false;
  for (const line of body.split("\n").map((line) => line.trim()).filter(Boolean)) {
    if (line === "_ => None,") {
      if (fallback) fail("duplicate keyword fallback");
      fallback = true;
      continue;
    }
    if (fallback) fail("keyword arm after fallback");
    const arm = line.match(/^s if s == Syntax::(\w+) => Some\(TokKind::(Kw\w+)\),$/);
    if (!arm) fail(`unparsed keyword arm: ${line}`);
    const [, constant, kind] = arm;
    const values = constants.get(constant) ?? [];
    if (values.length !== 1 || !/^"(?:[^"\\]|\\.)*"$/.test(values[0])) fail(`unresolved/ambiguous literal Syntax::${constant}`);
    let spelling;
    try { spelling = JSON.parse(values[0]); } catch { fail(`unsupported Rust literal Syntax::${constant}`); }
    // Current keyword authority is ASCII. Fail rather than silently invent a
    // Unicode normalization/identifier policy if that authority changes.
    if (!/^[A-Za-z][A-Za-z0-9_]*$/.test(spelling)) fail(`unsupported keyword spelling Syntax::${constant}`);
    const tag = `K_${kind.slice(2).replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase()}`;
    if (!enumKinds.includes(kind) || tagNames.filter((name) => name === tag).length !== 1) fail(`unresolved token kind ${kind}/${tag}`);
    if (spellings.has(spelling) || kinds.has(kind)) fail(`duplicate keyword spelling/kind ${constant}/${kind}`);
    spellings.add(spelling);
    kinds.add(kind);
    rows.push({ constant, kind, tag, spelling, bytes: [...Buffer.from(spelling, "utf8")] });
  }
  if (!fallback || !rows.length || enumKinds.some((kind) => !kinds.has(kind))) fail("incomplete keyword mapping/TokKind coverage");
  return rows;
}

export function readKeywordRows(root = ROOT) {
  const read = (path) => readFileSync(resolve(root, path), "utf8");
  const source = read("crates/jet-foundation/src/Syntax.rs");
  const modules = [...source.matchAll(/^mod (\w+);\s*\npub use \1::\*;/gm)].map((match) => match[1]);
  if (!modules.length) fail("missing Syntax module exports");
  const syntax = [source, ...modules.map((name) => read(`crates/jet-foundation/src/Syntax/${name}.rs`))].join("\n");
  return keywordRows(read("crates/jet-lexer/src/Lexer/mod.rs"), syntax,
    read("crates/jet-lexer/src/Lexer/Tokens.rs"), read("Compiler/JetLexer/Source/Lexer/Terminators.jet"));
}

function branches(rows, offset, indent) {
  if (rows.length === 1) {
    const row = rows[0];
    const checks = row.bytes.slice(offset).map((byte, index) => `byte_at(source, start + ${offset + index}) == ${byte}`);
    return `${indent}if ${checks.join(" && ")} -> ${row.tag} else -> K_IDENT // Syntax::${row.constant}\n`;
  }
  const groups = new Map();
  for (const row of rows) {
    const byte = row.bytes[offset];
    const group = groups.get(byte) ?? [];
    group.push(row);
    groups.set(byte, group);
  }
  let out = `${indent}if byte_at(source, start + ${offset}) == {\n`;
  for (const [byte, group] of [...groups].sort(([a], [b]) => a - b)) {
    out += `${indent}    ${byte} -> {\n`;
    if (offset + 1 === group[0].bytes.length) out += `${indent}        ${group[0].tag} // Syntax::${group[0].constant}\n`;
    else out += branches(group, offset + 1, `${indent}        `);
    out += `${indent}    }\n`;
  }
  return out + `${indent}    else -> K_IDENT\n${indent}}\n`;
}

export function renderKeywordSpans(rows) {
  const lengths = new Map();
  for (const row of rows) {
    const group = lengths.get(row.bytes.length) ?? [];
    group.push(row);
    lengths.set(row.bytes.length, group);
  }
  let out = `${OPEN}\n// Derived from Rust Lexer::keyword, Syntax literals and TokKind; do not hand edit.\n// No spelling ownership here: Payload still builds the exact token text.\nfn word_kind(source: [U8], start: Int, end: Int, previous_kind: Int) -> Int Never! {\n    kind :: if end - start == {\n`;
  for (const [length, group] of [...lengths].sort(([a], [b]) => a - b)) {
    out += `        ${length} -> {\n${branches(group, 0, "            ")}        }\n`;
  }
  return out + `        else -> K_IDENT\n    }\n    // D-TIME-IN1=C: only exact in becomes a member after a postfix dot.\n    if kind == K_IN && (previous_kind == K_DOT || previous_kind == K_QUESTION_DOT) -> return K_IDENT\n    kind\n}\n${CLOSE}`;
}

export function replaceKeywordSpans(source, generated) {
  const marked = /\/\/ BEGIN GENERATED LEXER KEYWORD SPANS\n[\s\S]*?\/\/ END GENERATED LEXER KEYWORD SPANS/g;
  const matches = [...source.matchAll(marked)];
  if (matches.length !== 1) fail("expected exactly one generated keyword span block");
  return source.slice(0, matches[0].index) + generated + source.slice(matches[0].index + matches[0][0].length);
}

function main(args) {
  if (args.length > 1 || (args[0] && !["--check", "--write"].includes(args[0]))) fail("usage: lexer-keyword-rows.mjs [--check|--write]");
  const source = readFileSync(resolve(ROOT, SCAN_PATH), "utf8");
  const next = replaceKeywordSpans(source, renderKeywordSpans(readKeywordRows()));
  if (args[0] === "--write") writeFileSync(resolve(ROOT, SCAN_PATH), next);
  else if (next !== source) fail(`${SCAN_PATH} is stale; run node Tools/agent/lexer-keyword-rows.mjs --write`);
  console.log(`lexer keyword rows ${args[0] === "--write" ? "written" : "check passed"}`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main(process.argv.slice(2));
