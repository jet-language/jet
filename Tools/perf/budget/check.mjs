#!/usr/bin/env node
// SPEED-500K budget checker: reads a compiler trace (JET_TRACE_FILE) and the
// per-phase budget table next to this file (budget.json, the ns/line columns
// of SPEED-500K.md §1) and prints, per phase, its time, its ns per source
// line, the budget of the chosen milestone column and the ratio. Exits 1 when
// any phase is over its budget.
//
//   node Tools/perf/budget/check.mjs <trace> [--column 225K|500K|1M]
//        [--clock auto|proc_cpu|cpu|wall] [--lines N] [--budget FILE] [--strict]
//
// --column   the milestone being claimed (default 225K).
// --clock    proc_cpu: the process CPU time of each span (`proc_cpu_ns`, every
//            thread; the budget's single-thread-equivalent work). cpu: the
//            span thread's CPU time (`cpu_ns`). wall: the span's wall time.
//            auto (default): proc_cpu when the trace carries it, else wall
//            (traces written before the S0 change carry wall time only).
// --lines    the per-line denominator; default the trace's `parse.user_lines`
//            counter, else budget.json `default_lines` (the self-compile unit).
// --strict   a budget row whose spans are absent from the trace fails too.
//
// A row sums its span kinds (a span nested in another span of the same row
// counts once) less the nested spans of its `exclude` kinds; `residual` (P15)
// is what the top-level spans and, in wall mode, the gaps between them leave.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

function usage(message) {
  if (message) console.error(`check.mjs: ${message}`);
  console.error("usage: check.mjs <trace> [--column 225K|500K|1M] [--clock auto|proc_cpu|cpu|wall] [--lines N] [--budget FILE] [--strict]");
  process.exit(64);
}

function parseArgs(argv) {
  const options = { column: "225K", clock: "auto", lines: null, budget: join(here, "budget.json"), strict: false, trace: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const value = () => {
      const inline = arg.includes("=") ? arg.slice(arg.indexOf("=") + 1) : null;
      if (inline !== null) return inline;
      index += 1;
      if (index >= argv.length) usage(`${arg} needs a value`);
      return argv[index];
    };
    if (arg.startsWith("--column")) options.column = value();
    else if (arg.startsWith("--clock")) options.clock = value();
    else if (arg.startsWith("--lines")) options.lines = Number(value());
    else if (arg.startsWith("--budget")) options.budget = value();
    else if (arg === "--strict") options.strict = true;
    else if (arg.startsWith("--")) usage(`unknown option ${arg}`);
    else if (options.trace === null) options.trace = arg;
    else usage(`unexpected argument ${arg}`);
  }
  if (options.trace === null) usage("no trace given");
  if (!["auto", "proc_cpu", "cpu", "wall"].includes(options.clock)) usage(`unknown clock ${options.clock}`);
  if (options.lines !== null && !(options.lines > 0)) usage("--lines must be a positive number");
  return options;
}

// One event per line; the array may be unterminated and its last line half
// written by a running compiler (Tools/perf/jetc-trace/summarize.mjs).
function readEvents(file) {
  const events = [];
  for (const raw of readFileSync(file, "utf8").split("\n")) {
    const line = raw.trim().replace(/,$/, "");
    if (line === "" || line === "[" || line === "]") continue;
    try {
      events.push(JSON.parse(line));
    } catch {
      // A partial last line.
    }
  }
  return events;
}

// Spans with their ancestors (indices of the spans open around them on their
// thread), and the counter events summed by name.
function collect(events) {
  const spans = [];
  const stacks = new Map();
  const counters = new Map();
  let lastTs = 0;
  for (const event of events) {
    if (typeof event.ts === "number") lastTs = Math.max(lastTs, event.ts);
    if (event.ph === "B") {
      const stack = stacks.get(event.tid) ?? [];
      stacks.set(event.tid, stack);
      const span = { kind: event.cat ?? event.name, label: event.name, tid: event.tid, start: event.ts, end: null, args: {}, ancestors: [...stack] };
      stack.push(spans.length);
      spans.push(span);
    } else if (event.ph === "E") {
      const stack = stacks.get(event.tid) ?? [];
      const index = stack.pop();
      if (index === undefined) continue;
      spans[index].end = event.ts;
      spans[index].args = event.args ?? {};
    } else if (event.ph === "C" && event.name !== "memory" && typeof event.args?.value === "number") {
      counters.set(event.name, (counters.get(event.name) ?? 0) + event.args.value);
    }
  }
  for (const span of spans) {
    if (span.end === null) {
      span.end = lastTs;
      span.open = true;
    }
  }
  return { spans, counters, lastTs };
}

