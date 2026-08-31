#!/usr/bin/env node
// Card #1414: execute the frozen seven-workload corpus and emit a complete
// report. This file is a producer only. The report's review fields are filled
// by an independent current-commit/fairness review before release checking.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");
const corpus = path.join(root, "tests", "compiled_workloads");
const contractScript = path.join(root, "tools", "ci", "compiled-workload-gate.sh");
let platform = process.env.JET_COMPILED_WORKLOAD_PLATFORM || "";
let reportDir = "";
for (let i = 2; i < process.argv.length; i += 1) {
  if (process.argv[i] === "--platform") platform = process.argv[++i] || "";
  else if (process.argv[i] === "--report-dir") reportDir = process.argv[++i] || "";
  else {
    console.error("usage: compiled-workload-runner.mjs --platform linux|macos|windows --report-dir DIR");
    process.exit(64);
  }
}
if (!["linux", "macos", "windows"].includes(platform) || !reportDir) {
  console.error("usage: compiled-workload-runner.mjs --platform linux|macos|windows --report-dir DIR");
  process.exit(64);
}
const hostPlatform = process.platform === "linux" ? "linux" :
  process.platform === "darwin" ? "macos" :
    process.platform === "win32" ? "windows" : "";
if (platform !== hostPlatform) fail("report platform does not match the host: " + platform + " (host=" + hostPlatform + ")");
reportDir = path.isAbsolute(reportDir) ? reportDir : path.join(root, reportDir);

const jetBin = path.resolve(process.env.JET_COMPILED_WORKLOAD_JET_BIN || path.join(root, "target", "debug", process.platform === "win32" ? "jet.exe" : "jet"));
const exeSuffix = process.platform === "win32" ? ".exe" : "";
const scratchRoot = process.env.JET_COMPILED_WORKLOAD_SCRATCH || path.join(os.homedir(), ".cache", "jet-test-scratch");
const scratchPath = path.resolve(scratchRoot);
const tmpPath = path.resolve(os.tmpdir());
if (scratchPath === tmpPath || scratchPath.startsWith(tmpPath + path.sep)) fail("scratch must not use the RAM-backed system temporary directory: " + scratchRoot);
fs.mkdirSync(scratchRoot, { recursive: true });
const work = fs.mkdtempSync(path.join(scratchRoot, "compiled-workload-runner-"));
const preserveScratch = process.env.JET_COMPILED_WORKLOAD_KEEP_SCRATCH === "1";
process.on("exit", () => {
  if (preserveScratch) return;
  try {
    fs.rmSync(work, { recursive: true, force: true });
  } catch {
    // Preserve the report even when a failed run cannot clean its scratch tree.
  }
});
if (fs.existsSync(reportDir)) {
  if (!fs.statSync(reportDir).isDirectory()) fail("report path is not a directory: " + reportDir);
  if (fs.readdirSync(reportDir).length > 0) fail("report directory must be empty: " + reportDir);
} else {
  fs.mkdirSync(reportDir, { recursive: true });
}
fs.mkdirSync(path.join(work, "runs"), { recursive: true });

