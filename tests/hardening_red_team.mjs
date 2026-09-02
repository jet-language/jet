import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import {
  RED_TEAM_EXECUTION_GATE,
  RED_TEAM_LANE_COUNT,
  RED_TEAM_MAX_ACTIVE,
  RED_TEAM_WAVE_COUNT,
  hardeningDedupKey,
  assimilateFindings,
  createSessionManifest,
  makeContextPackets,
  makeLaneReceipt,
  runRedTeamSession,
  sessionManifestDigest,
  signReceipt,
  validateContextPacket,
  validateLaneReceipt,
  verifySignedReceipt,
  validateSessionManifest,
} from "../scripts/agent/hardening-red-team.mjs";
import {
  RECEIPT_DIR_ENV,
  canonicalJson,
  runReceiptRunner,
} from "../scripts/agent/hardening-red-team-receipt-runner.mjs";
import { bundleIdentity, makeResultBundle } from "../scripts/agent/hardening-oracle-layer.mjs";
import { prepareHardening } from "../plugins/tower/app/hardening.mjs";

const ROOT = resolve(".");
const RED_TEAM = resolve("scripts/agent/hardening-red-team.mjs");
const HASH = `sha256:${"a".repeat(64)}`;

function manifest() {
  return createSessionManifest({
    root: ROOT,
    session_id: "red-team-test-session",
    created_at: "2026-08-30T00:00:00.000Z",
    commit: "b".repeat(40),
    binary_sha256: HASH,
    registry_snapshot: {
      path: ".jet/test-registry.json",
      sha256: HASH,
      source_snapshot_hash: HASH,
    },
    rig_config: {
      schema_version: 1,
      suite_concurrency: 2,
      cargo_build_jobs: 4,
      seed: "test",
      variants: "1",
    },
    resource_limits: {
      max_active_lanes: RED_TEAM_MAX_ACTIVE,
      lane_timeout_ms: 5_000,
      capture_bytes: 16 * 1024,
      target_cap_gib: 80,
      cache_cap_gib: 4,
      interesting_cap_mib: 512,
      log_cap_mib: 1,
      scratch: "disk-backed cache scratch only",
      cleanup: "required",
    },
  });
}

function targetOf(value) {
  return {
    root: value.target.root,
    commit: value.target.commit,
    binary_path: value.target.binary_path,
    binary_sha256: value.target.binary_sha256,
    platform: value.target.platform,
    arch: value.target.arch,
    registry_snapshot: value.registry_snapshot,
    public_surface_snapshot: value.public_surface_snapshot,
  };
}

function laneReports(value, overrides = {}) {
  return Array.from({ length: RED_TEAM_LANE_COUNT }, (_, index) => makeLaneReceipt(value, {
    lane_id: `lane-${index + 1}`,
    context_id: `fresh-context-${index + 1}`,
    agent_id: `luna-agent-${index + 1}`,
    ...overrides,
  }));
}
const RECEIPT_RUNNER = resolve("scripts/agent/hardening-red-team-receipt-runner.mjs");

function receiptScratch() {
  const parent = join(homedir(), ".cache/jet-test-scratch");
  mkdirSync(parent, { recursive: true });
  return mkdtempSync(join(parent, "red-team-receipt-runner-"));
}

function invokeReceiptRunner(packet, receiptText, {
  directory = receiptScratch(),
  setDirectory = true,
} = {}) {
  if (receiptText !== undefined) {
    writeFileSync(join(directory, `${packet.lane_id}.json`), receiptText);
  }
  const env = { ...process.env };
  if (setDirectory) env[RECEIPT_DIR_ENV] = directory;
  else delete env[RECEIPT_DIR_ENV];
  return {
    directory,
    result: spawnSync(process.execPath, [RECEIPT_RUNNER], {
      cwd: ROOT,
      env,
      input: JSON.stringify(packet),
      encoding: "utf8",
    }),
  };
}

