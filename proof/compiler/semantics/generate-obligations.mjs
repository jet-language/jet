#!/usr/bin/env node

/** Generate proof/compiler/obligations.json from current compiler authorities. */

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { readLedger } from "../../../scripts/agent/lexical-ledger.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const OUTPUT = resolve(ROOT, "proof/compiler/obligations.json");
const TIR_PATH = "crates/jet-codegen/src/Codegen/TIR/mod.rs";
const CORE_PATH = "crates/jet-codegen/src/Prelude/Core.jet";
const SYNTAX_PATH = "crates/jet-foundation/src/Syntax.rs";
const SYNTAX_SPEC_PATH = "docs/spec/syntax-decisions.md";
const MIR_PATH = "crates/jet-foundation/src/MIR.rs";
const ARCHITECTURE_PATH = "docs/spec/architecture.md";
export const OBLIGATION_AUTHORITIES = Object.freeze([
  TIR_PATH,
  CORE_PATH,
  SYNTAX_PATH,
  SYNTAX_SPEC_PATH,
  MIR_PATH,
  ARCHITECTURE_PATH,
]);

function readSource(path) {
  const absolute = resolve(ROOT, path);
  if (!existsSync(absolute)) throw new Error(`missing authority source: ${path}`);
  return readFileSync(absolute, "utf8");
}

function digest(text) {
  return `sha256-${createHash("sha256").update(text, "utf8").digest("hex")}`;
}

