#!/usr/bin/env node
/**
 * Derive the explain/predict/modify/derive learning census from Jet's
 * owner-controlled registries.  This is deliberately a census, not a second
 * compiler, tutor, or editor: source registries define membership and the
 * optional probes only record evidence that was actually observed.
 */

import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { buildCapabilityRelation, CAPABILITY_RELATION_SCHEMA, sourceSnapshot } from "./hardening-manifest.mjs";

const SCHEMA = "jet-learning-census-v1";
const EXAMPLE_CENSUS = "scripts/agent/example-core-census.mjs";
const DEFAULT_ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../..");
const RUNTIME_REPRESENTATIVE = "examples/features/basics/numbers.jet";
const RUNTIME_AUTHORITY_FIXTURE = "examples/continuity/script_to_system/run.jet";
const AXES = ["explain", "predict", "modify", "derive"];
const FAMILY_ORDER = [
  "language",
  "core",
  "cli",
  "editor",
  "debugger",
  "build",
  "package",
  "ffi",
];
const HUMAN_FIELDS = [
  "participant",
  "task_id",
  "completion",
  "prediction_correct",
  "transfer",
  "recovery",
  "latency_ms",
];
const REVISION_RULE = "source-bound observations are valid only when source_identity.digest matches; a source edit invalidates the observation until the dependent projection is regenerated and the task is re-run";
const HUMAN_ABSENCE_REASON = "human transfer evidence is unavailable: agent/model output cannot establish whether a reader can explain, predict, modify, or derive; supply a consented observation record";
const LEARNING_CURRICULUM = "examples/learn/curriculum.json";

function loadLearningCurriculum(root) {
  const path = join(root, LEARNING_CURRICULUM);
  if (!existsSync(path)) {
    return {
      status: "unavailable",
      path: LEARNING_CURRICULUM,
      reason: "authored learning curriculum projection is missing",
      witnesses: [],
    };
  }
  let raw;
  let value;
  try {
    raw = readFileSync(path, "utf8");
    value = JSON.parse(raw);
  } catch (error) {
    return {
      status: "unavailable",
      path: LEARNING_CURRICULUM,
      reason: `authored learning curriculum projection is malformed: ${error.message}`,
      witnesses: [],
    };
  }
  if (!value || value.schema !== "jet-learning-curriculum-v1") {
    return {
      status: "unavailable",
      path: LEARNING_CURRICULUM,
      reason: "authored learning curriculum projection has an unsupported schema",
      witnesses: [],
    };
  }
  if (value.relation !== SCHEMA
    || value.relation_source !== "scripts/agent/learning-census.mjs"
    || value.revision_rule !== REVISION_RULE) {
    return {
      status: "unavailable",
      path: LEARNING_CURRICULUM,
      reason: "authored learning curriculum does not match the canonical census identity/revision contract",
      witnesses: [],
    };
  }
  const witnesses = Array.isArray(value.tasks) ? value.tasks : [];
  const ids = new Set();
  const capabilities = new Set();
  const families = new Set();
  for (const witness of witnesses) {
    if (!witness || typeof witness !== "object") {
      return {
        status: "unavailable",
        path: LEARNING_CURRICULUM,
        reason: "authored learning curriculum contains a non-object witness",
        witnesses: [],
      };
    }
    if (typeof witness.id !== "string" || !witness.id
      || typeof witness.capability_id !== "string" || !witness.capability_id
      || typeof witness.census_task_id !== "string" || !witness.census_task_id.endsWith(".predict")
      || witness.axis !== "predict"
      || typeof witness.family !== "string" || !witness.family
      || typeof witness.prompt !== "string" || !witness.prompt
      || typeof witness.wrong_model !== "string" || !witness.wrong_model
      || !witness.source || !witness.input_identity || !witness.oracle
      || !witness.reveal || !witness.controlled_edit || !witness.transfer
      || !witness.accessibility) {
      return {
        status: "unavailable",
        path: LEARNING_CURRICULUM,
        reason: `authored learning curriculum witness ${witness.id ?? "<missing>"} lacks the required task contract`,
        witnesses: [],
      };
    }
    if (ids.has(witness.id) || capabilities.has(witness.capability_id)) {
      return {
        status: "unavailable",
        path: LEARNING_CURRICULUM,
        reason: `authored learning curriculum repeats witness identity ${witness.id}`,
        witnesses: [],
      };
    }
    ids.add(witness.id);
    capabilities.add(witness.capability_id);
    families.add(witness.family);
  }
  for (const family of ["loop", "state-lifetime", "effects", "foreign"]) {
    if (!families.has(family)) {
      return {
        status: "unavailable",
        path: LEARNING_CURRICULUM,
        reason: `authored learning curriculum is missing the ${family} witness`,
        witnesses: [],
      };
    }
  }
  return {
    status: "available",
    path: LEARNING_CURRICULUM,
    schema: value.schema,
    relation: value.relation,
    relation_source: value.relation_source,
    revision_rule: value.revision_rule,
    revision: digestText(raw),
    witnesses,
  };
}

function projectLearningCurriculum(curriculum, taskById) {
  if (curriculum.status !== "available") return curriculum;
  const witnesses = curriculum.witnesses.map((witness) => {
    const canonical = taskById.get(witness.census_task_id)
      ?? taskById.get(witness.id);
    return {
      ...witness,
      status: canonical ? "projected" : "unavailable",
      canonical_task_id: canonical?.id ?? null,
      canonical_task: canonical
        ? {
          id: canonical.id,
          axis: canonical.axis,
          source_identity: canonical.source_identity,
          input_identity: canonical.input_identity,
          oracle: canonical.oracle,
          paths: canonical.paths,
          evidence: canonical.evidence,
          derivation: canonical.derivation,
        }
        : null,
      reason: canonical
        ? null
        : "witness does not name a task in the current canonical census relation",
    };
  });
  return {
    ...curriculum,
    witnesses,
    projected: witnesses.filter((witness) => witness.status === "projected").length,
    unavailable: witnesses.filter((witness) => witness.status !== "projected").length,
  };
}

class CensusError extends Error {
  constructor(kind, message) {
    super(`${kind}: ${message}`);
    this.kind = kind;
  }
}

function fail(kind, message) {
  throw new CensusError(kind, message);
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function normalizePath(value) {
  return value.split("\\").join("/");
}

function relPath(root, value) {
  return normalizePath(relative(root, value) || ".");
}

function digestText(text) {
  return createHash("sha256").update(text).digest("hex");
}
function digestFile(path) {
  try {
    return digestText(readFileSync(path));
  } catch {
    return null;
  }
}


function stripAnsi(value) {
  return value
    .replace(/\u001B\[([A-Za-z]+)\u001B\[0m/g, "$1")
    .replace(/\u001B\][^\u0007]*(?:\u0007|\u001B\\)/g, "")
    .replace(/\u001B\[[0-?]*[ -/]*[@-~]/g, "");
}

function diagnosticText(result) {
  const lines = stripAnsi(`${result.stderr ?? ""}\n${result.stdout ?? ""}`)
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const start = lines.findIndex((line) => /internal compiler error|error\[[A-Z0-9]+\]|^error:|panic|failed/i.test(line));
  return (start >= 0 ? lines.slice(start) : lines).join("\n");
}

function firstDiagnosticLine(result) {
  const lines = diagnosticText(result).split("\n").filter(Boolean);
  return lines.find((line) => /internal compiler error|error\[[A-Z0-9]+\]|^error:|panic|failed/i.test(line)) ?? lines[0] ?? null;
}

function diagnosticEffects(text, field) {
  const match = text.match(new RegExp(`${field}=([^;\\n]+)`));
  return match ? match[1].trim().split(/,\s*/).filter(Boolean) : [];
}


function countStatuses(values) {
  const counts = {};
  for (const value of values) counts[value] = (counts[value] ?? 0) + 1;
  return Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right)));
}
function classifyRuntimeDefect({ command, source, sourceRevision, result }) {
  const output = diagnosticText(result);
  const authority = /\bE1803\b|application authority is undecided|undecided_effects=/i.test(output);
  const constructor = /data constructor without a resolved return type/i.test(output);
  const dataTree = /missing checked MIR type row [`']DataTree[`']/i.test(output);
  const typeRow = /missing checked MIR type row [`'][^`']+[`']/i.test(output);
  const coreReturn = /core call .* has no resolved return type/i.test(output);
  const ordering = (typeRow && !dataTree) || constructor || coreReturn;
  const receiver = /unbound TIR local [`']self[`']/i.test(output);
  const owner = authority
    ? "example authority declaration"
    : dataTree
      ? "DataTreeTypeRow"
      : ordering
        ? "OrderingTypeRow"
        : receiver
          ? "CliDeriveRegression"
          : null;
  return {
    owner,
    owner_card: null,
    assignment_status: owner && owner !== "example authority declaration" ? "assigned" : "unassigned",
    reason_class: authority
      ? "authority-declaration"
      : dataTree
        ? "compiler-datatree-type-row"
        : constructor
          ? "compiler-constructor-return-type"
          : coreReturn
            ? "compiler-core-call-return-type"
            : typeRow
              ? "compiler-missing-type-row"
              : receiver
                ? "compiler-receiver-binding"
                : "runtime-failure",
    command,
    source,
    source_revision: sourceRevision,
    exit_code: result.exit_code,
    diagnostic: output,
    required_effects: diagnosticEffects(output, "required_effects"),
    granted_effects: diagnosticEffects(output, "granted_effects"),
  };
}

function decodeRustString(raw) {
  return raw
    .replace(/\\r/g, "\r")
    .replace(/\\n/g, "\n")
    .replace(/\\t/g, "\t")
    .replace(/\\0/g, "\0")
    .replace(/\\"/g, '"')
    .replace(/\\\\/g, "\\");
}


function digestJson(value) {
  return digestText(JSON.stringify(value));
}
function stringValue(raw) {
  return decodeRustString(raw);
}

function lineAt(text, index) {
  return text.slice(0, index).split("\n").length;
}

function sourceRecord(root, path, text, line, role, sourceOfTruth = null) {
  return {
    path: normalizePath(path),
    line,
    digest: digestText(text),
    role,
    source_of_truth: sourceOfTruth,
  };
}

function readSource(root, path, role, sourceOfTruth = null) {
  const absolute = join(root, path);
  let text;
  try {
    text = readFileSync(absolute, "utf8");
  } catch (error) {
    fail("source error", `cannot read ${path}: ${error.message}`);
  }
  return {
    absolute,
    path: normalizePath(path),
    text,
    digest: digestText(text),
    role,
    source_of_truth: sourceOfTruth,
  };
}

/** Extract a balanced Rust/JSON-ish delimiter, ignoring strings and comments. */
function delimited(text, start, open, close) {
  const openAt = text.indexOf(open, start);
  if (openAt < 0) fail("source error", `missing ${open} after offset ${start}`);
  let depth = 0;
  let state = "normal";
  let rawHash = 0;
  for (let index = openAt; index < text.length; index += 1) {
    const char = text[index];
    const next = text[index + 1];
    if (state === "line-comment") {
      if (char === "\n") state = "normal";
      continue;
    }
    if (state === "block-comment") {
      if (char === "*" && next === "/") {
        state = "normal";
        index += 1;
      }
      continue;
    }
    if (state === "string") {
      if (char === "\\") {
        index += 1;
      } else if (char === '"') {
        state = "normal";
      }
      continue;
    }
    if (state === "raw-string") {
      if (char === '"') {
        let hashes = 0;
        while (text[index + 1 + hashes] === "#") hashes += 1;
        if (hashes === rawHash && text[index + 1 + hashes] === '"') {
          state = "normal";
          index += hashes + 1;
        }
      }
      continue;
    }
    if (char === "/" && next === "/") {
      state = "line-comment";
      index += 1;
      continue;
    }
    if (char === "/" && next === "*") {
      state = "block-comment";
      index += 1;
      continue;
    }
    if (char === '"') {
      state = "string";
      continue;
    }
    if (char === "r" && (text[index + 1] === '"' || text[index + 1] === "#")) {
      let cursor = index + 1;
      while (text[cursor] === "#") cursor += 1;
      if (text[cursor] === '"') {
        rawHash = cursor - index - 1;
        state = "raw-string";
        index = cursor;
        continue;
      }
    }
    if (char === open) {
      depth += 1;
    } else if (char === close) {
      depth -= 1;
      if (depth === 0) {
        return {
          body: text.slice(openAt + 1, index),
          bodyStart: openAt + 1,
          end: index + 1,
        };
      }
    }
  }
  fail("source error", `unclosed ${open}${close} block after offset ${start}`);
}

function quotedStrings(body) {
  const values = [];
  const regex = /"((?:\\.|[^"\\])*)"/g;
  for (const match of body.matchAll(regex)) values.push(stringValue(match[1]));
  return values;
}

