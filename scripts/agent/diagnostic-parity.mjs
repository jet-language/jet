#!/usr/bin/env node

import {
  existsSync,
  readFileSync,
  readdirSync,
  statSync,
} from "node:fs";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const REGISTRY = join(ROOT, "crates/jet-codegen/src/Prelude/Diagnostics.jet");
const CORPUS = join(ROOT, "scripts/agent/diagnostic-parity-corpus.json");
// Registry rows are the canonical explain fields. Website output is the only
// checked-in explain projection; do not add a Markdown row mirror here.
const EXPLAIN = [join(ROOT, "site/dist/e")];
const SNAPSHOTS = [
  join(ROOT, "tests/ui"),
  join(ROOT, "tests/ui_lint"),
  join(ROOT, "tests/cli"),
  join(ROOT, "tests/release"),
  join(ROOT, "tests/fixtures/cli-diagnostics"),
  join(ROOT, "tests/fixtures/jetpack-diagnostics"),
  join(ROOT, "tests/fixtures/module-eval-diagnostics"),
];

export const SCHEMA = "jet.diagnostic-parity.v1";
export const CORPUS_SCHEMA = "jet.diagnostic-parity.corpus.v1";
export const FIELD_IDS = Object.freeze([
  "code",
  "what",
  "why",
  "fix",
  "span",
  "actionable_edit",
]);
export const REQUIRED_PRODUCT_FIELDS = Object.freeze(["code", "what", "why", "fix"]);
const DIAGNOSTIC_CODE = /^[A-Z][A-Z0-9-]*$/u;
const EXPLAIN_EXTENSIONS = new Set([".html", ".json", ".jsonl", ".md", ".txt"]);
const SNAPSHOT_EXTENSIONS = new Set([".json", ".jsonl", ".stderr", ".txt", ".warn"]);
const PEER_FIELD_STATES = new Set(["present", "absent", "unknown"]);
const FEATURE_STATUS_STATES = new Set(["unmeasured", "unknown", "unsupported"]);

// D-REPORT-HOME1/A: the feature census is a manifest, not a result ledger.
// Keep the card's 19 names here so a missing or duplicate row fails closed
// before any peer comparison can be mistaken for a measurement.
export const REQUIRED_FEATURES = Object.freeze([
  Object.freeze({ id: "benchmark-history-baselines", feature: "Statistics-driven benchmark history and named baselines" }),
  Object.freeze({ id: "workspace-command-lock-cache", feature: "Workspace-wide command with shared lock and target cache" }),
  Object.freeze({ id: "named-feature-flags", feature: "Named feature flags with additive defaults and explicit activation" }),
  Object.freeze({ id: "machine-readable-build-diagnostic-events", feature: "Machine-readable build and diagnostic event stream" }),
  Object.freeze({ id: "contextual-inlay-hints", feature: "Contextual inlay hints for inferred types and argument names" }),
  Object.freeze({ id: "automatic-imports-completion-actions", feature: "Automatic imports and completion actions" }),
  Object.freeze({ id: "editor-lightbulb-refactoring", feature: "Editor lightbulb assists and local refactoring" }),
  Object.freeze({ id: "cursor-actions", feature: "Run, benchmark, related-test, and rename actions at cursor" }),
  Object.freeze({ id: "interactive-flamegraph", feature: "One-command interactive flamegraph profiling" }),
  Object.freeze({ id: "multiline-labels-width-aware", feature: "Multi-file and multiline labels with width-aware rendering" }),
  Object.freeze({ id: "build-script-help-options", feature: "Build-script options appear in generated help" }),
  Object.freeze({ id: "go-test-filters", feature: "Go test filter, list, and package-aware execution" }),
  Object.freeze({ id: "interleaved-build-test-events", feature: "Machine-readable interleaved build and test events" }),
  Object.freeze({ id: "multi-module-workspace-sync", feature: "Multi-module workspace with use/sync commands" }),
  Object.freeze({ id: "interactive-profile-views", feature: "Interactive profile analysis with top/list/web/disasm views" }),
  Object.freeze({ id: "official-lsp", feature: "Official LSP with diagnostics, completion, analysis, and refactoring" }),
  Object.freeze({ id: "named-background-processes", feature: "Named background processes and task execution in the development environment" }),
  Object.freeze({ id: "builtin-lsp-semantic-source", feature: "Built-in LSP with one semantic source of truth" }),
  Object.freeze({ id: "package-workspace-authority", feature: "Package/workspace authority and reproducible environment boundary" }),
]);

class ParityInputError extends Error {
  constructor(message) {
    super(`diagnostic parity: ${message}`);
    this.name = "ParityInputError";
  }
}

function inputError(message) {
  throw new ParityInputError(message);
}

function nonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function normalizeText(value) {
  return String(value ?? "")
    .replace(/\r\n?/gu, "\n")
    .replace(/[ \t]+/gu, " ")
    .trim();
}

function normalizedEqual(left, right) {
  return normalizeText(left) === normalizeText(right);
}

function displayPath(path) {
  const rel = relative(ROOT, path).replaceAll("\\", "/");
  return rel || ".";
}

