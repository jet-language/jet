import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import os from "node:os";

import { elf } from "../../perf/asm-parity/ref-build.mjs";
import { elfLinkage, loadEntries, standingsPlans } from "./run.mjs";
import {
  buildStandings,
  ceilingMeasurements,
  entryStandings,
  planCells,
  planEntry,
  plannedSizeBasis,
  STANDINGS_COLUMNS,
  STANDINGS_METRICS,
  STANDINGS_SCHEMA,
  standingsLines,
  SIZE_BASES,
  standingsPolicy,
} from "./standings.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const readJson = async (relative) => JSON.parse(await fs.readFile(path.join(repoRoot, relative), "utf8"));
const matrix = await readJson("Tools/gauntlet/matrix.json");
const manifest = await readJson("Tools/gauntlet/measurement-manifest.json");
const references = await readJson("Tools/perf/asm-parity/cells.json");
const policy = standingsPolicy(matrix, manifest);
const PEERS = ["rust", "c", "zig", "go", "python", "bun", "node", "vite"];
const METRICS = STANDINGS_METRICS.map((metric) => metric.id);

const batchEntry = { name: "sieve", mode: "batch", cells: ["cli.sieve"], languages: ["jet", "rust", "python", "node"] };
const programs = (present) => Object.fromEntries(PEERS.map((peer) => [peer, { path: `${peer}/main`, exists: present.includes(peer) }]));
const ok = (metrics, extra = {}) => ({ status: "ok", metrics, verification: { status: "passed", kind: "byte_exact_stdout" }, ...extra });
const timed = (wall, cycles, extra = {}) => ({
  runtime_first_stdout_seconds: wall / 2,
  runtime_wall_seconds: wall,
  runtime_cycles: cycles,
  runtime_peak_rss_kb: 1000,
  ...extra,
});
const deployed = (bytes, basis = "static stripped") => ({ size: { status: "ok", basis, build: "static stripped build", file_bytes: bytes, libraries: [], bytes } });

test("the standings policy names every peer and the asm ceiling, and the manifest explains what no peer can run", () => {
  assert.deepEqual(policy.issues, []);
  assert.deepEqual(policy.peers, PEERS);
  assert.equal(policy.ceiling, "asm");
  const vite = policy.not_applicable.find((rule) => rule.peer === "vite");
  assert.deepEqual(vite.modes, ["batch", "batch-steps", "service"]);
  assert.match(vite.reason, /web front end/);
  assert.deepEqual(METRICS, ["startup", "wall", "cycles", "peak_rss", "size"]);
  assert.deepEqual(Object.keys(policy.size_basis), SIZE_BASES);
  assert.deepEqual(standingsPolicy({ rails: {} }, {}).issues.length > 0, true);
  const unstated = { ...manifest, standings: { ...manifest.standings, size_basis: { binary: "bytes of the built executable" } } };
  assert.ok(standingsPolicy(matrix, unstated).issues.some((issue) => /size_basis must define static stripped/.test(issue)));
});

test("a plan gives every peer and the ceiling a row: measured, uncovered, not applicable or unavailable, with a reason", () => {
  const reference = references.find((item) => item.name === "sieve");
  const plan = planEntry(batchEntry, policy, {
    programs: { ...programs(["rust", "python", "node", "bun", "go"]), asm: { path: "Tools/perf/asm-parity/sieve/ref.S", exists: true } },
    references: [reference],
    toolchains: { go: { status: "unavailable", command: ["go", "version"], exit_code: 127, version: "go: command not found" }, rust: { status: "ok" } },
  });
  assert.deepEqual(Object.keys(plan.peers), [...PEERS, "asm"]);
  assert.equal(plan.peers.rust.plan, "measure");
  assert.equal(plan.peers.bun.plan, "measure");
  assert.equal(plan.peers.c.plan, "uncovered");
  assert.match(plan.peers.c.reason, /no c program/);
  assert.equal(plan.peers.go.plan, "unavailable");
  assert.match(plan.peers.go.reason, /go toolchain unavailable: `go version` exit 127/);
  assert.equal(plan.peers.vite.plan, "not_applicable");
  assert.equal(plan.peers.asm.plan, "measure");
  assert.equal(plan.peers.asm.role, "ceiling");
  assert.equal(plan.peers.asm.reference, "sieve");

  const bare = planEntry(batchEntry, policy, { programs: programs([]), references: [] });
  assert.equal(bare.peers.asm.plan, "uncovered");
  assert.match(bare.peers.asm.reason, /no hand-written reference/);
  const doubled = planEntry(batchEntry, policy, { programs: programs([]), references: [{ name: "a" }, { name: "b" }] });
  assert.equal(doubled.peers.asm.plan, "failed");
  const web = planEntry({ ...batchEntry, mode: "web" }, policy, { programs: programs(["vite"]), references: [] });
  assert.equal(web.peers.vite.plan, "measure");
});