function literalOrSymbol(raw, constants) {
  if (raw.startsWith('"') && raw.endsWith('"')) return stringValue(raw.slice(1, -1));
  return constants.get(raw) ?? raw;
}

function slug(value) {
  const text = String(value);
  const readable = text
    .normalize("NFKD")
    .replace(/[^A-Za-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase();
  return readable || `x-${Buffer.from(text).toString("hex")}`;
}

function sourceIdentity(source, line, authority = null, revisionSource = null) {
  const revision = revisionSource ?? source;
  return {
    path: source.path,
    line,
    digest: revision.digest,
    authority: authority ?? source.source_of_truth ?? source.path,
    projection: revision === source
      ? null
      : { path: source.path, digest: source.digest },
  };

}
function arrayBlock(text, start) {
  const equals = text.indexOf("=", start);
  if (equals < 0) fail("source error", `missing array assignment after offset ${start}`);
  return delimited(text, equals, "[", "]");
}

function enumBody(source, enumName) {
  const marker = `enum ${enumName}`;
  const start = source.text.indexOf(marker);
  if (start < 0) fail("source error", `missing ${marker} in ${source.path}`);
  return delimited(source.text, start, "{", "}");
}

function addRow(rows, row) {
  if (!FAMILY_ORDER.includes(row.family)) fail("inventory error", `unknown family ${row.family}`);
  if (!row.id || !row.label || !row.source_identity) fail("inventory error", `incomplete row ${row.id ?? "<missing>"}`);
  if (!rows.has(row.id)) rows.set(row.id, row);
}

function enumVariants(source, enumName) {
  const block = enumBody(source, enumName);
  const variants = [];
  const regex = /^\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?:\([^\n]*\)|\{[^\n]*\})?\s*,/gm;
  for (const match of block.body.matchAll(regex)) {
    variants.push({ name: match[1], line: lineAt(source.text, block.bodyStart + match.index) });
  }
  return variants;
}

function structFields(source, structName) {
  const marker = `struct ${structName}`;
  const start = source.text.indexOf(marker);
  if (start < 0) fail("source error", `missing ${marker} in ${source.path}`);
  const block = delimited(source.text, start, "{", "}");
  const fields = [];
  const regex = /^\s*pub\s+([A-Za-z_][A-Za-z0-9_]*)\s*:/gm;
  for (const match of block.body.matchAll(regex)) {
    fields.push({ name: match[1], line: lineAt(source.text, block.bodyStart + match.index) });
  }
  return fields;
}

function parseSyntax(root, rows, sources) {
  const source = readSource(root, "crates/jet-foundation/src/Syntax.rs", "language lexical registry");
  sources.language.push(sourceRecord(root, source.path, source.text, 20, source.role));
  const start = source.text.indexOf("pub const LEXICAL_LEDGER");
  if (start < 0) fail("source error", "LEXICAL_LEDGER disappeared");
  const block = arrayBlock(source.text, start);
  const regex = /LexicalEntry\s*\{\s*spelling:\s*"((?:\\.|[^"\\])*)"\s*,\s*meaning:\s*"((?:\\.|[^"\\])*)"\s*,\s*decision:\s*"((?:\\.|[^"\\])*)"\s*\}/g;
  for (const match of block.body.matchAll(regex)) {
    const spelling = stringValue(match[1]);
    const meaning = stringValue(match[2]);
    const decision = stringValue(match[3]);
    const line = lineAt(source.text, block.bodyStart + match.index);
    addRow(rows, {
      id: `language.lexical.${slug(spelling)}`,
      family: "language",
      kind: "lexical",
      label: spelling,
      detail: meaning,
      decision,
      source_identity: sourceIdentity(source, line),
    });
  }
  const tir = readSource(root, "crates/jet-codegen/src/Codegen/TIR/mod.rs", "language TIR registry");
  sources.language.push(sourceRecord(root, tir.path, tir.text, 1, tir.role));
  for (const [enumName, kind] of [["TExprKind", "tir-expr"], ["TStmt", "tir-stmt"]]) {
    for (const variant of enumVariants(tir, enumName)) {
      addRow(rows, {
        id: `language.${kind}.${slug(variant.name)}`,
        family: "language",
        kind,
        label: variant.name,
        detail: `${enumName} capability variant`,
        source_identity: sourceIdentity(tir, variant.line),
      });
    }
  }
  if (!rowsHasFamily(rows, "language")) fail("inventory error", "language registries have no rows");
}

function rowsHasFamily(rows, family) {
  for (const row of rows.values()) if (row.family === family) return true;
  return false;
}