function fail(message) {
  console.error("compiled workload runner: " + message);
  process.exit(1);
}
const contract = spawnSync("bash", [contractScript, "--contract"], {
  cwd: root,
  encoding: "utf8",
});
if (contract.error || contract.status !== 0) {
  fail("frozen contract rejected: " + cleanText(contract.stderr || contract.stdout || contract.error?.message));
}
function requireFile(file, label) {
  try {
    if (!fs.statSync(file).isFile()) fail("missing " + label + ": " + file);
  } catch {
    fail("missing " + label + ": " + file);
  }
}
function table(file) {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(line => line.length > 0);
  const header = lines.shift().split("\t");
  return lines.map(line => {
    const values = line.split("\t");
    if (values.length !== header.length) fail("row width drifted in " + file);
    return Object.fromEntries(header.map((key, index) => [key, values[index]]));
  });
}
function hashFile(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
function relativeFiles(relativePaths) {
  return relativePaths.map(file => path.relative(root, file).replaceAll(path.sep, "/")).sort();
}
function hashRelativeFiles(relativePaths) {
  const hash = crypto.createHash("sha256");
  for (const relative of relativeFiles(relativePaths)) {
    hash.update(relative);
    hash.update("\0");
    hash.update(fs.readFileSync(path.join(root, relative)));
    hash.update("\0");
  }
  return hash.digest("hex");
}
function walkFiles(dir) {
  const result = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) result.push(...walkFiles(file));
    else if (entry.isFile()) result.push(file);
  }
  return result;
}
function fileSize(file) {
  return fs.statSync(file).size;
}
function lineCount(file) {
  const text = fs.readFileSync(file, "utf8");
  return text.length === 0 ? 0 : text.split(/\r?\n/).length - (text.endsWith("\n") ? 1 : 0);
}
function cleanText(value) {
  return String(value || "").replace(/[\t\r\n]+/g, " ").trim();
}
function commandText(command, args) {
  return [command, ...args].map(value => JSON.stringify(String(value))).join(" ");
}
function nowNs() {
  return process.hrtime.bigint();
}