function absolutePath(value) {
  return resolve(ROOT, value);
}

function readInput(path, label) {
  if (!existsSync(path)) inputError(`missing ${label}: ${displayPath(path)}`);
  try {
    return readFileSync(path, "utf8");
  } catch (error) {
    inputError(`cannot read ${label} ${displayPath(path)}: ${error.message}`);
  }
}

function walk(path) {
  if (!existsSync(path)) return [];
  const stat = statSync(path);
  if (stat.isFile()) return [path];
  if (!stat.isDirectory()) return [];
  const files = [];
  for (const entry of readdirSync(path, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) files.push(...walk(child));
    else if (entry.isFile()) files.push(child);
  }
  return files.sort((left, right) => left.localeCompare(right));
}

function filesFor(paths, extensions, label) {
  const files = [];
  for (const path of paths) {
    if (!existsSync(path)) inputError(`missing ${label}: ${displayPath(path)}`);
    for (const file of walk(path)) {
      if (extensions.has(extname(file).toLowerCase())) files.push(file);
    }
  }
  return [...new Set(files)].sort((left, right) => left.localeCompare(right));
}

function codeKey(code) {
  const numeric = code.match(/^([A-Z]+)(\d{4})$/u);
  return numeric
    ? [numeric[1], 0, Number(numeric[2]), code]
    : [code, 1, 0, code];
}

function compareCodes(left, right) {
  const a = codeKey(left);
  const b = codeKey(right);
  for (let index = 0; index < a.length; index += 1) {
    if (a[index] < b[index]) return -1;
    if (a[index] > b[index]) return 1;
  }
  return 0;
}

function parseRegistry(text, sourcePath) {
  const rows = [];
  const errors = [];
  const seen = new Map();
  for (const [index, rawLine] of text.split(/\r?\n/u).entries()) {
    const line = rawLine.replace(/\r$/u, "");
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("//")) continue;
    const fields = line.split("\t");
    const reasons = [];
    if (fields.length !== 12 && fields.length !== 13) {
      reasons.push(`expected 12 or 13 tab-separated fields, found ${fields.length}`);
    }
    if (fields[0] !== "diagnostic") reasons.push("first field must be diagnostic");
    const code = fields[1] ?? "";
    if (!DIAGNOSTIC_CODE.test(code)) reasons.push("code is missing or malformed");
    if (!fields[5]) reasons.push("status is empty");
    for (const [fieldIndex, name] of [[7, "what"], [8, "why"], [9, "fix"]]) {
      if (!nonEmptyString(fields[fieldIndex])) reasons.push(`${name} is empty`);
    }
    if (fields[3] === "lint" && fields.length !== 13) reasons.push("lint rows require a lint-name field");
    if (fields[3] === "error" && fields.length !== 12) reasons.push("error rows must not have a lint-name field");
    if (reasons.length > 0) {
      errors.push({ source: displayPath(sourcePath), line: index + 1, reasons });
      if (fields.length < 10 || !DIAGNOSTIC_CODE.test(code)) continue;
    }
    if (seen.has(code)) {
      errors.push({
        source: displayPath(sourcePath),
        line: index + 1,
        reasons: [`duplicate code; first declared at line ${seen.get(code)}`],
      });
      continue;
    }
    seen.set(code, index + 1);
    rows.push({
      code,
      stage: fields[2] ?? "",
      severity: fields[3] ?? "",
      moment: fields[4] ?? "",
      status: fields[5] ?? "",
      meaning: fields[6] ?? "",
      what: fields[7] ?? "",
      why: fields[8] ?? "",
      fix: fields[9] ?? "",
      detail: fields[10] ?? "",
      structured_fix: fields[11] ?? "",
      lint_name: fields[12] ?? null,
      source: displayPath(sourcePath),
      source_line: index + 1,
    });
  }
  rows.sort((left, right) => compareCodes(left.code, right.code));
  return { rows, errors };
}

function emptyFields() {
  return Object.fromEntries(FIELD_IDS.map((field) => [field, {
    present: false,
    sources: [],
    values: [],
  }]));
}

function addFieldValue(fields, field, value, source) {
  if (!FIELD_IDS.includes(field)) return;
  const entry = fields[field];
  if (value === undefined || value === null || value === false) return;
  if (typeof value === "string" && value.trim() === "") return;
  entry.present = true;
  if (source && !entry.sources.includes(source)) entry.sources.push(source);
  if (typeof value === "string") {
    const normalized = normalizeText(value);
    if (normalized && !entry.values.includes(normalized)) entry.values.push(normalized);
  }
}

function spanValue(value) {
  if (value === true) return true;
  if (!value || typeof value !== "object") return null;
  if (Array.isArray(value)) return value.length > 0 ? value[0] : null;
  if (value.start !== undefined || value.end !== undefined) return value;
  if (value.span && typeof value.span === "object") return value.span;
  if (value.location && typeof value.location === "object") return value.location;
  if (Array.isArray(value.spans) && value.spans.length > 0) return value.spans[0];
  return null;
}

function actionableValue(value) {
  if (value === true) return true;
  if (Array.isArray(value)) return value.length > 0;
  if (value && typeof value === "object") return true;
  return false;
}