function parseCore(root, rows, sources) {
  const exportsSource = readSource(
    root,
    "crates/jet-foundation/src/CoreModuleExports.rs",
    "generated Core module registry",
    "crates/jet-codegen/src/Prelude/Core.jet",
  );
  const coreSource = readSource(root, "crates/jet-codegen/src/Prelude/Core.jet", "Core source");
  const callsSource = readSource(root, "crates/jet-foundation/src/Syntax/core_calls.rs", "Core call registry");
  const moduleItemsSource = readSource(
    root,
    "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
    "Core semantic item projection",
    coreSource.path,
  );
  sources.core.push(sourceRecord(root, exportsSource.path, exportsSource.text, 23, exportsSource.role, coreSource.path));
  sources.core.push(sourceRecord(root, coreSource.path, coreSource.text, 1, coreSource.role));
  sources.core.push(sourceRecord(root, callsSource.path, callsSource.text, 1514, callsSource.role));
  sources.core.push(sourceRecord(root, moduleItemsSource.path, moduleItemsSource.text, 4, moduleItemsSource.role, coreSource.path));

  const namesStart = exportsSource.text.indexOf("pub const CORE_MODULE_NAMES");
  if (namesStart >= 0) {
    const namesBlock = arrayBlock(exportsSource.text, namesStart);
    const names = [];
    const nameRegex = /"((?:\\.|[^"\\])*)"/g;
    for (const match of namesBlock.body.matchAll(nameRegex)) {
      names.push({ name: stringValue(match[1]), line: lineAt(exportsSource.text, namesBlock.bodyStart + match.index) });
    }
    for (const { name, line } of names) {
      addRow(rows, {
        id: `core.module.${slug(name)}`,
        family: "core",
        kind: "module",
        label: name,
        detail: "public Core module",
        module: name,
        source_identity: sourceIdentity(exportsSource, line, coreSource.path, coreSource),
      });
    }
  }

  const declarationRegex = /CoreModuleDeclaration\s*\{\s*module:\s*"((?:\\.|[^"\\])*)"\s*,\s*members:\s*CORE_MODULE_(\d+)_MEMBERS/g;
  const declarations = [];
  for (const match of exportsSource.text.matchAll(declarationRegex)) {
    declarations.push({
      module: stringValue(match[1]),
      index: match[2],
      line: lineAt(exportsSource.text, match.index),
    });
  }
  const declarationByIndex = new Map(declarations.map((entry) => [entry.index, entry]));
  const memberRegex = /const CORE_MODULE_(\d+)_MEMBERS\s*:\s*&\[&str\]\s*=\s*&\[/g;
  for (const match of exportsSource.text.matchAll(memberRegex)) {
    const index = match[1];
    const declaration = declarationByIndex.get(index);
    if (!declaration) continue;
    const block = arrayBlock(exportsSource.text, match.index);
    const values = quotedStrings(block.body);
    for (const value of values) {
      const valueOffset = exportsSource.text.indexOf(`"${value.replace(/"/g, '\\"')}"`, match.index);
      const line = valueOffset >= 0 ? lineAt(exportsSource.text, valueOffset) : lineAt(exportsSource.text, match.index);
      addRow(rows, {
        id: `core.member.${slug(declaration.module)}.${slug(value)}`,
        family: "core",
        kind: "member",
        label: value,
        module: declaration.module,
        detail: `public member of ${declaration.module}`,
        source_identity: sourceIdentity(exportsSource, line, coreSource.path, coreSource),
      });
    }
  }
  const typeRegex = /const CORE_MODULE_(\d+)_TYPES\s*:\s*&\[\(&str,\s*CoreLeafKind\)\]\s*=\s*&\[/g;
  for (const match of exportsSource.text.matchAll(typeRegex)) {
    const declaration = declarationByIndex.get(match[1]);
    if (!declaration) continue;
    const block = arrayBlock(exportsSource.text, match.index);
    const values = [];
    const valueRegex = /\(\s*"((?:\\.|[^"\\])*)"\s*,\s*CoreLeafKind::/g;
    for (const typeMatch of block.body.matchAll(valueRegex)) values.push({ name: stringValue(typeMatch[1]), offset: typeMatch.index });
    for (const value of values) {
      addRow(rows, {
        id: `core.type.${slug(declaration.module)}.${slug(value.name)}`,
        family: "core",
        kind: "type",
        label: value.name,
        module: declaration.module,
        detail: `public type export of ${declaration.module}`,
        source_identity: sourceIdentity(exportsSource, lineAt(exportsSource.text, match.index + value.offset), coreSource.path, coreSource),
      });
    }
  }

  const callRegex = /CoreCallRecord::new\(\s*"((?:\\.|[^"\\])*)"\s*,\s*"((?:\\.|[^"\\])*)"/g;
  for (const match of callsSource.text.matchAll(callRegex)) {
    const module = stringValue(match[1]);
    const member = stringValue(match[2]);
    addRow(rows, {
      id: `core.call.${slug(module)}.${slug(member)}`,
      family: "core",
      kind: "call",
      label: member,
      module,
      detail: `call projection for ${module}.${member}`,
      source_identity: sourceIdentity(callsSource, lineAt(callsSource.text, match.index)),
    });
  }
  const receiverRegex = /CoreCallRecord::receiver\(\s*&\[[\s\S]*?\]\s*,\s*"((?:\\.|[^"\\])*)"/g;
  for (const match of callsSource.text.matchAll(receiverRegex)) {
    const member = stringValue(match[1]);
    addRow(rows, {
      id: `core.receiver.${slug(member)}`,
      family: "core",
      kind: "receiver",
      label: member,
      detail: "receiver-method projection",
      source_identity: sourceIdentity(callsSource, lineAt(callsSource.text, match.index)),
    });
  }
  if (!rowsHasFamily(rows, "core")) fail("inventory error", "Core registries have no rows");
}

function parseCli(root, rows, sources) {
  const source = readSource(root, "crates/jet-cli/src/CLI.rs", "CLI command and flag registry");
  sources.cli.push(sourceRecord(root, source.path, source.text, 1029, source.role));
  const constants = new Map();
  const constantRegex = /(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"((?:\\.|[^"\\])*)"/g;
  for (const match of source.text.matchAll(constantRegex)) constants.set(match[1], stringValue(match[2]));

  const commandsStart = source.text.indexOf("pub const COMMANDS");
  if (commandsStart < 0) fail("source error", "CLI COMMANDS registry disappeared");
  const commandsBlock = arrayBlock(source.text, commandsStart);
  const commandRegex = /CommandSpec\s*\{\s*name:\s*"((?:\\.|[^"\\])*)"/g;
  for (const match of commandsBlock.body.matchAll(commandRegex)) {
    const name = stringValue(match[1]);
    addRow(rows, {
      id: `cli.command.${slug(name)}`,
      family: "cli",
      kind: "command",
      label: name,
      detail: "top-level Jet command",
      source_identity: sourceIdentity(source, lineAt(source.text, commandsBlock.bodyStart + match.index)),
    });
  }

  const actionDecl = /const\s+([A-Z][A-Z0-9_]*)_ACTIONS\s*:\s*&\[NestedCommandSpec\]\s*=\s*&\[/g;
  for (const match of source.text.matchAll(actionDecl)) {
    const group = match[1].toLowerCase().replace(/_/g, "-");
    const block = arrayBlock(source.text, match.index);
    const actionRegex = /NestedCommandSpec\s*\{\s*name:\s*"((?:\\.|[^"\\])*)"/g;
    for (const action of block.body.matchAll(actionRegex)) {
      const name = stringValue(action[1]);
      addRow(rows, {
        id: `cli.action.${slug(group)}.${slug(name)}`,
        family: "cli",
        kind: "action",
        label: name,
        group,
        detail: `nested ${group} action`,
        source_identity: sourceIdentity(source, lineAt(source.text, block.bodyStart + action.index)),
      });
    }
  }

  const inspectStart = source.text.indexOf("pub enum InspectPlane");
  if (inspectStart >= 0) {
    const inspectBody = delimited(source.text, inspectStart, "{", "}");
    const variants = enumVariants(source, "InspectPlane").map((entry) => entry.name);
    const inspectNames = new Map();
    const inspectNameStart = source.text.indexOf("impl InspectPlane");
    const inspectNameEnd = source.text.indexOf("/// Every built-in", inspectNameStart);
    const inspectText = inspectNameStart >= 0
      ? source.text.slice(inspectNameStart, inspectNameEnd >= 0 ? inspectNameEnd : inspectNameStart + 4000)
      : inspectBody.body;
    const nameRegex = /Self::([A-Za-z_][A-Za-z0-9_]*)\s*=>\s*"((?:\\.|[^"\\])*)"/g;
    for (const match of inspectText.matchAll(nameRegex)) inspectNames.set(match[1], stringValue(match[2]));
    for (const variant of variants) {
      const name = inspectNames.get(variant) ?? variant.toLowerCase();
      addRow(rows, {
        id: `cli.inspect.${slug(name)}`,
        family: "cli",
        kind: "inspect-plane",
        label: name,
        detail: `inspect plane ${variant}`,
        source_identity: sourceIdentity(source, lineAt(source.text, inspectStart), "crates/jet-cli/src/CLI.rs"),
      });
    }
  }

  const flagsStart = source.text.indexOf("const BASE_FLAGS");
  if (flagsStart >= 0) {
    const flagsBlock = arrayBlock(source.text, flagsStart);
    const flagRegex = /FlagSpec\s*\{\s*long:\s*([^,}\n]+)/g;
    for (const match of flagsBlock.body.matchAll(flagRegex)) {
      const raw = match[1].trim();
      const value = literalOrSymbol(raw, constants);
      addRow(rows, {
        id: `cli.flag.${slug(value)}`,
        family: "cli",
        kind: "flag",
        label: value,
        detail: "canonical CLI flag",
        source_identity: sourceIdentity(source, lineAt(source.text, flagsBlock.bodyStart + match.index)),
      });
    }
  }
  if (!rowsHasFamily(rows, "cli")) fail("inventory error", "CLI registries have no rows");
}

function parseEditor(root, rows, sources) {
  const source = readSource(root, "crates/jet-devserver/src/EditorHost.rs", "editor host protocol registry");
  sources.editor.push(sourceRecord(root, source.path, source.text, 23, source.role));
  const methodRegex = /pub const (EDITOR_[A-Z0-9_]+_METHOD):\s*&str\s*=\s*"((?:\\.|[^"\\])*)"/g;
  for (const match of source.text.matchAll(methodRegex)) {
    const value = stringValue(match[2]);
    addRow(rows, {
      id: `editor.method.${slug(value)}`,
      family: "editor",
      kind: "method",
      label: value,
      detail: match[1],
      source_identity: sourceIdentity(source, lineAt(source.text, match.index)),
    });
  }
  for (const enumName of ["EditorHost", "EditorCommand"]) {
    for (const variant of enumVariants(source, enumName)) {
      addRow(rows, {
        id: `editor.${enumName === "EditorHost" ? "host" : "command"}.${slug(variant.name)}`,
        family: "editor",
        kind: enumName === "EditorHost" ? "host" : "command",
        label: variant.name,
        detail: `${enumName} variant`,
        source_identity: sourceIdentity(source, variant.line),
      });
    }
  }
  if (!rowsHasFamily(rows, "editor")) fail("inventory error", "editor registry has no rows");
}

function parseDebugger(root, rows, sources) {
  const syntax = readSource(root, "crates/jet-foundation/src/Syntax/package_files.rs", "debugger command registry");
  const debugSource = readSource(root, "crates/jet-devserver/src/Canvas/debug_source_git.rs", "debugger backend registry");
  const debugLib = readSource(root, "crates/jet-debug/src/lib.rs", "interpreter debugger implementation");
  sources.debugger.push(sourceRecord(root, syntax.path, syntax.text, 322, syntax.role));
  sources.debugger.push(sourceRecord(root, debugSource.path, debugSource.text, 36, debugSource.role));
  sources.debugger.push(sourceRecord(root, debugLib.path, debugLib.text, 17, debugLib.role));
  const commandRegex = /pub const (DBG_[A-Z0-9_]+):\s*&str\s*=\s*"((?:\\.|[^"\\])*)"/g;
  for (const match of syntax.text.matchAll(commandRegex)) {
    const value = stringValue(match[2]);
    addRow(rows, {
      id: value === "(jet)" ? "debugger.prompt.jet" : `debugger.command.${slug(value)}`,
      family: "debugger",
      kind: value === "(jet)" ? "prompt" : "command",
      label: value,
      detail: match[1],
      source_identity: sourceIdentity(syntax, lineAt(syntax.text, match.index)),
    });
  }
  for (const variant of enumVariants(debugSource, "DebugTier")) {
    const wireMatch = new RegExp(`"([^"\\n]+)"\\s*=>\\s*Ok\\(Self::${variant.name}\\)`).exec(debugSource.text);
    const wire = wireMatch ? wireMatch[1] : variant.name;
    addRow(rows, {
      id: `debugger.tier.${slug(wire)}`,
      family: "debugger",
      kind: "tier",
      label: wire,
      detail: `${variant.name} debugger backend`,
      source_identity: sourceIdentity(debugSource, variant.line),
    });
  }
  if (!rowsHasFamily(rows, "debugger")) fail("inventory error", "debugger registries have no rows");
}

