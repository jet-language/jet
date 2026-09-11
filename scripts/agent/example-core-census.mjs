#!/usr/bin/env node

import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { dirname, join, relative, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const TIR_SOURCE = "crates/jet-codegen/src/Codegen/TIR/mod.rs";
const CORE_SOURCE = "crates/jet-foundation/src/Syntax/core_calls.rs";
const EXAMPLES_SOURCE = "examples";
const SCHEMA = "jet-example-core-census-v1";
const OUTPUT_JSON = "docs/audits/example-core-census-2026-09-05.json";
const OUTPUT_TSV = "docs/audits/example-core-census-2026-09-05.tsv";
const OUTPUT_MD = "docs/audits/example-core-census-2026-09-05.md";

const EXECUTE = process.argv.includes("--execute");
const POLICY_CHECK = process.argv.includes("--policy-check");
const JET_BINARY = resolve(ROOT, "target/debug/jet");
const EXECUTION_ROOT = resolve(
  process.env.JET_TEST_SCRATCH_DIR ?? resolve(ROOT, ".agent-scratch-example-core-census"),
  "example-core-runtime",
);
const EXECUTION_TIMEOUT_MS = Number(process.env.JET_EXAMPLE_CENSUS_TIMEOUT_MS ?? 120_000);

class CensusError extends Error {
  constructor(kind, message) {
    super(`${kind}: ${message}`);
    this.kind = kind;
  }
}

function fail(kind, message) {
  throw new CensusError(kind, message);
}

function sha256(value) {
  return createHash("sha256").update(String(value), "utf8").digest("hex");
}

function sourceSpanHash(file, text) {
  return sha256(`${file}\0${String(text).trim().replace(/\s+/g, " ")}`);
}

function validSpanHash(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function usage() {
  return [
    "usage: node scripts/agent/example-core-census.mjs [options]",
    "",
    "Options:",
    "  --json                 emit the complete deterministic JSON report (default)",
    "  --human                emit the short human summary instead of JSON",
    "  --format FORMAT       report format: json, tsv, or markdown",
    "  --out PATH             write the selected report serialization to PATH",
    "  --write                write the canonical JSON, TSV, and Markdown reports",
    "  --execute              run one representative golden-backed example on all tiers",
    "  --check                compare current source coverage with the checked-in JSON report",
    "  --policy-check         prove the coverage gate rejects a changed coverage row",
    "  --fail-on-uncovered    exit 1 when any construct or Core row lacks a golden example",
    "  --root PATH            repository root (default: inferred from this script)",
    "  --examples PATH        example directory relative to --root (default: examples)",
    "  -h, --help             show this help",
  ].join("\n");
}

function parseArgs(argv) {
  const options = {
    format: "json",
    formatSeen: false,
    failOnUncovered: false,
    root: ROOT,
    examples: EXAMPLES_SOURCE,
    out: null,
    write: false,
    help: false,
    check: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--execute" || argument === "--policy-check") {
      continue;
    }
    if (argument === "--json" || argument === "--human") {
      if (options.formatSeen) fail("argument error", "--json, --human, and --format may be given only once");
      options.format = argument === "--json" ? "json" : "human";
      options.formatSeen = true;
    } else if (argument === "--format") {
      if (options.formatSeen) fail("argument error", "--json, --human, and --format may be given only once");
      if (index + 1 >= argv.length || argv[index + 1].startsWith("-")) fail("argument error", "--format requires json, tsv, or markdown");
      const value = argv[++index];
      if (!["json", "tsv", "markdown"].includes(value)) fail("argument error", `unsupported report format ${value}`);
      options.format = value;
      options.formatSeen = true;
    } else if (argument === "--fail-on-uncovered") {
      if (options.failOnUncovered) fail("argument error", "--fail-on-uncovered may be given only once");
      options.failOnUncovered = true;
    } else if (argument === "--check") {
      if (options.check) fail("argument error", "--check may be given only once");
      options.check = true;
    } else if (argument === "--write") {
      if (options.write) fail("argument error", "--write may be given only once");
      options.write = true;
    } else if (argument === "--out" || argument === "--root" || argument === "--examples") {
      if (index + 1 >= argv.length || argv[index + 1].startsWith("-")) {
        fail("argument error", `${argument} requires a path`);
      }
      const value = argv[++index];
      if (argument === "--out") {
        if (options.out !== null) fail("argument error", "--out may be given only once");
        options.out = value;
      } else if (argument === "--root") {
        options.root = resolve(value);
      } else {
        options.examples = value;
      }
    } else if (argument === "--help" || argument === "-h") {
      options.help = true;
    } else {
      fail("argument error", `unknown option ${argument}`);
    }
  }
  if (options.write && options.out !== null) fail("argument error", "--write and --out cannot be combined");
  if (options.check && options.out !== null) fail("argument error", "--check and --out cannot be combined");
  return options;
}


function lineStarts(source) {
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "\n") starts.push(index + 1);
  }
  return starts;
}

function positionAt(starts, offset) {
  let low = 0;
  let high = starts.length;
  while (low + 1 < high) {
    const middle = (low + high) >> 1;
    if (starts[middle] <= offset) low = middle;
    else high = middle;
  }
  return { line: low + 1, column: offset - starts[low] + 1 };
}

function decodeRustString(body, file, line) {
  const output = [];
  for (let index = 0; index < body.length; index += 1) {
    if (body[index] !== "\\") {
      output.push(body[index]);
      continue;
    }
    const escape = body[++index];
    if (escape === undefined) fail("parser error", `${file}:${line}: trailing escape in Rust string literal`);
    const simple = { "0": "\0", "n": "\n", "r": "\r", "t": "\t", "\\": "\\", '"': '"', "'": "'" };
    if (escape in simple) {
      output.push(simple[escape]);
      continue;
    }
    if (escape === "x") {
      const hex = body.slice(index + 1, index + 3);
      if (!/^[0-9A-Fa-f]{2}$/u.test(hex)) fail("parser error", `${file}:${line}: invalid Rust hexadecimal escape`);
      output.push(String.fromCodePoint(Number.parseInt(hex, 16)));
      index += 2;
      continue;
    }
    if (escape === "u" && body[index + 1] === "{") {
      const end = body.indexOf("}", index + 2);
      const hex = end < 0 ? "" : body.slice(index + 2, end);
      if (!/^[0-9A-Fa-f]{1,6}$/u.test(hex)) fail("parser error", `${file}:${line}: invalid Rust Unicode escape`);
      const codePoint = Number.parseInt(hex, 16);
      if (codePoint > 0x10ffff) fail("parser error", `${file}:${line}: Rust Unicode escape is out of range`);
      output.push(String.fromCodePoint(codePoint));
      index = end;
      continue;
    }
    fail("parser error", `${file}:${line}: unsupported Rust string escape \\${escape}`);
  }
  return output.join("");
}