function extractJsonRecord(value, source) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const nested = value.diagnostic && typeof value.diagnostic === "object" ? value.diagnostic : value;
  const code = [nested.code, nested.diagnostic_code, value.code]
    .find((candidate) => typeof candidate === "string" && DIAGNOSTIC_CODE.test(candidate));
  if (!code) return null;
  const fields = emptyFields();
  addFieldValue(fields, "code", code, source);
  addFieldValue(fields, "what", nested.what ?? nested.message, source);
  addFieldValue(fields, "why", nested.why ?? nested.reason ?? nested.note, source);
  addFieldValue(fields, "fix", nested.fix ?? nested.help, source);
  const span = spanValue(nested.span ?? nested.location ?? nested.spans);
  if (span) addFieldValue(fields, "span", true, source);
  const edits = nested.fix_edits ?? nested.edits ?? nested.actionable_edit;
  if (actionableValue(edits)) addFieldValue(fields, "actionable_edit", true, source);
  return {
    code,
    fields,
    source,
    link: nested.more ?? nested.link ?? null,
  };
}

function jsonRecords(value, source) {
  if (Array.isArray(value)) return value.flatMap((item) => jsonRecords(item, source));
  if (!value || typeof value !== "object") return [];
  const direct = extractJsonRecord(value, source);
  const records = direct ? [direct] : [];
  for (const key of ["diagnostics", "reports", "rows", "explain", "items", "results"]) {
    if (Array.isArray(value[key])) records.push(...value[key].flatMap((item) => jsonRecords(item, source)));
  }
  if (!direct) {
    for (const [key, item] of Object.entries(value)) {
      if (DIAGNOSTIC_CODE.test(key) && item && typeof item === "object") {
        records.push(...jsonRecords({ code: key, ...item }, source));
      }
    }
  }
  return records;
}

function parseJsonRecords(text, source) {
  try {
    return jsonRecords(JSON.parse(text), source);
  } catch {
    const records = [];
    for (const line of text.split(/\r?\n/u)) {
      if (!line.trim()) continue;
      try {
        records.push(...jsonRecords(JSON.parse(line), source));
      } catch {
        // A text export can contain prose around JSON lines. The text parser
        // below remains the source of truth for those files.
      }
    }
    return records;
  }
}

const HTML_ENTITIES = Object.freeze({
  amp: "&",
  apos: "'",
  gt: ">",
  lt: "<",
  nbsp: " ",
  quot: '"',
});

