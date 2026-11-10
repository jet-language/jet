import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { evaluateGate, evaluatePerformanceSurfacePairs } from "./integrated-gate.mjs";
import { projectStatus } from "./status.mjs";
import { EXPECTED_OUTPUT, METRIC, TARGET, TIER, TRIALS, WARMUPS, measurePair } from "./performance-surface.mjs";

const NOW = "2026-09-20T12:00:00.000Z";
const NOW_MS = Date.parse(NOW);
const CELL = "fixture.cell";
const EVIDENCE_FILE = "Tools/gauntlet/harness/status.mjs";
const COMMIT = "a".repeat(40);
const TOOLCHAIN_SHA256 = "b".repeat(64);
const MATRIX_SHA256 = "c".repeat(64);
const MANIFEST_SHA256 = "d".repeat(64);
const STRICT_POLICY = { win: "<1", loss: ">=1" };
const TIERS = ["aot", "run"];
const PRIMARY_METRIC = {
  batch: "runtime_wall_seconds",
  service: "service_latency_ms_p50",
};
const TIER_POLICY = { batch: TIERS, service: TIERS };
const REQUIRED_METRICS = Object.fromEntries(Object.entries(PRIMARY_METRIC).map(([mode, metric]) => [mode, [metric]]));
const BACKENDS = ["aot_rust", "interpreter", "cranelift", "web"];

function performanceSurfaceSpec() {
  return {
    schema: "jet.gauntlet.performance-surface-pairs.v1",
    version: 1,
    decision: "D-PERF-SURFACE-WIN1=A",
    evidence: {
      file: EVIDENCE_FILE,
      path: "rows",
      tier: TIER,
      metric: METRIC,
      target: TARGET,
      output_oracle: EXPECTED_OUTPUT,
      input: "synthetic gate fixture input",
      setup: "synthetic gate fixture setup",
      warmups: WARMUPS,
      trials: TRIALS,
    },
    comparator: {
      direction: "lower_is_better",
      surface_over_plain: "<1",
      equality: "loss",
      missing: "failure",
      confidence: {
        method: "paired_index_bootstrap",
        confidence: 0.95,
        resamples: 10000,
        seed: 2977,
        quantile: "nearest_rank_one_based",
        lower_rank: 250,
        upper_rank: 9750,
      },
    },
    pairs: [{
      id: "fixture-pair",
      surface: "fixture surface",
      plain: "fixture plain",
      workload: "fixture workload",
      surface_program: "fixture-surface-program",
      plain_program: "fixture-plain-program",
      evidence_key: "fixture-pair",
      measured: { surface_wall_time_ms: 1, plain_wall_time_ms: 2, ratio: 0.5 },
    }],
  };
}

function manifest() {
  return {
    version: 1,
    corpus: { matrix_cell_count: 1 },
    report_contract: {
      id: "gauntlet-report-v1",
      scope: "full_matrix",
      required_jet_tiers: TIERS,
      tier_policy_by_mode: TIER_POLICY,
      primary_metric_by_mode: PRIMARY_METRIC,
      ratio_verdicts: STRICT_POLICY,
      aot_only_metrics: [],
    },
    integrated_gate: {
      schema: "jet.gauntlet.integrated-gate.v1",
      version: 1,
      inputs: {
        matrix: "Tools/gauntlet/matrix.json",
        status: "Tools/gauntlet/status.json",
        conformance_manifest: ".jet/hardening-manifest.json",
        tier_census: "Docs/audits/tier-census.json",
        compiler_speed: "Tools/perf/baseline.json",
        entries_dir: "Tools/gauntlet/entries",
      },
      freshness: { max_age_seconds: 86400, future_skew_seconds: 300 },
      required_metrics_by_mode: REQUIRED_METRICS,
      tier_census: { backends: BACKENDS },
      compiler_speed: { schema: "fixture.compiler-speed", version: 1, required_fields: ["toolchain_sha256"] },
      identity: { expected_commit: COMMIT },
    },
    performance_surface_pairs: performanceSurfaceSpec(),
    entries: [{ name: "fixture" }],
  };
}

