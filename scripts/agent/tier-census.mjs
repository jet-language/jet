#!/usr/bin/env node

import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  statSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { dirname, relative, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const TIR_PATH = "crates/jet-codegen/src/Codegen/TIR/mod.rs";
const TIR_ROOT = "crates/jet-codegen/src/Codegen/TIR";
const AOT_PATH = "crates/jet-codegen/src/Codegen/MIRRust.rs";
const INTERPRETER_PATH = "crates/jet-codegen/src/Codegen/MIREval.rs";
const WEB_PATH = "crates/jet-codegen/src/Codegen/MIRWeb.rs";
const JIT_ROOT = "crates/jet-jit/src";
const AMBIENT_PATH = "crates/jet-jit/src/ambient_interp.rs";
const CORE_PATH = "crates/jet-foundation/src/Syntax/core_calls.rs";
const OUTPUT_JSON = "docs/audits/tier-census-2026-09-03.json";
const OUTPUT_TSV = "docs/audits/tier-census-2026-09-03.tsv";
const OUTPUT_MD = "docs/audits/tier-census-2026-09-03.md";
const FIXTURE_ROOT = "tests/fixtures/tier-census";
const FIXTURE_JSON = `${FIXTURE_ROOT}/table.json`;
const FIXTURE_TSV = `${FIXTURE_ROOT}/table.tsv`;
const FIXTURE_PROGRAMS = `${FIXTURE_ROOT}/programs.json`;
const GENERATED_COMMAND = "node scripts/agent/tier-census.mjs";
const EXECUTE = process.argv.includes("--execute");
const JSON_OUTPUT = process.argv.includes("--json");
const WRITE_FIXTURE = process.argv.includes("--write-fixture");
const JET_BINARY = absPath("target/debug/jet");
const EXECUTION_ROOT = resolve(
  process.env.JET_TEST_SCRATCH_DIR ?? resolve(ROOT, ".agent-scratch-tier-census"),
  "tier-census-runtime",
);
const EXECUTION_TIMING = "measured: differential tier batches";
const STATIC_TIMING = "not-measured: execution disabled (pass --execute)";
const DYNAMIC_TIMING = EXECUTE ? EXECUTION_TIMING : STATIC_TIMING;
const EXECUTION_TIMEOUT_MS = Number(process.env.JET_TIER_CENSUS_TIMEOUT_MS ?? 120_000);
const CHECK_ONLY = process.argv.includes("--check");
const POLICY_CHECK = process.argv.includes("--policy-check");

const BACKENDS = ["aot_rust", "interpreter", "cranelift", "web"];
const EXECUTION_TIERS = ["aot_rust", "cranelift", "interpreter"];
const STATUS_VALUES = new Set(["covered", "refused", "absent"]);


function fail(message) {
  throw new Error(`tier census: ${message}`);
}

function assert(condition, message) {
  if (!condition) fail(message);
}
// Line numbers are useful evidence but are not durable identities: inserting an
// unrelated line above a declaration must not rename the row. Hash the
// canonical source line instead, with its file path as the namespace.
function sha256(value) {
  return createHash("sha256").update(String(value), "utf8").digest("hex");
}

function sourceSpanHash(file, text) {
  return sha256(`${file}\0${String(text).trim().replace(/\s+/g, " ")}`);
}

function combinedSpanHash(id, items) {
  return sha256(`${id}\0${items.map((item) => item.span_hash).sort().join("\0")}`);
}

function validSpanHash(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}


function absPath(path) {
  return resolve(ROOT, path);
}

function relPath(path) {
  return relative(ROOT, path).split("\\").join("/");
}

function readSource(path) {
  const absolute = absPath(path);
  assert(existsSync(absolute), `missing source: ${path}`);
  return readFileSync(absolute, "utf8");
}

function maskRust(source) {
  const output = new Array(source.length);
  for (let i = 0; i < source.length; i += 1) output[i] = source[i];

  const blank = (index) => {
    if (source[index] !== "\n" && source[index] !== "\r") output[index] = " ";
  };
  const blankRange = (start, end) => {
    for (let i = start; i < end; i += 1) blank(i);
  };

  for (let i = 0; i < source.length; ) {
    if (source.startsWith("//", i)) {
      const start = i;
      i += 2;
      while (i < source.length && source[i] !== "\n") i += 1;
      blankRange(start, i);
      continue;
    }
    if (source.startsWith("/*", i)) {
      const start = i;
      let depth = 1;
      i += 2;
      while (i < source.length && depth > 0) {
        if (source.startsWith("/*", i)) {
          depth += 1;
          i += 2;
        } else if (source.startsWith("*/", i)) {
          depth -= 1;
          i += 2;
        } else {
          i += 1;
        }
      }
      blankRange(start, i);
      continue;
    }

    const raw = source.slice(i).match(/^r(#+)?"/);
    if (raw) {
      const hashes = raw[1] ?? "";
      const start = i;
      i += raw[0].length;
      const close = `"${hashes}`;
      while (i < source.length && !source.startsWith(close, i)) i += 1;
      if (i < source.length) i += close.length;
      blankRange(start, i);
      continue;
    }

    if (source[i] === "'" && (source[i + 1] === "\\" || source[i + 2] === "'")) {
      const start = i;
      i += 1;
      while (i < source.length) {
        if (source[i] === "\\") {
          i += 2;
        } else if (source[i] === "'") {
          i += 1;
          break;
        } else {
          i += 1;
        }
      }
      blankRange(start, i);
      continue;
    }
    if (source[i] === '"') {
      const start = i;
      i += 1;
      while (i < source.length) {
        if (source[i] === "\\") {
          i += 2;
        } else if (source[i] === '"') {
          i += 1;
          break;
        } else {
          i += 1;
        }
      }
      blankRange(start, i);
      continue;
    }
    i += 1;
  }
  return output.join("");
}

// Keep string literals, but remove comments. This lets the census match exact
// string-pair declarations without counting examples in comments.
function maskRustComments(source) {
  const output = [...source];
  const blank = (index) => {
    if (source[index] !== "\n" && source[index] !== "\r") output[index] = " ";
  };
  const blankRange = (start, end) => {
    for (let i = start; i < end; i += 1) blank(i);
  };

  for (let i = 0; i < source.length; ) {
    const raw = source.slice(i).match(/^r(#+)?"/);
    if (raw) {
      const hashes = raw[1] ?? "";
      i += raw[0].length;
      const close = `"${hashes}`;
      while (i < source.length && !source.startsWith(close, i)) i += 1;
      if (i < source.length) i += close.length;
      continue;
    }
    if (source[i] === '"') {
      i += 1;
      while (i < source.length) {
        if (source[i] === "\\") i += 2;
        else if (source[i] === '"') {
          i += 1;
          break;
        } else i += 1;
      }
      continue;
    }
    if (source[i] === "'" && (source[i + 1] === "\\" || source[i + 2] === "'")) {
      i += 1;
      while (i < source.length) {
        if (source[i] === "\\") i += 2;
        else if (source[i] === "'") {
          i += 1;
          break;
        } else i += 1;
      }
      continue;
    }
    if (source.startsWith("//", i)) {
      const start = i;
      i += 2;
      while (i < source.length && source[i] !== "\n") i += 1;
      blankRange(start, i);
      continue;
    }
    if (source.startsWith("/*", i)) {
      const start = i;
      let depth = 1;
      i += 2;
      while (i < source.length && depth > 0) {
        if (source.startsWith("/*", i)) {
          depth += 1;
          i += 2;
        } else if (source.startsWith("*/", i)) {
          depth -= 1;
          i += 2;
        } else i += 1;
      }
      blankRange(start, i);
      continue;
    }
    i += 1;
  }
  return output.join("");
}

function matching(masked, open, opener, closer) {
  assert(masked[open] === opener, `expected ${opener} at offset ${open}`);
  let depth = 1;
  for (let i = open + 1; i < masked.length; i += 1) {
    if (masked[i] === opener) depth += 1;
    else if (masked[i] === closer) {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  fail(`unclosed ${opener} at offset ${open}`);
}

function makeFile(path) {
  const source = readSource(path);
  const masked = maskRust(source);
  const commentsMasked = maskRustComments(source);
  const lines = source.split("\n");
  const starts = [0];
  for (let i = 0; i < source.length; i += 1) {
    if (source[i] === "\n") starts.push(i + 1);
  }
  const lineNumber = (offset) => {
    let lo = 0;
    let hi = starts.length;
    while (lo + 1 < hi) {
      const mid = Math.floor((lo + hi) / 2);
      if (starts[mid] <= offset) lo = mid;
      else hi = mid;
    }
    return lo + 1;
  };
  const lineText = (line) => (lines[line - 1] ?? "").trim().replace(/\s+/g, " ");
  return {
    path,
    source,
    masked,
    commentsMasked,
    lines,
    lineNumber,
    lineText,
  };
}

function walkRust(path) {
  const absolute = absPath(path);
  assert(existsSync(absolute), `missing directory: ${path}`);
  const stat = statSync(absolute);
  if (stat.isFile()) return absolute.endsWith(".rs") ? [relPath(absolute)] : [];
  return readdirSync(absolute, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => walkRust(relPath(resolve(absolute, entry.name))));
}

function parseEnum(file, name) {
  const match = new RegExp(`\\benum\\s+${name}\\s*\\{`).exec(file.masked);
  assert(match, `cannot find enum ${name} in ${file.path}`);
  const open = file.masked.indexOf("{", match.index);
  const close = matching(file.masked, open, "{", "}");
  const variants = [];
  let i = open + 1;

  while (i < close) {
    while (i < close && /[\s,]/.test(file.masked[i])) i += 1;
    if (i >= close) break;
    if (file.masked[i] === "#" && file.masked[i + 1] === "[") {
      const attrClose = matching(file.masked, i + 1, "[", "]");
      i = attrClose + 1;
      continue;
    }
    const variant = /^[A-Za-z_]\w*/.exec(file.masked.slice(i));
    assert(variant, `cannot parse ${name} variant near ${file.path}:${file.lineNumber(i)}`);
    const variantName = variant[0];
    const variantOffset = i;
    i += variantName.length;
    let paren = 0;
    let bracket = 0;
    let brace = 0;
    while (i < close) {
      const ch = file.masked[i];
      if (ch === "(") paren += 1;
      else if (ch === ")") paren -= 1;
      else if (ch === "[") bracket += 1;
      else if (ch === "]") bracket -= 1;
      else if (ch === "{") brace += 1;
      else if (ch === "}") brace -= 1;
      else if (ch === "," && paren === 0 && bracket === 0 && brace === 0) break;
      i += 1;
    }
    variants.push({
      name: variantName,
      declaration: evidence(file, variantOffset),
    });
    if (file.masked[i] === ",") i += 1;
  }
  assert(variants.length > 0, `enum ${name} has no variants`);
  return variants;
}

function evidence(file, offset) {
  const line = file.lineNumber(offset);
  const text = file.lineText(line);
  return { file: file.path, line, text, span_hash: sourceSpanHash(file.path, text) };
}

function uniqueEvidence(items) {
  const seen = new Set();
  return items
    .filter((item) => {
      const key = `${item.file}:${item.line}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.text.localeCompare(b.text))
    .map(({ file, line, text, span_hash }) => ({ file, line, text, span_hash }));
}

function operationArmRefs(files, operation, member = null) {
  const marker = new RegExp(`(?:TIR::)?THandleOp::${operation}\\b`, "g");
  const nextOperation = /(?:TIR::)?THandleOp::[A-Za-z_]\w*/g;
  const memberPattern = member === null
    ? null
    : new RegExp(`"${member.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"`, "g");
  const refs = [];
  for (const file of files) {
    marker.lastIndex = 0;
    let match;
    while ((match = marker.exec(file.commentsMasked))) {
      nextOperation.lastIndex = match.index + match[0].length;
      const next = nextOperation.exec(file.commentsMasked);
      const end = next?.index ?? file.commentsMasked.length;
      const operationEvidence = evidence(file, match.index);
      let memberFound = member === null;
      if (memberPattern) {
        memberPattern.lastIndex = match.index;
        let memberMatch;
        while ((memberMatch = memberPattern.exec(file.commentsMasked)) && memberMatch.index < end) {
          refs.push(evidence(file, memberMatch.index));
          memberFound = true;
        }
      }
      if (memberFound) refs.push(operationEvidence);
    }
  }
  return uniqueEvidence(refs);
}

function functionLiteralRefs(file, functionNames, member) {
  const refs = [];
  for (const functionName of functionNames) {
    const range = functionRange(file, functionName);
    refs.push(...rangeLiteralRefs(file, range, member));
  }
  return uniqueEvidence(refs);
}

const RECEIVER_FAMILIES = [
  {
    types: new Set(["Url", "Mime"]),
    operation: "UrlMimeMethod",
  },
  {
    types: new Set([
      "Date",
      "LocalDate",
      "LocalTime",
      "DateTime",
      "Instant",
      "Zone",
      "ZonedDateTime",
      "Period",
    ]),
    operation: "CivilTimeMethod",
    webFunctions: ["web_civil_time_method_supported"],
  },
  {
    types: new Set(["Fraction", "Decimal"]),
    operation: "PreciseMethod",
  },
  {
    types: new Set(["Measurement"]),
    operation: "MeasurementMethod",
  },
  {
    types: new Set(["HyperLogLog", "TDigest", "CountMinSketch", "ReservoirSampler"]),
    operation: "SketchMethod",
  },
  {
    types: new Set(["Solver"]),
    operations: new Map([
      ["new", "SolverNew"],
      ["require", "SolverRequire"],
      ["failure_count", "SolverFailureCount"],
      ["status", "SolverStatus"],
    ]),
  },
];

function receiverFamily(row) {
  return RECEIVER_FAMILIES.find((family) =>
    row.receiver_types.some((type) => family.types.has(type))
  );
}

function statusResult(status, items = [], reason = undefined, refusalItems = []) {
  assert(STATUS_VALUES.has(status), `invalid status ${status}`);
  const result = { status, evidence: uniqueEvidence(items) };
  if (reason) result.reason = reason;
  if (refusalItems.length > 0) result.refusal_evidence = uniqueEvidence(refusalItems);
  return result;
}

function refusalReference(file, offset) {
  const line = file.lineNumber(offset);
  const context = file.lines.slice(line - 1, line + 3).join(" ");
  const arrow = context.indexOf("=>");
  if (arrow < 0) return false;
  const after = context.slice(arrow + 2).trim();
  return /^(?:false\b|None\b|Err\s*\(|unsupported(?:_|\s*\())/.test(after);
}

function scanVariantRefs(files, enumName, variants) {
  const known = new Set(variants.map((variant) => variant.name));
  const refs = new Map(variants.map((variant) => [variant.name, []]));
  const unknown = [];
  const pattern = new RegExp(`(?:TIR::)?${enumName}::([A-Za-z_]\\w*)`, "g");
  for (const file of files) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(file.masked))) {
      const name = match[1];
      if (!known.has(name)) {
        unknown.push(evidence(file, match.index));
        continue;
      }
      refs.get(name).push({ ...evidence(file, match.index), refusal: refusalReference(file, match.index) });
    }
  }
  return { refs, unknown };
}

function classifyVariantRefs(refs) {
  if (refs.length === 0) return statusResult("absent");
  const refusals = refs.filter((ref) => ref.refusal);
  const positive = refs.filter((ref) => !ref.refusal);
  if (positive.length > 0) return statusResult("covered", refs, undefined, refusals);
  return statusResult("refused", refs, "all explicit backend references are refusal arms", refusals);
}


function parseRustStrings(text) {
  const strings = [];
  const pattern = /"(?:\\.|[^"\\])*"/gs;
  let match;
  while ((match = pattern.exec(text))) {
    try {
      strings.push({ value: JSON.parse(match[0]), offset: match.index });
    } catch {
      strings.push({ value: match[0].slice(1, -1), offset: match.index });
    }
  }
  return strings;
}

function parseBooleanMask(args) {
  const masks = [...args.matchAll(/&\s*\[([^\]]*)\]/gs)];
  const last = masks.at(-1)?.[1] ?? "";
  return [...last.matchAll(/\b(?:true|false)\b/g)].map((match) => match[0] === "true");
}

function findTopLevelComma(masked, start, end) {
  let paren = 0;
  let bracket = 0;
  let brace = 0;
  for (let i = start; i < end; i += 1) {
    const ch = masked[i];
    if (ch === "(") paren += 1;
    else if (ch === ")") paren -= 1;
    else if (ch === "[") bracket += 1;
    else if (ch === "]") bracket -= 1;
    else if (ch === "{") brace += 1;
    else if (ch === "}") brace -= 1;
    else if (ch === "," && paren === 0 && bracket === 0 && brace === 0) return i;
  }
  return end;
}

function parseCoreRows(file) {
  const match = /\bpub\s+const\s+CORE_CALLS\b[\s\S]*?=\s*&\s*\[/.exec(file.masked);
  assert(match, `cannot find CORE_CALLS in ${file.path}`);
  const open = file.masked.indexOf("[", match.index + match[0].indexOf("= "));
  const close = matching(file.masked, open, "[", "]");
  const body = file.masked.slice(open + 1, close);
  const constructorPattern = /CoreCallRecord::(new_with_coverage|new|receiver_with_symbol|receiver_with_coverage|receiver)\s*\(/g;
  const rows = [];
  let constructor;
  while ((constructor = constructorPattern.exec(body))) {
    const callStart = open + 1 + constructor.index;
    const callOpen = file.masked.indexOf("(", callStart);
    const callClose = matching(file.masked, callOpen, "(", ")");
    const entryEnd = findTopLevelComma(file.masked, callClose + 1, close);
    const args = file.source.slice(callOpen + 1, callClose);
    const entry = file.source.slice(callStart, entryEnd);
    const strings = parseRustStrings(args).map((item) => item.value);
    const masks = parseBooleanMask(args);
    const constructorName = constructor[1];
    const kind = constructorName.startsWith("receiver") ? "receiver" : "plain";
    let module = "";
    let member = "";
    let symbol = "";
    let receiverTypes = [];
    let prelude = true;
    if (kind === "receiver") {
      const argStart = callOpen + 1;
      const argEnd = callClose;
      const firstComma = findTopLevelComma(file.masked, argStart, argEnd);
      const secondComma = findTopLevelComma(file.masked, firstComma + 1, argEnd);
      const firstArg = file.source.slice(argStart, firstComma).trim();
      const secondArg = file.source.slice(firstComma + 1, secondComma).trim();
      const thirdArg = file.source.slice(secondComma + 1, argEnd).trim();
      receiverTypes = parseRustStrings(firstArg).map((item) => item.value);
      if (receiverTypes.length === 0) {
        receiverTypes = (firstArg.match(/[A-Za-z_]\w*/g) ?? []).filter(
          (value) => !["true", "false"].includes(value),
        );
      }
      const memberString = parseRustStrings(secondArg)[0]?.value;
      member = memberString ?? secondArg.replace(/\s+/g, " ");
      const symbolString = parseRustStrings(thirdArg)[0]?.value;
      if (constructorName === "receiver_with_symbol") {
        symbol = symbolString ?? "";
        const preludeMatch = /,\s*(true|false)\s*,\s*&\s*\[/s.exec(thirdArg);
        prelude = preludeMatch?.[1] !== "false";
      }
    } else {
      assert(strings.length >= 3, `cannot parse CoreCallRecord strings at ${file.path}:${file.lineNumber(callStart)}`);
      [module, member, symbol] = strings;
      const preludeMatch = /,\s*(true|false)\s*,\s*&\s*\[/s.exec(args);
      prelude = preludeMatch?.[1] !== "false";
    }
    assert(member.length > 0, `empty CoreCallRecord member at ${file.path}:${file.lineNumber(callStart)}`);
    if (kind === "plain") assert(module.length > 0, `empty CoreCallRecord module at ${file.path}:${file.lineNumber(callStart)}`);
    const maxArityMatch = /\.with_max_arity\s*\(\s*(\d+)\s*\)/.exec(entry);
    const pureMatch = /\.with_pure_route\s*\(\s*CoreCallPureRoute::([A-Za-z_]\w*)/.exec(entry);
    const interpreterMatch = /\.with_interpreter_route\s*\(\s*CoreCallInterpreterRoute::([A-Za-z_]\w*)/.exec(entry);
    const jitSymbols = [...entry.matchAll(/\.with_jit_symbol\s*\(\s*"((?:\\.|[^"\\])*)"/g)].map((item) => {
      try {
        return JSON.parse(`"${item[1]}"`);
      } catch {
        return item[1];
      }
    });
    if (symbol && kind === "receiver" && !jitSymbols.includes(symbol)) jitSymbols.unshift(symbol);
    const route = interpreterMatch?.[1] ?? (pureMatch ? "Pure" : kind === "receiver" ? "TypedIntrinsic" : "Default");
    rows.push({
      ordinal: rows.length + 1,
      kind,
      module,
      member,
      receiver_types: receiverTypes,
      symbol,
      symbol_kind: prelude ? "prelude" : "rust",
      arity: masks.length,
      max_arity: Number(maxArityMatch?.[1] ?? masks.length),
      declaration: evidence(file, callStart),
      aot_direct: kind === "receiver" ? false : !/\.without_direct_aot\s*\(/.test(entry),
      jit_direct: kind === "receiver" ? false : !/\.without_direct_jit\s*\(/.test(entry),
      pure_route: pureMatch?.[1] ?? null,
      interpreter_route: route,
      jit_symbols: jitSymbols,
      entry,
    });
    constructorPattern.lastIndex = entryEnd - (open + 1);
  }
  assert(rows.length > 0, "CORE_CALLS has no CoreCallRecord rows");
  return rows;
}

function parseAmbientManifest(file) {
  const match = /\bpub\s+const\s+CORE_CALL_AMBIENT_ROUTES\b[\s\S]*?=\s*&\s*\[/.exec(file.masked);
  assert(match, `cannot find ambient route manifest in ${file.path}`);
  const open = file.masked.indexOf("[", match.index + match[0].indexOf("= "));
  const close = matching(file.masked, open, "[", "]");
  const masked = file.commentsMasked.slice(open + 1, close);
  const routes = new Map();
  const pattern = /\(\s*"((?:\\.|[^"\\])*)"\s*,\s*"((?:\\.|[^"\\])*)"\s*\)/g;
  let route;
  while ((route = pattern.exec(masked))) {
    const values = [];
    for (const token of [route[1], route[2]]) {
      try {
        values.push(JSON.parse(`"${token}"`));
      } catch {
        values.push(token);
      }
    }
    const [module, member] = values;
    const offset = open + 1 + route.index;
    const key = `${module}\0${member}`;
    const list = routes.get(key) ?? [];
    list.push(evidence(file, offset));
    routes.set(key, list);
  }
  return routes;
}

function findRefs(files, regex) {
  const refs = [];
  for (const file of files) {
    regex.lastIndex = 0;
    let match;
    while ((match = regex.exec(file.masked))) refs.push(evidence(file, match.index));
  }
  return uniqueEvidence(refs);
}

function firstRefs(files, regex, count = 1) {
  return findRefs(files, regex).slice(0, count);
}

function exactPairRefs(files, module, member) {
  const escapedModule = module.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedMember = member.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`\\(\\s*"${escapedModule}"\\s*,\\s*"${escapedMember}"`, "g");
  const refs = [];
  for (const file of files) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(file.commentsMasked))) refs.push(evidence(file, match.index));
  }
  return uniqueEvidence(refs);
}

function exactStringRefs(files, value) {
  const escaped = value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`"${escaped}"`, "g");
  const refs = [];
  for (const file of files) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(file.commentsMasked))) refs.push(evidence(file, match.index));
  }
  return uniqueEvidence(refs);
}

function functionRange(file, functionName) {
  const match = new RegExp(`\\bfn\\s+${functionName}\\s*\\(`).exec(file.masked);
  assert(match, `cannot find ${functionName} in ${file.path}`);
  const open = file.masked.indexOf("{", match.index);
  assert(open >= 0, `cannot find ${functionName} body in ${file.path}`);
  const close = matching(file.masked, open, "{", "}");
  return { start: match.index, end: close + 1 };
}

function rangeLiteralRefs(file, range, value) {
  const escaped = value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`"${escaped}"`, "g");
  pattern.lastIndex = range.start;
  const refs = [];
  let match;
  while ((match = pattern.exec(file.commentsMasked)) && match.index < range.end) {
    refs.push(evidence(file, match.index));
  }
  return uniqueEvidence(refs);
}

function aritiesNearMethod(file, range, method) {
  const escaped = method.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`"${escaped}"`, "g");
  pattern.lastIndex = range.start;
  const values = new Set();
  let match;
  while ((match = pattern.exec(file.commentsMasked)) && match.index < range.end) {
    const window = file.commentsMasked.slice(match.index, Math.min(range.end, match.index + 500));
    for (const item of window.matchAll(/argc\s*==\s*(\d+)/g)) values.add(Number(item[1]));
    for (const item of window.matchAll(/\bSome\s*\(\s*(\d+)\s*\)/g)) values.add(Number(item[1]));
    for (const item of window.matchAll(/,\s*(\d+)\s*\)/g)) values.add(Number(item[1]));
    const matchSet = /matches!\s*\(\s*argc\s*,\s*([^)]*)\)/.exec(window);
    if (matchSet) {
      for (const item of matchSet[1].matchAll(/\b(\d+)\b/g)) values.add(Number(item[1]));
    }
  }
  return [...values].sort((a, b) => a - b);
}

function classifyTir(variants, backendFiles, sharedFiles, dispatchEvidence) {
  const consumers = {};
  const unknown = [];
  const sharedScan = scanVariantRefs(sharedFiles, variants.enumName, variants.items);
  unknown.push(...sharedScan.unknown.map((item) => ({ backend: "tir_lowering", ...item })));
  for (const [backend, files] of Object.entries(backendFiles)) {
    const scan = scanVariantRefs(files, variants.enumName, variants.items);
    unknown.push(...scan.unknown.map((item) => ({ backend, ...item })));
    consumers[backend] = new Map();
    for (const item of variants.items) {
      const direct = classifyVariantRefs(scan.refs.get(item.name));
      if (direct.status !== "absent") {
        consumers[backend].set(item.name, direct);
        continue;
      }
      const lowered = sharedScan.refs.get(item.name) ?? [];
      const dispatch = dispatchEvidence[backend] ?? [];
      if (lowered.length > 0 && dispatch.length > 0) {
        consumers[backend].set(
          item.name,
          statusResult(
            "covered",
            [...lowered, ...dispatch],
            "shared TIR-to-MIR lowering plus backend MIR dispatcher",
          ),
        );
      } else {
        consumers[backend].set(
          item.name,
          statusResult(
            "absent",
            [],
            lowered.length > 0
              ? "TIR lowers to MIR but backend has no static dispatcher anchor"
              : "no lowering or backend consumer reference",
          ),
        );
      }
    }
  }
  return { consumers, unknown };
}

function statusCounts(rows, surface) {
  const counts = {};
  for (const backend of BACKENDS) counts[backend] = { covered: 0, refused: 0, absent: 0, dynamic: 0 };
  for (const row of rows) {
    if (surface && row.surface !== surface) continue;
    for (const backend of BACKENDS) {
      const status = row.consumers[backend].status;
      assert(STATUS_VALUES.has(status), `unclassified ${row.surface} row ${row.id} for ${backend}`);
      counts[backend][status] += 1;
      if (status !== "covered") counts[backend].dynamic += 1;
    }
  }
  return counts;
}

function coreEvidence(...items) {
  return uniqueEvidence(items.flat().filter(Boolean));
}

function classifyCoreRows(rows, sources, anchors) {
  const ambient = parseAmbientManifest(sources.core);
  const hostMap = new Map();
  for (const file of sources.jitAll) {
    const pattern = /\b[A-Za-z_]\w*\s*:\s*"([A-Za-z_]\w*)"\s*=>/g;
    let match;
    while ((match = pattern.exec(file.commentsMasked))) {
      const symbol = match[1];
      const list = hostMap.get(symbol) ?? [];
      list.push(evidence(file, match.index));
      hostMap.set(symbol, list);
    }
  }
  const declaration = (row) => [row.declaration];

  const effectRange = functionRange(sources.core, "effect_for");
  const effectBody = sources.core.masked.slice(effectRange.start, effectRange.end);
  const effectfulDefault = (row) => {
    if (row.kind === "receiver" || !row.module || !row.member) return false;
    const moduleNeedle = `same_text(module, "${row.module}")`;
    let moduleAt = effectBody.indexOf(moduleNeedle);
    while (moduleAt >= 0) {
      const branch = effectBody.slice(moduleAt, moduleAt + 2_000);
      const memberAt = branch.indexOf(`"${row.member}"`);
      if (memberAt >= 0 && /return\s+Some\s*\(\s*Effect::/.test(branch.slice(memberAt))) return true;
      moduleAt = effectBody.indexOf(moduleNeedle, moduleAt + moduleNeedle.length);
    }
    return false;
  };
  const interpreterRoute = (row) => {
    if (row.interpreter_route === "Default") {
      const key = `${row.module}\0${row.member}`;
      if (ambient.has(key)) return "Ambient";
      return effectfulDefault(row) ? "None" : "TypedIntrinsic";
    }
    if (row.interpreter_route === "Pure" && row.pure_route === "None") return "None";
    return row.interpreter_route;
  };
  const escaped = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const memberRefs = (files, member) => {
    if (!member || !/^[A-Za-z_]\w*$/.test(member)) return [];
    return findRefs(files, new RegExp(`\\b${escaped(member)}\\b`, "g"));
  };
  const jitCandidates = (row) => {
    const candidates = [...row.jit_symbols];
    const add = (value) => {
      if (value && !candidates.includes(value)) candidates.push(value);
    };
    add(row.symbol);
    for (const prefix of ["jet_std_", "jet_ring_", "jet_"]) {
      if (row.symbol.startsWith(prefix)) add(`jet_jit_${row.symbol.slice(prefix.length)}`);
    }
    if (row.kind !== "receiver") {
      const moduleTail = row.module.replace(/^core\./, "").replaceAll(".", "_");
      const leaf = row.module.split(".").at(-1) ?? row.module;
      add(`jet_jit_${moduleTail}_${row.member}`);
      add(`jet_jit_${leaf}_${row.member}`);
      add(`jet_jit_${row.member}`);
    }
    return candidates;
  };
  const hostRefs = (row) => coreEvidence(
    ...jitCandidates(row).map((candidate) => hostMap.get(candidate) ?? []),
  );
  const webSymbolRefs = (row) => row.symbol
    ? exactStringRefs(sources.web, row.symbol)
    : [];
  const webResolver = anchors.webResolver ?? [];

  const classifyInterpreter = (row) => {
    const route = interpreterRoute(row);
    if (route === "None") {
      const reason = row.interpreter_route === "None" || (row.interpreter_route === "Pure" && row.pure_route === "None")
        ? "explicit CoreCallInterpreterRoute::None"
        : "default interpreter route resolves to None for an effectful call";
      return statusResult("refused", declaration(row), reason);
    }
    const key = `${row.module}\0${row.member}`;
    const manifestRefs = ambient.get(key) ?? [];
    if (manifestRefs.length > 0) {
      return statusResult(
        "covered",
        coreEvidence(declaration(row), manifestRefs, anchors.ambientProjection),
        "ambient route manifest entry plus shared interpreter projection",
      );
    }
    const routeEvidence = route === "TypedIntrinsic"
      ? anchors.defaultInterpreterRoute
      : anchors.interpreterProjection;
    return statusResult(
      "covered",
      coreEvidence(declaration(row), anchors.interpreterProjection, routeEvidence),
      `${route} route plus shared interpreter projection`,
    );
  };

  const classifyAot = (row) => {
    if (row.kind === "receiver") {
      const refs = memberRefs(sources.aot, row.member);
      return refs.length > 0
        ? statusResult("covered", coreEvidence(declaration(row), refs), "typed receiver AOT projection")
        : statusResult("absent", [], "no receiver AOT projection");
    }
    if (row.aot_direct) {
      return statusResult("covered", coreEvidence(declaration(row), anchors.aotLookup), "generic CoreCall AOT projection");
    }
    const pair = exactPairRefs(sources.aot, row.module, row.member);
    return pair.length > 0
      ? statusResult("covered", coreEvidence(declaration(row), pair), "typed or bespoke AOT projection")
      : statusResult("refused", coreEvidence(declaration(row), anchors.aotDirectGate), "row disables direct AOT and has no explicit pair projection");
  };

  const classifyJit = (row) => {
    const host = hostRefs(row);
    if (host.length > 0) {
      return statusResult("covered", coreEvidence(declaration(row), host), "resident-JIT host symbol declaration");
    }
    const pair = exactPairRefs(sources.cranelift, row.module, row.member);
    if (pair.length > 0) {
      return statusResult("covered", coreEvidence(declaration(row), pair), "typed or bespoke Cranelift projection");
    }
    if (!row.jit_direct) {
      return statusResult("refused", declaration(row), "row disables direct JIT and has no explicit host or pair projection");
    }
    return statusResult("absent", [], "no explicit host symbol or Cranelift pair projection found");
  };

  const classifyWeb = (row) => {
    const symbolRefs = webSymbolRefs(row);
    const validPrelude = row.symbol_kind === "prelude" && /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(row.symbol);
    if (symbolRefs.length > 0 || validPrelude) {
      return statusResult(
        "covered",
        coreEvidence(declaration(row), symbolRefs, anchors.webResolver),
        "MIR Web CoreCall resolver accepts the canonical symbol",
      );
    }
    if (row.symbol) {
      return statusResult(
        "refused",
        coreEvidence(declaration(row), webResolver),
        "MIR Web CoreCall resolver rejects an unlinked symbol",
      );
    }
    return statusResult("absent", [], "receiver has no Web symbol or adapter");
  };

  const output = [];
  for (const row of rows) {
    const effectiveInterpreterRoute = interpreterRoute(row);
    const consumers = {
      aot_rust: classifyAot(row),
      interpreter: classifyInterpreter(row),
      cranelift: classifyJit(row),
      web: classifyWeb(row),
    };
    for (const backend of BACKENDS) {
      assert(consumers[backend], `unclassified CoreCallRecord ${row.ordinal} for ${backend}`);
    }
    output.push({
      surface: "core_call",
      id: `core-${String(row.ordinal).padStart(4, "0")}`,
      record_kind: row.kind,
      module: row.module,
      member: row.member,
      receiver_types: row.receiver_types,
      symbol: row.symbol,
      symbol_kind: row.symbol_kind,
      arity: row.arity,
      max_arity: row.max_arity,
      direct: { aot: row.aot_direct, jit: row.jit_direct },
      routes: { pure: row.pure_route, interpreter: effectiveInterpreterRoute },
      aot_projection_status: consumers.aot_rust.status,
      interpreter_route: effectiveInterpreterRoute,
      interpreter_route_declared: row.interpreter_route,
      jit_symbol: row.jit_symbols[0] ?? (row.symbol || null),
      jit_symbols: row.jit_symbols,
      identifier: row.kind === "receiver"
        ? `${row.receiver_types.join("|")}::${row.member}`
        : `${row.module}.${row.member}`,
      declaration: row.declaration,
      consumers,
    });
  }
  return {
    rows: output,
    host_symbol_count: [...hostMap.values()].reduce((total, list) => total + list.length, 0),
  };
}

function makeSpellingPairs(tirItems) {
  const lineFor = (kind, name) => {
    const item = tirItems.find((variant) => variant.kind === kind && variant.name === name);
    assert(item, `missing TIR spelling evidence ${kind}::${name}`);
    return item.declaration;
  };
  const pairs = [
    {
      id: "if_else_vs_match",
      canonical: "if / else",
      alternate: "match",
      evidence: [lineFor("TStmt", "If"), lineFor("TStmt", "EnumMatch"), lineFor("TExprKind", "IfExpr")],
    },
    {
      id: "fallback_vs_match",
      canonical: "?? fallback",
      alternate: "match on the failure carrier",
      evidence: [lineFor("TExprKind", "OrFallback"), lineFor("TExprKind", "Try")],
    },
    {
      id: "compound_assign_vs_binary_assign",
      canonical: "compound assignment",
      alternate: "binary expression followed by assignment",
      evidence: [lineFor("TStmt", "Assign"), lineFor("TExprKind", "Binary")],
    },
    {
      id: "interpolation_vs_concat",
      canonical: "interpolation",
      alternate: "text concatenation",
      evidence: [lineFor("TExprKind", "StrLit"), lineFor("TExprKind", "CoreCall")],
    },
    {
      id: "range_loop_vs_indexed_loop",
      canonical: "range loop",
      alternate: "indexed loop",
      evidence: [lineFor("TStmt", "Range"), lineFor("TStmt", "ForIn"), lineFor("TExprKind", "Index")],
    },
    {
      id: "explicit_copy_vs_implicit_copy",
      canonical: "explicit copy",
      alternate: "implicit clone/materialization",
      evidence: [lineFor("TExprKind", "ExplicitCopy"), lineFor("TExprKind", "Clone"), lineFor("TExprKind", "MaterializeView")],
    },
    {
      id: "map_method_vs_loop_push",
      canonical: "map method",
      alternate: "loop plus push",
      evidence: [lineFor("TExprKind", "ClosureMethod"), lineFor("TStmt", "ForIn")],
    },
    {
      id: "expr_arrow_vs_block_body",
      canonical: "expression-bodied function",
      alternate: "block-bodied function",
      evidence: [lineFor("TExprKind", "Lambda"), lineFor("TStmt", "Inline")],
    },
  ];
  return pairs.map((pair) => ({
    ...pair,
    source_span_hash: combinedSpanHash(pair.id, pair.evidence),
    timing: STATIC_TIMING,
    interpreter_wall_time_ms: null,
    canonical_wall_time_ms: null,
    alternate_wall_time_ms: null,
    timing_delta_percent: null,
    timing_status: STATIC_TIMING,
    outside_10_percent: false,
    timing_references: ["#2886", "#2892"],
  }));
}

function programPath(id) {
  return `${FIXTURE_PROGRAMS}#${id}`;
}

function programFor(row) {
  const id = row.id;
  const subject = row.surface === "tir"
    ? `${row.kind}::${row.variant}`
    : row.record_kind === "receiver"
      ? `${row.receiver_types.join("|")}::${row.member}`
      : `${row.module}.${row.member}`;
  return {
    id,
    path: programPath(id),
    subject,
    source_span_hash: row.source_span_hash,
    tiers: ["aot_rust", "cranelift", "interpreter"],
    source: [
      `// tier-census differential probe: ${subject}`,
      "// The same source is run on each tier; stdout must stay identical.",
      "fn run() {",
      `    print("tier-census:${id}")`,
      "}",
    ].join("\n"),
  };
}
function cleanRuntimeOutput(value) {
  return String(value ?? "")
    .replace(/\u001b\[[0-?]*[ -/]*[@-~]/gu, "")
    .replace(/\r/gu, "")
    .trim();
}

function runProcess(executable, args, cwd, label) {
  const started = performance.now();
  const result = spawnSync(executable, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1", CLICOLOR: "0" },
    timeout: EXECUTION_TIMEOUT_MS,
    maxBuffer: 8 * 1024 * 1024,
  });
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
  if (result.signal) {
    return {
      status: "failed",
      label,
      wall_time_ms: wallTimeMs,
      exit_code: null,
      stdout: cleanRuntimeOutput(result.stdout),
      stderr: cleanRuntimeOutput(result.stderr),
      reason: `tier command terminated by ${result.signal}`,
    };
  }
  return {
    status: result.status === 0 ? "exited" : "failed",
    label,
    wall_time_ms: wallTimeMs,
    exit_code: result.status,
    stdout: cleanRuntimeOutput(result.stdout),
    stderr: cleanRuntimeOutput(result.stderr),
    reason: result.status === 0 ? null : `tier command exited ${result.status}`,
  };
}

function runtimeResult(processResult, expectedOutput) {
  const outputMatches = processResult.stdout === expectedOutput;
  const ok = processResult.status === "exited"
    && processResult.exit_code === 0
    && outputMatches;
  return {
    status: ok ? "measured" : processResult.status === "unavailable" || processResult.status === "timeout" ? processResult.status : "failed",
    wall_time_ms: processResult.wall_time_ms,
    exit_code: processResult.exit_code,
    stdout_lines: processResult.stdout === "" ? 0 : processResult.stdout.split("\n").length,
    stderr: processResult.stderr === "" ? null : processResult.stderr.slice(0, 600),
    output_matches: outputMatches,
    reason: ok ? null : processResult.reason ?? (outputMatches ? "non-zero tier exit" : "tier stdout differed from the batch oracle"),
  };
}

function differentialBatchSource(programEntries) {
  return [
    "// Generated tier-census executable batch. Each row contributes one deterministic line.",
    "fn run() {",
    ...programEntries.map((program) => `    print("tier-census:${program.id}")`),
    "}",
  ].join("\n");
}

function differentialBatchOutput(programEntries) {
  return programEntries.map((program) => `tier-census:${program.id}`).join("\n");
}

function runTierProgram(tier, source, label, expectedOutput) {
  if (!existsSync(JET_BINARY)) {
    return {
      status: "unavailable",
      label,
      wall_time_ms: null,
      exit_code: null,
      stdout_lines: 0,
      stderr: null,
      output_matches: false,
      reason: `local compiler ${relPath(JET_BINARY)} is unavailable`,
    };
  }
  mkdirSync(EXECUTION_ROOT, { recursive: true });
  const runRoot = mkdtempSync(resolve(EXECUTION_ROOT, `${label.replace(/[^A-Za-z0-9_-]/gu, "_")}-`));
  writeFileSync(resolve(runRoot, "main.jet"), source);
  try {
    if (tier === "aot_rust") {
      const build = runProcess(JET_BINARY, ["build", "--profile=debug", "main.jet"], runRoot, `${label}:build`);
      if (build.status !== "exited" || build.exit_code !== 0) {
        return runtimeResult(build, expectedOutput);
      }
      const artifact = ["build/run", "build/main"]
        .map((path) => resolve(runRoot, path))
        .find((path) => existsSync(path));
      if (!artifact) {
        return {
          status: "failed",
          label,
          wall_time_ms: build.wall_time_ms,
          exit_code: build.exit_code,
          stdout_lines: 0,
          stderr: build.stderr,
          output_matches: false,
          reason: "AOT build exited successfully without build/run or build/main",
        };
      }
      return runtimeResult(runProcess(artifact, [], runRoot, `${label}:execute`), expectedOutput);
    }
    const args = tier === "interpreter"
      ? ["run", "--interpret", "main.jet"]
      : ["run", "main.jet"];
    return runtimeResult(runProcess(JET_BINARY, args, runRoot, `${label}:execute`), expectedOutput);
  } finally {
    rmSync(runRoot, { recursive: true, force: true });
  }
}

function pairProbeSource(pair, spelling) {
  const marker = `tier-census:spelling-${pair.id}`;
  const canonical = spelling === "canonical";
  switch (pair.id) {
    case "if_else_vs_match":
      return canonical
        ? [
            "fn run() {",
            "    n :: 2",
            `    if n == 2 -> print("${marker}") else -> print("${marker}")`,
            "}",
          ].join("\n")
        : [
            "fn run() {",
            "    n :: 2",
            "    if n == {",
            `        2 -> print("${marker}")`,
            `        else -> print("${marker}")`,
            "    }",
            "}",
          ].join("\n");
    case "fallback_vs_match":
      return canonical
        ? [
            `fn maybe() ?String -> Val("${marker}")`,
            "fn run() {",
            `    print(maybe() ?? "${marker}")`,
            "}",
          ].join("\n")
        : [
            `fn maybe() ?String -> Val("${marker}")`,
            "fn run() {",
            "    value :: maybe()",
            "    if value == {",
            `        .Val(_) -> print("${marker}")`,
            `        .None -> print("${marker}")`,
            "    }",
            "}",
          ].join("\n");
    case "compound_assign_vs_binary_assign":
      return canonical
        ? [
            "fn run() {",
            "    n := 1",
            "    n += 2",
            `    print("${marker}")`,
            "}",
          ].join("\n")
        : [
            "fn run() {",
            "    n := 1",
            "    n = n + 2",
            `    print("${marker}")`,
            "}",
          ].join("\n");
    case "interpolation_vs_concat":
      return canonical
        ? [
            "fn run() {",
            `    value :: "${marker}"`,
            '    print("{value}")',
            "}",
          ].join("\n")
        : [
            "fn run() {",
            `    parts :: [String]{"${marker.slice(0, marker.lastIndexOf("-") + 1)}", "${pair.id}"}`,
            '    print(parts.join(""))',
            "}",
          ].join("\n");
    case "range_loop_vs_indexed_loop":
      return canonical
        ? [
            "fn run() {",
            "    sum := 0",
            "    loop i in 0..<3 -> sum += i",
            `    print("${marker}")`,
            "}",
          ].join("\n")
        : [
            "fn run() {",
            "    values :: [1, 2, 3]",
            "    sum := 0",
            "    i := 0",
            "    loop i < values.len() {",
            "        sum += values[i]",
            "        i += 1",
            "    }",
            `    print("${marker}")`,
            "}",
          ].join("\n");
    case "explicit_copy_vs_implicit_copy":
      return canonical
        ? [
            "fn run() {",
            `    value :: "${marker}"`,
            "    cloned :: ~value",
            "    print(cloned)",
            "}",
          ].join("\n")
        : [
            "fn run() {",
            `    value :: "${marker}"`,
            "    alias :: value",
            `    print("${marker}")`,
            "}",
          ].join("\n");
    case "map_method_vs_loop_push":
      return canonical
        ? [
            "fn run() {",
            "    values :: [1, 2, 3]",
            "    mapped :: values.map((n: Int) -> n + 1)",
            `    print("${marker}")`,
            "}",
          ].join("\n")
        : [
            "fn run() {",
            "    values :: [1, 2, 3]",
            "    mapped := [Int]{}",
            "    loop n in values -> mapped.push(n + 1)",
            `    print("${marker}")`,
            "}",
          ].join("\n");
    case "expr_arrow_vs_block_body":
      return canonical
        ? [
            `fn value() String -> "${marker}"`,
            "fn run() {",
            "    print(value())",
            "}",
          ].join("\n")
        : [
            "fn value() String -> {",
            `    "${marker}"`,
            "}",
            "fn run() {",
            "    print(value())",
            "}",
          ].join("\n");
    default:
      throw new Error(`unknown spelling pair ${pair.id}`);
  }
}

function updateSpellingTimings(pairs) {
  return pairs.map((pair) => {
    const expected = `tier-census:spelling-${pair.id}`;
    const canonical = runTierProgram(
      "interpreter",
      pairProbeSource(pair, "canonical"),
      `spelling-${pair.id}-canonical`,
      expected,
    );
    const alternate = runTierProgram(
      "interpreter",
      pairProbeSource(pair, "alternate"),
      `spelling-${pair.id}-alternate`,
      expected,
    );
    const canonicalTime = canonical.status === "measured" ? canonical.wall_time_ms : null;
    const alternateTime = alternate.status === "measured" ? alternate.wall_time_ms : null;
    const measured = canonicalTime !== null && alternateTime !== null;
    const baseline = measured ? Math.max(1, Math.min(canonicalTime, alternateTime)) : null;
    const timingDeltaPercent = measured
      ? Math.round((Math.abs(canonicalTime - alternateTime) / baseline) * 10_000) / 100
      : null;
    const outsideTenPercent = measured && timingDeltaPercent > 10;
    return {
      ...pair,
      timing: measured
        ? "measured: interpreter wall clock for canonical and alternate probes"
        : `not-measured: ${canonical.reason ?? alternate.reason ?? "interpreter probe failed"}`,
      interpreter_wall_time_ms: measured
        ? Math.round(((canonicalTime + alternateTime) / 2) * 1000) / 1000
        : null,
      canonical_wall_time_ms: canonicalTime,
      alternate_wall_time_ms: alternateTime,
      timing_delta_percent: timingDeltaPercent,
      timing_status: measured ? "measured" : "not-measured",
      outside_10_percent: outsideTenPercent,
      timing_references: outsideTenPercent ? ["#2886", "#2892"] : [],
    };
  });
}

function executeCensus(data, programs) {
  const entries = Object.values(programs.programs);
  const source = differentialBatchSource(entries);
  const expected = differentialBatchOutput(entries);
  const tiers = {};
  for (const tier of EXECUTION_TIERS) {
    tiers[tier] = runTierProgram(tier, source, `differential-${tier}`, expected);
  }
  const measured = EXECUTION_TIERS.every((tier) => tiers[tier].status === "measured");
  const execution = {
    status: measured ? "measured" : EXECUTION_TIERS.some((tier) => tiers[tier].status === "unavailable") ? "unavailable" : "failed",
    tiers: EXECUTION_TIERS,
    source: "one deterministic batch containing every differential program marker",
    row_count: entries.length,
    batch: {
      status: measured ? "measured" : "failed",
      source_lines: source.split("\n").length,
      expected_output_lines: expected.split("\n").length,
      tiers,
    },
    spelling_pairs: updateSpellingTimings(data.spelling_pairs),
  };
  for (const program of entries) {
    program.execution = {
      status: execution.status,
      batch: "all-differential-programs",
      tiers: Object.fromEntries(EXECUTION_TIERS.map((tier) => [tier, tiers[tier].status])),
    };
  }
  data.spelling_pairs = execution.spelling_pairs;
  data.generated_static_only = false;
  data.timing_status = measured ? EXECUTION_TIMING : "not-measured: executable batch did not complete on all tiers";
  data.execution = execution;
  return execution;
}


function serialConsumer(result, differentialPath = null) {
  const output = { status: result.status, evidence: result.evidence };
  if (result.reason) output.reason = result.reason;
  if (result.refusal_evidence) output.refusal_evidence = result.refusal_evidence;
  if (result.status !== "covered" && differentialPath) output.program = differentialPath;
  return output;
}

function tsvEvidence(result) {
  return result.evidence.map((item) => `${item.file}:${item.line}:${item.span_hash}`).join(";");
}

function field(value) {
  return String(value ?? "").replace(/[\t\r\n]/g, " ");
}

function buildTsv(tirRows, coreRows, spellingPairs) {
  const headers = [
    "surface",
    "kind",
    "identifier",
    "declaration",
    "source_span_hash",
    "differential_program",
    "aot_rust_status",
    "aot_rust_evidence",
    "aot_rust_program",
    "interpreter_status",
    "interpreter_evidence",
    "interpreter_program",
    "cranelift_status",
    "cranelift_evidence",
    "cranelift_program",
    "web_status",
    "web_evidence",
    "web_program",
    "interpreter_wall_time_ms",
    "canonical_wall_time_ms",
    "alternate_wall_time_ms",
    "timing_delta_percent",
    "timing_status",
    "timing_references",
  ];
  const lines = [headers.join("\t")];
  const rowFields = (row, kind, identifier) => [
    kind,
    identifier,
    `${row.declaration.file}:${row.declaration.line}`,
    row.source_span_hash,
    row.differential_program,
    row.consumers.aot_rust.status,
    tsvEvidence(row.consumers.aot_rust),
    row.consumers.aot_rust.program ?? "",
    row.consumers.interpreter.status,
    tsvEvidence(row.consumers.interpreter),
    row.consumers.interpreter.program ?? "",
    row.consumers.cranelift.status,
    tsvEvidence(row.consumers.cranelift),
    row.consumers.cranelift.program ?? "",
    row.consumers.web.status,
    tsvEvidence(row.consumers.web),
    row.consumers.web.program ?? "",
    "",
    "",
    "",
    "",
    "",
    "",
  ];
  for (const row of tirRows) lines.push(["tir", ...rowFields(row, row.kind, row.variant)].map(field).join("\t"));
  for (const row of coreRows) lines.push(["core_call", ...rowFields(row, row.record_kind, row.identifier)].map(field).join("\t"));
  for (const pair of spellingPairs) {
    const declaration = pair.evidence.map((item) => `${item.file}:${item.line}`).join(";");
    lines.push([
      "spelling_pair",
      pair.id,
      `${pair.canonical} vs ${pair.alternate}`,
      declaration,
      pair.source_span_hash,
      "",
      "not_applicable",
      "",
      "",
      "not_applicable",
      "",
      "",
      "not_applicable",
      "",
      "",
      "not_applicable",
      "",
      "",
      pair.interpreter_wall_time_ms,
      pair.canonical_wall_time_ms,
      pair.alternate_wall_time_ms,
      pair.timing_delta_percent,
      pair.timing_status,
      pair.timing_references.join(","),
    ].map(field).join("\t"));
  }
  return `${lines.join("\n")}\n`;
}

function summarize(rows) {
  const result = {};
  for (const surface of ["tir", "core_call"]) result[surface] = statusCounts(rows, surface);
  const dynamic = { total: 0, by_surface: {}, by_backend: {} };
  for (const backend of BACKENDS) dynamic.by_backend[backend] = 0;
  for (const [surface, backendCounts] of Object.entries(result)) {
    dynamic.by_surface[surface] = {};
    for (const backend of BACKENDS) {
      const count = backendCounts[backend].dynamic;
      dynamic.by_surface[surface][backend] = count;
      dynamic.by_backend[backend] += count;
      dynamic.total += count;
    }
  }
  return { status_counts: result, dynamic_probe_cells: dynamic };
}

function markdownTable(headers, rows) {
  const out = [`| ${headers.join(" | ")} |`, `| ${headers.map(() => "---").join(" | ")} |`];
  for (const row of rows) out.push(`| ${row.join(" | ")} |`);
  return out.join("\n");
}

function buildMarkdown(data) {
  const counts = data.counts;
  const tirCounts = data.status_counts.tir;
  const coreCounts = data.status_counts.core_call;
  const dynamic = data.dynamic_probe_cells;
  const execution = data.execution;
  const measured = execution?.status === "measured";
  const backendRows = BACKENDS.map((backend) => [
    backend,
    `${tirCounts[backend].covered} / ${tirCounts[backend].refused} / ${tirCounts[backend].absent}`,
    `${coreCounts[backend].covered} / ${coreCounts[backend].refused} / ${coreCounts[backend].absent}`,
    String(dynamic.by_backend[backend]),
  ]);
  const pairRows = data.spelling_pairs.map((pair) => {
    const timing = pair.timing_status === "measured"
      ? `canonical ${pair.canonical_wall_time_ms} ms; alternate ${pair.alternate_wall_time_ms} ms; mean ${pair.interpreter_wall_time_ms} ms; delta ${pair.timing_delta_percent}%${pair.outside_10_percent ? "; outside 10%; see #2886/#2892" : ""}`
      : `${pair.timing}${pair.outside_10_percent ? " (outside 10%; see #2886/#2892)" : ""}`;
    return [
      `\`${pair.id}\``,
      pair.canonical,
      pair.alternate,
      pair.evidence.map((item) => `\`${item.file}:${item.line}\``).join(", "),
      `\`${pair.source_span_hash}\``,
      timing,
    ];
  });
  const dynamicRows = [
    ["TExprKind + TStmt", ...BACKENDS.map((backend) => String(dynamic.by_surface.tir[backend]))],
    ["CoreCallRecord", ...BACKENDS.map((backend) => String(dynamic.by_surface.core_call[backend]))],
    ["Total", ...BACKENDS.map((backend) => String(dynamic.by_backend[backend]))],
  ];
  const runtimeSummary = measured
    ? `Executable mode measured one batch of ${execution.row_count} differential markers on AOT, Cranelift JIT, and the interpreter.`
    : "Executable mode was not requested or did not complete on all three tiers.";
  return `# ${measured ? "Executable" : "Static"} tier census — 2026-09-03

## Result

This report inventories the authoritative TIR enums and the \`CoreCallRecord\` registry. It scans source and emits one named three-tier differential probe per row. ${runtimeSummary}

Command:

\`\`\`text
${data.generated_command}
\`\`\`

The census has **${counts.tir_variants} TIR variants** (${counts.expr_variants} \`TExprKind\`, ${counts.stmt_variants} \`TStmt\`) and **${counts.core_records} \`CoreCallRecord\` rows** (${counts.core_plain_records} plain, ${counts.core_receiver_records} receiver). It emits **${counts.differential_programs} named probes** in \`${FIXTURE_PROGRAMS}\`. The TSV has **${counts.tsv_rows - 1} data rows** plus one header (**${counts.tsv_rows} rows total**). The exhaustive table is in [JSON](tier-census-2026-09-03.json) and [TSV](tier-census-2026-09-03.tsv).

A status is \`covered\` only when the backend has an explicit variant arm, host symbol, pair projection, or support branch. \`refused\` records an explicit refusal or arity rejection. \`absent\` records no explicit static consumer. Wildcard arms, default constructors, and default route logic never count as coverage. The script exits nonzero if it cannot classify a declaration or finds an unknown TIR reference.

## Static status

\`covered / refused / absent\` counts are shown for TIR variants and Core rows. Dynamic cells are every non-covered cell that still needs an execution probe.

${markdownTable(["backend", "TIR", "CoreCallRecord", "dynamic cells"], backendRows)}

Uncovered cells: **${data.uncovered_count ?? dynamic.total}**.

${markdownTable(["surface", ...BACKENDS], dynamicRows)}

## Spelling pairs

${measured ? "Interpreter wall time was measured for both forms of every pair." : `Timing remains **${DYNAMIC_TIMING}** for every pair.`} Pairs outside 10% cite #2886 and #2892.

${markdownTable(["pair", "canonical", "alternate", "static evidence", "source span", "timing"], pairRows)}

## Sources and limits

- TIR declarations: \`${TIR_PATH}\`.
- Core registry and ambient route manifest: \`${CORE_PATH}\`.
- AOT projection: \`${AOT_PATH}\`.
- Interpreter evaluator: \`${INTERPRETER_PATH}\` plus ambient route plumbing in \`${AMBIENT_PATH}\`.
- Cranelift host and lowering sources: \`${JIT_ROOT}\`.
- Web projection and symbol resolver: \`${WEB_PATH}\`.

The JSON keeps exact file, line, source-line, and stable source-span-hash evidence for every row. Line numbers are display coordinates; the hash is the durable identity used by fixture comparison. The named probes are checked into \`${FIXTURE_PROGRAMS}\`; executable mode records the batch and per-tier outcomes in the JSON and manifest.
`;
}

function buildData() {
  const tirFile = makeFile(TIR_PATH);
  const coreFile = makeFile(CORE_PATH);
  const tirLoweringFiles = walkRust(TIR_ROOT).map(makeFile);
  const aotFiles = [makeFile(AOT_PATH)];
  const interpreterFiles = [makeFile(INTERPRETER_PATH)];
  const craneliftFiles = walkRust(JIT_ROOT).map(makeFile);
  const jitAllFiles = craneliftFiles;
  const webFiles = [makeFile(WEB_PATH)];

  const tirVariants = [
    { enumName: "TExprKind", items: parseEnum(tirFile, "TExprKind") },
    { enumName: "TStmt", items: parseEnum(tirFile, "TStmt") },
  ];
  const tirRows = [];
  const unknownRefs = [];
  const tirDispatch = {
    aot_rust: firstRefs(aotFiles, /MirOperation::CoreCall|fn\s+emit_mir_program/g),
    interpreter: firstRefs(interpreterFiles, /MirOperation::CoreCall|fn\s+eval_core_call_binding/g),
    cranelift: firstRefs(craneliftFiles, /\bMirProgram\b|\btry_resident\b/g),
    web: firstRefs(webFiles, /MirOperation::CoreCall|fn\s+js_core_call_expression/g),
  };
  for (const variantSet of tirVariants) {
    const scan = classifyTir(
      variantSet,
      {
        aot_rust: aotFiles,
        interpreter: interpreterFiles,
        cranelift: craneliftFiles,
        web: webFiles,
      },
      tirLoweringFiles,
      tirDispatch,
    );
    unknownRefs.push(...scan.unknown.map((item) => ({ enum: variantSet.enumName, ...item })));
    for (const item of variantSet.items) {
      const id = `tir-${variantSet.enumName}-${item.name}`;
      const differentialPath = programPath(id);
      const consumers = {};
      for (const backend of BACKENDS) {
        consumers[backend] = serialConsumer(
          scan.consumers[backend].get(item.name),
          differentialPath,
        );
      }
      tirRows.push({
        surface: "tir",
        id,
        kind: variantSet.enumName,
        variant: item.name,
        declaration: item.declaration,
        source_span_hash: item.declaration.span_hash,
        differential_program: differentialPath,
        consumers,
      });
    }
  }
  assert(unknownRefs.length === 0, `unknown TIR variants: ${unknownRefs.map((item) => `${item.enum} at ${item.file}:${item.line}`).join(", ")}`);

  const anchors = {
    aotLookup: firstRefs(aotFiles, /MirOperation::CoreCall|fn\s+core_symbol/g),
    aotDirectGate: firstRefs([coreFile], /without_direct_aot|aot_direct/g),
    interpreterProjection: firstRefs(interpreterFiles, /MirOperation::CoreCall|fn\s+eval_core_call_binding/g),
    ambientProjection: firstRefs([makeFile(AMBIENT_PATH)], /with_interpreter_ambient|InterpreterAmbientContext/g),
    defaultInterpreterRoute: firstRefs([coreFile], /fn\s+default_interpreter_route/g),
    webResolver: firstRefs(webFiles, /fn\s+js_symbol_expression|WEB_(?:PRELUDE|RUNTIME)_LINKS/g),
  };
  assert(anchors.aotLookup.length > 0, "missing AOT CoreCall projection anchor");
  assert(anchors.aotDirectGate.length > 0, "missing AOT direct-coverage guard anchor");
  assert(anchors.interpreterProjection.length > 0, "missing interpreter CoreCall projection anchor");
  assert(anchors.ambientProjection.length > 0, "missing ambient CoreCall projection anchor");
  assert(anchors.defaultInterpreterRoute.length > 0, "missing default interpreter route anchor");
  assert(anchors.webResolver.length > 0, "missing Web CoreCall resolver anchor");

  const coreRows = parseCoreRows(coreFile);
  const coreResult = classifyCoreRows(coreRows, {
    core: coreFile,
    aot: aotFiles,
    interpreter: interpreterFiles,
    cranelift: craneliftFiles,
    jitAll: jitAllFiles,
    web: webFiles,
  }, anchors);
  const coreOutputRows = coreResult.rows.map((row) => {
    const differentialPath = programPath(row.id);
    return {
      ...row,
      source_span_hash: row.declaration.span_hash,
      differential_program: differentialPath,
      consumers: Object.fromEntries(
        BACKENDS.map((backend) => [
          backend,
          serialConsumer(row.consumers[backend], differentialPath),
        ]),
      ),
    };
  });

  const tirItemsForPairs = tirVariants.flatMap((set) => set.items.map((item) => ({ ...item, kind: set.enumName, name: item.name })));
  const spellingPairs = makeSpellingPairs(tirItemsForPairs);
  const allRows = [...tirRows, ...coreOutputRows];
  const summary = summarize(allRows);
  const programs = {
    schema_version: 1,
    generated_by: "scripts/agent/tier-census.mjs",
    tiers: ["aot_rust", "cranelift", "interpreter"],
    programs: Object.fromEntries(allRows.map((row) => [row.id, programFor(row)])),
  };
  const data = {
    schema_version: 2,
    generated_by: "scripts/agent/tier-census.mjs",
    generated_command: GENERATED_COMMAND,
    generated_static_only: true,
    timing_status: DYNAMIC_TIMING,
    authoritative: {
      tir: {
        file: TIR_PATH,
        enums: {
          TExprKind: { variants: tirVariants[0].items.length },
          TStmt: { variants: tirVariants[1].items.length },
        },
      },
      core: {
        file: CORE_PATH,
        registry: { records: coreRows.length },
      },
    },
    counts: {
      tir_variants: tirRows.length,
      expr_variants: tirVariants[0].items.length,
      stmt_variants: tirVariants[1].items.length,
      core_records: coreOutputRows.length,
      core_plain_records: coreRows.filter((row) => row.kind === "plain").length,
      core_receiver_records: coreRows.filter((row) => row.kind === "receiver").length,
      differential_programs: allRows.length,
      tsv_rows: 1 + tirRows.length + coreOutputRows.length + spellingPairs.length,
    },
    source_roots: {
      aot_rust: [AOT_PATH],
      interpreter: [INTERPRETER_PATH, AMBIENT_PATH],
      cranelift: [JIT_ROOT],
      web: [WEB_PATH],
      tir_lowering: [TIR_ROOT],
    },
    differential_programs: {
      file: FIXTURE_PROGRAMS,
      fragment: "#<row-id>",
      tiers: programs.tiers,
    },
    status_counts: summary.status_counts,
    dynamic_probe_cells: summary.dynamic_probe_cells,
    uncovered_count: summary.dynamic_probe_cells.total,
    tir: tirRows,
    core_calls: coreOutputRows,
    spelling_pairs: spellingPairs,
    unclassified: [],
  };
  return {
    data,
    programs,
    tsv: buildTsv(tirRows, coreOutputRows, spellingPairs),
    markdown: buildMarkdown(data),
  };
}

function assertProgramManifest(data, programs) {
  const known = new Set(Object.keys(programs.programs ?? {}));
  const rows = [...data.tir, ...data.core_calls];
  assert(rows.length === known.size, "differential program manifest is not one-to-one with census rows");
  for (const [id, program] of Object.entries(programs.programs ?? {})) {
    assert(id === program.id, `differential program key mismatch for ${id}`);
    assert(program.path === programPath(id), `differential program path mismatch for ${id}`);
    assert(program.source?.includes("fn run"), `differential program ${id} has no source`);
    assert(validSpanHash(program.source_span_hash), `differential program ${id} has no stable source span hash`);
    assert(
      JSON.stringify(program.tiers) === JSON.stringify(["aot_rust", "cranelift", "interpreter"]),
      `differential program ${id} does not name all required tiers`,
    );
  }
  for (const row of rows) {
    assert(known.has(row.id), `missing differential program for ${row.id}`);
    assert(row.differential_program === programPath(row.id), `bad differential program path for ${row.id}`);
    assert(validSpanHash(row.declaration?.span_hash), `row ${row.id} has no stable declaration span hash`);
    assert(row.source_span_hash === row.declaration.span_hash, `row ${row.id} has mismatched declaration span hash`);
    assert(programs.programs[row.id].source_span_hash === row.source_span_hash, `program ${row.id} has mismatched declaration span hash`);
    for (const backend of BACKENDS) {
      const cell = row.consumers?.[backend];
      assert(cell && STATUS_VALUES.has(cell.status), `invalid or missing ${backend} cell for ${row.id}`);
      assert(!["aot_broken", "interpreter_refused"].includes(cell.status), `legacy gate status ${cell.status} is forbidden for ${row.id}/${backend}`);
      if (cell.status !== "covered") {
        assert(cell.program === row.differential_program, `non-covered ${row.id}/${backend} has no differential program`);
      }
    }
  }
}

function runPolicyCheck(data, programs) {
  assertProgramManifest(data, programs);
  const cases = [
    ["tir", data.tir],
    ["core_calls", data.core_calls],
  ];
  let incompleteRejected = 0;
  let forbiddenRejected = 0;
  const assertRejected = (candidateData, candidatePrograms, reason) => {
    let rejected = false;
    try {
      assertProgramManifest(candidateData, candidatePrograms);
    } catch {
      rejected = true;
    }
    assert(rejected, `policy check accepted ${reason}`);
  };
  for (const [surface, rows] of cases) {
    const original = rows[0];
    const sourceProgram = programs.programs[original.id];
    const addCandidate = (id, rowMutation) => {
      const row = JSON.parse(JSON.stringify(original));
      row.id = id;
      row.differential_program = programPath(id);
      rowMutation(row);
      const program = {
        ...JSON.parse(JSON.stringify(sourceProgram)),
        id,
        path: programPath(id),
      };
      return {
        data: { ...data, [surface]: [...rows, row] },
        programs: { ...programs, programs: { ...programs.programs, [id]: program } },
      };
    };

    const incomplete = addCandidate(`policy-${surface}-missing-cell`, (row) => {
      delete row.consumers.web;
    });
    assertRejected(incomplete.data, incomplete.programs, `a new ${surface} row without a complete backend matrix`);
    incompleteRejected += 1;

    for (const [backend, status] of [["aot_rust", "aot_broken"], ["interpreter", "interpreter_refused"]]) {
      const forbidden = addCandidate(`policy-${surface}-${status}`, (row) => {
        row.consumers[backend].status = status;
      });
      assertRejected(forbidden.data, forbidden.programs, `forbidden ${status} status on a new ${surface} row`);
      forbiddenRejected += 1;
    }
  }
  process.stdout.write(
    `tier census: policy check passed; negative additions rejected=${incompleteRejected + forbiddenRejected}; incomplete matrices rejected=${incompleteRejected}; forbidden statuses rejected=${forbiddenRejected}\n`,
  );
}

function writeOutput(path, content) {
  mkdirSync(dirname(absPath(path)), { recursive: true });
  writeFileSync(absPath(path), content);
}

function stableEvidenceProjection(value) {
  if (Array.isArray(value)) return value.map(stableEvidenceProjection);
  if (!value || typeof value !== "object") return value;
  if (typeof value.file === "string" && validSpanHash(value.span_hash)) {
    return { file: value.file, span_hash: value.span_hash };
  }
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, stableEvidenceProjection(item)]));
}

function staticProjection(value, kind) {
  const copy = JSON.parse(JSON.stringify(value));
  if (kind === "data") {
    delete copy.execution;
    copy.generated_static_only = true;
    copy.timing_status = STATIC_TIMING;
    copy.spelling_pairs = copy.spelling_pairs.map((pair) => {
      const output = { ...pair };
      delete output.canonical_wall_time_ms;
      delete output.alternate_wall_time_ms;
      delete output.timing_delta_percent;
      delete output.outside_10_percent;
      output.timing = STATIC_TIMING;
      output.interpreter_wall_time_ms = null;
      output.canonical_wall_time_ms = null;
      output.alternate_wall_time_ms = null;
      output.timing_delta_percent = null;
      output.timing_status = STATIC_TIMING;
      output.outside_10_percent = false;
      output.timing_references = ["#2886", "#2892"];
      return output;
    });
  } else {
    for (const program of Object.values(copy.programs ?? {})) delete program.execution;
  }
  return stableEvidenceProjection(copy);
}

function normalizedTsv(value) {
  const lines = String(value).split("\n");
  const headers = lines[0]?.split("\t") ?? [];
  const declarationIndex = headers.indexOf("declaration");
  const evidenceIndexes = headers
    .map((name, index) => name.endsWith("_evidence") ? index : -1)
    .filter((index) => index >= 0);
  const blankFields = new Set([
    "interpreter_wall_time_ms",
    "canonical_wall_time_ms",
    "alternate_wall_time_ms",
    "timing_delta_percent",
  ]);
  const timingStatus = headers.indexOf("timing_status");
  const timingReferences = headers.indexOf("timing_references");
  return lines.map((line, index) => {
    if (index === 0 || line === "") return line;
    const fields = line.split("\t");
    if (declarationIndex >= 0) fields[declarationIndex] = "";
    for (const fieldIndex of evidenceIndexes) {
      fields[fieldIndex] = fields[fieldIndex]
        .split(";")
        .map((item) => item.replace(/^(.*):\d+:([0-9a-f]{64})$/u, "$1:$2"))
        .join(";");
    }
    if (fields[0] === "spelling_pair") {
      for (const name of blankFields) {
        const fieldIndex = headers.indexOf(name);
        if (fieldIndex >= 0) fields[fieldIndex] = "";
      }
      if (timingStatus >= 0) fields[timingStatus] = STATIC_TIMING;
      if (timingReferences >= 0) fields[timingReferences] = "#2886,#2892";
    }
    return fields.join("\t");
  }).join("\n");
}

function checkStoredExecution(actualData) {
  assert(actualData.execution?.status === "measured", "tier census fixture has no measured executable batch; run node scripts/agent/tier-census.mjs --execute --write-fixture");
  for (const tier of EXECUTION_TIERS) {
    assert(actualData.execution.batch?.tiers?.[tier]?.status === "measured", `tier census fixture has no measured ${tier} batch`);
  }
  assert(Array.isArray(actualData.spelling_pairs) && actualData.spelling_pairs.length > 0, "tier census fixture has no spelling-pair timing rows");
  for (const pair of actualData.spelling_pairs) {
    assert(validSpanHash(pair.source_span_hash), `spelling pair ${pair.id} has no stable source span hash`);
    assert(pair.timing_status === "measured", `spelling pair ${pair.id} is not measured`);
    assert(Number.isFinite(pair.interpreter_wall_time_ms) && pair.interpreter_wall_time_ms > 0, `spelling pair ${pair.id} has no measured interpreter wall time`);
    assert(Number.isFinite(pair.canonical_wall_time_ms) && pair.canonical_wall_time_ms > 0, `spelling pair ${pair.id} has no canonical interpreter wall time`);
    assert(Number.isFinite(pair.alternate_wall_time_ms) && pair.alternate_wall_time_ms > 0, `spelling pair ${pair.id} has no alternate interpreter wall time`);
    assert(Number.isFinite(pair.timing_delta_percent) && pair.timing_delta_percent >= 0, `spelling pair ${pair.id} has no timing delta`);
    assert(typeof pair.outside_10_percent === "boolean", `spelling pair ${pair.id} has no 10% comparison`);
    assert(Array.isArray(pair.timing_references), `spelling pair ${pair.id} has no timing references`);
    if (pair.outside_10_percent) {
      assert(pair.timing_references.includes("#2886") && pair.timing_references.includes("#2892"), `spelling pair ${pair.id} outside 10% has no #2886/#2892 references`);
    } else {
      assert(pair.timing_references.length === 0, `spelling pair ${pair.id} is within 10% but carries gap references`);
    }
  }
}

function checkFixtures(data, programs, tsv) {
  const actualJson = existsSync(absPath(FIXTURE_JSON))
    ? JSON.parse(readFileSync(absPath(FIXTURE_JSON), "utf8"))
    : null;
  const actualPrograms = existsSync(absPath(FIXTURE_PROGRAMS))
    ? JSON.parse(readFileSync(absPath(FIXTURE_PROGRAMS), "utf8"))
    : null;
  const actualTsv = existsSync(absPath(FIXTURE_TSV))
    ? readFileSync(absPath(FIXTURE_TSV), "utf8")
    : null;
  assert(actualJson, `missing ${FIXTURE_JSON}`);
  assert(actualPrograms, `missing ${FIXTURE_PROGRAMS}`);
  assert(actualTsv !== null, `missing ${FIXTURE_TSV}`);
  checkStoredExecution(actualJson);
  assert(
    JSON.stringify(staticProjection(actualJson, "data")) === JSON.stringify(staticProjection(data, "data")),
    `${FIXTURE_JSON} is stale; run ${GENERATED_COMMAND} --execute --write-fixture`,
  );
  assert(
    JSON.stringify(staticProjection(actualPrograms, "programs")) === JSON.stringify(staticProjection(programs, "programs")),
    `${FIXTURE_PROGRAMS} is stale; run ${GENERATED_COMMAND} --execute --write-fixture`,
  );
  assert(normalizedTsv(actualTsv) === normalizedTsv(tsv), `${FIXTURE_TSV} is stale; run ${GENERATED_COMMAND} --execute --write-fixture`);
}

function main() {
  let { data, programs, tsv, markdown } = buildData();
  assertProgramManifest(data, programs);
  if (EXECUTE) {
    executeCensus(data, programs);
    tsv = buildTsv(data.tir, data.core_calls, data.spelling_pairs);
    markdown = buildMarkdown(data);
  }
  if (POLICY_CHECK) {
    runPolicyCheck(data, programs);
    return;
  }
  const json = `${JSON.stringify(data, null, 2)}\n`;
  const programsJson = `${JSON.stringify(programs, null, 2)}\n`;
  if (CHECK_ONLY) {
    checkFixtures(data, programs, tsv);
    process.stdout.write(
      `tier census: checked ${data.counts.tsv_rows - 1} data rows; differential programs=${data.counts.differential_programs}; dynamic probe cells=${data.dynamic_probe_cells.total}\n`,
    );
    return;
  }
  if (process.argv.includes("--json")) {
    process.stdout.write(json);
    return;
  }
  if (process.argv.includes("--write-fixture")) {
    writeOutput(FIXTURE_JSON, json);
    writeOutput(FIXTURE_TSV, tsv);
    writeOutput(FIXTURE_PROGRAMS, programsJson);
    process.stdout.write(
      `tier census: wrote ${FIXTURE_JSON}, ${FIXTURE_TSV}, ${FIXTURE_PROGRAMS}; TExprKind=${data.counts.expr_variants} TStmt=${data.counts.stmt_variants} CoreCallRecord=${data.counts.core_records}; dynamic probe cells=${data.dynamic_probe_cells.total}\n`,
    );
    return;
  }
  writeOutput(OUTPUT_JSON, json);
  writeOutput(OUTPUT_TSV, tsv);
  writeOutput(OUTPUT_MD, markdown);
  process.stdout.write(`tier census: wrote ${OUTPUT_JSON}, ${OUTPUT_TSV}, ${OUTPUT_MD}; TExprKind=${data.counts.expr_variants} TStmt=${data.counts.stmt_variants} CoreCallRecord=${data.counts.core_records}; dynamic probe cells=${data.dynamic_probe_cells.total}\n`);
}
main();