function decodeHtml(value) {
  return value
    .replace(/&#x([0-9a-f]+);/giu, (_, hex) => String.fromCodePoint(Number.parseInt(hex, 16)))
    .replace(/&#(\d+);/gu, (_, decimal) => String.fromCodePoint(Number.parseInt(decimal, 10)))
    .replace(/&([a-z]+);/giu, (match, name) => HTML_ENTITIES[name.toLowerCase()] ?? match);
}

function htmlText(value) {
  return normalizeText(decodeHtml(value.replace(/<br\s*\/?>/giu, "\n").replace(/<[^>]*>/gu, "")));
}

function parseHtmlExplain(text, source) {
  const records = [];
  const pages = [...text.matchAll(/<h1[^>]*>\s*([A-Z][A-Z0-9-]*)\s*<\/h1>/giu)];
  for (const [index, page] of pages.entries()) {
    const code = page[1].toUpperCase();
    const start = page.index ?? 0;
    const end = pages[index + 1]?.index ?? text.length;
    const body = text.slice(start, end);
    const fields = emptyFields();
    addFieldValue(fields, "code", code, source);
    for (const field of ["what", "why", "fix"]) {
      const match = body.match(new RegExp(
        `data-registry-field=["']${field}["'][^>]*>([\\s\\S]*?)<\\/[^>]+>`,
        "iu",
      ));
      if (match) addFieldValue(fields, field, htmlText(match[1]), source);
    }
    records.push({ code, fields, source, link: null });
  }
  return records;
}

function parseMarkdownExplain(text, source) {
  const lines = text.split(/\r?\n/u);
  const headerIndex = lines.findIndex((line) => /\|\s*code\s*\|/iu.test(line));
  if (headerIndex < 0) return [];
  const header = lines[headerIndex]
    .split("|")
    .slice(1, -1)
    .map((value) => normalizeText(value).toLowerCase());
  const indexes = Object.fromEntries(FIELD_IDS
    .filter((field) => header.includes(field))
    .map((field) => [field, header.indexOf(field)]));
  const records = [];
  for (const line of lines.slice(headerIndex + 2)) {
    if (!line.trim().startsWith("|")) continue;
    const cells = line.split("|").slice(1, -1).map((value) => normalizeText(value.replaceAll("\\|", "|")));
    const codeIndex = indexes.code ?? header.indexOf("code");
    const code = cells[codeIndex]?.toUpperCase();
    if (!code || !DIAGNOSTIC_CODE.test(code)) continue;
    const fields = emptyFields();
    for (const field of ["code", "what", "why", "fix"]) {
      const value = cells[indexes[field]];
      if (value) addFieldValue(fields, field, value, source);
    }
    records.push({ code, fields, source, link: null });
  }
  return records;
}

function parseTextDiagnosticRecords(text, source) {
  const lines = text.split(/\r?\n/u);
  const starts = [];
  for (const [index, line] of lines.entries()) {
    const match = line.match(/^(?:Error|Warning|Stop|Lint)\s+(?:\[([A-Z][A-Z0-9-]*)\]|\(([A-Z][A-Z0-9-]*)\)):\s*(.*)$/u);
    if (match) starts.push({ index, code: (match[1] ?? match[2]).toUpperCase(), what: match[3] });
  }
  const records = [];
  for (const [index, start] of starts.entries()) {
    const end = starts[index + 1]?.index ?? lines.length;
    const block = lines.slice(start.index, end);
    const fields = emptyFields();
    addFieldValue(fields, "code", start.code, source);
    addFieldValue(fields, "what", start.what, source);
    let link = null;
    for (const line of block) {
      const location = line.match(/-->\s+.+:\d+:\d+\s*$/u);
      if (location) addFieldValue(fields, "span", true, source);
      const why = line.match(/^\s*Why:\s*(.*)$/u);
      if (why) addFieldValue(fields, "why", why[1], source);
      const fix = line.match(/^\s*(?:Fix|Help):\s*(.*)$/u);
      if (fix) addFieldValue(fields, "fix", fix[1], source);
      const more = line.match(/^\s*More:\s*(\S+)\s*$/u);
      if (more) link = more[1];
    }
    records.push({ code: start.code, fields, source, link });
  }
  return records;
}

function parseExportFile(path, mode) {
  const text = readInput(path, mode === "snapshot" ? "snapshot" : "explain export");
  const source = displayPath(path);
  const extension = extname(path).toLowerCase();
  if (mode === "explain" && extension === ".html") return parseHtmlExplain(text, source);
  if (mode === "explain" && extension === ".md") return parseMarkdownExplain(text, source);
  const json = parseJsonRecords(text, source);
  if (json.length > 0) return json;
  return parseTextDiagnosticRecords(text, source);
}

function collectEvidence(paths, extensions, mode) {
  const files = filesFor(paths, extensions, mode === "snapshot" ? "snapshot root" : "explain root");
  const records = files.flatMap((path) => parseExportFile(path, mode));
  const byCode = new Map();
  for (const record of records) {
    if (!DIAGNOSTIC_CODE.test(record.code)) continue;
    const existing = byCode.get(record.code) ?? {
      code: record.code,
      files: [],
      count: 0,
      fields: emptyFields(),
      links: [],
    };
    existing.count += 1;
    if (!existing.files.includes(record.source)) existing.files.push(record.source);
    for (const field of FIELD_IDS) {
      const incoming = record.fields[field];
      if (!incoming?.present) continue;
      existing.fields[field].present = true;
      existing.fields[field].sources.push(...incoming.sources.filter((source) => !existing.fields[field].sources.includes(source)));
      existing.fields[field].values.push(...incoming.values.filter((value) => !existing.fields[field].values.includes(value)));
    }
    if (record.link && !existing.links.includes(record.link)) existing.links.push(record.link);
    byCode.set(record.code, existing);
  }
  return {
    files,
    records: [...byCode.values()].sort((left, right) => compareCodes(left.code, right.code)),
  };
}

function readCorpus(path) {
  let value;
  try {
    value = JSON.parse(readInput(path, "peer corpus"));
  } catch (error) {
    if (error instanceof ParityInputError) throw error;
    inputError(`peer corpus is not valid JSON: ${displayPath(path)}: ${error.message}`);
  }
  validateCorpus(value, path);
  return value;
}

function validateFeatureDescriptor(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    inputError(`${label} must be a feature state descriptor`);
  }
  if (!FEATURE_STATUS_STATES.has(value.status)) {
    inputError(`${label}.status must be unmeasured, unknown, or unsupported`);
  }
  if (!nonEmptyString(value.evidence)) inputError(`${label}.evidence is required`);
}

function validateFeatureMatrix(corpus, knownTools) {
  if (!Array.isArray(corpus.features) || corpus.features.length !== REQUIRED_FEATURES.length) {
    inputError(`peer corpus features must contain exactly ${REQUIRED_FEATURES.length} rows`);
  }
  const expected = new Map(REQUIRED_FEATURES.map((item) => [item.id, item.feature]));
  const seen = new Set();
  for (const item of corpus.features) {
    if (!item || typeof item !== "object" || Array.isArray(item) || !nonEmptyString(item.id)) {
      inputError("each feature row needs a non-empty id");
    }
    if (seen.has(item.id)) inputError(`duplicate feature row ${item.id}`);
    seen.add(item.id);
    if (expected.get(item.id) !== item.feature) {
      inputError(`${item.id}: feature name is missing, duplicated, or not the card census name`);
    }
    validateFeatureDescriptor(item.jet, `${item.id}/jet`);
    if (!item.peers || typeof item.peers !== "object" || Array.isArray(item.peers)) {
      inputError(`${item.id}: peers must be an object`);
    }
    for (const toolId of knownTools) {
      validateFeatureDescriptor(item.peers[toolId], `${item.id}/${toolId}`);
    }
    for (const toolId of Object.keys(item.peers)) {
      if (!knownTools.has(toolId)) inputError(`${item.id}: unknown peer ${toolId}`);
    }
  }
  for (const expectedId of expected.keys()) {
    if (!seen.has(expectedId)) inputError(`missing feature row ${expectedId}`);
  }
}

function validateCorpus(corpus, path) {
  if (!corpus || typeof corpus !== "object" || Array.isArray(corpus)) inputError("peer corpus must be an object");
  if (corpus.schema !== CORPUS_SCHEMA || corpus.schema_version !== 1) {
    inputError(`peer corpus schema must be ${CORPUS_SCHEMA} version 1`);
  }
  if (!Array.isArray(corpus.fields) || corpus.fields.join("\0") !== FIELD_IDS.join("\0")) {
    inputError(`peer corpus fields must be ${FIELD_IDS.join(", ")}`);
  }
  if (!Array.isArray(corpus.tools) || corpus.tools.length === 0) inputError("peer corpus tools must be a non-empty array");
  const toolIds = corpus.tools.map((tool) => tool?.id);
  if (new Set(toolIds).size !== toolIds.length || toolIds.some((id) => !nonEmptyString(id))) {
    inputError("peer corpus tools must have unique non-empty ids");
  }
  if (!Array.isArray(corpus.cases) || corpus.cases.length === 0) inputError("peer corpus cases must be a non-empty array");
  const knownTools = new Set(toolIds);
  const seenCases = new Set();
  for (const item of corpus.cases) {
    if (!item || typeof item !== "object" || !nonEmptyString(item.id)) inputError("each peer corpus case needs an id");
    if (seenCases.has(item.id)) inputError(`duplicate peer corpus case ${item.id}`);
    seenCases.add(item.id);
    const codes = item.jet_codes ?? (item.jet_code ? [item.jet_code] : null);
    if (!Array.isArray(codes) || codes.length === 0 || codes.some((code) => typeof code !== "string" || !DIAGNOSTIC_CODE.test(code))) {
      inputError(`${item.id}: jet_codes must contain diagnostic codes`);
    }
    const required = item.required_jet_fields ?? REQUIRED_PRODUCT_FIELDS;
    if (!Array.isArray(required) || required.some((field) => !FIELD_IDS.includes(field))) {
      inputError(`${item.id}: required_jet_fields contains an unknown field`);
    }
    if (!item.peers || typeof item.peers !== "object" || Array.isArray(item.peers)) {
      inputError(`${item.id}: peers must be an object`);
    }
    for (const tool of corpus.tools) {
      const descriptor = item.peers[tool.id];
      if (!descriptor || typeof descriptor !== "object" || Array.isArray(descriptor)) {
        inputError(`${item.id}: missing peer descriptor for ${tool.id}`);
      }
      if (!descriptor.fields || typeof descriptor.fields !== "object" || Array.isArray(descriptor.fields)) {
        inputError(`${item.id}/${tool.id}: fields must be an object`);
      }
      for (const field of FIELD_IDS) {
        const value = descriptor.fields[field];
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          inputError(`${item.id}/${tool.id}: ${field} must be a state descriptor`);
        }
        if (!PEER_FIELD_STATES.has(value.status)) {
          inputError(`${item.id}/${tool.id}: ${field}.status must be present, absent, or unknown`);
        }
        if (!nonEmptyString(value.evidence)) inputError(`${item.id}/${tool.id}: ${field}.evidence is required`);
      }
    }
    for (const id of Object.keys(item.peers)) {
      if (!knownTools.has(id)) inputError(`${item.id}: unknown peer ${id}`);
    }
  }
  validateFeatureMatrix(corpus, knownTools);
  if (path && !existsSync(path)) inputError(`missing peer corpus: ${displayPath(path)}`);
}

