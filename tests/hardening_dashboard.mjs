import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { afterEach, describe, it } from "node:test";

import { buildDashboard } from "../scripts/agent/hardening-dashboard.mjs";
import { canonicalJson, sha256 } from "../scripts/agent/hardening-repro.mjs";
import { manifestContentDigest, validateManifest } from "../scripts/agent/hardening-manifest.mjs";
import { signReceipt } from "../scripts/agent/hardening-red-team.mjs";

const GIB = 1024 ** 3;
const roots = [];

function rawSha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function manifestFixture(status = "covered") {
  const stableId = "module:core.math.add";
  const projections = ["aot", "jet_run", "interpreter"].map((tier) => ({
    tier,
    route: `core.math.add.${tier}`,
    seam: null,
    evidence: ["fixture:1"],
  }));
  const row = {
    stable_id: stableId,
    kind: "module_call",
    owner: "core.math",
    member: "add",
    domain: "math",
    applicable_tiers: ["aot", "jet_run", "interpreter"],
    projections,
    dispatcher_arms: projections.map((projection) => projection.route),
    membership_sources: ["fixture"],
    membership_evidence: ["fixture:1"],
    seed: "fixture.jet",
    value_consuming: status === "covered" ? true : null,
    sink: status === "covered" ? { type_aware: true, operation: "print" } : null,
    status,
    exclusion: null,
  };
  const manifest = {
    schema: "jet.hardening.surface.v1",
    schema_version: 1,
    source_snapshot: { algorithm: "sha256", files: [], hash: sha256(canonicalJson([])) },
    denominator: {
      source_ids: { module_call: [stableId], receiver_method: [], field: [], nominal_type: [] },
      counts: { module_call: 1, receiver_method: 0, field: 0, nominal_type: 0, exclusions: 0 },
    },
    actual_routes: {
      aot: [{ stable_id: stableId, route: projections[0].route, seam: null, evidence: ["fixture:1"] }],
      jet_run: [{ stable_id: stableId, route: projections[1].route, seam: null, evidence: ["fixture:1"] }],
      interpreter: [{ stable_id: stableId, route: projections[2].route, seam: null, evidence: ["fixture:1"] }],
    },
    exclusions: [],
    rows: [row],
  };
  manifest.content_digest = manifestContentDigest(manifest);
  assert.equal(validateManifest(manifest).ok, true);
  return manifest;
}

function fixture({ rowStatus = "covered", towerCards = [], lowRow = false, missingLane = null, targetBytes = 1 } = {}) {
  const root = mkdtempSync("/tmp/jet-hardening-dashboard-");
  roots.push(root);
  const evidence = join(root, "evidence");
  const redTeam = join(evidence, "red-team");
  mkdirSync(join(root, ".jet"), { recursive: true });
  mkdirSync(redTeam, { recursive: true });
  mkdirSync(join(root, "target", "debug"), { recursive: true });
  const manifestPath = join(root, ".jet/hardening-manifest.json");
  writeFileSync(manifestPath, `${JSON.stringify(manifestFixture(rowStatus), null, 2)}\n`);
  const binaryPath = join(root, "target/debug/jet");
  writeFileSync(binaryPath, "fixture binary\n");
  const binarySha = rawSha256(readFileSync(binaryPath));
  const manifestSha = rawSha256(readFileSync(manifestPath));
  const commit = "commit-dashboard-fixture";
  const now = new Date("2026-08-30T12:00:00.000Z");
  const stableId = "module:core.math.add";
  for (let index = 0; index < 14; index += 1) {
    const date = new Date(now);
    date.setUTCDate(date.getUTCDate() - (13 - index));
    const iso = date.toISOString();
    writeFileSync(join(evidence, `cycle-${index + 1}.json`), JSON.stringify({
      run_id: `cycle-${index + 1}`,
      started: iso,
      finished: iso,
      status: "PASS",
      commit,
      binary_sha256: binarySha,
      registry_snapshot: { sha256: manifestSha },
      config_hash: "config-dashboard-fixture",
      oracle: { status: "PASS", valid_case_count: 1_000_000, last_seed: index + 1 },
      mutation: {
        status: "PASS",
        row_counts: { [stableId]: lowRow ? (index === 13 ? 99 : 0) : 100 },
        domain_counts: { math: 1 },
      },
    }));
  }
  const lanes = Array.from({ length: missingLane === null ? 8 : 7 }, (_, index) => ({
    lane_id: `lane-${index + 1}`,
    complete: true,
    valid_cases: [{ id: `case-${index + 1}` }],
    counts: { valid_cases: 2 },
    duplicates: [],
    false_positives: [],
    unique_findings: [],
  }));
  writeFileSync(join(redTeam, "session.json"), JSON.stringify({
    schema: "jet.hardening.red-team.session.v1",
    target: { commit, binary_sha256: binarySha },
    commit,
    binary_sha256: binarySha,
    registry_snapshot: { sha256: manifestSha },
    quota: { lanes: 8, waves: 4 },
  }));
  const receipt = signReceipt({
    receipt_kind: "fresh-context-red-team-verdict",
    status: "PASS",
    session: { commit, binary_sha256: binarySha, registry_sha256: manifestSha },
    quota: { lanes: 8, waves: 4 },
    lanes,
    findings: [],
    finding_duplicates: [],
    p0_count: 0,
    unique_finding_count: 0,
    cleanup: { active_agents: 0, active_processes: 0, scratch_paths: [], alternate_targets: [], unbounded_logs: false, complete: true },
  }, { signer_id: "fixture-signer", reviewer_id: "fixture-reviewer" });
  writeFileSync(join(redTeam, "receipt.json"), JSON.stringify(receipt));
  const towerPath = join(root, "tower-fixture.mjs");
  writeFileSync(towerPath, `process.stdout.write(${JSON.stringify(JSON.stringify(towerCards))});\n`);
  chmodSync(towerPath, 0o755);
  return {
    root,
    evidence,
    manifestPath,
    binaryPath,
    towerPath,
    commit,
    binarySha,
    resources: {
      target_bytes: targetBytes,
      cache_bytes: 1,
      interesting_bytes: 1,
      log_bytes: 1,
      memory_available_gib: 32,
      free_space_bytes: 32 * GIB,
    },
    now,
  };
}

