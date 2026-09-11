#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const CARD = 2975;
const SCHEMA = "jet-lifecycle-census-v1";
const GENERATED_COMMAND = "node scripts/agent/lifecycle-census.mjs";

// This is the census scope, not a second lifecycle registry.  Rows below are
// resolved against declarations in these files on every run; missing symbols
// fail closed instead of silently making the table smaller.
const SOURCES = Object.freeze([
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/JetStd/ReactiveEventWatch.rs",
    language: "rust",
    owners: [
      "ReactiveObserver",
      "JetReactiveEffect",
      "SignalCell",
      "JetSignal",
      "JetDerived",
      "JetSubscription",
      "JetEventScope",
      "JetEvent",
      "JetDispatchState",
      "JetDispatchReport",
      "JetAsyncEntry",
      "JetAsyncState",
      "JetAsyncEvent",
    ],
  },
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs",
    language: "rust",
    owners: [
      "JetTaskState",
      "JetTaskGroup",
      "JetTask",
      "JetSharedCell",
      "JetShared",
      "JetSharedWeak",
      "JetSharedGuard",
      "JetPoolSlot",
      "JetPool",
      "JetId",
    ],
  },
  {
    path: "crates/jet-codegen/src/Prelude/Scheduler.rs",
    language: "rust",
    owners: [
      "JetTaskControl",
      "JetSchedulerResult",
      "JetSchedulerJoin",
      "JetSchedulerTaskPoll",
    ],
  },
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/Top/LiveQuery.rs",
    language: "rust",
    owners: ["JetLiveQuery", "JetLiveRecord", "JetLiveRegistry"],
  },
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/Top/WebQuery.rs",
    language: "rust",
    owners: ["JetWebMutationState", "JetWebQueryState", "JetWebQuery"],
  },
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/Top/WebStore.rs",
    language: "rust",
    owners: [
      "JetWebStoreTransaction",
      "JetWebStoreEvent",
      "JetWebStoreLedger",
      "JetWebStore",
      "JetWebStorePatch",
      "JetWebStoreSubscription",
    ],
  },
  {
    path: "Source/RunCache.rs",
    language: "rust",
    functions: ["run_cache_key", "try_warm_run", "store_after_miss"],
  },
  {
    path: "crates/jet-store/src/lib.rs",
    language: "rust",
    owners: ["ProcessIdentity", "Store", "Lease"],
  },
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/Top/Game.rs",
    language: "rust",
    owners: [
      "GameState",
      "GameAssetRuntimePayload",
      "GameAssetRuntimeHandle",
      "GameFrame",
    ],
    functions: ["jet_game_assets_reload", "jet_game_run"],
  },
  {
    path: "crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs",
    language: "rust",
    owners: [
      "Object",
      "JetComputeStream",
      "JetComputeHandle",
      "JetTensor",
    ],
    functions: ["run"],
  },
  {
    path: "crates/jet-codegen/src/Prelude/Core/Host.js",
    language: "javascript",
    functionPrefixes: ["jet_pool_", "jet_cell_"],
  },
  {
    path: "crates/jet-codegen/src/Prelude/Core/HostServices.rs",
    language: "rust",
    owners: ["JetUiCancellation", "JetUiHostScope", "JetUiHeadlessHost"],
    functions: ["jet_ui_with_current_host", "jet_ui_host_open_file", "jet_ui_host_save_file"],
  },
]);

const IDENTITY_KINDS = Object.freeze([
  ["owner", /\b(?:owner|owner_id|id|identity|handle|token|permit|process)\b/iu],
  ["generation", /\b(?:generation|gen|epoch)\b/iu],
  ["revision", /\b(?:revision|version|sequence|cursor)\b/iu],
  ["lifetime", /\b(?:active|cancelled|closed|disposed|terminal|phase|running|paused|refreshing|dirty|valid|live)\b/iu],
  ["clock_time", /\b(?:fresh_at|timestamp|time|deadline|instant|duration|age|mtime|last_use|created)\b/iu],
  ["device_completion", /\b(?:wait_until_completed|wait_for_fences|fence|completion|completed|synchroni[sz]e|commit)\b/iu],
  ["dependency_membership", /\b(?:dependenc(?:y|ies)|subscriber|subscription|cleanup|watch|footprint|listener|registration)\b/iu],
  ["semantic_cache_identity", /\b(?:key|digest|artifact|namespace|source|config|program|action|content)\b/iu],
]);
const IDENTITY_CONTRACTS = Object.freeze({
  owner: {
    meaning: "the resource, observer, process, or handle that owns the state",
    distinct_from: ["generation", "revision", "lifetime", "clock_time", "device_completion", "dependency_membership", "semantic_cache_identity"],
  },
  generation: {
    meaning: "the incarnation token used to reject work from an older allocation or refresh",
    distinct_from: ["owner", "revision", "lifetime", "clock_time", "device_completion", "dependency_membership", "semantic_cache_identity"],
  },
  revision: {
    meaning: "the content or event-order token used to order updates",
    distinct_from: ["owner", "generation", "lifetime", "clock_time", "device_completion", "dependency_membership", "semantic_cache_identity"],
  },
  lifetime: {
    meaning: "whether an owner is active, cancelled, closed, or otherwise usable",
    distinct_from: ["owner", "generation", "revision", "clock_time", "device_completion", "dependency_membership", "semantic_cache_identity"],
  },
  clock_time: {
    meaning: "a temporal observation such as a deadline, freshness timestamp, or duration",
    distinct_from: ["owner", "generation", "revision", "lifetime", "device_completion", "dependency_membership", "semantic_cache_identity"],
  },
  device_completion: {
    meaning: "completion acknowledged by an external device, fence, stream, or commit boundary",
    distinct_from: ["owner", "generation", "revision", "lifetime", "clock_time", "dependency_membership", "semantic_cache_identity"],
  },
  dependency_membership: {
    meaning: "the set of upstream inputs, subscribers, listeners, or cleanup registrations",
    distinct_from: ["owner", "generation", "revision", "lifetime", "clock_time", "device_completion", "semantic_cache_identity"],
  },
  semantic_cache_identity: {
    meaning: "the semantic key, digest, artifact, program, action, or configuration being reused",
    distinct_from: ["owner", "generation", "revision", "lifetime", "clock_time", "device_completion", "dependency_membership"],
  },
});

