#!/usr/bin/env node

import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import { spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runIsolated } from "./isolated/host-driver.mjs";

const assuranceDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)));
const repoDir = path.resolve(assuranceDir, "../../..");
const manifestPath = path.join(assuranceDir, "manifest.json");
const nativeDir = path.join(assuranceDir, "native");
const isolatedDir = path.join(assuranceDir, "isolated");
const jetDir = path.join(assuranceDir, "jet");
const defaultTimeoutMs = 120_000;

const CHECKED_CASES = [
  "valid", "spatial", "temporal", "initialization", "alias", "race",
  "optimizer", "assembly", "dynamic-link", "allocator",
];
const ISOLATED_CASES = ["valid", "bad-magic", "bad-version", "bad-kind", "bad-length", "bad-checksum", "wrong-result"];
const CHECKED_INPUTS = Object.freeze({
  valid: [1, 2, 3, 4, 5],
  spatial: [9, 8, 7],
  temporal: [1, 1, 2, 3, 8, 13],
  initialization: [],
  alias: [2, 4, 6, 8],
  race: [3, 1, 4, 1, 5, 9],
  optimizer: null,
  assembly: null,
  "dynamic-link": null,
  allocator: null,
});

function checkedInput(caseId) {
  return Buffer.from(JSON.stringify({ case: caseId, fixture: CHECKED_INPUTS[caseId] ?? null }));
}


function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function monotonicNs() {
  return process.hrtime.bigint();
}

async function exists(file) {
  try {
    await fs.access(file);
    return true;
  } catch {
    return false;
  }
}

async function readJson(file) {
  return JSON.parse(await fs.readFile(file, "utf8"));
}

async function processTreeRssBytes(rootPid) {
  if (!Number.isInteger(rootPid)) return null;
  let entries;
  try {
    entries = await fs.readdir("/proc", { withFileTypes: true });
  } catch {
    return null;
  }
  const rows = new Map();
  await Promise.all(entries.filter((entry) => entry.isDirectory() && /^\d+$/.test(entry.name)).map(async (entry) => {
    try {
      const status = await fs.readFile(`/proc/${entry.name}/status`, "utf8");
      const parent = status.match(/^PPid:\s+(\d+)$/m);
      const rss = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
      if (parent && rss) rows.set(Number(entry.name), { parent: Number(parent[1]), rss: Number(rss[1]) * 1024 });
    } catch {}
  }));
  const tree = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const [pid, row] of rows) {
      if (tree.has(row.parent) && !tree.has(pid)) {
        tree.add(pid);
        changed = true;
      }
    }
  }
  let total = 0;
  let found = false;
  for (const pid of tree) {
    const row = rows.get(pid);
    if (row) {
      total += row.rss;
      found = true;
    }
  }
  return found ? total : null;
}

async function runCommand(command, args, { cwd = repoDir, env = {}, input = undefined, timeoutMs = defaultTimeoutMs } = {}) {
  const started = monotonicNs();
  const child = spawn(command, args, {
    cwd,
    env: { ...process.env, ...env },
    stdio: [input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
  });
  const stdout = [];
  const stderr = [];
  let firstOutput = null;
  let timedOut = false;
  let peakRssBytes = null;
  processTreeRssBytes(child.pid).then((rss) => {
    if (rss !== null) peakRssBytes = Math.max(peakRssBytes ?? 0, rss);
  }).catch(() => {});
  const rssTimer = setInterval(() => {
    processTreeRssBytes(child.pid).then((rss) => {
      if (rss !== null) peakRssBytes = Math.max(peakRssBytes ?? 0, rss);
    }).catch(() => {});
  }, 20);
  child.stdout.on("data", (chunk) => {
    if (firstOutput === null) firstOutput = monotonicNs();
    stdout.push(chunk);
  });
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  if (input !== undefined) child.stdin.end(input);
  const result = await new Promise((resolve) => {
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    child.on("error", (error) => {
      clearTimeout(timer);
      resolve({ code: 127, signal: null, error });
    });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolve({ code: timedOut ? 124 : (code ?? 128), signal });
    });
  });
  clearInterval(rssTimer);
  const finished = monotonicNs();
  return {
    ...result,
    timed_out: timedOut,
    stdout: Buffer.concat(stdout),
    stderr: Buffer.concat(stderr),
    metrics: {
      runtime_ns: Number(finished - started),
      startup_ns: firstOutput === null ? null : Number(firstOutput - started),
      peak_rss_bytes: peakRssBytes,
    },
  };
}