function finding(value, severity = "P0") {
  const source = `fn run() {\n    value :: 1\n    print(value)\n}\n`;
  const tierObservations = ["aot", "jet_run", "interpreter"].map((tier) => ({
    tier,
    stdout: "wrong\n",
    stderr: "",
    exit: 0,
    signal: null,
    timeout: false,
    relation: "wrong",
  }));
  const bundle = makeResultBundle({
    run_id: "red-team-finding-run",
    stable_surface_id: "surface:test",
    tier: "aot",
    tier_command: "jet run source.jet",
    seed: "seed-test",
    mutation_arm: "boundary-min",
    source,
    stdout: "wrong\n",
    stderr: "",
    exit: 0,
    signal: null,
    timeout: false,
    expected_relation: "expected",
    actual_relation: "wrong",
    normalization: [],
    classification: severity === "P0" ? "silent-data" : "loud-failure",
    tower_action: "create-or-update",
    oracle: {
      name: "red-team-test-oracle",
      version: "1",
      input_digest: HASH,
      independence_class: "published-vector",
      provenance: "red-team-test",
    },
    commit: value.target.commit,
    binary_sha256: value.target.binary_sha256,
    registry_snapshot_hash: value.registry_snapshot.sha256,
    config_hash: HASH,
    tier_observations: tierObservations,
    applicable_tiers: ["aot", "jet_run", "interpreter"],
  });
  return {
    finding_id: "finding-test",
    severity,
    silent_wrong_data: severity === "P0",
    reproducer_id: "reproducer-test",
    bundle,
    bundle_identity: bundleIdentity(bundle),
  };
}

function withFinding(value, severity = "P0") {
  const item = finding(value, severity);
  return laneReports(value, {
    minimized_reproducers: [{
      id: "reproducer-test",
      source: `fn run() {\n    value :: 1\n    print(value)\n}\n`,
      value_consuming: true,
      observer: "print(value)",
    }],
    unique_findings: [item],
  });
}

test("manifest freezes the target and emits eight independent hidden-card packets", () => {
  const value = manifest();
  assert.equal(validateSessionManifest(value), true);
  assert.equal(sessionManifestDigest(value), value.manifest_sha256);
  assert.equal(value.quota.lanes, RED_TEAM_LANE_COUNT);
  assert.equal(value.quota.waves, RED_TEAM_WAVE_COUNT);
  assert.equal(value.quota.lanes_per_wave, RED_TEAM_MAX_ACTIVE);
  assert.equal(value.current_defect_cards_hidden, true);
  const packets = makeContextPackets(value);
  assert.equal(packets.length, RED_TEAM_LANE_COUNT);
  assert.deepEqual(packets.map((packet) => packet.wave), [1, 1, 2, 2, 3, 3, 4, 4]);
  assert.ok(packets.every((packet) => packet.visibility.current_defect_cards === "hidden"));
  assert.ok(packets.every((packet) => !Object.hasOwn(packet, "known_findings")));
  assert.ok(packets.every((packet) => validateContextPacket(packet, value)));
  assert.notEqual(packets[0], packets[1]);
  assert.throws(() => validateContextPacket({ ...packets[0], known_findings: [] }, value), /forbidden known_findings/);
  assert.throws(
    () => validateSessionManifest({ ...value, lane_briefs: [...value.lane_briefs].reverse() }),
    /ratified attack slice or wave order/,
  );
});