function fieldState(present, value = null, sources = [], reason = null) {
  return {
    present: Boolean(present),
    value: value ?? null,
    sources: [...new Set(sources)].sort((left, right) => left.localeCompare(right)),
    reason,
  };
}

function rowActionable(row) {
  return nonEmptyString(row.structured_fix) && row.structured_fix !== "-";
}

function productRecord(row, snapshot, explain) {
  const registryFields = {
    code: fieldState(nonEmptyString(row.code), row.code, [row.source]),
    what: fieldState(nonEmptyString(row.what), row.what, [row.source]),
    why: fieldState(nonEmptyString(row.why), row.why, [row.source]),
    fix: fieldState(nonEmptyString(row.fix), row.fix, [row.source]),
  };
  const fields = {
    ...registryFields,
    span: fieldState(
      Boolean(snapshot?.fields.span.present || explain?.fields.span.present),
      null,
      [
        ...(snapshot?.fields.span.sources ?? []),
        ...(explain?.fields.span.sources ?? []),
      ],
      snapshot?.fields.span.present || explain?.fields.span.present ? null : "no source span in a registered snapshot or explain export",
    ),
    actionable_edit: fieldState(
      rowActionable(row) || Boolean(snapshot?.fields.actionable_edit.present || explain?.fields.actionable_edit.present),
      rowActionable(row) ? row.structured_fix : null,
      [
        ...(rowActionable(row) ? [row.source] : []),
        ...(snapshot?.fields.actionable_edit.sources ?? []),
        ...(explain?.fields.actionable_edit.sources ?? []),
      ],
      rowActionable(row) || snapshot?.fields.actionable_edit.present || explain?.fields.actionable_edit.present
        ? null
        : "registered row and exports declare no machine edit",
    ),
  };
  const explainAgreement = {};
  for (const field of ["code", "what", "why", "fix"]) {
    const exportField = explain?.fields[field];
    explainAgreement[field] = {
      compared: Boolean(exportField?.present),
      agreement: exportField?.present ? exportField.values.some((value) => normalizedEqual(value, row[field])) : false,
      export_present: Boolean(exportField?.present),
    };
  }
  return {
    code: row.code,
    status: row.status,
    source: row.source,
    source_line: row.source_line,
    structured_fix: row.structured_fix || null,
    fields,
    explain_agreement: explainAgreement,
    snapshot_count: snapshot?.count ?? 0,
    snapshot_files: snapshot?.files ?? [],
    explain_files: explain?.files ?? [],
    links: [...new Set([
      ...(snapshot?.links ?? []),
      ...(explain?.links ?? []),
    ])].sort((left, right) => left.localeCompare(right)),
  };
}

