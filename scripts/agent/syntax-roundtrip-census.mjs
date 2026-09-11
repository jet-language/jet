#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { readLedger } from "./lexical-ledger.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DATE = "2026-09-03";
const SYNTAX_ROOT = "crates/jet-foundation/src/Syntax.rs";
const SYNTAX_MODULE_DIR = "crates/jet-foundation/src/Syntax";
const AST_ROOTS = [
  { category: "stmt", enum_name: "Stmt", path: "crates/jet-foundation/src/AST/statements.rs" },
  { category: "expr", enum_name: "Expr", path: "crates/jet-foundation/src/AST/expressions.rs" },
];
const FIXTURE_ROOT = "scripts/agent/syntax-roundtrip-fixtures";
const FIXTURE_MATRIX_JSON = `${FIXTURE_ROOT}/matrix.json`;
const FIXTURE_MATRIX_TSV = `${FIXTURE_ROOT}/matrix.tsv`;
const FIXTURE_MATRIX_MD = `${FIXTURE_ROOT}/matrix.md`;
const REGISTERED_SURFACES = ["jet_fmt", "language_server", "canvas"];
const DYNAMIC_TIMEOUT_MS = 8_000;
const DEFAULT_SCRATCH = `${process.env.HOME ?? "~"}/.cache/jet-test-scratch`;
const AST_FIXTURE_ROOT = `${FIXTURE_ROOT}/ast`;
const OUTPUT_JSON = `docs/audits/syntax-roundtrip-census-${DATE}.json`;
const OUTPUT_TSV = `docs/audits/syntax-roundtrip-census-${DATE}.tsv`;
const OUTPUT_MD = `docs/audits/syntax-roundtrip-census-${DATE}.md`;
const GENERATED_COMMAND = "node scripts/agent/syntax-roundtrip-census.mjs --write";

const DECISION_RE = /\b(?:D-[A-Za-z0-9][A-Za-z0-9-]*|S\d+(?:-[A-Za-z0-9-]+)?)\b/g;
const COMMENT_RE = /^\s*\/\/(?:!|\/)?\s?(.*)$/;
const DECL_RE = /\b(?:const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:/;
const STATUS_VALUES = new Set(["covered", "missing", "not_applicable"]);
const COVERAGE_COLUMNS = [
  "parser",
  "formatter_emission",
  "formatter_roundtrip",
  "linter_recognition",
  "linter_teaching",
  "language_server",
  "canvas",
];

const MAX_EVIDENCE = 6;
const NON_SYNTAX_LITERAL_TABLES = new Set(["CORE_CALLS", "RETIREMENTS"]);
function fail(message) {
  throw new Error(`syntax roundtrip census: ${message}`);
}

function assert(condition, message) {
  if (!condition) fail(message);
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
function lineNumberAt(source, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (source[index] === "\n") line += 1;
  }
  return line;
}

function registryDecisionIds(decision) {
  return [...new Set(String(decision ?? "")
    .split(/[;,]/)
    .map((value) => value.trim())
    .filter(Boolean)
    .flatMap((value) => value.match(DECISION_RE) ?? []))].sort();
}

// Syntax::LEXICAL_LEDGER is the only registry this census enumerates.  The
// parser is deliberately delegated to lexical-ledger.mjs; this function only
// attaches source coordinates and stable per-row identities.  No spelling list
// is repeated here or in the fixtures.
function enumerateRegisteredForms() {
  const source = readSource(SYNTAX_ROOT);
  const rows = readLedger();
  const rowRe = /LexicalEntry\s*\{\s*spelling:\s*("(?:\\.|[^"\\])*")\s*,\s*meaning:\s*("(?:\\.|[^"\\])*")\s*,\s*decision:\s*("(?:\\.|[^"\\])*")\s*\}/g;
  const forms = [];
  for (const match of source.matchAll(rowRe)) {
    const index = forms.length;
    const spelling = JSON.parse(match[1]);
    const meaning = JSON.parse(match[2]);
    const decision = JSON.parse(match[3]);
    const registered = rows[index];
    assert(registered, `Syntax.rs has an unparsed LexicalEntry at row ${index}`);
    assert(registered.spelling === spelling, `lexical ledger spelling mismatch at row ${index}`);
    assert(registered.meaning === meaning, `lexical ledger meaning mismatch at row ${index}`);
    assert(registered.decision === decision, `lexical ledger decision mismatch at row ${index}`);
    forms.push({
      index,
      registry_symbol: `LEXICAL_LEDGER[${index}]`,
      registry: "Syntax::LEXICAL_LEDGER",
      source: `${SYNTAX_ROOT}:${lineNumberAt(source, match.index)}`,
      spelling,
      meaning,
      decision,
      decision_ids: registryDecisionIds(decision),
      id: `syntax-registry-${String(index).padStart(3, "0")}-${stableSurfaceSlug(spelling)}`,
    });
  }
  assert(forms.length === rows.length, `Syntax.rs lexical registry changed shape: parsed ${forms.length}, ledger returned ${rows.length}`);
  assert(forms.length > 0, "Syntax::LEXICAL_LEDGER has no registered forms");
  return forms;
}

function fixturePathForRegistry(index) {
  return `${FIXTURE_ROOT}/${String(index).padStart(3, "0")}.jet`;
}

function fixtureIndex(fileName) {
  const match = fileName.match(/^(\d{3})\.jet$/);
  return match ? Number(match[1]) : null;
}

function loadRegisteredFixtures(forms) {
  const directory = absPath(FIXTURE_ROOT);
  assert(existsSync(directory), `missing syntax fixture directory: ${FIXTURE_ROOT}`);
  const byIndex = new Map();
  for (const name of readdirSync(directory).sort((left, right) => left.localeCompare(right))) {
    const index = fixtureIndex(name);
    if (index === null) continue;
    assert(forms[index], `fixture ${FIXTURE_ROOT}/${name} names unknown registry row ${index}`);
    assert(!byIndex.has(index), `duplicate fixture for registry row ${index}`);
    const path = `${FIXTURE_ROOT}/${name}`;
    const source = readSource(path);
    byIndex.set(index, {
      index,
      path,
      source,
      semantic_tokens: semanticTokens(source),
      anchor_present: source.includes(forms[index].spelling),
    });
  }
  return byIndex;
}

function parseCommand(value) {
  const text = String(value ?? "").trim();
  if (!text) return null;
  const tokens = [];
  const tokenRe = /"([^"\\]*(?:\\.[^"\\]*)*)"|'([^']*)'|(\S+)/g;
  for (const match of text.matchAll(tokenRe)) {
    tokens.push(match[1] ?? match[2] ?? match[3]);
  }
  return tokens.length ? tokens : null;
}

function defaultJetCommand() {
  const local = absPath("target/debug/jet");
  return existsSync(local) ? [local] : ["jet"];
}

function configuredCommand(name, fallback) {
  return parseCommand(process.env[name]) ?? fallback;
}

function commandText(command, args = []) {
  return [...command, ...args].map((value) => JSON.stringify(value)).join(" ");
}

function stableOutput(text, limit = 2_000) {
  return String(text ?? "")
    .replaceAll(ROOT, "<repo>")
    .replaceAll(/\r\n/g, "\n")
    .slice(0, limit);
}
function stableArgument(value) {
  const text = String(value ?? "");
  const scratchRoot = resolve(process.env.JET_TEST_SCRATCH_DIR ?? DEFAULT_SCRATCH);
  if (text === scratchRoot || text.startsWith(`${scratchRoot}/`)) {
    return `<scratch>${text.slice(scratchRoot.length)}`;
  }
  return text.replaceAll(ROOT, "<repo>");
}

function unavailableResult(reason, command = null) {
  return {
    status: "unavailable",
    reason,
    command,
    evidence: [],
  };
}

function failedResult(reason, command, evidence = {}) {
  return {
    status: "fail",
    reason,
    command,
    evidence: [evidence],
  };
}

function passedResult(reason, command, evidence = {}) {
  return {
    status: "pass",
    reason,
    command,
    evidence: [evidence],
  };
}

function processUnavailable(error, result) {
  if (error?.code === "ENOENT" || error?.code === "ETIMEDOUT" || result?.status === 127 || result?.status === 126) return true;
  const stderr = `${result?.stderr ?? ""} ${result?.stdout ?? ""}`;
  return /DataTree|build\.rs|rustc|cargo|workspace|compiler.+(?:unavailable|failed|error)|No such file|not found/i.test(stderr);
}

function runCommand(command, args, options = {}) {
  const result = spawnSync(command[0], [...command.slice(1), ...args], {
    cwd: ROOT,
    encoding: "utf8",
    input: options.input,
    timeout: options.timeout ?? DYNAMIC_TIMEOUT_MS,
    maxBuffer: 4 * 1024 * 1024,
    env: { ...process.env, ...(options.env ?? {}) },
  });
  return {
    command: [...command, ...args].map(stableArgument),
    status: result.status,
    signal: result.signal,
    stdout: stableOutput(result.stdout),
    stderr: stableOutput(result.stderr),
    raw_stdout: result.stdout ?? "",
    raw_stderr: result.stderr ?? "",
    error: result.error ? { code: result.error.code, message: result.error.message } : null,
  };
}

function walkFiles(path, extensions) {
  const absolute = absPath(path);
  assert(existsSync(absolute), `missing source root: ${path}`);
  const stat = statSync(absolute);
  if (stat.isFile()) {
    return extensions.has(absolute.slice(absolute.lastIndexOf(".")))
      ? [relPath(absolute)]
      : [];
  }
  return readdirSync(absolute, { withFileTypes: true })
    .filter((entry) => !entry.isSymbolicLink())
    .filter((entry) => ![".git", "node_modules", "target", "dist", "build"].includes(entry.name))
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => walkFiles(relPath(resolve(absolute, entry.name)), extensions));
}

function blankRange(output, source, start, end) {
  for (let i = start; i < end; i += 1) {
    if (source[i] !== "\n" && source[i] !== "\r") output[i] = " ";
  }
}

// Keep string literals, but remove comments. This makes source evidence count
// only code and literal tables, never a decision comment that repeats a form.
function maskComments(source) {
  const output = [...source];
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
    if (source.startsWith("//", i)) {
      const start = i;
      i += 2;
      while (i < source.length && source[i] !== "\n") i += 1;
      blankRange(output, source, start, i);
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
      blankRange(output, source, start, i);
      continue;
    }
    i += 1;
  }
  return output.join("");
}
function semanticTokens(source) {
  const code = maskComments(source);
  const tokens = [];
  const tokenRe = /"(?:\\.|[^"\\])*"|r#*"(?:.|\n)*?"#*|[A-Za-z_][A-Za-z0-9_]*|[0-9]+(?:\.[0-9]+)?|[^\s]/g;
  for (const match of code.matchAll(tokenRe)) tokens.push(match[0]);
  return tokens;
}
function processEvidence(result) {
  return {
    exit_code: result.status,
    signal: result.signal,
    stdout: result.stdout,
    stderr: result.stderr,
    error: result.error,
  };
}

function commandOutcome(result, surface) {
  if (result.error || result.status !== 0) {
    const evidence = processEvidence(result);
    if (processUnavailable(result.error, result)) {
      return unavailableResult(
        `${surface} probe could not execute the compiler; green compiler evidence is required`,
        result.command,
      );
    }
    return failedResult(
      `${surface} probe exited without a successful round-trip result`,
      result.command,
      evidence,
    );
  }
  return null;
}