const timeCandidates = ["/usr/bin/time", "/bin/time", "/run/current-system/sw/bin/time"];
const timeBin = process.env.JET_TIME_BIN || timeCandidates.find(file => fs.existsSync(file)) || "";
if (!timeBin) fail("external time with peak RSS output is required");
let runNumber = 0;
function runMeasured(command, args, cwd, extraEnv = {}) {
  const runId = String(runNumber++);
  const rssPath = path.join(work, "runs", runId + ".rss");
  const started = nowNs();
  const result = spawnSync(
    timeBin,
    ["-q", "-f", "%M", "-o", rssPath, command, ...args],
    {
      cwd,
      env: { ...process.env, ...extraEnv, LC_ALL: "C", LANG: "C" },
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  const duration = Number(nowNs() - started);
  if (result.error) fail("could not run " + commandText(command, args) + ": " + result.error.message);
  let rss = "";
  try {
    rss = fs.readFileSync(rssPath, "utf8").trim();
  } catch {
    fail("RSS measurement missing for " + commandText(command, args));
  }
  if (!/^[0-9]+$/.test(rss)) fail("RSS measurement is not numeric for " + commandText(command, args));
  return {
    status: result.status === null ? 1 : result.status,
    stdout: Buffer.isBuffer(result.stdout) ? result.stdout : Buffer.from(result.stdout || ""),
    stderr: Buffer.isBuffer(result.stderr) ? result.stderr : Buffer.from(result.stderr || ""),
    duration,
    rss: Number(rss),
    command: commandText(command, args),
  };
}
function checked(result, expected, label) {
  if (result.status !== 0) fail(label + " exited " + result.status + ": " + cleanText(result.stderr));
  const expectedBytes = fs.readFileSync(expected);
  if (!result.stdout.equals(expectedBytes)) {
    fail(label + " output differs from " + expected);
  }
}
function toolVersion(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0) fail("tool version failed: " + command);
  return cleanText(result.stdout);
}
function sourceExtension(relative) {
  const extension = path.extname(relative);
  return extension || ".src";
}
function writeWorkloadPackage(project) {
  fs.writeFileSync(
    path.join(project, "package.jet"),
    "name: \"compiled-workload\"\n" +
      "version: \"0.1.0\"\n" +
      "authority: .{ holds: { allow: [Browser, FS, IO, Mem.Alloc, Net, Panic] } }\n",
  );
}

const manifest = table(path.join(corpus, "manifest.tsv"));
const peers = table(path.join(corpus, "peer_ledger.tsv"));
const adapters = table(path.join(corpus, "adapter_ledger.tsv"));
const metricContract = table(path.join(corpus, "metric_contract.tsv"));
const policyRows = table(path.join(corpus, "measurement_policy.tsv"));
const tierMatrix = table(path.join(corpus, "tier_matrix.tsv"));
const selectedPeers = new Map(peers.filter(row => row.selection === "best-applicable").map(row => [row.task_id, row]));
const adapterByTask = new Map(adapters.map(row => [row.task_id, row]));
const taskById = new Map(manifest.map(row => [row.task_id, row]));
const metricUnits = new Map(metricContract.map(row => [row.metric, row.unit]));
const policies = new Map(policyRows.map(row => [row.metric, row]));
const sampleCounts = new Set(policyRows.map(row => Number(row.samples)));
if (sampleCounts.size !== 1 || !Number.isInteger([...sampleCounts][0]) || [...sampleCounts][0] < 1) fail("measurement policy must use one positive sample count");
const sampleCount = [...sampleCounts][0];
if (manifest.length !== 7 || selectedPeers.size !== 7 || adapterByTask.size !== 7) fail("frozen task/peer/adapter count is incomplete");

for (const row of manifest) {
  requireFile(path.join(corpus, row.input), "input fixture");
  requireFile(path.join(corpus, row.expected), "expected fixture");
  const adapter = adapterByTask.get(row.task_id);
  const peer = selectedPeers.get(row.task_id);
  if (!adapter || !peer) fail("missing selected adapter or peer: " + row.task_id);
  if (peer.source_revision !== adapter.peer_commit || !/^[0-9a-f]{40}$/.test(adapter.peer_commit)) fail("peer identity is not immutable: " + row.task_id);
  requireFile(path.join(corpus, adapter.jet_source), "Jet adapter");
  requireFile(path.join(corpus, adapter.jet_hostile), "Jet hostile fixture");
  requireFile(path.join(corpus, adapter.peer_source), "peer adapter");
  requireFile(path.join(corpus, adapter.peer_hostile), "peer hostile fixture");
  requireFile(path.join(corpus, "expected", row.task_id + ".hostile.out"), "hostile expected output");
}

const candidateCommitResult = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
if (candidateCommitResult.status !== 0) fail("git rev-parse HEAD failed");
const candidateCommit = candidateCommitResult.stdout.trim();
if (!/^[0-9a-f]{40}$/.test(candidateCommit)) fail("candidate commit is not immutable");
const environment = "os=" + platform + ";ci=compiled-workload;locale=C;network=disabled";
const machine = os.platform() + "-" + os.arch() + "-" + cleanText(os.release()).replaceAll(" ", "_");
if (!fs.existsSync(jetBin)) fail("Jet compiler not found: " + jetBin);
const jetToolVersion = toolVersion(jetBin, ["--version"]);
const contractFiles = [
  path.join(corpus, "manifest.tsv"),
  path.join(corpus, "domain_contract.tsv"),
  path.join(corpus, "peer_ledger.tsv"),
  path.join(corpus, "metric_contract.tsv"),
  path.join(corpus, "tier_matrix.tsv"),
  path.join(corpus, "canaries.tsv"),
  path.join(corpus, "adapter_ledger.tsv"),
  path.join(corpus, "measurement_policy.tsv"),
];
const sourceFiles = [
  ...walkFiles(path.join(corpus, "adapters")),
  ...walkFiles(path.join(corpus, "fixtures")),
  ...walkFiles(path.join(corpus, "expected")),
  ...walkFiles(path.join(corpus, "task-definitions")),
];
const contractHash = hashRelativeFiles(contractFiles);
const sourceClosureHash = hashRelativeFiles(sourceFiles);
const identity = [
  ["1", "candidate_commit", candidateCommit],
  ["1", "platform", platform],
  ["1", "environment", environment],
  ["1", "machine", machine],
  ["1", "jet_tool_version", jetToolVersion],
  ["1", "contract_sha256", contractHash],
  ["1", "source_closure_sha256", sourceClosureHash],
  ["1", "samples", String(sampleCount)],
  ["1", "peer_commits", manifest.map(row => row.task_id + ":" + selectedPeers.get(row.task_id).source_revision).join(",")],
];

const samples = [];
const receipts = [];
const tierReceipts = [];
const tierReports = [];
const outcomes = [];
const toolchainByKey = new Map();
function addSamples(task, language, metric, values, unit, method) {
  if (values.length !== sampleCount) fail("sample count does not match policy: " + task + "/" + language + "/" + metric);
  values.forEach((value, index) => samples.push(["1", task, language, metric, String(index + 1), String(value), unit, method]));
}
function artifactHash(file) {
  return hashFile(file);
}
function jetBuild(project, source, target = "") {
  fs.mkdirSync(project, { recursive: true });
  writeWorkloadPackage(project);
  fs.copyFileSync(path.join(corpus, source), path.join(project, "main.jet"));
  const args = ["build", "--profile=release"];
  if (target) args.push("--target=" + target);
  args.push("main.jet");
  const result = runMeasured(jetBin, args, project);
  if (result.status !== 0) fail("Jet build failed for " + source + ": " + cleanText(result.stderr));
  const binary = path.join(project, "build", "main" + exeSuffix);
  if (!target || target !== "web") requireFile(binary, "Jet build artifact");
  return { result, binary, args };
}
function jetWebBuild(project, source) {
  const build = jetBuild(project, source, "web");
  const js = path.join(project, "build", "app.js");
  const wasm = path.join(project, "build", "app.wasm");
  requireFile(js, "Jet web JavaScript");
  requireFile(wasm, "Jet web Wasm");
  return { ...build, js, wasm };
}
function peerBuild(language, source, dir, crossTarget = "") {
  fs.mkdirSync(dir, { recursive: true });
  const output = path.join(dir, "peer" + exeSuffix);
  const absolute = path.join(corpus, source);
  let command;
  let args;
  let env = {};
  if (language === "rust") {
    command = "rustc";
    args = ["--edition=2021", "-O"];
    if (crossTarget) args.push("--target=" + crossTarget, "-C", "linker=aarch64-linux-gnu-gcc");
    args.push(absolute, "-o", output);
  } else if (language === "go") {
    command = "go";
    args = ["build", "-trimpath", "-o", output, absolute];
    if (crossTarget) env = { CGO_ENABLED: "0", GOOS: "linux", GOARCH: "arm64" };
  } else if (language === "cxx") {
    command = process.env.CXX || "c++";
    if (crossTarget) command = process.env.CXX_CROSS || "aarch64-linux-gnu-g++";
    args = ["-std=c++20", "-O2", "-pipe", absolute, "-o", output];
  } else if (language === "zig") {
    command = "zig";
    args = ["build-exe", "-O", "ReleaseFast"];
    if (crossTarget) args.push("-target", "aarch64-linux-gnu");
    args.push(absolute, "-femit-bin=" + output);
  } else if (language === "domain") {
    if (crossTarget) fail("domain peer declared for aarch64");
    command = "node";
    args = ["--check", absolute];
  } else {
    fail("unsupported peer language: " + language);
  }
  const result = runMeasured(command, args, root, env);
  if (result.status !== 0) fail("peer build failed for " + source + ": " + cleanText(result.stderr));
  const artifact = language === "domain" ? absolute : output;
  requireFile(artifact, "peer build artifact");
  return { result, artifact, command, args };
}
function peerRun(language, artifact, input, cwd) {
  if (language === "domain") return runMeasured("node", [artifact, input], cwd);
  return runMeasured(artifact, [input], cwd);
}
function lineCountBuffer(buffer) {
  const text = buffer.toString("utf8");
  return text.length === 0 ? 0 : text.split(/\r?\n/).length - (text.endsWith("\n") ? 1 : 0);
}

for (const task of manifest) {
  const taskId = task.task_id;
  const adapter = adapterByTask.get(taskId);
  const peer = selectedPeers.get(taskId);
  const taskWork = path.join(work, taskId);
  fs.mkdirSync(taskWork, { recursive: true });
  const input = path.join(corpus, task.input);
  const expected = path.join(corpus, task.expected);
  const hostileInput = path.join(corpus, adapter.jet_hostile);
  const hostileExpected = path.join(corpus, "expected", taskId + ".hostile.out");
  let jetAot = null;
  let peerAot = null;
  let jetNormal = null;
  let peerNormal = null;
  let jetHostile = null;
  let peerHostile = null;
  let jetTool = jetToolVersion;
  let peerTool = "";
  toolchainByKey.set(taskId + "\tjet", jetTool);

  for (const language of ["jet", "peer"]) {
    const sampleLanguage = language === "jet" ? "jet" : peer.language;
    const source = language === "jet" ? adapter.jet_source : adapter.peer_source;
    const sourcePath = path.join(corpus, source);
    const sourceLines = lineCount(sourcePath);
    const unsafeCount = (fs.readFileSync(sourcePath, "utf8").match(/unsafe/g) || []).length;
    const buildValues = [];
    const editValues = [];
    let artifact = "";
    let lastBuildCommand = "";
    for (let sample = 1; sample <= sampleCount; sample += 1) {
      if (language === "jet") {
        const build = jetBuild(path.join(taskWork, "jet-build-" + sample), source);
        artifact = build.binary;
        buildValues.push(build.result.duration);
        lastBuildCommand = commandText(jetBin, build.args);
        const editProject = path.join(taskWork, "jet-edit-" + sample);
        fs.mkdirSync(editProject, { recursive: true });
        writeWorkloadPackage(editProject);
        const edited = path.join(editProject, "main.jet");
        fs.copyFileSync(sourcePath, edited);
        fs.appendFileSync(edited, "\n// measured source edit\n");
        const edit = runMeasured(jetBin, ["build", "--profile=release", "main.jet"], editProject);
        if (edit.status !== 0) fail("Jet source-edit rebuild failed: " + taskId);
        editValues.push(edit.duration);
      } else {
        const build = peerBuild(peer.language, source, path.join(taskWork, "peer-build-" + sample));
        artifact = build.artifact;
        buildValues.push(build.result.duration);
        lastBuildCommand = commandText(build.command, build.args);
        const editDir = path.join(taskWork, "peer-edit-" + sample);
        fs.mkdirSync(editDir, { recursive: true });
        const edited = path.join(editDir, "source" + sourceExtension(source));
        fs.copyFileSync(sourcePath, edited);
        fs.appendFileSync(edited, "\n// measured source edit\n");
        let editCommand = build.command;
        let editArgs = [];
        if (peer.language === "rust") editArgs = ["--edition=2021", "-O", edited, "-o", path.join(editDir, "edit" + exeSuffix)];
        else if (peer.language === "go") editArgs = ["build", "-trimpath", "-o", path.join(editDir, "edit" + exeSuffix), edited];
        else if (peer.language === "cxx") editArgs = ["-std=c++20", "-O2", "-pipe", edited, "-o", path.join(editDir, "edit" + exeSuffix)];
        else if (peer.language === "zig") editArgs = ["build-exe", "-O", "ReleaseFast", edited, "-femit-bin=" + path.join(editDir, "edit" + exeSuffix)];
        else {
          editCommand = "node";
          editArgs = ["--check", edited];
        }
        const edit = runMeasured(editCommand, editArgs, root);
        if (edit.status !== 0) fail("peer source-edit rebuild failed: " + taskId);
        editValues.push(edit.duration);
      }
    }
    addSamples(taskId, sampleLanguage, "source_effort", Array(sampleCount).fill(sourceLines), "count", "source-lines");
    addSamples(taskId, sampleLanguage, "build_time", buildValues, "nanoseconds", "cold-build");
    addSamples(taskId, sampleLanguage, "edit_time", editValues, "nanoseconds", "source-edit-rebuild");
    addSamples(taskId, sampleLanguage, "artifact_size", Array(sampleCount).fill(fileSize(artifact)), "bytes", "artifact-size");
    addSamples(taskId, sampleLanguage, "unsafe_burden", Array(sampleCount).fill(unsafeCount), "count", "unsafe-token-scan");

    const runtimeValues = [];
    const memoryValues = [];
    let firstOutputHash = "";
    let normalCommand = "";
    for (let sample = 1; sample <= sampleCount; sample += 1) {
      let result;
      if (language === "jet") result = runMeasured(artifact, [input], taskWork);
      else result = peerRun(peer.language, artifact, input, root);
      checked(result, expected, taskId + "/" + language + "/native");
      runtimeValues.push(result.duration);
      memoryValues.push(result.rss);
      if (sample === 1) {
        firstOutputHash = crypto.createHash("sha256").update(result.stdout).digest("hex");
        normalCommand = result.command;
      }
      if (language === "jet") jetNormal = result;
      else peerNormal = result;
    }
    addSamples(taskId, sampleLanguage, "runtime", runtimeValues, "nanoseconds", "native-run");
    addSamples(taskId, sampleLanguage, "memory", memoryValues, "bytes", "peak-rss");

    let hostileResult;
    if (language === "jet") hostileResult = runMeasured(artifact, [hostileInput], taskWork);
    else hostileResult = peerRun(peer.language, artifact, path.join(corpus, adapter.peer_hostile), root);
    checked(hostileResult, hostileExpected, taskId + "/" + language + "/hostile");
    addSamples(taskId, sampleLanguage, "diagnostics", Array(sampleCount).fill(lineCountBuffer(hostileResult.stdout) + lineCountBuffer(hostileResult.stderr)), "count-and-review", "hostile-output-lines");
    addSamples(taskId, sampleLanguage, "debugging", Array(sampleCount).fill(hostileResult.duration), "nanoseconds-and-steps", "hostile-replay");
    const deploymentValues = [];
    for (let sample = 1; sample <= sampleCount; sample += 1) {
      const destination = path.join(taskWork, "deploy-" + language + "-" + sample, "artifact");
      fs.mkdirSync(path.dirname(destination), { recursive: true });
      const started = nowNs();
      fs.copyFileSync(artifact, destination);
      deploymentValues.push(Number(nowNs() - started));
    }
    addSamples(taskId, sampleLanguage, "deployment", deploymentValues, "count-and-time", "package-copy");
    if (language === "jet") {
      jetAot = artifact;
      jetHostile = hostileResult;
      receipts.push(["1", taskId, "jet", hashFile(sourcePath), hashFile(input), hashFile(expected), firstOutputHash, hashFile(hostileInput), hashFile(path.join(corpus, "expected", taskId + ".hostile.out")), environment, machine, jetTool, "aot=" + normalCommand + ";jit=jet run " + source + " -- " + input, peer.source_revision, "0", String(hostileResult.status)]);
    } else {
      peerAot = artifact;
      peerHostile = hostileResult;
      peerTool = peer.language === "rust" ? toolVersion("rustc", ["-Vv"]) :
        peer.language === "go" ? toolVersion("go", ["version"]) :
        peer.language === "cxx" ? toolVersion(process.env.CXX || "c++", ["--version"]) :
        peer.language === "zig" ? "zig " + toolVersion("zig", ["version"]) : "node " + toolVersion("node", ["--version"]);
      toolchainByKey.set(taskId + "\t" + peer.language, peerTool);
      receipts.push(["1", taskId, peer.language, hashFile(sourcePath), hashFile(input), hashFile(expected), firstOutputHash, hashFile(path.join(corpus, adapter.peer_hostile)), hashFile(hostileExpected), environment, machine, peerTool, "build=" + lastBuildCommand + ";run=" + normalCommand, peer.source_revision, "0", String(hostileResult.status)]);
    }
  }

  outcomes.push(["1", taskId, peer.language, peer.program, task.input, task.expected, task.declared_outcome, "platform=" + platform + ";candidate=" + candidateCommit, jetToolVersion, peerTool, peer.dependency_rule, peer.source_boundary, "pass", "pass", "-", "pending", "-"]);

  let jetJitOutputHash = "-";
  let jetJitCommand = "not-run";
  if (platform === "linux") {
    const jitProject = path.join(taskWork, "jet-build-5");
    const jit = runMeasured(jetBin, ["run", "main.jet", "--", input], jitProject);
    checked(jit, expected, taskId + "/jet/jit");
    jetJitOutputHash = crypto.createHash("sha256").update(jit.stdout).digest("hex");
    jetJitCommand = jit.command;
  }
  let jetCrossHash = "-";
  let peerCrossHash = "-";
  let jetWebHash = "-";
  let jetWebOutputHash = "-";
  let peerWebHash = "-";
  let peerWebOutputHash = "-";
  if (platform === "linux") {
    if (taskId === "cross-platform-notes") {
      fail("cross-platform-notes web peer requires a browser artifact and browser execution receipt");
    } else {
      const jetCross = jetBuild(path.join(taskWork, "jet-cross"), adapter.jet_source, "aarch64-unknown-linux-gnu");
      const peerCross = peerBuild(peer.language, adapter.peer_source, path.join(taskWork, "peer-cross"), "aarch64-unknown-linux-gnu");
      jetCrossHash = artifactHash(jetCross.binary);
      peerCrossHash = artifactHash(peerCross.artifact);
    }
  }

  for (const tier of tierMatrix.filter(row => row.task_id === taskId)) {
    const excluded = tier.requirement === "excluded";
    const inScope = tier.platform === platform || (platform === "linux" && tier.platform === "cross-target");
    const evidencePrefix = excluded ? "excluded:" + tier.rationale : !inScope ? "platform-scope=" + platform : "";
    const languages = ["jet", peer.language];
    for (const language of languages) {
      let status = "not-applicable";
      let artifact = "-";
      let output = "-";
      let command = "not-run:" + evidencePrefix;
      if (!excluded && inScope && tier.tier === "jit" && language === "jet") {
        status = "pass";
        artifact = jetJitOutputHash;
        output = jetJitOutputHash;
        command = jetJitCommand;
      } else if (!excluded && inScope && tier.tier === "jit" && language !== "jet") {
        command = "not-run:peer-native-only";
      } else if (!excluded && inScope && tier.platform === "cross-target" && tier.target === "web") {
        status = "pass";
        artifact = language === "jet" ? jetWebHash : peerWebHash;
        output = language === "jet" ? jetWebOutputHash : peerWebOutputHash;
        command = language === "jet" ? "jet build --target=web;node app.js" : "node " + adapter.peer_source;
      } else if (!excluded && inScope && tier.platform === "cross-target") {
        status = "pass";
        artifact = language === "jet" ? jetCrossHash : peerCrossHash;
        command = language === "jet" ? "jet build --target=" + tier.target : "peer build --target=" + tier.target;
      } else if (!excluded && inScope && tier.tier === "aot") {
        status = "pass";
        artifact = artifactHash(language === "jet" ? jetAot : peerAot);
        output = crypto.createHash("sha256").update(language === "jet" ? jetNormal.stdout : peerNormal.stdout).digest("hex");
        command = language === "jet" ? "jet build;run" : "peer build;run";
      }
      const evidence = status === "pass" ? "artifact=" + artifact + ";output=" + output + ";command=" + command : command;
      tierReports.push(["1", taskId, language, tier.platform, tier.target, tier.tier, status, evidence, "-"]);
      tierReceipts.push(["1", taskId, language, tier.platform, tier.target, tier.tier, artifact, output, command, status]);
    }
  }
}

const groups = new Map();
for (const row of samples) {
  const key = row[1] + "\t" + row[2] + "\t" + row[3];
  if (!groups.has(key)) groups.set(key, { task: row[1], language: row[2], metric: row[3], unit: row[6], values: [] });
  groups.get(key).values.push(Number(row[5]));
}
function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}
const statistics = [];
for (const group of groups.values()) {
  const policy = policies.get(group.metric);
  if (!policy) fail("measurement policy missing: " + group.metric);
  const values = group.values;
  const sampleLimit = Number(policy.samples);
  const minSamples = Number(policy.min_samples);
  const threshold = Number(policy.min_value);
  if (values.length !== sampleLimit || values.length < minSamples || values.some(value => !Number.isFinite(value) || value < threshold)) fail("sample threshold failed: " + group.task + "/" + group.language + "/" + group.metric);
  const med = median(values);
  const mean = values.reduce((a, b) => a + b, 0) / values.length;
  const stdev = Math.sqrt(values.reduce((a, b) => a + (b - mean) ** 2, 0) / values.length);
  const relative = mean === 0 ? 0 : stdev / Math.abs(mean);
  const mad = median(values.map(value => Math.abs(value - med)));
  const outlierLimit = mad === 0 ? 0 : mad * Number(policy.outlier_mad_multiplier);
  const outliers = values.filter(value => mad === 0 ? value !== med : Math.abs(value - med) > outlierLimit).length;
  if (relative > Number(policy.max_relative_stdev) || outliers > Number(policy.max_outliers)) fail("variance/outlier policy failed: " + group.task + "/" + group.language + "/" + group.metric);
  statistics.push({ ...group, samples: values.length, median: med, min: Math.min(...values), max: Math.max(...values), relative, outliers, threshold, tolerance: Number(policy.tolerance_ratio), status: "measured", owner: "-" });
}
const statsByKey = new Map(statistics.map(stat => [stat.task + "\t" + stat.language + "\t" + stat.metric, stat]));
for (const stat of statistics) {
  if (stat.language !== "jet") continue;
  const peer = statsByKey.get(stat.task + "\t" + selectedPeers.get(stat.task).language + "\t" + stat.metric);
  if (!peer) fail("peer statistic missing: " + stat.task + "/" + stat.metric);
  if (peer.median === 0 ? stat.median > 0 : stat.median > peer.median * stat.tolerance) {
    stat.status = "loss";
    stat.owner = "#1414";
  }
}
const measurementRows = statistics.map(stat => [
  "1", stat.task, stat.language, stat.metric, String(stat.median), stat.unit,
  toolchainByKey.get(stat.task + "\t" + stat.language) || fail("toolchain identity missing: " + stat.task + "/" + stat.language),
  "samples=" + stat.samples + ";median=" + stat.median + ";relative-stdev=" + stat.relative + ";outliers=" + stat.outliers,
  stat.status, stat.owner,
]);
const statisticsRows = statistics.map(stat => [
  "1", stat.task, stat.language, stat.metric, String(stat.samples), String(stat.median), String(stat.min), String(stat.max),
  String(stat.relative), String(stat.outliers), String(stat.threshold), String(stat.tolerance), stat.status, stat.owner,
  "samples=" + stat.samples + ";median=" + stat.median + ";relative-stdev=" + stat.relative + ";outliers=" + stat.outliers,
]);