function report(mode, peer = "python", jet = 1, peerValue = 2) {
  const metric = PRIMARY_METRIC[mode];
  const verificationKind = mode === "service" ? "service_probe_sequence" : "byte_exact_stdout";
  const metricValue = {
    status: "measured",
    jet,
    peer: peerValue,
    ratio: jet / peerValue,
    verdict: jet / peerValue < 1 ? "win" : "loss",
    measured_iso: NOW,
    source_file: EVIDENCE_FILE,
  };
  return {
    contract: "gauntlet-report-v1",
    generated: NOW,
    run_id: "synthetic-output-verification-fixture",
    options: { scope: "full" },
    scoreboard: {
      primary_metric_by_mode: PRIMARY_METRIC,
      verdict_policy: STRICT_POLICY,
      cells: [{ id: CELL, domain: "fixture", kind: mode, verdict: "win" }],
    },
    reproducibility: { tier_policy_by_mode: TIER_POLICY },
    provenance: {
      matrix_sha256: MATRIX_SHA256,
      measurement_manifest_sha256: MANIFEST_SHA256,
      jet_commit: COMMIT,
      toolchain_sha256: TOOLCHAIN_SHA256,
    },
    entries: [{
      entry: { name: "fixture", mode, cells: [CELL], languages: [peer] },
      comparisons: {
        [peer]: {
          primary_metric: metric,
          tiers: Object.fromEntries(TIERS.map((tier) => [tier, { metrics: { [metric]: metricValue } }])),
        },
      },
      jet_tiers: Object.fromEntries(TIERS.map((tier) => [tier, {
        status: "ok",
        verification: { status: "passed", kind: verificationKind },
      }])),
    }],
  };
}

function census() {
  const statusCounts = Object.fromEntries(["tir", "core_call"].map((surface) => [
    surface,
    Object.fromEntries(BACKENDS.map((backend) => [backend, { covered: 1, refused: 0, absent: 0, dynamic: 0 }])),
  ]));
  return {
    schema_version: 1,
    counts: { tir_variants: 1, core_records: 1 },
    tir: [{}],
    core_calls: [{}],
    timing_status: "complete",
    status_counts: statusCounts,
    dynamic_probe_cells: { total: 0 },
  };
}

function compilerSpeed() {
  const shared = {
    program: "fixture-program",
    state: "cold",
    metric: "runtime",
    workload_sha256: "e".repeat(64),
    source_sha256: "f".repeat(64),
    expected_sha256: "1".repeat(64),
    manifest_sha256: "2".repeat(64),
    toolchain_sha256: TOOLCHAIN_SHA256,
  };
  return {
    schema: "fixture.compiler-speed",
    version: 1,
    toolchain_sha256: TOOLCHAIN_SHA256,
    jet_commit: COMMIT,
    runs: [{ program: "fixture-program", state: "cold", stage: "compile", latency_ns: 1, memory_bytes: 1 }],
    peer_rows: [
      { ...shared, language: "jet", value: 1 },
      { ...shared, language: "python", value: 2 },
    ],
  };
}

function gateInputs({ mode = "batch", verificationOverrides = {}, peer = "python", jet = 1, peerValue = 2 } = {}) {
  const status = projectStatus(report(mode, peer, jet, peerValue), EVIDENCE_FILE);
  status.publication = { scope: "full_matrix", status: "complete", complete: true, blockers: [] };
  for (const [tier, verification] of Object.entries(verificationOverrides)) {
    const tierValue = status.cells[0].peers[0].metrics[PRIMARY_METRIC[mode]].tiers[tier];
    if (verification === undefined) delete tierValue.verification;
    else tierValue.verification = verification;
  }

  const paths = {
    manifest: "Tools/gauntlet/measurement-manifest.json",
    matrix: "Tools/gauntlet/matrix.json",
    status: "Tools/gauntlet/status.json",
    conformance_manifest: ".jet/hardening-manifest.json",
    tier_census: "Docs/audits/tier-census.json",
    compiler_speed: "Tools/perf/baseline.json",
    entries_dir: "Tools/gauntlet/entries",
    performance_surface_evidence: EVIDENCE_FILE,
  };
  const stat = { mtimeMs: NOW_MS };
  return {
    manifest: manifest(),
    manifestResult: { text: null },
    paths,
    matrix: { version: 1, cells: [{ id: CELL }] },
    status,
    entries: new Map([["fixture", {
      path: "Tools/gauntlet/entries/fixture/entry.json",
      value: { mode, languages: [peer], cells: [CELL] },
    }]]),
    conformanceManifest: {
      schema: "jet.hardening.surface.v1",
      denominator: { rows: 2, by_status: { covered: 1, excluded: 1 } },
      rows: [
        { stable_id: "module:core.fixture.covered", status: "covered" },
        { stable_id: "module:core.fixture.excluded", status: "excluded" },
      ],
    },
    tierCensus: census(),
    compilerSpeed: compilerSpeed(),
    performanceSurfaceEvidence: {
      generated: NOW,
      rows: [{ id: "fixture-pair", status: "passed", surface_wall_time_ms: 1, plain_wall_time_ms: 2, ratio: 0.5 }],
    },
    files: {
      status: { stat },
      matrix: { text: null },
      conformanceManifest: { stat },
      performanceSurfaceEvidence: { stat },
    },
    loadErrors: [],
  };
}