function parseBuild(root, rows, sources) {
  const effects = readSource(root, "crates/jet-foundation/src/BuildEffects.rs", "build-effect registry", "crates/jet-codegen/src/Prelude/Effects.jet");
  const effectsSource = readSource(root, effects.source_of_truth, "build-effect source");
  sources.build.push(sourceRecord(root, effects.path, effects.text, 9, effects.role, effects.source_of_truth));
  sources.build.push(sourceRecord(root, effectsSource.path, effectsSource.text, 1, effectsSource.role));
  for (const variant of enumVariants(effects, "BuildEffect")) {
    addRow(rows, {
      id: `build.effect.${slug(variant.name)}`,
      family: "build",
      kind: "effect",
      label: variant.name,
      detail: "declared build capability",
      source_identity: sourceIdentity(effects, variant.line, effects.source_of_truth, effectsSource),
    });
  }
  const blocks = readSource(root, "crates/jet-pkg-model/src/Package/Blocks.rs", "package build-profile registry");
  sources.build.push(sourceRecord(root, blocks.path, blocks.text, 1104, blocks.role));
  for (const enumName of ["BuildOptimize", "BuildPanic"]) {
    for (const variant of enumVariants(blocks, enumName)) {
      addRow(rows, {
        id: `build.${slug(enumName)}.${slug(variant.name)}`,
        family: "build",
        kind: "profile-option",
        label: variant.name,
        detail: `${enumName} option`,
        source_identity: sourceIdentity(blocks, variant.line),
      });
    }
  }
  if (!rowsHasFamily(rows, "build")) fail("inventory error", "build registries have no rows");
}

function parsePackage(root, rows, sources) {
  const packageSource = readSource(root, "crates/jet-pkg-model/src/Package/mod.rs", "package facts registry");
  const blocks = readSource(root, "crates/jet-pkg-model/src/Package/Blocks.rs", "package target registry");
  const bundler = readSource(root, "crates/jet-pkg-model/src/Bundler.rs", "package bundle registry");
  sources.package.push(sourceRecord(root, packageSource.path, packageSource.text, 48, packageSource.role));
  sources.package.push(sourceRecord(root, blocks.path, blocks.text, 614, blocks.role));
  sources.package.push(sourceRecord(root, bundler.path, bundler.text, 24, bundler.role));
  for (const variant of enumVariants(packageSource, "PackageOutputKind")) {
    addRow(rows, {
      id: `package.output-kind.${slug(variant.name)}`,
      family: "package",
      kind: "output-kind",
      label: variant.name,
      detail: "declared package output kind",
      source_identity: sourceIdentity(packageSource, variant.line),
    });
  }
  for (const field of structFields(packageSource, "PackageFacts")) {
    addRow(rows, {
      id: `package.fact.${slug(field.name)}`,
      family: "package",
      kind: "fact",
      label: field.name,
      detail: "PackageFacts field",
      source_identity: sourceIdentity(packageSource, field.line),
    });
  }
  for (const variant of enumVariants(blocks, "Target")) {
    addRow(rows, {
      id: `package.target.${slug(variant.name)}`,
      family: "package",
      kind: "target-kind",
      label: variant.name,
      detail: "declared package target kind",
      source_identity: sourceIdentity(blocks, variant.line),
    });
  }
  for (const variant of enumVariants(bundler, "BundleTarget")) {
    addRow(rows, {
      id: `package.bundle-target.${slug(variant.name)}`,
      family: "package",
      kind: "bundle-target",
      label: variant.name,
      detail: "declared package bundle target",
      source_identity: sourceIdentity(bundler, variant.line),
    });
  }
  if (!rowsHasFamily(rows, "package")) fail("inventory error", "package registries have no rows");
}

function parseFfi(root, rows, sources) {
  const source = readSource(root, "crates/jet-foundation/src/AST/ffi.rs", "foreign binder descriptor registry");
  sources.ffi.push(sourceRecord(root, source.path, source.text, 477, source.role));
  const regex = /binder\(\s*ForeignLanguage::([A-Za-z0-9_]+)\s*,\s*BinderRuntime::([A-Za-z0-9_]+)\s*,\s*BindingStubKind::([A-Za-z0-9_]+)\s*,\s*BinderStatus::([A-Za-z0-9_]+)\s*,\s*ForeignAbiContract::([A-Za-z0-9_]+)[\s\S]*?ForeignProvider::([A-Za-z0-9_]+)/g;
  for (const match of source.text.matchAll(regex)) {
    const language = match[1];
    addRow(rows, {
      id: `ffi.binder.${slug(language)}`,
      family: "ffi",
      kind: "binder",
      label: language,
      detail: {
        runtime: match[2],
        stub_kind: match[3],
        status: match[4],
        contract: match[5],
        provider: match[6],
      },
      source_identity: sourceIdentity(source, lineAt(source.text, match.index)),
    });
  }
  if (!rowsHasFamily(rows, "ffi")) fail("inventory error", "FOREIGN_BINDERS has no rows");
}

function walkFiles(root, directory, suffix = null) {
  const absolute = join(root, directory);
  if (!existsSync(absolute) || !statSync(absolute).isDirectory()) return [];
  const output = [];
  const visit = (current) => {
    const entries = readdirSync(current, { withFileTypes: true }).sort((left, right) => compareText(left.name, right.name));
    for (const entry of entries) {
      if (entry.name.startsWith(".")) continue;
      const child = join(current, entry.name);
      if (entry.isDirectory()) visit(child);
      else if (!suffix || entry.name.endsWith(suffix)) output.push(relPath(root, child));
    }
  };
  visit(absolute);
  return output.sort(compareText);
}

function commandSummary(root, args) {
  const command = join(root, "scripts/agent/jet-env");
  return [relPath(root, command), ...args].join(" ");
}

function runCommand(root, args, maxBuffer = 64 * 1024 * 1024) {
  const commandPath = join(root, "scripts/agent/jet-env");
  const scratch = join(process.env.HOME ?? root, ".cache/jet-test-scratch");
  mkdirSync(scratch, { recursive: true });
  try {
    const stdout = execFileSync(commandPath, args, {
      cwd: root,
      encoding: "utf8",
      maxBuffer,
      env: { ...process.env, TMPDIR: scratch },
    });
    return { status: "passed", exit_code: 0, stdout, stderr: "" };
  } catch (error) {
    return {
      status: "failed",
      exit_code: typeof error.status === "number" ? error.status : null,
      stdout: typeof error.stdout === "string" ? error.stdout : "",
      stderr: typeof error.stderr === "string" ? error.stderr : String(error.message ?? error),
    };
  }
}

function compactProcessResult(result) {
  return {
    status: result.status,
    exit_code: result.exit_code,
    diagnostic: firstDiagnosticLine(result),
    stdout_digest: digestText(result.stdout ?? ""),
    stdout_lines: (result.stdout ?? "").split("\n").filter(Boolean).length,
    stderr_digest: digestText(result.stderr ?? ""),
    stderr_lines: (result.stderr ?? "").split("\n").filter(Boolean).length,
  };
}

function loadExampleCensus(root, options) {
  const command = commandSummary(root, ["node", EXAMPLE_CENSUS, "--json"]);
  if (options.skipExamples) {
    return {
      status: "unavailable",
      command,
      reason: "example census was skipped by --skip-examples",
      report: null,
    };
  }
  const result = runCommand(root, ["node", EXAMPLE_CENSUS, "--json"]);
  if (result.status !== "passed") {
    return {
      status: "unavailable",
      command,
      reason: `example census exited ${result.exit_code ?? "without a status"}`,
      process: compactProcessResult(result),
      report: null,
    };
  }
  try {
    const report = JSON.parse(result.stdout);
    if (typeof report.schema !== "string" || !Array.isArray(report.records) || !Array.isArray(report.examples)) {
      fail("manifest error", "example census JSON has no records/examples envelope");
    }
    return {
      status: "available",
      command,
      reason: null,
      process: compactProcessResult(result),
      report,
    };
  } catch (error) {
    return {
      status: "unavailable",
      command,
      reason: `example census output was not JSON: ${error.message}`,
      process: compactProcessResult(result),
      report: null,
    };
  }
}
function loadSuiteManifest(root) {
  const path = "tests/suites.txt";
  const absolute = join(root, path);
  if (!existsSync(absolute)) {
    return {
      status: "unavailable",
      path,
      suites: [],
      entries: [],
      invalid: [],
      digest: null,
      reason: "tests/suites.txt is absent",
    };
  }
  const text = readFileSync(absolute, "utf8");
  const suites = [];
  const entries = [];
  const invalid = [];
  let suite = null;
  for (const [index, line] of text.split("\n").entries()) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const section = trimmed.match(/^([A-Za-z0-9_-]+):$/);
    if (section) {
      suite = section[1];
      suites.push({ name: suite, line: index + 1 });
      continue;
    }
    if (!suite || !line.startsWith(" ")) {
      invalid.push({ line: index + 1, text: line });
      continue;
    }
    entries.push({ suite, target: trimmed, line: index + 1 });
  }
  suites.sort((left, right) => compareText(left.name, right.name));
  entries.sort((left, right) => compareText(`${left.suite}:${left.target}`, `${right.suite}:${right.target}`));
  invalid.sort((left, right) => left.line - right.line);
  return {
    status: invalid.length === 0 ? "available" : "invalid",
    path,
    suites,
    entries,
    invalid,
    digest: digestText(text),
    reason: invalid.length === 0 ? null : `suite manifest has ${invalid.length} unscoped or malformed lines`,
  };
}