function fieldScore() {
  return {
    total: 0,
    present: 0,
    absent: 0,
    unknown: 0,
    compared: 0,
    matched: 0,
  };
}

function ratio(matched, compared) {
  return compared === 0 ? null : Number((matched / compared).toFixed(6));
}

function addScore(score, state, peerStatus) {
  score.total += 1;
  if (state.present === true) score.present += 1;
  else if (state.present === false) score.absent += 1;
  else score.unknown += 1;
  if (peerStatus !== "unknown" && state.present !== null) {
    score.compared += 1;
    if (state.present === (peerStatus === "present")) score.matched += 1;
  }
}

function summarizeScore(score) {
  return {
    total: score.total,
    present: score.present,
    absent: score.absent,
    unknown: score.unknown,
    agreement: {
      compared: score.compared,
      matched: score.matched,
      ratio: ratio(score.matched, score.compared),
    },
  };
}

function jetCaseState(records, codes, field) {
  const matches = codes.map((code) => records.get(code)).filter(Boolean);
  if (field === "code") {
    const match = matches.find((record) => record.fields.code.present);
    return match
      ? { present: true, values: [match.code], sources: match.fields.code.sources }
      : { present: false, values: [], sources: [] };
  }
  const values = matches.flatMap((record) => record.fields[field].value ? [record.fields[field].value] : []);
  const match = matches.find((record) => record.fields[field].present);
  return match
    ? { present: true, values, sources: matches.flatMap((record) => record.fields[field].sources) }
    : { present: false, values: [], sources: [] };
}

function compareCorpus(corpus, records) {
  const fields = Object.fromEntries(FIELD_IDS.map((field) => [field, fieldScore()]));
  const peerScores = Object.fromEntries(corpus.tools.map((tool) => [tool.id, Object.fromEntries(FIELD_IDS.map((field) => [field, fieldScore()]))]));
  const comparisons = [];
  const missing = [];
  for (const item of corpus.cases) {
    const codes = item.jet_codes ?? [item.jet_code];
    const required = item.required_jet_fields ?? REQUIRED_PRODUCT_FIELDS;
    for (const field of FIELD_IDS) {
      const jet = jetCaseState(records, codes, field);
      const peers = {};
      for (const tool of corpus.tools) {
        const descriptor = item.peers[tool.id].fields[field];
        const comparable = descriptor.status !== "unknown";
        const agreement = comparable ? jet.present === (descriptor.status === "present") : null;
        peers[tool.id] = {
          status: descriptor.status,
          evidence: descriptor.evidence,
          value: descriptor.value ?? null,
          comparable,
          agreement,
        };
        addScore(peerScores[tool.id][field], jet, descriptor.status);
        addScore(fields[field], jet, descriptor.status);
      }
      comparisons.push({
        case: item.id,
        jet_codes: codes,
        field,
        jet: {
          present: jet.present,
          values: [...new Set(jet.values)].sort((left, right) => String(left).localeCompare(String(right))),
          sources: [...new Set(jet.sources)].sort((left, right) => left.localeCompare(right)),
        },
        peers,
      });
    }
    for (const field of required) {
      const state = jetCaseState(records, codes, field);
      if (!state.present) {
        missing.push({
          case: item.id,
          field,
          jet_codes: codes,
          reason: `required by peer corpus case ${item.id}`,
        });
      }
    }
  }
  const uniqueMissing = [];
  const seen = new Set();
  for (const item of missing) {
    const key = `${item.case}\0${item.field}`;
    if (!seen.has(key)) {
      seen.add(key);
      uniqueMissing.push(item);
    }
  }
  return {
    fields: Object.fromEntries(FIELD_IDS.map((field) => [field, summarizeScore(fields[field])])),
    peers: Object.fromEntries(corpus.tools.map((tool) => [tool.id, {
      label: tool.label ?? tool.id,
      fields: Object.fromEntries(FIELD_IDS.map((field) => [field, summarizeScore(peerScores[tool.id][field])])),
    }])),
    comparisons,
    missing,
  };
}