function report(options = {}) {
  const fixtureState = fixture(options);
  const value = buildDashboard({
    root: fixtureState.root,
    evidenceRoot: fixtureState.evidence,
    manifestPath: fixtureState.manifestPath,
    binaryPath: fixtureState.binaryPath,
    towerCli: fixtureState.towerPath,
    target: { commit: fixtureState.commit, clean: true, binary_sha256: fixtureState.binarySha },
    resources: fixtureState.resources,
    now: fixtureState.now,
  });
  return { fixtureState, value };
}

afterEach(() => {
  while (roots.length) rmSync(roots.pop(), { recursive: true, force: true });
});

describe("hardening handoff dashboard", () => {
  it("reports READY only when every ratified gate shares one target", () => {
    const { value } = report();
    assert.equal(value.status, "READY");
    assert.equal(value.fuzz.clean_days, 14);
    assert.equal(value.fuzz.valid_cases, 14_000_000);
    assert.equal(value.fuzz.lowest_row, 1_400);
    assert.equal(value.red_team.quota.completed_lanes, 8);
    assert.equal(value.red_team.unique_p0, 0);
    assert.equal(value.tower.open_p0, 0);
    assert.equal(value.gates.target.ok, true);
    assert.equal(value.gates.resources.ok, true);
  });

  it("breaks the conformance gate for a missing row", () => {
    const { value } = report({ rowStatus: "missing" });
    assert.equal(value.status, "NOT READY");
    assert.equal(value.conformance.ok, false);
    assert.equal(value.conformance.totals.missing, 1);
  });

  it("breaks the clean fuzz gate below the per-row floor", () => {
    const { value } = report({ lowRow: true });
    assert.equal(value.status, "NOT READY");
    assert.equal(value.fuzz.ok, false);
    assert.equal(value.fuzz.lowest_row, 99);
  });

  it("breaks the red-team gate when a fresh lane is absent", () => {
    const { value } = report({ missingLane: 8 });
    assert.equal(value.status, "NOT READY");
    assert.equal(value.red_team.ok, false);
    assert.equal(value.red_team.quota.completed_lanes, 7);
  });

  it("reports exact open P0 refs from the Tower CLI", () => {
    const { value } = report({ towerCards: [{ num: 2340, priority: "P0", phase: "ready" }] });
    assert.equal(value.status, "NOT READY");
    assert.deepEqual(value.tower.refs, ["#2340"]);
    assert.equal(value.tower.open_p0, 1);
  });

  it("breaks the resource gate over the target cap", () => {
    const { value } = report({ targetBytes: 81 * GIB });
    assert.equal(value.status, "NOT READY");
    assert.equal(value.gates.resources.ok, false);
    assert.match(value.resources.violations.join(";"), /target over 80GiB/);
  });
});
