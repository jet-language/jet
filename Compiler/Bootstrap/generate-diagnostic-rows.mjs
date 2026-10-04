#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(bootstrapDir, "../..");
const sourcePath = resolve(repoRoot, "crates/jet-codegen/src/Prelude/Diagnostics.jet");
const outputPath = resolve(repoRoot, "Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet");
const sourceDisplayPath = "crates/jet-codegen/src/Prelude/Diagnostics.jet";

function fail(message) {
  throw new Error(`diagnostic-row-generator: ${message}`);
}

function jetStringExpression(value) {
  const needsByteExpression = Array.from(value).some(function (character) {
    const codepoint = character.codePointAt(0);
    return (codepoint < 0x20 || (codepoint >= 0x7f && codepoint <= 0x9f)) &&
      character !== "\n" && character !== "\t";
  });
  if (needsByteExpression) {
    const bytes = Array.from(Buffer.from(value, "utf8"));
    const items = bytes.map(function (byte) { return `U8{${byte}}`; });
    return `String.from_bytes([U8]{${items.join(", ")}}) ?? ""`;
  }
  let literal = '"';
  for (const character of value) {
    if (character === "\\") literal += "\\\\";
    else if (character === '"') literal += '\\"';
    else if (character === "\n") literal += "\\n";
    else if (character === "\t") literal += "\\t";
    else if (character === "{") literal += "{{";
    else if (character === "}") literal += "}}";
    else literal += character;
  }
  return `${literal}"`;
}