function gate(inputs) {
  return evaluateGate(inputs, { now: NOW, commit: COMMIT });
}

function tierResult(report, tier) {
  return report.checks.gauntlet.cells[0].metrics[0].peers[0].tiers.find((item) => item.tier === tier);
}

test("accepts producer verification projected into byte-exact and service metric tiers", () => {
  // These fixed ratios are plumbing fixtures only, not benchmark evidence.
  for (const mode of ["batch", "service"]) {
    const result = gate(gateInputs({ mode }));
    assert.equal(result.findings.some((finding) => finding.kind === "wrong-output"), false, `${mode} verification should be accepted`);
    assert.equal(tierResult(result, "aot").verdict, "win");
    assert.equal(tierResult(result, "run").verdict, "win");
  }
});

test("Rust is judged like every peer: Jet/Rust 1.00 and 1.05 are losses", () => {
  for (const jet of [100, 105]) {
    const result = gate(gateInputs({ peer: "rust", jet, peerValue: 100 }));
    assert.equal(tierResult(result, "aot").verdict, "loss", `Jet/Rust ${jet / 100} must lose`);
    assert.equal(tierResult(result, "run").verdict, "loss", `Jet/Rust ${jet / 100} must lose`);
    assert.ok(result.findings.some((finding) => finding.kind === "loss" && finding.peer === "rust" && finding.tier === "aot"));
    assert.equal(result.checks.gauntlet.cells[0].verdict, "loss");
  }
});

test("missing output verification fails an otherwise-winning tier closed", () => {
  const result = gate(gateInputs({ verificationOverrides: { aot: undefined } }));
  assert.equal(result.verdict, "fail");
  assert.ok(result.findings.some((finding) => finding.kind === "wrong-output" && finding.cell === CELL && finding.tier === "aot"));
  assert.equal(tierResult(result, "aot").verdict, "unmeasured");
  assert.equal(result.checks.gauntlet.cells[0].verdict, "unmeasured");
});

test("rejects malformed, failed, and mode-mismatched output verification", () => {
  const invalidProofs = [
    ["malformed", true],
    ["failed", { status: "failed", kind: "byte_exact_stdout" }],
    ["wrong kind", { status: "passed", kind: "service_probe_sequence" }],
  ];
  for (const [label, verification] of invalidProofs) {
    const result = gate(gateInputs({ verificationOverrides: { aot: verification } }));
    assert.ok(result.findings.some((finding) => finding.kind === "wrong-output" && finding.cell === CELL && finding.tier === "aot"), `${label} proof must fail as wrong-output`);
    assert.equal(tierResult(result, "aot").verdict, "unmeasured", `${label} proof must not retain a ratio win`);
  }
});

test("the conformance check reads the Tools/hardening manifest and fails on an uncovered or miscounted row", () => {
  assert.equal(gate(gateInputs()).checks.conformance.status, "pass");
  const uncovered = gateInputs();
  uncovered.conformanceManifest.rows.push({ stable_id: "module:core.fixture.missing", status: "missing" });
  uncovered.conformanceManifest.denominator = { rows: 3, by_status: { covered: 1, excluded: 1, missing: 1 } };
  const failed = gate(uncovered);
  assert.equal(failed.checks.conformance.status, "fail");
  assert.ok(failed.findings.some((finding) => finding.kind === "conformance" && finding.cause === "conformance manifest reports 1 uncovered row(s)"));
  const miscounted = gateInputs();
  miscounted.conformanceManifest.denominator.rows = 3;
  assert.ok(gate(miscounted).findings.some((finding) => finding.kind === "summary-mismatch"));
  const foreign = gateInputs();
  foreign.conformanceManifest.schema = "core-conformance";
  assert.equal(gate(foreign).checks.conformance.status, "fail");
});