const CLOCK_ARG = { proc_cpu: "proc_cpu_ns", cpu: "cpu_ns" };

// A span's time in ns on `clock`; wall time when the span lacks that clock.
function spanNs(span, clock, notes) {
  const field = CLOCK_ARG[clock];
  if (field && typeof span.args[field] === "number") return span.args[field];
  if (field) notes.add(`${span.kind}: no ${field}, wall time used`);
  return (span.end - span.start) * 1000;
}

function measureRow(row, spans, clock, notes) {
  const kinds = new Set(row.spans);
  const excluded = new Set(row.exclude ?? []);
  const included = new Set();
  spans.forEach((span, index) => {
    if (kinds.has(span.kind)) included.add(index);
  });
  let ns = 0;
  let count = 0;
  for (const index of included) {
    if (spans[index].ancestors.some((ancestor) => included.has(ancestor))) continue;
    ns += spanNs(spans[index], clock, notes);
    count += 1;
  }
  const removed = new Set();
  spans.forEach((span, index) => {
    if (excluded.has(span.kind) && span.ancestors.some((ancestor) => included.has(ancestor))) removed.add(index);
  });
  for (const index of removed) {
    if (spans[index].ancestors.some((ancestor) => removed.has(ancestor))) continue;
    ns -= spanNs(spans[index], clock, notes);
  }
  return { ns, count };
}

function fixed(value, digits) {
  return Number.isFinite(value) ? value.toFixed(digits) : "-";
}

function grouped(value) {
  return Number.isFinite(value) ? Math.round(value).toLocaleString("en-US") : "-";
}