// Match Registry.rs::unescape_source exactly. The Prelude table keeps
// backslash escapes and markdown fences in source form; the generated Source
// view must carry the same decoded values as Native.
function unescapeSource(value) {
  if (value.startsWith("`` ") && value.endsWith(" ``")) {
    value = value.slice(3, -3);
  }
  let output = "";
  let escaped = false;
  for (const character of value) {
    if (escaped) {
      if (character === "n") output += "\n";
      else if (character === "r") output += "\r";
      else if (character === "t") output += "\t";
      else output += character;
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else {
      output += character;
    }
  }
  if (escaped) output += "\\";
  return output;
}


function optionString(value) {
  return value === null ? "None" : `Val(${jetStringExpression(value)})`;
}

const statusVariants = new Map([
  ["active", "Active"],
  ["retired", "Retired"],
  ["reserved", "Reserved"],
]);
const momentVariants = new Map([
  ["compile", "Compile"],
  ["compile/entry", "Compile"],
  ["run", "Run"],
  ["test", "Test"],
  ["tool", "Tool"],
]);
const severityValues = new Map([
  ["error", 1],
  ["lint", 2],
]);
const safetyVariants = new Map([
  ["formatting", "Formatting"],
  ["behavior-preserving", "BehaviorPreserving"],
  ["api-changing", "APIChanging"],
  ["target-changing", "TargetChanging"],
  ["needs-review", "NeedsReview"],
]);
const noFixKinds = new Map([
  ["behavior", "Behavior"],
  ["design", "Design"],
  ["ambiguous", "Ambiguous"],
]);
const generatedFixVariants = new Map([
  ["marker_group", "GeneratedMarkerGroup"],
  ["missing_arms", "GeneratedMissingArms"],
  ["script_run", "GeneratedScriptRun"],
  ["call_value", "GeneratedCallValue"],
  ["redundant_tail_return", "GeneratedRedundantTailReturn"],
]);


function isDiagnosticCode(code) {
  return /^[ELRW].+$/.test(code) || /^JT[0-9]{4}$/.test(code);
}

function isSnakeCaseLintName(name) {
  return name !== "" &&
    !name.startsWith("_") &&
    !name.endsWith("_") &&
    !name.includes("__") &&
    /^[a-z0-9_]+$/.test(name);
}

function parseNoFix(text, context) {
  if (!text.startsWith("reason:")) fail(`${context}: malformed no-fix marker`);
  const separator = text.indexOf("|", "reason:".length);
  if (separator < 0) fail(`${context}: no-fix marker needs a kind and next action`);
  const kindText = text.slice("reason:".length, separator);
  const next = text.slice(separator + 1);
  const kind = noFixKinds.get(kindText);
  if (!kind) fail(`${context}: unknown no-fix reason kind ${JSON.stringify(kindText)}`);
  if (next.trim() === "") fail(`${context}: no-fix reason needs a next action`);
  return `Val(NoFixReason{kind: NoFixReasonKind.${kind}, next: ${jetStringExpression(next)}})`;
}

function parseStructuredMarker(marker, context) {
  if (marker === "-") {
    return { structured: "None", safety: "None", noFix: "None" };
  }
  if (marker.startsWith("reason:")) {
    return { structured: "None", safety: "None", noFix: parseNoFix(marker, context) };
  }
  const reasonSeparator = marker.indexOf("|reason:");
  if (reasonSeparator >= 0) {
    const prefix = marker.slice(0, reasonSeparator);
    const reason = marker.slice(reasonSeparator + 1);
    return {
      structured: prefix === "crypto_misuse" ? "Val(DiagnosticRegistryFix.CryptoMisuse)" : "None",
      safety: "None",
      noFix: parseNoFix(reason, context),
    };
  }
  const markerBase = marker.split("|", 1)[0];
  if (markerBase === "crypto_misuse") {
    return { structured: "Val(DiagnosticRegistryFix.CryptoMisuse)", safety: "None", noFix: "None" };
  }

  let prefix;
  let structured;
  if (marker.startsWith("source_edit|")) {
    prefix = "source_edit|";
    structured = "SourceEdit";
  } else if (marker.startsWith("suggested_source_edit|")) {
    prefix = "suggested_source_edit|";
    structured = "SuggestedSourceEdit";
  } else if (marker.startsWith("replace:")) {
    prefix = "replace:";
    const separator = marker.indexOf("|", prefix.length);
    if (separator < 0) fail(`${context}: replacement marker needs a safety grade`);
    const replacement = marker.slice(prefix.length, separator);
    const arrow = replacement.indexOf("=>");
    if (arrow < 0) fail(`${context}: replacement marker needs ` + "`from=>to`");
    const from = replacement.slice(0, arrow);
    const to = replacement.slice(arrow + 2);
    if (from === "" || to === "") fail(`${context}: replacement marker needs non-empty sides`);
    structured = `Replace{from: ${jetStringExpression(from)}, to: ${jetStringExpression(to)}}`;
    const safety = parseMarkerSafety(marker, context);
    return {
      structured: `Val(DiagnosticRegistryFix.${structured})`,
      safety,
      noFix: "None",
    };
  } else if (marker.startsWith("remove:")) {
    prefix = "remove:";
    const separator = marker.indexOf("|", prefix.length);
    if (separator < 0) fail(`${context}: removal marker needs a safety grade`);
    const text = marker.slice(prefix.length, separator);
    if (text === "") fail(`${context}: removal marker needs non-empty text`);
    structured = `Remove{text: ${jetStringExpression(text)}}`;
    const safety = parseMarkerSafety(marker, context);
    return {
      structured: `Val(DiagnosticRegistryFix.${structured})`,
      safety,
      noFix: "None",
    };
  } else if (marker.startsWith("generated:")) {
    prefix = "generated:";
    const separator = marker.indexOf("|", prefix.length);
    if (separator < 0) fail(`${context}: generated marker needs a safety grade`);
    const kindText = marker.slice(prefix.length, separator);
    const kind = generatedFixVariants.get(kindText);
    if (!kind) fail(`${context}: unknown generated fix kind ${JSON.stringify(kindText)}`);
    structured = kind;
    const safety = parseMarkerSafety(marker, context);
    return {
      structured: `Val(DiagnosticRegistryFix.${structured})`,
      safety,
      noFix: "None",
    };
  } else {
    fail(`${context}: unknown structured-fix marker ${JSON.stringify(marker)}`);
  }

  const safety = parseMarkerSafety(marker, context);
  return {
    structured: `Val(DiagnosticRegistryFix.${structured})`,
    safety,
    noFix: "None",
  };
}

function parseSafety(text, context) {
  const safety = safetyVariants.get(text);
  if (!safety) fail(`${context}: unknown fix safety ${JSON.stringify(text)}`);
  return `Val(FixSafety.${safety})`;
}

function parseMarkerSafety(marker, context) {
  const separator = marker.lastIndexOf("|");
  if (separator < 0) fail(`${context}: structured marker needs a safety grade`);
  return parseSafety(marker.slice(separator + 1), context);
}

function parseRows(source) {
  const rows = [];
  const lines = source.split(/\r?\n/);
  const codes = new Set();
  for (const [index, line] of lines.entries()) {
    const lineNumber = index + 1;
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("//")) continue;
    const fields = trimmed.split("\t");
    if (fields[0] !== "diagnostic") fail(`${sourceDisplayPath}:${lineNumber}: expected diagnostic row`);
    if (fields.length !== 12 && fields.length !== 13) {
      fail(`${sourceDisplayPath}:${lineNumber}: expected 12 fields for an error or 13 for a lint, got ${fields.length}`);
    }
    const [, rawCode, rawStage, severityText, momentText, statusText, rawMeaning, rawWhat, rawWhy, rawFix, detailText, rawMarker, rawLintName] = fields;
    const code = unescapeSource(rawCode);
    const stage = unescapeSource(rawStage);
    const meaning = unescapeSource(rawMeaning);
    const what = unescapeSource(rawWhat);
    const why = unescapeSource(rawWhy);
    const fix = unescapeSource(rawFix);
    const marker = unescapeSource(rawMarker);
    const lintName = rawLintName === undefined ? undefined : unescapeSource(rawLintName);
    const context = `${sourceDisplayPath}:${lineNumber} (${code || "missing code"})`;
    if (!isDiagnosticCode(code)) fail(`${context}: diagnostic code has no accepted E/L/R/W or JT identity`);
    if (codes.has(code)) fail(`${context}: duplicate diagnostic code`);
    codes.add(code);
    if (!code || !stage || !meaning || !what || !why || !fix) fail(`${context}: required field is empty`);
    const severity = severityValues.get(severityText);
    if (!severity) fail(`${context}: unknown severity ${JSON.stringify(severityText)}`);
    if ((severityText === "lint") !== (fields.length === 13)) fail(`${context}: lint selector field does not match severity`);
    if (severityText === "error" && lintName !== undefined) fail(`${context}: error row must not have a lint selector`);
    if (lintName !== undefined && lintName === "") fail(`${context}: lint selector must not be empty`);
    if (lintName !== undefined && !isSnakeCaseLintName(lintName)) fail(`${context}: lint selector must be stable snake_case`);
    const moment = momentVariants.get(momentText);
    if (!moment) fail(`${context}: unknown diagnostic moment ${JSON.stringify(momentText)}`);
    const status = statusVariants.get(statusText);
    if (!status) fail(`${context}: unknown registry status ${JSON.stringify(statusText)}`);
    if (detailText !== "true" && detailText !== "false") fail(`${context}: detail must be true or false`);
    const parsedMarker = parseStructuredMarker(marker, context);
    rows.push({
      code,
      lintName: lintName === undefined ? null : lintName,
      stage,
      severity,
      moment,
      status,
      meaning,
      what,
      why,
      fix,
      detail: detailText,
      structured: parsedMarker.structured,
      safety: parsedMarker.safety,
      noFix: parsedMarker.noFix,
    });
  }
  if (rows.length === 0) fail(`${sourceDisplayPath}: no diagnostic rows found`);
  return rows;
}