function rustStringValue(token, file) {
  if (!token || token.kind !== "string") {
    fail("parser error", `${file}:${token?.line ?? "?"}: expected a Rust string literal`);
  }
  const raw = token.value;
  if (raw.startsWith("r")) {
    const match = raw.match(/^r(#+)?"([\s\S]*)"\1$/u);
    if (!match) fail("parser error", `${file}:${token.line}: malformed raw string literal`);
    return match[2];
  }
  if (!raw.startsWith('"') || !raw.endsWith('"')) {
    fail("parser error", `${file}:${token.line}: malformed Rust string literal`);
  }
  return decodeRustString(raw.slice(1, -1), file, token.line);
}

function rustStringConstant(argument, constants) {
  if (argument.length === 1 && argument[0].kind === "ident" && constants.has(argument[0].value)) {
    return constants.get(argument[0].value);
  }
  if (
    argument.length >= 3
    && argument.every((token, index) => index % 2 === 0 ? token.kind === "ident" : token.value === "::")
  ) {
    const name = argument[argument.length - 1];
    if (constants.has(name.value)) return constants.get(name.value);
  }
  return null;
}

function rustStringArgument(argument, constants, file, line, label) {
  if (argument.length === 1 && argument[0].kind === "string") {
    return rustStringValue(argument[0], file);
  }
  const value = rustStringConstant(argument, constants);
  if (value !== null) return value;
  fail("unresolved registry row", `${file}:${line}: ${label} is not one Rust string literal or resolved string constant`);
}
function rustTokens(source, file) {
  const starts = lineStarts(source);
  const tokens = [];
  const add = (kind, value, start, end) => {
    const position = positionAt(starts, start);
    tokens.push({ kind, value, start, end, line: position.line, column: position.column });
  };
  const multi = [
    "..=", "..", "=>", "::", "->", "&&", "||", "==", "!=", "<=", ">=", "<<", ">>",
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "++", "--", "=>",
  ];
  let index = 0;
  while (index < source.length) {
    const c = source[index];
    const n = source[index + 1];
    if (/\s/u.test(c)) {
      index += 1;
      continue;
    }
    if (c === "/" && n === "/") {
      index += 2;
      while (index < source.length && source[index] !== "\n") index += 1;
      continue;
    }
    if (c === "/" && n === "*") {
      const start = index;
      index += 2;
      let depth = 1;
      while (index < source.length && depth > 0) {
        if (source[index] === "/" && source[index + 1] === "*") {
          depth += 1;
          index += 2;
        } else if (source[index] === "*" && source[index + 1] === "/") {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      if (depth !== 0) fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated block comment`);
      continue;
    }
    if (c === '"') {
      const start = index;
      const triple = source.slice(index, index + 3) === '"""';
      index += triple ? 3 : 1;
      let escaped = false;
      let closed = false;
      while (index < source.length) {
        if (!triple && !escaped && source[index] === '"') {
          index += 1;
          closed = true;
          break;
        }
        if (triple && !escaped && source.slice(index, index + 3) === '"""') {
          index += 3;
          closed = true;
          break;
        }
        if (!triple && source[index] === "\n") {
          fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated Rust string literal`);
        }
        if (!escaped && source[index] === "\\") escaped = true;
        else escaped = false;
        index += 1;
      }
      if (!closed) fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated Rust string literal`);
      add("string", source.slice(start, index), start, index);
      continue;
    }
    if (c === "r" && (n === '"' || n === "#")) {
      const start = index;
      let cursor = index + 1;
      while (source[cursor] === "#") cursor += 1;
      if (source[cursor] !== '"') {
        // This is an ordinary identifier such as `r#name`.
      } else {
        const hashes = source.slice(index + 1, cursor);
        const close = `"${hashes}`;
        const end = source.indexOf(close, cursor + 1);
        if (end < 0) fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated raw string literal`);
        index = end + close.length;
        add("string", source.slice(start, index), start, index);
        continue;
      }
    }
    if (c === "'") {
      const start = index;
      let cursor = index + 1;
      let escaped = false;
      let closed = false;
      while (cursor < source.length && source[cursor] !== "\n") {
        if (!escaped && source[cursor] === "'") {
          closed = true;
          cursor += 1;
          break;
        }
        if (!escaped && source[cursor] === "\\") escaped = true;
        else escaped = false;
        cursor += 1;
      }
      if (closed) {
        index = cursor;
        add("char", source.slice(start, index), start, index);
        continue;
      }
      add("punct", c, index, index + 1);
      index += 1;
      continue;
    }
    if (/[A-Za-z_]/u.test(c)) {
      const start = index;
      index += 1;
      while (index < source.length && /[A-Za-z0-9_]/u.test(source[index])) index += 1;
      add("ident", source.slice(start, index), start, index);
      continue;
    }
    if (/[0-9]/u.test(c)) {
      const start = index;
      index += 1;
      while (index < source.length && /[0-9_]/u.test(source[index])) index += 1;
      if (source[index] === "." && source[index + 1] !== ".") {
        index += 1;
        while (index < source.length && /[0-9_]/u.test(source[index])) index += 1;
      }
      if (source[index] === "e" || source[index] === "E") {
        index += 1;
        if (source[index] === "+" || source[index] === "-") index += 1;
        while (index < source.length && /[0-9_]/u.test(source[index])) index += 1;
      }
      add("number", source.slice(start, index), start, index);
      continue;
    }
    let operator = null;
    for (const candidate of multi) {
      if (source.startsWith(candidate, index)) {
        operator = candidate;
        break;
      }
    }
    if (operator) {
      add("punct", operator, index, index + operator.length);
      index += operator.length;
      continue;
    }
    add("punct", c, index, index + 1);
    index += 1;
  }
  return { tokens, starts };
}

function jetTokens(source, file) {
  const starts = lineStarts(source);
  const tokens = [];
  const add = (kind, value, start, end) => {
    const position = positionAt(starts, start);
    tokens.push({ kind, value, start, end, line: position.line, column: position.column });
  };
  const multi = [
    "...", "..<", "..=", "..", "??", "?.", "::", ":=", "->", "=>", "==", "!=", "<=", ">=", "&&", "||",
    "<<", ">>", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "++", "--",
  ];
  let index = 0;
  while (index < source.length) {
    const c = source[index];
    const n = source[index + 1];
    if (/\s/u.test(c)) {
      index += 1;
      continue;
    }
    if (c === "/" && n === "/") {
      index += 2;
      while (index < source.length && source[index] !== "\n") index += 1;
      continue;
    }
    if (c === "/" && n === "*") {
      const start = index;
      index += 2;
      let depth = 1;
      while (index < source.length && depth > 0) {
        if (source[index] === "/" && source[index + 1] === "*") {
          depth += 1;
          index += 2;
        } else if (source[index] === "*" && source[index + 1] === "/") {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      if (depth !== 0) fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated block comment`);
      continue;
    }
    if (c === '"') {
      const start = index;
      const triple = source.slice(index, index + 3) === '"""';
      index += triple ? 3 : 1;
      let escaped = false;
      let closed = false;
      while (index < source.length) {
        if (!triple && !escaped && source[index] === '"') {
          index += 1;
          closed = true;
          break;
        }
        if (triple && !escaped && source.slice(index, index + 3) === '"""') {
          index += 3;
          closed = true;
          break;
        }
        if (!triple && source[index] === "\n") {
          fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated Jet string literal`);
        }
        if (!escaped && source[index] === "\\") escaped = true;
        else escaped = false;
        index += 1;
      }
      if (!closed) fail("parser error", `${file}:${positionAt(starts, start).line}: unterminated Jet string literal`);
      add("string", source.slice(start, index), start, index);
      continue;
    }
    if (c === "'") {
      const start = index;
      let cursor = index + 1;
      let escaped = false;
      let closed = false;
      while (cursor < source.length && source[cursor] !== "\n") {
        if (!escaped && source[cursor] === "'") {
          closed = true;
          cursor += 1;
          break;
        }
        if (!escaped && source[cursor] === "\\") escaped = true;
        else escaped = false;
        cursor += 1;
      }
      if (closed) {
        index = cursor;
        add("char", source.slice(start, index), start, index);
        continue;
      }
      add("punct", c, index, index + 1);
      index += 1;
      continue;
    }
    if (/[A-Za-z_]/u.test(c)) {
      const start = index;
      index += 1;
      while (index < source.length && /[A-Za-z0-9_]/u.test(source[index])) index += 1;
      add("ident", source.slice(start, index), start, index);
      continue;
    }
    if (/[0-9]/u.test(c)) {
      const start = index;
      index += 1;
      while (index < source.length && /[0-9_]/u.test(source[index])) index += 1;
      if (source[index] === "." && source[index + 1] !== ".") {
        index += 1;
        while (index < source.length && /[0-9_]/u.test(source[index])) index += 1;
      }
      if (source[index] === "e" || source[index] === "E") {
        index += 1;
        if (source[index] === "+" || source[index] === "-") index += 1;
        while (index < source.length && /[0-9_]/u.test(source[index])) index += 1;
      }
      add("number", source.slice(start, index), start, index);
      continue;
    }
    let operator = null;
    for (const candidate of multi) {
      if (source.startsWith(candidate, index)) {
        operator = candidate;
        break;
      }
    }
    if (operator) {
      add("punct", operator, index, index + operator.length);
      index += operator.length;
      continue;
    }
    add("punct", c, index, index + 1);
    index += 1;
  }
  return { tokens, starts };
}

function findSequence(tokens, values, start = 0, end = tokens.length) {
  outer: for (let index = start; index + values.length <= end; index += 1) {
    for (let offset = 0; offset < values.length; offset += 1) {
      if (tokens[index + offset].value !== values[offset]) continue outer;
    }
    return index;
  }
  return -1;
}

function matching(tokens, start, opening, closing) {
  if (!tokens[start] || tokens[start].value !== opening) {
    fail("parser error", `expected ${opening} at token ${start}`);
  }
  let depth = 0;
  for (let index = start; index < tokens.length; index += 1) {
    if (tokens[index].value === opening) depth += 1;
    else if (tokens[index].value === closing) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  fail("parser error", `${tokens[start].line}:${tokens[start].column}: unbalanced ${opening}`);
}

function splitTopLevel(tokens, start, end) {
  const groups = [];
  let groupStart = start;
  const stack = [];
  const pairs = new Map([["(", ")"], ["[", "]"], ["{", "}"]]);
  for (let index = start; index < end; index += 1) {
    const value = tokens[index].value;
    if (pairs.has(value)) stack.push(pairs.get(value));
    else if (stack.length > 0 && value === stack[stack.length - 1]) stack.pop();
    else if (stack.length === 0 && value === ",") {
      groups.push(tokens.slice(groupStart, index));
      groupStart = index + 1;
    }
  }
  if (stack.length > 0) fail("parser error", `unbalanced delimiter ${stack[stack.length - 1]}`);
  groups.push(tokens.slice(groupStart, end));
  return groups.filter((group) => group.length > 0);
}

function parseRustEnum(source, file, enumName) {
  const { tokens } = rustTokens(source, file);
  let enumIndex = findSequence(tokens, ["pub", "enum", enumName, "{"]);
  if (enumIndex < 0) enumIndex = findSequence(tokens, ["enum", enumName, "{"]);
  if (enumIndex < 0) fail("parser error", `${file}: missing enum ${enumName}`);
  const open = enumIndex + (tokens[enumIndex].value === "pub" ? 3 : 2);
  const close = matching(tokens, open, "{", "}");
  const variants = [];
  let braces = 1;
  let parens = 0;
  let brackets = 0;
  for (let index = open + 1; index < close; index += 1) {
    const value = tokens[index].value;
    if (value === "{") braces += 1;
    else if (value === "}") braces -= 1;
    else if (value === "(") parens += 1;
    else if (value === ")") parens -= 1;
    else if (value === "[") brackets += 1;
    else if (value === "]") brackets -= 1;
    if (braces === 1 && parens === 0 && brackets === 0 && tokens[index].kind === "ident") {
      variants.push({
        name: tokens[index].value,
        source: {
          file,
          line: tokens[index].line,
          column: tokens[index].column,
          span_hash: sourceSpanHash(file, sourceLine(source, tokens[index].line)),
        },
      });
    }
  }
  if (variants.length === 0) fail("parser error", `${file}:${tokens[enumIndex].line}: enum ${enumName} has no variants`);
  const seen = new Set();
  for (const variant of variants) {
    if (seen.has(variant.name)) fail("parser error", `${file}:${variant.source.line}: duplicate ${enumName} variant ${variant.name}`);
    seen.add(variant.name);
  }
  return variants;
}

function collectRustConstants(tokens, file) {
  const constants = new Map();
  for (let index = 0; index + 2 < tokens.length; index += 1) {
    if (tokens[index].value !== "const" && tokens[index].value !== "static") continue;
    const name = tokens[index + 1];
    if (!name || name.kind !== "ident") continue;
    let equal = index + 2;
    while (equal < tokens.length && tokens[equal].value !== "=") {
      if (tokens[equal].value === ";") break;
      equal += 1;
    }
    if (!tokens[equal] || tokens[equal].value !== "=") continue;
    const value = tokens[equal + 1];
    if (value?.kind === "string") {
      try {
        constants.set(name.value, rustStringValue(value, file));
      } catch (error) {
        // Other Syntax constants may use Rust-only escapes. They are not
        // registry receiver types unless a later resolver explicitly asks for
        // them; defer that failure until then.
        if (!(error instanceof CensusError) || error.kind !== "parser error") throw error;
      }
    }
  }
  return constants;
}
function loadImportedRustConstants(source, root, constants) {
  const requested = new Map();
  const request = (original, alias, line) => {
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/u.test(original) || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(alias)) return;
    if (requested.has(alias) && requested.get(alias) !== original) {
      fail("unresolved import", `${CORE_SOURCE}:${line}: imported alias ${alias} names both ${requested.get(alias)} and ${original}`);
    }
    requested.set(alias, original);
  };
  const syntaxPath = "(?:(?:crate|self|super)::)?Syntax::";
  for (const match of source.matchAll(/^\s*use\s+([^;]+);/gmu)) {
    const clause = match[1];
    const line = source.slice(0, match.index).split("\n").length;
    const grouped = clause.match(new RegExp(`${syntaxPath}\\{([^}]*)\\}`, "u"));
    if (grouped) {
      for (const item of grouped[1].split(",")) {
        const parts = item.trim().split(/\s+as\s+/u);
        request(parts[0]?.trim(), parts[1]?.trim() ?? parts[0]?.trim(), line);
      }
    }
    const directPattern = new RegExp(`(?:^|[,{]\\s*)${syntaxPath}([A-Za-z_][A-Za-z0-9_]*)(?:\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*))?`, "gu");
    for (const direct of clause.matchAll(directPattern)) {
      request(direct[1], direct[2] ?? direct[1], line);
    }
  }
  for (const match of source.matchAll(/(?:crate|self|super)::Syntax::([A-Za-z_][A-Za-z0-9_]*)/gu)) {
    const line = source.slice(0, match.index).split("\n").length;
    request(match[1], match[1], line);
  }

  if (requested.size === 0) return constants;
  const directory = dirname(resolve(root, CORE_SOURCE));
  let entries;
  try {
    entries = readdirSync(directory, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
      .sort((left, right) => compareText(left.name, right.name));
  } catch (error) {
    fail("source error", `cannot read Core syntax directory ${directory}: ${error.message}`);
  }
  for (const entry of entries) {
    if (requested.size === 0) break;
    const full = join(directory, entry.name);
    const text = readFileSync(full, "utf8");
    const relativeFile = normalizePath(relative(root, full));
    const discovered = collectRustConstants(rustTokens(text, relativeFile).tokens, relativeFile);
    for (const [name, value] of discovered) {
      for (const [alias, original] of requested) {
        if (original === name) {
          constants.set(alias, value);
          requested.delete(alias);
        }
      }
    }
  }
  return constants;
}

function parseReceiverTypes(argument, constants, file, line) {
  const arrayIndex = argument.findIndex((token) => token.value === "[");
  if (arrayIndex < 0 || argument.slice(0, arrayIndex).some((token) => token.value !== "&")) {
    fail("parser error", `${file}:${line}: receiver row needs a receiver type array`);
  }
  const close = (() => {
    let depth = 0;
    for (let index = arrayIndex; index < argument.length; index += 1) {
      if (argument[index].value === "[") depth += 1;
      else if (argument[index].value === "]") {
        depth -= 1;
        if (depth === 0) return index;
      }
    }
    return -1;
  })();
  if (close < 0 || close !== argument.length - 1) {
    fail("parser error", `${file}:${line}: unbalanced receiver type array`);
  }
  const values = [];
  for (const item of splitTopLevel(argument, arrayIndex + 1, close)) {
    values.push(rustStringArgument(item, constants, file, line, "receiver type"));
  }
  if (values.length === 0) fail("parser error", `${file}:${line}: receiver row has no receiver types`);
  return values;
}

function parseCoreRegistry(source, file, root) {
  const { tokens } = rustTokens(source, file);
  const constants = loadImportedRustConstants(source, root, collectRustConstants(tokens, file));
  const nameIndex = findSequence(tokens, ["CORE_CALLS"]);
  if (nameIndex < 0) fail("parser error", `${file}: missing CORE_CALLS registry`);
  let equal = nameIndex + 1;
  while (equal < tokens.length && tokens[equal].value !== "=") equal += 1;
  if (equal >= tokens.length) fail("parser error", `${file}:${tokens[nameIndex].line}: CORE_CALLS has no initializer`);
  let open = equal + 1;
  while (open < tokens.length && tokens[open].value !== "[") open += 1;
  if (open >= tokens.length) fail("parser error", `${file}:${tokens[nameIndex].line}: CORE_CALLS initializer has no array`);
  const close = matching(tokens, open, "[", "]");
  const rows = [];
  const seen = new Set();
  for (const item of splitTopLevel(tokens, open + 1, close)) {
    const coreConstructor = findSequence(item, ["CoreCallRecord", "::"]);
    const helperConstructor = findSequence(item, ["sema_web_call"]);
    const constructor = coreConstructor >= 0 ? coreConstructor : helperConstructor;
    if (constructor < 0) {
      fail("unresolved registry row", `${file}:${item[0]?.line ?? tokens[nameIndex].line}: registry item has no recognized Core row constructor`);
    }
    const methodToken = coreConstructor >= 0 ? item[constructor + 2] : item[constructor];
    const method = coreConstructor >= 0 ? methodToken?.value : "sema_web_call";
    if (coreConstructor >= 0 && !["new", "new_with_coverage", "receiver", "receiver_with_symbol", "receiver_with_coverage"].includes(method)) {
      fail("unresolved registry row", `${file}:${methodToken?.line ?? item[constructor].line}: unsupported CoreCallRecord constructor ${method ?? "<missing>"}`);
    }
    const callOpen = constructor + (coreConstructor >= 0 ? 3 : 1);
    if (item[callOpen]?.value !== "(") fail("parser error", `${file}:${item[constructor].line}: ${method} row has no argument list`);
    const callClose = matching(item, callOpen, "(", ")");
    const args = splitTopLevel(item, callOpen + 1, callClose);
    let row;
    if (method.startsWith("receiver")) {
      if (args.length < 2) fail("parser error", `${file}:${item[constructor].line}: receiver row has too few arguments`);
      const receiverTypes = parseReceiverTypes(args[0], constants, file, item[constructor].line);
      const member = rustStringArgument(args[1], constants, file, item[constructor].line, "receiver member");
      const key = `receiver:${receiverTypes.join("|")}.${member}`;
      row = {
        family: "core",
        kind: "receiver",
        key,
        module: "",
        member,
        receiver_types: receiverTypes,
        source: {
          file,
          line: item[constructor].line,
          column: item[constructor].column,
          span_hash: sourceSpanHash(file, sourceLine(source, item[constructor].line)),
        },
      };
    } else {
      if (args.length < 2) fail("parser error", `${file}:${item[constructor].line}: ${method} row has too few arguments`);
      const module = rustStringArgument(args[0], constants, file, item[constructor].line, "Core module");
      const member = rustStringArgument(args[1], constants, file, item[constructor].line, "Core member");
      const key = `plain:${module}.${member}`;
      row = {
        family: "core",
        kind: "plain",
        key,
        module,
        member,
        receiver_types: [],
        source: {
          file,
          line: item[constructor].line,
          column: item[constructor].column,
          span_hash: sourceSpanHash(file, sourceLine(source, item[constructor].line)),
        },
      };
    }
    if (seen.has(row.key)) fail("parser error", `${file}:${row.source.line}: duplicate Core registry row ${row.key}`);
    seen.add(row.key);
    rows.push(row);
  }
  if (rows.length === 0) fail("parser error", `${file}:${tokens[nameIndex].line}: CORE_CALLS has no records`);
  return rows;
}

function normalizePath(value) {
  return value.split("\\").join("/");
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function walkFiles(directory, root, predicate) {
  const output = [];
  let entries;
  try {
    entries = readdirSync(directory, { withFileTypes: true }).sort((left, right) => compareText(left.name, right.name));
  } catch (error) {
    fail("source error", `cannot read examples directory ${directory}: ${error.message}`);
  }
  for (const entry of entries) {
    if (entry.name.startsWith(".")) continue;
    const full = join(directory, entry.name);
    if (entry.isDirectory()) {
      output.push(...walkFiles(full, root, predicate));
    } else if (entry.isFile() && predicate(entry.name, full)) {
      output.push({ absolute: full, file: normalizePath(relative(root, full)) });
    }
  }
  return output;
}

function walkJetFiles(directory, root) {
  return walkFiles(directory, root, (name) => name.endsWith(".jet") && name !== "package.jet");
}

function walkGoldenFiles(directory, root) {
  return walkFiles(directory, root, (name) => name.endsWith(".out"));
}


function goldenPathsForExample(example, examplesRoot, goldenFiles) {
  const byAbsolute = new Map(goldenFiles.map((file) => [file.absolute, file.file]));
  const relativeExample = normalizePath(relative(examplesRoot, example.absolute));
  const paths = new Set();
  const add = (absolute) => {
    if (byAbsolute.has(absolute)) paths.add(byAbsolute.get(absolute));
  };
  const addExpectedPrefix = (prefix) => {
    const normalizedPrefix = normalizePath(prefix);
    for (const golden of goldenFiles) {
      const relativeGolden = normalizePath(relative(examplesRoot, golden.absolute));
      if (relativeGolden === `${normalizedPrefix}.out` || (relativeGolden.startsWith(`${normalizedPrefix}.`) && relativeGolden.endsWith(".out"))) {
        paths.add(golden.file);
      }
    }
  };
  const stem = relativeExample.slice(0, -".jet".length);
  if (relativeExample.startsWith("features/")) {
    const featureStem = stem.startsWith("features/") ? stem.slice("features/".length) : stem;
    const key = featureStem.endsWith("/run") ? featureStem.slice(0, -"/run".length) : featureStem;
    if (key.length > 0) addExpectedPrefix(`features/expected/${key}`);
  } else if (relativeExample.startsWith("suites/")) {
    addExpectedPrefix(`suites/expected/${stem.slice("suites/".length)}`);
  }
  add(join(dirname(example.absolute), "expected.out"));
  return [...paths].sort();
}
function isExecutableExample(tokens) {
  if (findSequence(tokens, ["fn", "run"]) >= 0 || findSequence(tokens, ["fn", "main"]) >= 0) return true;
  const declarations = new Set(["use", "fn", "pub", "struct", "enum", "trait", "impl", "module", "derive", "const", "extern", "type", "import", "#", "@"]);
  const depth = [];
  let braces = 0;
  for (const token of tokens) {
    if (token.value === "}") braces -= 1;
    if (braces === 0 && (depth.length === 0 || depth[depth.length - 1] !== token.line)) {
      depth.push(token.line);
      if (!declarations.has(token.value)) return true;
    }
    if (token.value === "{") braces += 1;
  }
  return false;
}

function parseJetImports(tokens, file) {
  const imports = new Map();
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "use" || tokens[index + 1]?.value !== "core") continue;
    const start = tokens[index];
    if (tokens[index + 2]?.value !== ".") fail("parser error", `${file}:${start.line}: Core import must use a dotted module path`);
    const parts = [];
    let cursor = index + 3;
    while (cursor < tokens.length && tokens[cursor].line === start.line) {
      if (tokens[cursor].kind !== "ident") break;
      parts.push(tokens[cursor].value);
      cursor += 1;
      if (tokens[cursor]?.value !== ".") break;
      cursor += 1;
    }
    if (parts.length === 0) fail("parser error", `${file}:${start.line}: Core import has no module name`);
    let alias = parts[parts.length - 1];
    if (tokens[cursor]?.value === "as") {
      if (tokens[cursor + 1]?.kind !== "ident") fail("parser error", `${file}:${start.line}: Core import alias is missing`);
      alias = tokens[cursor + 1].value;
    }
    const module = `core.${parts.join(".")}`;
    if (imports.has(alias) && imports.get(alias) !== module) {
      fail("unresolved import", `${file}:${start.line}: alias ${alias} names both ${imports.get(alias)} and ${module}`);
    }
    imports.set(alias, module);
  }
  return imports;
}

function callOpen(tokens, index) {
  let cursor = index + 1;
  if (tokens[cursor]?.value === "<") {
    let depth = 0;
    for (; cursor < tokens.length; cursor += 1) {
      if (tokens[cursor].value === "<") depth += 1;
      else if (tokens[cursor].value === ">") depth -= 1;
      else if (tokens[cursor].value === ">>") depth -= 2;
      if (depth <= 0) {
        cursor += 1;
        break;
      }
    }
  }
  return tokens[cursor]?.value === "(" ? cursor : -1;
}

function findMatchingBackward(tokens, index, opening, closing) {
  let depth = 0;
  for (let cursor = index; cursor >= 0; cursor -= 1) {
    if (tokens[cursor].value === closing) depth += 1;
    else if (tokens[cursor].value === opening) {
      depth -= 1;
      if (depth === 0) return cursor;
    }
  }
  return -1;
}


function rootedCoreAlias(tokens, memberIndex, imports) {
  if (tokens[memberIndex - 1]?.value !== ".") return null;
  const direct = tokens[memberIndex - 2];
  if (direct?.kind === "ident" && imports.has(direct.value)) return direct.value;
  let cursor = memberIndex - 2;
  if (tokens[cursor]?.value !== ">" && tokens[cursor]?.value !== ">>") return null;
  let depth = 0;
  for (; cursor >= 0; cursor -= 1) {
    if (tokens[cursor].value === ">" || tokens[cursor].value === ">>") depth += tokens[cursor].value === ">>" ? 2 : 1;
    else if (tokens[cursor].value === "<") {
      depth -= 1;
      if (depth <= 0) break;
    }
  }
  const owner = tokens[cursor - 1];
  if (owner?.kind !== "ident" || tokens[cursor - 2]?.value !== ".") return null;
  const alias = tokens[cursor - 3];
  return alias?.kind === "ident" && imports.has(alias.value) ? alias.value : null;
}
function declarationOperatorLength(tokens, index) {
  if (tokens[index + 1]?.value === "::") return 1;
  if (tokens[index + 1]?.value === ":" && tokens[index + 2]?.value === "=") return 2;
  if (tokens[index + 1]?.value === "=") return 1;
  return 0;
}
function parseJetContext(tokens, imports, receiverRows) {
  const structNames = new Set();
  const enumNames = new Set();
  const moduleNames = new Set();
  const distinctNames = new Set();
  const fnNames = new Set();
  const fnReturns = new Map();
  const bindingTypes = new Map();
  const bindingTypeScopes = [];
  const bindingNames = new Set();
  const declarationIndices = new Set();
  const fieldFnNames = new Set();
  const methodNames = new Set();
  const receiverTypes = new Set(receiverRows.flatMap((row) => row.receiver_types));
  const anyTypeNames = new Set(receiverTypes);

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const next = tokens[index + 1];
    if (["struct", "enum", "module", "trait"].includes(token.value) && next?.kind === "ident") {
      if (token.value === "struct") structNames.add(next.value);
      if (token.value === "enum") enumNames.add(next.value);
      if (token.value === "module") moduleNames.add(next.value);
      anyTypeNames.add(next.value);
      declarationIndices.add(index + 1);
    }
    if (token.value === "fn" && next?.kind === "ident") {
      fnNames.add(next.value);
      methodNames.add(next.value);
      declarationIndices.add(index + 1);
      const open = tokens[index + 2]?.value === "<"
        ? (() => {
            let depth = 0;
            for (let cursor = index + 2; cursor < tokens.length; cursor += 1) {
              if (tokens[cursor].value === "<") depth += 1;
              else if (tokens[cursor].value === ">") depth -= 1;
              if (depth === 0) return cursor + 1;
            }
            return -1;
          })()
        : index + 2;
      if (tokens[open]?.value === "(") {
        const close = matching(tokens, open, "(", ")");
        const params = splitTopLevel(tokens, open + 1, close);
        for (const parameter of params) {
          const colon = parameter.findIndex((item) => item.value === ":");
          if (colon > 0 && parameter[0].kind === "ident" && parameter[colon + 1]?.kind === "ident") {
            bindingNames.add(parameter[0].value);
            declarationIndices.add(tokens.indexOf(parameter[0], open + 1));
            const type = parameter[colon + 1].value;
            if (receiverTypes.has(type)) bindingTypes.set(parameter[0].value, type);
          }
        }
        const result = tokens[close + 1];
        if (result?.kind === "ident" && tokens[close + 2]?.value === "->" && receiverTypes.has(result.value)) {
          fnReturns.set(next.value, result.value);
        }
      }
    }
    if (token.value === "distinct" && tokens[index - 2]?.value === "::" && tokens[index - 3]?.kind === "ident") {
      distinctNames.add(tokens[index - 3].value);
      anyTypeNames.add(tokens[index - 3].value);
    }
    if (token.kind === "ident" && next?.value === ":" && tokens[index + 2]?.value === "fn") {
      fieldFnNames.add(token.value);
    }
  }
  for (const name of [...structNames, ...enumNames]) anyTypeNames.add(name);

  // Binding/type collection is intentionally a small structural pass. It only
  // accepts a known receiver type or a known function return type, never an
  // arbitrary identifier after `=`.
  for (let index = 0; index + 2 < tokens.length; index += 1) {
    const name = tokens[index];
    const operator = tokens[index + 1];
    if (name.kind !== "ident" || !["::", ":=", "="].includes(operator.value)) continue;
    bindingNames.add(name.value);
    declarationIndices.add(index);
    const rhs = tokens[index + 2];
    if (rhs?.kind === "ident" && receiverTypes.has(rhs.value)) {
      if (["{", "(", "."].includes(tokens[index + 3]?.value)) bindingTypes.set(name.value, rhs.value);
    }
    if (rhs?.kind === "ident" && fnReturns.has(rhs.value) && tokens[index + 3]?.value === "(") {
      bindingTypes.set(name.value, fnReturns.get(rhs.value));
    }
  }
  return {
    structNames,
    enumNames,
    moduleNames,
    distinctNames,
    fnNames,
    fnReturns,
    bindingTypes,
    bindingNames,
    declarationIndices,
    fieldFnNames,
    methodNames,
    receiverTypes,
    anyTypeNames,
    bindingTypeScopes,
  };
}

function sourceLine(source, line) {
  return (source.split(/\r?\n/u)[line - 1] ?? "").trim();
}

function sourceEvidence(file, source, line, column, text = null) {
  const lineText = text ?? sourceLine(source, line);
  return {
    file,
    line,
    column,
    text: lineText,
    span_hash: sourceSpanHash(file, lineText),
  };
}

function typeAcronym(name) {
  const capitals = [...name].filter((character) => /[A-Z]/u.test(character)).join("").toLowerCase();
  return capitals || name.replace(/[^A-Za-z0-9]/gu, "").toLowerCase();
}

function inferCoreReturnType(module, member, imports, receiverRows) {
  const moduleTail = module.split(".").at(-1) ?? "";
  const normalizedMember = member.replace(/_/gu, "").toLowerCase();
  const normalizedTail = moduleTail.replace(/_/gu, "").toLowerCase();
  const receiverType = receiverRows
    .flatMap((row) => row.receiver_types.map((type) => ({ type, member: row.member })))
    .find(({ type, member: rowMember }) => rowMember === "new" && (
      normalizedTail === type.replace(/_/gu, "").toLowerCase()
      || normalizedTail === typeAcronym(type)
      || type.replace(/_/gu, "").toLowerCase().includes(normalizedTail)
    ))?.type;
  if (member === "new" && receiverType) return receiverType;
  const known = new Map([
    ["time.local_time", "LocalTime"],
    ["time.time", "LocalTime"],
    ["time.datetime", "DateTime"],
    ["time.now_utc", "DateTime"],
    ["time.today", "LocalDate"],
    ["time.instant", "Instant"],
    ["time.utc", "Zone"],
    ["time.zoned", "ZonedDateTime"],
    ["time.date", "Date"],
    ["time.period", "Period"],
    ["math.fraction", "Fraction"],
    ["math.decimal", "Decimal"],
  ]);
  for (const [key, type] of known) {
    const suffix = key.split(".");
    if (normalizedTail === suffix[0] && normalizedMember === suffix[1]) return type;
  }
  return null;
}

function inferReceiverReturnType(receiver, member) {
  const exact = new Map([
    ["DateTime.date", "Date"],
    ["DateTime.time", "LocalTime"],
    ["DateTime.in_zone", "ZonedDateTime"],
    ["DateTime.to_datetime", "DateTime"],
    ["ZonedDateTime.date", "Date"],
    ["ZonedDateTime.time", "LocalTime"],
    ["ZonedDateTime.to_datetime", "DateTime"],
    ["DateTime.zone", "Zone"],
  ]);
  return exact.get(`${receiver}.${member}`) ?? null;
}

function enrichBindingTypes(tokens, context, imports, receiverRows) {
  const collections = new Map();
  for (let index = 0; index + 5 < tokens.length; index += 1) {
    const operatorLength = declarationOperatorLength(tokens, index);
    if (tokens[index].kind !== "ident" || operatorLength === 0) continue;
    const rhsIndex = index + 1 + operatorLength;
    if (tokens[rhsIndex]?.value !== "[") continue;
    const element = tokens[rhsIndex + 1];
    const close = tokens.findIndex((token, offset) => offset > rhsIndex + 1 && token.value === "]");
    if (element?.kind === "ident" && close > 0 && tokens[close + 1]?.value === "{") collections.set(tokens[index].value, element.value);
  }
  for (let index = 0; index + 3 < tokens.length; index += 1) {
    if (tokens[index].value !== "loop" || tokens[index + 1]?.kind !== "ident") continue;
    const inIndex = tokens.findIndex((token, offset) => offset > index && offset < index + 12 && token.value === "in");
    if (inIndex < 0) continue;
    const variable = tokens[index + 1].value;
    const source = tokens[inIndex + 1];
    let type = null;
    if (source?.kind === "ident" && collections.has(source.value)) type = collections.get(source.value);
    if (source?.value === "[" && tokens[inIndex + 2]?.kind === "ident") type = tokens[inIndex + 2].value;
    if (!type) continue;
    context.bindingTypes.set(variable, type);
    const bodyOpenOffset = tokens.slice(inIndex + 1, inIndex + 16).findIndex((token) => token.value === "{");
    if (bodyOpenOffset >= 0) {
      const open = inIndex + 1 + bodyOpenOffset;
      const close = matching(tokens, open, "{", "}");
      if (close > open) context.bindingTypeScopes.push({ name: variable, type, start: open, end: close });
    }
  }
  for (let pass = 0; pass < 3; pass += 1) {
    for (let index = 0; index + 3 < tokens.length; index += 1) {
      const name = tokens[index];
      const operatorLength = declarationOperatorLength(tokens, index);
      if (name.kind !== "ident" || operatorLength === 0) continue;
      const owner = tokens[index + 1 + operatorLength];
      if (owner?.kind !== "ident") continue;
      let member = null;
      if (tokens[index + 2 + operatorLength]?.value === ".") member = tokens[index + 3 + operatorLength]?.value;
      if (!member) continue;
      if (imports.has(owner.value)) {
        const type = inferCoreReturnType(imports.get(owner.value), member, imports, receiverRows);
        if (type) context.bindingTypes.set(name.value, type);
      } else {
        const receiver = context.receiverTypes.has(owner.value) ? owner.value : context.bindingTypes.get(owner.value);
        const type = receiver ? inferReceiverReturnType(receiver, member) : null;
        if (type) context.bindingTypes.set(name.value, type);
        else if (context.anyTypeNames.has(owner.value)) context.bindingTypes.set(name.value, owner.value);
      }
    }
  }
  return context;
}

function receiverTypeFor(tokens, memberIndex, context) {
  if (tokens[memberIndex - 1]?.value !== ".") return null;
  const receiver = tokens[memberIndex - 2];
  if (!receiver || receiver.kind !== "ident") return null;
  for (let index = context.bindingTypeScopes.length - 1; index >= 0; index -= 1) {
    const scope = context.bindingTypeScopes[index];
    if (scope.name === receiver.value && memberIndex > scope.start && memberIndex < scope.end) return scope.type;
  }
  if (context.receiverTypes.has(receiver.value)) return receiver.value;
  return context.bindingTypes.get(receiver.value) ?? null;
}

function inspectTable(tokens, ifIndex) {
  if (tokens[ifIndex + 1]?.value !== "{") return null;
  const close = matching(tokens, ifIndex + 1, "{", "}");
  const body = tokens.slice(ifIndex + 2, close);
  let hasAfter = false;
  let hasReceiverArm = false;
  let hasVariant = false;
  let hasRange = false;
  let hasArrow = false;
  for (let index = 0; index < body.length; index += 1) {
    if (body[index].value === "after") hasAfter = true;
    if (body[index].value === "->") hasArrow = true;
    if (body[index].value === ".." || body[index].value === "..<" || body[index].value === "..=") hasRange = true;
    if (body[index].value === "." && body[index + 1]?.kind === "ident" && /^[A-Z]/u.test(body[index + 1].value)) hasVariant = true;
    if (body[index].kind === "ident" && body[index + 1]?.value === ",") hasReceiverArm = true;
  }
  return { close, hasAfter, hasReceiverArm, hasVariant, hasRange, hasArrow };
}

function analyzeExample(example, coreRows, exprNames, stmtNames, receiverByTypeMember, coreByModuleMember) {
  const { source, file, tokens } = example;
  const imports = parseJetImports(tokens, file);
  let context = parseJetContext(tokens, imports, coreRows.filter((row) => row.kind === "receiver"));
  context = enrichBindingTypes(tokens, context, imports, coreRows.filter((row) => row.kind === "receiver"));
  const evidence = new Map();
  const seenEvidence = new Map();
  const add = (name, token, text = null) => {
    if (!token) return;
    if (!exprNames.has(name) && !stmtNames.has(name)) return;
    if (!seenEvidence.has(name)) seenEvidence.set(name, new Set());
    const key = `${file}:${token.line}:${token.column}`;
    if (seenEvidence.get(name).has(key)) return;
    seenEvidence.get(name).add(key);
    if (!evidence.has(name)) evidence.set(name, []);
    evidence.get(name).push({
      ...sourceEvidence(file, source, token.line, token.column, text),
      golden: example.golden,
      golden_paths: example.golden_paths,
    });
  };
  const addCore = (row, token) => {
    if (!row) return;
    if (!row.examples) row.examples = [];
    const key = `${file}:${token.line}:${token.column}`;
    if (row._seen.has(key)) return;
    row._seen.add(key);
    row.examples.push({
      ...sourceEvidence(file, source, token.line, token.column),
      golden: example.golden,
      golden_paths: example.golden_paths,
    });
  };
  const calls = [];
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].kind !== "ident") continue;
    const open = callOpen(tokens, index);
    if (open >= 0) calls.push({ index, open, close: matching(tokens, open, "(", ")") });
  }

  const binaryOperators = new Set(["+", "-", "*", "/", "%", "<<", ">>", "&", "|", "^", "==", "!=", "<", ">", "<=", ">=", "&&", "||"]);
  const prefixOperators = new Set(["!", "+", "-", "~", "*"]);
  const keywordNames = new Set([
    "use", "as", "fn", "struct", "enum", "trait", "impl", "module", "return", "if", "else", "loop", "in", "break", "next",
    "true", "false", "null", "unit", "uninit", "after", "defer", "extern", "region", "layout", "live", "unsafe", "type",
    "pub", "const", "let", "mut", "distinct", "derive", "where", "self", "super", "crate", "and", "or", "not",
  ]);
  const importedAliases = new Set(imports.keys());
  const declarationWords = new Set(["fn", "struct", "enum", "trait", "impl", "module", "use", "as", "distinct", "extern"]);

  // Literal and primitive expression shapes.
  for (const token of tokens) {
    if (token.kind === "number") {
      if (/[.eE]/u.test(token.value)) add("FloatLit", token);
      else add("IntLit", token);
    } else if (token.kind === "string") add("StrLit", token);
    else if (token.kind === "char") add("CharLit", token);
    else if (token.value === "true" || token.value === "false") add("BoolLit", token);
  }
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const prev = tokens[index - 1];
    const next = tokens[index + 1];
    if (token.value === "(" && next?.value === ")" && prev?.kind !== "ident" && prev?.value !== "." && tokens[index + 2]?.value !== "->") add("Unit", token);
    if (token.value === "{" && ["->", "=>"].includes(prev?.value)) add("InlineBlock", token);
    if (token.value === "uninit") add("Uninit", token);
    if (token.value === "@" && ["number", "string", "char"].includes(next?.kind) || token.value === "@" && ["true", "false"].includes(next?.value)) add("CtLit", token);
    if (token.value === "default") add("DefaultLit", token);
    if (token.value === "null" || (token.value === "None" && prev?.value !== ".")) add("Absent", token);
    if (token.value === "??") add("OrFallback", token);
    if (token.value === "?.") add("OptField", token);
    if (token.value === "~") add("ExplicitCopy", token);
    if (token.value === "^") {
      if (!prev || ["(", "[", "{", ",", "=", "::", ":="].includes(prev.value)) add("Borrow", token);
    }
    if (binaryOperators.has(token.value)) add("Binary", token);
    if (prefixOperators.has(token.value) && (!prev || binaryOperators.has(prev.value) || ["(", "[", "{", ",", "=", "::", ":="].includes(prev.value))) add("Unary", token);
    if (token.value === "++" || token.value === "--") add("IncDec", token);
    if (token.value === "." && next?.value === "*") add("Deref", token);
    if (token.value === "*" && (!prev || ["(", "[", "{", ",", "=", "::", ":="].includes(prev.value))) add("RawOf", token);
    if (token.value === "..." || token.value === "..spread") add("ListSpread", token);
    if (token.value === "?" && prev && ["ident", "number", "string", "char"].includes(prev.kind)) add("Try", token);
    if (token.value === "#" && next?.value === "Todo") add("Todo", token);
    if (token.value === "#" && next?.value === "Unreachable") add("Unreachable", token);
    if (token.value === "#" && next?.value === "Pre") {
      add("Contract", token);
      add("ContractScope", token);
    }
    if (token.value === "#" && next?.value === "Post") {
      add("Contract", token);
      add("ContractScope", token);
    }
    if (token.value === "#" && next?.value === "DebugOnly") add("DebugOnly", token);
    if (token.value === "#" && next?.value === "Unsafe") add("Unsafe", token);
    if (token.value === "#" && next?.value === "Sentry") add("SentryPolicy", token);
    if (token.value === "#" && next?.value === "Impure") add("Impure", token);
    if (token.value === "#" && next?.value === "Reactive") add("Reactive", token);
    if (token.value === "#" && next?.value === "Context") add("ContextBlock", token);
    if (token.value === "#" && next?.value === "Shield") add("Shield", token);
    if (token.value === "#" && next?.value === "Transact") add("Transact", token);
    if (token.value === "region") add("Region", token);
    if (token.value === "layout" && next?.kind === "ident") add("Layout", token);
    if (token.value === "live") add("Live", token);
    if (token.value === "." && ["setup", "expect_fail", "timeout", "skip"].includes(next?.value)) add("ScopeMember", token);
    if (token.value === "@" && next?.kind === "ident") add("Inline", token);
    if (token.value === "extern") add("ExternCall", token);
    if (token.value === "null" || (token.value === "None" && prev?.value !== ".")) add("Absent", token);
    if (token.value === "value" || token.value === "Val") {
      const open = callOpen(tokens, index);
      if (open >= 0) add("Present", token);
    }
  }
  const relationalOperators = new Set(["<", ">", "<=", ">="]);
  for (let index = 0; index + 2 < tokens.length; index += 1) {
    if (!relationalOperators.has(tokens[index].value)) continue;
    const middle = tokens[index + 1];
    const nextOperator = tokens[index + 2];
    if (middle.line === tokens[index].line && nextOperator.line === tokens[index].line && relationalOperators.has(nextOperator.value)) {
      add("CompareChain", tokens[index]);
    }
  }

  // Bindings and locals are structural, not substring matches.
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const next = tokens[index + 1];
    if (token.kind !== "ident") continue;
    if (["::", ":="].includes(next?.value)) add("Let", token);
    if (next?.value === "=" && tokens[index - 1]?.value !== "=" && tokens[index - 1]?.value !== "!") add("Assign", next);
    if (["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="].includes(next?.value)) add("Assign", next);
    if (token.value === "return") add("Return", token);
    if (token.value === "break") {
      add("Break", token);
      const after = tokens[index + 1];
      if (after && after.value !== "}" && after.value !== "\n") {
        if (after.value !== "(" || tokens[index + 2]?.value === "-" || tokens[index + 2]?.kind === "number" || tokens[index + 3]?.value === ",") add("BreakValue", token);
      }
    }
    if (token.value === "next") add("Continue", token);
    if (token.value === "defer" && next?.value === "close") add("DeferClose", token);
  }
  for (const token of tokens) {
    if (token.kind !== "ident" || keywordNames.has(token.value) || importedAliases.has(token.value)) continue;
    if (context.anyTypeNames.has(token.value)) continue;
    if (context.declarationIndices.has(tokens.indexOf(token))) continue;
    if (tokens[tokens.indexOf(token) - 1]?.value === ".") continue;
    if (tokens[tokens.indexOf(token) + 1]?.value === ":") continue;
    if (context.bindingNames.has(token.value)) add("Local", token);
  }

  // Collection/struct/index/tuple shapes.
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token.value === "{") {
      const owner = tokens[index - 1];
      if (owner?.kind === "ident" && context.structNames.has(owner.value)) add("StructLit", owner);
    }
    if (token.value === "[") {
      const close = matching(tokens, index, "[", "]");
      const inside = tokens.slice(index + 1, close);
      const colon = inside.findIndex((item) => item.value === ":");
      const previous = tokens[index - 1];
      const typedLiteral = tokens[close + 1]?.value === "{";
      if (colon >= 0 && (typedLiteral || !previous || previous.value === "=" || previous.value === "::" || previous.value === ":=")) add("MapLit", token);
      else if (typedLiteral || !previous || ["=", "::", ":=", "(", ",", "return"].includes(previous.value)) add("ListLit", token);
      if (previous?.kind === "ident" || [")", "]", "}"].includes(previous?.value)) {
        add("Index", token);
        if (tokens[close + 1]?.value === ".") add("ColumnarGather", token);
      }
      if (inside.some((item) => ["..", "..<", "..="].includes(item.value))) add("Slice", token);
      const afterIndex = tokens[close + 1]?.value;
      if (afterIndex === "=" || ["=", "+=", "-=", "*=", "/="].includes(afterIndex)) {
        add("IndexAssign", token);
      }
      if (afterIndex === "." && ["=", "+=", "-=", "*=", "/="].includes(tokens[close + 3]?.value)) {
        add("IndexFieldAssign", token);
      }
    }
    if (token.value === "(") {
      const close = matching(tokens, index, "(", ")");
      const inside = tokens.slice(index + 1, close);
      const previous = tokens[index - 1];
      if (inside.some((item) => item.value === ":") && previous?.kind !== "ident") add("TupleLit", token);
    }
  }
  if (tokens.some((token) => token.value === "columnar")) {
    for (const token of tokens) {
      if (token.value === "[") add("ColumnarListLit", token);
    }
  }

  // Call forms: local, Core, receiver, module, builtin and handle methods.
  const coreRowsByKind = new Map(coreRows.map((row) => [row.key, row]));
  const receiverRows = coreRows.filter((row) => row.kind === "receiver");
  for (const call of calls) {
    const callee = tokens[call.index];
    const previous = tokens[call.index - 1];
    const isMethod = previous?.value === ".";
    let receiver = null;
    if (isMethod) receiver = receiverTypeFor(tokens, call.index, context);

    if (!isMethod && context.fnNames.has(callee.value) && !context.declarationIndices.has(call.index)) {
      add("Call", callee);
    }
    if (!isMethod && callee.value === "print") add("Print", callee);
    if (!isMethod && callee.value === "drop") add("Drop", callee);
    if (!isMethod && callee.value === "close") add("Close", callee);
    if (!isMethod && callee.value === "input") add("AmbientInput", callee);
    if (!isMethod && ["assert", "assert_eq", "panic"].includes(callee.value)) add("RequireStop", callee);
    if (!isMethod && ["wrapping", "saturating", "checked", "rotate_left", "rotate_right"].includes(callee.value)) add("OverflowOpt", callee);
    if (!isMethod && callee.value === "unreachable") add("Unreachable", callee);
    if (!isMethod && ["Option", "JSON", "DBValue"].includes(tokens[call.index - 2]?.value) && previous?.value === ".") {
      // handled by qualified forms below
    }

    // A Core plain call is a rooted alias/member pair. Generic receiver spelling
    // (`mem.Ptr<T>.from_addr`) is accepted only when the alias is rooted.
    const alias = isMethod ? rootedCoreAlias(tokens, call.index, imports) : null;
    if (alias) {
      const module = imports.get(alias);
      const row = coreRowsByKind.get(`plain:${module}.${callee.value}`);
      if (row) {
        addCore(row, callee);
        add("CoreCall", callee);
      }
    }
    if (!isMethod && !context.fnNames.has(callee.value)) {
      const row = coreRowsByKind.get(`plain:core.prelude.${callee.value}`);
      if (row) addCore(row, callee);
    }

    if (isMethod && receiver) {
      for (const row of receiverRows) {
        if (row.member === callee.value && row.receiver_types.includes(receiver)) addCore(row, callee);
      }
    }
    if (isMethod && context.methodNames.has(callee.value) && !receiver) add("MethodCall", callee);
    if (isMethod && context.fieldFnNames.has(callee.value)) add("FnFieldCall", callee);
    if (isMethod && context.anyTypeNames.has(tokens[call.index - 2]?.value) && context.methodNames.has(callee.value)) add("StaticCall", callee);

    const owner = tokens[call.index - 2]?.value;
    if (!isMethod && imports.has(owner)) {
      const module = imports.get(owner);
      if (!module.startsWith("core.")) add("ModuleCall", callee);
    }
    if (isMethod && context.moduleNames.has(tokens[call.index - 2]?.value)) add("ModuleCall", callee);

    if (callee.value === "Option" && tokens[call.index + 1]?.value === "." && tokens[call.index + 2]?.value === "lift2") add("OptionLift2", callee);
    if (callee.value === "Decimal" || callee.value === "Fraction") add("PreciseBuiltin", callee);
    if (context.anyTypeNames.has(callee.value) && (/^(?:Vec|Mat|F|I|U)\d/u.test(callee.value) || /x\d/u.test(callee.value))) add("MathBuiltin", callee);
    if (context.anyTypeNames.has(callee.value) && context.distinctNames.has(callee.value)) add("DistinctCtor", callee);
    if (callee.value === "extern") add("ExternCall", callee);

    if (isMethod) {
      if (["map", "filter", "filter_map", "each", "find", "any", "all", "sort_by", "reduce", "flat_map", "try_collect"].includes(callee.value)) {
        const body = tokens.slice(call.open + 1, call.close);
        if (body.some((token) => token.value === "->")) add("ClosureMethod", callee);
      }
      if (["clone"].includes(callee.value)) add("Clone", callee);
      if (callee.value === "raw") add("DistinctRaw", callee);
      if (["wrapping_add", "wrapping_sub", "wrapping_mul", "wrapping_div", "checked_add", "checked_sub", "checked_mul", "checked_div", "saturating_add", "saturating_sub"].includes(callee.value)) add("NumericBinaryMethod", callee);
      if (["is_nan", "is_finite", "is_infinite", "count_ones", "count_zeros", "leading_zeros", "trailing_zeros", "abs", "signum"].includes(callee.value)) add("NumericMethod", callee);
      if (callee.value === "decode") add("DecodeUnder", callee);
      if (["entries", "to_map"].includes(callee.value)) add("DataEntriesToMap", callee);
      if (["add", "sub", "mul", "div", "sqrt"].includes(callee.value) && receiver && ["Fraction", "Decimal", "Measurement"].includes(receiver)) add("NumericBinaryMethod", callee);
      if (receiver && /(?:Reader|Writer|Handle|Stream|Listener|Request|Response|Arena|Allocator|Process|File)/u.test(receiver)) add("HandleMethod", callee);
      if (/^[xyzw]{2,4}$/u.test(callee.value) && receiver && /(?:Vec|F\d+x\d+|I\d+x\d+)/u.test(receiver)) add("MathSwizzleRead", callee);
    }
  }
  // Closure-taking task forms use a block argument rather than a parenthesized
  // call, so they are intentionally handled outside the call list.
  for (let index = 0; index + 2 < tokens.length; index += 1) {
    if (tokens[index].value !== "task") continue;
    const next = tokens[index + 1];
    if (next?.value === ".") {
      const member = tokens[index + 2];
      if (member?.kind !== "ident") continue;
      if (member.value === "group") add("TaskGroup", tokens[index]);
      if (tokens[index + 3]?.value !== "{") continue;
      if (member.value === "all") add("TaskGroupAll", member);
      if (member.value === "race") add("TaskGroupRace", member);
      if (member.value === "any") add("TaskGroupAny", member);
    } else {
      let body = index + 1;
      if (tokens[body]?.value === "^") body += 2;
      if (tokens[body]?.value === "{") add("CoreClosureCall", tokens[index]);
    }
  }

  // Qualified enum/foreign literals and pattern heads.
  for (let index = 0; index + 2 < tokens.length; index += 1) {
    if (tokens[index].kind !== "ident" || tokens[index + 1]?.value !== ".") continue;
    const owner = tokens[index].value;
    const member = tokens[index + 2];
    if (context.enumNames.has(owner)) add("EnumLit", tokens[index]);
    if (owner === "JSON") add("JSONLit", tokens[index]);
    if (owner === "DBValue") add("DBValueLit", tokens[index]);
    if (member?.kind === "ident" && ["Ok", "Err"].includes(owner)) add(owner, tokens[index]);
  }
  // A field read is a member token that is not a call, enum/foreign literal, or
  // rooted Core alias. This remains token-structural and ignores comments/text.
  for (let index = 1; index + 2 < tokens.length; index += 1) {
    if (tokens[index].value !== "." || tokens[index + 1]?.kind !== "ident") continue;
    if (tokens[index + 2]?.value === "(") continue;
    const owner = tokens[index - 1];
    if (owner?.kind !== "ident") continue;
    if (context.anyTypeNames.has(owner.value) || context.moduleNames.has(owner.value) || importedAliases.has(owner.value)) continue;
    add("Field", tokens[index + 1]);
  }
  for (let index = 0; index + 1 < tokens.length; index += 1) {
    if (tokens[index].value !== "." || tokens[index + 1].kind !== "ident") continue;
    if (/^[A-Z]/u.test(tokens[index + 1].value) && ["==", "{", "|"].includes(tokens[index - 1]?.value)) add("PatternMatches", tokens[index]);
  }
  for (const token of tokens) {
    if (token.value === "Ok") add("Ok", token);
    if (token.value === "Err") add("Err", token);
  }

  // If tables and closures.
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "if") continue;
    add("If", tokens[index]);
    const table = inspectTable(tokens, index);
    if (table) {
      if (table.hasVariant) add("EnumMatch", tokens[index]);
      else if (table.hasRange) add("RangeSwitch", tokens[index]);
      else if (table.hasReceiverArm || table.hasAfter) {
        add("SelectStart", tokens[index]);
        add("SelectWait", tokens[index]);
        if (table.hasReceiverArm) add("SelectRecv", tokens[index]);
        if (table.hasAfter) add("SelectAfter", tokens[index]);
      } else if (table.hasArrow) add("MixedSwitch", tokens[index]);
    }
    const lineBefore = tokens[index - 1]?.value;
    let hasArrow = false;
    for (let cursor = index + 1; cursor < tokens.length && tokens[cursor].line <= tokens[index].line + 1; cursor += 1) {
      if (tokens[cursor].value === "->") {
        hasArrow = true;
        break;
      }
      if (tokens[cursor].value === "}") break;
    }
    if (hasArrow && ["::", ":=", "return"].includes(lineBefore)) add("IfExpr", tokens[index]);
  }
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "->") continue;
    const previous = tokens[index - 1];
    if (previous?.value === ")") {
      const open = findMatchingBackward(tokens, index - 1, "(", ")");
      if (open >= 0 && tokens[open - 1]?.value !== "fn") add("Lambda", tokens[index]);
    } else if (previous?.kind === "ident" && !["if", "else", "loop", "return"].includes(previous.value)) {
      const before = tokens[index - 2]?.value;
      if (!["fn", ")"].includes(before)) add("Lambda", tokens[index]);
    }
  }

  // Loop and control-flow statements.
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "loop" && tokens[index].value !== "while") continue;
    const token = tokens[index];
    if (tokens[index].value === "while") {
      add("While", token);
      continue;
    }
    const next = tokens[index + 1];
    if (next?.value === "{") add("Loop", token);
    else {
      let cursor = index + 1;
      let hasIn = false;
      let hasRange = false;
      let hasSemicolon = false;
      let hasArrow = false;
      for (; cursor < tokens.length && tokens[cursor].line <= token.line + 1; cursor += 1) {
        if (tokens[cursor].value === "in") hasIn = true;
        if (["..", "..<", "..="].includes(tokens[cursor].value)) hasRange = true;
        if (tokens[cursor].value === ";") hasSemicolon = true;
        if (tokens[cursor].value === "->") {
          hasArrow = true;
          break;
        }
        if (tokens[cursor].value === "{") break;
      }
      if (hasSemicolon) add("CountedLoop", token);
      else if (hasIn && hasRange) add("Range", token);
      else if (hasIn) add("ForIn", token);
      else if (!hasArrow || next?.value !== "->") add("While", token);
    }
  }

  // Operator/statement shapes that need the surrounding tokens.
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token.value === "=") {
      const previous = tokens[index - 1];
      if (previous?.value === "]") {
        add("IndexAssign", token);
        if (tokens[index - 2]?.value === ".") add("IndexFieldAssign", token);
      }
      if (previous?.kind === "ident" && /^[xyzw]{2,4}$/u.test(previous.value)) add("MathSwizzleAssign", token);
    }
    if (token.value === "impl" && tokens[index + 1]?.kind === "ident" && tokens[index + 2]?.value === "." && tokens[index + 3]?.value === "Index") add("IndexHookAssign", token);
  }
  if (tokens.some((token) => token.value === "Index")) {
    for (const token of tokens) if (token.value === "=") add("IndexHookAssign", token);
  }

  // A standalone call is an expression statement unless it is visibly bound.
  for (const call of calls) {
    const lineStart = tokens.findIndex((token) => token.line === tokens[call.index].line);
    const before = lineStart >= 0 ? tokens.slice(lineStart, call.index).map((token) => token.value) : [];
    if (!before.includes("::") && !before.includes(":=") && !before.includes("=")) add("ExprStmt", tokens[call.index]);
  }

  // Source-level function values and module calls.
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    if (token.kind !== "ident" || !context.fnNames.has(token.value)) continue;
    if (context.declarationIndices.has(index)) continue;
    if (callOpen(tokens, index) >= 0) continue;
    add("FnValue", token);
  }

  // Empty branch/no-source constructs deliberately remain uncovered. This is
  // important: a private lowering variant must not become covered by a word in
  // a comment or a similarly named API.
  return { evidence, imports };
}

function renderHuman(report) {
  const { summary, records } = report;
  const lines = [
    "Executable example / TIR / Core census",
    `examples: ${summary.examples.files} executable files (${summary.examples.source_files} .jet files scanned)`,
    `execution: ${report.execution.status}; witness=${report.execution.example?.file ?? "none"}`,
    `TIR expressions: ${summary.tir.expressions.covered}/${summary.tir.expressions.total} covered`,
    `TIR statements: ${summary.tir.statements.covered}/${summary.tir.statements.total} covered`,
    `Core plain rows: ${summary.core.plain.covered}/${summary.core.plain.total} covered`,
    `Core receiver rows: ${summary.core.receiver.covered}/${summary.core.receiver.total} covered`,
    `uncovered: ${summary.uncovered}`,
  ];
  const uncovered = records.filter((record) => !record.covered);
  if (uncovered.length > 0) {
    lines.push("first uncovered rows:");
    for (const record of uncovered.slice(0, 20)) {
      const name = record.family === "core" ? record.key : `${record.family}:${record.construct}`;
      lines.push(`  ${name} (${record.source.file}:${record.source.line})`);
    }
    if (uncovered.length > 20) lines.push(`  ... ${uncovered.length - 20} more`);
  }
  return `${lines.join("\n")}\n`;
}

function tsvCell(value) {
  return String(value ?? "").replace(/\t/gu, " ").replace(/\r?\n/gu, " ");
}

function recordIdentity(record) {
  if (record.identity) return record.identity;
  return record.family === "core" ? `core:${record.kind}:${record.key}` : `${record.family}:${record.construct}`;
}

function recordExamples(record) {
  return record.examples.map((location) => `${location.file}:${location.line}:${location.column}`).join("|");
}

function recordGoldens(record) {
  return [...new Set(record.examples.flatMap((location) => location.golden_paths))].sort().join("|");
}

function renderTsv(report) {
  const lines = ["family\tidentity\tcovered\tcoverage_status\tuncovered_reason\tinternal_only\texamples\tgoldens\truntime_status\truntime_reason"];
  for (const record of report.records) {
    lines.push([
      record.family,
      recordIdentity(record),
      record.covered,
      record.coverage_status,
      record.uncovered_reason,
      record.internal_only,
      recordExamples(record),
      recordGoldens(record),
      record.runtime.status,
      record.runtime.reason,
    ].map(tsvCell).join("\t"));
  }
  return `${lines.join("\n")}\n`;
}

function markdownCell(value) {
  return String(value ?? "").replace(/\\/gu, "\\\\").replace(/\|/gu, "\\|").replace(/\r?\n/gu, " ");
}

function renderMarkdown(report) {
  const lines = [
    "# Executable example, TIR, and Core coverage census",
    "",
    "Coverage is classified from executable source evidence and discovered `.out` golden files.",
    "Rows are keyed by construct/Core identity and stable source-span hashes; rendered line numbers are evidence only.",
    `Executable probe: ${report.execution.status}; ${report.execution.reason ?? "the representative golden matched on AOT, jet run, and --interpret"}.`,
    "",
    "## Summary",
    "",
    `- Executable examples: ${report.summary.examples.files}/${report.summary.examples.source_files}; golden-backed: ${report.summary.examples.golden_examples}.`,
    `- Rows: ${report.summary.total}; covered: ${report.summary.covered}; uncovered: ${report.summary.uncovered}.`,
    `- Runtime witness: ${report.execution.example?.file ?? "none"}; status: ${report.execution.status}.`,
    `- Gate: \`${report.ci_gate.command}\` fails when \`${report.ci_gate.fails_when}\`.`,
    "",
    "## Coverage table",
    "",
    "| family | identity | covered | status | uncovered reason | internal-only | example locations | golden files | runtime |",
    "| --- | --- | ---: | --- | --- | --- | --- | --- | --- |",
  ];
  for (const record of report.records) {
    lines.push([
      record.family,
      recordIdentity(record),
      record.covered,
      record.coverage_status,
      record.uncovered_reason,
      record.internal_only,
      recordExamples(record),
      recordGoldens(record),
      `${record.runtime.status}: ${record.runtime.reason}`,
    ].map(markdownCell).join(" | ").replace(/^/u, "| ").concat(" |"));
  }
  return `${lines.join("\n")}\n`;
}

function makeSummary(records, sourceFiles, executableFiles, goldenExamples) {
  const count = (family, kind = null) => records.filter((record) => record.family === family && (!kind || record.kind === kind));
  const stat = (rows) => ({ total: rows.length, covered: rows.filter((row) => row.covered).length, uncovered: rows.filter((row) => !row.covered).length });
  const expressions = count("tir-expr");
  const statements = count("tir-stmt");
  const plain = count("core", "plain");
  const receiver = count("core", "receiver");
  return {
    examples: {
      source_files: sourceFiles,
      files: executableFiles,
      golden_examples: goldenExamples,
      without_golden: executableFiles - goldenExamples,
    },
    tir: { expressions: stat(expressions), statements: stat(statements) },
    core: { plain: stat(plain), receiver: stat(receiver) },
    total: records.length,
    covered: records.filter((record) => record.covered).length,
    uncovered: records.filter((record) => !record.covered).length,
  };
}

const RUNTIME_TIERS = ["aot", "jet run", "--interpret"];
const RUNTIME_UNAVAILABLE_REASON = "executable tier probing was not requested; pass --execute to run a golden-backed witness";
const RUNTIME_NOT_PROBED_REASON = "this row was not included in the representative tier probe";

function classifyLocations(locations) {
  const golden = locations.filter((location) => location.golden);
  if (golden.length > 0) {
    return {
      covered: true,
      coverage_status: "golden-backed-static",
      uncovered_reason: null,
    };
  }
  if (locations.length > 0) {
    return {
      covered: false,
      coverage_status: "uncovered",
      uncovered_reason: "example without a golden",
    };
  }
  return {
    covered: false,
    coverage_status: "uncovered",
    uncovered_reason: "no executable example",
  };
}

function coverageFields(locations) {
  return {
    ...classifyLocations(locations),
    runtime: {
      status: EXECUTE ? "not-probed" : "not-requested",
      tiers: RUNTIME_TIERS,
      reason: EXECUTE ? RUNTIME_NOT_PROBED_REASON : RUNTIME_UNAVAILABLE_REASON,
      pending_reason: "golden does not exercise row",
    },
  };
}

function validateCoverage(records, examples) {
  const byFile = new Map(examples.map((example) => [example.file, example]));
  const identities = new Set();
  const allowedReasons = new Set(["no executable example", "example without a golden"]);
  for (const record of records) {
    const identity = record.family === "core" ? `core:${record.kind}:${record.key}` : `${record.family}:${record.construct}`;
    if (record.identity !== identity) fail("unresolved row", `${identity} has inconsistent identity`);
    if (identities.has(identity)) fail("unresolved row", `duplicate coverage identity ${identity}`);
    identities.add(identity);
    if (typeof record.covered !== "boolean" || !["golden-backed-static", "uncovered"].includes(record.coverage_status)) {
      fail("unresolved row", `${identity} has no coverage classification`);
    }
    if (record.covered && record.uncovered_reason !== null) fail("unresolved row", `${identity} is covered but has an uncovered reason`);
    if (!record.covered && !allowedReasons.has(record.uncovered_reason)) {
      fail("unresolved row", `${identity} has unknown uncovered reason ${record.uncovered_reason ?? "<missing>"}`);
    }
    if (record.internal_only !== null && (!record.internal_only.startsWith("internal-only: ") || record.internal_only.length <= "internal-only: ".length)) {
      fail("unresolved row", `${identity} has malformed internal-only classification`);
    }
    if (record.source_span_hash !== record.source?.span_hash || !validSpanHash(record.source_span_hash)) {
      fail("unresolved row", `${identity} has no stable source span hash`);
    }
    for (const location of record.examples) {
      if (!validSpanHash(location.span_hash)) {
        fail("unresolved row", `${identity} has no stable example span hash`);
      }
      const example = byFile.get(location.file);
      if (!example) fail("unresolved row", `${identity} cites non-executable example ${location.file}`);
      if (location.golden !== example.golden || JSON.stringify(location.golden_paths) !== JSON.stringify(example.golden_paths)) {
        fail("unresolved row", `${identity} has inconsistent golden evidence for ${location.file}`);
      }
    }
  }
}

function cleanRuntimeOutput(value) {
  return String(value ?? "")
    .replace(/\u001b\[[0-?]*[ -/]*[@-~]/gu, "")
    .replace(/\r/gu, "")
    .trim();
}

function runProcess(executable, args, cwd, label) {
  const started = performance.now();
  let result;
  try {
    result = spawnSync(executable, args, {
      cwd,
      encoding: "utf8",
      env: { ...process.env, NO_COLOR: "1", CLICOLOR: "0" },
      timeout: EXECUTION_TIMEOUT_MS,
      maxBuffer: 8 * 1024 * 1024,
    });
  } catch (error) {
    return {
      status: "unavailable",
      label,
      wall_time_ms: Math.round((performance.now() - started) * 1000) / 1000,
      exit_code: null,
      stdout: "",
      stderr: String(error?.message ?? error),
      reason: "tier command could not start",
    };
  }
  const wallTimeMs = Math.round((performance.now() - started) * 1000) / 1000;
  if (result.error) {
    return {
      status: result.error.code === "ETIMEDOUT" ? "timeout" : "unavailable",
      label,
      wall_time_ms: wallTimeMs,
      exit_code: null,
      stdout: "",
      stderr: String(result.error.message ?? result.error),
      reason: result.error.code === "ETIMEDOUT" ? "tier command timed out" : `tier command could not start (${result.error.code ?? "unknown"})`,
    };
  }
  const stdout = cleanRuntimeOutput(result.stdout);
  const stderr = cleanRuntimeOutput(result.stderr);
  return {
    status: result.signal ? "failed" : result.status === 0 ? "exited" : "failed",
    label,
    wall_time_ms: wallTimeMs,
    exit_code: result.signal ? null : result.status,
    stdout,
    stderr,
    reason: result.signal
      ? `tier command terminated by ${result.signal}`
      : result.status === 0
        ? null
        : `tier command exited ${result.status}`,
  };
}

function runtimeResult(processResult, expectedOutput, extra = {}) {
  const outputMatches = processResult.stdout === expectedOutput;
  const passed = processResult.status === "exited"
    && processResult.exit_code === 0
    && outputMatches;
  return {
    status: passed
      ? "measured"
      : processResult.status === "unavailable" || processResult.status === "timeout"
        ? processResult.status
        : "failed",
    wall_time_ms: processResult.wall_time_ms,
    exit_code: processResult.exit_code,
    stdout_lines: processResult.stdout === "" ? 0 : processResult.stdout.split("\n").length,
    output_matches: outputMatches,
    stderr: processResult.stderr === "" ? null : processResult.stderr.slice(0, 600),
    reason: passed
      ? null
      : processResult.reason ?? (outputMatches ? "non-zero tier exit" : "tier stdout differed from golden"),
    ...extra,
  };
}

function runRepresentativeTier(tier, source, expectedOutput, runRoot) {
  if (!existsSync(JET_BINARY)) {
    return {
      status: "unavailable",
      wall_time_ms: null,
      exit_code: null,
      stdout_lines: 0,
      output_matches: false,
      stderr: null,
      reason: `local compiler ${normalizePath(relative(ROOT, JET_BINARY))} is unavailable`,
    };
  }
  if (tier === "aot") {
    const build = runProcess(JET_BINARY, ["build", "--profile=debug", "main.jet"], runRoot, "representative:aot:build");
    if (build.status !== "exited" || build.exit_code !== 0) {
      return runtimeResult(build, expectedOutput, { build_wall_time_ms: build.wall_time_ms });
    }
    const artifact = ["build/run", "build/main"]
      .map((path) => resolve(runRoot, path))
      .find((path) => existsSync(path));
    if (!artifact) {
      return {
        status: "failed",
        wall_time_ms: build.wall_time_ms,
        exit_code: build.exit_code,
        stdout_lines: 0,
        output_matches: false,
        stderr: build.stderr === "" ? null : build.stderr,
        reason: "AOT build exited successfully without build/run or build/main",
        build_wall_time_ms: build.wall_time_ms,
      };
    }
    const execution = runProcess(artifact, [], runRoot, "representative:aot:execute");
    return runtimeResult(execution, expectedOutput, {
      build_wall_time_ms: build.wall_time_ms,
      artifact: normalizePath(relative(runRoot, artifact)),
    });
  }
  const args = tier === "--interpret" ? ["run", "--interpret", "main.jet"] : ["run", "main.jet"];
  return runtimeResult(runProcess(JET_BINARY, args, runRoot, `representative:${tier}`), expectedOutput);
}

function executeRepresentative(root, examples) {
  const preferred = examples.find((example) =>
    example.golden
    && example.file.endsWith("examples/features/basics/hello.jet")
    && example.golden_paths.length > 0);
  const example = preferred ?? examples.find((candidate) => candidate.golden && candidate.golden_paths.length > 0);
  const base = {
    tiers: ["aot", "jet run", "--interpret"],
    source: "one representative golden-backed executable example",
  };
  if (!example) {
    return {
      ...base,
      status: "unavailable",
      example: null,
      reason: "no golden-backed executable example is available",
    };
  }
  const golden = resolve(root, example.golden_paths[0]);
  if (!existsSync(golden)) {
    return {
      ...base,
      status: "unavailable",
      example: { file: example.file, golden_paths: example.golden_paths },
      reason: `golden file ${example.golden_paths[0]} is missing`,
    };
  }
  const expectedOutput = cleanRuntimeOutput(readFileSync(golden, "utf8"));
  const executionRoot = resolve(
    process.env.JET_TEST_SCRATCH_DIR ?? resolve(root, ".agent-scratch-example-core-census"),
    "example-core-runtime",
  );
  mkdirSync(executionRoot, { recursive: true });
  const runRoot = mkdtempSync(resolve(executionRoot, "representative-"));
  writeFileSync(resolve(runRoot, "main.jet"), example.source);
  const packagePath = resolve(dirname(example.absolute), "package.jet");
  if (existsSync(packagePath)) writeFileSync(resolve(runRoot, "package.jet"), readFileSync(packagePath));
  try {
    const results = {
      aot: runRepresentativeTier("aot", example.source, expectedOutput, runRoot),
      "jet run": runRepresentativeTier("jet run", example.source, expectedOutput, runRoot),
      "--interpret": runRepresentativeTier("--interpret", example.source, expectedOutput, runRoot),
    };
    const measured = Object.values(results).every((result) => result.status === "measured");
    return {
      ...base,
      status: measured ? "measured" : Object.values(results).some((result) => result.status === "unavailable") ? "unavailable" : "failed",
      example: { file: example.file, golden_paths: example.golden_paths },
      expected_output_lines: expectedOutput === "" ? 0 : expectedOutput.split("\n").length,
      results,
      reason: measured
        ? null
        : Object.entries(results)
          .filter(([, result]) => result.reason)
          .map(([tier, result]) => `${tier}: ${result.reason}`)
          .join("; "),
    };
  } finally {
    rmSync(runRoot, { recursive: true, force: true });
  }
}

function runCensus(options) {
  const root = options.root;
  const tirAbsolute = join(root, TIR_SOURCE);
  const coreAbsolute = join(root, CORE_SOURCE);
  const examplesAbsolute = resolve(root, options.examples);
  let tirSource;
  let coreSource;
  try {
    tirSource = readFileSync(tirAbsolute, "utf8");
    coreSource = readFileSync(coreAbsolute, "utf8");
    if (!statSync(examplesAbsolute).isDirectory()) fail("source error", `${examplesAbsolute} is not a directory`);
  } catch (error) {
    if (error instanceof CensusError) throw error;
    fail("source error", `cannot read canonical source: ${error.message}`);
  }
  const tirFile = normalizePath(TIR_SOURCE);
  const coreFile = normalizePath(CORE_SOURCE);
  const exprRows = parseRustEnum(tirSource, tirFile, "TExprKind");
  const stmtRows = parseRustEnum(tirSource, tirFile, "TStmt");
  const coreRows = parseCoreRegistry(coreSource, coreFile, root).map((row) => ({ ...row, examples: [], _seen: new Set() }));
  const exprNames = new Set(exprRows.map((row) => row.name));
  const stmtNames = new Set(stmtRows.map((row) => row.name));
  const sourceEntries = walkJetFiles(examplesAbsolute, root);
  const goldenFiles = walkGoldenFiles(examplesAbsolute, root);
  const examples = [];
  for (const entry of sourceEntries) {
    const source = readFileSync(entry.absolute, "utf8");
    const { tokens } = jetTokens(source, entry.file);
    if (isExecutableExample(tokens)) {
      const golden_paths = goldenPathsForExample(entry, examplesAbsolute, goldenFiles);
      examples.push({ ...entry, source, tokens, golden_paths, golden: golden_paths.length > 0 });
    }
  }
  const receiverRows = coreRows.filter((row) => row.kind === "receiver");
  const receiverByTypeMember = new Map();
  for (const row of receiverRows) {
    for (const type of row.receiver_types) receiverByTypeMember.set(`${type}.${row.member}`, row);
  }
  const coreByModuleMember = new Map(coreRows.filter((row) => row.kind === "plain").map((row) => [`${row.module}.${row.member}`, row]));
  const exprEvidence = new Map(exprRows.map((row) => [row.name, []]));
  const stmtEvidence = new Map(stmtRows.map((row) => [row.name, []]));
  for (const example of examples) {
    const result = analyzeExample(example, coreRows, exprNames, stmtNames, receiverByTypeMember, coreByModuleMember);
    for (const [name, locations] of result.evidence) {
      const target = exprEvidence.has(name) ? exprEvidence : stmtEvidence;
      target.get(name).push(...locations);
    }
  }
  const records = [];
  for (const row of exprRows) {
    const locations = exprEvidence.get(row.name).sort(compareLocations);
    records.push({
      family: "tir-expr",
      identity: `tir-expr:${row.name}`,
      construct: row.name,
      source: row.source,
      source_span_hash: row.source.span_hash,
      internal_only: locations.length === 0 ? "internal-only: no executable example in corpus" : null,
      ...coverageFields(locations),
      examples: locations,
    });
  }
  for (const row of stmtRows) {
    const locations = stmtEvidence.get(row.name).sort(compareLocations);
    records.push({
      family: "tir-stmt",
      identity: `tir-stmt:${row.name}`,
      construct: row.name,
      source: row.source,
      source_span_hash: row.source.span_hash,
      internal_only: locations.length === 0 ? "internal-only: no executable example in corpus" : null,
      ...coverageFields(locations),
      examples: locations,
    });
  }
  for (const row of coreRows) {
    const examplesForRow = row.examples.sort(compareLocations);
    const output = {
      ...row,
      identity: `core:${row.kind}:${row.key}`,
      source_span_hash: row.source.span_hash,
      internal_only: examplesForRow.length === 0 ? "internal-only: no executable example in corpus" : null,
      ...coverageFields(examplesForRow),
      examples: examplesForRow,
    };
    delete output._seen;
    records.push(output);
  }
  records.sort(compareRecords);
  validateCoverage(records, examples);
  const summary = makeSummary(
    records,
    sourceEntries.length,
    examples.length,
    examples.filter((example) => example.golden).length,
  );
  const execution = EXECUTE
    ? executeRepresentative(root, examples)
    : {
      status: "not-requested",
      tiers: RUNTIME_TIERS,
      source: "one representative golden-backed executable example",
      reason: RUNTIME_UNAVAILABLE_REASON,
      required_for: "proving that a golden exercises each row on AOT, jet run, and --interpret",
    };
  const acceptance2 = !EXECUTE
    ? { status: "unavailable", reason: "run with --execute to collect AOT, jet run, and --interpret evidence" }
    : execution.status !== "measured"
      ? { status: "blocked", reason: execution.reason ?? "representative executable probe did not complete" }
      : summary.uncovered > 0
        ? { status: "blocked", reason: `${summary.uncovered} coverage rows remain without a golden-backed executable example` }
        : { status: "met", evidence: "all coverage rows have golden-backed examples and the representative witness passed on all tiers" };
  const report = {
    schema: SCHEMA,
    generated_by: "scripts/agent/example-core-census.mjs",
    generated_static_only: !EXECUTE,
    execution,
    classification: {
      no_example: "no executable example",
      no_golden: "example without a golden",
      golden_not_exercising: "golden does not exercise row",
      internal_only: "internal-only: <reason>",
    },
    ci_gate: {
      command: "scripts/agent/jet-env node scripts/agent/example-core-census.mjs --check",
      fails_when: "coverage table differs from the checked-in report",
      uncovered_command: "scripts/agent/jet-env node scripts/agent/example-core-census.mjs --fail-on-uncovered",
      uncovered_fails_when: "summary.uncovered > 0",
    },
    acceptance: {
      "1": { status: "reported", evidence: "authoritative source anchors, stable source-span hashes, examples inventory, and records table" },
      "2": acceptance2,
      "3": { status: "enforced", evidence: "ci_gate command compares the current source census with the checked-in report" },
    },
    authoritative: {
      tir: { file: tirFile, enums: { TExprKind: exprRows.length, TStmt: stmtRows.length } },
      core: { file: coreFile, registry: { records: coreRows.length } },
      examples: {
        root: normalizePath(relative(root, examplesAbsolute)),
        source_files: sourceEntries.length,
        golden_files: goldenFiles.length,
      },
    },
    summary,
    examples: examples
      .map((example) => ({
        file: example.file,
        golden: example.golden,
        golden_paths: example.golden_paths,
      }))
      .sort((left, right) => compareText(left.file, right.file)),
    records,
  };
  return report;
}

function compareLocations(left, right) {
  const file = compareText(left.file, right.file);
  if (file !== 0) return file;
  if (left.line !== right.line) return left.line - right.line;
  return left.column - right.column;
}

function compareRecords(left, right) {
  const familyOrder = new Map([["tir-expr", 0], ["tir-stmt", 1], ["core", 2]]);
  const family = familyOrder.get(left.family) - familyOrder.get(right.family);
  if (family !== 0) return family;
  if (left.family === "core") {
    const kind = compareText(left.kind, right.kind);
    if (kind !== 0) return kind;
    return compareText(left.key, right.key);
  }
  return compareText(left.construct, right.construct);
}

function stableSource(source) {
  return source
    ? { file: source.file, span_hash: source.span_hash }
    : source;
}

function stableLocation(location) {
  return {
    file: location.file,
    golden: location.golden,
    golden_paths: location.golden_paths,
    span_hash: location.span_hash,
  };
}
function coverageProjection(report) {
  return {
    schema: report.schema,
    authoritative: report.authoritative,
    summary: report.summary,
    examples: report.examples,
    records: report.records.map((record) => {
      const { runtime, source, examples, ...coverage } = record;
      return {
        ...coverage,
        source: stableSource(source),
        examples: examples.map(stableLocation),
      };
    }),
  };
}

function assertCoverageMatch(stored, report) {
  if (JSON.stringify(coverageProjection(stored)) !== JSON.stringify(coverageProjection(report))) {
    fail("stale report", `${OUTPUT_JSON} is stale; run scripts/agent/jet-env node scripts/agent/example-core-census.mjs --execute --write`);
  }
}

function checkStoredReport(root, report) {
  const destination = resolve(root, OUTPUT_JSON);
  if (!existsSync(destination)) fail("stale report", `missing ${OUTPUT_JSON}`);
  let stored;
  try {
    stored = JSON.parse(readFileSync(destination, "utf8"));
  } catch (error) {
    fail("stale report", `${OUTPUT_JSON} is not valid JSON: ${error.message}`);
  }
  assertCoverageMatch(stored, report);
}

function runPolicyCheck(report) {
  const changed = JSON.parse(JSON.stringify(report));
  changed.records[0].covered = !changed.records[0].covered;
  let refused = false;
  try {
    assertCoverageMatch(changed, report);
  } catch {
    refused = true;
  }
  if (!refused) fail("policy check", "coverage gate accepted a changed coverage row");
  process.stdout.write("example core census: policy check passed; changed coverage row rejected\n");
}

function writeCanonicalReports(root, report) {
  const json = `${JSON.stringify(report, null, 2)}\n`;
  const outputs = [
    [OUTPUT_JSON, json],
    [OUTPUT_TSV, renderTsv(report)],
    [OUTPUT_MD, renderMarkdown(report)],
  ];
  for (const [path, content] of outputs) {
    const destination = resolve(root, path);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return;
  }
  const report = runCensus(options);
  if (POLICY_CHECK) {
    runPolicyCheck(report);
    return;
  }
  if (options.check) {
    checkStoredReport(options.root, report);
    process.stdout.write(
      `example core census: checked ${report.summary.total} rows; covered=${report.summary.covered} uncovered=${report.summary.uncovered}\n`,
    );
    if (options.failOnUncovered && report.summary.uncovered > 0) process.exitCode = 1;
    return;
  }
  if (options.write) {
    writeCanonicalReports(options.root, report);
    process.stdout.write(
      `example core census: wrote ${OUTPUT_JSON}, ${OUTPUT_TSV}, ${OUTPUT_MD}; covered=${report.summary.covered} uncovered=${report.summary.uncovered} execution=${report.execution.status}\n`,
    );
    if (options.failOnUncovered && report.summary.uncovered > 0) process.exitCode = 1;
    return;
  }
  const serialized = options.format === "json"
    ? `${JSON.stringify(report, null, 2)}\n`
    : options.format === "tsv"
      ? renderTsv(report)
      : options.format === "markdown"
        ? renderMarkdown(report)
        : renderHuman(report);
  if (options.out !== null) {
    const destination = resolve(options.root, options.out);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, serialized);
    process.stdout.write(`WRITE OK format=${options.format} covered=${report.summary.covered} uncovered=${report.summary.uncovered}\n`);
  } else {
    process.stdout.write(serialized);
  }
  if (options.failOnUncovered && report.summary.uncovered > 0) process.exitCode = 1;
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`example-core-census: ${message}\n`);
  process.exitCode = 2;
}
