import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeStatus, projectStatus } from "./status.mjs";

function fixture(scope = "partial_entry") {
  return {
    contract: "gauntlet-report-v1",
    generated: "2026-09-01T12:00:00.000Z",
    run_id: "run-fixture",
    options: { scope },
    scoreboard: {
      primary_metric_by_mode: { batch: "runtime_wall_seconds" },
      verdict_policy: { rust: { win: "<1", parity: "<=1.05", loss: ">1.05" } },
      summary: {
        cells: 4,
        win: 1,
        parity: 1,
        loss: 1,
        unmeasured: 1,
        unmeasured_allowed: 0,
        unmeasured_required: 1,
        metric_win: 1,
        metric_parity: 1,
        metric_loss: 1,
        metric_unmeasured: 1,
        metric_not_applicable: 0,
      },
      cells: [
        {
          id: "z.win",
          domain: "z",
          kind: "batch",
          verdict: "win",
          entries: [{
            entry: "winner",
            mode: "batch",
            primary_metric: "runtime_wall_seconds",
            verdict: "win",
            peers: [{
              language: "python",
              verdict: "win",
              metric_comparisons: {
                runtime_wall_seconds: {
                  tiers: { aot: { status: "measured", jet: 1, peer: 2, ratio: 0.5, verdict: "win" } },
                },
              },
            }],
            metric_failures: [],
            loss_owners: [],
          }],
        },
        {
          id: "a.loss",
          domain: "a",
          kind: "batch",
          verdict: "loss",
          entries: [{
            entry: "loser",
            mode: "batch",
            primary_metric: "runtime_wall_seconds",
            verdict: "loss",
            peers: [{
              language: "rust",
              verdict: "loss",
              metric_comparisons: {
                runtime_wall_seconds: {
                  tiers: {
                    aot: { status: "measured", jet: 2, peer: 1, ratio: 7, verdict: "loss" },
                    run: { status: "unmeasured", verdict: null },
                  },
                },
              },
            }],
            metric_failures: [{ metric: "runtime_wall_seconds", verdict: "loss" }],
            loss_owners: [{ entry: "loser", peer: "rust", status: "live" }],
          }],
        },
        { id: "m.missing", domain: "m", kind: "batch", verdict: "unmeasured", entries: [] },
        {
          id: "p.parity",
          domain: "p",
          kind: "batch",
          verdict: "parity",
          entries: [{
            entry: "parity",
            mode: "batch",
            primary_metric: "runtime_wall_seconds",
            verdict: "parity",
            peers: [{ language: "rust", verdict: "parity", tier_verdicts: { aot: "parity" }, metric_comparisons: {} }],
            metric_failures: [],
            loss_owners: [],
          }],
        },
      ],
    },
    axes: {},
    publication: { scope, status: "complete", complete: true, blockers: [] },
    provenance: {},
    reproducibility: { tier_policy_by_mode: { batch: ["aot", "run"] } },
  };
}

test("projects counts, deterministic verdict ordering, and null measurements", () => {
  const status = projectStatus(fixture(), "gauntlet/results/fixture.json");
  assert.deepEqual(status.summary, {
    cells: 4,
    win: 1,
    parity: 1,
    loss: 1,
    unmeasured: 1,
    unmeasured_allowed: 0,
    unmeasured_required: 1,
    metric_win: 1,
    metric_parity: 1,
    metric_loss: 1,
    metric_unmeasured: 1,
    metric_not_applicable: 0,
  });
  assert.deepEqual(status.cells.map((cell) => cell.id), ["a.loss", "m.missing", "p.parity", "z.win"]);
  const loss = status.cells[0];
  assert.equal(loss.peers[0].peer, "rust");
  assert.equal(loss.peers[0].tiers.aot.ratio, 7, "projection must use the report ratio, not divide values");
  assert.equal(loss.peers[0].tiers.run.jet, null);
  assert.equal(loss.peers[0].tiers.run.peer, null);
  assert.equal(loss.peers[0].tiers.run.ratio, null);
  assert.equal(status.source.report_path, "gauntlet/results/fixture.json");
});

test("partial scope can never publish as complete", () => {
  const status = projectStatus(fixture("partial_entry"));
  assert.equal(status.source.scope, "partial_entry");
  assert.equal(status.publication.complete, false);
  assert.equal(status.publication.status, "incomplete");
  assert.match(status.publication.blockers.join("\n"), /run scope is partial/);
});

