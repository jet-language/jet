#!/usr/bin/env node
// Card #1414: review a producer report in a separate workflow. This reviewer
// never runs the producer; it checks the immutable report, records provenance,
// then delegates only final contract enforcement to the committed gate.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");
const reportDir = path.resolve(process.argv[2] || "compiled-workload-report");
const gate = path.join(root, "tools", "ci", "compiled-workload-gate.sh");
const reportFiles = [
  "identity.tsv", "samples.tsv", "statistics.tsv", "outcomes.tsv",
  "measurements.tsv", "tiers.tsv", "receipts.tsv", "tier_receipts.tsv",
  "peer_coverage.tsv", "peer_measurements.tsv",
];
const metrics = new Set([
  "source_effort", "build_time", "edit_time", "runtime", "memory",
  "artifact_size", "diagnostics", "debugging", "deployment", "unsafe_burden",
]);

function fail(message) {
  console.error(`compiled workload review: ${message}`);
  process.exit(1);
}
function readTable(name, width) {
  const file = path.join(reportDir, name);
  if (!fs.existsSync(file)) fail(`missing ${name}`);
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(Boolean);
  if (lines.length < 2) fail(`${name} has no data rows`);
  const header = lines[0].split("\t");
  if (header.length !== width) fail(`${name} header width is ${header.length}, expected ${width}`);
  return lines.slice(1).map((line, index) => {
    const values = line.split("\t");
    if (values.length !== width) fail(`${name}:${index + 2} width is ${values.length}, expected ${width}`);
    return Object.fromEntries(header.map((key, field) => [key, values[field]]));
  });
}
function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}
function reportDigest() {
  const digest = crypto.createHash("sha256");
  for (const name of [...reportFiles].sort()) {
    digest.update(name);
    digest.update("\0");
    digest.update(fs.readFileSync(path.join(reportDir, name)));
    digest.update("\0");
  }
  return digest.digest("hex");
}
function identityMap(rows) {
  const map = new Map();
  for (const row of rows) {
    if (map.has(row.key)) fail(`duplicate identity key: ${row.key}`);
    map.set(row.key, row.value);
  }
  return map;
}
function requireSha(value, label) {
  if (!/^[0-9a-f]{64}$/.test(value) || /^0+$/.test(value)) fail(`${label} is not a nonzero SHA-256`);
}
function readPeerLedger() {
  const file = path.join(root, "tests", "compiled_workloads", "peer_ledger.tsv");
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(Boolean);
  const header = lines.shift().split("\t");
  return lines.map((line, index) => {
    const values = line.split("\t");
    if (values.length !== header.length) fail(`peer ledger row ${index + 2} width drifted`);
    return Object.fromEntries(header.map((key, field) => [key, values[field]]));
  });
}
function readPeerAdapterLedger() {
  const file = path.join(root, "tests", "compiled_workloads", "peer_adapter_ledger.tsv");
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(Boolean);
  const header = lines.shift().split("\t");
  return lines.map((line, index) => {
    const values = line.split("\t");
    if (values.length !== header.length) fail(`peer adapter ledger row ${index + 2} width drifted`);
    return Object.fromEntries(header.map((key, field) => [key, values[field]]));
  });
}
function readMeasurementPolicy() {
  const file = path.join(root, "tests", "compiled_workloads", "measurement_policy.tsv");
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(Boolean);
  const header = lines.shift().split("\t");
  return lines.map((line, index) => {
    const values = line.split("\t");
    if (values.length !== header.length) fail(`measurement policy row ${index + 2} width drifted`);
    return Object.fromEntries(header.map((key, field) => [key, values[field]]));
  });
}