function scratchPath(id, fileName = null) {
  const root = resolve(process.env.JET_TEST_SCRATCH_DIR ?? DEFAULT_SCRATCH, "syntax-roundtrip");
  const directory = fileName ? resolve(root, id) : root;
  mkdirSync(directory, { recursive: true });
  return { root, path: resolve(directory, fileName ?? `${id}.jet`) };
}

function prepareScratchFixture(form, fixture) {
  const scratch = scratchPath(form.id);
  rmSync(scratch.path, { force: true });
  writeFileSync(scratch.path, fixture.source);
  return scratch;
}

function runFormatter(form, fixture) {
  if (!fixture.anchor_present) {
    return failedResult(
      `fixture does not contain registered spelling ${JSON.stringify(form.spelling)}`,
      null,
      { program_path: fixture.path },
    );
  }
  const command = configuredCommand("JET_SYNTAX_CENSUS_FMT_COMMAND", defaultJetCommand());
  const scratch = prepareScratchFixture(form, fixture);
  const first = runCommand(command, ["fmt", scratch.path]);
  const firstFailure = commandOutcome(first, "jet fmt");
  if (firstFailure) {
    firstFailure.evidence.push({ program_path: fixture.path });
    return firstFailure;
  }
  if (!existsSync(scratch.path)) {
    return failedResult("jet fmt reported success but removed the fixture program", first.command, {
      program_path: fixture.path,
    });
  }
  const once = readFileSync(scratch.path, "utf8");
  const onceTokens = semanticTokens(once);
  if (JSON.stringify(onceTokens) !== JSON.stringify(fixture.semantic_tokens)) {
    return failedResult("jet fmt rewrote, reordered, or dropped source meaning", first.command, {
      program_path: fixture.path,
      before_tokens: fixture.semantic_tokens,
      after_tokens: onceTokens,
    });
  }
  const second = runCommand(command, ["fmt", scratch.path]);
  const secondFailure = commandOutcome(second, "jet fmt");
  if (secondFailure) {
    secondFailure.evidence.push({ program_path: fixture.path, pass: 2 });
    return secondFailure;
  }
  const twice = readFileSync(scratch.path, "utf8");
  if (once !== twice) {
    return failedResult("jet fmt is not idempotent on the registered-form fixture", second.command, {
      program_path: fixture.path,
      first_pass: once,
      second_pass: twice,
    });
  }
  return passedResult("jet fmt preserved semantic tokens and was byte-idempotent", second.command, {
    program_path: fixture.path,
    semantic_token_count: onceTokens.length,
    idempotent: true,
  });
}

function lspPositionAt(source, offset) {
  const prefix = source.slice(0, offset);
  const line = prefix.split("\n").length - 1;
  const lastNewline = prefix.lastIndexOf("\n");
  const lineText = prefix.slice(lastNewline + 1);
  return { line, character: [...lineText].length };
}

function lspOffsetAt(source, position) {
  const lines = source.split("\n");
  const line = Math.max(0, Math.min(Number(position?.line ?? 0), lines.length - 1));
  let offset = lines.slice(0, line).reduce((total, value) => total + value.length + 1, 0);
  let units = 0;
  for (const char of lines[line]) {
    if (units >= Number(position?.character ?? 0)) break;
    units += char.length;
    offset += char.length;
  }
  return offset;
}

function applyLspEdits(source, edits) {
  if (!Array.isArray(edits)) return source;
  const spans = edits.map((edit) => ({
    start: lspOffsetAt(source, edit.range?.start),
    end: lspOffsetAt(source, edit.range?.end),
    text: String(edit.newText ?? ""),
  })).sort((left, right) => right.start - left.start || right.end - left.end);
  let result = source;
  for (const edit of spans) {
    if (edit.end < edit.start || edit.end > result.length) {
      throw new Error("language server returned an invalid edit range");
    }
    result = `${result.slice(0, edit.start)}${edit.text}${result.slice(edit.end)}`;
  }
  return result;
}

function rpcRequest(id, method, params) {
  return {
    jsonrpc: "2.0",
    id,
    method,
    params,
  };
}

function rpcNotification(method, params) {
  return { jsonrpc: "2.0", method, params };
}

function rpcFrame(message) {
  const body = JSON.stringify(message);
  return `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`;
}

function parseRpcResponses(output) {
  const bytes = Buffer.from(output ?? "", "utf8");
  const marker = Buffer.from("Content-Length:");
  const responses = [];
  let cursor = 0;
  while (cursor < bytes.length) {
    const start = bytes.indexOf(marker, cursor);
    if (start < 0) break;
    const headerEnd = bytes.indexOf(Buffer.from("\r\n\r\n"), start);
    if (headerEnd < 0) break;
    const header = bytes.slice(start, headerEnd).toString("utf8");
    const length = Number(header.match(/Content-Length:\s*(\d+)/i)?.[1]);
    if (!Number.isInteger(length) || length < 0) break;
    const bodyStart = headerEnd + 4;
    const body = bytes.slice(bodyStart, bodyStart + length).toString("utf8");
    try {
      responses.push(JSON.parse(body));
    } catch {
      break;
    }
    cursor = bodyStart + length;
  }
  return responses;
}

function fileUri(path) {
  return `file://${path.split("\\").join("/").replaceAll(" ", "%20")}`;
}

function runLanguageServer(form, fixture) {
  if (!fixture.anchor_present) {
    return failedResult(
      `fixture does not contain registered spelling ${JSON.stringify(form.spelling)}`,
      null,
      { program_path: fixture.path },
    );
  }
  const command = configuredCommand("JET_SYNTAX_CENSUS_LSP_COMMAND", defaultJetCommand());
  const path = absPath(fixture.path);
  const uri = fileUri(path);
  const anchor = fixture.source.indexOf(form.spelling);
  const position = lspPositionAt(fixture.source, Math.max(0, anchor));
  const messages = [
    rpcRequest(1, "initialize", {
      processId: process.pid,
      rootUri: fileUri(ROOT),
      capabilities: {},
      workspaceFolders: [{ uri: fileUri(ROOT), name: "jet" }],
    }),
    rpcNotification("initialized", {}),
    rpcNotification("textDocument/didOpen", {
      textDocument: { uri, languageId: "jet", version: 1, text: fixture.source },
    }),
    rpcRequest(2, "textDocument/formatting", {
      textDocument: { uri },
      options: { tabSize: 4, insertSpaces: true },
    }),
    rpcRequest(3, "textDocument/hover", {
      textDocument: { uri },
      position,
    }),
    rpcRequest(4, "shutdown", null),
    rpcNotification("exit", null),
  ];
  const lspArgs = parseCommand(process.env.JET_SYNTAX_CENSUS_LSP_ARGS) ?? ["self", "lsp"];
  const result = runCommand(command, lspArgs, { input: messages.map(rpcFrame).join("") });
  const failure = commandOutcome(result, "language server");
  if (failure) {
    failure.evidence.push({ program_path: fixture.path, span: { start: anchor, end: anchor + form.spelling.length } });
    return failure;
  }
  const responses = parseRpcResponses(result.raw_stdout);
  const formatting = responses.find((response) => response.id === 2);
  const hover = responses.find((response) => response.id === 3);
  if (!formatting || formatting.error) {
    return failedResult("language server returned no usable formatting response", result.command, {
      program_path: fixture.path,
      span: { start: anchor, end: anchor + form.spelling.length },
      response: formatting ?? null,
    });
  }
  if (!hover || hover.error) {
    return failedResult("language server returned no usable span response", result.command, {
      program_path: fixture.path,
      span: { start: anchor, end: anchor + form.spelling.length },
      response: hover ?? null,
    });
  }
  const beforeTokens = fixture.semantic_tokens;
  const formatted = applyLspEdits(fixture.source, formatting.result);
  const afterTokens = semanticTokens(formatted);
  if (JSON.stringify(beforeTokens) !== JSON.stringify(afterTokens)) {
    return failedResult("language server rewrote, reordered, or dropped source meaning", result.command, {
      program_path: fixture.path,
      before_tokens: beforeTokens,
      after_tokens: afterTokens,
    });
  }
  return passedResult("language server formatting and span response preserved semantic tokens", result.command, {
    program_path: fixture.path,
    span: { start: anchor, end: anchor + form.spelling.length },
    formatting_edits: Array.isArray(formatting.result) ? formatting.result.length : 0,
    span_response: hover.result === null ? "null" : "present",
  });
}
function projectedSource(value) {
  if (typeof value === "string") return null;
  if (!value || typeof value !== "object") return null;
  if (typeof value.source_text === "string") return value.source_text;
  for (const key of Object.keys(value).sort()) {
    const found = projectedSource(value[key]);
    if (found !== null) return found;
  }
  return null;
}

function canvasCommandFor(form, fixture) {
  const configured = parseCommand(process.env.JET_SYNTAX_CENSUS_CANVAS_COMMAND)
    ?? ["node", "scripts/agent/syntax-roundtrip-canvas-probe.mjs", "{fixture}"];
  const replacements = new Map([
    ["{fixture}", absPath(fixture.path)],
    ["{form}", form.id],
    ["{root}", ROOT],
  ]);
  const command = configured.map((token) => [...replacements.entries()]
    .reduce((value, [needle, replacement]) => value.replaceAll(needle, replacement), token));
  if (!command.some((token) => token === absPath(fixture.path))) command.push(absPath(fixture.path));
  return command;
}

function runCanvasProjection(form, fixture) {
  if (!fixture.anchor_present) {
    return failedResult(
      `fixture does not contain registered spelling ${JSON.stringify(form.spelling)}`,
      null,
      { program_path: fixture.path },
    );
  }
  const command = canvasCommandFor(form, fixture);
  const result = runCommand(command, []);
  const failure = commandOutcome(result, "Canvas projection");
  if (failure) {
    failure.evidence.push({ program_path: fixture.path });
    return failure;
  }
  let payload;
  try {
    payload = JSON.parse(result.raw_stdout);
  } catch (error) {
    return failedResult("Canvas projection did not return JSON", result.command, {
      program_path: fixture.path,
      parse_error: error.message,
      stdout: result.stdout,
    });
  }
  if (payload?.canvas_probe_status === "unavailable") {
    return unavailableResult(
      payload.canvas_probe_reason ?? "Canvas projection probe could not run",
      result.command,
    );
  }
  const httpStatus = payload?.canvas_http_status;
  if (Number.isInteger(httpStatus) && (httpStatus < 200 || httpStatus >= 300)) {
    return failedResult(`Canvas projection returned HTTP ${httpStatus}`, result.command, {
      program_path: fixture.path,
      http_status: httpStatus,
      response: payload.canvas_response ?? null,
    });
  }
  const projected = projectedSource(payload);
  if (projected === null) {
    return failedResult("Canvas projection has no source_text round-trip field", result.command, {
      program_path: fixture.path,
    });
  }
  const beforeTokens = fixture.semantic_tokens;
  const afterTokens = semanticTokens(projected);
  if (JSON.stringify(beforeTokens) !== JSON.stringify(afterTokens)) {
    return failedResult("Canvas projection rewrote, reordered, or dropped source meaning", result.command, {
      program_path: fixture.path,
      before_tokens: beforeTokens,
      after_tokens: afterTokens,
    });
  }
  return passedResult("Canvas projection retained source_text semantic tokens", result.command, {
    program_path: fixture.path,
    semantic_token_count: afterTokens.length,
  });
}