test("pre-produced lane adapter binds packet identities and emits canonical JSON", () => {
  const value = manifest();
  const packet = makeContextPackets(value)[0];
  const receipt = makeLaneReceipt(value, {
    lane_id: packet.lane_id,
    packet_digest: packet.context_digest,
    context_id: "fresh-adapter-context",
    agent_id: "fresh-adapter-agent",
  });
  const { directory, result } = invokeReceiptRunner(packet, JSON.stringify(receipt));
  try {
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, "");
    assert.equal(result.stdout, `${canonicalJson(receipt)}\n`);
    assert.deepEqual(runReceiptRunner(JSON.stringify(packet), { receipt_dir: directory }), receipt);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("pre-produced lane adapter refuses absent, unsafe, stale, and mismatched receipts", () => {
  const value = manifest();
  const packet = makeContextPackets(value)[0];
  const valid = makeLaneReceipt(value, {
    lane_id: packet.lane_id,
    packet_digest: packet.context_digest,
    context_id: "fresh-adapter-context",
    agent_id: "fresh-adapter-agent",
  });
  const clone = (item) => JSON.parse(JSON.stringify(item));
  const cases = [
    ["missing explicit receipt directory", valid, { setDirectory: false }, /explicit receipt directory/],
    ["missing receipt file", undefined, {}, /missing pre-produced lane receipt/],
    ["malformed JSON", "{", {}, /not valid JSON|unterminated|invalid/],
    ["wrong schema", { ...valid, schema: "wrong" }, {}, /lane receipt schema/],
    ["missing field", (() => { const item = clone(valid); delete item.counts; return item; })(), {}, /missing counts/],
    ["extra field", { ...valid, extra: true }, {}, /unexpected field/],
    ["nonterminal status", { ...valid, status: "running" }, {}, /status/],
    ["early lane", { ...valid, complete: false }, {}, /stopped before completing/],
    ["stale lane", { ...valid, semantic_change: true }, {}, /stale/],
    ["session mismatch", { ...valid, session_id: "other-session" }, {}, /another session/],
    ["lane mismatch", { ...valid, lane_id: "lane-2" }, {}, /lane_id does not match packet/],
    ["packet mismatch", { ...valid, packet_digest: `sha256:${"b".repeat(64)}` }, {}, /independent context packet/],
    ["target mismatch", { ...valid, target: { ...valid.target, commit: "c".repeat(40) } }, {}, /target does not match packet/],
    ["surface mismatch", {
      ...valid,
      target: {
        ...valid.target,
        public_surface_snapshot: { ...valid.target.public_surface_snapshot, sha256: `sha256:${"c".repeat(64)}` },
      },
    }, {}, /public surface does not match packet/],
    ["duplicate JSON field", JSON.stringify(valid).replace("{", '{"lane_id":"lane-2",'), {}, /duplicate object field lane_id/],
    ["invalid packet lane path", undefined, { packet: { ...packet, lane_id: "../lane-1" } }, /context packet lane_id is invalid/],
  ];
  for (const [label, input, options, expected] of cases) {
    const directory = options.directory || receiptScratch();
    try {
      const packetInput = options.packet || packet;
      const { result } = invokeReceiptRunner(
        packetInput,
        input === undefined ? undefined : typeof input === "string" ? input : JSON.stringify(input),
        { directory, setDirectory: options.setDirectory ?? true },
      );
      assert.notEqual(result.status, 0, `${label} unexpectedly passed`);
      assert.equal(result.stdout, "", `${label} wrote protocol output`);
      assert.match(result.stderr, expected, label);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  }

  const extraDirectory = receiptScratch();
  try {
    writeFileSync(join(extraDirectory, `${packet.lane_id}.json`), JSON.stringify(valid));
    writeFileSync(join(extraDirectory, "extra.json"), "{}");
    const { result } = invokeReceiptRunner(packet, undefined, { directory: extraDirectory });
    assert.notEqual(result.status, 0);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /unexpected receipt file/);
  } finally {
    rmSync(extraDirectory, { recursive: true, force: true });
  }

  const symlinkDirectory = receiptScratch();
  const symlinkTarget = receiptScratch();
  try {
    writeFileSync(join(symlinkTarget, `${packet.lane_id}.json`), JSON.stringify(valid));
    const symlinkPath = join(symlinkDirectory, `${packet.lane_id}.json`);
    symlinkSync(join(symlinkTarget, `${packet.lane_id}.json`), symlinkPath);
    const { result } = invokeReceiptRunner(packet, undefined, { directory: symlinkDirectory });
    assert.notEqual(result.status, 0);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /symlink/);
  } finally {
    rmSync(symlinkDirectory, { recursive: true, force: true });
    rmSync(symlinkTarget, { recursive: true, force: true });
  }

  const linkedDirectoryParent = receiptScratch();
  const linkedDirectoryTarget = receiptScratch();
  try {
    writeFileSync(join(linkedDirectoryTarget, `${packet.lane_id}.json`), JSON.stringify(valid));
    const linkedDirectory = join(linkedDirectoryParent, "receipts");
    symlinkSync(linkedDirectoryTarget, linkedDirectory, "dir");
    const { result } = invokeReceiptRunner(packet, undefined, { directory: linkedDirectory });
    assert.notEqual(result.status, 0);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /symlink/);
  } finally {
    rmSync(linkedDirectoryParent, { recursive: true, force: true });
    rmSync(linkedDirectoryTarget, { recursive: true, force: true });
  }
});