function maskRust(source) {
  const output = [...source];
  const blank = (index) => {
    if (source[index] !== "\n" && source[index] !== "\r") output[index] = " ";
  };
  const blankRange = (start, end) => {
    for (let index = start; index < end; index += 1) blank(index);
  };
  for (let index = 0; index < source.length; ) {
    if (source.startsWith("//", index)) {
      const start = index;
      index += 2;
      while (index < source.length && source[index] !== "\n") index += 1;
      blankRange(start, index);
      continue;
    }
    if (source.startsWith("/*", index)) {
      const start = index;
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source.startsWith("/*", index)) {
          depth += 1;
          index += 2;
        } else if (source.startsWith("*/", index)) {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      blankRange(start, index);
      continue;
    }
    const raw = source.slice(index).match(/^r(#+)?"/u);
    if (raw) {
      const hashes = raw[1] ?? "";
      const start = index;
      index += raw[0].length;
      const close = `"${hashes}`;
      while (index < source.length && !source.startsWith(close, index)) index += 1;
      if (index < source.length) index += close.length;
      blankRange(start, index);
      continue;
    }
    if (source[index] === "\"") {
      const start = index;
      index += 1;
      while (index < source.length) {
        if (source[index] === "\\") index += 2;
        else if (source[index] === "\"") {
          index += 1;
          break;
        } else index += 1;
      }
      blankRange(start, index);
      continue;
    }
    if (source[index] === "'" && (source[index + 1] === "\\" || source[index + 2] === "'")) {
      const start = index;
      index += 1;
      while (index < source.length) {
        if (source[index] === "\\") index += 2;
        else if (source[index] === "'") {
          index += 1;
          break;
        } else index += 1;
      }
      blankRange(start, index);
      continue;
    }
    index += 1;
  }
  return output.join("");
}

function matching(masked, open, opener, closer) {
  if (masked[open] !== opener) throw new Error(`expected ${opener} at offset ${open}`);
  let depth = 1;
  for (let index = open + 1; index < masked.length; index += 1) {
    if (masked[index] === opener) depth += 1;
    else if (masked[index] === closer) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  throw new Error(`unclosed ${opener} at offset ${open}`);
}

function enumVariants(source, name) {
  const masked = maskRust(source);
  const match = new RegExp(`\\benum\\s+${name}\\s*\\{`, "u").exec(masked);
  if (!match) throw new Error(`missing ${name} enum`);
  const open = masked.indexOf("{", match.index);
  const close = matching(masked, open, "{", "}");
  const variants = [];
  let index = open + 1;
  while (index < close) {
    while (index < close && /[\s,]/u.test(masked[index])) index += 1;
    if (index >= close) break;
    if (masked[index] === "#" && masked[index + 1] === "[") {
      index = matching(masked, index + 1, "[", "]") + 1;
      continue;
    }
    const variant = /^[A-Za-z_]\w*/u.exec(masked.slice(index));
    if (!variant) throw new Error(`cannot parse ${name} variant near offset ${index}`);
    const variantName = variant[0];
    index += variantName.length;
    let paren = 0;
    let bracket = 0;
    let brace = 0;
    while (index < close) {
      const character = masked[index];
      if (character === "(") paren += 1;
      else if (character === ")") paren -= 1;
      else if (character === "[") bracket += 1;
      else if (character === "]") bracket -= 1;
      else if (character === "{") brace += 1;
      else if (character === "}") brace -= 1;
      else if (character === "," && paren === 0 && bracket === 0 && brace === 0) break;
      index += 1;
    }
    variants.push(variantName);
    if (masked[index] === ",") index += 1;
  }
  if (!variants.length) throw new Error(`empty ${name} enum`);
  return variants;
}
const LEXICAL_ROW_RE = /LexicalEntry\s*\{\s*spelling:\s*("(?:\\.|[^"\\])*")\s*,\s*meaning:\s*("(?:\\.|[^"\\])*")\s*,\s*decision:\s*("(?:\\.|[^"\\])*")\s*\}/g;

function stableSurfaceSlug(value) {
  const base = (value || "none")
    .toLowerCase()
    .replace(/[^a-z0-9]+/gu, "-")
    .replace(/^-+|-+$/gu, "")
    .slice(0, 48) || "none";
  let hash = 2166136261;
  for (const character of value || "none") {
    hash ^= character.codePointAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return `${base}-${(hash >>> 0).toString(16).padStart(8, "0")}`;
}

function syntaxRows(source) {
  const ledger = readLedger();
  const rows = [];
  for (const match of source.matchAll(LEXICAL_ROW_RE)) {
    const index = rows.length;
    const registered = ledger[index];
    if (!registered) throw new Error(`Syntax.rs has an unparsed LexicalEntry at row ${index}`);
    const spelling = JSON.parse(match[1]);
    const meaning = JSON.parse(match[2]);
    const decision = JSON.parse(match[3]);
    if (registered.spelling !== spelling || registered.meaning !== meaning || registered.decision !== decision) {
      throw new Error(`lexical ledger mismatch at row ${index}`);
    }
    rows.push({
      ...row(
        `syntax-registry-${String(index).padStart(3, "0")}-${stableSurfaceSlug(spelling)}`,
        "syntax",
        "construct",
        spelling,
        "semantic.lexical",
        "#2935",
        SYNTAX_PATH,
      ),
      registry_symbol: `LEXICAL_LEDGER[${index}]`,
      meaning,
      decision,
      source_line: source.slice(0, match.index).split("\n").length,
    });
  }
  if (rows.length === 0 || rows.length !== ledger.length) {
    throw new Error(`Syntax.rs lexical registry changed shape: parsed ${rows.length}, ledger returned ${ledger.length}`);
  }
  return rows;
}

function targetSlug(value) {
  return value
    .replace(/([a-z0-9])([A-Z])/gu, "$1-$2")
    .replace(/([A-Z])([A-Z][a-z])/gu, "$1-$2")
    .toLowerCase();
}

function targetRows(source) {
  return enumVariants(source, "MirArtifactTarget").map((variant) => ({
    ...row(
      `adapter:${targetSlug(variant)}`,
      "adapter",
      "execution_boundary",
      `MirArtifactTarget::${variant}`,
      "adapter.observation-correspondence",
      "#2941",
      MIR_PATH,
    ),
    descriptor: "MirArtifactTarget",
    target: variant,
  }));
}

function coreRows(source) {
  const rows = [];
  const lines = source.split("\n");
  for (const [lineNumber, line] of lines.entries()) {
    const match = line.match(/^dispatcher_row\s+(plain|adapter|receiver)\s+(\S+)\s+(\S+)\s+\|\s*(.*)$/u);
    if (!match) continue;
    const [, family, module, member, expression] = match;
    let key;
    if (family === "receiver") {
      const receiver = expression.match(/CoreCallRecord::receiver(?:_with_symbol|_with_coverage)?\s*\(\s*&?\s*\[([^\]]*)\]\s*,\s*([^,\s]+)/u);
      if (!receiver) throw new Error(`cannot parse receiver row at ${CORE_PATH}:${lineNumber + 1}`);
      const receiverTypes = receiver[1].replace(/\s+/gu, "").replaceAll('"', "");
      const receiverMember = receiver[2].replaceAll('"', "");
      key = `core-call:receiver:${receiverTypes}::${receiverMember}`;
    } else {
      key = `core-call:${family}:${module}.${member}`;
    }
    rows.push({
      id: key,
      family: "core",
      kind: family === "adapter" ? "adapter" : "core_call",
      subject: `${module}.${member}`,
      rule: family === "adapter" ? "core.adapter.correspondence" : "core.call.observation",
      owner: family === "adapter" ? "#2941" : "#2940",
      source: CORE_PATH,
      source_line: lineNumber + 1,
      obligation_state: "uncovered",
    });
  }
  if (!rows.length) throw new Error("no Core dispatcher rows found");
  const ids = new Set();
  for (const row of rows) {
    if (ids.has(row.id)) throw new Error(`duplicate Core obligation ${row.id}`);
    ids.add(row.id);
  }
  return rows;
}

function row(id, family, kind, subject, rule, owner, source) {
  return { id, family, kind, subject, rule, owner, source, obligation_state: "uncovered" };
}

export function buildObligations() {
  const tir = readSource(TIR_PATH);
  const core = readSource(CORE_PATH);
  const syntax = readSource(SYNTAX_PATH);
  const syntaxSpec = readSource(SYNTAX_SPEC_PATH);
  const mir = readSource(MIR_PATH);
  const architecture = readSource(ARCHITECTURE_PATH);
  const rows = [];
  rows.push(...syntaxRows(syntax));
  for (const variant of enumVariants(tir, "TExprKind")) {
    rows.push(row(`tir-expr:TExprKind.${variant}`, "syntax", "construct", `TExprKind::${variant}`, "semantic.expression", "#2935", TIR_PATH));
  }
  for (const variant of enumVariants(tir, "TStmt")) {
    rows.push(row(`tir-stmt:TStmt.${variant}`, "syntax", "construct", `TStmt::${variant}`, "semantic.statement", "#2935", TIR_PATH));
  }
  rows.push(...coreRows(core));
  rows.push(...targetRows(mir));

  const boundaries = [
    ["source-bytes", "source", "#2936", "source.bytes-and-origins", "source processing"],
    ["lexing", "source", "#2936", "source.lexical-structure", "lexical structure"],
    ["parsing", "source", "#2936", "source.parse-and-rejection", "parsing and rejection"],
    ["names", "checking", "#2937", "sema.names-and-facts", "names and source graph"],
    ["types", "checking", "#2937", "sema.types", "types"],
    ["ownership", "checking", "#2937", "sema.ownership", "ownership"],
    ["effects", "checking", "#2937", "sema.effects", "effects and capability"],
    ["compile-time", "comptime", "#2938", "comptime.evaluation-and-cache", "compile-time evaluation"],
    ["lowering", "lowering", "#2939", "lowering.shared-operations", "shared lowering"],
    ["optimization", "lowering", "#2939", "lowering.optimization", "optimization"],
    ["core-prelude", "runtime", "#2940", "runtime.core-and-prelude", "Jet-owned Core and Prelude"],
    ["tasks", "runtime", "#2940", "runtime.tasks-and-scheduling", "tasks and scheduling"],
    ["ffi", "foreign", "#2941", "adapter.ffi-contract", "FFI and callbacks"],
    ["layout", "foreign", "#2941", "adapter.layout-contract", "layout and ABI"],
    ["resource", "runtime", "#2935", "semantic.resource-premises", "fuel, stack, allocation, and resource failures"],
    ["foreign-tool", "foreign", "#2925", "boundary.foreign-tools", "foreign compiler and environment assumptions"],
  ];
  for (const [name, family, owner, ruleName, subject] of boundaries) {
    rows.push(row(`boundary:${name}`, family, "boundary", subject, ruleName, owner, ARCHITECTURE_PATH));
  }

  const owners = ["#2925", "#2935", "#2936", "#2937", "#2938", "#2939", "#2940", "#2941", "#2944"];
  const duplicate = new Set();
  for (const entry of rows) {
    if (duplicate.has(entry.id)) throw new Error(`duplicate obligation id ${entry.id}`);
    duplicate.add(entry.id);
    if (!owners.includes(entry.owner)) throw new Error(`unknown obligation owner ${entry.owner}`);
  }
  return {
    schema: "jet.compiler-obligations.v1",
    schema_version: 1,
    generated_by: "proof/compiler/semantics/generate-obligations.mjs",
    authorities: [
      { path: TIR_PATH, sha256: digest(tir) },
      { path: CORE_PATH, sha256: digest(core) },
      { path: SYNTAX_PATH, sha256: digest(syntax) },
      { path: SYNTAX_SPEC_PATH, sha256: digest(syntaxSpec) },
      { path: MIR_PATH, sha256: digest(mir) },
      { path: ARCHITECTURE_PATH, sha256: digest(architecture) },
    ],
    owner_set: owners,
    row_count: rows.length,
    rows,
  };
}

export function writeObligations(path = OUTPUT) {
  const result = buildObligations();
  writeFileSync(path, `${JSON.stringify(result, null, 2)}\n`);
  return result;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try {
    const result = writeObligations();
    process.stdout.write(`obligations: wrote ${result.row_count} rows\n`);
  } catch (error) {
    process.stderr.write(`obligations: ${error.message}\n`);
    process.exitCode = 1;
  }
}