function writeRows(file, header, rows) {
  fs.writeFileSync(file, [header, ...rows].map(row => row.join("\t")).join("\n") + "\n");
}
writeRows(path.join(reportDir, "identity.tsv"), ["version", "key", "value"], identity);
writeRows(path.join(reportDir, "samples.tsv"), ["version", "task_id", "language", "metric", "sample", "value", "unit", "method"], samples);
writeRows(path.join(reportDir, "outcomes.tsv"), ["version", "task_id", "peer_language", "peer_program", "input", "expected", "outcome", "toolchain_id", "jet_tool_version", "peer_tool_version", "dependency_rule", "source_boundary", "jet_status", "peer_status", "loss_owner", "review_status", "review_evidence"], outcomes);
writeRows(path.join(reportDir, "measurements.tsv"), ["version", "task_id", "language", "metric", "value", "unit", "toolchain_id", "evidence", "status", "loss_owner"], measurementRows);
writeRows(path.join(reportDir, "statistics.tsv"), ["version", "task_id", "language", "metric", "samples", "median", "min", "max", "relative_stdev", "outliers", "threshold", "tolerance_ratio", "status", "loss_owner", "evidence"], statisticsRows);
writeRows(path.join(reportDir, "tiers.tsv"), ["version", "task_id", "language", "platform", "target", "tier", "status", "evidence", "loss_owner"], tierReports);
writeRows(path.join(reportDir, "receipts.tsv"), ["version", "task_id", "language", "source_sha256", "input_sha256", "expected_sha256", "output_sha256", "hostile_input_sha256", "hostile_output_sha256", "environment", "machine", "tool_version", "command", "peer_commit", "exit_code", "hostile_exit_code"], receipts);
writeRows(path.join(reportDir, "tier_receipts.tsv"), ["version", "task_id", "language", "platform", "target", "tier", "artifact_sha256", "output_sha256", "command", "status"], tierReceipts);
console.log("compiled workload report: produced platform=" + platform + " report=" + reportDir);