function uncoveredCell(form, surface, reason) {
  return {
    status: "uncovered",
    reason,
    surface,
    program_path: fixturePathForRegistry(form.index),
    evidence: [],
  };
}

function dynamicUnavailableCell(form, surface, fixture, reason) {
  return {
    ...unavailableResult(reason),
    surface,
    program_path: fixture?.path ?? fixturePathForRegistry(form.index),
  };
}

function registeredSurfaceCell(form, surface, fixture) {
  if (!fixture) {
    return uncoveredCell(form, surface, `No fixture program is registered for ${form.registry_symbol}`);
  }
  if (!fixture.anchor_present) {
    return {
      status: "fail",
      reason: `Fixture ${fixture.path} does not contain registered spelling ${JSON.stringify(form.spelling)}`,
      surface,
      program_path: fixture.path,
      evidence: [],
    };
  }
  if (process.env.JET_SYNTAX_CENSUS_SKIP_DYNAMIC === "1") {
    return dynamicUnavailableCell(
      form,
      surface,
      fixture,
      "Dynamic surface execution was disabled; green compiler evidence is required",
    );
  }
  const result = surface === "jet_fmt"
    ? runFormatter(form, fixture)
    : surface === "language_server"
      ? runLanguageServer(form, fixture)
      : runCanvasProjection(form, fixture);
  return {
    ...result,
    surface,
    program_path: fixture.path,
  };
}

function registeredStatusCounts(rows) {
  const counts = Object.fromEntries(REGISTERED_SURFACES.map((surface) => [surface, {
    pass: 0,
    fail: 0,
    unavailable: 0,
    uncovered: 0,
  }]));
  let failureCells = 0;
  for (const row of rows) {
    for (const surface of REGISTERED_SURFACES) {
      const cell = row.surfaces[surface];
      assert(counts[surface][cell.status] !== undefined, `unknown registered status ${cell.status}`);
      counts[surface][cell.status] += 1;
      if (cell.status !== "pass") failureCells += 1;
    }
  }
  return { counts, failureCells };
}

function buildRegisteredMatrix() {
  const forms = enumerateRegisteredForms();
  const fixtures = loadRegisteredFixtures(forms);
  const rows = forms.map((form) => ({
    ...form,
    fixture: fixtures.get(form.index)?.path ?? fixturePathForRegistry(form.index),
    fixture_present: fixtures.has(form.index),
    surfaces: Object.fromEntries(REGISTERED_SURFACES.map((surface) => [
      surface,
      finalizeRoundtripCell(form, surface, registeredSurfaceCell(form, surface, fixtures.get(form.index))),
    ])),
  }));
  const status = registeredStatusCounts(rows);
  const compilerRequired = rows
    .flatMap((row) => REGISTERED_SURFACES
      .filter((surface) => row.surfaces[surface].status === "unavailable")
      .map((surface) => `${row.registry_symbol}:${surface}`))
    .sort();
  const uncovered = rows
    .flatMap((row) => REGISTERED_SURFACES
      .filter((surface) => row.surfaces[surface].status === "uncovered")
      .map((surface) => `${row.registry_symbol}:${surface}`))
    .sort();
  return {
    schema_version: 1,
    registry: "Syntax::LEXICAL_LEDGER",
    registry_source: SYNTAX_ROOT,
    surfaces: REGISTERED_SURFACES,
    forms: rows,
    status_counts: status.counts,
    failure_cells: status.failureCells,
    uncovered,
    compiler_required: compilerRequired,
    closed: status.failureCells === 0,
  };
}
function astFixtureFor(row) {
  const path = `${AST_FIXTURE_ROOT}/${row.id}.jet`;
  if (!existsSync(absPath(path))) return null;
  const source = readSource(path);
  return {
    path,
    source,
    semantic_tokens: semanticTokens(source),
    anchor_present: source.includes(row.variant),
  };
}

function buildAstRoundtripMatrix(astRows) {
  const rows = astRows.map((row) => {
    const fixture = astFixtureFor(row);
    const form = {
      id: row.id,
      index: row.id,
      spelling: row.variant,
    };
    const surfaces = Object.fromEntries(REGISTERED_SURFACES.map((surface) => {
      if (!fixture) {
        return [surface, {
          status: "uncovered",
          reason: `No AST fixture program is registered for ${row.enum_name}::${row.variant}`,
          surface,
          program_path: `${AST_FIXTURE_ROOT}/${row.id}.jet`,
          evidence: [],
        }];
      }
      const result = process.env.JET_SYNTAX_CENSUS_SKIP_DYNAMIC === "1"
        ? dynamicUnavailableCell(form, surface, fixture, "Dynamic surface execution was disabled; green compiler evidence is required")
        : surface === "jet_fmt"
          ? runFormatter(form, fixture)
          : surface === "language_server"
            ? runLanguageServer(form, fixture)
            : runCanvasProjection(form, fixture);
      return [surface, { ...result, surface, program_path: fixture.path }];
    }));
    for (const surface of REGISTERED_SURFACES) {
      surfaces[surface] = finalizeRoundtripCell(form, surface, surfaces[surface]);
    }
    return {
      id: row.id,
      category: row.category,
      enum_name: row.enum_name,
      variant: row.variant,
      source_evidence: row.source_evidence,
      fixture: fixture?.path ?? `${AST_FIXTURE_ROOT}/${row.id}.jet`,
      fixture_present: Boolean(fixture),
      surfaces,
    };
  });
  const status = registeredStatusCounts(rows);
  return {
    registry: "AST::Stmt + AST::Expr",
    surfaces: REGISTERED_SURFACES,
    rows,
    status_counts: status.counts,
    failure_cells: status.failureCells,
    closed: status.failureCells === 0,
  };
}
function runExamplesFmtGate() {
  const paths = walkFiles("examples", new Set([".jet"]));
  if (!paths.length) {
    return {
      status: "uncovered",
      reason: "No examples/*.jet programs were found",
      checked: 0,
      cells: [],
    };
  }
  const command = configuredCommand("JET_SYNTAX_CENSUS_FMT_COMMAND", defaultJetCommand());
  const cells = [];
  for (const path of paths) {
    const id = `example-${stableSurfaceSlug(path)}`;
    // `jet fmt` selects the Package formatter by the reserved `package.jet` basename.
    const scratch = scratchPath(id, path.endsWith("/package.jet") ? "package.jet" : null);
    rmSync(scratch.path, { force: true });
    copyFileSync(absPath(path), scratch.path);
    const first = runCommand(command, ["fmt", scratch.path]);
    const firstFailure = commandOutcome(first, "examples jet fmt");
    if (firstFailure) {
      cells.push({
        status: firstFailure.status,
        reason: firstFailure.reason,
        program_path: path,
        evidence: firstFailure.evidence,
      });
      if (firstFailure.status === "unavailable") {
        for (const remaining of paths.slice(cells.length)) {
          cells.push({
            status: "unavailable",
            reason: firstFailure.reason,
            program_path: remaining,
            evidence: [],
          });
        }
        break;
      }
      continue;
    }
    const once = readFileSync(scratch.path, "utf8");
    const second = runCommand(command, ["fmt", scratch.path]);
    const secondFailure = commandOutcome(second, "examples jet fmt");
    if (secondFailure) {
      cells.push({
        status: secondFailure.status,
        reason: secondFailure.reason,
        program_path: path,
        evidence: secondFailure.evidence,
      });
      continue;
    }
    const twice = readFileSync(scratch.path, "utf8");
    cells.push({
      status: once === twice ? "pass" : "fail",
      reason: once === twice ? "jet fmt is byte-idempotent" : "jet fmt is not byte-idempotent",
      program_path: path,
      evidence: [{ command: second.command, idempotent: once === twice }],
    });
  }
  const nonPass = cells.filter((cell) => cell.status !== "pass");
  return {
    status: nonPass.length ? nonPass[0].status : "pass",
    reason: nonPass.length ? nonPass[0].reason : "jet fmt was idempotent across the examples tree",
    checked: cells.length,
    total: paths.length,
    cells,
  };
}

function buildCriteria(registeredSyntax, astRoundtrip, examplesFmt) {
  const everySurfaceCell = registeredSyntax.forms.every((row) => REGISTERED_SURFACES.every((surface) => row.surfaces[surface]));
  const allPass = registeredSyntax.closed && astRoundtrip.closed && examplesFmt.status === "pass";
  return {
    "1": {
      status: everySurfaceCell ? "implemented" : "blocked",
      result: `${registeredSyntax.forms.length} canonical registry forms and ${astRoundtrip.rows.length} AST variants have stable ${REGISTERED_SURFACES.length}-surface cells with program paths`,
      matrix_paths: [FIXTURE_MATRIX_JSON, FIXTURE_MATRIX_TSV, FIXTURE_MATRIX_MD],
    },
    "2": {
      status: allPass ? "implemented" : "blocked",
      result: allPass
        ? "All surface cells pass and jet fmt is idempotent across examples"
        : "Fail-closed until every cell passes and the examples fmt idempotence gate is green",
      failing_cells: [
        ...registeredSyntax.forms.flatMap((row) => REGISTERED_SURFACES
          .filter((surface) => row.surfaces[surface].status !== "pass")
          .map((surface) => row.surfaces[surface].gap_id)),
        ...astRoundtrip.rows.flatMap((row) => REGISTERED_SURFACES
          .filter((surface) => row.surfaces[surface].status !== "pass")
          .map((surface) => row.surfaces[surface].gap_id)),
      ].filter(Boolean).sort(),
      examples_fmt_idempotence: examplesFmt.status,
    },
    "3": {
      status: astRoundtrip.closed ? "implemented" : "blocked",
      result: astRoundtrip.closed
        ? "AST inventory gate requires three pass cells for every variant"
        : "AST variants are fail-closed until each gets all three passing cells",
      ast_matrix_rows: astRoundtrip.rows.length,
    },
  };
}

function normaliseLine(text) {
  return text.trim().replace(/\s+/g, " ");
}

function makeFile(path) {
  const source = readSource(path);
  const lines = source.split("\n");
  const codeSource = maskComments(source);
  const code = codeSource.split("\n");
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "\n") starts.push(index + 1);
  }
  const lineNumber = (offset) => {
    let low = 0;
    let high = starts.length;
    while (low + 1 < high) {
      const middle = Math.floor((low + high) / 2);
      if (starts[middle] <= offset) low = middle;
      else high = middle;
    }
    return low + 1;
  };
  return {
    path,
    source,
    lines,
    code,
    codeSource,
    lineNumber,
    lineText(line) {
      return normaliseLine(lines[line - 1] ?? "");
    },
  };
}

function extractIds(text) {
  DECISION_RE.lastIndex = 0;
  return [...new Set([...text.matchAll(DECISION_RE)].map((match) => match[0]))];
}

function commentBody(line) {
  const match = line.match(COMMENT_RE);
  return match ? match[1] : null;
}

