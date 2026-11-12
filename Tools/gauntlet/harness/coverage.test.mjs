import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { MINIMUM_PROGRAMS, buildCensus, corpusIssues, loadCorpus, parseCells } from "./coverage.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const HEADER = "workload\ttier\tpeer\tmetric\tjet\tpeer_value\tratio\tverdict\tcards";

function gauntletProgram(name, twin) {
  return { source: "gauntlet", name, key: name, cells: [`cell.${name}`], peers: ["c"], twins: { c: twin }, owners: { runtime: 1111, rss: 1112 } };
}

function scriptbenchProgram(name, twin) {
  return { source: "scriptbench", name, key: `w-${name}`, cells: [], peers: ["python"], twins: { python: twin }, owners: ["#2222"] };
}

function aotResult(name, metrics) {
  return {
    entry: { name },
    comparisons: {
      c: {
        primary_metric: "runtime_wall_seconds",
        tiers: { aot: { metrics: Object.fromEntries(Object.entries(metrics).map(([metric, ratio]) => [metric, { jet: ratio * 10, peer: 10, ratio }])) } },
      },
    },
  };
}

test("the repository's coverage corpus has at least 30 programs, each with its peer twins on disk", async () => {
  const programs = await loadCorpus();
  assert.ok(programs.length >= MINIMUM_PROGRAMS, `${programs.length} programs`);
  assert.ok(programs.some((program) => program.source === "gauntlet"));
  assert.ok(programs.some((program) => program.source === "scriptbench"));
  assert.deepEqual(await corpusIssues(programs), []);
});

test("a planted memory loss and time loss are listed with program, peer, metric, ratio and owner, grouped by owner", () => {
  const programs = [gauntletProgram("kernel", here), scriptbenchProgram("words", here)];
  const report = { entries: [aotResult("kernel", { runtime_wall_seconds: 0.5, runtime_peak_rss_kb: 1.25 })] };
  const cells = parseCells([
    HEADER,
    "w-words\tjet-bin\tpython\twall\t30\t20\t1.50\tLOSS\t#2222",
    "w-words\tjet-bin\tpython\trss\t1000\t9000\t0.11\twin\t#2222",
    "w-words\tjet-run-first\tpython\twall\t90\t20\t4.50\tloss (report-only)\t#2222",
  ].join("\n"));
  const census = buildCensus(programs, report, cells);
  assert.equal(census.measured, 4);
  assert.deepEqual(census.uncovered, []);
  assert.deepEqual(census.losses.map(({ source, program, peer, metric, ratio, owner }) => ({ source, program, peer, metric, ratio, owner })), [
    { source: "gauntlet", program: "kernel", peer: "c", metric: "runtime_peak_rss_kb", ratio: 1.25, owner: "#1112" },
    { source: "scriptbench", program: "words", peer: "python", metric: "runtime_wall_seconds", ratio: 1.5, owner: "#2222" },
  ]);
  assert.deepEqual(census.roots.map((root) => [root.owner, root.losses.map((loss) => loss.program)]), [["#1112", ["kernel"]], ["#2222", ["words"]]]);
});

test("a ratio of exactly 1.00 is a loss, and a peer that overran the timeout is a win without a ratio", () => {
  const programs = [gauntletProgram("tie", here), scriptbenchProgram("slow", here)];
  const report = { entries: [aotResult("tie", { runtime_wall_seconds: 1, runtime_peak_rss_kb: 0.9 })] };
  const cells = parseCells([
    HEADER,
    "w-slow\tjet-bin\tpython\twall\t30\t>60s\t<1\twin\t#2222",
    "w-slow\tjet-bin\tpython\trss\t1000\t9000\t0.11\twin\t#2222",
  ].join("\n"));
  const census = buildCensus(programs, report, cells);
  assert.deepEqual(census.losses.map((loss) => [loss.program, loss.metric, loss.owner]), [["tie", "runtime_wall_seconds", "#1111"]]);
  assert.deepEqual(census.uncovered, []);
});

test("a program, peer or metric without a reading is uncovered, not a win", () => {
  const programs = [gauntletProgram("absent", here), gauntletProgram("partial", here), scriptbenchProgram("halfway", here)];
  const report = { entries: [aotResult("partial", { runtime_wall_seconds: 0.5 })] };
  const cells = parseCells([HEADER, "w-halfway\tjet-bin\tpython\twall\t30\t40\t0.75\twin\t#2222"].join("\n"));
  const census = buildCensus(programs, report, cells);
  assert.equal(census.losses.length, 0);
  assert.deepEqual(census.uncovered.map((row) => `${row.source}/${row.program}/${row.peer}/${row.metric}`), [
    "gauntlet/absent/c/runtime_wall_seconds",
    "gauntlet/absent/c/runtime_peak_rss_kb",
    "gauntlet/partial/c/runtime_peak_rss_kb",
    "scriptbench/halfway/python/runtime_peak_rss_kb",
  ]);
});

test("a corpus under 30 programs, a repeated program and a missing twin are corpus issues", async () => {
  const programs = Array.from({ length: MINIMUM_PROGRAMS - 1 }, (_, index) => gauntletProgram(`p${index}`, here));
  assert.deepEqual(await corpusIssues(programs), [`the corpus has ${MINIMUM_PROGRAMS - 1} programs, fewer than ${MINIMUM_PROGRAMS}`]);
  const repeated = [...programs, gauntletProgram("p0", path.join(here, "no-such-twin"))];
  assert.deepEqual(await corpusIssues(repeated), ["gauntlet/p0 is listed twice", "gauntlet/p0: the c twin is missing"]);
});

test("the CLI fails when the repository corpus has no readings and names every uncovered program", () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "coverage-cli-"));
  writeFileSync(path.join(dir, "report.json"), JSON.stringify({ entries: [] }));
  writeFileSync(path.join(dir, "cells.tsv"), `${HEADER}\n`);
  const run = spawnSync(process.execPath, [path.join(here, "coverage.mjs"), "--gauntlet", path.join(dir, "report.json"), "--scriptbench", dir, "--out", path.join(dir, "census.json")], { encoding: "utf8" });
  assert.equal(run.status, 1, run.stderr);
  assert.match(run.stdout, /^uncovered\tgauntlet\/binparse\t/m);
  assert.match(run.stdout, /^uncovered\tscriptbench\//m);
  assert.match(run.stdout, /^coverage: \d+ programs \(\d+ gauntlet, \d+ scriptbench\), 0 readings, 0 losses in 0 roots, \d+ uncovered$/m);
});
