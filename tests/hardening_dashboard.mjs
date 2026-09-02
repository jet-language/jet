import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { afterEach, describe, it } from "node:test";

import { buildDashboard, normalizeVerifiedCycle } from "../scripts/agent/hardening-dashboard.mjs";
import { canonicalJson, sha256 } from "../scripts/agent/hardening-repro.mjs";
import {
  buildManifest,
  sourceSnapshot,
  validateManifest,
} from "../scripts/agent/hardening-manifest.mjs";
import {
  createSessionManifest,
  makeLaneReceipt,
  sessionManifestDigest,
  signReceipt,
} from "../scripts/agent/hardening-red-team.mjs";

const GIB = 1024 ** 3;
const roots = [];

function rawSha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function canonicalDigest(value) {
  return rawSha256(Buffer.from(canonicalJson(value), "utf8"));
}

function caseBundle({
  id,
  rowId,
  domain,
  layer,
  seedId,
  manifestHash,
  configHash,
  value,
}) {
  const bundle = {
    id,
    row_id: rowId,
    domain,
    layer,
    seed_id: seedId,
    expected_relation: `oracle:${domain}`,
    manifest_sha256: manifestHash,
    config_sha256: configHash,
    applicable_tiers: ["aot", "jet_run", "interpreter"],
    input: { id },
    expected_value: value,
    observations: ["aot", "jet_run", "interpreter"].map((tier) => ({
      tier,
      value,
      relation: canonicalJson(value),
      exit: 0,
      signal: null,
      timeout: false,
    })),
    validity: "valid",
    rejection_reason: null,
  };
  return { ...bundle, digest_sha256: canonicalDigest(bundle) };
}
function verifiedManifestFixture() {
  const root = process.cwd();
  const production = JSON.parse(readFileSync(join(root, ".jet/hardening-manifest.json"), "utf8"));
  const sourceRow = production.rows.find((row) => (
    row.status === "covered"
      && row.kind === "module_call"
      && row.applicable_tiers.length === 3
      && typeof row.seed === "string"
  ));
  assert.ok(sourceRow, "production manifest must provide a covered three-tier seed");
  const stableId = sourceRow.stable_id;
  const routes = Object.fromEntries(["aot", "jet_run", "interpreter"].map((tier) => [
    tier,
    sourceRow.projections
      .filter((projection) => projection.tier === tier)
      .map((projection) => ({
        stable_id: stableId,
        route: projection.route,
        seam: projection.seam,
        evidence: projection.evidence,
      })),
  ]));
  const surface = {
    moduleCalls: [stableId],
    receivers: [],
    fields: [],
    types: [],
    routes,
    seeds: new Map([[
      stableId,
      {
        path: sourceRow.seed,
        errors: [],
        sink: sourceRow.sink,
      },
    ]]),
    exclusions: new Map(),
    snapshot: sourceSnapshot(root),
    membershipSources: {
      module_call: sourceRow.membership_sources,
      receiver_method: [],
      field: [],
      nominal_type: [],
    },
    membershipEvidence: {
      module_call: { [stableId]: sourceRow.membership_evidence },
      receiver_method: {},
      field: {},
      nominal_type: {},
    },
  };
  const manifest = buildManifest({ root, surface });
  const expectedIds = JSON.parse(JSON.stringify(manifest.denominator));
  const validation = validateManifest(manifest, {
    expectedIds,
    expectedExclusions: surface.exclusions,
    root,
  });
  assert.equal(validation.ok, true, validation.errors.join("; "));
  return { manifest, row: sourceRow };
}

function verifiedManifestInfo(manifest) {
  return {
    path: ".jet/hardening-manifest.json",
    present: true,
    readable: true,
    hash: manifest.content_digest,
    content_digest: manifest.content_digest,
    source_snapshot_hash: manifest.source_snapshot.hash,
    manifest,
    stale: false,
    errors: [],
  };
}