test("bounded runner executes four waves of two and signs a complete verdict", async () => {
  const value = manifest();
  let active = 0;
  let maximum = 0;
  const started = [];
  const receipt = await runRedTeamSession({
    manifest: value,
    current_target: targetOf(value),
    lane_runner: async (packet) => {
      active += 1;
      maximum = Math.max(maximum, active);
      started.push(packet.lane_id);
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 2));
      active -= 1;
      return makeLaneReceipt(value, {
        lane_id: packet.lane_id,
        packet_digest: packet.context_digest,
        context_id: `context-${packet.lane_id}`,
        agent_id: `agent-${packet.lane_id}`,
      });
    },
    execution_gate: RED_TEAM_EXECUTION_GATE,
    reviewer_id: "independent-reviewer",
  });
  assert.equal(receipt.status, "PASS");
  assert.equal(maximum, RED_TEAM_MAX_ACTIVE);
  assert.deepEqual(started, ["lane-1", "lane-2", "lane-3", "lane-4", "lane-5", "lane-6", "lane-7", "lane-8"]);
  assert.equal(receipt.lanes.length, RED_TEAM_LANE_COUNT);

  assert.equal(receipt.max_active_lanes, RED_TEAM_MAX_ACTIVE);
  assert.equal(verifySignedReceipt(receipt), true);
  assert.equal(verifySignedReceipt(receipt, value), true);
});
test("pre-recorded lanes without observed waves cannot pass", async () => {
  const value = manifest();
  const outOfOrder = await runRedTeamSession({
    manifest: value,
    lane_receipts: laneReports(value).reverse(),
    current_target: targetOf(value),
    execution_gate: RED_TEAM_EXECUTION_GATE,
    signer_id: "signer-order",
    reviewer_id: "reviewer-order",
  });
  assert.equal(outOfOrder.status, "FAILED");
  assert.match(outOfOrder.failure_reasons.join(";"), /frozen wave order/);
  assert.match(outOfOrder.failure_reasons.join(";"), /observed active lane/);
  assert.equal(outOfOrder.max_active_lanes, 0);
  assert.equal(verifySignedReceipt(outOfOrder), true);
  assert.equal(verifySignedReceipt(outOfOrder, value), true);
});
test("manifest-bound verification rejects re-signed packet, target, and slice drift", async () => {
  const value = manifest();
  const receipt = await runRedTeamSession({
    manifest: value,
    current_target: targetOf(value),
    lane_runner: async (packet) => makeLaneReceipt(value, {
      lane_id: packet.lane_id,
      context_id: `context-${packet.lane_id}`,
      agent_id: `agent-${packet.lane_id}`,
    }),
    execution_gate: RED_TEAM_EXECUTION_GATE,
    reviewer_id: "independent-reviewer-bound",
  });
  assert.equal(receipt.status, "PASS");
  const resign = (lanePatch) => signReceipt({
    ...receipt,
    lanes: receipt.lanes.map((lane, index) => index === 0 ? { ...lane, ...lanePatch } : lane),
  }, { signer_id: "tamper-signer", reviewer_id: "tamper-reviewer" });
  assert.throws(
    () => verifySignedReceipt(resign({ packet_digest: `sha256:${"c".repeat(64)}` }), value),
    /independent context packet/,
  );
  assert.throws(
    () => verifySignedReceipt(resign({ target: { ...receipt.lanes[0].target, root: "/tmp/other-root" } }), value),
    /frozen binary/,
  );
  assert.throws(
    () => verifySignedReceipt(resign({ target: { ...receipt.lanes[0].target, root: "/tmp/other-root" } })),
    /different frozen binary/,
  );
  assert.throws(
    () => verifySignedReceipt(resign({ attack_surface: "not-the-frozen-slice" }), value),
    /frozen attack slice/,
  );
});