function exampleApplicable(row) {
  return row.family === "core" || (
    row.family === "language"
    && (row.kind === "tir-expr" || row.kind === "tir-stmt")
  );
}

function conformanceApplicable(row) {
  return row.family === "core";
}

function coverageStatus(row, example, conformance) {
  const evidence = [];
  if (exampleApplicable(row)) {
    evidence.push({
      manifest: "examples",
      status: example.status === "covered" ? "covered" : example.status,
    });
  }
  if (conformanceApplicable(row)) {
    evidence.push({
      manifest: "conformance",
      status: conformance.status === "matched" ? "covered" : conformance.status,
    });
  }
  if (evidence.length === 0) {
    return {
      status: "not-applicable",
      reason: "no current example or conformance manifest defines evidence for this registry family",
      evidence,
    };
  }
  if (evidence.some((entry) => entry.status === "covered")) {
    return { status: "covered", reason: null, evidence };
  }
  if (evidence.some((entry) => entry.status === "unavailable")) {
    return { status: "unavailable", reason: "an applicable evidence manifest is unavailable", evidence };
  }
  if (evidence.every((entry) => entry.status === "excluded")) {
    return { status: "excluded", reason: "the applicable conformance fixture is deliberately excluded", evidence };
  }
  return {
    status: "uncovered",
    reason: "applicable manifests contain no executable evidence for this registry row",
    evidence,
  };
}

function coverageSummary(records) {
  const rowCoverage = countStatuses(records.map((row) => row.coverage.status));
  const taskCoverage = countStatuses(records.flatMap((row) => AXES.map(() => row.coverage.status)));
  const examples = countStatuses(records.map((row) => {
    if (!exampleApplicable(row)) return "not-applicable";
    if (row.evidence.examples.status === "covered") return "covered";
    if (row.evidence.examples.status === "uncovered" || row.evidence.examples.status === "not-matched") return "uncovered";
    return row.evidence.examples.status;
  }));
  const conformance = countStatuses(records.map((row) => {
    if (!conformanceApplicable(row)) return "not-applicable";
    if (row.evidence.conformance.status === "matched") return "covered";
    if (row.evidence.conformance.status === "not-matched") return "uncovered";
    return row.evidence.conformance.status;
  }));
  return {
    rows: {
      total: records.length,
      covered: rowCoverage.covered ?? 0,
      uncovered: rowCoverage.uncovered ?? 0,
      unknown: rowCoverage.unknown ?? 0,
      unavailable: rowCoverage.unavailable ?? 0,
      stale: rowCoverage.stale ?? 0,
      excluded: rowCoverage.excluded ?? 0,
      not_applicable: rowCoverage["not-applicable"] ?? 0,
    },
    tasks: {
      total: records.length * AXES.length,
      covered: taskCoverage.covered ?? 0,
      uncovered: taskCoverage.uncovered ?? 0,
      unknown: taskCoverage.unknown ?? 0,
      unavailable: taskCoverage.unavailable ?? 0,
      stale: taskCoverage.stale ?? 0,
      excluded: taskCoverage.excluded ?? 0,
      not_applicable: taskCoverage["not-applicable"] ?? 0,
    },
    examples: {
      applicable: records.filter((row) => exampleApplicable(row)).length,
      covered: examples.covered ?? 0,
      uncovered: examples.uncovered ?? 0,
      unknown: examples.unknown ?? 0,
      unavailable: examples.unavailable ?? 0,
      not_applicable: examples["not-applicable"] ?? 0,
    },
    conformance: {
      applicable: records.filter((row) => conformanceApplicable(row)).length,
      covered: conformance.covered ?? 0,
      uncovered: conformance.uncovered ?? 0,
      unknown: conformance.unknown ?? 0,
      unavailable: conformance.unavailable ?? 0,
      excluded: conformance.excluded ?? 0,
      not_applicable: conformance["not-applicable"] ?? 0,
    },
  };
}

function observationSummary(records) {
  const tasks = records.flatMap((row) => AXES.map((axis) => row.tasks[axis]));
  const execution = countStatuses(tasks.map((task) => {
    const status = task.evidence.executed.status;
    if (status === "passed") return "covered";
    if (status === "failed") return "failed";
    if (status === "sampled-not-task-proven" || status === "not-requested") return "unknown";
    return status;
  }));
  const human = countStatuses(tasks.map((task) => {
    const status = task.evidence.human.status;
    if (status === "available") return "covered";
    if (status === "not-observed") return "unknown";
    return status;
  }));
  return {
    tasks: tasks.length,
    execution: {
      covered: execution.covered ?? 0,
      unknown: execution.unknown ?? 0,
      unavailable: execution.unavailable ?? 0,
      stale: execution.stale ?? 0,
      failed: execution.failed ?? 0,
    },
    human: {
      covered: human.covered ?? 0,
      unknown: human.unknown ?? 0,
      unavailable: human.unavailable ?? 0,
      stale: human.stale ?? 0,
    },
  };
}

function loadConformance(root) {
  const corpus = walkFiles(root, "tests/conformance/corpus", ".jet");
  const exclusionsPath = join(root, "tests/conformance/exclusions.tsv");
  const exclusions = [];
  if (existsSync(exclusionsPath)) {
    const text = readFileSync(exclusionsPath, "utf8");
    for (const [index, line] of text.split("\n").entries()) {
      if (!line.trim() || line.trim().startsWith("#")) continue;
      const [key, reason, owner, decision] = line.split("\t");
      if (!key || !reason || !owner || !decision) continue;
      exclusions.push({ key, reason, owner, decision, line: index + 1 });
    }
  }
  exclusions.sort((left, right) => compareText(left.key, right.key));
  return {
    status: corpus.length > 0 || exclusions.length > 0 ? "available" : "unavailable",
    manifest: "tests/conformance/exclusions.tsv",
    corpus_root: "tests/conformance/corpus",
    corpus_files: corpus,
    exclusions,
    reason: corpus.length > 0 || exclusions.length > 0 ? null : "conformance corpus and exclusions manifest are absent",
  };
}


function exampleEvidence(row, exampleCensus) {
  if (exampleCensus.status !== "available") {
    return { status: "unavailable", manifest: exampleCensus.command, references: [], reason: exampleCensus.reason };
  }
  const records = exampleCensus.report.records;
  const candidates = [];
  if (row.family === "core") {
    const keys = [];
    if (row.kind === "member" || row.kind === "call") keys.push(`plain:${row.module}.${row.label}`);
    if (row.kind === "receiver") keys.push(`receiver:${row.label}`);
    for (const key of keys) {
      for (const record of records) {
        if (record.family === "core" && record.key === key) candidates.push(record);
      }
    }
  } else if (row.family === "language" && (row.kind === "tir-expr" || row.kind === "tir-stmt")) {
    for (const record of records) {
      if (record.family === row.kind && record.construct === row.label) candidates.push(record);
    }
  }
  const references = [];
  for (const record of candidates) {
    for (const location of record.examples ?? []) {
      references.push({
        file: location.file,
        line: location.line,
        column: location.column,
        golden: location.golden,
        golden_paths: location.golden_paths,
        census_identity: record.identity,
      });
      if (references.length >= 3) break;
    }
    if (references.length >= 3) break;
  }
  if (references.length > 0) return { status: "covered", manifest: exampleCensus.command, references };
  if (candidates.length > 0) return { status: "uncovered", manifest: exampleCensus.command, references: [], reason: "matching example-census row has no executable example evidence" };
  return { status: "not-matched", manifest: exampleCensus.command, references: [], reason: "this registry row has no direct example-census identity" };
}

function conformanceEvidence(row, conformance) {
  if (conformance.status !== "available") return { status: "unavailable", manifest: conformance.manifest, references: [], reason: conformance.reason };
  const labels = [String(row.label).toLowerCase()];
  if (row.module) labels.push(String(row.module).split(".").at(-1).toLowerCase());
  const references = conformance.corpus_files
    .filter((file) => {
      const name = file.slice(file.lastIndexOf("/") + 1, file.length - ".jet".length).toLowerCase();
      return labels.includes(name);
    })
    .slice(0, 3);
  const exclusionKey = row.family === "core" && row.module
    ? `${row.module}.${row.label}`
    : null;
  const exclusion = exclusionKey ? conformance.exclusions.find((entry) => entry.key === exclusionKey) : null;
  if (references.length > 0 || exclusion) {
    return {
      status: exclusion ? "excluded" : "matched",
      manifest: conformance.manifest,
      references,
      exclusion: exclusion ?? null,
    };
  }
  return { status: "not-matched", manifest: conformance.manifest, references: [], exclusion: null };
}