function extractBackticks(text) {
  return [...text.matchAll(/`([^`]+)`/g)].map((match) => match[1].trim());
}

function isUsefulExample(value) {
  const text = value.trim();
  if (!text || text.length > 180) return false;
  if (/^(?:D-|S\d|E\d{4}|L\d{4}|W\d{4})/.test(text)) return false;
  if (/^(?:card\s+#|crates\/|Source\/|docs\/|tests\/)/i.test(text)) return false;
  if (/^[A-Za-z_][A-Za-z0-9_]*::/.test(text)) return false;
  if (/^(?:true|false|null|None|Some|Ok|Err)$/.test(text)) return false;
  return true;
}

function parseCommentParagraphs(file) {
  const paragraphs = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const body = commentBody(file.lines[index]);
    if (body === null || !body.trim() || !extractIds(body).length) continue;
    const start = index;
    const ids = new Set(extractIds(body));
    let end = index + 1;
    while (end < file.lines.length) {
      const nextBody = commentBody(file.lines[end]);
      if (nextBody === null || !nextBody.trim() || extractIds(nextBody).length) break;
      end += 1;
    }
    const text = file.lines.slice(start, end).join(" ");
    paragraphs.push({
      startLine: start + 1,
      endLine: end,
      ids: [...ids].sort(),
      examples: [...new Set(extractBackticks(text).filter(isUsefulExample))].sort(),
      text,
      key: `${file.path}:${start + 1}`,
    });
    index = end - 1;
  }
  return paragraphs;
}

function rustStringLiterals(text) {
  const values = [];
  for (let index = 0; index < text.length; ) {
    const raw = text.slice(index).match(/^r(#+)"/);
    if (raw) {
      const hashes = raw[1];
      const close = `"${hashes}`;
      const start = index + raw[0].length;
      const end = text.indexOf(close, start);
      if (end < 0) break;
      values.push(text.slice(start, end));
      index = end + close.length;
      continue;
    }
    if (text[index] !== `"`) {
      index += 1;
      continue;
    }
    let end = index + 1;
    while (end < text.length) {
      if (text[end] === "\\") end += 2;
      else if (text[end] === `"`) break;
      else end += 1;
    }
    if (end >= text.length) break;
    const literal = text.slice(index, end + 1);
    try {
      values.push(JSON.parse(literal));
    } catch {
      values.push(literal.slice(1, -1));
    }
    index = end + 1;
  }
  return values.filter((value) => value.length > 0);
}

function declarationLiteralBlock(file, startIndex) {
  const lines = [];
  let bracketDepth = 0;
  let quote = false;
  let escaped = false;
  let endIndex = startIndex;
  for (; endIndex < file.code.length && endIndex < startIndex + 512; endIndex += 1) {
    const line = file.code[endIndex];
    lines.push(line);
    for (const char of line) {
      if (quote) {
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === `"`) quote = false;
      } else if (char === `"`) {
        quote = true;
      } else if (char === "[") {
        bracketDepth += 1;
      } else if (char === "]") {
        bracketDepth = Math.max(0, bracketDepth - 1);
      } else if (char === ";" && bracketDepth === 0) {
        return { text: lines.join("\n"), endIndex };
      }
    }
  }
  return { text: lines.join("\n"), endIndex: Math.min(endIndex, file.code.length - 1) };
}


function declarationLiterals(file, startIndex, symbol) {
  const block = declarationLiteralBlock(file, startIndex);
  const values = NON_SYNTAX_LITERAL_TABLES.has(symbol) ? [] : rustStringLiterals(block.text);
  return { ...block, values };
}

function declarationSymbol(line) {
  return line.match(DECL_RE)?.[1] ?? null;
}

function isDeclarationLine(line) {
  return DECL_RE.test(line);
}

function declarationDocs(declarationLine, paragraphs) {
  return paragraphs.find((paragraph) => paragraph.endLine + 1 === declarationLine) ?? null;
}

function internalDeclaration(symbol, text) {
  return /^INTERNAL_|^GENERATED_|^ACRONYM_/.test(symbol) || /(?:sema-only|compiler-private|never source syntax|not source syntax|cannot be written by source code|not a source spelling|hidden member)/i.test(text);
}

function lifecycleFor(symbol, text) {
  return /(?:retired|teaching-only|teaching diagnostic|foreign spelling)/i.test(`${symbol} ${text}`)
    ? "retired"
    : "current";
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function tokenMatches(line, token) {
  if (!token) return false;
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(token)) {
    return new RegExp(`\\b${escapeRegex(token)}\\b`).test(line);
  }
  return line.includes(token);
}

function meaningfulTokens(candidate, includeExamples = false) {
  const tokens = [];
  if (candidate.symbol) tokens.push({ value: candidate.symbol, kind: "symbol" });
  if (candidate.surface && candidate.surface.length >= 2) {
    tokens.push({ value: candidate.surface, kind: "surface" });
  }
  if (includeExamples) {
    for (const example of candidate.examples) {
      if (example.length >= 2 && !tokens.some((token) => token.value === example)) {
        tokens.push({ value: example, kind: "example" });
      }
    }
  }
  return tokens;
}
function evidence(file, line, kind) {
  return {
    kind,
    file: file.path,
    line,
    text: file.lineText(line),
  };
}