function featureMatrixSummary(features, tools) {
  const jet = Object.fromEntries([...FEATURE_STATUS_STATES].map((status) => [status, 0]));
  const peers = Object.fromEntries(tools.map((tool) => [
    tool.id,
    Object.fromEntries([...FEATURE_STATUS_STATES].map((status) => [status, 0])),
  ]));
  for (const feature of features) {
    jet[feature.jet.status] += 1;
    for (const tool of tools) peers[tool.id][feature.peers[tool.id].status] += 1;
  }
  const complete = features.every((feature) => (
    !["unmeasured", "unknown"].includes(feature.jet.status)
    && tools.every((tool) => !["unmeasured", "unknown"].includes(feature.peers[tool.id].status))
  ));
  return {
    total: features.length,
    jet,
    peers,
    complete,
  };
}

function featureMatrixRows(features, tools) {
  return features.map((feature) => ({
    id: feature.id,
    feature: feature.feature,
    jet: { ...feature.jet },
    peers: Object.fromEntries(tools.map((tool) => [tool.id, { ...feature.peers[tool.id] }])),
  }));
}

export function compare({
  registryPath = REGISTRY,
  corpusPath = CORPUS,
  snapshotPaths = SNAPSHOTS,
  explainPaths = EXPLAIN,
} = {}) {
  const registryInput = absolutePath(registryPath);
  const corpusInput = absolutePath(corpusPath);
  const snapshots = (snapshotPaths ?? []).map(absolutePath);
  const explains = (explainPaths ?? []).map(absolutePath);
  const registry = parseRegistry(readInput(registryInput, "diagnostic registry"), registryInput);
  const corpus = readCorpus(corpusInput);
  const snapshotEvidence = collectEvidence(snapshots, SNAPSHOT_EXTENSIONS, "snapshot");
  const explainEvidence = collectEvidence(explains, EXPLAIN_EXTENSIONS, "explain");
  const snapshotByCode = new Map(snapshotEvidence.records.map((record) => [record.code, record]));
  const explainByCode = new Map(explainEvidence.records.map((record) => [record.code, record]));
  const jetRecords = new Map(registry.rows.map((row) => [
    row.code,
    productRecord(row, snapshotByCode.get(row.code), explainByCode.get(row.code)),
  ]));
  const allProductMissing = [];
  for (const record of jetRecords.values()) {
    if (record.status !== "active") continue;
    for (const field of REQUIRED_PRODUCT_FIELDS) {
      if (!record.fields[field].present) {
        allProductMissing.push({
          code: record.code,
          field,
          source: record.source,
          source_line: record.source_line,
          reason: "required registered Jet product field is empty or absent from the explain export",
        });
      }
    }
  }
  const compared = compareCorpus(corpus, jetRecords);
  const missing = [...allProductMissing, ...compared.missing];
  const uniqueMissing = [];
  const missingKeys = new Set();
  for (const item of missing) {
    const key = `${item.code ?? item.case}\0${item.field}`;
    if (missingKeys.has(key)) continue;
    missingKeys.add(key);
    uniqueMissing.push(item);
  }
  const activeRows = [...jetRecords.values()].filter((record) => record.status === "active");
  const registryFieldScores = Object.fromEntries(FIELD_IDS.map((field) => {
    const present = activeRows.filter((record) => record.fields[field].present).length;
    return [field, {
      total: activeRows.length,
      present,
      absent: activeRows.length - present,
      ratio: ratio(present, activeRows.length),
    }];
  }));
  const featureSummary = featureMatrixSummary(corpus.features, corpus.tools);
  const featureRows = featureMatrixRows(corpus.features, corpus.tools);
  return {
    schema: SCHEMA,
    schema_version: 1,
    inputs: {
      registry: displayPath(registryInput),
      corpus: displayPath(corpusInput),
      snapshots: snapshots.map(displayPath),
      explain: explains.map(displayPath),
    },
    corpus: {
      schema: corpus.schema,
      schema_version: corpus.schema_version,
      cases: corpus.cases.length,
      tools: corpus.tools.map((tool) => tool.id),
      fields: [...corpus.fields],
      feature_matrix: featureRows.length,
    },
    jet: {
      registered_rows: registry.rows.length,
      active_rows: activeRows.length,
      registry_errors: registry.errors,
      snapshot_files: snapshotEvidence.files.map(displayPath),
      snapshot_records: snapshotEvidence.records.length,
      explain_files: explainEvidence.files.map(displayPath),
      explain_records: explainEvidence.records.length,
      registry_field_scores: registryFieldScores,
      records: [...jetRecords.values()],
    },
    score: compared.fields,
    peers: compared.peers,
    feature_matrix: featureRows,
    feature_summary: featureSummary,
    comparisons: compared.comparisons,
    missing_jet_product_fields: uniqueMissing,
    status: {
      // Unmeasured feature rows keep the CLI fail-closed. A capability
      // descriptor is not a comparator result.
      complete: uniqueMissing.length === 0 && featureSummary.complete,
      missing_jet_product_fields: uniqueMissing.length,
      registry_errors: registry.errors.length,
      feature_matrix_complete: featureSummary.complete,
      feature_matrix_unmeasured: featureSummary.jet.unmeasured,
    },
  };
}

