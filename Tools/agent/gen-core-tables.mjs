#!/usr/bin/env node
// D-OPENTABLE1=D: generate the checked-in views of the declarations that have
// a readable Jet twin. The declarations stay in Compiler/Prelude and
// Compiler/JetFoundation; --check rejects a stale view.
//
// Usage:
//   node Tools/agent/gen-core-tables.mjs --check
//   node Tools/agent/gen-core-tables.mjs --write
//
// Core module and dispatcher rendering is shared with
// check-core-surface-ledger.mjs. This file checks the effect declarations and
// owns the collection-operation reference projection; it must not grow a
// second Core parser or renderer.

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  coreSourceFacts,
  publicDispatcherRows,
  validatePublicDispatcher,
  validateGeneratedViews,
  writeCoreViews,
} from "./check-core-surface-ledger.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const EFFECT_SOURCE_PATH = "Compiler/Prelude/Effects.jet";
const CORE_SOURCE_PATH = "Compiler/Prelude/Core.jet";
// D-ONE-OPERATIONS1=A: the collection operation rows are authored once in
// Jet; the Core reference tables are their projection.
const OPERATIONS_SOURCE_PATH = "Compiler/JetFoundation/Source/Collections.jet";
const OPERATIONS_DOC_PATH = "Docs/spec/reference/core-library.md";
const OPERATIONS_BEGIN = "<!-- BEGIN GENERATED COLLECTION OPERATIONS -->";
const OPERATIONS_END = "<!-- END GENERATED COLLECTION OPERATIONS -->";

// Build authority intentionally remains the existing ten-effect projection.
// FFI, Browser, and Secret are runtime authority roots, not build grants.
const BUILD_EFFECT_NAMES = [
  "Net", "FS", "IO", "DB", "Time", "Rand", "Env", "Exec", "Log", "GPU",
];
// The deny-only roots: declared with the rest of the vocabulary, refused by a
// policy but never granted.
const DENY_ONLY_ROOT_NAMES = ["Mem", "Panic"];

function file(rel) {
  return path.join(ROOT, rel);
}

function read(rel) {
  return readFileSync(file(rel), "utf8");
}

function fail(message) {
  throw new Error(message);
}

function parseEffects(source) {
  const declarations = [];
  const names = new Set();
  for (const [index, raw] of source.split(/\r?\n/).entries()) {
    const line = raw.replace(/\/\/.*$/, "").trim();
    if (!line) continue;
    const fields = line.split(/\s+/);
    if (fields[0] !== "effect") {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: expected an effect declaration`);
    }
    if (fields.length < 2 || fields.length > 3) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: malformed effect declaration`);
    }
    const name = fields[1];
    if (!/^[A-Z][A-Za-z0-9]*(?:\.[A-Z][A-Za-z0-9]*)*$/.test(name)) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: invalid effect name ${name}`);
    }
    if (names.has(name)) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: duplicate effect declaration ${name}`);
    }
    const irreversible = fields.length === 3;
    if (irreversible && fields[2] !== "$irreversible") {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: unknown effect annotation ${fields[2]}`);
    }
    if (name.includes(".") && !names.has(name.split(".", 1)[0])) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: effect leaf ${name} has no declared root`);
    }
    names.add(name);
    declarations.push({ name, irreversible });
  }
  if (!declarations.length) fail(`${EFFECT_SOURCE_PATH}: no effect declarations`);

  const roots = declarations
    .filter((declaration) => !declaration.name.includes("."))
    .map((declaration) => declaration.name);
  const rootSet = new Set(roots);
  for (const denyOnly of DENY_ONLY_ROOT_NAMES) {
    if (!rootSet.has(denyOnly)) {
      fail(`${EFFECT_SOURCE_PATH}: deny-only root ${denyOnly} is not declared`);
    }
  }
  for (const build of BUILD_EFFECT_NAMES) {
    if (!rootSet.has(build)) {
      fail(`${EFFECT_SOURCE_PATH}: build effect ${build} has no declared root`);
    }
  }
  const irreversible = declarations
    .filter((declaration) => declaration.irreversible)
    .map((declaration) => declaration.name);
  const expectedIrreversible = new Set(["FS.Write", "Net", "Exec", "FFI"]);
  if (irreversible.length !== expectedIrreversible.size ||
      irreversible.some((name) => !expectedIrreversible.has(name))) {
    fail(`${EFFECT_SOURCE_PATH}: irreversible declarations must be FS.Write, Net, Exec, and FFI`);
  }
  return { declarations, roots };
}

function replaceSection(source, begin, end, replacement, rel) {
  const starts = source.split(begin).length - 1;
  const ends = source.split(end).length - 1;
  if (starts !== 1 || ends !== 1) {
    fail(`${rel}: expected exactly one generated section (${begin}, ${end})`);
  }
  const start = source.indexOf(begin);
  const finish = source.indexOf(end, start);
  if (finish < start) fail(`${rel}: generated section markers are out of order`);
  return source.slice(0, start) + replacement + source.slice(finish + end.length);
}