function runtimeProbe(root, options, exampleCensus) {
  const binary = join(root, "target/debug/jet");
  const examples = exampleCensus.status === "available" ? exampleCensus.report.examples : [];
  const sample = examples.find((entry) => entry.file === RUNTIME_REPRESENTATIVE)?.file
    ?? examples.find((entry) => entry.golden)?.file
    ?? examples[0]?.file
    ?? null;
  const commandArgs = sample ? ["jet", "run", "--interpret", sample] : null;
  const command = commandSummary(root, commandArgs ?? ["jet", "--version"]);
  const sourceRevision = sample && existsSync(join(root, sample)) ? digestFile(join(root, sample)) : null;
  const authorityCommandArgs = ["jet", "run", "--interpret", RUNTIME_AUTHORITY_FIXTURE];
  const authorityCommand = commandSummary(root, authorityCommandArgs);
  const base = {
    command,
    binary: relPath(root, binary),
    binary_revision: digestFile(binary),
    source: sample,
    source_revision: sourceRevision,
    evidence_scope: sample
      ? "one passing representative executable example; the known authority fixture is probed separately for corpus defects; neither proves every census row"
      : "no representative executable example selected",
  };
  if (!existsSync(binary)) {
    return {
      ...base,
      status: "unavailable",
      reason: "target/debug/jet is absent; fresh compiler execution is unavailable",
      process: null,
      defect_probes: [],
      observed_defect: null,
    };
  }
  if (!sample) {
    return {
      ...base,
      status: "unavailable",
      reason: "no representative executable example was available; a version banner is not execution evidence",
      process: null,
      defect_probes: [],
      observed_defect: null,
    };
  }
  if (!options.execute) {
    return {
      ...base,
      status: "not-requested",
      reason: "target/debug/jet exists; pass --execute to run the representative task",
      process: null,
      defect_probes: [],
      observed_defect: null,
    };
  }
  const result = runCommand(root, commandArgs);
  const representativeDefect = result.status === "passed"
    ? null
    : classifyRuntimeDefect({
      command,
      source: sample,
      sourceRevision,
      result,
    });
  const defectProbes = [];
  if (sample !== RUNTIME_AUTHORITY_FIXTURE && existsSync(join(root, RUNTIME_AUTHORITY_FIXTURE))) {
    const authorityResult = runCommand(root, authorityCommandArgs);
    if (authorityResult.status !== "passed") {
      defectProbes.push({
        ...classifyRuntimeDefect({
          command: authorityCommand,
          source: RUNTIME_AUTHORITY_FIXTURE,
          sourceRevision: digestFile(join(root, RUNTIME_AUTHORITY_FIXTURE)),
          result: authorityResult,
        }),
        probe_kind: "known-corpus-defect",
      });
    }
  }
  const reason = representativeDefect
    ? `representative task exited ${result.exit_code ?? "without a status"}: ${representativeDefect.diagnostic.split("\n")[0] ?? "no diagnostic output"}`
    : defectProbes.length > 0
      ? `known corpus defect probe failed: ${defectProbes[0].diagnostic.split("\n")[0] ?? "no diagnostic output"}`
      : null;
  return {
    ...base,
    status: result.status === "passed" ? "passed" : "failed",
    reason,
    process: compactProcessResult(result),
    defect_probes: defectProbes,
    observed_defect: representativeDefect,
  };
}

function loadHumanRecord(pathValue, root) {
  if (!pathValue) {
    return {
      status: "unavailable",
      permanent: true,
      reason: HUMAN_ABSENCE_REASON,
      records: [],
      required_fields: HUMAN_FIELDS,
    };
  }
  const absolute = resolve(root, pathValue);
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(absolute, "utf8"));
  } catch (error) {
    return {
      status: "unavailable",
      permanent: false,
      reason: `cannot read human observation record ${pathValue}: ${error.message}`,
      records: [],
      required_fields: HUMAN_FIELDS,
    };
  }
  const records = Array.isArray(parsed)
    ? parsed
    : (parsed && typeof parsed === "object" ? parsed.records : null);
  if (!Array.isArray(records)) {
    return {
      status: "unavailable",
      permanent: false,
      reason: "human observation record must be an array or { records }",
      records: [],
      required_fields: HUMAN_FIELDS,
    };
  }
  const invalid = [];
  for (const [index, record] of records.entries()) {
    if (!record || typeof record !== "object" || Array.isArray(record)) {
      invalid.push(`${index}.record`);
      continue;
    }
    for (const field of HUMAN_FIELDS) if (!(field in record)) invalid.push(`${index}.${field}`);
    if (typeof record.task_id !== "string" || record.task_id.length === 0) invalid.push(`${index}.task_id`);
    if (record.participant !== "newcomer" && record.participant !== "experienced") invalid.push(`${index}.participant`);
    for (const field of ["completion", "prediction_correct", "transfer", "recovery"]) {
      if (typeof record[field] !== "boolean") invalid.push(`${index}.${field}`);
    }
    if (typeof record.latency_ms !== "number" || record.latency_ms < 0) invalid.push(`${index}.latency_ms`);
    if ("source_revision" in record && (typeof record.source_revision !== "string" || record.source_revision.length === 0)) {
      invalid.push(`${index}.source_revision`);
    }
  }
  if (invalid.length > 0) {
    return {
      status: "unavailable",
      permanent: false,
      reason: `human observation record is missing or invalid: ${invalid.slice(0, 12).join(", ")}`,
      records: [],
      required_fields: HUMAN_FIELDS,
    };
  }
  return {
    status: "available",
    permanent: false,
    reason: null,
    records: records.slice().sort((left, right) => compareText(
      `${left.participant}:${left.task_id}:${left.source_revision ?? ""}:${JSON.stringify(left)}`,
      `${right.participant}:${right.task_id}:${right.source_revision ?? ""}:${JSON.stringify(right)}`,
    )),
    required_fields: HUMAN_FIELDS,
    source: normalizePath(pathValue),
    source_revision: digestFile(absolute),
  };
}

function pathContract(row, runtime, taskId, oracle) {
  const editorApplicable = row.family !== "ffi";
  const cliCommand = row.family === "debugger"
    ? "jet debug <fixture.jet>"
    : "jet inspect compiler check <fixture.jet>";
  const identity = {
    task_id: taskId,
    source_identity: row.source_identity,
    oracle: oracle.kind,
    revision_rule: REVISION_RULE,
  };
  const editor = editorApplicable
    ? {
      ...identity,
      ai: false,
      studio: false,
      applicable: true,
      status: "unavailable",
      route: "EditorHost/EditorCommand",
      reason: "ordinary editor host was not run in this census",
    }
    : {
      ...identity,
      applicable: false,
      ai: false,
      studio: false,
      status: "n/a",
      route: null,
      reason: "foreign binder compilation has no ordinary editor execution route; use the CLI binding path",
    };
  return {
    ai: false,
    studio: false,
    cli: {
      ...identity,
      ai: false,
      studio: false,
      applicable: true,
      status: runtime.status === "passed" ? "sampled" : runtime.status,
      command: cliCommand,
      reason: runtime.status === "passed"
        ? "representative runtime only; task-specific semantics remain unproven"
        : runtime.reason,
    },
    editor,
  };
}

function expectedFor(row, axis) {
  const authority = `${row.source_identity.path}:${row.source_identity.line}`;
  if (axis === "explain") return `${row.label} is a ${row.family} ${row.kind} owned by ${authority}; its source registry, not a copied name list, defines the meaning.`;
  if (axis === "predict") return `A prior observation is valid only for the same source identity and revision; editing ${authority} invalidates that observation until the dependent projection is regenerated and re-run.`;
  if (axis === "modify") return `Edit the owning declaration at ${authority}, then regenerate or re-read dependent projections; do not edit a generated projection as the source of truth.`;
  return `Derive a neighboring task by reusing the ${row.family} registry and its identity prefix; the neighboring item must cite its own source row and revision.`;
}

function wrongModelFor(row, axis) {
  const subject = `${row.label} (${row.family}/${row.kind})`;
  if (axis === "explain") return `Treat ${subject} as a spelling that can be added in any parser or runtime module, instead of the owning registry row.`;
  if (axis === "predict") return `Assume an observation for ${subject} remains valid after the source changes because the name did not change.`;
  if (axis === "modify") return `Edit a generated projection or add a second list for ${subject}, instead of changing its owning declaration.`;
  return `Copy ${subject} into a hand-maintained teaching list; nearby surfaces do not need the same registry identity.`;
}

function buildTasks(row, runtime, human, example, conformance) {
  const tasks = {};
  for (const axis of AXES) {
    const expected = expectedFor(row, axis);
    const taskId = `${row.id}.${axis}`;
    const humanMatches = human.records.filter((record) => record.task_id === taskId);
    const currentMatches = humanMatches.filter((record) => (
      !record.source_revision || record.source_revision === row.source_identity.digest
    ));
    const staleMatches = humanMatches.filter((record) => (
      record.source_revision && record.source_revision !== row.source_identity.digest
    ));
    let humanEvidence;
    if (currentMatches.length > 0) {
      humanEvidence = {
        status: "available",
        records: currentMatches.length,
        observations: currentMatches,
        stale_records: staleMatches.length,
      };
    } else if (staleMatches.length > 0) {
      humanEvidence = {
        status: "stale",
        records: 0,
        observations: [],
        stale_observations: staleMatches,
        current_source_revision: row.source_identity.digest,
        reason: "human observation source revision differs from the current owning registry",
      };
    } else if (human.status === "available") {
      humanEvidence = {
        status: "unknown",
        records: 0,
        reason: "human observation record exists, but this task has not been observed",
      };
    } else {
      humanEvidence = {
        status: "unavailable",
        records: 0,
        reason: human.reason,
        permanent: human.permanent ?? false,
      };
    }
    const oracle = {
      deterministic: true,
      kind: "source-and-revision",
      answer: expected,
      distinguishes: `A response is correct only when it names ${row.source_identity.path}:${row.source_identity.line} and uses revision ${row.source_identity.digest}.`,
      source_identity: row.source_identity,
      runtime_identity: runtime,
      example_evidence: example,
      conformance_evidence: conformance,
    };
    const failedHuman = currentMatches.filter((record) => (
      record.completion === false
      || record.prediction_correct === false
      || record.transfer === false
      || record.recovery === false
    ));
    tasks[axis] = {
      id: taskId,
      axis,
      concept: row.label,
      prerequisites: [
        {
          kind: "source-identity",
          source_identity: row.source_identity,
          requirement: "identify the owning registry row before changing a projection",
        },
      ],
      prompt: `${axis[0].toUpperCase()}${axis.slice(1)} ${row.label} using the owning registry and the current source revision.`,
      expected,
      wrong_model: wrongModelFor(row, axis),
      source_identity: row.source_identity,
      input_identity: row.source_identity,
      runtime_identity: runtime,
      oracle,
      reveal: {
        policy: "predict_then_reveal",
        evidence: "show only checked execution or an explicit unavailable/stale state",
        reason: "tie the response to the owning source identity and current revision",
        counterexample: `change ${row.source_identity.path}:${row.source_identity.line} and re-run the dependent projection`,
      },
      controlled_edit: {
        required: true,
        instruction: `edit the owning source row at ${row.source_identity.path}:${row.source_identity.line}`,
        source_identity: row.source_identity,
      },
      transfer: {
        required: true,
        instruction: "derive a structurally different neighboring task from the same canonical relation",
        source_identity: row.source_identity,
      },
      accessibility: {
        keyboard: true,
        text_only: true,
        reduced_motion: true,
        no_auto_quiz: true,
      },
      derivation: {
        relation: "EvidenceReport.derivations",
        status: "runtime-dependent",
        record_ref: null,
      },
      evidence: {
        modeled: {
          status: "available",
          basis: "canonical source registry and source-revision policy",
          source_identity: row.source_identity,
        },
        executed: {
          status: runtime.status === "passed" ? "sampled-not-task-proven" : runtime.status,
          reason: runtime.reason,
          command: runtime.command,
          runtime_identity: runtime,
        },
        human: humanEvidence,
      },
      coverage: row.coverage,
      paths: pathContract(row, runtime, taskId, oracle),
      failure: {
        status: failedHuman.length > 0 ? "observed" : "none-observed",
        observations: failedHuman,
        smallest_fix: null,
        generalization: null,
        owner_card: null,
      },
    };
  }
  return tasks;
}

