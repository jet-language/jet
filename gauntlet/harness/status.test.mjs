import { test } from "node:test";
import assert from "node:assert/strict";
import { projectStatus } from "./status.mjs";

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
