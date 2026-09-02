import test from "node:test";
import { spawn } from "node:child_process";
import http from "node:http";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { buildScoreboard, comparisons, httpProbe, metricApplicability, processTreeRssKb, probeMatches, publicationState, ratioVerdict, validateEntryShape, validateResultShape } from "./run.mjs";
import { liveReloadInternals } from "./live-reload.mjs";

const gauntletDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = path.resolve(gauntletDir, "..");
const entriesDir = path.join(gauntletDir, "entries");
const manifestPath = path.join(gauntletDir, "measurement-manifest.json");
const matrixPath = path.join(gauntletDir, "matrix.json");

test("HTTP behavior probes retain and validate response headers", async () => {
  const server = http.createServer((request, response) => {
    response.statusCode = 200;
    if (request.url !== "/missing") {
      response.setHeader("Content-Type", request.url === "/wrong" ? "text/plain" : "application/json");
    }
    response.end('{"ok":true}');
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = server.address().port;
  const probe = {
    method: "GET",
    path: "/",
    expectStatus: 200,
    expectBody: '{"ok":true}',
    expectHeaders: { "Content-Type": "application/json" },
  };
  try {
    const accepted = await httpProbe(port, probe);
    assert.equal(accepted.ok, true);
    assert.equal(accepted.headers["content-type"], "application/json");
    assert.equal(probeMatches(probe, accepted, "http"), true);

    const wrong = await httpProbe(port, { ...probe, path: "/wrong" });
    assert.equal(probeMatches(probe, wrong, "http"), false);
    const missing = await httpProbe(port, { ...probe, path: "/missing" });
    assert.equal(probeMatches(probe, missing, "http"), false);
    assert.equal(probeMatches({ ...probe, expectHeaders: undefined }, accepted, "http"), false);
  } finally {
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
});

test("process-tree RSS sampling includes service descendants", async () => {
  if (!(await exists("/proc"))) return;
  const residentChild = "const resident = Buffer.alloc(16 * 1024 * 1024, 1); setInterval(() => resident[0], 1000);";
  const root = spawn(process.execPath, ["-e", [
    "const { spawn } = require(\"node:child_process\");",
    `spawn(${JSON.stringify(process.execPath)}, ["-e", ${JSON.stringify(residentChild)}], { stdio: "ignore" });`,
    "setInterval(() => {}, 1000);",
  ].join(" ")], { stdio: "ignore", detached: true });
  const readRootRss = async () => {
    try {
      const status = await fs.readFile(`/proc/${root.pid}/status`, "utf8");
      const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
      return match ? Number(match[1]) : null;
    } catch {
      return null;
    }
  };
  try {
    let treeRss = null;
    let rootRss = null;
    for (let attempt = 0; attempt < 20; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 50));
      rootRss = await readRootRss();
      treeRss = await processTreeRssKb(root.pid);
      if (Number.isFinite(rootRss) && Number.isFinite(treeRss) && treeRss > rootRss) break;
    }
    assert.ok(Number.isFinite(rootRss));
    assert.ok(Number.isFinite(treeRss));
    assert.ok(treeRss > rootRss);
  } finally {
    try { process.kill(-root.pid, "SIGKILL"); } catch {}
    await new Promise((resolve) => {
      if (root.exitCode !== null) resolve();
      else root.once("close", resolve);
    });
  }
});

test("HTTP service probes require an expected header map", () => {
  const entry = {
    name: "header-fixture",
    mode: "service",
    cells: ["fixture.cell"],
    languages: ["jet"],
    service: {
      readyPath: "/",
      probe: [{ method: "GET", path: "/" }],
    },
    authoring: { jet: { author: "test", notes: "", turns: 0, retries: 0, diagnosticsHit: [] } },
  };
  const issues = validateEntryShape(
    { entry, nameDeclared: true, directoryName: "header-fixture", dir: "." },
    { cells: [{ id: "fixture.cell" }] },
  );
  assert.ok(issues.some((issue) => issue.includes("expectHeaders")));
});

test("live reload probes use the caller environment by default", async () => {
  const probe = await liveReloadInternals.probeAxisTool(null, repoDir, process.execPath, path.join(repoDir, "target/debug/jet"));
  assert.equal(probe.status, "available");
  assert.equal(probe.version_exit_code, 0);
});

test("entr help probe records its release line on documented exit one", () => {
  const accepted = liveReloadInternals.toolVersion("entr", {
    code: 1,
    stdout: Buffer.from("entr help\nrelease: 5.7\n"),
    stderr: Buffer.alloc(0),
  });
  assert.deepEqual(accepted, {
    ok: true,
    version: "release: 5.7",
    output: "entr help\nrelease: 5.7",
  });

  const rejected = liveReloadInternals.toolVersion("entr", {
    code: 1,
    stdout: Buffer.from("entr help\n"),
    stderr: Buffer.alloc(0),
  });
  assert.equal(rejected.ok, false);
});

test("nodemon probe preserves non-semver store identity", () => {
  const probe = liveReloadInternals.toolVersion("nodemon", {
    code: 0,
    stdout: Buffer.from("master: <none>\n"),
    stderr: Buffer.alloc(0),
  });
  assert.deepEqual(probe, {
    ok: true,
    version: "master: <none>",
    output: "master: <none>",
  });
});

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

function passedTierTrace() {
  const rows = [{ function: "fixture", tier: "native", reason: null, millis: 0.001 }];
  return {
    status: "passed",
    channel: "compiler_owned_sidecar",
    rows,
    native_rows: 1,
    interp_rows: 0,
    whole_program_deopt: false,
    invocations: [{
      command: ["jet", "run", "run.jet", "--"],
      exit_code: 0,
      rows,
      native_rows: 1,
      interp_rows: 0,
      whole_program_deopt: false,
    }],
  };
}

test("run-tier validation rejects untrusted or mixed trace evidence", () => {
  const missing = scoreboardFixture();
  delete missing.result.jet_tiers.run.trace;
  assert.ok(validateResultShape(missing.result).some((issue) => issue.includes("native tier trace")));

  const mixed = scoreboardFixture();
  mixed.result.jet_tiers.run.trace.interp_rows = 1;
  mixed.result.jet_tiers.run.trace.invocations[0].interp_rows = 1;
  assert.ok(validateResultShape(mixed.result).some((issue) => issue.includes("native tier trace")));

  const untrusted = scoreboardFixture();
  untrusted.result.jet_tiers.run.trace.channel = "combined_stderr";
  assert.ok(validateResultShape(untrusted.result).some((issue) => issue.includes("native tier trace")));
});

test("scoreboard applies the Rust noise band to every Rust rail", () => {
  assert.equal(ratioVerdict(0.99, "rust"), "win");
  assert.equal(ratioVerdict(1, "rust"), "parity");
  assert.equal(ratioVerdict(1.05, "rust"), "parity");
  assert.equal(ratioVerdict(1.0501, "rust"), "loss");
  assert.equal(ratioVerdict(0.99, "zig"), "win");
  assert.equal(ratioVerdict(1, "zig"), "loss");
  assert.equal(ratioVerdict(1.01, "c"), "loss");
  assert.equal(ratioVerdict(1.01, "rust-expert"), "parity");
  assert.equal(ratioVerdict(1.01, "unknown-expert"), "loss");
  assert.equal(ratioVerdict(Number.NaN, "rust"), null);
  assert.equal(ratioVerdict(0, "rust"), null);
  assert.equal(ratioVerdict(-1, "zig"), null);
});

test("expert peer comparisons use the matched Jet expert AOT row", () => {
  const entry = { name: "expert-fixture", mode: "batch", languages: ["jet", "jet-expert", "c-expert", "rust-expert"] };
  const metricNames = [
    "runtime_wall_seconds",
    "runtime_peak_rss_kb",
    "runtime_first_stdout_seconds",
    "cold_build_seconds",
    "warm_build_seconds",
    "binary_bytes",
    "loc",
    "source_bytes",
    "tokens",
    "source_tokens",
  ];
  const values = (runtime) => Object.fromEntries(metricNames.map((metric) => [metric, runtime]));
  const rows = {
    jet: { status: "ok", metrics: values(1) },
    "jet-expert": { status: "ok", metrics: values(2) },
    "c-expert": { status: "ok", metrics: values(4) },
    "rust-expert": { status: "ok", metrics: values(4) },
  };
  const tiers = {
    aot: { status: "ok", metrics: values(1) },
    run: { status: "ok", metrics: values(1) },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = comparisons(entry, Object.keys(rows), rows, tiers);
  assert.equal(result["jet-expert"], undefined);
  assert.equal(result["c-expert"].jet_configuration, "jet-expert");
  assert.deepEqual(result["c-expert"].required_tiers, ["aot"]);
  assert.equal(result["c-expert"].tiers.aot.metrics.runtime_wall_seconds.jet, 2);
  assert.equal(result["c-expert"].tiers.aot.metrics.runtime_wall_seconds.peer, 4);
  assert.equal(result["c-expert"].tiers.aot.metrics.runtime_wall_seconds.verdict, "win");
  const missingExpert = comparisons(entry, ["jet", "c-expert"], { jet: rows.jet, "c-expert": rows["c-expert"] }, tiers);
  assert.equal(missingExpert["c-expert"].tiers.aot.metrics.runtime_wall_seconds.status, "unmeasured");
});

test("compile-only metrics are explicitly not applicable on interpreted tiers", () => {
  const entry = { name: "tier-fixture", mode: "batch", languages: ["jet", "rust"] };
  const metrics = {
    runtime_wall_seconds: 1,
    runtime_peak_rss_kb: 1,
    runtime_first_stdout_seconds: 1,
    cold_build_seconds: 1,
    warm_build_seconds: 1,
    binary_bytes: 1,
    loc: 1,
    source_bytes: 1,
    tokens: 1,
    source_tokens: 1,
  };
  const rows = {
    jet: { status: "ok", metrics },
    rust: { status: "ok", metrics: Object.fromEntries(Object.keys(metrics).map((metric) => [metric, 2])) },
  };
  const tiers = {
    aot: { status: "ok", metrics },
    run: { status: "ok", metrics },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = comparisons(entry, Object.keys(rows), rows, tiers);
  assert.equal(result.rust.tiers.aot.metrics.binary_bytes.status, "measured");
  assert.equal(result.rust.tiers.run.metrics.binary_bytes.status, "not_applicable");
  assert.equal(result.rust.tiers.run.metrics.binary_bytes.applicability.basis, "no_compile_phase");
  assert.equal(result.rust.tiers.run.metrics.loc.status, "measured");
  assert.equal(result.rust.tiers.run.metrics.loc.jet, 1);
  assert.equal(result.rust.tiers.run.metrics.loc.peer, 2);
});

test("candidate peer loss is retained in the aggregate cell verdict", () => {
  const metrics = [
    "runtime_wall_seconds",
    "runtime_peak_rss_kb",
    "runtime_first_stdout_seconds",
    "cold_build_seconds",
    "warm_build_seconds",
    "binary_bytes",
    "loc",
    "source_bytes",
    "tokens",
    "source_tokens",
  ];
  const values = (value) => Object.fromEntries(metrics.map((metric) => [metric, value]));
  const entry = {
    name: "candidate-fixture",
    mode: "batch",
    cells: ["fixture.cell"],
    languages: ["jet", "rust", "cxx"],
  };
  const rows = {
    jet: { status: "ok", metrics: values(1) },
    rust: { status: "ok", metrics: values(1) },
    cxx: { status: "ok", metrics: values(1) },
  };
  const tiers = {
    aot: { status: "ok", metrics: values(1) },
    run: { status: "ok", metrics: values(1) },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = {
    entry,
    status: "ok",
    rows,
    comparisons: comparisons(entry, Object.keys(rows), rows, tiers),
    jet_tiers: tiers,
  };
  const matrix = {
    metric_applicability: {
      default: "required",
      not_applicable: "explicit_structural_reason",
      missing: "unmeasured_and_publication_blocked",
    },
    cells: [{ id: "fixture.cell", domain: "text", kind: "application" }],
  };
  const scoreboard = buildScoreboard(matrix, [result], { corpus: { allowed_uncovered_cells: [] }, loss_owners: {} }, {
    status: "available",
    revision: 1,
    cards: new Map(),
  });
  const record = scoreboard.cells[0].entries[0];
  assert.equal(record.peers.find((peer) => peer.language === "rust").verdict, "parity");
  assert.equal(record.peers.find((peer) => peer.language === "cxx").verdict, "loss");
  assert.equal(record.verdict, "loss");
});
function scoreboardFixture({ lossMetrics = [], missingMetrics = [], includeNode = false } = {}) {
  const metrics = [
    "runtime_wall_seconds",
    "runtime_peak_rss_kb",
    "runtime_first_stdout_seconds",
    "cold_build_seconds",
    "warm_build_seconds",
    "binary_bytes",
    "loc",
    "source_bytes",
    "tokens",
    "source_tokens",
  ];
  const languages = includeNode ? ["jet", "rust", "node"] : ["jet", "rust"];
  const entry = {
    name: "fixture",
    mode: "batch",
    cells: ["fixture.cell"],
    languages,
    ...(includeNode
      ? {
          non_applicable: {
            node: {
              basis: "no_compile_phase",
              reason: "The fixture declares no compiled Node rail.",
              evidence: "The fixture has no Node compilation phase.",
            },
          },
        }
      : {}),
  };
  const rawMetrics = (language) => Object.fromEntries([
    ...metrics.map((metric) => {
      if (missingMetrics.includes(metric)) return [metric, null];
      if (language === "jet") return [metric, 1];
      return [metric, lossMetrics.includes(metric) ? 0.5 : 2];
    }),
    ["source_sha256", `${language}-source`],
  ]);
  const rows = {
    jet: {
      language: "jet",
      status: "ok",
      disqualified: false,
      verification: { status: "passed", kind: "byte_exact_stdout" },
      metrics: rawMetrics("jet"),
      provenance: { base_language: "jet", source_sha256: "jet-source" },
    },
    rust: {
      language: "rust",
      status: "ok",
      disqualified: false,
      verification: { status: "passed", kind: "byte_exact_stdout" },
      metrics: rawMetrics("rust"),
      provenance: { base_language: "rust", source_sha256: "rust-source" },
    },
    ...(includeNode ? {
      node: {
        language: "node",
        status: "not_applicable",
        metrics: {},
      },
    } : {}),
  };
  const tiers = {
    aot: { status: "ok", verification: { status: "passed", kind: "byte_exact_stdout" }, metrics: rawMetrics("jet") },
    run: { status: "ok", verification: { status: "passed", kind: "byte_exact_stdout" }, metrics: rawMetrics("jet"), trace: passedTierTrace() },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = {
    entry,
    status: "ok",
    rows,
    comparisons: comparisons(entry, Object.keys(rows), rows, tiers),
    jet_tiers: tiers,
  };
  return {
    matrix: {
      metric_applicability: {
        default: "required",
        not_applicable: "explicit_structural_reason",
        missing: "unmeasured_and_publication_blocked",
      },
      cells: [{ id: "fixture.cell", domain: "text", kind: "application" }],
    },
    result,
    manifest: { corpus: { allowed_uncovered_cells: [] }, loss_owners: {} },
  };
}

test("successful run tiers require compiler-owned native trace evidence", () => {
  const missing = scoreboardFixture();
  delete missing.result.jet_tiers.run.trace;
  assert.ok(validateResultShape(missing.result).some((issue) => issue.includes("native tier trace")));

  const mixed = scoreboardFixture();
  mixed.result.jet_tiers.run.trace.interp_rows = 1;
  mixed.result.jet_tiers.run.trace.invocations[0].interp_rows = 1;
  assert.ok(validateResultShape(mixed.result).some((issue) => issue.includes("native tier trace")));

  const untrusted = scoreboardFixture();
  untrusted.result.jet_tiers.run.trace.channel = "combined_stderr";
  assert.ok(validateResultShape(untrusted.result).some((issue) => issue.includes("native tier trace")));
});


function fixturePublication(fixture, scoreboard) {
  return publicationState({
    fullScope: true,
    loaded: [{ entry: fixture.result.entry }],
    skipped: [],
    matrix: fixture.matrix,
    manifest: { ...fixture.manifest, corpus: { ...fixture.manifest.corpus, entry_count: 1 } },
    sourceMeasurements: {
      contract: {},
      coverage: { denominator_pass: true },
      aggregate: {},
    },
    results: [fixture.result],
    scoreboard,
    axes: {},
    validationIssues: [],
  });
}

test("scoreboard publication fails when a non-primary metric loses", () => {
  const fixture = scoreboardFixture({ lossMetrics: ["runtime_peak_rss_kb", "binary_bytes", "loc", "source_bytes", "tokens", "source_tokens"] });
  const scoreboard = buildScoreboard(fixture.matrix, [fixture.result], fixture.manifest, { status: "available", revision: 1, cards: new Map() });
  const record = scoreboard.cells[0].entries[0];
  assert.equal(record.primary_verdict, "win");
  assert.equal(record.metric_verdicts.runtime_peak_rss_kb, "loss");
  assert.equal(record.metric_verdicts.binary_bytes, "loss");
  assert.equal(record.metric_verdicts.loc, "loss");
  assert.equal(record.metric_verdicts.source_bytes, "loss");
  assert.equal(record.metric_verdicts.tokens, "loss");
  assert.equal(record.metric_verdicts.source_tokens, "loss");
  assert.equal(record.verdict, "loss");
  const publication = fixturePublication(fixture, scoreboard);
  assert.equal(publication.complete, false);
  assert.equal(publication.blockers.some((blocker) => blocker.includes("source LOC contract failed")), false);
  assert.equal(publication.blockers.some((blocker) => blocker.includes("source token contract failed")), false);
  assert.ok(publication.blockers.some((blocker) => blocker.includes("runtime_peak_rss_kb: metric loss")));
  assert.ok(publication.blockers.some((blocker) => blocker.includes("loc: metric loss")));
  assert.ok(publication.blockers.some((blocker) => blocker.includes("source_tokens: metric loss")));
});

test("required null metric is unmeasured while valid N/A stays excluded", () => {
  const missing = scoreboardFixture({ missingMetrics: ["runtime_peak_rss_kb"], includeNode: true });
  const missingScoreboard = buildScoreboard(missing.matrix, [missing.result], missing.manifest, { status: "available", revision: 1, cards: new Map() });
  const missingRecord = missingScoreboard.cells[0].entries[0];
  assert.equal(missingRecord.metric_verdicts.runtime_peak_rss_kb, "unmeasured");
  assert.equal(missingRecord.peers.find((peer) => peer.language === "node").metric_comparisons.runtime_wall_seconds.status, "not_applicable");
  assert.equal(missingRecord.peers.find((peer) => peer.language === "node").metric_comparisons.runtime_wall_seconds.applicability.reason, missing.result.entry.non_applicable.node.reason);
  assert.equal(fixturePublication(missing, missingScoreboard).complete, false);
});

test("not_applicable requires an explicit structural reason", () => {
  const entry = {
    name: "fixture",
    mode: "batch",
    cells: ["fixture.cell"],
    languages: ["jet", "node"],
    non_applicable: { node: { basis: "workload_surface", evidence: "structural evidence" } },
    authoring: { jet: { author: "test", notes: "", turns: 0, retries: 0, diagnosticsHit: [] } },
  };
  const issues = validateEntryShape(
    { entry, nameDeclared: true, directoryName: "fixture", dir: "." },
    { metric_applicability: { default: "required", not_applicable: "explicit_structural_reason", missing: "unmeasured_and_publication_blocked" }, cells: [{ id: "fixture.cell" }] },
  );
  assert.ok(issues.some((issue) => issue.includes("missing reason")));
  assert.ok(issues.some((issue) => issue.includes("structural basis")));
});



test("scoreboard ignores selected or aggregate peer claims", () => {
  const fixture = scoreboardFixture();
  fixture.result.comparisons.rust.selected_peer = "untrusted";
  fixture.result.comparisons.rust.tiers.aot.metrics.runtime_wall_seconds = {
    status: "measured",
    jet: 100,
    peer: 1,
    ratio: 100,
    verdict: "loss",
  };
  const scoreboard = buildScoreboard(fixture.matrix, [fixture.result], fixture.manifest, {
    status: "available",
    revision: 1,
    cards: new Map(),
  });
  assert.equal(scoreboard.cells[0].entries[0].primary_verdict, "win");
  assert.ok(scoreboard.validation_issues.some((issue) => issue.includes("selected or aggregate")));
});

test("publication rejects invalid result status and omitted peer rows", () => {
  const invalid = scoreboardFixture();
  invalid.result.rows.rust.status = "inconclusive";
  const invalidScoreboard = buildScoreboard(invalid.matrix, [invalid.result], invalid.manifest, {
    status: "available",
    revision: 1,
    cards: new Map(),
  });
  const invalidPublication = fixturePublication(invalid, invalidScoreboard);
  assert.equal(invalidPublication.complete, false);
  assert.ok(invalidPublication.blockers.some((blocker) => blocker.includes("inconclusive")));

  const omitted = scoreboardFixture();
  delete omitted.result.rows.rust;
  const omittedScoreboard = buildScoreboard(omitted.matrix, [omitted.result], omitted.manifest, {
    status: "available",
    revision: 1,
    cards: new Map(),
  });
  const omittedPublication = fixturePublication(omitted, omittedScoreboard);
  assert.equal(omittedPublication.complete, false);
  assert.ok(omittedPublication.blockers.some((blocker) => blocker.includes("missing result row")));
});

test("publication rejects peer sample identity mismatch", () => {
  const fixture = scoreboardFixture();
  fixture.result.comparisons.rust.peer_sample.source = "rows.jet.metrics";
  const scoreboard = buildScoreboard(fixture.matrix, [fixture.result], fixture.manifest, {
    status: "available",
    revision: 1,
    cards: new Map(),
  });
  const publication = fixturePublication(fixture, scoreboard);
  assert.equal(publication.complete, false);
  assert.ok(publication.blockers.some((blocker) => blocker.includes("peer sample identity")));
});

test("web-app results omit undeclared Jet tiers", () => {
  const fixture = scoreboardFixture();
  fixture.result.entry.mode = "web-app";
  delete fixture.result.jet_tiers.run;
  delete fixture.result.jet_tiers.dev;
  fixture.result.comparisons = comparisons(
    fixture.result.entry,
    Object.keys(fixture.result.rows),
    fixture.result.rows,
    { aot: fixture.result.jet_tiers.aot },
  );
  assert.deepEqual(validateResultShape(fixture.result), []);
});

test("measurement manifest covers every corpus entry and source pair", async () => {
  const manifest = JSON.parse(await fs.readFile(manifestPath, "utf8"));
  assert.equal(manifest.version, 1);
  assert.deepEqual(manifest.contract, {
    loc: "nonblank_noncomment_lines",
    token_metric: "source_tokens",
    token_definition: "nonempty_runs_split_by_unicode_whitespace",
    eligible_pair: "jet_and_python_sources",
  });
  assert.deepEqual(manifest.corpus, {
    entry_count: 22,
    python_pair_count: 20,
    matrix_cell_count: 25,
    allowed_uncovered_cells: [],
    entry_names: [
      "binparse",
      "bulkrename",
      "concurrency-service",
      "csvtransform",
      "datasummary",
      "embedded-data",
      "embedded-sensor-ring",
      "http-client",
      "http-service",
      "logreport",
      "nbody",
      "parallel-grep",
      "procpipe",
      "regex-find-all-4mb",
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
  assert.deepEqual(manifest.report_contract.ratio_verdicts, {
    rust: { win: "<1", parity: "<=1.05", loss: ">1.05" },
    non_rust: { win: "<1", parity: null, loss: ">=1" },
  });
  assert.equal(manifest.report_contract.loss_owner_required_for, "any_comparable_metric_loss");
  assert.deepEqual(manifest.report_contract.peer_measurement, {
    ratio_tiers: ["aot", "run"],
    trace_only_tiers: [],
    sample_binding: "immutable_peer_row_reused_per_declared_jet_tier",
  });
  assert.deepEqual(manifest.report_contract.metric_applicability, {
    default: "required",
    not_applicable: "explicit_structural_reason",
    missing: "unmeasured_and_publication_blocked",
  });
  assert.deepEqual(manifest.report_contract.aot_only_metrics, ["cold_build_seconds", "warm_build_seconds", "binary_bytes"]);
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
  const ownerCategories = new Set(["runtime", "latency", "rss", "build", "binary", "source"]);
  for (const owners of Object.values(manifest.loss_owners)) {
    assert.equal(typeof owners, "object");
    for (const [category, card] of Object.entries(owners)) {
      assert.equal(ownerCategories.has(category), true);
      assert.equal(Number.isInteger(card), true);
    }
  }
  for (const row of manifest.entries) assert.equal(Number.isInteger(manifest.loss_owners[row.name]?.source), true);

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
  assert.deepEqual(matrix.metric_applicability, {
    default: "required",
    not_applicable: "explicit_structural_reason",
    missing: "unmeasured_and_publication_blocked",
  });
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
