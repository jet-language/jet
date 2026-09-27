#!/usr/bin/env node
// Criterion11 producer for the canonical tools/perf Source-implementation lane.
//
// This file intentionally has no compiler fallback.  Foundation must provide
// the private executable adapter named by JET_SOURCE_COMPILER_PERF_ADAPTER.
// The adapter owns the real generated Source entry/Runner and native
// Rust-reference invocations for each actual execution tier; this producer
// only supplies the frozen matrix, records source-bound receipts, and hands
// the completed report to the strict gate.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const perfDir = path.join(root, "tools", "perf");
const contractPath = path.join(perfDir, "source-compiler-contract.tsv");
const policyPath = path.join(perfDir, "source-compiler-policy.tsv");
const gatePath = path.join(perfDir, "source-compiler-gate.sh");
const adapterContract = "source-compiler-performance-adapter-v2";
const policyHeader = "version\tmetric\tunit\tcomparison\tsamples\tmin_samples\tmin_value\tmax_relative_stdev\tmax_outliers\tratio_limit";
const contractHeader = "version\tcell_id\tcase\tworkflow\texecution_tier\tinput\texpected\tsource_entry\tsource_manifest\toutcome\ttarget\tprofile\toptimization\tartifact_kind\tmetrics";
const identityHeader = ["version", "key", "value"];
const sampleHeader = ["version", "cell_id", "case", "workflow", "execution_tier", "implementation", "metric", "sample", "pair_id", "value", "unit", "receipt_key", "evidence"];
const receiptHeader = ["version", "cell_id", "case", "workflow", "execution_tier", "implementation", "status", "source_input_sha256", "source_closure_sha256", "expected_sha256", "output_sha256", "diagnostic_sha256", "diagnostic_count", "semantic_expected_sha256", "semantic_output_sha256", "semantic_equivalence", "artifact_sha256", "generated_source_sha256", "compiler_sha256", "target", "profile", "optimization", "artifact_kind", "correctness", "source_boundary", "command", "adapter_sha256", "pairing", "evidence"];

