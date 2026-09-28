import { test } from "node:test";
import assert from "node:assert/strict";
import { evaluateGate } from "./integrated-gate.mjs";
import { projectStatus } from "./status.mjs";
import { EXPECTED_OUTPUT, METRIC, TARGET, TRIALS, WARMUPS } from "./performance-surface.mjs";

const NOW = "2026-09-20T12:00:00.000Z";
const NOW_MS = Date.parse(NOW);
const CELL = "fixture.cell";
const EVIDENCE_FILE = "Tools/gauntlet/harness/status.mjs";
const COMMIT = "a".repeat(40);
const TOOLCHAIN_SHA256 = "b".repeat(64);
const MATRIX_SHA256 = "c".repeat(64);
const MANIFEST_SHA256 = "d".repeat(64);
const STRICT_POLICY = {
  rust: { win: "<1", parity: "<=1.05", loss: ">1.05" },
  non_rust: { win: "<1", parity: null, loss: ">=1" },
};
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
      tier: "interpreter",
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
        conformance_inventory: ".jet/core-conformance-inventory.json",
        conformance_summary: ".jet/core-conformance-check.txt",
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

function report(mode) {
  const metric = PRIMARY_METRIC[mode];
  const verificationKind = mode === "service" ? "service_probe_sequence" : "byte_exact_stdout";
  const metricValue = {
    status: "measured",
    jet: 1,
    peer: 2,
    ratio: 0.5,
    verdict: "win",
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
      entry: { name: "fixture", mode, cells: [CELL], languages: ["python"] },
      comparisons: {
        python: {
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

function gateInputs({ mode = "batch", verificationOverrides = {} } = {}) {
  const status = projectStatus(report(mode), EVIDENCE_FILE);
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
    conformance_inventory: ".jet/core-conformance-inventory.json",
    conformance_summary: ".jet/core-conformance-check.txt",
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
      value: { mode, languages: ["python"], cells: [CELL] },
    }]]),
    conformanceInventory: ["fixture.public_function"],
    conformanceSummary: "core conformance denominator: 1 public function(s); 1 program(s); 0 carve-out(s); 0 uncovered row(s)",
    tierCensus: census(),
    compilerSpeed: compilerSpeed(),
    performanceSurfaceEvidence: {
      generated: NOW,
      rows: [{ id: "fixture-pair", status: "passed", surface_wall_time_ms: 1, plain_wall_time_ms: 2, ratio: 0.5 }],
    },
    files: {
      status: { stat },
      matrix: { text: null },
      conformanceInventory: { stat },
      conformanceSummary: { stat },
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
