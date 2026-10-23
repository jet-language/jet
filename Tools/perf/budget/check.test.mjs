import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const checker = join(dirname(fileURLToPath(import.meta.url)), "check.mjs");

// A trace in the native host's shape: `[`, one event per line, B/E spans
// with `cat` the kind, E args carrying the S0 clocks, C counter events.
function writeTrace(events) {
  const file = join(mkdtempSync(join(tmpdir(), "budget-check-")), "trace.jsonl");
  writeFileSync(file, `[\n${events.map((event) => `${JSON.stringify({ pid: 1, tid: 1, ...event })},`).join("\n")}\n`);
  return file;
}

const span = (kind, start, end, procCpuNs, label = kind, extra = {}) => [
  { name: label, cat: kind, ph: "B", ts: start },
  { name: label, cat: kind, ph: "E", ts: end, args: { items: 1, cpu_ns: procCpuNs, proc_cpu_ns: procCpuNs, majflt: 0, ...extra } },
];
const counter = (name, value) => ({ name, ph: "C", ts: 0, args: { value } });

function run(file, ...args) {
  const result = spawnSync(process.execPath, [checker, file, ...args], { encoding: "utf8" });
  return { status: result.status, out: result.stdout, err: result.stderr };
}

function row(out, id) {
  const line = out.split("\n").find((text) => text.startsWith(`${id} `));
  assert.ok(line, `no ${id} row in:\n${out}`);
  return line.trim().split(/\s{2,}/);
}

test("process CPU per line, nested checks and digests subtracted from their phase", () => {
  const file = writeTrace([
    counter("parse.user_lines", 1000),
    counter("parse.tokens", 9000),
    ...span("sources", 0, 1000, 1_000_000),
    { name: "load", cat: "load", ph: "B", ts: 1000 },
    ...span("parse", 1000, 5000, 3_000_000, "selfhost.jet"),
    ...span("load.marker_groups.relex", 5000, 6000, 1_000_000, "a.jet"),
    { name: "load", cat: "load", ph: "E", ts: 10000, args: { proc_cpu_ns: 9_000_000 } },
    { name: "optimize", cat: "optimize", ph: "B", ts: 20000 },
    ...span("optimize.legality", 20000, 21000, 50_000_000),
    ...span("mir.digest", 21000, 22000, 20_000_000, "optimize.seal"),
    ...span("optimize.lint", 22000, 23000, 30_000_000),
    { name: "optimize", cat: "optimize", ph: "E", ts: 30000, args: { proc_cpu_ns: 200_000_000 } },
    { name: "native.lower", cat: "native.lower", ph: "B", ts: 30000 },
    { name: "native.lower", cat: "native.lower", ph: "E", ts: 40000, args: { proc_cpu_ns: 10_000_000 } },
    counter("native.lower.lir_insts", 2000),
    ...span("native.object", 40000, 50000, 13_000_000),
  ]);
  const { status, out } = run(file, "--column", "1M");
  assert.equal(status, 1, out);
  assert.match(out, /clock:\s+proc_cpu/);
  assert.match(out, /lines:\s+1,000 \(trace counter parse\.user_lines\)/);
  // load (9 ms) + sources (1 ms) = 10 ms over 1,000 lines.
  assert.deepEqual(row(out, "P1+P2").slice(3, 7), ["10,000", "400", "25.0x", "OVER"]);
  // optimize 200 ms less legality 50, lint 30 and digest 20.
  assert.deepEqual(row(out, "P8").slice(3, 7), ["100,000", "1,300", "76.9x", "OVER"]);
  assert.deepEqual(row(out, "P9").slice(3, 7), ["80,000", "500", "160.0x", "OVER"]);
  assert.deepEqual(row(out, "D5").slice(3, 7), ["20,000", "0", "inf", "OVER"]);
  assert.deepEqual(row(out, "P10").slice(3, 7), ["10,000", "1,300", "7.7x", "OVER"]);
  assert.deepEqual(row(out, "P11+P12+P13").slice(3, 7), ["13,000", "3,250", "4.0x", "OVER"]);
  assert.equal(row(out, "P3")[6], "absent");
  assert.doesNotMatch(out, /native\.lir_lint/);
  assert.match(out, /load\.marker_groups\.relex\s+1\s+0\.001/);
  assert.match(out, /FAIL: over the 1M budget/);
});

test("a trace within budget passes; absent rows fail only with --strict", () => {
  const file = writeTrace([counter("parse.user_lines", 1_000_000), ...span("register", 0, 1000, 100_000_000)]);
  const relaxed = run(file, "--column", "225K");
  assert.equal(relaxed.status, 0, relaxed.out);
  assert.deepEqual(row(relaxed.out, "P3").slice(3, 7), ["100", "1,800", "0.1x", "ok"]);
  assert.match(relaxed.out, /PASS: within the 225K budget/);
  assert.equal(run(file, "--column", "225K", "--strict").status, 1);
});

test("a wall-only trace is measured on the wall clock per --lines", () => {
  const file = writeTrace([
    { name: "register", cat: "register", ph: "B", ts: 0 },
    { name: "register", cat: "register", ph: "E", ts: 2_000_000, args: { items: 3 } },
  ]);
  const { status, out } = run(file, "--lines", "1000");
  assert.equal(status, 1);
  assert.match(out, /clock:\s+wall/);
  assert.deepEqual(row(out, "P3").slice(2, 7), ["2.00", "2,000,000", "1,800", "1111.1x", "OVER"]);
  // Nothing else ran: the residual P15 is zero on the wall clock.
  assert.deepEqual(row(out, "P15").slice(3, 7), ["0", "800", "0.0x", "ok"]);
});

test("usage errors exit 64", () => {
  assert.equal(spawnSync(process.execPath, [checker], { encoding: "utf8" }).status, 64);
  assert.equal(run(writeTrace([]), "--column", "2M").status, 64);
});