function dedupeEvidence(items) {
  const seen = new Set();
  return items
    .filter((item) => {
      const key = item.status === "missing"
        ? `missing:${item.reason}`
        : `${item.file}:${item.line}:${item.kind}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => {
      if (a.status === "missing" && b.status !== "missing") return 1;
      if (a.status !== "missing" && b.status === "missing") return -1;
      if (a.file !== b.file) return (a.file ?? "").localeCompare(b.file ?? "");
      if (a.line !== b.line) return (a.line ?? 0) - (b.line ?? 0);
      return (a.kind ?? "").localeCompare(b.kind ?? "");
    });
}

function missingEvidence(reason) {
  return [{ status: "missing", reason }];
}

function coveredCell(items, reason) {
  const values = dedupeEvidence(items);
  return values.length
    ? { status: "covered", reason, evidence: values }
    : { status: "missing", reason, evidence: missingEvidence(reason) };
}

function missingCell(reason) {
  return { status: "missing", reason, evidence: missingEvidence(reason) };
}

function notApplicableCell(reason, items) {
  return {
    status: "not_applicable",
    reason,
    evidence: dedupeEvidence(items),
  };
}

function tokenOffsets(file, token) {
  const offsets = [];
  let start = 0;
  while (start < file.codeSource.length) {
    const offset = file.codeSource.indexOf(token, start);
    if (offset < 0) break;
    if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(token)) {
      const before = file.codeSource[offset - 1] ?? "";
      const after = file.codeSource[offset + token.length] ?? "";
      if (/[A-Za-z0-9_]/.test(before) || /[A-Za-z0-9_]/.test(after)) {
        start = offset + token.length;
        continue;
      }
    }
    offsets.push(offset);
    start = offset + Math.max(1, token.length);
  }
  return offsets;
}

function searchEvidence(files, candidate, predicate, max = MAX_EVIDENCE) {
  const tokens = meaningfulTokens(candidate);
  if (!tokens.length) return [];
  const matches = [];
  for (const file of files) {
    for (const token of tokens) {
      for (const offset of tokenOffsets(file, token.value)) {
        const line = file.lineNumber(offset);
        const codeLine = file.code[line - 1] ?? "";
        if (predicate && !predicate(codeLine, file.path)) continue;
        matches.push(evidence(file, line, `${token.kind}:static`));
        if (matches.length >= max) return dedupeEvidence(matches);
      }
    }
  }
  return dedupeEvidence(matches);
}

function fixtureEvidence(files, candidate) {
  const tokens = meaningfulTokens(candidate, true);
  if (!tokens.length) return [];
  const matches = [];
  for (const file of files) {
    if (!/format|fmt|parser|fixture|test/i.test(file.path)) continue;
    const formatterLines = file.code
      .map((line, index) => /format_source|Formatter|fmt_/i.test(line) ? index + 1 : null)
      .filter((line) => line !== null);
    if (!formatterLines.length) continue;
    for (const token of tokens) {
      for (const offset of tokenOffsets(file, token.value)) {
        const tokenLine = file.lineNumber(offset);
        const formatterLine = formatterLines
          .map((line) => ({ line, distance: Math.abs(line - tokenLine) }))
          .sort((a, b) => a.distance - b.distance || a.line - b.line)[0];
        if (!formatterLine || formatterLine.distance > 64) continue;
        const start = Math.max(0, Math.min(tokenLine, formatterLine.line) - 1 - 16);
        const end = Math.min(file.code.length, Math.max(tokenLine, formatterLine.line) + 16);
        const window = file.code.slice(start, end).join("\n");
        if (!/assert_eq!/.test(window) || !/(?:once|twice|formatted)/.test(window)) continue;
        matches.push(evidence(file, formatterLine.line, "roundtrip:formatter-call"));
        if (tokenLine !== formatterLine.line) matches.push(evidence(file, tokenLine, "roundtrip:surface"));
        if (matches.length >= MAX_EVIDENCE) return dedupeEvidence(matches);
      }
    }
  }
  return dedupeEvidence(matches);
}

function rowFormKind(candidate) {
  if (candidate.scope === "internal") return "internal";
  const surface = candidate.surface;
  if (!surface) return candidate.symbol ? "declared_surface" : "decision_row";
  if (/^--?[A-Za-z]/.test(surface)) return "cli_flag";
  if (/^#|^@|^\.[\[({]/.test(surface)) return "marker_or_sigil";
  if (/^[A-Z][A-Za-z0-9_]*$/.test(surface)) return candidate.symbol?.startsWith("MARKER_") ? "marker" : "type_or_name";
  if (/^[a-z][A-Za-z0-9_]*$/.test(surface)) return "keyword_or_name";
  if (/^[A-Za-z_][A-Za-z0-9_]*\([^)]*\)$/.test(surface)) return "call_shape";
  if (/[{}()[\],.:?=!+*\-/<>]/.test(surface)) return "syntax_shape";
  return "identifier";
}

function stableSurfaceSlug(value) {
  const base = (value || "none")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48) || "none";
  let hash = 2166136261;
  for (const char of value || "none") {
    hash ^= char.codePointAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return `${base}-${(hash >>> 0).toString(16).padStart(8, "0")}`;
}

function stableCandidateSlug(candidate) {
  const prefix = candidate.symbol ? "symbol" : "surface";
  const identity = candidate.symbol
    ? `${candidate.symbol}:${candidate.surface ?? ""}`
    : candidate.surface ?? `decision:${candidate.sourceEvidence[0]?.file ?? "unknown"}:${candidate.sourceEvidence[0]?.line ?? 0}`;
  return `${prefix}-${stableSurfaceSlug(identity)}`;
}

function candidateIdentity(candidate) {
  const decision = candidate.decision_id.toLowerCase();
  if (candidate.symbol && candidate.surface) {
    return `${decision}|symbol:${candidate.symbol}|surface:${candidate.surface}`;
  }
  if (candidate.symbol) return `${decision}|symbol:${candidate.symbol}`;
  if (candidate.surface) return `${decision}|surface:${candidate.surface}`;
  const source = candidate.sourceEvidence[0];
  return `${decision}|none:${source?.file ?? "unknown"}:${source?.line ?? 0}`;
}

function addEvidenceToCandidate(candidate, items) {
  for (const item of items) {
    const key = `${item.file}:${item.line}:${item.kind}`;
    if (!candidate.sourceEvidence.some((existing) => `${existing.file}:${existing.line}:${existing.kind}` === key)) {
      candidate.sourceEvidence.push(item);
    }
  }
}

function addCandidate(candidates, value) {
  const key = candidateIdentity(value);
  const existing = candidates.get(key);
  if (!existing) {
    candidates.set(key, {
      decision_id: value.decision_id,
      symbol: value.symbol ?? null,
      surface: value.surface ?? null,
      examples: new Set(value.examples ?? []),
      scope: value.scope ?? "user",
      lifecycle: value.lifecycle ?? "current",
      sourceEvidence: [...value.sourceEvidence],
    });
    return;
  }
  if (existing.symbol !== value.symbol || existing.surface !== value.surface) {
    fail(`conflicting duplicate candidate ${key}`);
  }
  for (const example of value.examples ?? []) existing.examples.add(example);
  if (existing.scope !== value.scope) fail(`conflicting scope for ${key}`);
  if (existing.lifecycle !== value.lifecycle) existing.lifecycle = "retired";
  addEvidenceToCandidate(existing, value.sourceEvidence);
}

function buildCandidates(syntaxFiles) {
  const candidates = new Map();
  const decisionEvidence = new Map();
  const decisionIds = new Set();
  const attachedParagraphs = new Set();

  for (const file of syntaxFiles) {
    for (let index = 0; index < file.lines.length; index += 1) {
      for (const decision of extractIds(file.lines[index])) {
        decisionIds.add(decision);
        if (!decisionEvidence.has(decision)) decisionEvidence.set(decision, []);
        const item = evidence(file, index + 1, "decision:source");
        const list = decisionEvidence.get(decision);
        if (!list.some((existing) => existing.file === item.file && existing.line === item.line)) list.push(item);
      }
    }
    const paragraphs = parseCommentParagraphs(file);
    for (const paragraph of paragraphs) {
      for (const decision of paragraph.ids) {
        decisionIds.add(decision);
        if (!decisionEvidence.has(decision)) decisionEvidence.set(decision, []);
        const list = decisionEvidence.get(decision);
        const item = evidence(file, paragraph.startLine, "decision:source");
        if (!list.some((existing) => existing.file === item.file && existing.line === item.line)) list.push(item);
      }
    }

    for (let index = 0; index < file.lines.length; index += 1) {
      const line = file.lines[index];
      if (!isDeclarationLine(line)) continue;
      const lineNumber = index + 1;
      const symbol = declarationSymbol(line);
      assert(symbol, `cannot name declaration at ${file.path}:${lineNumber}`);
      const doc = declarationDocs(lineNumber, paragraphs);
      const docText = doc?.text ?? "";
      const literalBlock = declarationLiterals(file, index, symbol);
      const blockPreview = file.lines.slice(index, Math.min(literalBlock.endIndex + 1, index + 8)).join("\n");
      const ids = [...new Set([...(doc?.ids ?? []), ...extractIds(line), ...extractIds(blockPreview)])].sort();
      if (NON_SYNTAX_LITERAL_TABLES.has(symbol)) continue;
      const internal = internalDeclaration(symbol, docText);
      const scope = internal ? "internal" : "user";
      const lifecycle = lifecycleFor(symbol, docText);
      const sourceItems = [];
      for (const decision of ids) decisionIds.add(decision);
      for (const decision of ids) {
        const decisionLine = file.lines
          .slice(index, literalBlock.endIndex + 1)
          .findIndex((value) => extractIds(value).includes(decision));
        if (decisionLine >= 0) {
          sourceItems.push(evidence(file, index + decisionLine + 1, "decision:source"));
        }
      }
      if (doc) {
        attachedParagraphs.add(doc.key);
        for (const decision of ids) {
          decisionIds.add(decision);
          if (!decisionEvidence.has(decision)) decisionEvidence.set(decision, []);
          const item = evidence(file, doc.startLine, "decision:source");
          const list = decisionEvidence.get(decision);
          if (!list.some((existing) => existing.file === item.file && existing.line === item.line)) list.push(item);
        }
        sourceItems.push(evidence(file, doc.startLine, "decision:source"));
      }
      sourceItems.push(evidence(file, lineNumber, "declaration:source"));
      const surfaces = internal
        ? [null]
        : [...new Set(literalBlock.values)];
      if (!surfaces.length) surfaces.push(null);
      for (const decision of ids) {
        for (const surface of surfaces) {
          const rowSourceItems = [...sourceItems];
          if (surface) {
            const literalLine = file.code
              .slice(index, literalBlock.endIndex + 1)
              .findIndex((value) => value.includes(surface));
            if (literalLine >= 0) rowSourceItems.push(evidence(file, index + literalLine + 1, "literal:source"));
          }
          addCandidate(candidates, {
            decision_id: decision,
            symbol,
            surface,
            examples: doc?.examples ?? [],
            scope,
            lifecycle,
            sourceEvidence: rowSourceItems,
          });
        }
      }
    }

    for (const paragraph of paragraphs) {
      if (attachedParagraphs.has(paragraph.key)) continue;
      const sourceItems = [evidence(file, paragraph.startLine, "decision:source")];
      for (const decision of paragraph.ids) {
        decisionIds.add(decision);
        const examples = paragraph.examples;
        if (examples.length) {
          for (const surface of examples) {
            const scope = /(?:sema-only|compiler-private|never source syntax|not source syntax|cannot be written by source code)/i.test(paragraph.text)
              ? "internal"
              : "user";
            addCandidate(candidates, {
              decision_id: decision,
              symbol: null,
              surface,
              examples: [],
              scope,
              lifecycle: lifecycleFor("", paragraph.text),
              sourceEvidence: sourceItems,
            });
          }
        } else {
          const scope = /(?:sema-only|compiler-private|never source syntax|not source syntax|cannot be written by source code)/i.test(paragraph.text)
            ? "internal"
            : "user";
          addCandidate(candidates, {
            decision_id: decision,
            symbol: null,
            surface: null,
            examples: [],
            scope,
            lifecycle: lifecycleFor("", paragraph.text),
            sourceEvidence: sourceItems,
          });
        }
      }
    }
  }

  for (const decision of [...decisionIds].sort()) {
    if ([...candidates.values()].some((candidate) => candidate.decision_id === decision)) continue;
    addCandidate(candidates, {
      decision_id: decision,
      symbol: null,
      surface: null,
      examples: [],
      scope: "user",
      lifecycle: "current",
      sourceEvidence: decisionEvidence.get(decision) ?? [],
    });
  }

  const rows = [...candidates.values()]
    .map((candidate) => ({
      ...candidate,
      examples: [...candidate.examples].sort(),
      sourceEvidence: dedupeEvidence(candidate.sourceEvidence),
    }))
    .sort((a, b) => {
      const aKey = `${a.decision_id}\u0000${a.symbol ?? ""}\u0000${a.surface ?? ""}`;
      const bKey = `${b.decision_id}\u0000${b.symbol ?? ""}\u0000${b.surface ?? ""}`;
      return aKey.localeCompare(bKey);
    });
  assert(rows.length > 0, "no syntax decision rows found");
  return { rows, decisionIds: [...decisionIds].sort() };
}

function classifyCandidate(candidate, files) {
  const formKind = rowFormKind(candidate);
  assert(formKind && formKind !== "unclassified", `unclassified form ${candidate.decision_id}`);
  const baseReason = candidate.surface
    ? "No direct source consumer matched the declared spelling."
    : "The decision row has no concrete source spelling; coverage cannot be inferred.";
  if (candidate.scope === "internal") {
    const internalReason = "Compiler-owned metadata is not user-typeable and has no formatter or editor contract.";
    const internalEvidence = candidate.sourceEvidence;
    return {
      id: `syntax-${candidate.decision_id.toLowerCase()}-${stableCandidateSlug(candidate)}`,
      decision_id: candidate.decision_id,
      symbol: candidate.symbol,
      surface: candidate.surface,
      examples: candidate.examples,
      scope: candidate.scope,
      lifecycle: candidate.lifecycle,
      form_kind: formKind,
      source_evidence: candidate.sourceEvidence,
      coverage: {
        parser: notApplicableCell(internalReason, internalEvidence),
        formatter: {
          emission: notApplicableCell(internalReason, internalEvidence),
          roundtrip: notApplicableCell(internalReason, internalEvidence),
        },
        linter: {
          recognition: notApplicableCell(internalReason, internalEvidence),
          teaching: notApplicableCell(internalReason, internalEvidence),
        },
        language_server: notApplicableCell(internalReason, internalEvidence),
        canvas: notApplicableCell(internalReason, internalEvidence),
      },
    };
  }

  const parserEvidence = searchEvidence(files.parser, candidate, null);
  const formatterEvidence = searchEvidence(files.formatter, candidate, null);
  const roundtripEvidence = fixtureEvidence(files.fixtures, candidate);
  const recognitionEvidence = searchEvidence(
    files.linter,
    candidate,
    (line) => /\b(?:lint|LintPolicy|registered_lints|lint_rows|lint_by_name|prefer_[A-Za-z0-9_]*lint)\b/.test(line),
  );
  const teachingEvidence = searchEvidence(
    files.linter,
    candidate,
    (line, path) => /(?:Diagnostic|diagnostic|teach|retired|E\d{4}|L\d{4}|W\d{4})/.test(line) || /(?:\.stderr|\.warn)$/.test(path),
  );
  const languageServerEvidence = searchEvidence(files.languageServer, candidate, null);
  const canvasEvidence = searchEvidence(files.canvas, candidate, null);

  const noSurface = !candidate.surface;
  const parser = noSurface
    ? missingCell(baseReason)
    : coveredCell(parserEvidence, "A parser or lexer source line directly names this symbol or spelling.");
  const formatterEmission = noSurface
    ? missingCell(baseReason)
    : coveredCell(formatterEvidence, "A formatter source line directly names this symbol or spelling.");
  const formatterRoundtrip = noSurface
    ? missingCell(baseReason)
    : coveredCell(roundtripEvidence, "A formatter fixture calls the formatter and checks a stable second pass while naming this form.");
  const linterRecognition = noSurface
    ? missingCell(baseReason)
    : coveredCell(recognitionEvidence, "A linter policy or recognition source line directly names this symbol or spelling.");
  const linterTeaching = noSurface
    ? missingCell(baseReason)
    : coveredCell(teachingEvidence, "A diagnostic teaching path or diagnostic fixture directly names this symbol or spelling.");
  const languageServer = noSurface
    ? missingCell(baseReason)
    : coveredCell(languageServerEvidence, "An LSP or editor projection source line directly names this symbol or spelling.");
  const canvas = noSurface
    ? missingCell(baseReason)
    : coveredCell(canvasEvidence, "A Canvas projection source line directly names this symbol or spelling.");

  return {
    id: `syntax-${candidate.decision_id.toLowerCase()}-${stableCandidateSlug(candidate)}`,
    decision_id: candidate.decision_id,
    symbol: candidate.symbol,
    surface: candidate.surface,
    examples: candidate.examples,
    scope: candidate.scope,
    lifecycle: candidate.lifecycle,
    form_kind: formKind,
    source_evidence: candidate.sourceEvidence,
    coverage: {
      parser,
      formatter: { emission: formatterEmission, roundtrip: formatterRoundtrip },
      linter: { recognition: linterRecognition, teaching: linterTeaching },
      language_server: languageServer,
      canvas,
    },
  };
}

function coverageCells(row) {
  return {
    parser: row.coverage.parser,
    formatter_emission: row.coverage.formatter.emission,
    formatter_roundtrip: row.coverage.formatter.roundtrip,
    linter_recognition: row.coverage.linter.recognition,
    linter_teaching: row.coverage.linter.teaching,
    language_server: row.coverage.language_server,
    canvas: row.coverage.canvas,
  };
}

function summarize(rows) {
  const statuses = Object.fromEntries(COVERAGE_COLUMNS.map((column) => [column, {
    covered: 0,
    missing: 0,
    not_applicable: 0,
  }]));
  let failureCells = 0;
  for (const row of rows) {
    for (const [column, cell] of Object.entries(coverageCells(row))) {
      assert(STATUS_VALUES.has(cell.status), `unknown ${column} status on ${row.id}: ${cell.status}`);
      statuses[column][cell.status] += 1;
      if (cell.status === "missing") failureCells += 1;
    }
  }
  return { statuses, failureCells };
}

function astVariantRows(file, category, enumName) {
  const enumIndex = file.code.findIndex((line) => line.includes(`pub enum ${enumName} {`));
  assert(enumIndex >= 0, `missing ${enumName} enum in ${file.path}`);
  const rows = [];
  let depth = 1;
  let pendingDocs = [];
  for (let index = enumIndex + 1; index < file.code.length; index += 1) {
    const sourceLine = file.lines[index];
    const codeLine = file.code[index];
    if (depth === 1) {
      const doc = commentBody(sourceLine);
      if (doc !== null) {
        pendingDocs.push(doc);
      } else if (!sourceLine.trim()) {
        pendingDocs = [];
      } else {
        const match = codeLine.match(/^\s{4}([A-Za-z][A-Za-z0-9_]*)\s*(?:\{|\(|,)/);
        if (match) {
          const variant = match[1];
          const decisionIds = [...new Set(extractIds(`${pendingDocs.join(" ")} ${sourceLine}`))].sort();
          rows.push({
            id: `ast-${category}-${stableSurfaceSlug(variant)}`,
            category,
            enum_name: enumName,
            variant,
            decision_ids: decisionIds,
            source_evidence: [{
              kind: "ast:variant",
              file: file.path,
              line: index + 1,
              text: sourceLine.trim(),
            }],
          });
          pendingDocs = [];
        } else {
          pendingDocs = [];
        }
      }
    }
    depth += (codeLine.match(/\{/g) ?? []).length;
    depth -= (codeLine.match(/\}/g) ?? []).length;
    if (depth === 0) break;
  }
  assert(rows.length > 0, `no variants found in ${file.path}:${enumName}`);
  return rows;
}

function astCoverageCells(row) {
  return {
    formatter_emission: row.coverage.formatter.emission,
    formatter_roundtrip: row.coverage.formatter.roundtrip,
    language_server: row.coverage.language_server,
    canvas: row.coverage.canvas,
  };
}

function buildAstVariants(astFiles, files) {
  const variants = [];
  for (const definition of AST_ROOTS) {
    const file = astFiles.find((item) => item.path === definition.path);
    assert(file, `missing AST file: ${definition.path}`);
    for (const variant of astVariantRows(file, definition.category, definition.enum_name)) {
      const candidate = {
        symbol: variant.variant,
        surface: null,
        examples: [],
      };
      const formatterEvidence = searchEvidence(files.formatter, candidate, null);
      const roundtripEvidence = fixtureEvidence(files.fixtures, candidate);
      const languageServerEvidence = searchEvidence(files.languageServer, candidate, null);
      const canvasEvidence = searchEvidence(files.canvas, candidate, null);
      variants.push({
        ...variant,
        coverage: {
          formatter: {
            emission: coveredCell(formatterEvidence, "Formatter source directly names this AST variant."),
            roundtrip: coveredCell(roundtripEvidence, "A formatter fixture directly names this AST variant in its second-pass check."),
          },
          language_server: coveredCell(languageServerEvidence, "An LSP or editor projection source line directly names this AST variant."),
          canvas: coveredCell(canvasEvidence, "A Canvas projection source line directly names this AST variant."),
        },
      });
    }
  }
  return variants.sort((a, b) => a.id.localeCompare(b.id));
}

function summarizeAst(rows) {
  const columns = ["formatter_emission", "formatter_roundtrip", "language_server", "canvas"];
  const statuses = Object.fromEntries(columns.map((column) => [column, {
    covered: 0,
    missing: 0,
    not_applicable: 0,
  }]));
  let failureCells = 0;
  for (const row of rows) {
    for (const [column, cell] of Object.entries(astCoverageCells(row))) {
      assert(STATUS_VALUES.has(cell.status), `unknown AST ${column} status on ${row.id}: ${cell.status}`);
      statuses[column][cell.status] += 1;
      if (cell.status === "missing") failureCells += 1;
    }
  }
  return { statuses, failureCells };
}

function validateEvidence(data, filesByPath) {
  const checkItems = [];
  for (const row of data.rows) {
    checkItems.push(...row.source_evidence);
    for (const cell of Object.values(coverageCells(row))) checkItems.push(...cell.evidence);
  }
  for (const row of data.ast_variants ?? []) {
    checkItems.push(...row.source_evidence);
    for (const cell of Object.values(astCoverageCells(row))) checkItems.push(...cell.evidence);
  }
  for (const item of checkItems) {
    if (item.status === "missing") {
      assert(item.reason, "missing evidence has no reason");
      continue;
    }
    assert(item.file && Number.isInteger(item.line), "evidence lacks file/line");
    const file = filesByPath.get(item.file);
    assert(file, `evidence points to unknown path: ${item.file}`);
    assert(item.line >= 1 && item.line <= file.lines.length, `evidence line out of range: ${item.file}:${item.line}`);
    assert(item.text === file.lineText(item.line), `stale evidence text: ${item.file}:${item.line}`);
  }
}

const ROUNDTRIP_STATUS_VALUES = new Set(["pass", "fail", "unavailable", "uncovered"]);

function finalizeRoundtripCell(form, surface, cell) {
  const fix = cell.status === "pass"
    ? null
    : cell.status === "uncovered"
      ? `Add a real fixture for ${form.id} and exercise ${surface}`
      : cell.status === "unavailable"
        ? `Restore the compiler/projection command, then rerun ${surface}`
        : `Fix ${surface} so semantic tokens are preserved`;
  return {
    ...cell,
    surface,
    gap_id: cell.status === "pass" ? null : `syntax-roundtrip:${form.id}:${surface}`,
    fix,
  };
}

function validateRoundtripMatrix(matrix, label) {
  assert(matrix && Array.isArray(matrix.surfaces), `${label} matrix missing surfaces`);
  assert(JSON.stringify(matrix.surfaces) === JSON.stringify(REGISTERED_SURFACES), `${label} surfaces disagree`);
  const rows = matrix.forms ?? matrix.rows;
  for (const row of rows) {
    for (const surface of REGISTERED_SURFACES) {
      const cell = row.surfaces?.[surface];
      assert(cell, `${label} row ${row.id} missing ${surface} cell`);
      assert(ROUNDTRIP_STATUS_VALUES.has(cell.status), `${label} row ${row.id} has unknown ${surface} status ${cell.status}`);
      assert(typeof cell.program_path === "string" && cell.program_path.length > 0, `${label} row ${row.id} ${surface} lacks program path`);
      assert(cell.status === "pass" ? cell.gap_id === null && cell.fix === null : typeof cell.gap_id === "string" && typeof cell.fix === "string", `${label} row ${row.id} ${surface} gap/fix mismatch`);
    }
  }
  return rows;
}

function astMatrixStatusCounts(matrix) {
  const counts = Object.fromEntries(REGISTERED_SURFACES.map((surface) => [surface, {
    pass: 0,
    fail: 0,
    unavailable: 0,
    uncovered: 0,
  }]));
  let failureCells = 0;
  for (const row of matrix.rows) {
    for (const surface of REGISTERED_SURFACES) {
      const status = row.surfaces[surface].status;
      counts[surface][status] += 1;
      if (status !== "pass") failureCells += 1;
    }
  }
  return { counts, failureCells };
}

function validateAstRoundtripMatrix(matrix) {
  const rows = validateRoundtripMatrix(matrix, "AST round-trip");
  const summary = astMatrixStatusCounts(matrix);
  assert(JSON.stringify(summary.counts) === JSON.stringify(matrix.status_counts), "AST round-trip status counts disagree");
  assert(summary.failureCells === matrix.failure_cells, "AST round-trip failure count disagrees");
  return rows;
}

function validateRegisteredMatrix(matrix) {
  const rows = validateRoundtripMatrix(matrix, "registered syntax");
  const summary = registeredStatusCounts(rows);
  assert(JSON.stringify(summary.counts) === JSON.stringify(matrix.status_counts), "registered status counts disagree");
  assert(summary.failureCells === matrix.failure_cells, "registered failure count disagrees");
  return rows;
}

function validateData(data, filesByPath) {
  assert(data.schema_version === 1, "unsupported schema version");
  assert(typeof data.generated_static_only === "boolean", "static/dynamic census mode missing");
  assert(data.counts.rows === data.rows.length, "row count mismatch");
  assert(data.counts.tsv_rows === data.rows.length + 1, "TSV row count mismatch");
  assert(data.unclassified.length === 0, "unclassified forms are not allowed");
  const ids = new Set();
  for (const row of data.rows) {
    assert(row.id && !ids.has(row.id), `duplicate row identity: ${row.id}`);
    ids.add(row.id);
    assert(data.authoritative.decision_ids.includes(row.decision_id), `row has unknown decision: ${row.id}`);
    assert(row.source_evidence.length > 0, `row has no source evidence: ${row.id}`);
    assert(row.form_kind && row.form_kind !== "unclassified", `row form is unclassified: ${row.id}`);
  }
  assert(Array.isArray(data.ast_variants), "AST variant inventory missing");
  assert(data.counts.ast_variants === data.ast_variants.length, "AST variant count mismatch");
  const astIds = new Set();
  for (const row of data.ast_variants) {
    assert(row.id && !astIds.has(row.id), `duplicate AST variant identity: ${row.id}`);
    astIds.add(row.id);
    assert(["stmt", "expr"].includes(row.category), `unknown AST category: ${row.id}`);
    assert(row.enum_name && row.variant, `AST variant lacks enum/name: ${row.id}`);
    assert(row.source_evidence.length > 0, `AST variant has no source evidence: ${row.id}`);
    for (const [column, cell] of Object.entries(astCoverageCells(row))) {
      assert(STATUS_VALUES.has(cell.status), `unknown AST ${column} status on ${row.id}: ${cell.status}`);
    }
  }
  validateRegisteredMatrix(data.registered_syntax);
  validateAstRoundtripMatrix(data.ast_roundtrip);
  assert(data.examples_fmt_idempotence && Array.isArray(data.examples_fmt_idempotence.cells), "examples fmt idempotence gate missing");
  assert(data.criteria && data.criteria["1"] && data.criteria["2"] && data.criteria["3"], "card criteria report missing");
  validateEvidence(data, filesByPath);
  const summary = summarize(data.rows);
  assert(JSON.stringify(summary.statuses) === JSON.stringify(data.status_counts), "status counts disagree");
  assert(summary.failureCells === data.counts.failure_cells, "failure count disagrees");
}

function tsvCell(value) {
  return String(value ?? "")
    .replace(/\t/g, " ")
    .replace(/\r?\n/g, " ")
    .replace(/\\/g, "\\\\");
}

function evidenceRefs(items) {
  return items
    .map((item) => item.status === "missing" ? `MISSING:${item.reason}` : `${item.file}:${item.line}`)
    .join(";");
}

function sourceRefs(items) {
  return items.map((item) => `${item.file}:${item.line}`).join(";");
}

function buildTsv(rows) {
  const headers = [
    "id",
    "decision_id",
    "scope",
    "lifecycle",
    "form_kind",
    "symbol",
    "surface",
    "examples",
    "source_evidence",
    "parser_status",
    "parser_evidence",
    "formatter_emission_status",
    "formatter_emission_evidence",
    "formatter_roundtrip_status",
    "formatter_roundtrip_evidence",
    "linter_recognition_status",
    "linter_recognition_evidence",
    "linter_teaching_status",
    "linter_teaching_evidence",
    "language_server_status",
    "language_server_evidence",
    "canvas_status",
    "canvas_evidence",
  ];
  const lines = [headers.join("\t")];
  for (const row of rows) {
    const cells = coverageCells(row);
    lines.push([
      row.id,
      row.decision_id,
      row.scope,
      row.lifecycle,
      row.form_kind,
      row.symbol,
      row.surface,
      row.examples.join(" | "),
      sourceRefs(row.source_evidence),
      cells.parser.status,
      evidenceRefs(cells.parser.evidence),
      cells.formatter_emission.status,
      evidenceRefs(cells.formatter_emission.evidence),
      cells.formatter_roundtrip.status,
      evidenceRefs(cells.formatter_roundtrip.evidence),
      cells.linter_recognition.status,
      evidenceRefs(cells.linter_recognition.evidence),
      cells.linter_teaching.status,
      evidenceRefs(cells.linter_teaching.evidence),
      cells.language_server.status,
      evidenceRefs(cells.language_server.evidence),
      cells.canvas.status,
      evidenceRefs(cells.canvas.evidence),
    ].map(tsvCell).join("\t"));
  }
  return `${lines.join("\n")}\n`;
}

function roundtripTsv(matrix) {
  const headers = ["registry", "index", "id", "spelling", "surface", "program_path", "status", "reason", "gap_id", "fix", "command", "evidence"];
  const lines = [headers.join("\t")];
  const rows = matrix.forms ?? matrix.rows;
  for (const row of rows) {
    for (const surface of REGISTERED_SURFACES) {
      const cell = row.surfaces[surface];
      lines.push([
        matrix.registry,
        row.index ?? row.id,
        row.id,
        row.spelling ?? row.variant,
        surface,
        cell.program_path,
        cell.status,
        cell.reason,
        cell.gap_id,
        cell.fix,
        JSON.stringify(cell.command ?? null),
        JSON.stringify(cell.evidence ?? []),
      ].map(tsvCell).join("\t"));
    }
  }
  return `${lines.join("\n")}\n`;
}
function buildMatrixTsv(data) {
  const registered = roundtripTsv(data.registered_syntax);
  const ast = roundtripTsv(data.ast_roundtrip);
  return `${registered}${ast.slice(ast.indexOf("\n") + 1)}`;
}


function buildMatrixMarkdown(data) {
  const syntaxRows = data.registered_syntax.forms.flatMap((row) => REGISTERED_SURFACES.map((surface) => {
    const cell = row.surfaces[surface];
    return [row.registry_symbol, row.index, row.spelling, surface, cell.program_path, cell.status, cell.gap_id ?? "", cell.fix ?? ""];
  }));
  const astRows = data.ast_roundtrip.rows.flatMap((row) => REGISTERED_SURFACES.map((surface) => {
    const cell = row.surfaces[surface];
    return [row.id, row.variant, surface, cell.program_path, cell.status, cell.gap_id ?? "", cell.fix ?? ""];
  }));
  return `# Syntax round-trip matrix

Registry: \`${data.registered_syntax.registry}\`

Examples fmt idempotence: **${data.examples_fmt_idempotence.status}** (${data.examples_fmt_idempotence.checked}/${data.examples_fmt_idempotence.total ?? data.examples_fmt_idempotence.checked} programs checked).

${markdownTable(["registry row", "index", "spelling", "surface", "program", "status", "gap", "fix"], syntaxRows)}

## AST

${markdownTable(["AST row", "variant", "surface", "program", "status", "gap", "fix"], astRows)}
`;
}

function markdownTable(headers, rows) {
  const line = (values) => `| ${values.map((value) => String(value).replaceAll("|", "\\|")).join(" | ")} |`;
  return [line(headers), line(headers.map(() => "---")), ...rows.map(line)].join("\n");
}

function roundtripStatus(cell) {
  return `${cell.status} — ${cell.program_path}${cell.gap_id ? ` — ${cell.gap_id}` : ""}`;
}

function buildRoundtripMarkdown(data) {
  const syntaxRows = data.registered_syntax.forms.map((row) => [
    `\`${row.registry_symbol}\``,
    row.index,
    row.spelling,
    roundtripStatus(row.surfaces.jet_fmt),
    roundtripStatus(row.surfaces.language_server),
    roundtripStatus(row.surfaces.canvas),
  ]);
  const astRows = data.ast_roundtrip.rows.map((row) => [
    `\`${row.id}\``,
    row.variant,
    roundtripStatus(row.surfaces.jet_fmt),
    roundtripStatus(row.surfaces.language_server),
    roundtripStatus(row.surfaces.canvas),
  ]);
  return `## Executable syntax round-trip matrix

The machine-readable matrix is [JSON](../../${FIXTURE_MATRIX_JSON}) and [TSV](../../${FIXTURE_MATRIX_TSV}). The registry source is \`${data.registered_syntax.registry}\` in \`${data.registered_syntax.registry_source}\`; every row has all three executable surface cells.

Registered forms: **${data.registered_syntax.forms.length}**; non-pass cells: **${data.registered_syntax.failure_cells}**; uncovered cells: **${data.counts.registered_uncovered_cells}**; unavailable cells: **${data.counts.registered_unavailable_cells}**.

${markdownTable(["registry row", "index", "spelling", "jet fmt", "language server", "Canvas"], syntaxRows)}

AST round-trip variants: **${data.ast_roundtrip.rows.length}**; non-pass cells: **${data.ast_roundtrip.failure_cells}**.

${markdownTable(["AST row", "variant", "jet fmt", "language server", "Canvas"], astRows)}

`;
}

function buildMarkdown(data) {
  const statusRows = COVERAGE_COLUMNS.map((column) => [
    column,
    data.status_counts[column].covered,
    data.status_counts[column].missing,
    data.status_counts[column].not_applicable,
  ]);
  const formRows = data.rows.map((row) => {
    const cells = coverageCells(row);
    const short = (cell) => cell.status;
    return [
      `\`${row.id}\``,
      `\`${row.decision_id}\``,
      row.scope,
      row.lifecycle,
      row.form_kind,
      row.surface ?? "(no concrete spelling)",
      short(cells.parser),
      short(cells.formatter_emission),
      short(cells.formatter_roundtrip),
      short(cells.linter_recognition),
      short(cells.linter_teaching),
      short(cells.language_server),
      short(cells.canvas),
      sourceRefs(row.source_evidence),
    ];
  });
  const astStatusRows = Object.entries(data.ast_status_counts).map(([column, counts]) => [
    column,
    counts.covered,
    counts.missing,
    counts.not_applicable,
  ]);
  const astRows = data.ast_variants.map((row) => {
    const cells = astCoverageCells(row);
    return [
      `\`${row.id}\``,
      row.category,
      row.enum_name,
      row.variant,
      row.decision_ids.join(", ") || "(none)",
      cells.formatter_emission.status,
      cells.formatter_roundtrip.status,
      cells.language_server.status,
      cells.canvas.status,
      sourceRefs(row.source_evidence),
    ];
  });
  return `# Executable syntax round-trip census — ${DATE}

## Result

This report inventories decision-backed syntax declarations and source examples from the canonical Syntax module, then executes the formatter, language server, and Canvas projection probes. The static coverage inventory and dynamic round-trip matrices are recorded together.


Command:

\`\`\`text
${data.generated_command}
\`\`\`

\`--matrix\` is the coverage gate: it executes every registered-form and AST-variant cell and exits nonzero until all three surfaces pass. \`--check\` only verifies that these generated reports are present and fresh; it prints the recorded non-pass counts but does not replace the coverage gate.

The census has **${data.counts.rows} rows** for **${data.counts.decision_ids} decision IDs**. The TSV has **${data.counts.tsv_rows - 1} data rows** plus one header (**${data.counts.tsv_rows} rows total**). Exhaustive evidence is in [JSON](syntax-roundtrip-census-${DATE}.json) and [TSV](syntax-roundtrip-census-${DATE}.tsv).
A status is \`covered\` only when a source line directly names the symbol or spelling. \`missing\` is an explicit static failure and remains missing; it is never inferred from a wildcard, default, neighboring form, or prose comment. \`not_applicable\` is reserved for compiler-owned metadata that cannot be written by a user. Dynamic matrix cells are independently recorded as \`pass\`, \`fail\`, \`unavailable\`, or \`uncovered\`.

## Coverage status

${markdownTable(["area", "covered", "missing", "not applicable"], statusRows)}

Static coverage failure cells (all \`missing\` states): **${data.counts.failure_cells}**.

## Form census

${markdownTable(["row", "decision", "scope", "lifecycle", "kind", "surface", "parser", "fmt emission", "fmt roundtrip", "lint recognition", "lint teaching", "language server", "Canvas", "source evidence"], formRows)}

## AST variant inventory

The AST gate enumerates every direct \`Stmt\` and \`Expr\` enum variant. Each variant has independent formatter emission, formatter round-trip, language-server, and Canvas cells. A new variant changes the authoritative ID list, so \`--check\` detects a stale generated inventory and \`--matrix\` fails until every cell is reviewed and passes.

\`${data.counts.ast_variants} AST variants\`; AST failure cells: **${data.counts.ast_failure_cells}**.

${markdownTable(["AST area", "covered", "missing", "not applicable"], astStatusRows)}

${markdownTable(["AST row", "category", "enum", "variant", "decision IDs", "fmt emission", "fmt roundtrip", "language server", "Canvas", "source evidence"], astRows)}
${buildRoundtripMarkdown(data)}

## Sources and limits

- Authoritative syntax rows: \`${SYNTAX_ROOT}\` and its private modules under \`${SYNTAX_MODULE_DIR}/\`.
- AST variant authority: \`crates/jet-foundation/src/AST/statements.rs\` and \`crates/jet-foundation/src/AST/expressions.rs\`.
- Parser and lexer projection: \`crates/jet-parser/src\` and \`crates/jet-lexer/src\`.
- Formatter projection and fixtures: \`crates/jet-parser/src/Formatter\` and formatter tests under \`tests/\`.
- Linter recognition and teaching paths: \`crates/jet-foundation/src\`, \`crates/jet-sema/src\`, \`crates/jet-parser/src\`, and diagnostic fixtures under \`tests/\`.
- Language-server/editor projection: \`Source/LSP\` and \`editors/\`.
- Canvas projection: \`crates/jet-devserver/src/Canvas\` and \`crates/jet-canvas/src\`.

Every static evidence item stores the exact repository-relative file, line, and source text. The executable matrix records the command result for every registered form and AST variant; \`unavailable\` and \`uncovered\` cells are fail-closed and retain a deterministic gap ID.
`;
}

function buildData() {
  const syntaxPaths = [
    SYNTAX_ROOT,
    ...walkFiles(SYNTAX_MODULE_DIR, new Set([".rs"])),
  ];
  const astPaths = AST_ROOTS.map((definition) => definition.path);
  const parserPaths = [
    ...walkFiles("crates/jet-parser/src", new Set([".rs"])),
    ...walkFiles("crates/jet-lexer/src", new Set([".rs"])),
  ];
  const fixturePaths = [
    "tests/fmt.rs",
    "tests/support/fmt_lossless.rs",
    ...walkFiles("crates/jet-parser/src", new Set([".rs"])),
  ];
  const linterPaths = [
    ...walkFiles("crates/jet-parser/src", new Set([".rs"])),
    ...walkFiles("crates/jet-sema/src", new Set([".rs"])),
    ...walkFiles("crates/jet-foundation/src", new Set([".rs"])),
    ...walkFiles("tests/ui_lint", new Set([".jet", ".stderr", ".warn"])),
    ...walkFiles("tests/ui", new Set([".jet", ".stderr", ".warn"])),
  ];
  const languageServerPaths = [
    ...walkFiles("Source/LSP", new Set([".rs"])),
    "editors/jet.tmGrammar",
    ...walkFiles("editors", new Set([".js", ".json", ".scm", ".toml", ".xml", ".grammar"])),
  ];
  const canvasPaths = [
    ...walkFiles("crates/jet-devserver/src/Canvas", new Set([".rs"])),
    ...walkFiles("crates/jet-canvas/src", new Set([".rs", ".js"])),
  ];

  const allPaths = [...new Set([
    ...astPaths,
    ...syntaxPaths,
    ...parserPaths,
    ...fixturePaths,
    ...linterPaths,
    ...languageServerPaths,
    ...canvasPaths,
  ])].sort();
  const filesByPath = new Map(allPaths.map((path) => [path, makeFile(path)]));
  const astFiles = astPaths.map((path) => filesByPath.get(path));
  const syntaxFiles = syntaxPaths.map((path) => filesByPath.get(path));
  const parserFiles = parserPaths.map((path) => filesByPath.get(path));
  const fixtureFiles = fixturePaths.map((path) => filesByPath.get(path));
  const linterFiles = linterPaths.map((path) => filesByPath.get(path));
  const languageServerFiles = languageServerPaths.map((path) => filesByPath.get(path));
  const canvasFiles = canvasPaths.map((path) => filesByPath.get(path));
  const candidateResult = buildCandidates(syntaxFiles);
  const rows = candidateResult.rows.map((candidate) => classifyCandidate(candidate, {
    parser: parserFiles,
    formatter: parserFiles.filter((file) => file.path.includes("/Formatter/") || file.path.endsWith("/Formatter/mod.rs")),
    fixtures: fixtureFiles,
    linter: linterFiles,
    languageServer: languageServerFiles,
    canvas: canvasFiles,
  }));
  const astRows = buildAstVariants(astFiles, {
    formatter: parserFiles.filter((file) => file.path.includes("/Formatter/") || file.path.endsWith("/Formatter/mod.rs")),
    fixtures: fixtureFiles,
    languageServer: languageServerFiles,
    canvas: canvasFiles,
  });
  const registeredSyntax = buildRegisteredMatrix();
  const astRoundtrip = buildAstRoundtripMatrix(astRows);
  const rowIds = new Set();
  for (const row of rows) {
    assert(!rowIds.has(row.id), `duplicate stable identity: ${row.id}`);
    rowIds.add(row.id);
  }
  const summary = summarize(rows);
  const astSummary = summarizeAst(astRows);
  const examplesFmt = runExamplesFmtGate();
  const criteria = buildCriteria(registeredSyntax, astRoundtrip, examplesFmt);
  const data = {
    schema_version: 1,
    generated_command: GENERATED_COMMAND,
    generated_static_only: false,
    census_date: DATE,
    authoritative: {
      syntax_root: SYNTAX_ROOT,
      syntax_modules: syntaxPaths.slice(1),
      decision_ids: candidateResult.decisionIds,
      parser_roots: ["crates/jet-parser/src", "crates/jet-lexer/src"],
      formatter_roots: ["crates/jet-parser/src/Formatter", "tests/fmt.rs"],
      linter_roots: ["crates/jet-foundation/src", "crates/jet-sema/src", "crates/jet-parser/src", "tests"],
      language_server_roots: ["Source/LSP", "editors"],
      canvas_roots: ["crates/jet-devserver/src/Canvas", "crates/jet-canvas/src"],
      ast_roots: AST_ROOTS.map((definition) => definition.path),
      ast_variant_ids: astRows.map((row) => row.id),
    },
    counts: {
      rows: rows.length,
      decision_ids: candidateResult.decisionIds.length,
      source_files: syntaxPaths.length,
      tsv_rows: rows.length + 1,
      failure_cells: summary.failureCells,
      ast_variants: astRows.length,
      ast_failure_cells: astSummary.failureCells,
      registered_forms: registeredSyntax.forms.length,
      registered_failure_cells: registeredSyntax.failure_cells,
      registered_unavailable_cells: Object.values(registeredSyntax.status_counts).reduce((total, counts) => total + counts.unavailable, 0),
      registered_uncovered_cells: Object.values(registeredSyntax.status_counts).reduce((total, counts) => total + counts.uncovered, 0),
      ast_roundtrip_failure_cells: astRoundtrip.failure_cells,
    },
    ast_status_counts: astSummary.statuses,
    status_counts: summary.statuses,
    rows,
    ast_variants: astRows,
    registered_syntax: registeredSyntax,
    ast_roundtrip: astRoundtrip,
    examples_fmt_idempotence: examplesFmt,
    criteria,
    unclassified: [],
  };
  assert(data.authoritative.ast_variant_ids.length === data.ast_variants.length, "authoritative AST inventory mismatch");
  assert(JSON.stringify(data.authoritative.ast_variant_ids) === JSON.stringify(data.ast_variants.map((row) => row.id)), "AST variant IDs disagree");
  const checkedAstSummary = summarizeAst(data.ast_variants);
  assert(JSON.stringify(checkedAstSummary.statuses) === JSON.stringify(data.ast_status_counts), "AST status counts disagree");
  assert(checkedAstSummary.failureCells === data.counts.ast_failure_cells, "AST failure count disagrees");
  validateData(data, filesByPath);
  return {
    data,
    tsv: buildTsv(rows),
    markdown: buildMarkdown(data),
    matrixJson: `${JSON.stringify({
      schema_version: 1,
      registry: data.registered_syntax,
      ast: data.ast_roundtrip,
      examples_fmt_idempotence: data.examples_fmt_idempotence,
      criteria: data.criteria,
    }, null, 2)}\n`,
    matrixTsv: buildMatrixTsv(data),
    matrixMarkdown: buildMatrixMarkdown(data),
    filesByPath,
  };
}
function usage() {
  return `Usage: ${GENERATED_COMMAND}\n       node scripts/agent/syntax-roundtrip-census.mjs --check\n       node scripts/agent/syntax-roundtrip-census.mjs --matrix [--json]\n       node scripts/agent/syntax-roundtrip-census.mjs --help\n\nModes:\n  --write  regenerate the dated JSON, TSV, Markdown, and executable matrix reports (default)\n  --check  fail when any generated report is missing or stale\n  --matrix run the executable matrix and fail closed unless every cell passes\n  --json   with --matrix, emit the registered/AST matrix JSON on stdout\n  --help   print this help\n`;
}
function parseMode() {
  const args = process.argv.slice(2);
  if (args.includes("--help")) return { mode: "help", json: false };
  const unknown = args.filter((arg) => !["--write", "--check", "--matrix", "--json"].includes(arg));
  assert(unknown.length === 0, `unknown option(s): ${unknown.join(", ")}`);
  const modes = args.filter((arg) => arg === "--write" || arg === "--check" || arg === "--matrix");
  assert(modes.length <= 1, "choose only one of --write, --check, or --matrix");
  return { mode: modes[0] ?? "--write", json: args.includes("--json") };
}

function main() {
  const options = parseMode();
  if (options.mode === "help") {
    process.stdout.write(usage());
    return;
  }
  const { data, tsv, markdown, matrixJson, matrixTsv, matrixMarkdown } = buildData();
  const json = `${JSON.stringify(data, null, 2)}\n`;
  const outputs = [
    [OUTPUT_JSON, json],
    [OUTPUT_TSV, tsv],
    [OUTPUT_MD, markdown],
    [FIXTURE_MATRIX_JSON, matrixJson],
    [FIXTURE_MATRIX_TSV, matrixTsv],
    [FIXTURE_MATRIX_MD, matrixMarkdown],
  ];
  mkdirSync(absPath(FIXTURE_ROOT), { recursive: true });
  if (options.mode === "--matrix") {
    for (const [path, contents] of outputs.slice(3)) writeFileSync(absPath(path), contents);
    if (options.json) process.stdout.write(matrixJson);
    else process.stdout.write(`syntax roundtrip matrix: registered=${data.registered_syntax.forms.length}; registered non-pass=${data.registered_syntax.failure_cells}; AST non-pass=${data.ast_roundtrip.failure_cells}\n`);
    if (!data.registered_syntax.closed || !data.ast_roundtrip.closed) {
      fail("syntax roundtrip matrix is not closed: every registered form and AST variant needs pass cells");
    }
    return;
  }
  if (options.mode === "--check") {
    for (const [path, expected] of outputs) {
      const actual = existsSync(absPath(path)) ? readFileSync(absPath(path), "utf8") : null;
      if (actual !== expected) fail(`${path} is stale or missing; run ${GENERATED_COMMAND}`);
    }
    process.stdout.write(`syntax roundtrip census: checked ${data.counts.rows} rows; failure cells=${data.counts.failure_cells}; registered non-pass=${data.registered_syntax.failure_cells}; AST non-pass=${data.ast_roundtrip.failure_cells}; examples fmt non-pass=${data.examples_fmt_idempotence.cells.filter((cell) => cell.status !== "pass").length}/${data.examples_fmt_idempotence.total}; --matrix is the coverage gate\n`);
    return;
  }
  for (const [path, contents] of outputs) writeFileSync(absPath(path), contents);
  process.stdout.write(`syntax roundtrip census: wrote ${OUTPUT_JSON}, ${OUTPUT_TSV}, ${OUTPUT_MD}, ${FIXTURE_MATRIX_JSON}, ${FIXTURE_MATRIX_TSV}, ${FIXTURE_MATRIX_MD}; rows=${data.counts.rows}; failure cells=${data.counts.failure_cells}\n`);
}

try {
  main();
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
