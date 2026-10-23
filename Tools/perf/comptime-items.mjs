#!/usr/bin/env node
// Where compile-time work goes: reads a compiler trace (JET_TRACE_FILE) and
// prints the comptime phase's spans per kind, per source file and per item.
//
// The driver (JetDriver Pipeline.jet) opens one span per comptime item step,
// its `args` naming the item's source `file` and `item`:
//   comptime.check    the item's own check
//   comptime.closure  checking the functions it reaches
//   comptime.stage    staging the declaration rows (once per run)
//   comptime.build    building a batch's IR (`item` lists the batch's items;
//                     its time is split evenly among them)
//   comptime.eval     running the item
// and per-pass spans without an item: comptime.precheck (the pass's
// threaded item checks) and comptime.apply (landing values in the graph).
// Wall and CPU are summed per row; CPU is the span thread's `cpu_ns`.
//
//   node Tools/perf/comptime-items.mjs TRACE [--top N] [--json]

import { readFileSync } from "node:fs";

const STEPS = ["check", "closure", "stage", "build", "eval"];

function usage(message) {
  if (message) console.error(`comptime-items: ${message}`);
  console.error("usage: node Tools/perf/comptime-items.mjs TRACE [--top N] [--json]");
  process.exit(2);
}

function parseArgs(argv) {
  const options = { trace: null, top: 30, json: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") options.json = true;
    else if (arg === "--top") {
      options.top = Number(argv[index + 1]);
      index += 1;
      if (!Number.isInteger(options.top) || options.top < 0) usage("--top takes a count");
    } else if (arg === "--help" || arg === "-h") usage();
    else if (options.trace === null) options.trace = arg;
    else usage(`unexpected argument ${arg}`);
  }
  if (options.trace === null) usage("no trace file");
  return options;
}

// The trace is a JSON array the compiler streams, possibly unterminated.
function readEvents(path) {
  const text = readFileSync(path, "utf8").trim().replace(/,\s*$/, "");
  return JSON.parse(text.endsWith("]") ? text : `${text}]`);
}

// Closed spans: kind (`cat`), label, args of the begin, wall µs and CPU ns.
function closedSpans(events) {
  const open = new Map();
  const spans = [];
  for (const event of events) {
    if (event.ph !== "B" && event.ph !== "E") continue;
    const stack = open.get(event.tid) ?? [];
    open.set(event.tid, stack);
    if (event.ph === "B") {
      stack.push(event);
      continue;
    }
    const kind = event.cat ?? event.name;
    let at = stack.length - 1;
    while (at >= 0 && (stack[at].cat ?? stack[at].name) !== kind) at -= 1;
    if (at < 0) continue;
    const begin = stack[at];
    stack.length = at;
    spans.push({
      kind,
      label: begin.name,
      args: begin.args ?? {},
      start: begin.ts,
      wall: event.ts - begin.ts,
      cpu: event.args?.cpu_ns ?? 0,
    });
  }
  return spans;
}

function row(key) {
  const out = { key, count: 0, wall: 0, cpu: 0 };
  for (const step of STEPS) out[step] = 0;
  return out;
}

function add(table, key, step, wall, cpu, count) {
  const entry = table.get(key) ?? row(key);
  table.set(key, entry);
  entry.count += count;
  entry.wall += wall;
  entry.cpu += cpu;
  entry[step] += wall;
}

function summarize(spans) {
  const phase = spans.find((span) => span.kind === "comptime" && span.label === "comptime");
  const kinds = new Map();
  const files = new Map();
  const items = new Map();
  for (const span of spans) {
    if (!span.kind.startsWith("comptime.")) continue;
    const kind = kinds.get(span.kind) ?? { kind: span.kind, count: 0, wall: 0, cpu: 0 };
    kinds.set(span.kind, kind);
    kind.count += 1;
    kind.wall += span.wall;
    kind.cpu += span.cpu;
    const step = span.kind.slice("comptime.".length);
    if (!STEPS.includes(step) || typeof span.args.item !== "string") continue;
    const file = span.args.file ?? "";
    const names = step === "build" ? span.args.item.split(", ") : [span.args.item];
    const share = names.length;
    for (const name of names) {
      const counted = step === "check" ? 1 : 0;
      add(files, file, step, span.wall / share, span.cpu / share, counted);
      add(items, `${file} ${name}`, step, span.wall / share, span.cpu / share, counted);
    }
  }
  const byWall = (a, b) => b.wall - a.wall;
  return {
    phase: phase ? { wall: phase.wall, cpu: phase.cpu } : null,
    kinds: [...kinds.values()].sort(byWall),
    files: [...files.values()].sort(byWall),
    items: [...items.values()].sort(byWall),
  };
}

const seconds = (us) => (us / 1e6).toFixed(2).padStart(7);
const cpuSeconds = (ns) => (ns / 1e9).toFixed(2).padStart(7);

function printTable(title, rows, top) {
  console.log(`\n${title}`);
  console.log(`${"count".padStart(6)} ${"wall s".padStart(7)} ${"cpu s".padStart(7)} ${STEPS.map((step) => step.padStart(7)).join(" ")}  name`);
  for (const entry of rows.slice(0, top)) {
    console.log(`${String(entry.count).padStart(6)} ${seconds(entry.wall)} ${cpuSeconds(entry.cpu)} ${STEPS.map((step) => seconds(entry[step])).join(" ")}  ${entry.key}`);
  }
  if (rows.length > top) console.log(`  ... ${rows.length - top} more`);
}

const options = parseArgs(process.argv.slice(2));
const summary = summarize(closedSpans(readEvents(options.trace)));
if (options.json) {
  console.log(JSON.stringify(summary, null, 2));
} else {
  if (summary.phase) console.log(`comptime phase ${seconds(summary.phase.wall).trim()} s wall, ${cpuSeconds(summary.phase.cpu).trim()} s cpu`);
  console.log(`\n${"count".padStart(6)} ${"wall s".padStart(7)} ${"cpu s".padStart(7)}  span`);
  for (const kind of summary.kinds) console.log(`${String(kind.count).padStart(6)} ${seconds(kind.wall)} ${cpuSeconds(kind.cpu)}  ${kind.kind}`);
  printTable("per file (count = items checked)", summary.files, options.top);
  printTable("per item", summary.items, options.top);
}
