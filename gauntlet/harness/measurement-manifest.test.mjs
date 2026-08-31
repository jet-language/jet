import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const gauntletDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = path.resolve(gauntletDir, "..");
const entriesDir = path.join(gauntletDir, "entries");
const manifestPath = path.join(gauntletDir, "measurement-manifest.json");
const matrixPath = path.join(gauntletDir, "matrix.json");

async function exists(file) {
  try {
    await fs.access(file);
    return true;
  } catch {
    return false;
  }
}

function sourcePath(entryDir, relative) {
  assert.equal(typeof relative, "string");
  assert.ok(relative.length > 0 && !path.isAbsolute(relative));
  const resolved = path.resolve(entryDir, relative);
  const remainder = path.relative(entryDir, resolved);
  assert.ok(remainder && !remainder.startsWith("..") && !path.isAbsolute(remainder));
  return resolved;
}

function repoSourcePath(relative) {
  assert.equal(typeof relative, "string");
  assert.ok(relative.length > 0 && !path.isAbsolute(relative));
  const resolved = path.resolve(repoDir, relative);
  const remainder = path.relative(repoDir, resolved);
  assert.ok(remainder && !remainder.startsWith("..") && !path.isAbsolute(remainder));
  return resolved;
}

test("measurement manifest covers every corpus entry and source pair", async () => {
  const manifest = JSON.parse(await fs.readFile(manifestPath, "utf8"));
  assert.equal(manifest.version, 1);
  assert.deepEqual(manifest.contract, {
    loc: "nonblank_noncomment_lines",
    token_metric: "source_tokens",
    token_definition: "nonempty_runs_split_by_unicode_whitespace",
    loc_ratio_max: 1.2,
    token_verdict: "jet_less_than_python",
    eligible_pair: "jet_and_python_sources",
  });
  assert.deepEqual(manifest.corpus, {
    entry_count: 19,
    python_pair_count: 17,
    matrix_cell_count: 24,
    allowed_uncovered_cells: ["embedded.data", "embedded.kernel"],
    entry_names: [
      "binparse",
      "bulkrename",
      "concurrency-service",
      "csvtransform",
      "datasummary",
      "http-client",
      "http-service",
      "logreport",
      "nbody",
      "parallel-grep",
      "procpipe",
      "regex-logscan",
      "sieve",
      "taskfile-cli",
      "text-script",
      "tzreport",
      "web-app",
      "web-widget",
      "wordfreq",
    ],
  });
  assert.equal(manifest.report_contract.id, "gauntlet-report-v1");
  assert.deepEqual(manifest.report_contract.primary_metric_by_mode, {
    batch: "runtime_wall_seconds",
    "batch-steps": "runtime_wall_seconds",
    service: "service_latency_ms_p50",
    web: "runtime_first_stdout_seconds",
    "web-app": "runtime_first_stdout_seconds",
  });
  assert.deepEqual(manifest.report_contract.tier_policy_by_mode, {
    batch: ["aot", "run", "dev"],
    "batch-steps": ["aot", "run", "dev"],
    service: ["aot", "run", "dev"],
    web: ["aot", "run", "dev"],
    "web-app": ["aot"],
  });
  assert.deepEqual(manifest.report_contract.required_jet_tiers, ["aot", "run"]);
  assert.deepEqual(manifest.report_contract.optional_jet_tiers, ["dev"]);
  assert.equal(manifest.report_contract.missing_metric_verdict, "unmeasured");
  assert.deepEqual(manifest.report_contract.axis_schemas, {
    live_reload: "gauntlet-axis-live-reload-v1",
    memory_safety_fuzz: "gauntlet-axis-memory-safety-fuzz-v1",
  });
  assert.equal(manifest.report_contract.axis_publication, "required_axes_complete_and_unblocked");
  assert.deepEqual(Object.keys(manifest.axes).sort(), ["live_reload", "memory_safety_fuzz"]);
  for (const axis of Object.values(manifest.axes)) assert.equal(axis.status, "required");
  for (const card of Object.values(manifest.loss_owners)) assert.equal(Number.isInteger(card), true);

  const liveReload = manifest.axes.live_reload;
  assert.equal(liveReload.schema, "gauntlet-axis-live-reload-v1");
  assert.equal(liveReload.metric, "reload_latency_ms");
  assert.equal(liveReload.workload, "web-app");
  assert.deepEqual(liveReload.signal, {
    kind: "monotonic_http_counter",
    definition: "GET readiness path returns a numeric value greater than the value observed before the edit",
  });
  assert.deepEqual(liveReload.budget, {
    sample_count: 3,
    startup_timeout_ms: 30000,
    reload_timeout_ms: 30000,
    poll_interval_ms: 20,
  });
  assert.deepEqual(liveReload.edit, { from: "reload-before", to: "reload-after" });
  assert.deepEqual(liveReload.phases, {
    cold: "first measured edit after a fresh process reaches readiness",
    warm: "measured edit after two unmeasured edits in the same fresh process",
  });
  assert.deepEqual(liveReload.fairness, [
    "same source edit",
    "same observable readiness signal",
    "fresh process per sample",
    "median cold and warm reload samples",
  ]);
  assert.deepEqual(liveReload.runners.map((runner) => runner.id).sort(), ["bun", "entr+cc", "jet-dev", "nodemon", "vite"]);
  for (const runner of liveReload.runners) {
    assert.ok(runner.files.length > 0, `${runner.id}: no fixture files`);
    assert.ok(runner.command.length > 0, `${runner.id}: no command`);
    assert.equal(runner.readiness.status, 200, `${runner.id}: readiness status`);
    assert.deepEqual(runner.output, {
      path: runner.id === "jet-dev" ? "/app.js" : "/__axis_output",
      status: 200,
    }, `${runner.id}: output acknowledgement`);
    assert.ok(runner.files.some((file) => file.target === runner.edit_file), `${runner.id}: edit file is not staged`);
    for (const file of runner.files) assert.equal(await exists(repoSourcePath(file.source)), true, `${runner.id}: missing ${file.source}`);
  }

  const memorySafety = manifest.axes.memory_safety_fuzz;
  assert.equal(memorySafety.schema, "gauntlet-axis-memory-safety-fuzz-v1");
  assert.equal(memorySafety.metric, "memory_safety_findings");
  assert.deepEqual(memorySafety.corpus, {
    path: "fuzz-input.bin",
    generator: "xorshift32-v1",
    seed: 2272,
    case_count: 128,
    bytes_per_case: 64,
  });
  assert.deepEqual(memorySafety.budget, { wall_timeout_ms: 30000, cpu_seconds: 10, memory_mb: 512 });
  assert.deepEqual(memorySafety.oracle, {
    algorithm: "memory-safety-case-summary-v1",
    output: "cases {case_count} valid {valid} boundary {boundary} oob {oob} use_after_free {use_after_free} wrong_output {wrong_output} bytes {byte_count} checksum {u32_sum} semantic {semantic}\n",
  });
  assert.deepEqual(memorySafety.fairness, [
    "same generated input file",
    "same timeout and resource budget",
    "sanitizer or equivalent finding evidence",
    "deduplicate each finding before close",
  ]);
  assert.deepEqual(memorySafety.runners.map((runner) => runner.id).sort(), ["c", "jet-default", "rust", "zig"]);
  for (const runner of memorySafety.runners) {
    assert.ok(runner.files.length > 0, `${runner.id}: no fixture files`);
    assert.ok(runner.run.length > 0, `${runner.id}: no run command`);
    assert.ok(runner.evidence.patterns.length > 0, `${runner.id}: no finding evidence`);
    for (const file of runner.files) assert.equal(await exists(repoSourcePath(file.source)), true, `${runner.id}: missing ${file.source}`);
  }

  const matrix = JSON.parse(await fs.readFile(matrixPath, "utf8"));
  assert.equal(matrix.cells.length, manifest.corpus.matrix_cell_count);
  assert.deepEqual(
    manifest.corpus.allowed_uncovered_cells.filter((id) => matrix.cells.some((cell) => cell.id === id)),
    manifest.corpus.allowed_uncovered_cells,
  );

  const rows = manifest.entries;
  assert.ok(Array.isArray(rows));
  const names = rows.map((row) => row.name);
  assert.equal(new Set(names).size, names.length);
  assert.equal(rows.length, manifest.corpus.entry_count);
  assert.equal(rows.filter((row) => row.python !== null).length, manifest.corpus.python_pair_count);
  assert.deepEqual([...names].sort(), [...manifest.corpus.entry_names].sort());

  const directories = (await fs.readdir(entriesDir, { withFileTypes: true }))
    .filter((item) => item.isDirectory())
    .map((item) => item.name);
  assert.deepEqual([...names].sort(), [...directories].sort());

  const cellOwners = new Map();
  for (const row of rows) {
    assert.equal(typeof row.name, "string");
    assert.equal(Object.hasOwn(row, "python"), true);
    const entryDir = path.join(entriesDir, row.name);
    const entry = JSON.parse(await fs.readFile(path.join(entryDir, "entry.json"), "utf8"));
    assert.equal(entry.name, row.name);
    assert.equal(entry.languages.includes("jet"), true);
    for (const cell of entry.cells) cellOwners.set(cell, (cellOwners.get(cell) ?? 0) + 1);
    assert.equal(row.python !== null, entry.languages.includes("python"));
    assert.equal(await exists(sourcePath(entryDir, row.jet)), true, `${row.name}: missing Jet source`);

    const pythonDefault = path.join(entryDir, "python", "main.py");
    if (row.python === null) {
      assert.equal(await exists(pythonDefault), false, `${row.name}: unlisted Python source`);
    } else {
      assert.equal(await exists(sourcePath(entryDir, row.python)), true, `${row.name}: missing Python source`);
    }
  }
  const allowed = new Set(manifest.corpus.allowed_uncovered_cells);
  for (const cell of matrix.cells) if (!allowed.has(cell.id)) assert.equal(cellOwners.get(cell.id), 1, `${cell.id}: expected one corpus owner`);
});