function renderRow(row) {
  return [
    "    DiagnosticRegistryRow{",
    `        code: ${jetStringExpression(row.code)},`,
    `        lint_name: ${optionString(row.lintName)},`,
    `        stage: ${jetStringExpression(row.stage)},`,
    `        severity: ${row.severity},`,
    `        moment: DiagnosticMoment.${row.moment},`,
    `        status: DiagnosticRegistryStatus.${row.status},`,
    `        meaning: ${jetStringExpression(row.meaning)},`,
    `        what: ${jetStringExpression(row.what)},`,
    `        why: ${jetStringExpression(row.why)},`,
    `        fix: ${jetStringExpression(row.fix)},`,
    `        detail: ${row.detail},`,
    `        structured_fix: ${row.structured},`,
    `        fix_safety: ${row.safety},`,
    `        no_fix_reason: ${row.noFix},`,
    "    },",
  ].join("\n");
}

const sourceBytes = await readFile(sourcePath).catch((error) => fail(`cannot read ${sourceDisplayPath}: ${error.message}`));
const source = sourceBytes.toString("utf8");
if (!Buffer.from(source, "utf8").equals(sourceBytes)) fail(`${sourceDisplayPath}: source is not valid UTF-8`);
const sourceHash = createHash("sha256").update(sourceBytes).digest("hex");
const rows = parseRows(source);
const output = [
  "// BEGIN GENERATED DIAGNOSTIC ROWS",
  `// Source: ${sourceDisplayPath}`,
  `// Source SHA-256: ${sourceHash}`,
  "// This file is generated by Compiler/Bootstrap/generate-diagnostic-rows.mjs; do not edit.",
  "pub DIAGNOSTIC_ROWS :: prep { [DiagnosticRegistryRow]{",
  rows.map(renderRow).join("\n"),
  "} }",
  "// END GENERATED DIAGNOSTIC ROWS",
  "",
].join("\n");
await writeFile(outputPath, output, "utf8");
console.log(`generated ${outputPath.replace(`${repoRoot}/`, "")} (${rows.length} rows, source ${sourceHash})`);