test("missing or early lanes fail, and a replayed P0 cannot pass", async () => {
  const value = manifest();
  const incomplete = await runRedTeamSession({
    manifest: value,
    lane_receipts: laneReports(value).slice(0, RED_TEAM_LANE_COUNT - 1),
    current_target: targetOf(value),
    signer_id: "signer-incomplete",
    reviewer_id: "reviewer-incomplete",
  });
  assert.equal(incomplete.status, "FAILED");
  assert.match(incomplete.failure_reasons.join(";"), /full eight-lane quota/);
  assert.equal(verifySignedReceipt(incomplete), true);

  const early = laneReports(value);
  early[0] = { ...early[0], complete: false, stopped_early: true };
  const earlyReceipt = await runRedTeamSession({
    manifest: value,
    lane_receipts: early,
    current_target: targetOf(value),
    signer_id: "signer-early",
    reviewer_id: "reviewer-early",
  });
  assert.equal(earlyReceipt.status, "FAILED");
  assert.match(earlyReceipt.failure_reasons.join(";"), /stopped before completing/);

  const p0 = withFinding(value);
  const assimilated = [];
  const p0Receipt = await runRedTeamSession({
    manifest: value,
    lane_receipts: p0,
    current_target: targetOf(value),
    replay_finding: async (item, frozen) => ({
      confirmed: true,
      target: targetOf(frozen),
      bundle_identity: item.bundle_identity,
    }),
    assimilate: async (items) => {
      assimilated.push(...items);
      return [{ status: "SKIPPED", route: "#2338" }];
    },
    signer_id: "signer-p0",
    reviewer_id: "reviewer-p0",
  });
  assert.equal(p0Receipt.status, "FAILED");
  assert.equal(p0Receipt.p0_count, 1);
  assert.equal(p0Receipt.replayed_findings.length, 1);
  assert.equal(assimilated.length, 1);
  assert.match(p0Receipt.failure_reasons.join(";"), /P0|/);
});