async function buildMetric(outDir) {
  try {
    const receipt = await readJson(path.join(outDir, "receipts", "build.json"));
    return Number.isFinite(receipt.build_ns) ? receipt.build_ns : null;
  } catch {
    return null;
  }
}

async function fileRecord(file, relative = null) {
  const bytes = await fs.readFile(file);
  return { path: file, relative, bytes: bytes.length, sha256: sha256(bytes) };
}

async function sourceClosure(manifest) {
  const records = [];
  const sourcePaths = manifest.profiles["checked-native"].sources
    .concat(manifest.profiles["isolated-binary"].sources)
    .concat(["driver.mjs", "manifest.json"]);
  for (const relative of sourcePaths) {
    const file = path.join(assuranceDir, relative);
    if (!records.some((record) => record.path === file)) records.push(await fileRecord(file, relative));
  }
  records.sort((left, right) => left.relative.localeCompare(right.relative));
  const digest = createHash("sha256");
  for (const record of records) {
    digest.update(record.relative);
    digest.update("\0");
    digest.update(record.sha256);
    digest.update("\0");
  }
  return { files: records, sha256: digest.digest("hex") };
}

async function artifactRecord(file) {
  const record = await fileRecord(file, path.relative(repoDir, file).split(path.sep).join("/"));
  return { path: record.path, bytes: record.bytes, sha256: record.sha256 };
}
async function requireRecordedArtifact(outDir, actual) {
  const receipt = await readJson(path.join(outDir, "receipts", "build.json"));
  const records = [
    ...(receipt.native?.artifacts ?? []),
    ...(receipt.jet?.archive ? [receipt.jet.archive] : []),
    ...(receipt.jet?.header ? [receipt.jet.header] : []),
    ...(receipt.jet?.host ? [receipt.jet.host] : []),
  ];
  const recorded = records.find((entry) => path.resolve(entry.path) === path.resolve(actual.path));
  if (!recorded || recorded.sha256 !== actual.sha256 || recorded.bytes !== actual.bytes) {
    throw new Error(`artifact identity mismatch; refusing to execute ${actual.path}`);
  }
  return receipt;
}

async function requireRecordedDependencies(outDir, dependencyId, current) {
  const receipt = await readJson(path.join(outDir, "receipts", "build.json"));
  const record = receipt.dependencies?.find((entry) => entry.id === dependencyId)?.identity;
  if (!record || record.status !== "observed") {
    throw new Error(`dependency identity unavailable for ${dependencyId}; refusing to execute`);
  }
  const recorded = record.entries.map((entry) => `${path.resolve(entry.path)}:${entry.sha256}:${entry.bytes}`).sort();
  const observed = current.entries.map((entry) => `${path.resolve(entry.path)}:${entry.sha256}:${entry.bytes}`).sort();
  if (recorded.length !== observed.length || recorded.some((entry, index) => entry !== observed[index])) {
    throw new Error(`dependency identity changed for ${dependencyId}; refusing to execute`);
  }
  return receipt;
}