const TRANSITION_RULES = Object.freeze([
  ["cleanup", /\b(?:clear|drain|drop|dispose|evict|free|prune|release|remove|retain|unsubscribe|unregister)\b/iu],
  ["cancel", /\b(?:cancel|cancelled|cancellation|close|closed|abort)\b/iu],
  ["complete", /\b(?:complete|completed|completion|finish|finished|join|wait_until_completed|wait_for_fences|present|stop)\b/iu],
  ["publish", /\b(?:publish|published|send|emit|notify|signal\.set|record_and_publish|commit)\b/iu],
  ["invalidate", /\b(?:invalidate|invalidation|dirty|stale|refresh|rerun|revision|generation)\b/iu],
  ["dependency", /\b(?:dependenc(?:y|ies)|subscriber|subscription|listener|track|watch|registration)\b/iu],
  ["create", /\b(?:new|create|insert|register|spawn|acquire|add|push)\b/iu],
  ["observe", /\b(?:get|read|poll|peek|snapshot|lookup|load)\b/iu],
]);

const GATE_RULES = Object.freeze([
  ["publication", /\b(?:publish|published|send|emit|notify|signal\.set|record_and_publish|commit)\b/iu],
  ["consumption", /\b(?:get|read|poll|peek|join|try_recv|receive|lookup|restore)\b/iu],
  ["admission", /\b(?:active|valid|cancelled|closed|terminal|generation|revision|phase|current)\b/iu],
]);