let provenance = null;
const provenanceFile = process.env.REVIEW_PROVENANCE_FILE || "";
if (provenanceFile) {
  try {
    provenance = JSON.parse(fs.readFileSync(provenanceFile, "utf8"));
  } catch (error) {
    fail(`review provenance cannot be read: ${error.message}`);
  }
  const reviewRun = provenance?.review;
  const producerRun = provenance?.producer;
  const trustedHeadResult = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
  const trustedHead = trustedHeadResult.status === 0 ? trustedHeadResult.stdout.trim() : "";
  if (!reviewRun || !producerRun || reviewRun.path !== ".github/workflows/compiled-workload-review.yml" ||
      reviewRun.event !== "workflow_run" || reviewRun.head_sha !== trustedHead ||
      producerRun.name !== "CI" || producerRun.conclusion !== "success" ||
      !Number.isInteger(producerRun.id) || !/^[0-9a-f]{40}$/.test(producerRun.head_sha || "") ||
      producerRun.head_sha !== process.env.CANDIDATE_SHA) {
    fail("Actions provenance does not identify the trusted review and producer runs");
  }
}
const identity = identityMap(readTable("identity.tsv", 3));
const candidate = provenance?.producer?.head_sha || process.env.CANDIDATE_SHA || identity.get("candidate_commit");
if (!/^[0-9a-f]{40}$/.test(candidate || "")) fail("candidate commit is not immutable");
const samples = readTable("samples.tsv", 11);
const statistics = readTable("statistics.tsv", 18);
const outcomes = readTable("outcomes.tsv", 17);
const measurements = readTable("measurements.tsv", 13);
const tiers = readTable("tiers.tsv", 9);
const receipts = readTable("receipts.tsv", 20);
const tierReceipts = readTable("tier_receipts.tsv", 10);
const peerCoverage = readTable("peer_coverage.tsv", 20);
const peerMeasurements = readTable("peer_measurements.tsv", 17);
const peerLedger = readPeerLedger();
const peerAdapterLedger = readPeerAdapterLedger();
const measurementPolicyByMetric = new Map(readMeasurementPolicy().map(row => [row.metric, row]));
const peerAdapterByKey = new Map(peerAdapterLedger.map(row => [row.peer_key, row]));
const reportPlatform = identity.get("platform");
requireSha(identity.get("peer_launcher_sha256"), "peer launcher identity");
if (!/^[1-9][0-9]*$/.test(identity.get("samples") || "")) fail("sample identity is invalid");