test("dry-run cells cover entry x peer x metric; service cycles and a missing perf stay rows with reasons", () => {
  const service = planEntry({ name: "http-service", mode: "service", cells: [] }, policy, { programs: programs(["rust"]), references: [] });
  const perfless = planEntry(batchEntry, policy, { programs: programs(["rust"]), references: [], toolchains: { perf: { status: "unavailable", command: ["perf", "--version"], exit_code: 127 } } });
  const cells = planCells([service, perfless]);
  assert.equal(cells.length, 2 * (PEERS.length + 1) * METRICS.length);
  const serviceCycles = cells.find((cell) => cell.entry === "http-service" && cell.peer === "rust" && cell.metric === "cycles");
  assert.equal(serviceCycles.status, "unmeasured");
  assert.match(serviceCycles.reason, /runs until it is stopped/);
  const perfCycles = cells.find((cell) => cell.entry === "sieve" && cell.peer === "rust" && cell.metric === "cycles");
  assert.equal(perfCycles.status, "unavailable");
  for (const cell of cells) {
    if (cell.status !== "measure") assert.ok(cell.reason, `${cell.entry}/${cell.peer}/${cell.metric} has no reason`);
    if (cell.metric !== "size" || cell.status !== "measure") assert.equal(cell.size_basis, null, `${cell.entry}/${cell.peer}/${cell.metric}`);
  }
  const rustSize = cells.find((cell) => cell.entry === "sieve" && cell.peer === "rust" && cell.metric === "size");
  assert.deepEqual([rustSize.jet_size_basis, rustSize.size_basis], ["static stripped", "static stripped, else stripped + shared libraries"]);
});

test("each measured size cell is planned on one stated basis", () => {
  assert.equal(plannedSizeBasis(batchEntry, "c"), "static stripped, else stripped + shared libraries");
  assert.equal(plannedSizeBasis(batchEntry, "asm", "ceiling"), "static stripped");
  assert.equal(plannedSizeBasis(batchEntry, "python"), "script+runtime");
  assert.equal(plannedSizeBasis({ ...batchEntry, mode: "web" }, "rust"), "web artifact");
  assert.equal(plannedSizeBasis({ ...batchEntry, mode: "web-app" }, "vite"), "bundle");
  assert.equal(plannedSizeBasis({ ...batchEntry, mode: "web" }, "node"), "script+runtime");
  const overridden = { ...batchEntry, spec: { build: { c: ["gcc", "-O3", "main.c", "-o", "main-c"] } } };
  assert.match(plannedSizeBasis(overridden, "c"), /^stripped \+ shared libraries if dynamic \(the entry overrides the build command\)$/);
  assert.equal(plannedSizeBasis(overridden, "rust"), "static stripped, else stripped + shared libraries");
  assert.equal(planEntry({ ...batchEntry, mode: "web" }, policy, { programs: programs([]), references: [] }).jet_size_basis, "web artifact");
});

test("every comparison entry in the corpus plans a row for every peer and its ceiling", async () => {
  const { loaded } = await loadEntries(path.join(repoRoot, "Tools/gauntlet/entries"), null);
  assert.ok(loaded.length >= 25);
  const plans = await standingsPlans(loaded, policy, references, {});
  for (const { plan, stage } of plans) {
    assert.deepEqual(Object.keys(plan.peers), [...PEERS, "asm"], plan.entry);
    for (const [peer, item] of Object.entries(plan.peers)) {
      assert.ok(["measure", "uncovered", "not_applicable", "unavailable", "failed"].includes(item.plan), `${plan.entry}/${peer}`);
      if (item.plan !== "measure") assert.ok(item.reason, `${plan.entry}/${peer} has no reason`);
    }
    assert.deepEqual(stage.peers, PEERS.filter((peer) => plan.peers[peer].plan === "measure"));
  }
  const byName = Object.fromEntries(plans.map(({ plan }) => [plan.entry, plan]));
  assert.equal(byName.hello.peers.asm.reference, "hello");
  assert.equal(byName.sieve.peers.asm.reference, "sieve");
  assert.equal(byName.sieve.peers.bun.plan, "measure");
  assert.equal(byName["web-widget"].peers.vite.plan, "measure");
  assert.equal(byName.wordfreq.peers.vite.plan, "not_applicable");
  assert.equal(byName["http-service"].peers.asm.plan, "uncovered");
});