function makeReport(root, options) {
  const capabilityRelation = buildCapabilityRelation({
    root,
    sourceSnapshotHash: sourceSnapshot(root).hash,
  });
  const rows = new Map();
  const sources = Object.fromEntries(FAMILY_ORDER.map((family) => [family, []]));
  const sourceSeen = new Set();
  for (const identity of capabilityRelation.identities) {
    const capabilityRows = capabilityRelation.rows.filter((row) => row.capability_id === identity.capability_id);
    const learningId = identity.legacy_stable_ids?.[0] || identity.capability_id;
    rows.set(identity.capability_id, {
      ...identity,
      id: learningId,
      capability_id: identity.capability_id,
      capability_rows: capabilityRows.map((row) => row.row_id),
      capability_dispositions: capabilityRows.map((row) => ({ mode: row.mode, disposition: row.disposition })),
    });
    const source = identity.source_identity;
    if (source && !sourceSeen.has(source.path)) {
      sourceSeen.add(source.path);
      sources[identity.family].push({
        path: source.path,
        line: source.line,
        digest: source.digest,
        role: source.authority,
        source_of_truth: source.projection?.path || null,
      });
    }
  }
  for (const family of FAMILY_ORDER) sources[family].sort((left, right) => compareText(`${left.path}:${left.line}`, `${right.path}:${right.line}`));

  const learningCurriculum = loadLearningCurriculum(root);
  const exampleCensus = loadExampleCensus(root, options);
  const conformance = loadConformance(root);
  const suiteManifest = loadSuiteManifest(root);
  const runtime = runtimeProbe(root, options, exampleCensus);
  const human = loadHumanRecord(options.humanRecord, root);
  const inventoryRows = [...rows.values()].sort((left, right) => {
    const family = FAMILY_ORDER.indexOf(left.family) - FAMILY_ORDER.indexOf(right.family);
    return family || compareText(left.id, right.id);
  });
  const records = inventoryRows.map((row) => {
    const example = exampleEvidence(row, exampleCensus);
    const conformanceMatch = conformanceEvidence(row, conformance);
    const coverage = coverageStatus(row, example, conformanceMatch);
    const record = {
      ...row,
      coverage,
      obligations: AXES.map((axis) => ({
        axis,
        status: coverage.status === "not-applicable" ? "not-applicable" : "required",
        na_reason: coverage.status === "not-applicable" ? coverage.reason : null,
      })),
      evidence: {
        examples: example,
        conformance: conformanceMatch,
      },
      observed_defects: [],
    };
    record.tasks = buildTasks({ ...row, coverage }, runtime, human, example, conformanceMatch);
    return record;
  });

  const byFamily = {};
  for (const family of FAMILY_ORDER) {
    const familyRows = records.filter((row) => row.family === family);
    const familyCoverage = countStatuses(familyRows.map((row) => row.coverage.status));
    byFamily[family] = {
      rows: familyRows.length,
      tasks: familyRows.length * AXES.length,
      obligations: familyRows.length * AXES.length,
      covered: familyCoverage.covered ?? 0,
      uncovered: familyCoverage.uncovered ?? 0,
      unknown: familyCoverage.unknown ?? 0,
      unavailable: familyCoverage.unavailable ?? 0,
      stale: familyCoverage.stale ?? 0,
      excluded: familyCoverage.excluded ?? 0,
      not_applicable: familyCoverage["not-applicable"] ?? 0,
    };
  }
  const taskCount = records.length * AXES.length;
  const coverage = coverageSummary(records);
  const observations = observationSummary(records);
  const observedFailures = [];
  const failureGaps = [];
  const staleObservations = [];
  const unmatchedObservations = [];
  const failureSeen = new Set();
  const taskById = new Map(records.flatMap((row) => AXES.map((axis) => [row.tasks[axis].id, row.tasks[axis]])));
  for (const observation of human.records) {
    const task = taskById.get(observation.task_id);
    if (!task) {
      unmatchedObservations.push({
        task_id: observation.task_id,
        participant: observation.participant,
        reason: "human observation does not name a task in the current census",
      });
      continue;
    }
    if (observation.source_revision && observation.source_revision !== task.source_identity.digest) {
      staleObservations.push({
        task_id: observation.task_id,
        participant: observation.participant,
        source_revision: observation.source_revision,
        current_source_revision: task.source_identity.digest,
        reason: "human observation is bound to an older source revision",
      });
      continue;
    }
    const failed = observation.completion === false
      || observation.prediction_correct === false
      || observation.transfer === false
      || observation.recovery === false;
    if (!failed) continue;
    const key = `${observation.participant}:${observation.task_id}:${observation.source_revision ?? task.source_identity.digest}`;
    if (failureSeen.has(key)) continue;
    failureSeen.add(key);
    const complete = typeof observation.misconception === "string"
      && observation.misconception.length > 0
      && Array.isArray(observation.neighboring_tasks)
      && observation.neighboring_tasks.length > 0
      && typeof observation.smallest_fix === "string"
      && observation.smallest_fix.length > 0
      && typeof observation.owner_card === "string"
      && observation.owner_card.length > 0;
    if (!complete) {
      failureGaps.push({
        task_id: observation.task_id,
        participant: observation.participant,
        reason: "failure requires misconception, neighboring_tasks, smallest_fix, and one live owner_card",
      });
    }
    observedFailures.push({
      task_id: observation.task_id,
      participant: observation.participant,
      source_revision: observation.source_revision ?? task.source_identity.digest,
      misconception: observation.misconception ?? null,
      neighboring_tasks: observation.neighboring_tasks ?? [],
      smallest_fix: observation.smallest_fix ?? null,
      owner_card: observation.owner_card ?? null,
      assignment_status: complete ? "assigned" : "blocked",
    });
  }
  failureGaps.sort((left, right) => compareText(`${left.participant}:${left.task_id}`, `${right.participant}:${right.task_id}`));
  staleObservations.sort((left, right) => compareText(`${left.participant}:${left.task_id}`, `${right.participant}:${right.task_id}`));
  unmatchedObservations.sort((left, right) => compareText(`${left.participant}:${left.task_id}`, `${right.participant}:${right.task_id}`));
  const allFamiliesPresent = FAMILY_ORDER.every((family) => rowsHasFamily(rows, family));
  const learningProjection = projectLearningCurriculum(learningCurriculum, taskById);
  const taskContractComplete = records.every((row) => AXES.every((axis) => {
    const task = row.tasks[axis];
    return task.oracle.deterministic
      && typeof task.wrong_model === "string"
      && task.wrong_model.length > 0
      && typeof task.oracle.distinguishes === "string"
      && task.oracle.distinguishes.length > 0
      && task.source_identity?.path
      && task.source_identity?.digest
      && task.runtime_identity?.command
      && task.input_identity?.path
      && task.reveal?.policy === "predict_then_reveal"
      && task.controlled_edit?.required === true
      && task.transfer?.required === true
      && task.accessibility?.keyboard === true
      && task.accessibility?.text_only === true
      && task.accessibility?.no_auto_quiz === true;
  }));
  const pathContractValid = records.every((row) => AXES.every((axis) => {
    const task = row.tasks[axis];
    const { cli, editor } = task.paths;
    const cliMatches = cli.task_id === task.id
      && cli.source_identity.path === task.source_identity.path
      && cli.source_identity.digest === task.source_identity.digest
      && cli.oracle === task.oracle.kind
      && cli.revision_rule === REVISION_RULE;
    const editorMatches = editor.task_id === task.id
      && editor.source_identity.path === task.source_identity.path
      && editor.source_identity.digest === task.source_identity.digest
      && editor.oracle === task.oracle.kind
      && editor.revision_rule === REVISION_RULE;
    return cliMatches && editorMatches;
  }));
  const currentHumanObservations = human.records.filter((observation) => {
    const task = taskById.get(observation.task_id);
    return task && (!observation.source_revision || observation.source_revision === task.source_identity.digest);
  });
  const humanEvidenceAvailable = human.status === "available" && currentHumanObservations.length > 0;
  const runtimeDefects = [
    ...(runtime.observed_defect ? [runtime.observed_defect] : []),
    ...(runtime.defect_probes ?? []),
  ];
  const runtimeUnassignedDefects = runtimeDefects.filter((defect) => defect.assignment_status !== "assigned");
  const humanFailuresAssigned = failureGaps.length === 0 && observedFailures.every((failure) => failure.assignment_status === "assigned");
  const allObservedFailuresAssigned = humanFailuresAssigned && runtimeUnassignedDefects.length === 0;
  const runtimeSamplingValid = runtime.status === "passed" && runtimeDefects.length === 0;
  const acceptance = {
    "1": {
      status: allFamiliesPresent ? "reported" : "blocked",
      evidence: "derived rows cover language, core, CLI, editor, debugger, build, package, and FFI registries",
      families: FAMILY_ORDER,
      coverage: coverage.rows,
    },
    "2": {
      status: taskContractComplete ? "reported" : "blocked",
      evidence: "every axis has a deterministic source/revision oracle, a distinguishing wrong model, and source/runtime identities",
    },
    "3": {
      status: pathContractValid && runtimeSamplingValid ? "sampled" : "blocked",
      evidence: "CLI and ordinary editor routes carry the same task identity, source identity, oracle, and revision rule without AI or studio",
      reason: runtime.status !== "passed"
        ? runtime.reason
        : (runtimeDefects.length > 0 ? runtime.reason : null),
    },
    "4": {
      status: humanEvidenceAvailable && allObservedFailuresAssigned ? "reported" : "blocked",
      evidence: "modeled, executed, and human evidence are separate; observed failures require a misconception, neighboring tasks, smallest fix, and one owner card",
      reason: !humanEvidenceAvailable
        ? (human.status === "available" ? "human observation record has no current task-bound observation" : human.reason)
        : (allObservedFailuresAssigned ? null : "one or more observed failures lacks complete generalization or owner assignment"),
    },
  };
  const report = {
    capability_relation: {
      schema: capabilityRelation.schema || CAPABILITY_RELATION_SCHEMA,
      relation: "capability_relation.rows",
      relation_source: "scripts/agent/hardening-manifest.mjs",
      source_snapshot_hash: capabilityRelation.source_snapshot_hash,
      content_digest: capabilityRelation.content_digest,
      denominator: capabilityRelation.denominator,
      rows: capabilityRelation.rows,
    },
    schema: SCHEMA,
    generated_by: "scripts/agent/learning-census.mjs",
    generated_static_only: runtime.status !== "passed",
    status: runtimeSamplingValid && humanEvidenceAvailable && allObservedFailuresAssigned ? "ready" : "blocked",
    source_inventory: {
      authority: "canonical capability relation identities",
      families: sources,
      inventory_digest: digestJson(sources),
      rows: records.length,
    },
    learning_curriculum: learningProjection,
    manifests: {
      examples: {
        schema: exampleCensus.report?.schema ?? null,
        status: exampleCensus.status,
        command: exampleCensus.command,
        reason: exampleCensus.reason,
        summary: exampleCensus.report?.summary ?? null,
      },
      conformance,
      suite_manifest: suiteManifest,
    },
    execution: {
      runtime,
      human,
      coverage,
      observations,
      stale_observations: staleObservations,
      unmatched_observations: unmatchedObservations,
      policy: "unknown means an observation was not requested or not recorded; unavailable means the required runner or human evidence cannot be supplied; stale means the recorded source revision no longer matches; modeled or agent success is not execution or human-transfer evidence",
    },
    path_contract: {
      cli: { ordinary: true, ai: false, studio: false, route: "jet inspect/check/run/debug" },
      editor: { ordinary: true, ai: false, studio: false, route: "EditorHost and EditorCommand" },
      equivalence: "the same task id, source identity, oracle, and revision rule must be used on both paths",
      revision_rule: REVISION_RULE,
    },
    summary: {
      rows: records.length,
      tasks: taskCount,
      covered: coverage.tasks.covered,
      uncovered: coverage.tasks.uncovered,
      not_applicable: coverage.tasks.not_applicable,
      excluded: coverage.tasks.excluded,
      unknown_tasks: observations.execution.unknown + observations.human.unknown,
      unavailable_tasks: observations.execution.unavailable + observations.human.unavailable,
      stale_tasks: observations.execution.stale + observations.human.stale,
      execution_failed_tasks: observations.execution.failed,
      human_unavailable_tasks: observations.human.unavailable,
      human_observations: human.records.length,
      current_human_observations: currentHumanObservations.length,
      observed_defects: observedFailures.length + runtimeDefects.length,
      unassigned_defects: failureGaps.length + runtimeUnassignedDefects.length,
      stale_observations: staleObservations.length,
      unmatched_observations: unmatchedObservations.length,
      families: byFamily,
      all_families_present: allFamiliesPresent,
    },
    acceptance,
    records,
    modeled_frictions: {
      status: "modeled-only",
      rows: records.length,
      tasks: taskCount,
      coverage,
      generalization: "Unavailable execution or human observation is an evidence gap, not a learner defect; a human failure must include neighboring_tasks, smallest_fix, and one deduplicated live owner_card.",
    },
    observed_failures: observedFailures,
    failure_gaps: failureGaps,
    stale_observations: staleObservations,
    unmatched_observations: unmatchedObservations,
    runtime_defects: runtimeDefects,
    defect_policy: {
      deduplicate_by: "task_id + participant + source revision",
      assignment: "an observed human defect must carry one live owner_card; runtime defects carry an owning lane or an explicit unassigned corpus owner pending carding",
      current: observedFailures,
      runtime: runtimeDefects,
    },
    revision_policy: {
      source_edit: REVISION_RULE,
      generated_projection: "regenerate from the owner source; never use a projection as a new authority",
    },
  };
  return report;
}