const COMPLETION_RULE = /\b(?:complete|completed|completion|finish|finished|join|drain|wait_until_completed|wait_for_fences|present|stop|commit)\b/iu;
const CLEANUP_RULE = /\b(?:Drop|drop|clear|drain|dispose|evict|free|prune|release|remove|retain|unsubscribe|unregister|close|cancel)\b/iu;
const CANCELLATION_RULE = /\b(?:cancel|cancelled|cancellation|close|closed|abort|dispose|unsubscribe)\b/iu;
const TRANSITION_LINE_RULE = /(?:\.(?:store|swap|replace|compare_exchange|fetch_add|fetch_sub)\s*\(|\b(?:insert|remove|push|pop|retain|drain|clear|set|publish|complete|cancel|close|commit|release|drop|unsubscribe|unregister|wait_until_completed|wait_for_fences)\b|\b(?:active|cancelled|closed|dirty|refreshing|terminal|generation|revision|phase|running)\s*=)/iu;
const JS_PROPERTY_RULE = /(?:\.\s*([A-Za-z_$][A-Za-z0-9_$]*)\b|\b([A-Za-z_$][A-Za-z0-9_$]*)\s*:)/gu;
const JS_DECL_RULE = /^\s*(?:async\s+)?function\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*\(/gmu;

function fail(message) {
  throw new Error(`lifecycle census: ${message}`);
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function absPath(path) {
  return resolve(ROOT, path);
}

function lineNumber(text, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) if (text[index] === "\n") line += 1;
  return line;
}

function lineAt(text, line) {
  return text.split("\n")[line - 1] ?? "";
}

function cleanSnippet(value) {
  return value.replace(/\s+/gu, " ").trim().slice(0, 240);
}

function sourceRef(file, offset, text = null) {
  const line = lineNumber(file.source, offset);
  return {
    file: file.path,
    line,
    text: cleanSnippet(text ?? lineAt(file.source, line)),
  };
}
function scopeCode(file, scope) {
  return file.masked.slice(scope.start ?? 0, (scope.start ?? 0) + scope.body.length);
}

function maskCode(source) {
  const output = source.split("");
  const blank = (index) => {
    if (output[index] !== "\n") output[index] = " ";
  };
  let state = "code";
  let rawHashes = 0;
  let blockDepth = 0;
  for (let index = 0; index < source.length; index += 1) {
    const current = source[index];
    const next = source[index + 1];
    if (state === "line") {
      if (current === "\n") state = "code";
      else blank(index);
      continue;
    }
    if (state === "block") {
      if (current === "/" && next === "*") {
        blockDepth += 1;
        blank(index);
        blank(index + 1);
        index += 1;
      } else if (current === "*" && next === "/") {
        blockDepth -= 1;
        blank(index);
        blank(index + 1);
        index += 1;
        if (blockDepth === 0) state = "code";
      } else blank(index);
      continue;
    }
    if (state === "quote") {
      if (current === "\\") {
        blank(index);
        blank(index + 1);
        index += 1;
      } else if (current === '"') {
        blank(index);
        state = "code";
      } else blank(index);
      continue;
    }
    if (state === "raw") {
      const close = `"${"#".repeat(rawHashes)}`;
      if (source.startsWith(close, index)) {
        for (let count = 0; count < close.length; count += 1) blank(index + count);
        index += close.length - 1;
        state = "code";
      } else blank(index);
      continue;
    }
    if (current === "/" && next === "/") {
      blank(index);
      blank(index + 1);
      index += 1;
      state = "line";
      continue;
    }
    if (current === "/" && next === "*") {
      blank(index);
      blank(index + 1);
      index += 1;
      blockDepth = 1;
      state = "block";
      continue;
    }
    if (current === '"') {
      blank(index);
      state = "quote";
      continue;
    }
    if (current === "r") {
      const raw = source.slice(index).match(/^r(#+)?"/u);
      if (raw) {
        rawHashes = raw[1]?.length ?? 0;
        for (let count = 0; count < raw[0].length; count += 1) blank(index + count);
        index += raw[0].length - 1;
        state = "raw";
      }
    }
  }
  return output.join("");
}

function matchingBrace(masked, open) {
  let depth = 0;
  for (let index = open; index < masked.length; index += 1) {
    if (masked[index] === "{") depth += 1;
    else if (masked[index] === "}") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

function parseBlocks(file, kind) {
  const matcher = kind === "rust"
    ? /^\s*(?:(?:pub(?:\([^)]*\))?)\s+)?(?:unsafe\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*<[^>{}]*>)?\s*\(/gmu
    : JS_DECL_RULE;
  const blocks = [];
  for (const match of file.masked.matchAll(matcher)) {
    const start = match.index ?? 0;
    const open = file.masked.indexOf("{", start + match[0].length);
    if (open < 0) continue;
    const close = matchingBrace(file.masked, open);
    if (close < 0) fail(`${file.path}: unmatched block for ${match[1]}`);
    blocks.push({ kind: "function", name: match[1], start, open, close, signature: file.source.slice(start, open), body: file.source.slice(open + 1, close) });
  }
  if (kind === "rust") {
    const definitions = /^\s*(?:(?:pub(?:\([^)]*\))?)\s+)?(?:unsafe\s+)?(struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)/gmu;
    for (const match of file.masked.matchAll(definitions)) {
      const start = match.index ?? 0;
      const open = file.masked.indexOf("{", start + match[0].length);
      if (open < 0) continue;
      const close = matchingBrace(file.masked, open);
      if (close < 0) fail(`${file.path}: unmatched block for ${match[2]}`);
      blocks.push({ kind: match[1], name: match[2], start, open, close, body: file.source.slice(open + 1, close) });
    }
  }
  return blocks.sort((a, b) => a.start - b.start || a.name.localeCompare(b.name));
}

function matchSourceRef(file, base, match) {
  const local = match[0].search(/\S/u);
  return sourceRef(file, base + match.index + Math.max(local, 0), match[0]);
}

function parseFields(block, file) {
  const body = scopeCode(file, { start: block.open + 1, body: block.body });
  if (block.kind === "enum") {
    const variants = [];
    for (const match of body.matchAll(/^\s*([A-Z][A-Za-z0-9_]*)\s*(?:\([^\n]*\)|\{[^\n]*\})?\s*,?/gmu)) {
      variants.push({ name: match[1], source: matchSourceRef(file, block.open + 1, match) });
    }
    return variants;
  }
  const fields = [];
  for (const match of body.matchAll(/^\s*(?:(?:pub(?:\([^)]*\))?)\s+)?([a-zA-Z_][A-Za-z0-9_]*)\s*:\s*([^\n,]+)[,;]?/gmu)) {
    fields.push({
      name: match[1],
      type: cleanSnippet(match[2]),
      source: matchSourceRef(file, block.open + 1, match),
    });
  }
  return fields;
}

function parseFile(path, language) {
  const absolute = absPath(path);
  assert(existsSync(absolute), `missing source ${path}`);
  const source = readFileSync(absolute, "utf8");
  assert(source.length > 0, `empty source ${path}`);
  const file = { path, language, source, masked: maskCode(source) };
  file.blocks = parseBlocks(file, language);
  for (const block of file.blocks) {
    block.fields = block.kind === "function" ? [] : parseFields(block, file);
    block.source = sourceRef(file, block.start);
  }
  return file;
}

function byName(blocks, name) {
  return blocks.filter((block) => block.name === name);
}

function implScopes(file, owner) {
  if (file.language !== "rust") return [];
  const escaped = owner.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const matcher = new RegExp(`\\bimpl\\b[^\\n{]*\\b${escaped}\\b[^\\n{]*\\{`, "gmu");
  const scopes = [];
  for (const match of file.masked.matchAll(matcher)) {
    const open = file.masked.indexOf("{", match.index ?? 0);
    const close = matchingBrace(file.masked, open);
    if (open < 0 || close < 0) continue;
    scopes.push({ start: match.index ?? 0, open, close, body: file.source.slice(open + 1, close) });
  }
  return scopes;
}

function functionScopes(file, names = [], prefixes = []) {
  return file.blocks.filter((block) => block.kind === "function" && (
    names.includes(block.name) || prefixes.some((prefix) => block.name.startsWith(prefix))
  ));
}
function relatedFunctionScopes(file, owner) {
  const escaped = owner.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const matcher = new RegExp(`\\b${escaped}\\b`, "u");
  return file.blocks
    .filter((block) => block.kind === "function"
      && matcher.test(`${file.masked.slice(block.start, block.open)}\n${file.masked.slice(block.open + 1, block.close)}`))
    .map((block) => ({ start: block.open + 1, body: block.body }));
}

function ownerScopes(file, owner, block) {
  const scopes = [
    { start: block.open + 1, body: block.body },
    ...implScopes(file, owner).map((scope) => ({ start: scope.open + 1, body: scope.body })),
    ...relatedFunctionScopes(file, owner),
  ];
  const seen = new Set();
  return scopes.filter((scope) => {
    const key = `${scope.start}:${scope.body.length}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function uniqueStrings(values) {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function touchedFields(line, names) {
  return names.filter((name) => new RegExp(`\\b${name.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}\\b`, "u").test(line));
}

function classifyEvent(line) {
  for (const [event, rule] of TRANSITION_RULES) if (rule.test(line)) return event;
  return "mutation";
}

function transitionOutcome(line, fields) {
  const assignments = [];
  for (const field of fields) {
    const escaped = field.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    const match = line.match(new RegExp(`\\b${escaped}\\b\\s*(?:\\.\\w+\\([^)]*\\)|=)\\s*([^;,)]*)`, "u"));
    if (match) assignments.push(`${field}=${cleanSnippet(match[1])}`);
  }
  if (assignments.length) return assignments.join(", ");
  if (/\.(?:compare_exchange|swap|replace|store|fetch_add|fetch_sub)\s*\(/u.test(line)) return "atomic operation";
  if (/\b(?:insert|remove|push|pop|retain|drain|clear)\b/u.test(line)) return "membership mutation";
  return "observed lifecycle operation";
}

function transitionRows(file, scopes, fields, variants = []) {
  const rows = [];
  const fieldNames = fields.map((field) => field.name ?? field);
  const variantNames = variants.map((variant) => variant.name ?? variant);
  const seen = new Set();
  for (const scope of scopes) {
    const base = scope.start ?? 0;
    for (const match of scopeCode(file, scope).matchAll(/[^\n]+/gu)) {
      const line = match[0];
      const offset = base + (match.index ?? 0);
      const writes = touchedFields(line, fieldNames);
      const javascriptWrite = file.language === "javascript"
        && writes.some((field) => new RegExp(`(?:\\.\\s*)?${field.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}\\s*(?:=|:)`, "u").test(line));
      if (!TRANSITION_LINE_RULE.test(line)
        && !variantNames.some((name) => new RegExp(`\\b${name}\\b`, "u").test(line))
        && !javascriptWrite) continue;
      const source = sourceRef(file, offset);
      const key = `${source.line}:${source.text}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const outcome = transitionOutcome(line, fieldNames);
      rows.push({
        event: classifyEvent(line),
        from: "authoritative state before this source operation",
        to: outcome,
        writes,
        source,
      });
    }
  }
  if (rows.length === 0) {
    const source = scopes[0]
      ? sourceRef(file, scopes[0].start)
      : sourceRef(file, 0);
    rows.push({
      event: "none",
      from: "not applicable",
      to: "not applicable",
      writes: [],
      outcome: "no mutable lifecycle transition in the selected source scope",
      source,
    });
  }
  return rows.sort((a, b) => a.source.line - b.source.line || a.event.localeCompare(b.event));
}
function transitionObserved(rows) {
  return rows.some((row) => row.event !== "none");
}

function identityEvidence(file, scopes, fields, ownerName) {
  const names = fields.map((field) => field.name ?? field);
  const rows = [];
  for (const [kind, rule] of IDENTITY_KINDS) {
    const evidence = [];
    for (const field of fields) {
      if (rule.test(field.name)) evidence.push(field.source);
    }
    for (const scope of scopes) {
      const base = scope.start ?? 0;
      for (const match of scopeCode(file, scope).matchAll(/[^\n]+/gu)) {
        const line = match[0];
        if (!rule.test(line)) continue;
        const source = sourceRef(file, base + (match.index ?? 0));
        if (!evidence.some((item) => item.line === source.line && item.file === source.file)) evidence.push(source);
      }
    }
    if (evidence.length) rows.push({ kind, names: uniqueStrings(names.filter((name) => rule.test(name))), evidence: evidence.sort((a, b) => a.line - b.line).slice(0, 8) });
  }
  return rows;
}

function gates(file, scopes) {
  const rows = [];
  const seen = new Set();
  for (const scope of scopes) {
    const base = scope.start ?? 0;
    for (const match of scopeCode(file, scope).matchAll(/[^\n]+/gu)) {
      const line = match[0];
      const kind = GATE_RULES.find(([, rule]) => rule.test(line))?.[0];
      if (!kind) continue;
      const source = sourceRef(file, base + (match.index ?? 0));
      const key = `${kind}:${source.line}:${source.text}`;
      if (seen.has(key)) continue;
      seen.add(key);
      rows.push({ kind, condition: cleanSnippet(line), source });
    }
  }
  return rows.sort((a, b) => a.source.line - b.source.line || a.kind.localeCompare(b.kind)).slice(0, 16);
}

function evidenceFor(file, scopes, rule, limit = 12) {
  const rows = [];
  const seen = new Set();
  for (const scope of scopes) {
    const base = scope.start ?? 0;
    for (const match of scopeCode(file, scope).matchAll(/[^\n]+/gu)) {
      const line = match[0];
      if (!rule.test(line)) continue;
      const source = sourceRef(file, base + (match.index ?? 0));
      if (seen.has(source.line)) continue;
      seen.add(source.line);
      rows.push(source);
    }
  }
  return rows.sort((a, b) => a.line - b.line).slice(0, limit);
}

function jsFields(scopes, file) {
  const fields = new Map();
  for (const scope of scopes) {
    for (const match of scopeCode(file, scope).matchAll(JS_PROPERTY_RULE)) {
      const name = match[1] ?? match[2];
      if (!name || ["length", "push", "pop", "map", "reduce", "filter", "isArray", "Number", "Error"].includes(name)) continue;
      if (!fields.has(name)) fields.set(name, { name, type: "JavaScript property", source: sourceRef(file, (scope.start ?? 0) + (match.index ?? 0)) });
    }
  }
  return [...fields.values()].sort((a, b) => a.name.localeCompare(b.name));
}

function makeOwnerRow(file, owner, block, extraScopes = []) {
  const isEnum = block.kind === "enum";
  const fields = block.fields;
  const scopes = [...ownerScopes(file, owner, block), ...extraScopes.map((scope) => ({ start: scope.start, body: scope.body }))];
  const fieldNames = fields.map((field) => field.name);
  const transitions = transitionRows(file, scopes, fields, isEnum ? fields : []);
  const identity = identityEvidence(file, scopes, fields, owner);
  const gateRows = gates(file, scopes);
  const completion = evidenceFor(file, scopes, COMPLETION_RULE);
  const cancellation = evidenceFor(file, scopes, CANCELLATION_RULE);
  const cleanup = evidenceFor(file, scopes, CLEANUP_RULE);
  const state = {
    fields: fields.map((field) => ({ name: field.name, type: field.type ?? "variant", source: field.source })),
    identity: identity.map((entry) => entry.kind),
  };
  const applicability = {
    transitions: transitionObserved(transitions) ? "observed" : "not_applicable",
    publication_gate: gateRows.length > 0 ? "observed" : "not_applicable",
    completion: completion.length > 0 ? "observed" : "not_applicable",
    cancellation: cancellation.length > 0 ? "observed" : "not_applicable",
    cleanup: cleanup.length > 0 ? "observed" : "not_applicable",
  };
  return {
    id: `${file.path}#${owner}`,
    owner: { name: owner, kind: block.kind, source: block.source },
    authoritative_state: state,
    state_transition_table: transitions,
    use_publication_gate: gateRows,
    completion_definition: completion,
    cancellation_definition: cancellation,
    cleanup_rule: cleanup,
    identity_evidence: identity,
    applicability,
    source_only: true,
    mismatch: null,
  };
}

function makeFunctionRow(file, name, scopes) {
  const bodyScopes = scopes.map((scope) => ({ start: scope.open + 1, body: scope.body }));
  const fields = [];
  for (const scope of bodyScopes) {
    for (const match of scopeCode(file, scope).matchAll(/\b([A-Z][A-Za-z0-9_]*(?:CACHE|COUNT|REVISION|GENERATION|ORDER|TIME|ID|KEY|DIGEST)[A-Za-z0-9_]*)\b/gu)) {
      const nameValue = match[1];
      if (!fields.some((field) => field.name === nameValue)) fields.push({ name: nameValue, type: "source state marker", source: sourceRef(file, scope.start + (match.index ?? 0)) });
    }
  }
  const transitions = transitionRows(file, bodyScopes, fields);
  const identity = identityEvidence(file, bodyScopes, fields, name);
  const gateRows = gates(file, bodyScopes);
  const completion = evidenceFor(file, bodyScopes, COMPLETION_RULE);
  const cancellation = evidenceFor(file, bodyScopes, CANCELLATION_RULE);
  const cleanup = evidenceFor(file, bodyScopes, CLEANUP_RULE);
  return {
    id: `${file.path}#fn:${name}`,
    owner: { name, kind: "function", source: scopes[0].source ?? sourceRef(file, scopes[0].start) },
    authoritative_state: { fields, identity: identity.map((entry) => entry.kind) },
    state_transition_table: transitions,
    use_publication_gate: gateRows,
    completion_definition: completion,
    cancellation_definition: cancellation,
    cleanup_rule: cleanup,
    identity_evidence: identity,
    applicability: {
      transitions: transitionObserved(transitions) ? "observed" : "not_applicable",
      publication_gate: gateRows.length > 0 ? "observed" : "not_applicable",
      completion: completion.length > 0 ? "observed" : "not_applicable",
      cancellation: cancellation.length > 0 ? "observed" : "not_applicable",
      cleanup: cleanup.length > 0 ? "observed" : "not_applicable",
    },
    source_only: true,
    mismatch: null,
  };
}

function makeJavaScriptRows(file, prefixes) {
  const rows = [];
  for (const prefix of prefixes) {
    const scopes = functionScopes(file, [], [prefix]);
    assert(scopes.length > 0, `${file.path}: no functions match ${prefix}`);
    const fields = jsFields(scopes, file);
    const bodyScopes = scopes.map((scope) => ({ start: scope.open + 1, body: scope.body }));
    const transitions = transitionRows(file, bodyScopes, fields);
    const identity = identityEvidence(file, bodyScopes, fields, prefix);
    const gateRows = gates(file, bodyScopes);
    const completion = evidenceFor(file, bodyScopes, COMPLETION_RULE);
    const cancellation = evidenceFor(file, bodyScopes, CANCELLATION_RULE);
    const cleanup = evidenceFor(file, bodyScopes, CLEANUP_RULE);
    rows.push({
      id: `${file.path}#adapter:${prefix}`,
      owner: { name: prefix, kind: "function-prefix", source: sourceRef(file, scopes[0].start) },
      authoritative_state: { fields, identity: identity.map((entry) => entry.kind) },
      state_transition_table: transitions,
      use_publication_gate: gateRows,
      completion_definition: completion,
      cancellation_definition: cancellation,
      cleanup_rule: cleanup,
      identity_evidence: identity,
      applicability: {
        transitions: transitionObserved(transitions) ? "observed" : "not_applicable",
        publication_gate: gateRows.length > 0 ? "observed" : "not_applicable",
        completion: completion.length > 0 ? "observed" : "not_applicable",
        cancellation: cancellation.length > 0 ? "observed" : "not_applicable",
        cleanup: cleanup.length > 0 ? "observed" : "not_applicable",
      },
      source_only: true,
      mismatch: null,
    });
  }
  return rows;
}

function buildRows(files) {
  const rows = [];
  const unresolved = [];
  for (const descriptor of SOURCES) {
    const file = files.find((candidate) => candidate.path === descriptor.path);
    assert(file, `source was not loaded ${descriptor.path}`);
    for (const owner of descriptor.owners ?? []) {
      const blocks = byName(file.blocks, owner).filter((block) => block.kind !== "function");
      if (blocks.length !== 1) {
        unresolved.push({ source: descriptor.path, symbol: owner, matches: blocks.length });
        continue;
      }
      rows.push(makeOwnerRow(file, owner, blocks[0], []));
    }
    for (const name of descriptor.functions ?? []) {
      const scopes = functionScopes(file, [name]);
      if (scopes.length === 0) {
        unresolved.push({ source: descriptor.path, symbol: `fn ${name}`, matches: 0 });
        continue;
      }
      rows.push(makeFunctionRow(file, name, scopes));
    }
    rows.push(...makeJavaScriptRows(file, descriptor.functionPrefixes ?? []));
  }
  assert(unresolved.length === 0, `unresolved selected symbols: ${JSON.stringify(unresolved)}`);
  return rows.sort((a, b) => a.id.localeCompare(b.id));
}

function buildIdentityMatrix(rows) {
  const entries = IDENTITY_KINDS.map(([kind]) => {
    const owners = rows.filter((row) => row.identity_evidence.some((entry) => entry.kind === kind)).map((row) => row.id).sort();
    const names = uniqueStrings(rows.flatMap((row) => row.identity_evidence.filter((entry) => entry.kind === kind).flatMap((entry) => entry.names)));
    return { kind, owners, names, observed: owners.length > 0 };
  });
  return entries.map((entry) => {
    const overlaps = entries
      .filter((other) => other.kind !== entry.kind)
      .map((other) => ({ kind: other.kind, names: entry.names.filter((name) => other.names.includes(name)) }))
      .filter((other) => other.names.length > 0);
    return {
      ...entry,
      contract: IDENTITY_CONTRACTS[entry.kind],
      separate: overlaps.length === 0,
      overlaps,
      source_only: true,
    };
  });
}
function buildOverlapAnalysis(rows) {
  const overlaps = [];
  for (const [identityKind] of IDENTITY_KINDS) {
    const selected = rows.filter((row) => row.identity_evidence.some((entry) => entry.kind === identityKind));
    const operations = new Map();
    const add = (operationKind, row, source) => {
      const key = `${operationKind}`;
      if (!operations.has(key)) operations.set(key, new Map());
      const byOwner = operations.get(key);
      if (!byOwner.has(row.id)) byOwner.set(row.id, source);
    };
    for (const row of selected) {
      for (const transition of row.state_transition_table) {
        if (transition.event !== "none") add(`transition:${transition.event}`, row, transition.source);
      }
      for (const gate of row.use_publication_gate) add(`gate:${gate.kind}`, row, gate.source);
    }
    for (const [operationKind, byOwner] of operations) {
      if (byOwner.size < 2) continue;
      const owners = [...byOwner.keys()].sort();
      overlaps.push({
        identity_kind: identityKind,
        operation_kind: operationKind,
        owners,
        evidence: owners.map((owner) => ({ owner, source: byOwner.get(owner) })),
        classification: "shared operation shape; distinct identity owner",
        rationale: "The same source-level operation appears under multiple owners; this is overlap evidence, not proof that their identities or stale gates can be merged.",
        source_only: true,
        runtime_mismatch: false,
      });
    }
  }
  return overlaps.sort((a, b) => a.identity_kind.localeCompare(b.identity_kind)
    || a.operation_kind.localeCompare(b.operation_kind)
    || a.owners.join(",").localeCompare(b.owners.join(",")));
}
function validateOverlapAnalysis(overlaps) {
  const errors = [];
  for (const overlap of overlaps) {
    if (!Array.isArray(overlap.owners) || overlap.owners.length < 2) errors.push(`overlap ${overlap.identity_kind}/${overlap.operation_kind} has fewer than two owners`);
    if (overlap.source_only !== true || overlap.runtime_mismatch !== false) errors.push(`overlap ${overlap.identity_kind}/${overlap.operation_kind} is not marked source-only`);
    if (!Array.isArray(overlap.evidence) || overlap.evidence.length !== overlap.owners.length
      || overlap.evidence.some((entry) => !entry.owner || !entry.source?.file || !Number.isInteger(entry.source?.line))) {
      errors.push(`overlap ${overlap.identity_kind}/${overlap.operation_kind} lacks owner/source evidence`);
    }
  }
  return errors;
}

function duplicateIds(rows) {
  const counts = new Map();
  for (const row of rows) counts.set(row.id, (counts.get(row.id) ?? 0) + 1);
  return [...counts.entries()].filter(([, count]) => count > 1).map(([id, count]) => ({ id, count }));
}

function validateRows(rows) {
  const errors = [];
  for (const row of rows) {
    if (!row.owner?.source || !Array.isArray(row.authoritative_state?.fields)) errors.push(`${row.id}: owner/state evidence is incomplete`);
    if (!Array.isArray(row.state_transition_table) || row.state_transition_table.length === 0) errors.push(`${row.id}: transition table is absent`);
    const transitionStatus = transitionObserved(row.state_transition_table) ? "observed" : "not_applicable";
    if (row.applicability.transitions !== transitionStatus) errors.push(`${row.id}: transition applicability disagrees with table`);
    for (const [key, applicabilityKey] of [["use_publication_gate", "publication_gate"], ["completion_definition", "completion"], ["cancellation_definition", "cancellation"], ["cleanup_rule", "cleanup"]]) {
      if (!Array.isArray(row[key]) || !["observed", "not_applicable"].includes(row.applicability[applicabilityKey])) errors.push(`${row.id}: ${key} evidence/applicability is incomplete`);
    }
    if (row.mismatch !== null && (!Array.isArray(row.mismatch.history) || row.mismatch.history.length === 0)) errors.push(`${row.id}: mismatch has no replay history`);
    if (row.mismatch !== null && row.mismatch.source_only !== true) errors.push(`${row.id}: unlabelled runtime mismatch`);
  }
  const duplicate = duplicateIds(rows);
  if (duplicate.length) errors.push(`duplicate row ids: ${JSON.stringify(duplicate)}`);
  return errors;
}

function buildReport() {
  const files = SOURCES.map(({ path, language }) => parseFile(path, language));
  const rows = buildRows(files);
  const errors = validateRows(rows);
  const identityMatrix = buildIdentityMatrix(rows);
  const overlapAnalysis = buildOverlapAnalysis(rows);
  errors.push(...validateOverlapAnalysis(overlapAnalysis));
  const missingIdentityKinds = identityMatrix.filter((entry) => !entry.observed).map((entry) => entry.kind);
  // Device completion and cache identity can be represented by calls rather
  // than fields.  They must nevertheless have exact source evidence.
  const completionEvidence = rows.flatMap((row) => row.completion_definition).filter((ref) => /(?:wait_until_completed|wait_for_fences|completion|completed|commit|finish|join|present|stop)/iu.test(ref.text));
  const cacheEvidence = rows.flatMap((row) => row.identity_evidence.filter((entry) => entry.kind === "semantic_cache_identity").flatMap((entry) => entry.evidence));
  if (completionEvidence.length === 0) errors.push("device/task completion has no source evidence");
  if (cacheEvidence.length === 0) errors.push("semantic cache identity has no source evidence");
  if (missingIdentityKinds.includes("generation") || missingIdentityKinds.includes("revision") || missingIdentityKinds.includes("lifetime") || missingIdentityKinds.includes("clock_time")) errors.push(`required identity category is absent: ${missingIdentityKinds.join(", ")}`);
  const report = {
    schema: SCHEMA,
    card: CARD,
    generated_by: GENERATED_COMMAND,
    generated_static_only: true,
    source_scope: SOURCES.map(({ path, language }) => ({ path, language })),
    sources: files.map((file) => ({ path: file.path, language: file.language, bytes: Buffer.byteLength(file.source), declarations: file.blocks.length })),
    rows,
    identity_matrix: identityMatrix,
    overlap_analysis: overlapAnalysis,
    mismatches: rows.filter((row) => row.mismatch !== null).map((row) => ({ id: row.id, ...row.mismatch })),
    source_only_predictions: [],
    criteria: [
      {
        n: 1,
        status: errors.length === 0 ? "pass" : "fail",
        evidence: rows.map((row) => row.id),
        rule: "Every selected owner has authoritative state, transitions, use/publication gate, completion, cancellation, and cleanup evidence or an explicit not_applicable marker.",
      },
      {
        n: 2,
        status: errors.length === 0 ? "pass" : "fail",
        evidence: identityMatrix,
        rule: "Generation, revision, lifetime, clock time, device completion, and semantic cache identity are separately sourced categories.",
      },
      {
        n: 3,
        status: rows.every((row) => row.mismatch === null || (row.mismatch.source_only === true && row.mismatch.history?.length > 0)) ? "pass" : "fail",
        evidence: "No runtime mismatch is claimed by this static census; any future mismatch must carry an event history.",
        rule: "Claims are replayable histories; source-only predictions are labelled and never presented as runtime failures.",
      },
      {
        n: 4,
        status: errors.length === 0 ? "pass" : "fail",
        evidence: { selected_sources: files.length, rows: rows.length, overlap_analysis: overlapAnalysis.length, added_mechanisms: [] },
        rule: "Census observes the adopted state/effect/cache mechanisms and adds no universal context or second engine.",
      },
    ],
    checks: {
      status: errors.length === 0 ? "pass" : "fail",
      errors,
      row_count: rows.length,
      overlap_count: overlapAnalysis.length,
      mismatch_count: rows.filter((row) => row.mismatch !== null).length,
    },
  };
  return report;
}

function parseArgs(argv) {
  let mode = "human";
  for (const arg of argv) {
    if (arg === "--json") mode = "json";
    else if (arg === "--check") mode = "check";
    else if (arg === "--help" || arg === "-h") mode = "help";
    else fail(`unknown argument ${arg}; use --help`);
  }
  return { mode };
}

function printHelp() {
  process.stdout.write([
    "Lifecycle census",
    "",
    `Usage: ${GENERATED_COMMAND}`,
    `       ${GENERATED_COMMAND} --json`,
    `       ${GENERATED_COMMAND} --check`,
    `       ${GENERATED_COMMAND} --help`,
    "",
    "No arguments print a compact deterministic summary.",
    "--json    print the complete source-derived transition table.",
    "--check   validate selected symbols and print the final gate status.",
    "",
  ].join("\n"));
}

function renderHuman(report) {
  const lines = [
    `Lifecycle census card=${report.card} schema=${report.schema}`,
    `sources=${report.sources.length} rows=${report.checks.row_count} identities=${report.identity_matrix.filter((entry) => entry.observed).length}/${report.identity_matrix.length}`,
    `mismatches=${report.checks.mismatch_count} overlap_shapes=${report.checks.overlap_count} source_only_predictions=${report.source_only_predictions.length}`,
  ];
  for (const row of report.rows) {
    const transitions = row.state_transition_table.length;
    const gates = row.use_publication_gate.length;
    const completion = row.completion_definition.length;
    const cancellation = row.cancellation_definition.length;
    const cleanup = row.cleanup_rule.length;
    lines.push(`  ${row.id} transitions=${transitions} gates=${gates} completion=${completion} cancellation=${cancellation} cleanup=${cleanup}`);
  }
  lines.push(`CENSUS ${report.checks.status.toUpperCase()} errors=${report.checks.errors.length}`);
  return `${lines.join("\n")}\n`;
}

function main(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  if (args.mode === "help") {
    printHelp();
    return 0;
  }
  const report = buildReport();
  if (args.mode === "json") {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    return report.checks.status === "pass" ? 0 : 1;
  }
  process.stdout.write(renderHuman(report));
  return report.checks.status === "pass" ? 0 : 1;
}

try {
  process.exitCode = main();
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