function table(header, rows) {
  const widths = header.map((cell, column) => Math.max(String(cell).length, ...rows.map((row) => String(row[column]).length)));
  const align = (cell, column) => (column <= 1 ? String(cell).padEnd(widths[column]) : String(cell).padStart(widths[column]));
  const lines = [header.map(align).join("  "), widths.map((width) => "-".repeat(width)).join("  ")];
  for (const row of rows) lines.push(row.map(align).join("  "));
  return lines.join("\n");
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const budget = JSON.parse(readFileSync(options.budget, "utf8"));
  if (!budget.columns.includes(options.column)) usage(`unknown column ${options.column} (budget.json has ${budget.columns.join(", ")})`);
  const { spans, counters, lastTs } = collect(readEvents(options.trace));
  if (spans.length === 0) {
    console.error(`check.mjs: ${options.trace} has no spans`);
    process.exit(65);
  }
  let clock = options.clock;
  if (clock === "auto") clock = spans.some((span) => typeof span.args.proc_cpu_ns === "number") ? "proc_cpu" : "wall";
  let lines = options.lines;
  let linesSource = "--lines";
  if (lines === null && counters.has(budget.lines_counter)) {
    lines = counters.get(budget.lines_counter);
    linesSource = `trace counter ${budget.lines_counter}`;
  }
  if (lines === null) {
    lines = budget.default_lines;
    linesSource = "budget.json default_lines (the trace has no line counter)";
  }

  const notes = new Set();
  const column = options.column;
  const measured = [];
  let accounted = 0;
  for (const row of budget.rows) {
    if (row.residual) continue;
    const { ns, count } = measureRow(row, spans, clock, notes);
    accounted += ns;
    measured.push({ row, ns, present: count > 0 });
  }
  // Residual: the top-level spans (and, on the wall clock, the gaps between
  // them up to the last event) less every measured row.
  const residualRow = budget.rows.find((row) => row.residual);
  if (residualRow) {
    let total = 0;
    if (clock === "wall") {
      total = lastTs * 1000;
    } else {
      for (const span of spans) if (span.ancestors.length === 0) total += spanNs(span, clock, notes);
      notes.add(`${residualRow.id}: on the ${clock} clock the gaps between top-level spans are not measured`);
    }
    measured.push({ row: residualRow, ns: total - accounted, present: true });
  }

  let failed = false;
  let budgetSum = 0;
  let totalNs = 0;
  const out = [];
  for (const { row, ns, present } of measured) {
    const limit = row.budget[column];
    budgetSum += limit;
    totalNs += present ? ns : 0;
    const perLine = ns / lines;
    let status;
    let ratio = "-";
    if (!present) {
      status = "absent";
      if (options.strict) failed = true;
    } else {
      ratio = limit > 0 ? `${fixed(perLine / limit, 1)}x` : perLine > 0 ? "inf" : "0.0x";
      const over = perLine > limit;
      status = over ? "OVER" : "ok";
      if (over) failed = true;
    }
    out.push([row.id, row.phase, present ? fixed(ns / 1e9, 2) : "-", present ? grouped(perLine) : "-", grouped(limit), ratio, status]);
  }
  const ratioTotal = totalNs / lines / budgetSum;
  out.push(["total", "work sum", fixed(totalNs / 1e9, 2), grouped(totalNs / lines), grouped(budgetSum), `${fixed(ratioTotal, 1)}x`, totalNs / lines > budgetSum ? "OVER" : "ok"]);
  if (totalNs / lines > budgetSum) failed = true;

  console.log(`trace:   ${options.trace}`);
  console.log(`clock:   ${clock}${clock === "wall" ? " (the trace has no CPU clocks)" : ""}`);
  console.log(`lines:   ${grouped(lines)} (${linesSource})`);
  console.log(`column:  ${column} (CPU budget ${grouped(budget.cpu_budget[column])} ns/line at 16 threads; plan's stated work sum ${grouped(budget.work_sum_stated[column])})`);
  if (lastTs > 0) console.log(`wall:    ${fixed(lastTs / 1e6, 1)} s to the last event = ${grouped(lines / (lastTs / 1e6))} lines/s`);
  console.log("");
  console.log(table(["row", "phase", "seconds", "ns/line", "budget", "ratio", "status"], out));

  // Work counters per natural unit (budgets at the 1M column only).
  const unitRows = [];
  for (const { row, ns, present } of measured) {
    for (const unit of row.units ?? []) {
      const value = counters.get(unit.counter);
      const limit = unit.budget[column];
      const perUnit = value && present ? ns / value : NaN;
      let status = "-";
      if (value === undefined) status = "no counter";
      else if (present && limit !== undefined) {
        const over = perUnit > limit;
        status = over ? "OVER" : "ok";
        if (over) failed = true;
      }
      unitRows.push([row.id, unit.counter, value === undefined ? "-" : grouped(value), perUnit >= 1000 ? grouped(perUnit) : fixed(perUnit, 1), `ns/${unit.unit}`, limit === undefined ? "-" : grouped(limit), status]);
    }
  }
  console.log("");
  console.log(table(["row", "counter", "count", "ns/unit", "unit", "budget", "status"], unitRows));

  // The four measurements the plan names as unmeasured until S0.
  const detail = (kind) => {
    const byLabel = new Map();
    for (const span of spans) {
      if (span.kind !== kind) continue;
      const label = span.label === span.kind ? "" : span.label;
      const entry = byLabel.get(label) ?? { count: 0, ns: 0, items: 0 };
      entry.count += 1;
      entry.ns += spanNs(span, clock, notes);
      entry.items += typeof span.args.items === "number" ? span.args.items : 0;
      byLabel.set(label, entry);
    }
    return byLabel;
  };
  const census = [];
  const addCensus = (name, kind, labelled) => {
    const byLabel = detail(kind);
    if (byLabel.size === 0) {
      census.push([name, kind, "-", "-", "-", "not in trace"]);
      return;
    }
    if (labelled) {
      for (const [label, entry] of [...byLabel.entries()].sort()) census.push([name, `${kind} ${label}`.trim(), entry.count, fixed(entry.ns / 1e9, 3), grouped(entry.ns / lines), grouped(entry.items)]);
    } else {
      const sum = [...byLabel.values()].reduce((acc, entry) => ({ count: acc.count + entry.count, ns: acc.ns + entry.ns, items: acc.items + entry.items }), { count: 0, ns: 0, items: 0 });
      census.push([name, kind, sum.count, fixed(sum.ns / 1e9, 3), grouped(sum.ns / lines), grouped(sum.items)]);
    }
  };
  addCensus("D10b marker groups", "load.marker_groups.relex", false);
  addCensus("D10b marker groups", "load.marker_groups.reparse", false);
  addCensus("D12 comptime build", "comptime.build", true);
  addCensus("D5 MIR digest", "mir.digest", true);
  console.log("");
  console.log(table(["census", "span", "spans", "seconds", "ns/line", "items"], census));

  if (notes.size > 0) {
    console.log("");
    for (const note of [...notes].sort()) console.log(`note: ${note}`);
  }
  console.log("");
  console.log(failed ? `FAIL: over the ${column} budget` : `PASS: within the ${column} budget`);
  process.exit(failed ? 1 : 0);
}

main();