function usage() {
  return [
    "usage: learning-census.mjs [--json|--human] [--root PATH] [--execute] [--human-record PATH] [--skip-examples] [--fail-on-unknown] [--out PATH]",
    "",
    "Derive the explain/predict/modify/derive census from canonical Jet registries.",
    "Default output is human-readable; --json emits the stable machine envelope.",
    "--execute runs the passing representative `jet run --interpret` probe and the known authority fixture when target/debug/jet exists.",
    "--human-record accepts {records:[...]} with participant/task evidence; agent/model output is never human evidence.",
    "--fail-on-unknown exits 1 when unknown, unavailable, stale, or failed evidence remains.",
  ].join("\n");
}

function parseArgs(argv) {
  const options = {
    root: DEFAULT_ROOT,
    format: "human",
    execute: false,
    humanRecord: null,
    skipExamples: false,
    failOnUnknown: false,
    out: null,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") options.help = true;
    else if (arg === "--json" || arg === "--format=json") options.format = "json";
    else if (arg === "--human" || arg === "--format=human") options.format = "human";
    else if (arg === "--execute") options.execute = true;
    else if (arg === "--skip-examples") options.skipExamples = true;
    else if (arg === "--fail-on-unknown") options.failOnUnknown = true;
    else if (arg === "--root") {
      index += 1;
      if (!argv[index]) fail("usage", "--root requires a path");
      options.root = resolve(argv[index]);
    } else if (arg.startsWith("--root=")) options.root = resolve(arg.slice("--root=".length));
    else if (arg === "--human-record") {
      index += 1;
      if (!argv[index]) fail("usage", "--human-record requires a path");
      options.humanRecord = argv[index];
    } else if (arg.startsWith("--human-record=")) options.humanRecord = arg.slice("--human-record=".length);
    else if (arg === "--out") {
      index += 1;
      if (!argv[index]) fail("usage", "--out requires a path");
      options.out = argv[index];
    } else if (arg.startsWith("--out=")) options.out = arg.slice("--out=".length);
    else fail("usage", `unknown option ${arg}`);
  }
  return options;
}

function renderHuman(report) {
  const lines = [
    `schema=${report.schema} status=${report.status}`,
    `rows=${report.summary.rows} tasks=${report.summary.tasks} covered=${report.summary.covered} uncovered=${report.summary.uncovered} not_applicable=${report.summary.not_applicable} excluded=${report.summary.excluded}`,
    `evidence=unknown=${report.summary.unknown_tasks} unavailable=${report.summary.unavailable_tasks} stale=${report.summary.stale_tasks} failed=${report.summary.execution_failed_tasks} human_unavailable=${report.summary.human_unavailable_tasks} human_observations=${report.summary.human_observations} current_human_observations=${report.summary.current_human_observations}`,
    `coverage=examples_covered=${report.execution.coverage.examples.covered} examples_uncovered=${report.execution.coverage.examples.uncovered} conformance_covered=${report.execution.coverage.conformance.covered} conformance_uncovered=${report.execution.coverage.conformance.uncovered}`,
    ...FAMILY_ORDER.map((family) => {
      const summary = report.summary.families[family];
      return `family=${family} rows=${summary.rows} tasks=${summary.tasks} covered=${summary.covered} uncovered=${summary.uncovered}`;
    }),
    `examples=${report.manifests.examples.status} conformance=${report.manifests.conformance.status} suites=${report.manifests.suite_manifest.status} runtime=${report.execution.runtime.status} human=${report.execution.human.status}`,
    `acceptance=1:${report.acceptance["1"].status} 2:${report.acceptance["2"].status} 3:${report.acceptance["3"].status} 4:${report.acceptance["4"].status}`,
    `CHECK OK schema=${report.schema} rows=${report.summary.rows} tasks=${report.summary.tasks} covered=${report.summary.covered} uncovered=${report.summary.uncovered} unknown=${report.summary.unknown_tasks} unavailable=${report.summary.unavailable_tasks} stale=${report.summary.stale_tasks} failed=${report.summary.execution_failed_tasks} status=${report.status}`,
  ];
  return `${lines.join("\n")}\n`;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return;
  }
  const report = makeReport(options.root, options);
  const serialized = options.format === "json" ? `${JSON.stringify(report, null, 2)}\n` : renderHuman(report);
  if (options.out) {
    const destination = isAbsolute(options.out) ? options.out : resolve(options.root, options.out);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, serialized);
    process.stdout.write(`WRITE OK path=${relPath(options.root, destination)} format=${options.format}\n`);
  } else {
    process.stdout.write(serialized);
  }
  if (options.failOnUnknown && (
    report.summary.unknown_tasks > 0
    || report.summary.unavailable_tasks > 0
    || report.summary.stale_tasks > 0
    || report.summary.execution_failed_tasks > 0
  )) process.exitCode = 1;
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`learning-census: ${message}\n`);
  process.exitCode = error instanceof CensusError && error.kind === "usage" ? 2 : 1;
}
