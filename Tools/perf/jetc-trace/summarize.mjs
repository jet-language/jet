#!/usr/bin/env node
// Summarize a compiler trace written with JET_TRACE_FILE=<path> (jetc or the
// Rust `jet`; JET_TRACE_DETAIL=functions adds per-function spans).
//
//   node Tools/perf/jetc-trace/summarize.mjs <trace.json> [--watch[=seconds]]
//
// Prints one row per span kind (count, total and longest wall time, items,
// peak RSS at its ends) and the spans still open with the latest progress
// counter, so a live, still-growing (unterminated) trace shows the current
// phase and how far along it is. --watch re-reads the file every 2 s (or the
// given seconds). The same file also loads in https://ui.perfetto.dev.

import { readFileSync } from "node:fs";

const args = process.argv.slice(2);
const path = args.find((arg) => !arg.startsWith("--"));
const watchArg = args.find((arg) => arg === "--watch" || arg.startsWith("--watch="));
if (!path) {
  console.error("usage: summarize.mjs <trace.json> [--watch[=seconds]]");
  process.exit(64);
}
const watchSeconds = watchArg ? Number(watchArg.split("=")[1] ?? 2) || 2 : 0;

// One event per line; the array may be unterminated and its last line may be
// half written by a running compiler.
function readEvents(file) {
  const events = [];
  for (const raw of readFileSync(file, "utf8").split("\n")) {
    const line = raw.trim().replace(/,$/, "");
    if (line === "" || line === "[" || line === "]") continue;
    try {
      events.push(JSON.parse(line));
    } catch {
      // A partial last line: the next read sees it whole.
    }
  }
  return events;
}

function summarize(events) {
  const kinds = new Map();
  const stacks = new Map();
  const progress = new Map();
  let startUnixMs = null;
  let lastTs = 0;
  let rssKb = null;
  let peakKb = null;
  const kindRow = (kind, ts) => {
    let row = kinds.get(kind);
    if (!row) {
      row = { kind, first: ts, count: 0, totalUs: 0, maxUs: 0, items: 0, hasItems: false, peakKb: 0 };
      kinds.set(kind, row);
    }
    return row;
  };
  for (const event of events) {
    if (typeof event.ts === "number") lastTs = Math.max(lastTs, event.ts);
    if (event.name === "trace.start") startUnixMs = event.args?.unix_ms ?? null;
    const stack = stacks.get(event.tid) ?? [];
    stacks.set(event.tid, stack);
    if (event.ph === "B") {
      stack.push({ kind: event.cat ?? event.name, label: event.name, ts: event.ts, args: event.args ?? {} });
      kindRow(event.cat ?? event.name, event.ts);
    } else if (event.ph === "E") {
      const open = stack.pop();
      if (!open) continue;
      const row = kindRow(open.kind, open.ts);
      const duration = event.ts - open.ts;
      row.count += 1;
      row.totalUs += duration;
      row.maxUs = Math.max(row.maxUs, duration);
      if (typeof event.args?.items === "number") {
        row.items += event.args.items;
        row.hasItems = true;
      }
      if (typeof event.args?.peak_rss_kb === "number") {
        row.peakKb = Math.max(row.peakKb, event.args.peak_rss_kb);
        peakKb = Math.max(peakKb ?? 0, event.args.peak_rss_kb);
        rssKb = event.args.rss_kb;
      }
    } else if (event.ph === "C" && event.name !== "memory") {
      const previous = progress.get(event.name);
      const done = event.args?.done ?? 0;
      // A restart (done fell) begins a new rate window.
      const first = previous && done >= previous.done ? previous.first : { done, ts: event.ts };
      progress.set(event.name, { done, total: event.args?.total ?? 0, ts: event.ts, first });
    }
  }
  // Spans still open count with their running time so far.
  for (const stack of stacks.values()) {
    for (const open of stack) {
      const row = kindRow(open.kind, open.ts);
      row.open = (row.open ?? 0) + 1;
      row.totalUs += lastTs - open.ts;
      row.maxUs = Math.max(row.maxUs, lastTs - open.ts);
    }
  }
  return { kinds: [...kinds.values()], stacks, progress, startUnixMs, lastTs, rssKb, peakKb };
}

const seconds = (us) => (us / 1e6).toFixed(us >= 1e8 ? 0 : us >= 1e7 ? 1 : 2);
const pad = (text, width) => String(text).padEnd(width);
const lpad = (text, width) => String(text).padStart(width);
const mb = (kb) => (kb == null ? "-" : `${Math.round(kb / 1024)}`);

function render(file) {
  const summary = summarize(readEvents(file));
  const lines = [];
  const width = Math.max(14, ...summary.kinds.map((row) => row.kind.length));
  lines.push(`${pad("span", width)} ${lpad("count", 7)} ${lpad("total s", 9)} ${lpad("max s", 8)} ${lpad("items", 9)} ${lpad("peak MB", 8)}`);
  for (const row of summary.kinds.sort((left, right) => left.first - right.first)) {
    // `+N`: N spans of this kind are still open; their time so far is included.
    const count = row.open ? `${row.count}+${row.open}` : row.count;
    lines.push(
      `${pad(row.kind, width)} ${lpad(count, 7)} ${lpad(seconds(row.totalUs), 9)} ${lpad(seconds(row.maxUs), 8)} ${lpad(row.hasItems ? row.items : "-", 9)} ${lpad(row.peakKb ? mb(row.peakKb) : "-", 8)}`,
    );
  }
  lines.push("");
  const stale = summary.startUnixMs == null ? null : (Date.now() - (summary.startUnixMs + summary.lastTs / 1000)) / 1000;
  lines.push(
    `trace time ${seconds(summary.lastTs)} s` +
      (stale == null ? "" : `, last event ${stale.toFixed(0)} s ago`) +
      `, rss ${mb(summary.rssKb)} MB, peak ${mb(summary.peakKb)} MB`,
  );
  let anyOpen = false;
  for (const [tid, stack] of summary.stacks) {
    if (stack.length === 0) continue;
    anyOpen = true;
    lines.push(`open on thread ${tid}:`);
    for (const [depth, span] of stack.entries()) {
      const place = span.args.index >= 0 ? ` [${span.args.index + 1}/${span.args.total}]` : "";
      const label = span.label === span.kind ? span.kind : `${span.kind} ${span.label}`;
      let line = `${"  ".repeat(depth + 1)}${label}${place} running ${seconds(summary.lastTs - span.ts)} s`;
      const counter = summary.progress.get(span.kind);
      if (counter && counter.ts >= span.ts && counter.total > 0) {
        const percent = ((100 * counter.done) / counter.total).toFixed(1);
        line += `, ${counter.done}/${counter.total} (${percent}%)`;
        const doneSince = counter.done - counter.first.done;
        const elapsed = counter.ts - counter.first.ts;
        if (doneSince > 0 && elapsed > 0) {
          const remainingUs = ((counter.total - counter.done) * elapsed) / doneSince;
          line += `, ~${seconds(remainingUs)} s left at the current rate`;
        }
      }
      lines.push(line);
    }
  }
  if (!anyOpen) lines.push("no open spans (compile finished or not started)");
  return lines.join("\n");
}

if (watchSeconds > 0) {
  const tick = () => {
    let text;
    try {
      text = render(path);
    } catch (error) {
      text = `cannot read ${path}: ${error.message}`;
    }
    process.stdout.write(`\x1b[2J\x1b[H${new Date().toLocaleTimeString()}  ${path}\n\n${text}\n`);
  };
  tick();
  setInterval(tick, watchSeconds * 1000);
} else {
  console.log(render(path));
}