async function requireRecordedSourceClosure(outDir, closure) {
  const receipt = await readJson(path.join(outDir, "receipts", "build.json"));
  if (receipt.source_closure_sha256 !== closure.sha256) {
    throw new Error("source closure changed since build; refusing to execute");
  }
  return receipt;
}
async function dependencyIdentity(file) {
  const probe = await runCommand("ldd", [file], { timeoutMs: 15_000 });
  const entries = [];
  for (const line of probe.stdout.toString("utf8").split(/\r?\n/)) {
    const match = line.match(/=>\s+(\/\S+)|^\s*(\/\S+)\s+\(/);
    const dependency = match?.[1] ?? match?.[2];
    if (!dependency || !(await exists(dependency))) continue;
    entries.push(await artifactRecord(dependency));
  }
  return {
    command: ["ldd", file],
    exit_code: probe.code,
    stdout: probe.stdout.toString("utf8"),
    stderr: probe.stderr.toString("utf8"),
    entries,
    status: probe.code === 0 ? "observed" : "unavailable",
  };
}

async function toolVersion(command) {
  const result = await runCommand(command, ["--version"], { timeoutMs: 15_000 });
  return {
    command,
    exit_code: result.code,
    stdout: result.stdout.toString("utf8").trim(),
    stderr: result.stderr.toString("utf8").trim(),
  };
}

function shellIndependentCommand(command, args) {
  return { command, args: [...args] };
}

async function writeReceipt(outDir, name, value) {
  const receiptDir = path.join(outDir, "receipts");
  await fs.mkdir(receiptDir, { recursive: true });
  const file = path.join(receiptDir, name);
  await fs.writeFile(file, `${JSON.stringify(value, null, 2)}\n`);
  return file;
}

function targetIdentity() {
  return {
    platform: process.platform,
    arch: process.arch,
    node: process.version,
    kernel: os.release(),
  };
}

function canonicalExpectation(profile, caseId, manifest) {
  const cases = manifest.adversarial_cases[profile] ?? [];
  const match = cases.find((entry) => entry.id === caseId);
  return match ?? { id: caseId, expected: "unknown", raw_expectation: "unrun" };
}

function metricProjection(manifest) {
  return {
    schema: "jet.ffi-assurance-comparator-adapter.v1",
    source_status: manifest.status,
    comparator: manifest.metrics.comparator_binding,
    rows: manifest.metrics.profiles.flatMap((profile) => {
      const cases = profile === "checked-native" ? CHECKED_CASES : ISOLATED_CASES;
      return cases.map((caseId) => ({
        workload: manifest.metrics.matched_workload,
        profile,
        case: caseId,
        status: "unmeasured",
        metrics: manifest.metrics.dimensions.map((dimension) => ({
          id: dimension.id,
          comparator_metric: dimension.comparator_metric,
          value: null,
          unit: dimension.unit,
        })),
      }));
    }),
    rule: "Rows remain per profile and case; no aggregate or winner field is emitted.",
  };
}

async function emitPlan(manifest, comparatorOnly = false) {
  const closure = await sourceClosure(manifest);
  const plan = {
    schema: "jet.ffi-assurance-plan.v1",
    status: manifest.status,
    qualification: manifest.qualification,
    safe_claim_allowed: manifest.safe_claim_allowed,
    source_closure_sha256: closure.sha256,
    sources: closure.files,
    profiles: manifest.profiles,
    boundaries: manifest.boundaries,
    uncovered_edges: manifest.uncovered_edges,
    authority: {
      checked_native: manifest.profiles["checked-native"].authority,
      isolated_binary: manifest.profiles["isolated-binary"].authority,
    },
    comparator: metricProjection(manifest),
    raw_expectations: manifest.adversarial_cases,
  };
  if (comparatorOnly) {
    console.log(JSON.stringify(plan.comparator, null, 2));
  } else {
    console.log(JSON.stringify(plan, null, 2));
  }
}

async function requireBuilt(outDir, fileName) {
  const file = path.join(outDir, fileName);
  if (!(await exists(file))) throw new Error(`missing built artifact ${file}; run --build first`);
  return file;
}

async function buildNative(outDir, cc) {
  await fs.mkdir(outDir, { recursive: true });
  const library = path.join(outDir, "libffi-assurance.so");
  const checkedDriver = path.join(outDir, "checked-driver");
  const worker = path.join(outDir, "ffi-assurance-worker");
  const commands = [
    shellIndependentCommand(cc, ["-std=c11", "-O2", "-fPIC", "-fno-omit-frame-pointer", "-shared", path.join(nativeDir, "checked_native.c"), "-o", library]),
    shellIndependentCommand(cc, ["-std=c11", "-O2", "-fno-omit-frame-pointer", "-I", nativeDir, path.join(nativeDir, "checked_driver.c"), "-L", outDir, "-Wl,-rpath,$ORIGIN", "-lffi-assurance", "-pthread", "-o", checkedDriver]),
    shellIndependentCommand(cc, ["-std=c11", "-O2", "-fno-omit-frame-pointer", path.join(isolatedDir, "worker.c"), "-o", worker]),
  ];
  const results = [];
  const started = monotonicNs();
  for (const invocation of commands) {
    const result = await runCommand(invocation.command, invocation.args, { cwd: repoDir });
    results.push({ ...invocation, result: { code: result.code, signal: result.signal, stderr: result.stderr.toString("utf8"), stdout: result.stdout.toString("utf8"), metrics: result.metrics } });
    if (result.code !== 0) throw new Error(`native build failed: ${invocation.command} ${invocation.args.join(" ")}\n${result.stderr.toString("utf8")}`);
  }
  const artifacts = [await artifactRecord(library), await artifactRecord(checkedDriver), await artifactRecord(worker)];
  return { artifacts, commands: results, build_ns: Number(monotonicNs() - started), library, checkedDriver, worker };
}

async function buildJetLibrary(outDir, cc, jetEnv) {
  const stage = path.join(outDir, "jet-source");
  await fs.rm(stage, { recursive: true, force: true });
  await fs.cp(jetDir, stage, { recursive: true });
  const jetBuild = await runCommand(jetEnv, ["jet", "build", "--lib", "library.jet"], { cwd: stage });
  if (jetBuild.code !== 0) throw new Error(`Jet library build failed:\n${jetBuild.stderr.toString("utf8")}`);
  const target = path.join(stage, "target");
  const archive = path.join(target, "libffiassurance.a");
  const header = path.join(target, "ffiassurance.h");
  if (!(await exists(archive)) || !(await exists(header))) throw new Error("Jet library build produced no archive/header pair");
  const host = path.join(outDir, "jet-host");
  const hostBuild = await runCommand(cc, ["-std=c11", "-O2", "-I", target, path.join(jetDir, "host.c"), path.join(jetDir, "foreign.c"), archive, "-pthread", "-ldl", "-lm", "-o", host], { cwd: repoDir });
  if (hostBuild.code !== 0) throw new Error(`generated Jet library host build failed:\n${hostBuild.stderr.toString("utf8")}`);
  return {
    stage,
    archive: await artifactRecord(archive),
    header: await artifactRecord(header),
    host: await artifactRecord(host),
    commands: [
      { command: jetEnv, args: ["jet", "build", "--lib", "library.jet"], result: { code: jetBuild.code, stdout: jetBuild.stdout.toString("utf8"), stderr: jetBuild.stderr.toString("utf8"), metrics: jetBuild.metrics } },
      { command: cc, args: ["-std=c11", "-O2", "-I", target, path.join(jetDir, "host.c"), path.join(jetDir, "foreign.c"), archive, "-pthread", "-ldl", "-lm", "-o", host], result: { code: hostBuild.code, stdout: hostBuild.stdout.toString("utf8"), stderr: hostBuild.stderr.toString("utf8"), metrics: hostBuild.metrics } },
    ],
  };
}

async function buildAll(options, manifest) {
  const outDir = path.resolve(options.out);
  const cc = options.cc ?? process.env.CC ?? "cc";
  const jetEnv = options.jetEnv ?? path.join(repoDir, "scripts/agent/jet-env");
  const compiler = await toolVersion(cc);
  const started = monotonicNs();
  const native = await buildNative(outDir, cc);
  const jet = await buildJetLibrary(outDir, cc, jetEnv);
  const buildNs = Number(monotonicNs() - started);
  const closure = await sourceClosure(manifest);
  const checkedDependencies = await dependencyIdentity(native.checkedDriver);
  const workerDependencies = await dependencyIdentity(native.worker);
  const jetHostDependencies = await dependencyIdentity(jet.host.path);
  const receipt = {
    schema: "jet.ffi-assurance-build-receipt.v1",
    status: "built_unqualified",
    profile: "checked-native+isolated-binary",
    source_closure_sha256: closure.sha256,
    source_files: closure.files,
    build_ns: buildNs,
    compiler,
    compiler_options: manifest.profiles["checked-native"].build.flags,
    target: targetIdentity(),
    dependencies: [
      { id: "libc-and-dynamic-loader", status: "trusted_outside_hardening" },
      { id: "checked-driver-loader-closure", identity: checkedDependencies },
      { id: "isolated-worker-loader-closure", identity: workerDependencies },
      { id: "jet-host-loader-closure", identity: jetHostDependencies },
      { id: "compiled-workload-peer-launcher", status: "required_for_isolated_run" },
      { id: "jet-generated-library-runtime", status: "artifact-bound" }
    ],
    native,
    jet,
    raw_execution_status: "unrun",
    claims: { safe: false, measured: false, equivalent: false },
  };
  const receiptPath = await writeReceipt(outDir, "build.json", receipt);
  console.log(JSON.stringify({ ...receipt, receipt: receiptPath }, null, 2));
}

async function runChecked(options, manifest) {
  const outDir = path.resolve(options.out);
  const binary = await requireBuilt(outDir, "checked-driver");
  if (!CHECKED_CASES.includes(options.case)) throw new Error(`unknown checked-native case ${options.case}`);
  const binaryArtifact = await artifactRecord(binary);
  const libraryArtifact = await artifactRecord(path.join(outDir, "libffi-assurance.so"));
  const binaryDependencies = await dependencyIdentity(binary);
  await requireRecordedArtifact(outDir, binaryArtifact);
  const payload = checkedInput(options.case);
  await requireRecordedDependencies(outDir, "checked-driver-loader-closure", binaryDependencies);
  const linkedLibrary = binaryDependencies.entries.find((entry) => entry.path === libraryArtifact.path && entry.sha256 === libraryArtifact.sha256);
  if (!linkedLibrary) throw new Error("dynamic-link identity mismatch; checked binary was not executed");
  const closure = await sourceClosure(manifest);
  await requireRecordedSourceClosure(outDir, closure);
  const result = await runCommand(binary, [options.case], { cwd: outDir });
  const output = Buffer.concat([result.stdout, result.stderr]);
  const receipt = {
    schema: "jet.ffi-assurance-evidence.v1",
    profile: "checked-native",
    case: options.case,
    status: result.code === 0 ? "observed_conditional" : "observed_failure",
    source_closure_sha256: closure.sha256,
    native_artifact: binaryArtifact,
    compiler_command: "see receipts/build.json",
    target: targetIdentity(),
    loaded_dependencies: [libraryArtifact, ...binaryDependencies.entries],
    dependency_probe: binaryDependencies,
    authority: { filesystem: "ambient", network: "inherited", process_boundary: false },
    input_sha256: sha256(payload),
    expected: canonicalExpectation("checked-native", options.case, manifest),
    expected_sha256: sha256(JSON.stringify(canonicalExpectation("checked-native", options.case, manifest))),
    actual_output_sha256: sha256(output),
    output: output.toString("utf8"),
    exit_code: result.code,
    signal: result.signal,
    metrics: { ...result.metrics, build_ns: await buildMetric(outDir), startup_ns: result.metrics.startup_ns, copy_ns: 0, copy_count: 0, copy_bytes: 0, artifact_size_bytes: binaryArtifact.bytes },
    claims: { safe: false, measured: true, equivalent: false },
  };
  const receiptPath = await writeReceipt(outDir, `checked-${options.case}.json`, receipt);
  console.log(JSON.stringify({ ...receipt, receipt: receiptPath }, null, 2));
  if (result.code !== 0) process.exitCode = 1;
}

async function runIsolatedProfile(options, manifest) {
  const outDir = path.resolve(options.out);
  const worker = await requireBuilt(outDir, "ffi-assurance-worker");
  const launcher = options.launcher ?? process.env.JET_FFI_ASSURANCE_LAUNCHER;
  if (!launcher) throw new Error("isolated run requires JET_FFI_ASSURANCE_LAUNCHER or --launcher; no direct-process fallback exists");
  const workerArtifact = await artifactRecord(worker);
  const launcherRecord = await artifactRecord(path.resolve(launcher));
  const workerDependencies = await dependencyIdentity(worker);
  await requireRecordedArtifact(outDir, workerArtifact);
  await requireRecordedDependencies(outDir, "isolated-worker-loader-closure", workerDependencies);
  const closure = await sourceClosure(manifest);
  await requireRecordedSourceClosure(outDir, closure);
  const root = path.join(outDir, "isolated", options.case);
  const receipt = await runIsolated({ launcher, worker, root, caseId: options.case, payload: Buffer.from(options.payload ?? "ffi-assurance") });
  const enriched = {
    schema: "jet.ffi-assurance-evidence.v1",
    profile: "isolated-binary",
    case: options.case,
    status: receipt.host_validation?.status === "rejected-untrusted-result" ? "observed_host_rejection" : "observed_conditional",
    source_closure_sha256: closure.sha256,
    native_artifact: receipt.artifact,
    launcher: launcherRecord,
    toolchain: { host_driver: process.version },
    target: targetIdentity(),
    loaded_dependencies: [
      { id: "libc-and-dynamic-loader", status: "trusted_outside_hardening" },
      ...workerDependencies.entries,
      launcherRecord,
    ],
    dependency_probe: workerDependencies,
    input_sha256: sha256(Buffer.from(options.payload ?? "ffi-assurance")),
    expected: canonicalExpectation("isolated-binary", options.case, manifest),
    expected_sha256: sha256(JSON.stringify(canonicalExpectation("isolated-binary", options.case, manifest))),
    actual_output_sha256: receipt.wire_output_sha256,
    validated_output_sha256: receipt.validated_output_sha256,
    authority: receipt.authority,
    host_validation: receipt.host_validation,
    output: receipt.message,
    exit_code: receipt.exit_code,
    signal: receipt.signal,
    metrics: { ...receipt.metrics, build_ns: await buildMetric(outDir) },
    claims: { safe: false, measured: true, equivalent: false },
  };
  const receiptPath = await writeReceipt(outDir, `isolated-${options.case}.json`, enriched);
  console.log(JSON.stringify({ ...enriched, receipt: receiptPath }, null, 2));
}

async function runJetHost(options, manifest) {
  const outDir = path.resolve(options.out);
  const host = await requireBuilt(outDir, "jet-host");
  const hostArtifact = await artifactRecord(host);
  await requireRecordedArtifact(outDir, hostArtifact);
  const hostDependencies = await dependencyIdentity(host);
  await requireRecordedDependencies(outDir, "jet-host-loader-closure", hostDependencies);
  const closure = await sourceClosure(manifest);
  await requireRecordedSourceClosure(outDir, closure);
  const result = await runCommand(host, [], { cwd: outDir });
  const output = Buffer.concat([result.stdout, result.stderr]);
  const receipt = {
    schema: "jet.ffi-assurance-evidence.v1",
    profile: "checked-native",
    caller: "generated-jet-library-export",
    case: "valid",
    status: result.code === 0 ? "observed_conditional" : "observed_failure",
    source_closure_sha256: closure.sha256,
    native_artifact: hostArtifact,
    generated_library: [
      await artifactRecord(path.join(outDir, "jet-source", "target", "libffiassurance.a")),
      await artifactRecord(path.join(outDir, "jet-source", "target", "ffiassurance.h"))
    ],
    dependency_probe: hostDependencies,
    loaded_dependencies: hostDependencies.entries,
    target: targetIdentity(),
    authority: { filesystem: "ambient", network: "inherited", process_boundary: false },
    input_sha256: sha256(Buffer.from("ffi-assurance", "utf8")),
    expected: { output: "ok\n", exit_code: 0 },
    expected_sha256: sha256(Buffer.from("ok\n", "utf8")),
    actual_output_sha256: sha256(output),
    output: output.toString("utf8"),
    exit_code: result.code,
    signal: result.signal,
    metrics: { ...result.metrics, build_ns: await buildMetric(outDir), copy_ns: 0, copy_count: 0, copy_bytes: 0, artifact_size_bytes: hostArtifact.bytes },
    claims: { safe: false, measured: true, equivalent: false }
  };
  const receiptPath = await writeReceipt(outDir, "jet-caller.json", receipt);
  console.log(JSON.stringify({ ...receipt, receipt: receiptPath }, null, 2));
  if (result.code !== 0) process.exitCode = 1;
}

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index < 0 ? null : args[index + 1] ?? null;
}