function fixture({
  towerCards = [],
  lowRow = false,
  missingLane = null,
  targetBytes = 1,
  forgedCount = false,
  forgedRowCount = false,
  tamperBundle = false,
  tamperPacket = false,
  summaryOnly = false,
  staleConfig = false,
} = {}) {
  const root = process.cwd();
  const workRoot = mkdtempSync(join(root, ".tmp-hardening-dashboard-"));
  roots.push(workRoot);
  const evidence = join(workRoot, "evidence");
  const redTeam = join(evidence, "red-team");
  mkdirSync(redTeam, { recursive: true });
  mkdirSync(join(workRoot, "target", "debug"), { recursive: true });
  const manifestPath = join(root, ".jet/hardening-manifest.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const binaryPath = join(workRoot, "target/debug/jet");
  writeFileSync(binaryPath, "fixture binary\n");
  const binarySha = rawSha256(readFileSync(binaryPath));
  const manifestSha = rawSha256(readFileSync(manifestPath));
  const manifestSourceHash = manifest.source_snapshot.hash;
  const commit = "b".repeat(40);
  const now = new Date("2026-08-30T12:00:00.000Z");
  const stableId = "module:core.crypto.expert.aes256gcm_open";
  const sessionBase = createSessionManifest({
    root,
    session_id: "dashboard-fixture-session",
    created_at: now.toISOString(),
    commit,
    binary_sha256: `sha256:${binarySha}`,
    binary_path: binaryPath,
    registry_snapshot: {
      path: ".jet/hardening-manifest.json",
      sha256: `sha256:${manifestSha}`,
      source_snapshot_hash: manifestSourceHash,
    },
    public_surface_snapshot: {
      path: ".jet/hardening-manifest.json",
      sha256: `sha256:${manifestSha}`,
      source_snapshot_hash: manifestSourceHash,
    },
    rig_config: {
      seed: "dashboard-fixture",
      variants: "dashboard",
      proof_targets: ["fixture"],
      deterministic_shards: ["fixture"],
      oracle_batch_size: 4,
      oracle_max_cases: 4,
      oracle_timeout_ms: 5_000,
    },
  });
  const session = JSON.parse(JSON.stringify(sessionBase));
  session.config = { ...session.rig_config };
  session.config_sha256 = canonicalDigest(session.config);
  session.manifest_sha256 = sessionManifestDigest(session);
  writeFileSync(join(redTeam, "session.json"), `${JSON.stringify(session, null, 2)}\n`);
  const configHash = session.config_sha256;
  for (let index = 0; index < 14; index += 1) {
    const date = new Date(now);
    date.setUTCDate(date.getUTCDate() - (13 - index));
    const iso = date.toISOString();
    const oracleBundles = [
      caseBundle({
        id: `cycle-${index + 1}-oracle`,
        rowId: stableId,
        domain: "crypto",
      }),
    ];
    const mutationCount = lowRow ? (index === 13 ? 99 : 0) : 100;
    const mutationBundles = Array.from({ length: mutationCount }, (_, mutation) => caseBundle({
      id: `cycle-${index + 1}-mutation-${mutation + 1}`,
      rowId: stableId,
      domain: "crypto",
    }));
    if (tamperBundle && index === 13) mutationBundles[0].expected_value = { value: -1 };
    const reportedMutationCount = forgedRowCount && index === 13 ? 100 : mutationBundles.length;
    const cycle = {
      run_id: `cycle-${index + 1}`,
      started: iso,
      finished: iso,
      status: "PASS",
      commit,
      binary_sha256: binarySha,
      registry_snapshot: { sha256: manifestSha },
      config: session.config,
      config_sha256: staleConfig ? "1".repeat(64) : configHash,
      oracle: {
        status: "PASS",
        valid_case_count: forgedCount ? 1_000_000 : oracleBundles.length,
        ...(summaryOnly ? {} : { case_bundles: oracleBundles }),
      },
      mutation: {
        status: "PASS",
        valid_case_count: mutationBundles.length,
        ...(summaryOnly ? {} : { case_bundles: mutationBundles }),
        row_counts: { [stableId]: reportedMutationCount },
        domain_counts: { math: mutationBundles.length },
      },
    };
    cycle.content_digest = sha256(canonicalJson(cycle));
    writeFileSync(join(evidence, `cycle-${index + 1}.json`), JSON.stringify(cycle));
  }
  const lanes = Array.from({ length: missingLane === null ? 8 : 7 }, (_, index) => makeLaneReceipt(session, {
    lane_id: `lane-${index + 1}`,
    context_id: `dashboard-context-${index + 1}`,
    agent_id: `dashboard-agent-${index + 1}`,
  }));
  if (tamperPacket) lanes[0].packet_digest = `sha256:${"0".repeat(64)}`;
  const receipt = signReceipt({
    receipt_kind: "fresh-context-red-team-verdict",
    status: "PASS",
    session_id: session.session_id,
    manifest_sha256: session.manifest_sha256,
    session: {
      commit: session.target.commit,
      binary_sha256: session.target.binary_sha256,
      registry_sha256: session.registry_snapshot.sha256,
      public_surface_sha256: session.public_surface_snapshot.sha256,
    },
    quota: session.quota,
    execution_gate: session.execution_gate,
    lanes,
    lane_agents: lanes.map((lane) => ({ lane_id: lane.lane_id, agent_id: lane.agent_id, context_id: lane.context_id })),
    findings: [],
    finding_duplicates: [],
    replayed_findings: [],
    assimilation: [],
    stale_reasons: [],
    failure_reasons: [],
    p0_count: 0,
    unique_finding_count: 0,
    max_active_lanes: 2,
    started_at: now.toISOString(),
    finished_at: now.toISOString(),
    cleanup: {
      active_agents: 0,
      active_processes: 0,
      scratch_paths: [],
      alternate_targets: [],
      unbounded_logs: false,
      complete: true,
    },
    independent_discovery: {
      current_defect_cards_hidden_until: "all-eight-independent-receipts",
      revealed_after_discovery: lanes.length === 8,
    },
  }, { signer_id: "fixture-signer", reviewer_id: "fixture-reviewer" });
  writeFileSync(join(redTeam, "receipt.json"), JSON.stringify(receipt));
  const towerPath = join(workRoot, "tower-fixture.mjs");
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

function verifiedCycle({
  forgedCount = false,
  forgedRowCount = false,
  tamperBundle = false,
  summaryOnly = false,
  staleConfig = false,
  lowRow = false,
} = {}) {
  const { manifest, row } = verifiedManifestFixture();
  const manifestInfo = verifiedManifestInfo(manifest);
  const stableId = row.stable_id;
  const commit = "c".repeat(40);
  const binarySha = "d".repeat(64);
  const target = { commit, binary_sha256: binarySha };
  const config = {
    schema_version: 1,
    suite_concurrency: 2,
    cargo_build_jobs: 4,
    seed: "dashboard-fixture",
    variants: "dashboard",
    proof_targets: ["fixture"],
    deterministic_shards: ["fixture"],
    oracle_batch_size: 4,
    oracle_max_cases: 4,
    oracle_timeout_ms: 5_000,
  };
  const configHash = canonicalDigest(config);
  const session = { config, config_sha256: configHash };
  const bundleArgs = {
    rowId: stableId,
    domain: row.domain,
    manifestHash: manifest.content_digest,
    configHash,
  };
  const oracleBundles = [
    caseBundle({
      ...bundleArgs,
      id: "cycle-oracle",
      layer: "oracle",
      seedId: "oracle-seed",
      value: { value: 3 },
    }),
  ];
  const mutationCount = lowRow ? 99 : 100;
  const mutationBundles = Array.from({ length: mutationCount }, (_, index) => caseBundle({
    ...bundleArgs,
    id: `cycle-mutation-${index + 1}`,
    layer: "mutation",
    seedId: `mutation-seed-${index + 1}`,
    value: { value: index + 1 },
  }));
  if (tamperBundle) mutationBundles[0].expected_value = { value: -1 };
  const cycle = {
    run_id: "cycle-verified",
    started: "2026-08-30T12:00:00.000Z",
    finished: "2026-08-30T12:00:00.000Z",
    status: "PASS",
    commit,
    binary_sha256: binarySha,
    registry_snapshot: { sha256: manifest.content_digest },
    config,
    config_sha256: staleConfig ? "1".repeat(64) : configHash,
    oracle: {
      status: "PASS",
      valid_case_count: forgedCount ? 1_000_000 : oracleBundles.length,
      ...(summaryOnly ? {} : { case_bundles: oracleBundles }),
    },
    mutation: {
      status: "PASS",
      valid_case_count: mutationBundles.length,
      ...(summaryOnly ? {} : { case_bundles: mutationBundles }),
      row_counts: { [stableId]: forgedRowCount ? 100 : mutationBundles.length },
      domain_counts: { [row.domain]: mutationBundles.length },
    },
  };
  cycle.content_digest = sha256(canonicalJson(cycle));
  const normalized = normalizeVerifiedCycle(
    cycle,
    "fixture/cycle-verified.json",
    manifestInfo,
    target,
    session,
    new Set(),
    new Set(),
  );
  return { normalized, cycle, manifest, row };
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
  it("accepts only executed case bundles as fuzz evidence", () => {
    const { normalized } = verifiedCycle();
    assert.equal(normalized.status, "PASS");
    assert.equal(normalized.valid_cases, 1);
    assert.equal(normalized.mutation_row_counts["module:core.crypto.expert.aes256gcm_open"], 100);
    assert.deepEqual(normalized.errors, []);
  });

  it("keeps the real dashboard red when the production manifest is unavailable", () => {
    const { value } = report();
    assert.equal(value.status, "NOT READY");
    assert.equal(value.manifest.readable, false);
    assert.equal(value.conformance.ok, false);
    assert.equal(value.fuzz.ok, false);
    assert.match(value.conformance.errors.join(";"), /manifest|unavailable|unresolved/);
  });

  it("rejects forged million-case summaries when bundles are smaller", () => {
    const { normalized } = verifiedCycle({ forgedCount: true });
    assert.equal(normalized.valid_cases, 1);
    assert.match(normalized.errors.join(";"), /valid case count does not match case bundles/);
    assert.ok(normalized.errors.length > 0);
  });

  it("rejects forged mutation row counters", () => {
    const { normalized } = verifiedCycle({ lowRow: true, forgedRowCount: true });
    assert.equal(normalized.valid_cases, 1);
    assert.equal(normalized.mutation_row_counts["module:core.crypto.expert.aes256gcm_open"], 99);
    assert.match(normalized.errors.join(";"), /row_counts does not match case bundles/);
  });

  it("rejects summary-only evidence with no executed case bundles", () => {
    const { normalized } = verifiedCycle({ summaryOnly: true });
    assert.equal(normalized.valid_cases, 0);
    assert.match(normalized.errors.join(";"), /case bundles are missing/);
  });

  it("rejects stale configuration hashes", () => {
    const { normalized } = verifiedCycle({ staleConfig: true });
    assert.equal(normalized.valid_cases, 0);
    assert.match(normalized.errors.join(";"), /config_sha256/);
  });

  it("rejects a signed receipt with an arbitrary lane packet digest", () => {
    const { value } = report({ tamperPacket: true });
    assert.equal(value.status, "NOT READY");
    assert.equal(value.red_team.ok, false);
    assert.match(value.red_team.errors.join(";"), /packet digest|signature is invalid/);
  });

  it("rejects a bundle whose signed content digest is stale", () => {
    const { normalized } = verifiedCycle({ tamperBundle: true });
    assert.equal(normalized.valid_cases, 1);
    assert.match(normalized.errors.join(";"), /case bundle digest does not match content/);
  });

  it("breaks the clean fuzz gate below the per-row floor", () => {
    const { normalized } = verifiedCycle({ lowRow: true });
    assert.equal(normalized.valid_cases, 1);
    assert.equal(normalized.mutation_row_counts["module:core.crypto.expert.aes256gcm_open"], 99);
    assert.deepEqual(normalized.errors, []);
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