function parseArgs(argv) {
  const options = {
    json: false,
    corpusPath: CORPUS,
    registryPath: REGISTRY,
    snapshotPaths: [],
    explainPaths: [],
    snapshotsExplicit: false,
    explainExplicit: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--json") {
      if (options.json) inputError("--json may be given only once");
      options.json = true;
    } else if (["--corpus", "--jet-corpus"].includes(argument)) {
      options.corpusPath = argv[++index];
      if (!options.corpusPath) inputError(`${argument} needs a path`);
    } else if (["--registry", "--jet-registry"].includes(argument)) {
      options.registryPath = argv[++index];
      if (!options.registryPath) inputError(`${argument} needs a path`);
    } else if (["--snapshot", "--jet-snapshot", "--snapshots", "--jet-snapshots"].includes(argument)) {
      const path = argv[++index];
      if (!path) inputError(`${argument} needs a path`);
      options.snapshotPaths.push(path);
      options.snapshotsExplicit = true;
    } else if (["--explain", "--jet-explain"].includes(argument)) {
      const path = argv[++index];
      if (!path) inputError(`${argument} needs a path`);
      options.explainPaths.push(path);
      options.explainExplicit = true;
    } else if (argument === "--help" || argument === "-h") {
      return { help: true };
    } else {
      inputError(`unknown option ${argument}`);
    }
  }
  if (!options.snapshotsExplicit) options.snapshotPaths = SNAPSHOTS;
  if (!options.explainExplicit) options.explainPaths = EXPLAIN;
  return { help: false, ...options };
}

function usage() {
  return [
    "usage: node scripts/agent/diagnostic-parity.mjs [options]",
    "",
    "Options:",
    "  --json                         emit the machine-readable comparison",
    "  --corpus PATH                  checked-in peer corpus descriptor",
    "  --registry PATH                registered Diagnostics.jet source",
    "  --snapshot PATH                snapshot file or root (repeatable)",
    "  --explain PATH                 explain export file or root (repeatable)",
    "  -h, --help                     show this help",
    "",
    "The default inputs are the checked-in Jet registry, UI/report snapshots,",
    "website diagnostic pages, and the rustc/miette/Zig/Go corpus.",
    "The feature matrix is declarative; unmeasured rows never count as parity.",
    "No peer command or network request is made.",
  ].join("\n");
}

function formatScore(score) {
  const agreement = score.agreement.compared === 0
    ? "n/a"
    : `${score.agreement.matched}/${score.agreement.compared} (${(score.agreement.ratio * 100).toFixed(1)}%)`;
  return `Jet ${score.present}/${score.total}; agreement ${agreement}`;
}

export function human(report) {
  const lines = [
    "Diagnostic parity comparator",
    `feature matrix: ${report.feature_summary.total} rows; Jet unmeasured ${report.feature_summary.jet.unmeasured}; complete ${report.feature_summary.complete}`,
    `corpus: ${report.corpus.cases} cases; peers: ${report.corpus.tools.join(", ")}`,
    `Jet registry: ${report.jet.registered_rows} rows (${report.jet.active_rows} active)`,
    `Jet snapshots: ${report.jet.snapshot_files.length} files; explain exports: ${report.jet.explain_files.length} files`,
    "field scores:",
  ];
  for (const field of FIELD_IDS) lines.push(`  ${field}: ${formatScore(report.score[field])}`);
  lines.push("peer field scores:");
  for (const tool of report.corpus.tools) {
    lines.push(`  ${tool}:`);
    for (const field of FIELD_IDS) lines.push(`    ${field}: ${formatScore(report.peers[tool].fields[field])}`);
  }
  lines.push(`Jet product gaps: ${report.missing_jet_product_fields.length}`);
  for (const item of report.missing_jet_product_fields.slice(0, 20)) {
    const identity = item.code ?? item.case;
    lines.push(`  ${identity} ${item.field}: ${item.reason}`);
  }
  if (report.missing_jet_product_fields.length > 20) {
    lines.push(`  ... ${report.missing_jet_product_fields.length - 20} more`);
  }
  if (report.jet.registry_errors.length > 0) {
    lines.push(`registry input errors: ${report.jet.registry_errors.length}`);
    for (const error of report.jet.registry_errors.slice(0, 10)) {
      lines.push(`  ${error.source}:${error.line}: ${error.reasons.join("; ")}`);
    }
  }
  lines.push(`status: ${report.status.complete ? "complete" : "incomplete"}`);
  return `${lines.join("\n")}\n`;
}

export function main(argv = process.argv.slice(2)) {
  try {
    const args = parseArgs(argv);
    if (args.help) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    const report = compare(args);
    process.stdout.write(args.json ? `${JSON.stringify(report, null, 2)}\n` : human(report));
    return report.status.complete && report.status.registry_errors === 0 ? 0 : 1;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 2;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