function parseArgs(args) {
  return {
    plan: args.includes("--plan"),
    comparatorPlan: args.includes("--comparator-plan"),
    build: args.includes("--build"),
    run: args.includes("--run"),
    runJet: args.includes("--run-jet"),
    profile: optionValue(args, "--profile"),
    case: optionValue(args, "--case"),
    out: optionValue(args, "--out"),
    cc: optionValue(args, "--cc"),
    jetEnv: optionValue(args, "--jet-env"),
    launcher: optionValue(args, "--launcher"),
    payload: optionValue(args, "--payload"),
    help: args.includes("--help") || args.includes("-h"),
  };
}

function usage() {
  return [
    "usage:",
    "  driver.mjs --plan",
    "  driver.mjs --comparator-plan",
    "  driver.mjs --build --out DIR [--cc CC] [--jet-env PATH]",
    "  driver.mjs --run --profile checked-native --case CASE --out DIR",
    "  driver.mjs --run --profile isolated-binary --case CASE --out DIR --launcher PATH",
    "  driver.mjs --run-jet --out DIR",
  ].join("\n");
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(usage());
    return;
  }
  const manifest = await readJson(manifestPath);
  if (options.comparatorPlan) {
    await emitPlan(manifest, true);
    return;
  }
  if (options.plan || (!options.build && !options.run && !options.runJet)) {
    await emitPlan(manifest, false);
    return;
  }
  if (!options.out) throw new Error("--out DIR is required for execution actions");
  if (options.build) {
    await buildAll(options, manifest);
    return;
  }
  if (options.run) {
    if (!options.profile || !options.case) throw new Error("--run requires --profile and --case");
    if (options.profile === "checked-native") await runChecked(options, manifest);
    else if (options.profile === "isolated-binary") await runIsolatedProfile(options, manifest);
    else throw new Error(`unknown run profile ${options.profile}`);
    return;
  }
  if (options.runJet) {
    await runJetHost(options, manifest);
    return;
  }
  throw new Error("no action selected");
}

main().catch((error) => {
  console.error(`ffi-assurance: ${error.message}`);
  process.exitCode = 1;
});
