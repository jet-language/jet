import test from "node:test";
import { spawn } from "node:child_process";
import http from "node:http";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { buildScoreboard, CEILING_POLICY, ceilingReferenceIssues, comparisons, httpProbe, metricApplicability, processTreeRssKb, probeMatches, publicationState, ratioVerdict, STRICT_PERFORMANCE_POLICY, strictPerformancePolicyIssues, validateEntryShape, validateResultShape } from "./run.mjs";
import { liveReloadInternals } from "./live-reload.mjs";
import { compileThroughputContractIssues } from "./compile-throughput.mjs";
import { BUILD_SYSTEM_CELLS, buildSystemContractIssues } from "./build-system.mjs";
import { EDIT_FILES, generate as generateBuildSystemProject } from "../axes/build-system/generate.mjs";

const gauntletDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = path.resolve(gauntletDir, "../..");
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
test("live-reload HTTP probes keep the default cap and accept the Jet app cap", async () => {
  const appBody = Buffer.alloc(4_900_082, "a");
  const server = http.createServer((_request, response) => response.end(appBody));
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = server.address().port;
  try {
    const rejected = await liveReloadInternals.httpProbe(port, { path: "/app.js" });
    assert.equal(rejected.ok, false);
    assert.equal(rejected.fatal, true);
    assert.equal(rejected.error, "HTTP probe body exceeded 4194304 bytes");

    const jetAppLimit = liveReloadInternals.axisOutputBodyLimit({ id: "jet-dev" }, { path: "/app.js" });
    assert.equal(jetAppLimit, 5 * 1024 * 1024);
    const accepted = await liveReloadInternals.httpProbe(port, { path: "/app.js" }, 5_000, jetAppLimit);
    assert.equal(accepted.ok, true);
    assert.equal(Buffer.byteLength(accepted.body), appBody.length);
    assert.equal(accepted.body, appBody.toString("utf8"));
    assert.equal(
      liveReloadInternals.axisOutputBodyLimit({ id: "vite" }, { path: "/app.js" }),
      4 * 1024 * 1024,
    );
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
  const probe = await liveReloadInternals.probeAxisTool(null, repoDir, process.execPath, "jet");
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

test("scoreboard judges every peer, Rust included, by the strict <1 comparator", () => {
  assert.equal(ratioVerdict(0.99), "win");
  assert.equal(ratioVerdict(1), "loss");
  assert.equal(ratioVerdict(1.05), "loss");
  assert.equal(ratioVerdict(1.01), "loss");
  assert.equal(ratioVerdict(Number.NaN), null);
  assert.equal(ratioVerdict(0), null);
  assert.equal(ratioVerdict(-1), null);
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
  const entry = { name: "tier-fixture", mode: "batch", languages: ["jet", "python"] };
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
    python: { status: "ok", metrics: Object.fromEntries(Object.keys(metrics).map((metric) => [metric, 2])) },
  };
  const tiers = {
    aot: { status: "ok", metrics },
    run: { status: "ok", metrics, cache_states: { edited: { status: "ok", metrics }, repeat: { status: "ok", metrics } } },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = comparisons(entry, Object.keys(rows), rows, tiers);
  assert.equal(result.python.tiers.aot.metrics.binary_bytes.status, "measured");
  assert.equal(result.python.tiers.run.metrics.binary_bytes.status, "not_applicable");
  assert.equal(result.python.tiers.run.metrics.binary_bytes.applicability.basis, "no_compile_phase");
  assert.equal(result.python.tiers.run.metrics.loc.status, "measured");
  assert.equal(result.python.tiers.run.metrics.loc.jet, 1);
  assert.equal(result.python.tiers.run.metrics.loc.peer, 2);
});

test("the run tier is judged against the interpreted peers in every gated cache state", () => {
  const entry = { name: "run-fixture", mode: "batch", languages: ["jet", "rust", "python"] };
  const metrics = (wall) => ({
    runtime_wall_seconds: wall,
    runtime_peak_rss_kb: 1,
    runtime_first_stdout_seconds: wall,
    cold_build_seconds: 1,
    warm_build_seconds: 1,
    binary_bytes: 1,
    loc: 1,
    source_bytes: 1,
    tokens: 1,
    source_tokens: 1,
  });
  const rows = {
    jet: { status: "ok", metrics: metrics(1) },
    rust: { status: "ok", metrics: metrics(2) },
    python: { status: "ok", metrics: metrics(4) },
  };
  const tiers = {
    aot: { status: "ok", metrics: metrics(1) },
    run: {
      status: "ok",
      metrics: metrics(5),
      cache_states: {
        edited: { status: "ok", metrics: metrics(5) },
        repeat: { status: "ok", metrics: metrics(3) },
        fresh_install: { status: "ok", metrics: metrics(50) },
      },
    },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = comparisons(entry, Object.keys(rows), rows, tiers);
  assert.deepEqual(Object.keys(result.rust.tiers), ["aot"]);
  assert.deepEqual(result.rust.required_tiers, ["aot"]);
  const wall = result.python.tiers.run.metrics.runtime_wall_seconds;
  assert.equal(wall.cache_state, "edited");
  assert.equal(wall.verdict, "loss");
  assert.deepEqual(Object.keys(wall.cache_states), ["edited", "repeat"]);
  assert.equal(wall.cache_states.repeat.verdict, "win");
  const unmeasured = comparisons(entry, Object.keys(rows), rows, { ...tiers, run: { ...tiers.run, cache_states: { repeat: tiers.run.cache_states.repeat } } });
  assert.equal(unmeasured.python.tiers.run.metrics.runtime_wall_seconds.cache_state, "edited");
  assert.equal(unmeasured.python.tiers.run.metrics.runtime_wall_seconds.status, "unmeasured");
});

test("web comparisons require artifact bytes and bind them to the AOT tier", () => {
  const entry = { name: "web-fixture", mode: "web", languages: ["jet", "node"] };
  const metrics = Object.fromEntries([
    ["runtime_first_stdout_seconds", 1],
    ["runtime_wall_seconds", 1],
    ["runtime_peak_rss_kb", 1],
    ["cold_build_seconds", 1],
    ["warm_build_seconds", 1],
    ["binary_bytes", 1],
    ["artifact_bytes", 1],
    ["wasm_bytes", 1],
    ["loc", 1],
    ["source_bytes", 1],
    ["tokens", 1],
    ["source_tokens", 1],
  ]);
  const rows = {
    jet: { status: "ok", metrics },
    node: { status: "ok", metrics: Object.fromEntries(Object.entries(metrics).map(([metric, value]) => [metric, value * 2])) },
  };
  const runMetrics = { ...metrics, artifact_bytes: null, wasm_bytes: null };
  const tiers = {
    aot: { status: "ok", metrics },
    run: { status: "ok", metrics: runMetrics, cache_states: { edited: { status: "ok", metrics: runMetrics }, repeat: { status: "ok", metrics: runMetrics } } },
    dev: { status: "unavailable", metrics: {} },
  };
  const result = comparisons(entry, Object.keys(rows), rows, tiers);
  assert.equal(result.node.tiers.aot.metrics.artifact_bytes.status, "measured");
  assert.equal(result.node.tiers.run.metrics.artifact_bytes.status, "not_applicable");
  const missing = { ...rows.node, metrics: { ...rows.node.metrics, artifact_bytes: null } };
  const missingResult = comparisons(entry, ["jet", "node"], { ...rows, node: missing }, tiers);
  assert.equal(missingResult.node.tiers.aot.metrics.artifact_bytes.status, "unmeasured");
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
  assert.equal(record.peers.find((peer) => peer.language === "rust").verdict, "loss");
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
    "artifact_bytes",
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
    run: { status: "ok", verification: { status: "passed", kind: "byte_exact_stdout" }, metrics: rawMetrics("jet") },
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

test("the matrix encodes the peer floor and the reported machine ceiling, and every ceiling reference names its gauntlet cells", async () => {
  const matrix = JSON.parse(await fs.readFile(matrixPath, "utf8"));
  assert.deepEqual(strictPerformancePolicyIssues(matrix), []);
  const ceiling = matrix.strict_performance_policy.ceiling;
  assert.deepEqual(ceiling, JSON.parse(JSON.stringify(CEILING_POLICY)));
  assert.deepEqual(ceiling.stages.map((stage) => [stage.id, stage.role]), [["peer_floor", "initial"], ["machine_ceiling", "final"]]);
  assert.deepEqual(ceiling.stages[1].comparators, { meet: "<=1", miss: ">1" });
  assert.equal(ceiling.closes_cell, "peer_floor");
  assert.equal(ceiling.peer_win_closes_cell, true);
  assert.equal(ceiling.ceiling_gate, "reported_until_hill_climbing_opens");
  const references = JSON.parse(await fs.readFile(repoSourcePath(ceiling.references), "utf8"));
  assert.deepEqual(await ceilingReferenceIssues(matrix, references), []);
  // AGENTS.md carries every paragraph of the gate's policy text word for word.
  const agents = (await fs.readFile(path.join(repoDir, "AGENTS.md"), "utf8")).replace(/\s+/g, " ");
  const paragraphs = STRICT_PERFORMANCE_POLICY.text.split("\n\n");
  assert.equal(paragraphs.length, 4);
  for (const paragraph of paragraphs) {
    assert.ok(agents.includes(paragraph), `AGENTS.md Performance gate lacks: ${paragraph.slice(0, 60)}`);
  }
  const missing = await ceilingReferenceIssues(matrix, [{ name: "nope", gauntlet: ["no.such-cell"] }, { name: "sieve" }]);
  assert.ok(missing.includes("nope: machine-ceiling reference names unknown matrix cell no.such-cell"));
  assert.ok(missing.includes("nope: machine-ceiling reference has no ref.S"));
  assert.ok(missing.includes("sieve: machine-ceiling reference does not name its gauntlet cells"));
});

test("a cell passes on the peer floor; its ceiling ratio is reported only", () => {
  const fixture = scoreboardFixture();
  const tower = { status: "available", revision: 1, cards: new Map() };
  const reference = [{ name: "fixture-ref", gauntlet: ["fixture.cell"] }];
  const rows = (cycles, size, output = "same") => ({
    rows: [
      { cell: "fixture-ref", variant: "asm", cycles_x: 1, size_x: 1, output: "same" },
      { cell: "fixture-ref", variant: "jet-default", cycles_x: cycles, size_x: size, output },
      { cell: "fixture-ref", variant: "jet-release", cycles_x: cycles, size_x: size, output },
    ],
  });
  const judge = (ceiling) => buildScoreboard(fixture.matrix, [fixture.result], fixture.manifest, tower, ceiling);
  const noCeilingBlocker = (scoreboard) => !fixturePublication(fixture, scoreboard).blockers.some((blocker) => blocker.includes("machine-ceiling"));

  const uncovered = judge(undefined);
  const cell = uncovered.cells[0];
  assert.equal(cell.peer_verdict, "win");
  assert.equal(cell.ceiling.status, "uncovered");
  assert.equal(cell.ceiling_verdict, "uncovered");
  assert.equal(cell.verdict, "win");
  assert.deepEqual(Object.keys(cell.ceiling.metrics), ["runtime_wall_seconds", "runtime_first_stdout_seconds", "binary_bytes"]);
  assert.equal(cell.ceiling.metrics.runtime_wall_seconds.reason, "no hand-written machine-code reference names this cell");
  assert.deepEqual(uncovered.ceiling_coverage.uncovered, ["fixture.cell"]);
  assert.equal(uncovered.summary.ceiling_uncovered, 1);
  assert.ok(noCeilingBlocker(uncovered));

  const unmeasured = judge({ references: reference, measurements: null });
  assert.equal(unmeasured.cells[0].ceiling.status, "covered");
  assert.equal(unmeasured.cells[0].ceiling_verdict, "unmeasured");
  assert.equal(unmeasured.cells[0].verdict, "win");
  assert.ok(noCeilingBlocker(unmeasured));

  const miss = judge({ references: reference, measurements: rows(1.2, 0.9) });
  assert.equal(miss.cells[0].ceiling.metrics.runtime_wall_seconds.verdict, "miss");
  assert.deepEqual(miss.cells[0].ceiling.metrics.runtime_wall_seconds.rows.map((row) => row.ratio), [1.2, 1.2]);
  assert.equal(miss.cells[0].ceiling.metrics.binary_bytes.verdict, "meet");
  assert.equal(miss.cells[0].verdict, "win");
  assert.equal(miss.summary.ceiling_miss, 1);
  assert.ok(noCeilingBlocker(miss));

  const wrong = judge({ references: reference, measurements: rows(0.5, 0.5, "DIFF (status 1)") });
  assert.equal(wrong.cells[0].ceiling_verdict, "wrong");
  assert.equal(wrong.cells[0].verdict, "win");

  const meet = judge({ references: reference, measurements: rows(1, 0.8) });
  assert.equal(meet.cells[0].ceiling_verdict, "meet");
  assert.equal(meet.cells[0].verdict, "win");
  assert.deepEqual(meet.ceiling_coverage.covered, [{ cell: "fixture.cell", references: ["fixture-ref"] }]);
  assert.ok(noCeilingBlocker(meet));

  // A peer loss fails the cell however well the ceiling is met.
  const lossFixture = scoreboardFixture({ lossMetrics: ["runtime_wall_seconds"] });
  const lost = buildScoreboard(lossFixture.matrix, [lossFixture.result], lossFixture.manifest, tower, { references: reference, measurements: rows(0.5, 0.5) });
  assert.equal(lost.cells[0].ceiling_verdict, "meet");
  assert.equal(lost.cells[0].verdict, "loss");
});

test("the peer floor fails a planted wall-time or peak-memory loss and names the cell, peer, metric and ratio", () => {
  const tower = { status: "available", revision: 1, cards: new Map() };
  for (const metric of ["runtime_wall_seconds", "runtime_peak_rss_kb"]) {
    // Jet/peer = 1 / 0.5 = 2.000 on the AOT tier, the only tier a compiled peer is judged on.
    const fixture = scoreboardFixture({ lossMetrics: [metric] });
    const scoreboard = buildScoreboard(fixture.matrix, [fixture.result], fixture.manifest, tower);
    assert.equal(scoreboard.cells[0].verdict, "loss");
    assert.equal(scoreboard.cells[0].metric_verdicts[metric], "loss");
    const publication = fixturePublication(fixture, scoreboard);
    assert.equal(publication.complete, false);
    assert.ok(publication.blockers.includes(`fixture.cell/rust/${metric}: metric loss (aot Jet/rust 2.000 is at or above 1.00)`), publication.blockers.join("\n"));
  }
  // Parity is a loss: Jet/peer = 1.00 is not below 1.00.
  const parity = scoreboardFixture();
  parity.result.rows.rust.metrics.runtime_peak_rss_kb = 1;
  parity.result.comparisons = comparisons(parity.result.entry, Object.keys(parity.result.rows), parity.result.rows, parity.result.jet_tiers);
  const tied = buildScoreboard(parity.matrix, [parity.result], parity.manifest, tower);
  assert.equal(tied.cells[0].metric_verdicts.runtime_peak_rss_kb, "loss");
  assert.ok(fixturePublication(parity, tied).blockers.some((blocker) => blocker.startsWith("fixture.cell/rust/runtime_peak_rss_kb: metric loss (aot Jet/rust 1.000 is at or above 1.00")));
});

test("a missing peak-memory reading for Jet or a peer is uncovered and fails the gate", () => {
  const tower = { status: "available", revision: 1, cards: new Map() };
  const noJet = scoreboardFixture({ missingMetrics: ["runtime_peak_rss_kb"] });
  const noJetBoard = buildScoreboard(noJet.matrix, [noJet.result], noJet.manifest, tower);
  assert.equal(noJetBoard.cells[0].verdict, "unmeasured");
  assert.ok(fixturePublication(noJet, noJetBoard).blockers.includes("fixture.cell/rust/runtime_peak_rss_kb: metric unmeasured (uncovered: no aot runtime_peak_rss_kb reading for jet and rust)"));

  const noPeer = scoreboardFixture();
  noPeer.result.rows.rust.metrics.runtime_peak_rss_kb = null;
  noPeer.result.comparisons = comparisons(noPeer.result.entry, Object.keys(noPeer.result.rows), noPeer.result.rows, noPeer.result.jet_tiers);
  const noPeerBoard = buildScoreboard(noPeer.matrix, [noPeer.result], noPeer.manifest, tower);
  assert.equal(noPeerBoard.cells[0].verdict, "unmeasured");
  const blockers = fixturePublication(noPeer, noPeerBoard).blockers;
  assert.ok(blockers.some((blocker) => blocker.startsWith("fixture.cell/rust/runtime_peak_rss_kb: metric unmeasured (uncovered: no aot runtime_peak_rss_kb reading for rust")), blockers.join("\n"));

  // A matched peer cannot declare one peer-floor metric not applicable.
  const optOut = scoreboardFixture();
  optOut.result.entry.metric_applicability = { rust: { runtime_peak_rss_kb: { status: "not_applicable", basis: "no_process", reason: "r", evidence: "e" } } };
  assert.equal(metricApplicability(optOut.result.entry, "rust", "runtime_peak_rss_kb").status, "invalid");
  const optOutBoard = buildScoreboard(optOut.matrix, [optOut.result], optOut.manifest, tower);
  assert.equal(optOutBoard.cells[0].metric_verdicts.runtime_peak_rss_kb, "unmeasured");
  assert.equal(optOutBoard.cells[0].verdict, "unmeasured");
});

test("web-app results omit undeclared Jet tiers", () => {
  const fixture = scoreboardFixture();
  fixture.result.entry.mode = "web-app";
  for (const row of Object.values(fixture.result.rows)) row.metrics.wasm_bytes = 1;
  fixture.result.jet_tiers.aot.metrics.wasm_bytes = 1;
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
  assert.deepEqual(manifest.report_contract.ratio_verdicts, { win: "<1", loss: ">=1" });
  assert.equal(manifest.report_contract.loss_owner_required_for, "any_comparable_metric_loss");
  assert.deepEqual(manifest.report_contract.peer_measurement, {
    ratio_tiers: ["aot", "run"],
    trace_only_tiers: [],
    sample_binding: "immutable_peer_row_reused_per_declared_jet_tier",
    run_tier_peers: ["lua", "luajit", "python", "node"],
    compiled_peer_tiers: ["aot"],
    run_cache_states: { gated: ["edited", "repeat"], reported: ["fresh_install"] },
    run_reference_margin_seconds: 0.0005,
  });
  assert.deepEqual(manifest.report_contract.metric_applicability, {
    default: "required",
    not_applicable: "explicit_structural_reason",
    missing: "unmeasured_and_publication_blocked",
  });
  assert.deepEqual(manifest.report_contract.aot_only_metrics, ["cold_build_seconds", "warm_build_seconds", "binary_bytes", "artifact_bytes", "wasm_bytes"]);
  assert.ok(manifest.report_contract.required_metrics_by_mode.web.includes("artifact_bytes"));
  assert.ok(manifest.report_contract.required_metrics_by_mode["web-app"].includes("artifact_bytes"));
  assert.deepEqual(manifest.performance_surface_pairs.pair_measurement, {
    inputs: { iterations: 100000, range: "0..<100000", expected_sum: 4999950000 },
    tier: "native",
    target: "jet build --optimize=full",
    artifact_binding: "one_artifact_sha256",
    environment_binding: "one_host_toolchain_affinity_identity",
    sampling: "balanced_seeded_fisher_yates; one build per arm, fresh process and fresh empty directory per sample",
    output_validation: "exact_utf8_stdout_bytes",
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
    compile_throughput: "gauntlet-axis-compile-throughput-v1",
    build_system: "gauntlet-axis-build-system-v1",
  });
  assert.equal(manifest.report_contract.axis_publication, "required_axes_complete_and_unblocked");
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
  assert.deepEqual(liveReload.runners.map((runner) => runner.id).sort(), ["bun", "entr+cc", "jet-dev", "nodemon", "vite"]);
  for (const runner of liveReload.runners) {
    assert.ok(runner.files.some((file) => file.target === runner.edit_file), `${runner.id}: edit file is not staged`);
    for (const file of runner.files) assert.equal(await exists(repoSourcePath(file.source)), true, `${runner.id}: missing ${file.source}`);
  }

  const memorySafety = manifest.axes.memory_safety_fuzz;
  assert.deepEqual(memorySafety.runners.map((runner) => runner.id).sort(), ["c", "jet-default", "rust", "zig"]);
  for (const runner of memorySafety.runners) {
    for (const file of runner.files) assert.equal(await exists(repoSourcePath(file.source)), true, `${runner.id}: missing ${file.source}`);
  }

  // The compile-throughput cell (#4529): its contract, its harness, the
  // bench300k pin shared with Tools/perf/throughput/bench.json, and the
  // phase budgets (Tools/perf/scaling/phase-budgets.json).
  const throughput = manifest.axes.compile_throughput;
  assert.deepEqual(compileThroughputContractIssues(throughput), []);
  assert.equal(await exists(repoSourcePath(throughput.harness)), true);
  const pins = JSON.parse(await fs.readFile(repoSourcePath("Tools/perf/throughput/bench.json"), "utf8"));
  const bench300k = throughput.programs.find((program) => program.id === "bench300k");
  assert.equal(bench300k.source_sha256, pins.programs.bench300k.source_sha256);
  assert.equal(bench300k.expected_output, pins.programs.bench300k.expected_output);
  assert.equal(bench300k.generator, `node Tools/perf/scaling/generate.mjs mixed --code-lines ${pins.programs.bench300k.code_lines} --seed ${pins.programs.bench300k.seed} {dir}`);
  assert.ok(pins.programs.bench300k.shape.code_lines >= bench300k.loc_contract.nonblank_noncomment_lines_min);
  assert.deepEqual(Object.keys(throughput.gates.memory).sort(), ["bench300k_peak_rss_bytes_max", "selfhost_peak_rss_bytes_max"]);
  const budgets = JSON.parse(await fs.readFile(repoSourcePath("Tools/perf/scaling/phase-budgets.json"), "utf8"));
  assert.ok(budgets.phase_budgets.every((row) => row.owners.length > 0));

  // The Nix peer: five paired cells, the stricter target on the three
  // everyday loops, and a generated project whose edit files exist.
  const buildSystem = manifest.axes.build_system;
  assert.deepEqual(buildSystemContractIssues(buildSystem), []);
  assert.equal(await exists(repoSourcePath(buildSystem.harness)), true);
  assert.deepEqual(buildSystem.cells.map((cell) => cell.id), BUILD_SYSTEM_CELLS);
  for (const cell of buildSystem.cells) {
    assert.deepEqual(cell.target, ["eval", "noop", "incremental"].includes(cell.id) ? { ratio_max: 0.25 } : { ratio_lt: 1.0 });
  }
  const project = generateBuildSystemProject();
  assert.deepEqual(buildSystem.project.edit_files, EDIT_FILES);
  for (const file of Object.values(EDIT_FILES)) assert.equal(typeof project[file], "string", `${file}: not generated`);
  assert.ok(project["jet/@build.jet"].includes("build: fn(b: Build)"));
  assert.ok(project["nix/flake.nix"].includes("stdenv.mkDerivation"));

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

  // Tool entries ("mode": "tool") pair a repository script with its Jet twin;
  // PortBars judges them (AGENTS.md, Tooling in other languages) and they never
  // join the matrix denominator (run.mjs loadEntries), so the manifest covers
  // every other entry.
  const directories = [];
  for (const item of await fs.readdir(entriesDir, { withFileTypes: true })) {
    if (!item.isDirectory()) continue;
    const entry = JSON.parse(await fs.readFile(path.join(entriesDir, item.name, "entry.json"), "utf8"));
    if (entry.mode !== "tool") directories.push(item.name);
  }
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