function readSection(source, begin, end, rel) {
  const start = source.indexOf(begin);
  const finish = source.indexOf(end, start < 0 ? 0 : start);
  if (start < 0 || finish < 0 || finish < start) {
    fail(`${rel}: generated section markers are missing or out of order`);
  }
  return source.slice(start, finish + end.length);
}

function ensureCoreNames() {
  const core = coreSourceFacts();
  if (!core.modules.some((entry) => entry.module === "core.tasks")) {
    fail(`${CORE_SOURCE_PATH}: missing canonical core.tasks module`);
  }
  if (core.modules.some((entry) => entry.module === "core.task")) {
    fail(`${CORE_SOURCE_PATH}: retired core.task alias is still declared`);
  }
  return core;
}

// Collection operation rows: one `CollectionOperationRow{...}` or
// `CollectionLawFacts{...}` literal per line inside the marked Jet tables. The
// Jet compiler type-checks the rows; this projection only checks table-level
// laws (unique receiver/method keys and known laws) and renders the reference
// tables.
const OPERATION_ROW = /^CollectionOperationRow\{receivers: CollectionReceivers\.(\w+), name: "(\w+)", minimum_arity: (\d+), maximum_arity: (\d+), access: CollectionAccess\.(\w+), args: \[CollectionArg\]\{([^}]*)\}, result: CollectionResult\.(\w+), op: (.+), law: CollectionLaw\.(\w+), empty: "([^"]*)", status: CollectionStatus\.(\w+)\},?$/;
const LAW_ROW = /^CollectionLawFacts\{law: CollectionLaw\.(\w+), timing: "([^"]*)", result: "([^"]*)", callbacks: "([^"]*)", stop: "([^"]*)", failure: "([^"]*)", buffering: "([^"]*)"\},?$/;
const RECEIVER_TEXT = {
  List: "`[T]`", Iter: "`Iter<T>`", View: "`View<T>`", Sequence: "`[T]`, `Iter<T>`",
  Map: "`[K:V]`", Set: "`Set<T>`", Rank: "`Rank<T>`", Bits: "`Bits`", Tally: "`Tally<T>`",
  Cache: "`Cache<K,V>`", Queue: "`Queue<T>`", PriorityQueue: "`PriorityQueue<T>`",
};
const ACCESS_TEXT = {
  Read: "borrows", Write: "exclusive, in place", Move: "consumes",
  Sequence: "borrows a list; consumes an iterator",
};
const ARG_TEXT = {
  Element: "item", Key: "key", Value: "value", Integer: "Int", Text: "String",
  IntegerList: "[Int]", DisjointFn: "(ViewMut<item>, ViewMut<item>) -> T",
  RemoveBy: "RemoveBy", ElementList: "[item]", KeyList: "[key]", SameKind: "same kind",
  Entries: "map", Seed: "seed", ItemFn: "(item) -> T", ItemPredicate: "(item) -> Bool",
  PairPredicate: "(item, item) -> Bool", Accumulator: "(acc, item) -> acc",
  Comparator: "(item) -> key or (a, b) -> Ordering", SortKey: "(item) -> key",
  EntryFn: "(key, value) -> T", EntryPredicate: "(key, value) -> Bool",
  EntryAccumulator: "(acc, key, value) -> acc", EntryFlatMap: "(key, value) -> map",
  EntryMerge: "(key, old, new) -> value",
};
const STATUS_TEXT = { Current: "current", SourceOnly: "source-only", Proposed: "proposed" };

function markedLines(source, begin, end) {
  const text = readSection(source, begin, end, OPERATIONS_SOURCE_PATH);
  return text.split(/\r?\n/).slice(1, -1).map((line) => line.trim()).filter((line) => line.startsWith("Collection"));
}

function parseOperationContract(source) {
  const laws = new Map();
  for (const line of markedLines(source, "// BEGIN COLLECTION LAWS", "// END COLLECTION LAWS")) {
    const match = LAW_ROW.exec(line) ?? fail(`${OPERATIONS_SOURCE_PATH}: unreadable law row: ${line}`);
    const [, law, timing, result, callbacks, stop, failure, buffering] = match;
    if (laws.has(law)) fail(`${OPERATIONS_SOURCE_PATH}: duplicate law ${law}`);
    laws.set(law, { law, timing, result, callbacks, stop, failure, buffering });
  }
  const operations = [];
  const keys = new Set();
  for (const line of markedLines(source, "// BEGIN COLLECTION OPERATIONS", "// END COLLECTION OPERATIONS")) {
    const match = OPERATION_ROW.exec(line) ?? fail(`${OPERATIONS_SOURCE_PATH}: unreadable operation row: ${line}`);
    const [, receivers, name, minimum, maximum, access, args, , , law, empty, status] = match;
    if (!(status in STATUS_TEXT)) fail(`${OPERATIONS_SOURCE_PATH}: ${receivers}.${name} has unknown status ${status}`);
    if (!laws.has(law)) fail(`${OPERATIONS_SOURCE_PATH}: ${receivers}.${name} names unknown law ${law}`);
    const families = receivers === "Sequence" ? ["List", "Iter"] : [receivers];
    for (const family of families) {
      const key = `${family}.${name}`;
      if (keys.has(key)) fail(`${OPERATIONS_SOURCE_PATH}: ${key} has more than one operation row`);
      keys.add(key);
    }
    const argNames = args.split(",").map((arg) => arg.trim()).filter(Boolean)
      .map((arg) => arg.replace(/^CollectionArg\./, ""));
    for (const arg of argNames) if (!(arg in ARG_TEXT)) fail(`${OPERATIONS_SOURCE_PATH}: unknown argument shape ${arg}`);
    operations.push({ receivers, name, minimum: Number(minimum), maximum: Number(maximum), access, args: argNames, law, empty, status });
  }
  for (const law of laws.keys()) {
    if (!operations.some((row) => row.law === law)) fail(`${OPERATIONS_SOURCE_PATH}: law ${law} has no operation row`);
  }
  return { laws, operations };
}

function generatedOperationContract(source) {
  const { laws, operations } = parseOperationContract(source);
  const lines = [
    OPERATIONS_BEGIN,
    `<!-- Source: ${OPERATIONS_SOURCE_PATH}; regenerate with node Tools/agent/gen-core-tables.mjs --write -->`,
    "",
    "| Law | Timing | Result | Callbacks | Stop item | Failure channel | Buffering |",
    "| --- | --- | --- | --- | --- | --- | --- |",
  ];
  for (const facts of laws.values()) {
    lines.push(`| ${facts.law} | ${facts.timing} | ${facts.result} | ${facts.callbacks} | ${facts.stop} | ${facts.failure} | ${facts.buffering} |`);
  }
  lines.push("", "| Receiver | Operation | Receiver ownership | Law | Empty result | Status |", "| --- | --- | --- | --- | --- | --- |");
  for (const row of operations) {
    if (row.name.startsWith("__")) continue; // Reserved embedded-Core seams are not public operations.
    const shown = row.args.map((arg, index) => (index < row.minimum ? ARG_TEXT[arg] : `[${ARG_TEXT[arg]}]`));
    lines.push(`| ${RECEIVER_TEXT[row.receivers]} | \`${row.name}(${shown.join(", ")})\` | ${ACCESS_TEXT[row.access]} | ${row.law} | ${row.empty} | ${STATUS_TEXT[row.status]} |`);
  }
  lines.push("", OPERATIONS_END);
  return lines.join("\n");
}

function checkOperationContract() {
  const expected = generatedOperationContract(read(OPERATIONS_SOURCE_PATH));
  const actual = readSection(read(OPERATIONS_DOC_PATH), OPERATIONS_BEGIN, OPERATIONS_END, OPERATIONS_DOC_PATH);
  if (actual !== expected) {
    fail(`${OPERATIONS_DOC_PATH} is stale; run --write to regenerate from ${OPERATIONS_SOURCE_PATH}`);
  }
}

function writeOperationContract() {
  const doc = replaceSection(
    read(OPERATIONS_DOC_PATH),
    OPERATIONS_BEGIN,
    OPERATIONS_END,
    generatedOperationContract(read(OPERATIONS_SOURCE_PATH)),
    OPERATIONS_DOC_PATH,
  );
  writeFileSync(file(OPERATIONS_DOC_PATH), doc);
}

function check() {
  parseEffects(read(EFFECT_SOURCE_PATH));
  const core = ensureCoreNames();
  validatePublicDispatcher(core, publicDispatcherRows(core));
  checkOperationContract();
  validateGeneratedViews(read(CORE_SOURCE_PATH), core);
  process.stdout.write("open tables: generated views are current\n");
}

function write() {
  parseEffects(read(EFFECT_SOURCE_PATH));
  const core = ensureCoreNames();
  writeOperationContract();
  writeCoreViews();
  validatePublicDispatcher(core, publicDispatcherRows(core));
  process.stdout.write("wrote Core generated views\n");
}

function usage() {
  process.stdout.write("usage: gen-core-tables.mjs --check|--write\n");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = process.argv.slice(2);
  try {
    if (args.includes("--check")) check();
    else if (args.includes("--write")) write();
    else if (args.includes("--help")) usage();
    else fail("usage: gen-core-tables.mjs --check|--write");
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