test("judged rows: the floor passes below 1.00, the ceiling meets at or below 1.00, and nothing measured is dropped", () => {
  const reference = { name: "sieve" };
  const plan = planEntry(batchEntry, policy, {
    programs: { ...programs(["rust", "python", "node", "bun", "c", "zig"]), asm: { path: "Tools/perf/asm-parity/sieve/ref.S", exists: true } },
    references: [reference],
  });
  const runtimes = {
    python: { status: "ok", bytes: 9000, path: "/runtime/python3" },
    node: { status: "ok", bytes: 90000, path: "/runtime/node" },
    bun: { status: "unmeasured", reason: "ldd exit 1" },
  };
  const result = {
    rows: {
      jet: ok(timed(1, 120), deployed(1000)),
      rust: ok(timed(2, 200), deployed(4000, "stripped + shared libraries")),
      python: ok(timed(0.5, 50, { source_bytes: 100 }), { script: { bytes: 100, files: [{ path: "main.py", bytes: 100 }] } }),
      node: { status: "broken", reason: "stdout differs", verification: { status: "failed", kind: "byte_exact_stdout", reason: "line 1 differs" } },
    },
    standings_rows: {
      c: { status: "broken", reason: "exit 1: boom", verification: { status: "failed", kind: "byte_exact_stdout", reason: "exit 1: boom" } },
      zig: { status: "broken", reason: "cold build exit 127", missing_toolchain: true, verification: { status: "failed", kind: "build" } },
      bun: ok(timed(1, 100), { script: { bytes: 100, files: [] } }),
      asm: ok(timed(0.5, 100), deployed(500)),
    },
  };
  const standing = entryStandings(plan, result, { runtimes });
  assert.equal(standing.peers.rust.metrics.wall.ratio, 0.5);
  assert.equal(standing.peers.rust.metrics.wall.verdict, "pass");
  assert.equal(standing.peers.rust.metrics.size.verdict, "pass");
  assert.equal(standing.peers.rust.metrics.size.peer_basis, "stripped + shared libraries");
  assert.equal(standing.peers.rust.metrics.size.jet_basis, "static stripped");
  assert.equal(standing.peers.rust.metrics.size.jet_detail.build, "static stripped build");
  assert.equal(standing.peers.python.metrics.wall.verdict, "fail");
  assert.equal(standing.peers.python.metrics.size.peer, 9100);
  assert.equal(standing.peers.python.metrics.size.peer_basis, "script+runtime");
  assert.equal(standing.peers.python.metrics.size.detail.runtime_bytes, 9000);
  assert.equal(standing.peers.bun.metrics.wall.verdict, "fail");
  assert.equal(standing.peers.bun.metrics.size.verdict, "unmeasured");
  assert.match(standing.peers.bun.metrics.size.reason, /bun runtime footprint unmeasured: ldd exit 1/);
  assert.equal(standing.peers.node.status, "mismatched");
  assert.equal(standing.peers.c.status, "failed");
  assert.equal(standing.peers.zig.status, "unavailable");
  assert.equal(standing.peers.go.status, "uncovered");
  assert.equal(standing.peers.vite.status, "not_applicable");
  const cycles = standing.peers.asm.metrics.cycles;
  assert.equal(cycles.verdict, "miss");
  assert.ok(Math.abs(cycles.gap - 0.2) < 1e-12);
  assert.equal(standing.peers.asm.metrics.startup.verdict, "miss");
  assert.equal(standing.peers.asm.metrics.peak_rss.verdict, "meet");
  for (const item of Object.values(standing.peers)) assert.deepEqual(Object.keys(item.metrics), METRICS);

  const broken = entryStandings(plan, { ...result, rows: { ...result.rows, jet: { status: "broken", reason: "cold build exit 1", verification: { status: "failed", kind: "build" } } } }, { runtimes });
  assert.equal(broken.peers.rust.metrics.wall.verdict, "unmeasured");
  assert.match(broken.peers.rust.metrics.wall.reason, /^Jet: failed: cold build exit 1/);

  const crashed = entryStandings(plan, { status: "broken", reason: "harness error: boom", rows: {} }, { runtimes });
  assert.equal(crashed.peers.rust.status, "failed");
  assert.match(crashed.peers.rust.reason, /harness error: boom/);

  const stripless = entryStandings(plan, { ...result, rows: { ...result.rows, rust: ok(timed(2, 200), { size: { status: "unavailable", reason: "strip main-rust exit 127: strip: command not found" } }) } }, { runtimes });
  assert.equal(stripless.peers.rust.metrics.size.verdict, "unavailable");
  assert.match(stripless.peers.rust.metrics.size.reason, /deployed size unmeasured: strip main-rust exit 127/);
  const unrecorded = entryStandings(plan, { ...result, rows: { ...result.rows, rust: ok(timed(2, 200)) } }, { runtimes });
  assert.equal(unrecorded.peers.rust.metrics.size.verdict, "unmeasured");
  assert.equal(unrecorded.peers.rust.metrics.size.reason, "deployed size was not recorded");

  const peersOnly = entryStandings(plan, { ...result, rows: { ...result.rows, jet: { status: "unavailable", not_measured: true, reason: "not measured: this run measures rust, python only" } } }, { runtimes });
  assert.deepEqual(peersOnly.jet, { status: "not_measured", reason: "not measured: this run measures rust, python only" });
  assert.deepEqual(peersOnly.peers.rust.metrics.wall, { jet: null, peer: 2, ratio: null, verdict: "unmeasured", reason: "Jet: not measured: this run measures rust, python only", unit: "s", source: "runtime_wall_seconds" });
  assert.equal(peersOnly.peers.asm.metrics.cycles.peer, 100);
  assert.equal(peersOnly.peers.bun.metrics.size.peer, null);
  assert.match(peersOnly.peers.bun.metrics.size.reason, /^bun runtime footprint unmeasured: ldd exit 1; Jet: not measured/);
});

