#!/usr/bin/env node
// Project the canonical Jet lexer keyword table into borrowed Jet byte checks.
// Spellings remain Jet Syntax constants; Jet token tags own the kinds.
// Usage: node Tools/agent/lexer-keyword-rows.mjs [--check|--write]
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const SCAN_PATH = "Compiler/JetLexer/Source/Lexer/Scan.jet";
const OPEN = "// BEGIN GENERATED LEXER KEYWORD SPANS";
const CLOSE = "// END GENERATED LEXER KEYWORD SPANS";
const fail = (message) => { throw new Error(`lexer keyword rows: ${message}`); };
const uncomment = (text) => text.replace(/\/\/[^\n]*/g, "");

export function keywordRows(mapping, syntax, tokens, tags) {
  const body = uncomment(mapping).match(/^LEXER_KEYWORDS :: \[LexerKeyword\]\{([\s\S]*?)^\}/m)?.[1];
  if (!body) fail("unrecognized LEXER_KEYWORDS table");
  const constants = new Map();
  for (const match of syntax.matchAll(/^\s*(\w+)\s*::\s*("(?:[^"\\]|\\.)*")[^\S\n]*(?:\/\/[^\n]*)?$/gm)) {
    const values = constants.get(match[1]) ?? [];
    values.push(match[2]);
    constants.set(match[1], values);
  }
  const tokenTags = [...tokens.matchAll(/^TOKEN_(KW_\w+|TRUE|FALSE|SELF|NULL|BREAK|RETURN) :: \d+\s*$/gm)]
    .map((match) => `K_${match[1].replace(/^KW_/, "")}`);
  if (!tokenTags.length || new Set(tokenTags).size !== tokenTags.length) fail("unparsed/ambiguous keyword token kinds");
  const tagNames = [...tags.matchAll(/^(K_\w+) ::/gm)].map((match) => match[1]);
  const rows = [];
  const spellings = new Set(), kinds = new Set(), mappedTags = new Set();
  for (const line of body.split("\n").map((line) => line.trim()).filter(Boolean)) {
    const arm = line.match(/^(?:LexerKeyword)?\{constant: "(\w+)", kind: "(Kw\w+)", tag: "(K_\w+)"\},$/);
    if (!arm) fail(`unparsed keyword row: ${line}`);
    const [, constant, kind, tag] = arm;
    const values = constants.get(constant) ?? [];
    if (values.length !== 1) fail(`unresolved/ambiguous literal Syntax::${constant}`);
    let spelling;
    try { spelling = JSON.parse(values[0]); } catch { fail(`unsupported Jet literal Syntax::${constant}`); }
    if (!/^[A-Za-z][A-Za-z0-9_]*$/.test(spelling)) fail(`unsupported keyword spelling Syntax::${constant}`);
    const kindTag = `K_${kind.slice(2).replace(/([a-z0-9])([A-Z])/g, "$1_$2").toUpperCase()}`;
    if (kindTag !== tag || !tokenTags.includes(tag) || tagNames.filter((name) => name === tag).length !== 1) fail(`unresolved token kind ${kind}/${tag}`);
    if (spellings.has(spelling) || kinds.has(kind) || mappedTags.has(tag)) fail(`duplicate keyword spelling/kind ${constant}/${kind}`);
    spellings.add(spelling); kinds.add(kind); mappedTags.add(tag);
    rows.push({ constant, kind, tag, spelling, bytes: [...Buffer.from(spelling, "utf8")] });
  }
  if (!rows.length || tokenTags.some((tag) => !mappedTags.has(tag))) fail("incomplete keyword mapping/token coverage");
  return rows;
}

export function readKeywordRows(root = ROOT) {
  const read = (path) => readFileSync(resolve(root, path), "utf8");
  const directory = "Compiler/JetFoundation/Data/Syntax";
  const syntax = readdirSync(resolve(root, directory)).filter((name) => name.endsWith(".jet")).sort()
    .map((name) => read(`${directory}/${name}`)).join("\n");
  return keywordRows(read("Compiler/JetLexer/Data/Lexer/Keywords.jet"), syntax,
    read("Compiler/JetLexer/Source/Lexer/Tokens.jet"), read("Compiler/JetLexer/Source/Lexer/Terminators.jet"));
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
  let out = `${OPEN}\n// Derived from Jet Lexer/Keywords and Syntax literals; do not hand edit.\n// No spelling ownership here: Payload still builds the exact token text.\nfn word_kind(source: [U8], start: Int, end: Int, previous_kind: Int) -> Int Never! {\n    kind :: if end - start == {\n`;
  for (const [length, group] of [...lengths].sort(([a], [b]) => a - b)) {
    out += `        ${length} -> {\n${branches(group, 0, "            ")}        }\n`;
  }
  return out + `        else -> K_IDENT\n    }\n    // Only exact in becomes a member after a postfix dot.\n    if kind == K_IN && (previous_kind == K_DOT || previous_kind == K_QUESTION_DOT) -> return K_IDENT\n    kind\n}\n${CLOSE}`;
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