for (const row of samples) {
  if (row.platform !== reportPlatform || row.target !== "native" || row.tier !== "aot") fail(`sample tier/target drift: ${row.task_id}/${row.language}/${row.metric}`);
}
for (const row of measurements) {
  if (row.platform !== reportPlatform || row.target !== "native" || row.tier !== "aot") fail(`measurement tier/target drift: ${row.task_id}/${row.language}/${row.metric}`);
}
for (const row of statistics) {
  if (row.platform !== reportPlatform || row.target !== "native" || row.tier !== "aot") fail(`statistics tier/target drift: ${row.task_id}/${row.language}/${row.metric}`);
}
if (outcomes.length !== 7) fail(`expected seven workload outcomes, got ${outcomes.length}`);
if (new Set(outcomes.map(row => row.task_id)).size !== outcomes.length) fail("outcome task ids are not unique");
for (const row of outcomes) {
  if (row.jet_status !== "pass" || row.peer_status !== "pass") fail(`unresolved outcome: ${row.task_id}`);
  if (row.review_status !== "pending" || row.review_evidence !== "-") fail(`producer contains review claims: ${row.task_id}`);
}
for (const row of measurements) {
  if (!metrics.has(row.metric) || !["pass", "measured"].includes(row.status) || !/^[0-9]+(?:\.[0-9]+)?$/.test(row.value)) {
    fail(`measurement is not independently reviewable: ${row.task_id}/${row.language}/${row.metric}`);
  }
}
for (const row of statistics) {
  if (!metrics.has(row.metric) || !["pass", "measured"].includes(row.status) || !/^[1-9][0-9]*$/.test(row.samples)) {
    fail(`statistics row is not independently reviewable: ${row.task_id}/${row.language}/${row.metric}`);
  }
}
const peerByKey = new Map(peerLedger.map(peer => [
  `${peer.task_id}|${peer.selection}|${peer.language}|${peer.program}`,
  peer,
]));
if (peerByKey.size !== peerLedger.length || peerAdapterByKey.size !== peerLedger.length) {
  fail("peer ledger identities are not unique or complete");
}
const peerCoverageByKey = new Map();
for (const row of peerCoverage) {
  if (peerCoverageByKey.has(row.peer_key)) fail(`duplicate peer coverage: ${row.peer_key}`);
  const peer = peerByKey.get(row.peer_key);
  const adapter = peerAdapterByKey.get(row.peer_key);
  if (!peer || !adapter) fail(`peer coverage names unknown ledger row: ${row.peer_key}`);
  if (row.task_id !== peer.task_id || row.selection !== peer.selection ||
      row.language !== peer.language || row.program !== peer.program ||
      row.source !== adapter.peer_source || row.source_revision !== peer.source_revision) {
    fail(`peer coverage identity drifted: ${row.peer_key}`);
  }
  const sourcePath = path.join(root, "tests", "compiled_workloads", row.source);
  const expectedPath = path.join(root, "tests", "compiled_workloads", "expected", `${row.task_id}.out`);
  if (!fs.existsSync(sourcePath) || !fs.statSync(sourcePath).isFile()) fail(`peer coverage source is unavailable: ${row.peer_key}`);
  if (row.source_sha256 !== sha256(fs.readFileSync(sourcePath))) fail(`peer coverage source hash drifted: ${row.peer_key}`);
  if (row.expected_sha256 !== sha256(fs.readFileSync(expectedPath))) fail(`peer coverage expected hash drifted: ${row.peer_key}`);
  peerCoverageByKey.set(row.peer_key, row);
  if (row.platform !== reportPlatform || row.target !== "-" || row.tier !== "native") fail(`peer coverage tier/target drift: ${row.peer_key}`);
  for (const field of ["source_sha256", "input_sha256", "expected_sha256", "hostile_input_sha256"]) {
    requireSha(row[field], `peer coverage ${row.peer_key} ${field}`);
  }
  if (row.status === "measured") {
    requireSha(row.output_sha256, `peer coverage ${row.peer_key} output_sha256`);
    requireSha(row.hostile_output_sha256, `peer coverage ${row.peer_key} hostile_output_sha256`);
    if (row.output_sha256 !== row.expected_sha256) fail(`peer coverage output drifted: ${row.peer_key}`);
    const hostileExpected = path.join(root, "tests", "compiled_workloads", "expected", `${row.task_id}.hostile.out`);
    if (row.hostile_output_sha256 !== sha256(fs.readFileSync(hostileExpected))) {
      fail(`peer coverage hostile output drifted: ${row.peer_key}`);
    }
    if (!row.command.startsWith("build=") || !row.command.includes(";run=") || !row.command.includes(";hostile=")) {
      fail(`peer coverage execution receipt is incomplete: ${row.peer_key}`);
    }
  } else if (row.status !== "not-applicable" || row.output_sha256 !== "-" || row.hostile_output_sha256 !== "-") {
    fail(`peer coverage status is incomplete: ${row.peer_key}`);
  }
}
const peerMeasurementsByKey = new Map();
for (const row of peerMeasurements) {
  const key = `${row.peer_key}|${row.metric}`;
  if (peerMeasurementsByKey.has(key)) fail(`duplicate peer measurement: ${key}`);
  if (!metrics.has(row.metric) || row.platform !== reportPlatform || row.target !== "-" || row.tier !== "native") {
    fail(`peer measurement is not independently reviewable: ${key}`);
  }
  peerMeasurementsByKey.set(key, row);
}
const reviewSampleCount = identity.get("samples");
for (const peer of peerLedger) {
  const key = `${peer.task_id}|${peer.selection}|${peer.language}|${peer.program}`;
  const coverage = peerCoverageByKey.get(key);
  if (!coverage) fail(`missing peer coverage: ${key}`);
  const applies = peer.applicable_targets.split(",").map(value => value.trim()).includes(reportPlatform);
  if ((applies && coverage.status !== "measured") || (!applies && coverage.status !== "not-applicable")) {
    fail(`peer coverage applicability drift: ${key}`);
  }
  for (const metric of metrics) {
    const measurement = peerMeasurementsByKey.get(`${key}|${metric}`);
    if (!measurement) fail(`missing peer measurement: ${key}/${metric}`);
    if (applies) {
      if (measurement.status !== "measured" || measurement.samples !== reviewSampleCount ||
          !/^[0-9]+(?:\.[0-9]+)?$/.test(measurement.median) ||
          !/^[0-9]+(?:\.[0-9]+)?$/.test(measurement.min) ||
          !/^[0-9]+(?:\.[0-9]+)?$/.test(measurement.max) ||
          !/^[0-9]+(?:\.[0-9]+)?$/.test(measurement.relative_stdev)) {
        fail(`peer measurement is incomplete: ${key}/${metric}`);
      }
    } else if (measurement.status !== "not-applicable" || measurement.samples !== "0" ||
      measurement.median !== "-" || measurement.min !== "-" || measurement.max !== "-" || measurement.relative_stdev !== "-" ||
      !measurement.evidence.startsWith("peer-targets=") || !measurement.evidence.includes(";reason=host-platform")) {
      fail(`non-applicable peer measurement is incomplete: ${key}/${metric}`);
    }
  }
}
const jetMeasurementByMetric = new Map();
for (const row of measurements) {
  if (row.language !== "jet") continue;
  const key = `${row.task_id}|${row.metric}`;
  if (jetMeasurementByMetric.has(key)) fail(`duplicate Jet measurement: ${key}`);
  jetMeasurementByMetric.set(key, row.value);
}
for (const peer of peerLedger) {
  const key = `${peer.task_id}|${peer.selection}|${peer.language}|${peer.program}`;
  const applies = peer.applicable_targets.split(",").map(value => value.trim()).includes(reportPlatform);
  if (!applies) continue;
  for (const metric of metrics) {
    const policy = measurementPolicyByMetric.get(metric);
    const measurement = peerMeasurementsByKey.get(`${key}|${metric}`);
    const jetValue = Number(jetMeasurementByMetric.get(`${peer.task_id}|${metric}`));
    const peerValue = Number(measurement?.median);
    const tolerance = Number(peer.language === "rust" ? policy?.rust_tolerance_ratio : policy?.tolerance_ratio);
    if (!policy || !measurement || !Number.isFinite(jetValue) || !Number.isFinite(peerValue) ||
        !Number.isFinite(tolerance) || jetValue < 0 || peerValue < 0 || tolerance <= 0 ||
        (jetValue === 0 && peerValue === 0)) {
      fail(`peer comparison is not independently reviewable: ${key}/${metric}`);
    }
    const loss = peerValue === 0
      ? jetValue > 0
      : peer.language === "rust"
        ? jetValue > peerValue * tolerance
        : jetValue >= peerValue * tolerance;
    if (loss) fail(`peer comparison loss: ${key}/${metric} (Jet=${jetValue} peer=${peerValue})`);
  }
}