test("finding assimilation uses Tower CLI and #2338 root-seam dedup without force", async () => {
  const value = manifest();
  const item = finding(value, "P1");
  const calls = [];
  const root = mkdtempSync(join(homedir(), ".cache/jet-test-scratch", "red-team-tower-"));
  const tower = join(root, "tower.mjs");
  writeFileSync(tower, "// fake Tower CLI\n");
  try {
    const actions = await assimilateFindings([item], value, {
      root: ROOT,
      tower_cli: tower,
      command: async (options) => {
        calls.push(options);
        return { ok: true, stdout: Buffer.from(JSON.stringify({ id: "card-id", num: 2340, action: "updated" })), stdout_truncated: false };
      },
    });
    assert.equal(actions[0].route, "#2338");
    assert.equal(calls.length, 1);
    assert.ok(calls[0].args.includes("card"));
    assert.ok(calls[0].args.includes("add"));
    assert.ok(calls[0].args.includes("--stdin"));
    assert.ok(calls[0].args.includes("--by"));
    assert.equal(calls[0].args.includes("--force"), false);
    const payload = JSON.parse(calls[0].stdin);
    assert.match(payload.body, /#2338/);
    assert.ok(payload.hardeningDedupKey.startsWith("hardening:v1|"));
    assert.equal(payload.hardeningFindingId, item.finding_id);
    assert.ok(payload.hardeningEvidence.bundleDigest.startsWith("sha256:"));
    const prepared = prepareHardening(payload);
    assert.equal(prepared.key, payload.hardeningDedupKey);
    assert.equal(prepared.evidence.bundleDigest, payload.hardeningEvidence.bundleDigest);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("lane findings deduplicate through the canonical #2338 hardening key", async () => {
  const value = manifest();
  const first = finding(value, "P1");
  const second = finding(value, "P1");
  second.finding_id = "finding-test-2";
  second.bundle = { ...second.bundle, seed: "seed-test-2" };
  second.bundle_identity = bundleIdentity(second.bundle);
  assert.notEqual(first.bundle_identity, second.bundle_identity);
  assert.equal(hardeningDedupKey(first), hardeningDedupKey(second));
  const reports = laneReports(value);
  reports[0] = makeLaneReceipt(value, {
    lane_id: "lane-1",
    context_id: "context-lane-1",
    agent_id: "agent-lane-1",
    minimized_reproducers: [{ id: "reproducer-test", source: first.bundle.source, value_consuming: true, observer: "print(value)" }],
    unique_findings: [first],
  });
  reports[1] = makeLaneReceipt(value, {
    lane_id: "lane-2",
    context_id: "context-lane-2",
    agent_id: "agent-lane-2",
    minimized_reproducers: [{ id: "reproducer-test", source: second.bundle.source, value_consuming: true, observer: "print(value)" }],
    unique_findings: [second],
  });
  const receipt = await runRedTeamSession({
    manifest: value,
    lane_runner: async (packet) => {
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 1));
      return reports.find((report) => report.lane_id === packet.lane_id);
    },
    current_target: targetOf(value),
    replay_finding: async (item, frozen) => ({
      confirmed: true,
      target: targetOf(frozen),
      bundle_identity: item.bundle_identity,
    }),
    assimilate: async (items) => items.map((item) => ({
      status: "WRITTEN",
      route: "#2338",
      finding_id: item.finding_id,
    })),
    execution_gate: RED_TEAM_EXECUTION_GATE,
    signer_id: "signer-canonical-dedup",
    reviewer_id: "reviewer-canonical-dedup",
  });
  assert.equal(receipt.status, "PASS");
  assert.equal(receipt.findings.length, 1);
  assert.equal(receipt.findings[0].hardening_dedup_key, hardeningDedupKey(first));
  assert.equal(receipt.finding_duplicates.length, 1);
  assert.equal(receipt.finding_duplicates[0].hardening_dedup_key, hardeningDedupKey(first));
  assert.equal(receipt.replayed_findings.length, 1);
  assert.equal(receipt.assimilation.length, 1);
  assert.throws(
    () => verifySignedReceipt(signReceipt({ ...receipt, finding_duplicates: [] }, {
      signer_id: "signer-dedup-tamper",
      reviewer_id: "reviewer-dedup-tamper",
    })),
    /duplicate evidence is incomplete/,
  );
});

test("signed verdict rejects self-review and tampering", () => {
  assert.throws(() => signReceipt({ status: "FAILED" }, { signer_id: "same", reviewer_id: "same" }), /distinct/);
  const receipt = signReceipt({ status: "FAILED", session_id: "s" }, { signer_id: "signer", reviewer_id: "reviewer" });
  assert.equal(verifySignedReceipt(receipt), true);
  assert.throws(() => verifySignedReceipt({ ...receipt, status: "PASS" }), /signature|digest/);
});

test("signed protocol PASS cannot omit lane evidence", () => {
  const receipt = signReceipt({
    receipt_kind: "fresh-context-red-team-verdict",
    status: "PASS",
    manifest_sha256: HASH,
    execution_gate: "OWNER_AUTHORIZED_REAL_EIGHT_LANE_EXECUTION",
    session_id: "forged-pass",
    session: {
      commit: "b".repeat(40),
      binary_sha256: HASH,
      binary_path: "target/debug/jet",
      root: ".",
      platform: process.platform,
      arch: process.arch,
      registry_sha256: HASH,
      public_surface_sha256: HASH,
    },
    quota: { lanes: RED_TEAM_LANE_COUNT, waves: RED_TEAM_WAVE_COUNT, lanes_per_wave: RED_TEAM_MAX_ACTIVE, full_quota_required: true },
    lanes: Array.from({ length: RED_TEAM_LANE_COUNT }, () => ({})),
    lane_agents: [],
    findings: [],
    finding_duplicates: [],
    replayed_findings: [],
    assimilation: [],
    stale_reasons: [],
    failure_reasons: [],
    p0_count: 0,
    unique_finding_count: 0,
    max_active_lanes: RED_TEAM_MAX_ACTIVE,
    started_at: "2026-08-30T00:00:00.000Z",
    finished_at: "2026-08-30T00:00:01.000Z",
    cleanup: { active_agents: 0, active_processes: 0, scratch_paths: [], alternate_targets: [], unbounded_logs: false, complete: true },
    independent_discovery: { current_defect_cards_hidden_until: "all-eight-independent-receipts", revealed_after_discovery: true },
  }, { signer_id: "forged-signer", reviewer_id: "forged-reviewer" });
  assert.throws(() => verifySignedReceipt(receipt), /invalid lane receipt/);
});

test("real eight-lane execution stays owner-gated", () => {
  const result = spawnSync(process.execPath, [RED_TEAM, "run", "--json"], {
    cwd: ROOT,
    encoding: "utf8",
    env: { ...process.env, JET_HARDENING_RED_TEAM_EXECUTE: "0" },
  });
  assert.equal(result.status, 2);
  const output = JSON.parse(result.stdout);
  assert.equal(output.execution_gate, "OWNER_REQUIRED_FOR_REAL_EIGHT_LANE_EXECUTION");
  assert.match(output.reason, /owner-gated/);
});

test("target drift and cleanup residue invalidate the verdict", async () => {
  const value = manifest();
  const stale = await runRedTeamSession({
    manifest: value,
    lane_receipts: laneReports(value),
    current_target: { ...targetOf(value), binary_sha256: `sha256:${"c".repeat(64)}` },
    signer_id: "signer-stale",
    reviewer_id: "reviewer-stale",
  });
  assert.equal(stale.status, "STALE");
  assert.match(stale.stale_reasons.join(";"), /binary changed/);

  const residue = await runRedTeamSession({
    manifest: value,
    lane_receipts: laneReports(value),
    current_target: targetOf(value),
    cleanup: async () => ({ active_agents: 0, active_processes: 1, scratch_paths: [], alternate_targets: [], complete: true }),
    signer_id: "signer-cleanup",
    reviewer_id: "reviewer-cleanup",
  });
  assert.equal(residue.status, "FAILED");
  assert.equal(residue.cleanup.active_processes, 1);
  assert.match(residue.failure_reasons.join(";"), /active processes remain/);
});

test("verifier rejects a lane agent as signer or reviewer", () => {
  const receipt = signReceipt({
    status: "FAILED",
    session_id: "self-review-session",
    lane_agents: [{ lane_id: "lane-1", agent_id: "lane-agent", context_id: "fresh-context" }],
  }, { signer_id: "lane-agent", reviewer_id: "independent-reviewer" });
  assert.throws(() => verifySignedReceipt(receipt), /lane agent cannot sign or review/);
});

test("silent-data classification cannot be demoted by a lane severity label", async () => {
  const value = manifest();
  const reports = withFinding(value, "P0");
  reports[0].unique_findings[0].severity = "P1";
  const receipt = await runRedTeamSession({
    manifest: value,
    lane_receipts: reports,
    current_target: targetOf(value),
    replay_finding: async (item, frozen) => ({
      confirmed: true,
      target: targetOf(frozen),
      bundle_identity: item.bundle_identity,
    }),
    assimilate: async () => [],
    signer_id: "signer-classification",
    reviewer_id: "reviewer-classification",
  });
  assert.equal(receipt.status, "FAILED");
  assert.equal(receipt.p0_count, 1);
});
test("manifest and lane receipts preserve the ratified resource and slice identity", () => {
  const value = manifest();
  for (const [key, expected] of [
    ["cpu_quota_percent", 200],
    ["memory_high_gib", 6],
    ["memory_max_gib", 8],
    ["memory_swap_max_gib", 2],
    ["tasks_max", 64],
    ["io_weight", 10],
    ["nice", 10],
    ["runtime_max_sec", 95 * 60],
    ["min_free_gib", 16],
    ["target_cap_gib", 80],
    ["cache_cap_gib", 4],
    ["interesting_cap_mib", 512],
    ["log_cap_mib", 1],
    ["incremental", 0],
  ]) {
    assert.equal(value.rig_config[key], expected);
    assert.equal(value.resource_limits[key], expected);
  }
  const lane = makeLaneReceipt(value, { lane_id: "lane-1" });
  assert.equal(lane.attack_surface, value.lane_briefs[0].attack_surface);
  assert.equal(lane.brief, value.lane_briefs[0].brief);
  assert.equal(lane.target.registry_snapshot.sha256, value.registry_snapshot.sha256);
  assert.equal(lane.target.public_surface_snapshot.sha256, value.public_surface_snapshot.sha256);
  assert.equal(validateLaneReceipt(lane, value).lane_id, "lane-1");
  assert.throws(
    () => validateLaneReceipt({ ...lane, attack_surface: "other-surface" }, value),
    /frozen attack slice/,
  );
  assert.throws(
    () => validateSessionManifest({
      ...value,
      resource_limits: { ...value.resource_limits, memory_max_gib: 7 },
    }),
    /resource_limits.memory_max_gib/,
  );
});

test("an unauthorized library run cannot sign a passing verdict", async () => {
  const value = manifest();
  const receipt = await runRedTeamSession({
    manifest: value,
    lane_receipts: laneReports(value),
    current_target: targetOf(value),
    signer_id: "unauthorized-signer",
    reviewer_id: "unauthorized-reviewer",
  });
  assert.equal(receipt.status, "FAILED");
  assert.match(receipt.failure_reasons.join(";"), /owner authorization/);
  assert.equal(verifySignedReceipt(receipt, value), true);
});