function fail(message) {
  console.error(`source compiler runner: ${message}`);
  process.exit(1);
}
function requireFile(file, label) {
  let stat;
  try {
    stat = fs.statSync(file);
  } catch {
    fail(`${label} is unavailable: ${file}`);
  }
  if (!stat.isFile()) fail(`${label} is unavailable: ${file}`);
}
function sha256File(file) {
  requireFile(file, "hash input");
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
function nonzeroSha(value, label) {
  if (!/^[0-9a-f]{64}$/.test(String(value)) || /^0+$/.test(String(value))) fail(`${label} is not a nonzero SHA-256`);
}
function commitIdentity() {
  const configured = process.env.JET_CI_CANDIDATE_COMMIT || process.env.GITHUB_SHA || "";
  const result = configured ? { status: 0, stdout: configured } : spawnSync("git", ["rev-parse", "--verify", "HEAD"], { cwd: root, encoding: "utf8" });
  const commit = String(result.stdout || "").trim();
  if (!/^[0-9a-fA-F]{40}$/.test(commit)) fail("candidate commit identity is unavailable or invalid");
  return commit;
}
function cleanText(value) {
  return String(value || "").replace(/[\t\r\n]+/g, " ").trim();
}
function table(file, expectedHeader, expectedWidth) {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(line => line.length > 0 && !line.startsWith("#"));
  if (lines.shift() !== expectedHeader) fail(`schema drifted: ${path.relative(root, file)}`);
  return lines.map((line, index) => {
    const fields = line.split("\t");
    if (fields.length !== expectedWidth) fail(`row ${index + 2} in ${path.relative(root, file)} has ${fields.length} fields, expected ${expectedWidth}`);
    return fields;
  });
}
function writeRowsAtomic(file, header, rows) {
  const temporary = `${file}.tmp.${process.pid}`;
  fs.writeFileSync(temporary, [header, ...rows].map(row => Array.isArray(row) ? row.join("\t") : row).join("\n") + "\n");
  fs.renameSync(temporary, file);
}
function rejectRamScratch(directory) {
  const resolved = path.resolve(directory);
  if (resolved === "/tmp" || resolved.startsWith(`/tmp${path.sep}`)) fail(`scratch must not use RAM-backed /tmp: ${directory}`);
  if (resolved === path.join(root, "target") || resolved.startsWith(`${path.join(root, "target")}${path.sep}`)) fail(`scratch must not use a Cargo target directory: ${directory}`);
}
function resultFile(resultDir, rawPath, label, required = true, expectedRoot = null) {
  if (!required && (rawPath === undefined || rawPath === null || rawPath === "-")) return null;
  if (typeof rawPath !== "string" || rawPath.length === 0 || path.isAbsolute(rawPath)) fail(`${label} must be a relative result path`);
  const resultRoot = fs.realpathSync(resultDir);
  if (expectedRoot !== null && resultRoot !== expectedRoot) fail(`request result directory was replaced before hashing: ${resultDir}`);
  const candidate = path.resolve(resultDir, rawPath);
  let resolved;
  try {
    resolved = fs.realpathSync(candidate);
  } catch {
    fail(`${label} is unavailable: ${candidate}`);
  }
  if (resolved !== resultRoot && !resolved.startsWith(`${resultRoot}${path.sep}`)) fail(`${label} escapes the request result directory: ${rawPath}`);
  requireFile(resolved, label);
  return { path: resolved, relative: path.relative(resultRoot, resolved), sha256: sha256File(resolved) };
}
function compilerFile(resultDir, rawPath, expectedRoot = null) {
  if (typeof rawPath !== "string" || rawPath.length === 0 || !path.isAbsolute(rawPath)) fail("compiler_path must be an absolute path");
  const rootReal = fs.realpathSync(root);
  const resultRoot = fs.realpathSync(resultDir);
  if (expectedRoot !== null && resultRoot !== expectedRoot) fail(`request result directory was replaced before hashing: ${resultDir}`);
  let resolved;
  try {
    resolved = fs.realpathSync(rawPath);
  } catch {
    fail(`compiler_path is unavailable: ${rawPath}`);
  }
  if ((resolved !== rootReal && !resolved.startsWith(`${rootReal}${path.sep}`)) &&
      (resolved !== resultRoot && !resolved.startsWith(`${resultRoot}${path.sep}`))) fail(`compiler_path is outside the checked source/result roots: ${rawPath}`);
  requireFile(resolved, "compiler artifact");
  return { path: resolved, relative: path.relative(rootReal, resolved), sha256: sha256File(resolved) };
}
function diagnosticCount(file) {
  const text = fs.readFileSync(file, "utf8");
  return text.length === 0 ? 0 : text.split(/\r?\n/).filter(line => line.length > 0).length;
}
function contractHash() {
  const rows = [
    `tools/perf/source-compiler-contract.tsv\t${sha256File(contractPath)}\n`,
    `tools/perf/source-compiler-policy.tsv\t${sha256File(policyPath)}\n`,
  ];
  return crypto.createHash("sha256").update(rows.join(""), "utf8").digest("hex");
}
function runAdapter(adapter, requestPath, environment) {
  const timeout = Number(process.env.JET_SOURCE_COMPILER_PERF_TIMEOUT_MS || "180000");
  if (!Number.isInteger(timeout) || timeout <= 0) fail("JET_SOURCE_COMPILER_PERF_TIMEOUT_MS must be a positive integer");
  const result = spawnSync(adapter, ["--request", requestPath], {
    cwd: root,
    env: environment,
    encoding: "utf8",
    timeout,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) fail(`private Foundation adapter failed to start: ${result.error.message}`);
  if (result.status !== 0) fail(`private Foundation adapter failed for ${path.basename(requestPath)}: ${cleanText(result.stderr)}`);
  const text = String(result.stdout || "").trim();
  if (!text) fail("private Foundation adapter returned no receipt");
  let response;
  try {
    response = JSON.parse(text);
  } catch (error) {
    fail(`private Foundation adapter returned non-JSON receipt: ${error.message}`);
  }
  if (!response || response.protocol !== adapterContract) fail("private Foundation adapter receipt protocol is missing or stale");
  return response;
}
function expectedMetrics(cell, policyByMetric) {
  const metrics = cell.metrics.split(",");
  if (metrics.some(metric => !policyByMetric.has(metric))) fail(`cell ${cell.cell_id} names an unknown metric`);
  return metrics;
}
function semanticDigest(entries) {
  const digest = crypto.createHash("sha256");
  for (const entry of entries) {
    const bytes = fs.readFileSync(entry.file);
    digest.update(`probe:${entry.probe_id}\ncase:${entry.case}\ninput_sha256:${entry.input_sha256}\nbytes:${bytes.length}\n`, "utf8");
    digest.update(bytes);
    digest.update("\n", "utf8");
  }
  return digest.digest("hex");
}
function assertAbsent(rawPath, label) {
  if (rawPath !== undefined && rawPath !== null && rawPath !== "-") fail(`${label} must be absent for this workload`);
}
// Adapter-reported hashes are deliberately ignored. The adapter must place
// evidence bytes in the request result directory; this process resolves and
// hashes those files before it writes any receipt.
// Required response paths are stdout_path, diagnostics_path,
// generated_source_path, artifact_path, and compiler_path. Selfcompile adds
// semantic_probes with stage2-compiler probe output/diagnostic paths.
function responseIdentity(response, cell, implementation, expectedInput, expectedClosure, expectedOutput, adapterSha, runId, pairId, resultDir, resultRoot, requestPath, canonicalProbes) {
  const required = ["cell_id", "case", "workflow", "execution_tier", "implementation", "status", "correctness", "target", "profile", "optimization", "artifact_kind", "execution_scope", "pairing", "run_id", "pair_id", "compiler_path"];
  for (const key of required) if (typeof response[key] !== "string" || response[key].length === 0) fail(`adapter receipt is missing ${key}: ${cell.cell_id}/${implementation}`);
  if (response.status !== "measured" || response.correctness !== "verified") fail(`adapter receipt is unavailable or unverified: ${cell.cell_id}/${implementation}`);
  if (response.cell_id !== cell.cell_id || response.case !== cell.case || response.workflow !== cell.workflow || response.execution_tier !== cell.execution_tier || response.implementation !== implementation) fail(`adapter workload identity disagrees: ${cell.cell_id}/${implementation}`);
  if (response.execution_scope !== "generated-target-program" || response.pairing !== "paired-same-run-v1" || response.run_id !== runId || response.pair_id !== pairId) fail(`adapter execution or pair identity disagrees: ${cell.cell_id}/${implementation}`);
  if (response.target !== cell.target || response.profile !== cell.profile || response.optimization !== cell.optimization || response.artifact_kind !== cell.artifact_kind) fail(`adapter target/profile/optimization/artifact identity disagrees: ${cell.cell_id}/${implementation}`);
  const executionHarness = cell.target === "web" ? "node-app-js-browser-runtime-v1" : "-";
  if (cell.target === "web") {
    if (response.execution_harness !== executionHarness) fail(`adapter Web execution harness identity disagrees: ${cell.cell_id}/${implementation}`);
  } else {
    assertAbsent(response.execution_harness, "Web execution harness");
  }
  const compiler = compilerFile(resultDir, response.compiler_path, resultRoot);
  const stdout = resultFile(resultDir, response.stdout_path, "target stdout", cell.case === "valid", resultRoot);
  const diagnostics = resultFile(resultDir, response.diagnostics_path, "target diagnostics", cell.case === "invalid", resultRoot);
  const generated = resultFile(resultDir, response.generated_source_path, "generated Source", cell.case !== "invalid", resultRoot);
  const artifact = resultFile(resultDir, response.artifact_path, "target artifact", cell.case !== "invalid", resultRoot);
  if (cell.case !== "valid") assertAbsent(response.stdout_path, "target stdout");
  if (cell.case !== "invalid") assertAbsent(response.diagnostics_path, "target diagnostics");
  let semanticExpectedSha = "-";
  let semanticOutputSha = "-";
  let semanticEquivalence = "-";
  let semanticProbePaths = "-";
  if (cell.case === "selfcompile") {
    assertAbsent(response.stdout_path, "selfcompile target stdout");
    assertAbsent(response.diagnostics_path, "selfcompile target diagnostics");
    if (!Array.isArray(response.semantic_probes) || response.semantic_probes.length !== canonicalProbes.length) fail(`selfcompile adapter probe set is incomplete: ${cell.cell_id}/${implementation}`);
    const probeById = new Map();
    for (const probe of response.semantic_probes) {
      if (!probe || typeof probe.probe_id !== "string" || probeById.has(probe.probe_id)) fail(`selfcompile adapter probe identity is invalid: ${cell.cell_id}/${implementation}`);
      probeById.set(probe.probe_id, probe);
    }
    const expectedEntries = [];
    const semanticPathEvidence = [];
    const actualEntries = [];
    for (const canonical of canonicalProbes) {
      const probe = probeById.get(canonical.probe_id);
      if (!probe) fail(`selfcompile adapter omitted canonical probe ${canonical.probe_id}: ${cell.cell_id}/${implementation}`);
      if (probe.case !== canonical.case || probe.expected_kind !== canonical.expected_kind || probe.execution !== "stage2-compiler" || probe.input_sha256 !== canonical.input_sha256) fail(`selfcompile adapter canonical probe execution identity disagrees: ${cell.cell_id}/${implementation}/${canonical.probe_id}`);
      const actualFile = canonical.case === "valid"
        ? resultFile(resultDir, probe.stdout_path, `${canonical.probe_id} stdout`, true, resultRoot)
        : resultFile(resultDir, probe.diagnostics_path, `${canonical.probe_id} diagnostics`, true, resultRoot);
      if (canonical.case === "valid") assertAbsent(probe.diagnostics_path, `${canonical.probe_id} diagnostics`);
      else assertAbsent(probe.stdout_path, `${canonical.probe_id} stdout`);
      if (actualFile.sha256 !== canonical.expected_sha256) fail(`selfcompile canonical probe output disagrees: ${cell.cell_id}/${implementation}/${canonical.probe_id}`);
      expectedEntries.push({ probe_id: canonical.probe_id, case: canonical.case, input_sha256: canonical.input_sha256, file: canonical.expected_path });
      actualEntries.push({ probe_id: canonical.probe_id, case: canonical.case, input_sha256: canonical.input_sha256, file: actualFile.path });
      semanticPathEvidence.push(`${canonical.probe_id}:${actualFile.relative}`);
    }
    semanticProbePaths = semanticPathEvidence.join(",");
    semanticExpectedSha = semanticDigest(expectedEntries);
    semanticOutputSha = semanticDigest(actualEntries);
    if (semanticExpectedSha !== semanticOutputSha) fail(`selfcompile canonical probe semantic digest disagrees: ${cell.cell_id}/${implementation}`);
    semanticEquivalence = "verified";
  } else {
    assertAbsent(response.semantic_probes, "semantic probes");
  }
  const outputSha = stdout?.sha256 ?? "-";
  const diagnosticSha = diagnostics?.sha256 ?? "-";
  const diagnosticCountValue = diagnostics ? diagnosticCount(diagnostics.path) : 0;
  if (cell.case === "selfcompile") {
    if (outputSha !== "-" || diagnosticSha !== "-" || diagnosticCountValue !== 0) fail(`selfcompile adapter target result fields are invalid: ${cell.cell_id}/${implementation}`);
  } else if (cell.case === "invalid") {
    if (outputSha !== "-" || diagnosticSha !== expectedOutput || diagnosticCountValue <= 0) fail(`rejected adapter diagnostics do not match the frozen bytes: ${cell.cell_id}/${implementation}`);
  } else if (outputSha !== expectedOutput || diagnosticSha !== "-" || diagnosticCountValue !== 0) {
    fail(`accepted adapter output/diagnostics do not match the frozen bytes: ${cell.cell_id}/${implementation}`);
  }
  const artifactSha = artifact?.sha256 ?? "-";
  const generatedSha = generated?.sha256 ?? "-";
  if (cell.case === "invalid") {
    if (artifactSha !== "-" || generatedSha !== "-") fail(`rejected adapter emitted an artifact: ${cell.cell_id}/${implementation}`);
  } else {
    nonzeroSha(artifactSha, "adapter artifact bytes");
    nonzeroSha(generatedSha, "adapter generated-source bytes");
  }
  const sourceBoundary = [
    `implementation=${implementation}`, `workflow=${cell.workflow}`, `execution_tier=${cell.execution_tier}`,
    "execution_scope=generated-target-program", "pairing=paired-same-run-v1", `target=${cell.target}`, `execution_harness=${executionHarness}`,
    `profile=${cell.profile}`, `optimization=${cell.optimization}`, `source_entry=${cell.source_entry}`,
    `source_input_sha256=${expectedInput}`, `source_manifest_sha256=${expectedClosure}`, `expected_sha256=${expectedOutput}`,
    `diagnostic_sha256=${diagnosticSha}`, `diagnostic_count=${diagnosticCountValue}`,
    `semantic_expected_sha256=${semanticExpectedSha}`, `semantic_output_sha256=${semanticOutputSha}`,
    `semantic_equivalence=${semanticEquivalence}`, `artifact_sha256=${artifactSha}`, `generated_source_sha256=${generatedSha}`,
  ].join(";");
  const command = "adapter-request-v2";
  const probeEvidence = cell.case === "selfcompile"
    ? canonicalProbes.map(probe => `${probe.probe_id}=${probe.case}`).join(",")
    : "-";
  const evidence = [
    "evidence_version=byte-hash-v1", `run_id=${runId}`, `pair_id=${pairId}`, "paired=source,rust-reference",
    "pairing=paired-same-run-v1", `execution_harness=${executionHarness}`, `request_path=${requestPath}`, `result_dir=${resultDir}`, `stdout_path=${stdout?.relative ?? "-"}`,
    `diagnostics_path=${diagnostics?.relative ?? "-"}`, `generated_source_path=${generated?.relative ?? "-"}`,
    `artifact_path=${artifact?.relative ?? "-"}`, `compiler_path=${compiler.path}`,
    `output_sha256=${outputSha}`, `diagnostic_sha256=${diagnosticSha}`, `diagnostic_count=${diagnosticCountValue}`,
    `semantic_expected_sha256=${semanticExpectedSha}`, `semantic_output_sha256=${semanticOutputSha}`,
    `semantic_equivalence=${semanticEquivalence}`, `artifact_sha256=${artifactSha}`,
    `generated_source_sha256=${generatedSha}`, `semantic_probes=${probeEvidence}`, `semantic_probe_paths=${semanticProbePaths}`,
  ].join(";");
  const identity = {
    status: "measured", source_input_sha256: expectedInput, source_closure_sha256: expectedClosure,
    expected_sha256: expectedOutput, output_sha256: outputSha, diagnostic_sha256: diagnosticSha,
    diagnostic_count: String(diagnosticCountValue), semantic_expected_sha256: semanticExpectedSha,
    semantic_output_sha256: semanticOutputSha, semantic_equivalence: semanticEquivalence,
    artifact_sha256: artifactSha, generated_source_sha256: generatedSha, compiler_sha256: compiler.sha256,
    target: cell.target, profile: cell.profile, optimization: cell.optimization, artifact_kind: cell.artifact_kind,
    correctness: "verified", source_boundary: sourceBoundary, command, execution_scope: "generated-target-program",
    pairing: "paired-same-run-v1", run_id: runId,
  };
  return {
    receipt: [1, cell.cell_id, cell.case, cell.workflow, cell.execution_tier, implementation, identity.status,
      identity.source_input_sha256, identity.source_closure_sha256, identity.expected_sha256, identity.output_sha256,
      identity.diagnostic_sha256, identity.diagnostic_count, identity.semantic_expected_sha256, identity.semantic_output_sha256,
      identity.semantic_equivalence, identity.artifact_sha256, identity.generated_source_sha256, identity.compiler_sha256,
      identity.target, identity.profile, identity.optimization, identity.artifact_kind, identity.correctness,
      identity.source_boundary, identity.command, adapterSha, identity.pairing, evidence],
    identity,
  };
}

const argv = process.argv.slice(2);
let reportDir = "";
for (let index = 0; index < argv.length; index += 1) {
  if (argv[index] === "--report") reportDir = argv[++index] || "";
  else if (argv[index] === "--help") {
    console.log("usage: node tools/perf/source-compiler-runner.mjs --report REPORT_DIR");
    process.exit(0);
  } else fail(`unknown argument: ${argv[index]}`);
}
if (!reportDir) fail("--report is required");
reportDir = path.isAbsolute(reportDir) ? reportDir : path.join(root, reportDir);
requireFile(contractPath, "Source compiler contract");
requireFile(policyPath, "Source compiler policy");
requireFile(gatePath, "Source compiler gate");
const contractCheck = spawnSync("bash", [gatePath, "--contract"], { cwd: root, encoding: "utf8", maxBuffer: 2 * 1024 * 1024 });
if (contractCheck.status !== 0) fail(`frozen Source compiler contract rejected: ${cleanText(contractCheck.stderr || contractCheck.stdout)}`);

const adapterConfigured = process.env.JET_SOURCE_COMPILER_PERF_ADAPTER || "";
if (!adapterConfigured) fail("private Foundation adapter is unavailable; set JET_SOURCE_COMPILER_PERF_ADAPTER to the real Performance adapter");
const adapter = path.resolve(adapterConfigured);
requireFile(adapter, "private Foundation adapter");
if (process.platform !== "win32" && !(fs.statSync(adapter).mode & 0o111)) fail(`private Foundation adapter is not executable: ${adapter}`);
const adapterSha = sha256File(adapter);
const adapterProbe = spawnSync(adapter, ["--contract"], { cwd: root, encoding: "utf8", maxBuffer: 1024 * 1024 });
if (adapterProbe.error || adapterProbe.status !== 0 || cleanText(adapterProbe.stdout) !== adapterContract) fail(`private Foundation adapter contract probe failed: ${adapter}`);

const policyRows = table(policyPath, policyHeader, 10);
const policyByMetric = new Map(policyRows.map(row => [row[1], { unit: row[2], samples: Number(row[4]) }]));
if (policyByMetric.size !== 5 || [...policyByMetric.values()].some(policy => policy.samples !== 5)) fail("Source compiler policy sample contract is incomplete");
const cells = table(contractPath, contractHeader, 15).map(row => ({
  version: row[0], cell_id: row[1], case: row[2], workflow: row[3], execution_tier: row[4], input: row[5], expected: row[6], source_entry: row[7], source_manifest: row[8], outcome: row[9], target: row[10], profile: row[11], optimization: row[12], artifact_kind: row[13], metrics: row[14],
}));
if (cells.length !== 20) fail(`Source compiler contract has ${cells.length} cells; expected 20`);
const sourceManifestPath = path.join(root, "Compiler", "Bootstrap", "sources.list");
const sourceManifestSha = sha256File(sourceManifestPath);
const canonicalProbeSpecs = [
  { probe_id: "valid-hello", case: "valid", expected_kind: "stdout", input: "examples/features/basics/hello.jet", expected: "examples/features/expected/basics/hello.out" },
  { probe_id: "invalid-missing-return", case: "invalid", expected_kind: "diagnostics", input: "tests/ui/missing_return.jet", expected: "tests/ui/missing_return.stderr" },
];
const canonicalProbes = canonicalProbeSpecs.map(spec => {
  const contractProbe = cells.find(cell => cell.case === spec.case && cell.input === spec.input && cell.expected === spec.expected);
  if (!contractProbe) fail(`frozen selfcompile canonical probe is absent from the contract: ${spec.probe_id}`);
  const inputPath = path.join(root, spec.input);
  const expectedPath = path.join(root, spec.expected);
  return { ...spec, input_path: inputPath, input_sha256: sha256File(inputPath), expected_path: expectedPath, expected_sha256: sha256File(expectedPath) };
});
const commit = commitIdentity();
const scratchRoot = path.resolve(process.env.JET_SOURCE_COMPILER_PERF_SCRATCH || process.env.JET_PERF_SCRATCH_ROOT || path.join(os.homedir(), ".cache", "jet-perf"));
rejectRamScratch(scratchRoot);
fs.mkdirSync(scratchRoot, { recursive: true });
const work = fs.mkdtempSync(path.join(scratchRoot, "source-compiler-perf-"));
const preserve = process.env.JET_SOURCE_COMPILER_PERF_KEEP_SCRATCH === "1";
if (!preserve) process.on("exit", () => { fs.rmSync(work, { recursive: true, force: true }); });
const requests = path.join(work, "requests");
const results = path.join(work, "results");
fs.mkdirSync(requests, { recursive: true });
fs.mkdirSync(results, { recursive: true });
const runId = `source-compiler-${Date.now()}-${process.pid}`;

const sampleRows = [];
const receiptRows = [];
for (const cell of cells) {
  const expectedInput = sha256File(path.join(root, cell.input));
  const expectedClosure = sha256File(path.join(root, cell.source_manifest));
  const expectedOutput = cell.expected === "canonical-stage2-probes" ? "-" : sha256File(path.join(root, cell.expected));
  const metrics = expectedMetrics(cell, policyByMetric);
  for (const implementation of ["source", "rust-reference"]) {
    const key = `${cell.cell_id}|${implementation}`;
    for (let sample = 1; sample <= 5; sample += 1) {
      const pairId = `${runId}|${cell.cell_id}|${sample}`;
      const resultDir = path.join(results, cell.cell_id, implementation, String(sample));
      fs.mkdirSync(resultDir, { recursive: true });
      const resultRoot = fs.realpathSync(resultDir);
      const requestPath = path.join(requests, `${cell.cell_id}-${implementation}-${sample}.json`);
      const request = {
        protocol: adapterContract,
        cell_id: cell.cell_id,
        case: cell.case,
        workflow: cell.workflow,
        execution_tier: cell.execution_tier,
        execution_scope: "generated-target-program",
        pairing: "paired-same-run-v1",
        run_id: runId,
        pair_id: pairId,
        implementation,
        sample,
        sample_count: 5,
        input: path.join(root, cell.input),
        source_entry: path.join(root, cell.source_entry),
        source_manifest: path.join(root, cell.source_manifest),
        result_dir: resultDir,
        outcome: cell.outcome,
        target: cell.target,
        profile: cell.profile,
        optimization: cell.optimization,
        artifact_kind: cell.artifact_kind,
        ...(cell.target === "web" ? { execution_harness: "node-app-js-browser-runtime-v1" } : {}),
        source_input_sha256: expectedInput,
        source_manifest_sha256: expectedClosure,
        ...(cell.case === "selfcompile" ? {
          semantic_probes: canonicalProbes.map(probe => ({
            probe_id: probe.probe_id,
            case: probe.case,
            expected_kind: probe.expected_kind,
            execution: "stage2-compiler",
            input: probe.input_path,
            input_sha256: probe.input_sha256,
          })),
        } : {}),
      };
      fs.writeFileSync(requestPath, `${JSON.stringify(request)}\n`);
      const response = runAdapter(adapter, requestPath, {
        ...process.env,
        LC_ALL: "C",
        LANG: "C",
        JET_SOURCE_COMPILER_PERF_CONTRACT: adapterContract,
        JET_SOURCE_COMPILER_PERF_REQUEST: requestPath,
      });
      const validated = responseIdentity(response, cell, implementation, expectedInput, expectedClosure, expectedOutput, adapterSha, runId, pairId, resultDir, resultRoot, requestPath, canonicalProbes);
      if (!receiptByKey.has(key)) {
        receiptByKey.set(key, validated.identity);
        receiptRows.push(validated.receipt);
      } else {
        const prior = receiptByKey.get(key);
        for (const field of ["status", "source_input_sha256", "source_closure_sha256", "expected_sha256", "output_sha256", "diagnostic_sha256", "diagnostic_count", "semantic_expected_sha256", "semantic_output_sha256", "semantic_equivalence", "artifact_sha256", "generated_source_sha256", "compiler_sha256", "target", "profile", "optimization", "artifact_kind", "correctness", "source_boundary", "command", "execution_scope", "pairing", "run_id"]) {
          if (validated.identity[field] !== prior[field]) fail(`adapter identity changed across samples: ${cell.cell_id}/${implementation}/${field}`);
        }
      }
      for (const metric of metrics) {
        const value = response.metrics?.[metric];
        if (!Number.isFinite(value) || value <= 0) fail(`adapter metric is unavailable or invalid: ${cell.cell_id}/${implementation}/${metric}`);
        sampleRows.push([1, cell.cell_id, cell.case, cell.workflow, cell.execution_tier, implementation, metric, sample, pairId, String(value), policyByMetric.get(metric).unit, key, `evidence_version=byte-hash-v1;adapter=${adapterContract};run_id=${runId};pair_id=${pairId};paired=source,rust-reference;pairing=paired-same-run-v1`]);
      }
    }
  }
}

const identityRows = [
  [1, "contract_sha256", contractHash()],
  [1, "candidate_commit", commit],
  [1, "machine", `${os.platform()}-${os.arch()}-${cleanText(os.release()).replaceAll(" ", "_")}`],
  [1, "environment", "os=linux;target=x86_64-unknown-linux-gnu,web;locale=C;network=disabled"],
  [1, "adapter_path", adapter],
  [1, "adapter_version", adapterContract],
  [1, "adapter_sha256", adapterSha],
  [1, "sample_count", "5"],
  [1, "source_manifest_sha256", sourceManifestSha],
  [1, "target_set", "x86_64-unknown-linux-gnu,web"],
  [1, "implementation_pair", "source,rust-reference"],
  [1, "workflows", "factory,runner"],
  [1, "execution_tiers", "aot,cranelift-jit,source-interpreter-deopt"],
  [1, "execution_scope", "generated-target-program"],
  [1, "pairing", "paired-same-run-v1"],
  [1, "run_id", runId],
  [1, "ratio_policy", "strict-lt-1.00"],
];
writeRowsAtomic(path.join(reportDir, "identity.tsv"), identityHeader, identityRows);
writeRowsAtomic(path.join(reportDir, "samples.tsv"), sampleHeader, sampleRows);
writeRowsAtomic(path.join(reportDir, "receipts.tsv"), receiptHeader, receiptRows);
const checked = spawnSync("bash", [gatePath, "--check", reportDir], { cwd: root, encoding: "utf8", maxBuffer: 4 * 1024 * 1024 });
if (checked.status !== 0) fail(`strict Source compiler gate rejected report: ${cleanText(checked.stderr || checked.stdout)}`);
console.log(`source compiler report: pass report=${reportDir} cells=${cells.length} adapter=${adapter}`);