for (const row of tiers) {
  if (!["pass", "not-applicable"].includes(row.status) || !row.evidence) fail(`tier row is incomplete: ${row.task_id}/${row.language}/${row.tier}`);
}
for (const row of receipts) {
  for (const field of ["source_sha256", "input_sha256", "expected_sha256", "output_sha256", "peer_launcher_sha256"]) {
    requireSha(row[field], `receipt ${row.task_id}/${row.language} ${field}`);
  }
}
for (const row of tierReceipts) {
  if (["pass", "loss"].includes(row.status)) {
    if (!(row.tier === "jit" && row.language === "jet" && row.artifact_sha256 === "-")) {
      requireSha(row.artifact_sha256, `tier receipt ${row.task_id}/${row.language} artifact`);
    }
    const outputOptional = row.tier !== "jit" && row.platform === "cross-target" && row.target !== "web";
    if (outputOptional) {
      if (row.output_sha256 !== "-") fail(`cross-target tier receipt carries output proof: ${row.task_id}/${row.language}/${row.tier}`);
    } else {
      requireSha(row.output_sha256, `tier receipt ${row.task_id}/${row.language} output`);
    }
  } else if (row.artifact_sha256 !== "-" || row.output_sha256 !== "-") {
    fail(`not-applicable tier receipt carries proof: ${row.task_id}/${row.language}/${row.tier}`);
  }
}
const workflow = "compiled-workload-review.yml";
const run = provenance ? String(provenance.review.id) : (process.env.REVIEW_RUN || process.env.GITHUB_RUN_ID || "");
const actor = provenance?.review?.actor?.login || process.env.REVIEW_ACTOR || process.env.GITHUB_ACTOR || "compiled-workload-review";
if (!/^(?:[1-9][0-9]*|self-check-[1-9][0-9]*)$/.test(run) || !/^[A-Za-z0-9_.-]+$/.test(actor) || actor.includes("synthetic")) {
  fail("review provenance is not authenticated");
}
const compilerSha = identity.get("jet_binary_sha256");
const contractSha = identity.get("contract_sha256");
const sourceSha = identity.get("source_closure_sha256");
const digest = reportDigest();
const review = [
  "version\treviewer\treviewed_candidate\tcompiler_sha256\tcontract_sha256\tsource_closure_sha256\treport_sha256\tworkload_fairness\tpeer_fairness\tauthority_fairness\tmeasurement_fairness\ttier_fairness\tloss_ownership\tstatus\tevidence\treview_workflow\treview_run\treview_actor",
  `1\tcompiled-workload-review\t${candidate}\t${compilerSha}\t${contractSha}\t${sourceSha}\t${digest}\tpass\tpass\tpass\tpass\tpass\tpass\tpass\tindependent-report-schema-and-boundary-review;workflow=${workflow};run=${run};actor=${actor}\t${workflow}\t${run}\t${actor}`,
  "",
].join("\n");
fs.writeFileSync(path.join(reportDir, "review.tsv"), review);
const checked = spawnSync("bash", [gate, "--check", reportDir], {
  cwd: root,
  env: { ...process.env, JET_COMPILED_WORKLOAD_CANDIDATE_COMMIT: candidate },
  encoding: "utf8",
});
if (checked.status !== 0) fail(`strict gate rejected reviewed report: ${(checked.stderr || checked.stdout || "").trim()}`);
console.log(`compiled workload review: pass report=${reportDir} candidate=${candidate} run=${run}`);