// A strict receipt for one manifest pair, measured through the real harness
// with a synthetic arm runner (surface 10-12 ms, plain 20-22 ms).
function pairReceipt(spec, mutate = (sample) => sample) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "integrated-gate-pair-"));
  try {
    const artifact = path.join(dir, "jet");
    writeFileSync(artifact, "fixture");
    const identity = {
      artifact: { path: artifact, sha256: createHash("sha256").update("fixture").digest("hex") },
      revision: COMMIT,
      toolchain: { rustc: "rustc 1.0.0", node: "v22.0.0" },
      host: { hostname: "fixture", cpu_affinity: "0" },
    };
    let calls = 0;
    const runArm = ({ source, label, context }) => {
      calls += 1;
      const stdout = spec.expected_output;
      return mutate({
        label,
        source_file: "main.jet",
        source_sha256: createHash("sha256").update(source).digest("hex"),
        pair_context_sha256: context.context_sha256,
        artifact_sha256: context.artifact_sha256,
        environment_sha256: context.environment_sha256,
        tier: context.tier,
        target: context.target,
        input_range: context.inputs.range,
        output_oracle: context.output_validation.oracle,
        wall_time_ms: (source === spec.surface_source ? 10 : 20) + (calls % 3),
        status: 0,
        exit_code: 0,
        signal: null,
        timed_out: false,
        error_code: null,
        stdout,
        stderr: "",
        stdout_bytes_base64: Buffer.from(stdout, "utf8").toString("base64"),
        stderr_bytes_base64: "",
        output_verified: true,
        output_matches: true,
        contaminated: false,
        directory_entries: [],
      });
    };
    const buildArm = ({ source, label }) => ({ root: null, executable: path.join(dir, `${label}-main`), executable_sha256: "b".repeat(64), source_sha256: createHash("sha256").update(source).digest("hex"), status: 0, signal: null, error_code: null, stderr: "" });
    return measurePair({ pair: spec, identity, artifactPath: artifact, scratchRoot: dir, buildArm, runArm }).receipt;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a pair with its own evidence_file is judged strictly from that receipt", () => {
  const manifestPairs = JSON.parse(readFileSync(new URL("../measurement-manifest.json", import.meta.url), "utf8")).performance_surface_pairs.pairs;
  const row = manifestPairs.find((pair) => pair.id === "list_fill_vs_index_loop");
  const receipt = pairReceipt(row);
  const measured = receipt.pairs[0];
  const spec = performanceSurfaceSpec();
  spec.pairs = [{ ...row, measured: { surface_wall_time_ms: measured.surface_wall_time_ms, plain_wall_time_ms: measured.plain_wall_time_ms, ratio: measured.ratio }, verdict: "win" }];
  const own = (value) => new Map([[row.id, { receipt: value, file: row.evidence_file }]]);
  const passed = evaluatePerformanceSurfacePairs(spec, null, { pairEvidence: own(receipt) });
  assert.deepEqual(passed.findings, []);
  assert.equal(passed.pairs[0].verdict, "win");
  assert.equal(passed.pairs[0].evidence_file, row.evidence_file);
  const missing = evaluatePerformanceSurfacePairs(spec, null, {});
  assert.ok(missing.findings.some((finding) => finding.kind === "missing-evidence" && finding.evidence_file === row.evidence_file));
  const forged = pairReceipt(row, (sample) => ({ ...sample, source_sha256: "e".repeat(64) }));
  const failed = evaluatePerformanceSurfacePairs(spec, null, { pairEvidence: own(forged) });
  assert.ok(failed.findings.some((finding) => finding.kind === "identity-mismatch" && finding.evidence_file === row.evidence_file));
  const shared = performanceSurfaceSpec();
  shared.pairs = [{ ...spec.pairs[0], evidence_file: EVIDENCE_FILE }];
  assert.ok(evaluatePerformanceSurfacePairs(shared, null, {}).findings.some((finding) => finding.kind === "malformed-input" && /evidence_file must name its own receipt/.test(finding.cause)));
});