test("projects live reload comparison rows and axis publication", () => {
  const report = fixture("axis_live_reload");
  report.axes = {
    live_reload: {
      status: "complete",
      schema: "gauntlet-axis-live-reload-v1",
      metric: "reload_latency_ms",
      metrics: {
        "jet-dev": { cold_reload_latency_ms: 100, warm_reload_latency_ms: 110 },
        vite: { cold_reload_latency_ms: 90, warm_reload_latency_ms: 105 },
      },
      comparisons: {
        vite: {
          verdict: "parity",
          cold: { status: "measured", jet: 100, peer: 90, ratio: 1.111, verdict: "loss" },
          warm: { status: "measured", jet: 110, peer: 105, ratio: 1.047, verdict: "parity" },
        },
      },
      verdicts: { vite: "parity" },
      publication: { status: "ready", blockers: [] },
    },
  };
  const status = projectStatus(report);
  assert.equal(status.publication.complete, true);
  assert.deepEqual(status.axes.live_reload.metrics.vite, {
    cold_reload_latency_ms: 90,
    warm_reload_latency_ms: 105,
  });
  assert.deepEqual(status.axes.live_reload.comparisons.vite, {
    verdict: "parity",
    cold: { status: "measured", jet: 100, peer: 90, ratio: 1.111, verdict: "loss" },
    warm: { status: "measured", jet: 110, peer: 105, ratio: 1.047, verdict: "parity" },
  });
  assert.deepEqual(status.axes.live_reload.verdicts, { vite: "parity" });
});

function mergeReport(date, runId, cells) {
  return {
    contract: "gauntlet-report-v1",
    generated: `${date}T12:00:00.000Z`,
    run_id: runId,
    scoreboard: {
      primary_metric_by_mode: { batch: "runtime_wall_seconds" },
      cells,
    },
    reproducibility: { tier_policy_by_mode: { batch: ["aot", "run"] } },
  };
}

function measuredTier(ratio, verdict) {
  return { status: "measured", jet: ratio, peer: 1, ratio, verdict };
}

function measuredCell(id, ratioByPeer) {
  return {
    id,
    domain: "text",
    kind: "batch",
    entries: [{
      entry: id,
      mode: "batch",
      primary_metric: "runtime_wall_seconds",
      peers: Object.entries(ratioByPeer).map(([language, ratios]) => ({
        language,
        required_tiers: ["aot", "run"],
        metric_comparisons: {
          runtime_wall_seconds: {
            tiers: {
              aot: measuredTier(ratios.aot.ratio, ratios.aot.verdict),
              run: measuredTier(ratios.run.ratio, ratios.run.verdict),
            },
          },
        },
      })),
    }],
  };
}

test("merges partial reports by cell, tier, and metric without erasing measurements", () => {
  const oldCell = measuredCell("text.kernel", {
    rust: { aot: { ratio: 1.03, verdict: "parity" }, run: { ratio: 0.99, verdict: "win" } },
    python: { aot: { ratio: 0.8, verdict: "win" }, run: { ratio: 1, verdict: "loss" } },
  });
  const newCell = measuredCell("text.regex", {
    rust: { aot: { ratio: 0.9, verdict: "win" }, run: { ratio: 0.91, verdict: "win" } },
  });
  const newerPartial = {
    id: "text.kernel",
    domain: "text",
    kind: "batch",
    entries: [],
  };
  const merged = mergeStatus([
    { report: mergeReport("2026-09-01", "run-old", [oldCell]), reportPath: "gauntlet/results/2026-09-01.json" },
    { report: mergeReport("2026-09-02", "run-new", [newerPartial, newCell]), reportPath: "gauntlet/results/2026-09-02.json" },
  ]);
  const old = merged.cells.find((cell) => cell.id === "text.kernel");
  const fresh = merged.cells.find((cell) => cell.id === "text.regex");
  assert.equal(merged.summary.cells, 2);
  assert.equal(merged.summary.loss, 1);
  assert.equal(merged.summary.win, 1);
  assert.equal(old.measured_at, "2026-09-01");
  assert.equal(old.run_id, "run-old");
  assert.equal(old.peers.find((peer) => peer.peer === "rust").tiers.aot.ratio, 1.03);
  assert.equal(old.peers.find((peer) => peer.peer === "rust").tiers.aot.verdict, "parity");
  assert.equal(old.peers.find((peer) => peer.peer === "python").tiers.run.verdict, "loss");
  assert.equal(old.verdict, "loss");
  assert.equal(fresh.measured_at, "2026-09-02");
  assert.deepEqual(merged.source.run_ids, ["run-old", "run-new"]);
});

test("applies the Rust parity band and non-Rust strict win boundary", () => {
  const report = mergeReport("2026-09-03", "run-law", [measuredCell("law", {
    rust: { aot: { ratio: 1.05, verdict: null }, run: { ratio: 0.99, verdict: null } },
    python: { aot: { ratio: 0.99, verdict: null }, run: { ratio: 1, verdict: null } },
  })]);
  const status = projectStatus(report, "gauntlet/results/2026-09-03.json");
  const cell = status.cells[0];
  assert.equal(cell.peers.find((peer) => peer.peer === "rust").tiers.aot.verdict, null);
  assert.equal(cell.metric_verdicts.runtime_wall_seconds, "loss");
  const merged = mergeStatus([{ report, reportPath: "gauntlet/results/2026-09-03.json" }]);
  const rust = merged.cells[0].peers.find((peer) => peer.peer === "rust");
  const python = merged.cells[0].peers.find((peer) => peer.peer === "python");
  assert.equal(rust.metric_comparisons.runtime_wall_seconds.tiers.aot.verdict, null);
  assert.equal(rust.verdict, "parity");
  assert.equal(python.verdict, "loss");
});