test("the section lists every floor loss and ceiling miss, feeds the scoreboard's ceiling and renders one line per cell", () => {
  const plan = planEntry(batchEntry, policy, {
    programs: { ...programs(["rust", "python"]), asm: { path: "x", exists: true } },
    references: [{ name: "sieve" }],
  });
  const result = {
    rows: { jet: ok(timed(1, 120), deployed(1000)), rust: ok(timed(2, 200, { runtime_peak_rss_kb: 2000 }), deployed(4000)), python: ok(timed(0.5, 50), { script: { bytes: 10, files: [] } }) },
    standings_rows: { asm: ok(timed(1, 100), deployed(500)) },
  };
  const runtimes = { python: { status: "ok", bytes: 100, path: "/runtime/python3" } };
  const section = buildStandings([entryStandings(plan, result, { runtimes })], policy, { generated: "2026-11-13T00:00:00.000Z", runtimes });
  assert.equal(section.schema, STANDINGS_SCHEMA);
  assert.deepEqual(Object.keys(section.size_basis), SIZE_BASES);
  assert.deepEqual(section.losses.floor.map((loss) => `${loss.peer}/${loss.metric}`), ["python/size", "python/cycles", "python/startup", "python/wall", "python/peak_rss"]);
  assert.ok(section.losses.ceiling.some((miss) => miss.metric === "cycles" && Math.abs(miss.ratio - 1.2) < 1e-12));
  assert.ok(section.open.some((cell) => cell.peer === "c" && cell.verdict === "uncovered"));
  assert.ok(!section.open.some((cell) => cell.peer === "vite"));
  assert.equal(section.summary.floor.pass, 5);
  assert.equal(section.summary.floor.fail, 5);
  const forbidden = new Set(["best", "selected", "average", "mean", "avg", "aggregate", "winner", "chosen"]);
  const walk = (value) => {
    if (!value || typeof value !== "object") return;
    for (const [key, nested] of Object.entries(value)) {
      assert.ok(!forbidden.has(key.toLowerCase()), `forbidden key ${key}`);
      walk(nested);
    }
  };
  walk(section);

  const ceiling = ceilingMeasurements(section.entries);
  assert.deepEqual(ceiling.rows.map((row) => row.variant), ["asm", "jet-default"]);
  assert.equal(ceiling.rows[1].cell, "sieve");
  assert.ok(Math.abs(ceiling.rows[1].cycles_x - 1.2) < 1e-12);
  assert.equal(ceiling.rows[1].size_x, 2);
  assert.equal(ceiling.rows[0].output, "same");

  const lines = standingsLines(section);
  assert.equal(lines[0], STANDINGS_COLUMNS.join("\t"));
  assert.equal(lines.length, 1 + (PEERS.length + 1) * METRICS.length);
  assert.ok(lines.includes(["sieve", "rust", "peer", "wall", "1", "2", "s", "0.50", "pass", "-", "-"].join("\t")));
  assert.ok(lines.includes(["sieve", "rust", "peer", "size", "1000", "4000", "bytes", "0.25", "pass", "static stripped / static stripped", "-"].join("\t")));
});

test("the deployed size reads each binary's linkage: a dynamic loader makes it dynamic, symbols make it unstripped", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "standings-elf-"));
  try {
    const image = elf(Buffer.from([0xb8, 0x3c, 0, 0, 0, 0x31, 0xff, 0x0f, 0x05]));
    await fs.writeFile(path.join(dir, "static"), image);
    assert.deepEqual(await elfLinkage(path.join(dir, "static")), { dynamic: false, stripped: true, bytes: image.length });
    const loaded = Buffer.from(image);
    loaded.writeUInt32LE(3, 64 + 56);
    await fs.writeFile(path.join(dir, "dynamic"), loaded);
    assert.equal((await elfLinkage(path.join(dir, "dynamic"))).dynamic, true);
    await fs.writeFile(path.join(dir, "script"), "#!/bin/sh\n");
    assert.match((await elfLinkage(path.join(dir, "script"))).error, /script is not an ELF executable/);
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});
