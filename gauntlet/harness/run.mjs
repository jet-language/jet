#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runLiveReloadAxis as runLiveReloadAxisAdapter } from "./live-reload.mjs";
import { runMemorySafetyFuzzAxis as runMemorySafetyFuzzAxisAdapter } from "./memory-safety-fuzz.mjs";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../..");
const envRunner = path.join(repoDir, "scripts/agent/jet-env");
const timer = path.join(harnessDir, "timer.py");

const ENTRY_MODES = ["batch", "batch-steps", "service", "web", "web-app"];
const MATRIX_UNCOVERED_DEFAULTS = ["embedded.data", "embedded.kernel"];
const COMPARISON_METRICS = [
  "runtime_wall_seconds",
  "runtime_peak_rss_kb",
  "runtime_first_stdout_seconds",
  "cold_build_seconds",
  "warm_build_seconds",
  "binary_bytes",
  "loc",
  "source_bytes",
  "tokens",
  "source_tokens",
];
const MODE_PRIMARY_METRIC = {
  batch: "runtime_wall_seconds",
  "batch-steps": "runtime_wall_seconds",
  service: "service_latency_ms_p50",
  web: "runtime_first_stdout_seconds",
  "web-app": "runtime_first_stdout_seconds",
};
const TIER_POLICY = {
  batch: { aot: { required: true }, run: { required: true }, dev: { required: false } },
  "batch-steps": { aot: { required: true }, run: { required: true }, dev: { required: false } },
  service: { aot: { required: true }, run: { required: true }, dev: { required: false } },
  web: { aot: { required: true }, run: { required: true }, dev: { required: false } },
  "web-app": { aot: { required: true } },
};

const LANGUAGE_FILES = {
  jet: "main.jet",
  rust: "main.rs",
  python: "main.py",
  c: "main.c",
  zig: "main.zig",
  go: "main.go",
  js: "main.mjs",
  node: "main.mjs",
};

function baseLanguage(language) {
  return language.endsWith("-expert") ? language.slice(0, -"-expert".length) : language;
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function parseArgs(argv) {
  const options = { runs: null, entry: null, jetBin: null, entriesDir: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--entry" || arg === "--jet-bin" || arg === "--runs" || arg === "--entries-dir") {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      const value = argv[++i];
      if (arg === "--entry") options.entry = value;
      if (arg === "--jet-bin") options.jetBin = value;
      if (arg === "--entries-dir") options.entriesDir = value;
      if (arg === "--runs") options.runs = Number.parseInt(value, 10);
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: node gauntlet/harness/run.mjs [--entry name] [--jet-bin path] [--runs n] [--entries-dir path]");
      process.exit(0);
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (options.runs !== null && (!Number.isInteger(options.runs) || options.runs < 1)) {
    throw new Error("--runs must be a positive integer");
  }
  return options;
}

async function exists(file) {
  try {
    await fs.access(file);
    return true;
  } catch {
    return false;
  }
}

function timeoutFromEnv(name, fallback) {
  const value = Number.parseInt(process.env[name] ?? "", 10);
  return Number.isInteger(value) && value > 0 ? value : fallback;
}

const DEFAULT_TIMEOUT_MS = timeoutFromEnv("JET_GAUNTLET_TIMEOUT_MS", 300_000);

async function processTreeRssKb(rootPid) {
  if (!Number.isInteger(rootPid)) return null;
  let entries;
  try {
    entries = await fs.readdir("/proc", { withFileTypes: true });
  } catch {
    return null;
  }
  const rows = new Map();
  await Promise.all(entries
    .filter((entry) => entry.isDirectory() && /^\d+$/.test(entry.name))
    .map(async (entry) => {
      try {
        const status = await fs.readFile(`/proc/${entry.name}/status`, "utf8");
        const parent = status.match(/^PPid:\s+(\d+)$/m);
        const rss = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
        if (parent && rss) rows.set(Number(entry.name), { parent: Number(parent[1]), rss: Number(rss[1]) });
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

async function runProcess(cwd, args, { input = undefined, full = false, timeoutMs = DEFAULT_TIMEOUT_MS, resourceBudget = null } = {}) {
  return new Promise((resolve) => {
    const child = spawn(envRunner, [...(full ? ["full"] : []), "sh", "-c", args.map(shellQuote).join(" ")], {
      cwd,
      env: process.env,
      stdio: [input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
      detached: true,
    });
    const stdout = [];
    const stderr = [];
    let timedOut = false;
    let resourceExceeded = null;
    let terminated = false;
    let closed = false;
    let resourceCheckBusy = false;
    const killTree = (reason = null) => {
      if (terminated) return;
      terminated = true;
      if (reason) {
        timedOut = true;
        resourceExceeded = reason;
      }
      timedOut = true;
      try { process.kill(-child.pid, "SIGKILL"); } catch {}
      try { child.kill("SIGKILL"); } catch {}
    };
    const deadline = timeoutMs > 0 ? setTimeout(killTree, timeoutMs) : null;
    const resourceTimer = resourceBudget?.memory_mb > 0 ? setInterval(() => {
      if (closed || resourceCheckBusy) return;
      resourceCheckBusy = true;
      processTreeRssKb(child.pid).then((rssKb) => {
        if (!closed && Number.isFinite(rssKb) && rssKb > resourceBudget.memory_mb * 1024) {
          killTree(`memory budget exceeded: ${rssKb}kB > ${resourceBudget.memory_mb * 1024}kB`);
        }
      }).catch(() => {}).finally(() => { resourceCheckBusy = false; });
    }, 50) : null;
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.on("error", (error) => {
      closed = true;
      clearTimeout(deadline);
      if (resourceTimer) clearInterval(resourceTimer);
      resolve({ code: 127, stdout: Buffer.concat(stdout), stderr: Buffer.from(String(error)), timedOut, resourceExceeded });
    });
    child.on("close", (code, signal) => {
      closed = true;
      clearTimeout(deadline);
      if (resourceTimer) clearInterval(resourceTimer);
      resolve({
        code: resourceExceeded ? 137 : (timedOut ? 124 : (code ?? 128)),
        signal,
        stdout: Buffer.concat(stdout),
        stderr: resourceExceeded
          ? Buffer.from(resourceExceeded)
          : (timedOut ? Buffer.from(`timeout after ${timeoutMs / 1000}s`) : Buffer.concat(stderr)),
        timedOut,
        resourceExceeded,
      });
    });
    if (input !== undefined) child.stdin.end(input);
  });
}

async function timedProcess(cwd, args, { full = false, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  const result = await runProcess(cwd, ["python3", timer, "--", ...args], { full, timeoutMs });
  let sample;
  try {
    sample = JSON.parse(result.stdout.toString("utf8").trim());
  } catch {
    sample = { error: result.stdout.toString("utf8").slice(0, 300) || result.stderr.toString("utf8").slice(0, 300) };
  }
  if (result.code !== 0 && sample.exit_code === undefined) sample.exit_code = result.code;
  sample.stderr = result.stderr.toString("utf8").trim().slice(0, 500);
  return sample;
}

async function timedSequence(cwd, commands, { full = false } = {}) {
  const result = await runProcess(cwd, ["python3", timer, "--sequence-json", JSON.stringify(commands)], { full });
  let sample;
  try {
    sample = JSON.parse(result.stdout.toString("utf8").trim());
  } catch {
    sample = { error: result.stdout.toString("utf8").slice(0, 300) || result.stderr.toString("utf8").slice(0, 300) };
  }
  if (result.code !== 0 && sample.exit_code === undefined) sample.exit_code = result.code;
  sample.stderr = result.stderr.toString("utf8").trim().slice(0, 500);
  return sample;
}

function startProcess(cwd, args, { full = false } = {}) {
  const child = spawn(envRunner, [...(full ? ["full"] : []), "sh", "-c", args.map(shellQuote).join(" ")], {
    cwd,
    env: process.env,
    stdio: ["ignore", "ignore", "pipe"],
    detached: true,
  });
  const stderr = [];
  child.spawnError = null;
  child.once("error", (error) => { child.spawnError = error; });
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  child.stderrText = () => Buffer.concat(stderr).toString("utf8").trim().slice(0, 500);
  return child;
}

function stopProcess(child) {
  if (!child || child.exitCode !== null) return;
  try { process.kill(-child.pid, "SIGTERM"); } catch { child.kill("SIGTERM"); }
}

function waitForExit(child, timeoutMs = 5000) {
  if (child.exitCode !== null) return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  return new Promise((resolve) => {
    let finished = false;
    const done = (result) => {
      if (finished) return;
      finished = true;
      clearTimeout(timeout);
      resolve(result);
    };
    const timeout = setTimeout(() => done({ code: null, signal: "TIMEOUT" }), timeoutMs);
    child.once("close", (code, signal) => done({ code, signal }));
  });
}

function waitForTcp(port, timeoutMs = 5000) {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.createConnection({ host: "127.0.0.1", port });
      socket.once("connect", () => { socket.destroy(); resolve(); });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() - started >= timeoutMs) reject(new Error(`TCP port ${port} did not open`));
        else setTimeout(attempt, 25);
      });
    };
    attempt();
  });
}

function httpProbe(port, probe, timeoutMs = 5000) {
  return new Promise((resolve) => {
    const started = performance.now();
    const body = probe.body === undefined ? undefined : String(probe.body);
    const request = http.request({
      host: "127.0.0.1",
      port,
      path: probe.path,
      method: probe.method ?? "GET",
      headers: body === undefined ? undefined : { "content-length": Buffer.byteLength(body) },
      timeout: timeoutMs,
    }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve({
        ok: true,
        status: response.statusCode,
        body: Buffer.concat(chunks).toString("utf8"),
        latencyMs: performance.now() - started,
      }));
    });
    request.on("timeout", () => request.destroy(new Error("HTTP probe timeout")));
    request.on("error", (error) => resolve({ ok: false, error: error.message, latencyMs: performance.now() - started }));
    if (body !== undefined) request.write(body);
    request.end();
  });
}

function lineProbe(port, probe, timeoutMs = 5000) {
  return new Promise((resolve) => {
    const started = performance.now();
    const chunks = [];
    let finished = false;
    const socket = net.createConnection({ host: "127.0.0.1", port });
    const done = (result) => {
      if (finished) return;
      finished = true;
      clearTimeout(timeout);
      socket.destroy();
      resolve({ ...result, latencyMs: performance.now() - started });
    };
    const timeout = setTimeout(() => done({ ok: false, error: "line probe timeout" }), timeoutMs);
    socket.once("connect", () => socket.end(String(probe.send ?? "")));
    socket.on("data", (chunk) => {
      chunks.push(chunk);
      const body = Buffer.concat(chunks).toString("utf8");
      const newline = body.indexOf("\n");
      if (newline >= 0) done({ ok: true, body: body.slice(0, newline + 1) });
    });
    socket.once("end", () => done({ ok: true, body: Buffer.concat(chunks).toString("utf8") }));
    socket.once("error", (error) => done({ ok: false, error: error.message }));
  });
}

function serviceProbe(port, probe, protocol) {
  return protocol === "line" ? lineProbe(port, probe) : httpProbe(port, probe);
}

function probeMatches(probe, result, protocol) {
  if (!result.ok) return false;
  if (protocol === "line") return result.body === probe.expect;
  return (probe.expectStatus === undefined || result.status === probe.expectStatus) &&
    (probe.expectBody === undefined || result.body === probe.expectBody);
}

function probeMismatch(probe, result, protocol) {
  if (!result.ok) return result.error;
  if (protocol === "line") return `body ${JSON.stringify(result.body)}, expected ${JSON.stringify(probe.expect)}`;
  if (probe.expectStatus !== undefined && result.status !== probe.expectStatus) return `status ${result.status}, expected ${probe.expectStatus}`;
  return `body ${JSON.stringify(result.body)}, expected ${JSON.stringify(probe.expectBody)}`;
}

async function probeSequence(port, probes, protocol = "http") {
  for (let index = 0; index < probes.length; index += 1) {
    const probe = probes[index];
    const result = await serviceProbe(port, probe, protocol);
    if (!probeMatches(probe, result, protocol)) return { ok: false, index, result, reason: probeMismatch(probe, result, protocol) };
  }
  return { ok: true };
}

async function readRssKb(pid) {
  try {
    const status = await fs.readFile(`/proc/${pid}/status`, "utf8");
    const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
    return match ? Number(match[1]) : null;
  } catch {
    return null;
  }
}

function median(values) {
  const numbers = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!numbers.length) return null;
  const middle = Math.floor(numbers.length / 2);
  return numbers.length % 2 ? numbers[middle] : (numbers[middle - 1] + numbers[middle]) / 2;
}

function percentile(values, fraction) {
  const numbers = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!numbers.length) return null;
  return numbers[Math.min(numbers.length - 1, Math.ceil(numbers.length * fraction) - 1)];
}

function summarizeSamples(samples) {
  const metrics = ["wall_seconds", "peak_rss_kb", "time_to_first_stdout_seconds"];
  const medians = Object.fromEntries(metrics.map((metric) => [metric, median(samples.map((sample) => sample[metric]))]));
  return { samples, median: medians };
}

function mismatch(expected, actual) {
  const limit = Math.min(expected.length, actual.length);
  let index = 0;
  while (index < limit && expected[index] === actual[index]) index += 1;
  if (index === expected.length && index === actual.length) return null;
  const show = (buffer) => JSON.stringify(buffer.subarray(index, index + 80).toString("utf8"));
  return `byte ${index}: expected ${show(expected)}, got ${show(actual)} (length ${expected.length}/${actual.length})`;
}

async function sourceMetrics(sourceDir, filename) {
  const source = path.join(sourceDir, filename);
  const text = await fs.readFile(source, "utf8");
  const lines = text.split(/\r?\n/);
  const extension = path.extname(filename);
  const loc = lines.filter((line) => {
    const trimmed = line.trim();
    const hashComment = extension === ".py" && trimmed.startsWith("#");
    return trimmed && !trimmed.startsWith("//") && !trimmed.startsWith("/*") && !trimmed.startsWith("*") && !hashComment && !trimmed.startsWith("# ") && !trimmed.startsWith("#!");
  }).length;
  const tokens = text.match(/[\p{L}\p{N}_]+|[^\s\p{L}\p{N}_]/gu)?.length ?? 0;
  const source_tokens = text.match(/\S+/gu)?.length ?? 0;
  return {
    loc,
    source_bytes: Buffer.byteLength(text),
    tokens,
    source_tokens,
    source_sha256: createHash("sha256").update(text).digest("hex"),
  };
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function fileSha256(file) {
  return sha256(await fs.readFile(file));
}

async function treeSha256(root) {
  const files = [];
  async function walk(current, relative) {
    const items = (await fs.readdir(current, { withFileTypes: true }))
      .sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
    for (const item of items) {
      const itemRelative = path.join(relative, item.name);
      const full = path.join(current, item.name);
      if (item.isDirectory()) await walk(full, itemRelative);
      else if (item.isFile()) files.push({ relative: itemRelative, full });
    }
  }
  await walk(root, "");
  const digest = createHash("sha256");
  for (const file of files) {
    digest.update(file.relative.replaceAll(path.sep, "/"));
    digest.update("\0");
    digest.update(await fs.readFile(file.full));
    digest.update("\0");
  }
  return digest.digest("hex");
}

async function pathSha256(target) {
  const stat = await fs.stat(target);
  return stat.isDirectory() ? treeSha256(target) : fileSha256(target);
}

function sortedUnique(values) {
  return [...new Set(values)].sort((left, right) => left < right ? -1 : left > right ? 1 : 0);
}

function equalStringArrays(left, right) {
  return JSON.stringify(sortedUnique(left)) === JSON.stringify(sortedUnique(right));
}

function tierPolicy(mode) {
  return TIER_POLICY[mode] ?? {};
}

function primaryMetric(mode) {
  return MODE_PRIMARY_METRIC[mode] ?? "runtime_wall_seconds";
}

function comparisonMetrics(mode) {
  if (mode === "service") {
    return [
      "service_latency_ms_p50",
      "service_latency_ms_p99",
      "service_startup_seconds",
      "runtime_peak_rss_kb",
      "binary_bytes",
      "loc",
      "source_bytes",
      "tokens",
      "source_tokens",
    ];
  }
  if (mode === "web" || mode === "web-app") {
    return [
      "runtime_first_stdout_seconds",
      "runtime_wall_seconds",
      "runtime_peak_rss_kb",
      "cold_build_seconds",
      "warm_build_seconds",
      "binary_bytes",
      "loc",
      "source_bytes",
      "tokens",
      "source_tokens",
    ];
  }
  return COMPARISON_METRICS;
}

function runtimeMetrics(runtime) {
  return {
    runtime_wall_seconds: runtime?.median?.wall_seconds ?? null,
    runtime_peak_rss_kb: runtime?.median?.peak_rss_kb ?? null,
    runtime_first_stdout_seconds: runtime?.median?.time_to_first_stdout_seconds ?? null,
  };
}

function buildMetrics(build) {
  return {
    cold_build_seconds: build?.cold?.wall_seconds ?? null,
    warm_build_seconds: build?.warm?.wall_seconds ?? null,
  };
}

function unavailableTier(required, reason, status = "unavailable") {
  return {
    applicable: true,
    required,
    status,
    reason,
    metrics: {},
  };
}

function manifestSource(entryDir, relative) {
  if (typeof relative !== "string" || relative.length === 0 || path.isAbsolute(relative)) {
    throw new Error(`measurement manifest source must be a relative path: ${relative}`);
  }
  const resolved = path.resolve(entryDir, relative);
  const remainder = path.relative(entryDir, resolved);
  if (!remainder || remainder.startsWith("..") || path.isAbsolute(remainder)) {
    throw new Error(`measurement manifest source escapes entry: ${relative}`);
  }
  return resolved;
}

async function manifestSourceMetrics(entryDir, relative) {
  const source = manifestSource(entryDir, relative);
  return sourceMetrics(path.dirname(source), path.basename(source));
}

async function measureSourceManifest(entriesDir, manifest, matrix = null) {
  if (manifest?.version !== 1 || !manifest.contract || !Array.isArray(manifest.entries)) {
    throw new Error("invalid gauntlet measurement manifest");
  }
  const contract = manifest.contract;
  if (contract.token_metric !== "source_tokens" || contract.loc_ratio_max !== 1.2 || contract.token_verdict !== "jet_less_than_python") {
    throw new Error("unsupported gauntlet measurement contract");
  }
  const corpus = manifest.corpus;
  if (!corpus || !Array.isArray(corpus.entry_names) || !Array.isArray(corpus.allowed_uncovered_cells)) {
    throw new Error("measurement manifest is missing corpus denominator contract");
  }
  const names = manifest.entries.map((row) => row.name);
  if (!names.every((name) => typeof name === "string") || new Set(names).size !== names.length) {
    throw new Error("measurement manifest has duplicate or invalid entry names");
  }
  if (manifest.entries.length !== corpus.entry_count || corpus.entry_names.length !== corpus.entry_count ||
    new Set(corpus.entry_names).size !== corpus.entry_names.length || !equalStringArrays(names, corpus.entry_names) ||
    new Set(corpus.allowed_uncovered_cells).size !== corpus.allowed_uncovered_cells.length) {
    throw new Error("measurement manifest entry denominator does not match its named corpus");
  }
  const reportContract = manifest.report_contract;
  const tierPolicyByMode = Object.fromEntries(Object.entries(TIER_POLICY).map(([mode, policy]) => [mode, Object.keys(policy)]));
  if (reportContract?.id !== "gauntlet-report-v1" || reportContract.scope !== "full_matrix" ||
    !equalStringArrays(reportContract.required_jet_tiers ?? [], ["aot", "run"]) ||
    !equalStringArrays(reportContract.optional_jet_tiers ?? [], ["dev"]) ||
    JSON.stringify(reportContract.tier_policy_by_mode) !== JSON.stringify(tierPolicyByMode) ||
    JSON.stringify(reportContract.primary_metric_by_mode) !== JSON.stringify(MODE_PRIMARY_METRIC) ||
    reportContract.ratio_verdicts?.win !== "<1" || reportContract.ratio_verdicts?.parity !== "<=1.05" || reportContract.ratio_verdicts?.loss !== ">1.05" ||
    reportContract.missing_metric_verdict !== "unmeasured" || reportContract.output_verification !== "byte_exact_utf8_or_declared_probe_sequence" ||
    reportContract.loss_owner_required_for !== "primary_metric_loss" ||
    JSON.stringify(reportContract.axis_schemas) !== JSON.stringify({
      live_reload: "gauntlet-axis-live-reload-v1",
      memory_safety_fuzz: "gauntlet-axis-memory-safety-fuzz-v1",
    }) || reportContract.axis_publication !== "required_axes_complete_and_unblocked") {
    throw new Error("unsupported gauntlet report contract");
  }
  const liveReload = manifest.axes?.live_reload;
  const memorySafetyFuzz = manifest.axes?.memory_safety_fuzz;
  const liveReloadRunnerIds = liveReload?.runners?.map((runner) => runner.id) ?? [];
  const memorySafetyRunnerIds = memorySafetyFuzz?.runners?.map((runner) => runner.id) ?? [];
  if (!manifest.axes || !liveReload || !memorySafetyFuzz ||
    liveReload.status !== "required" || memorySafetyFuzz.status !== "required" ||
    liveReload.schema !== "gauntlet-axis-live-reload-v1" ||
    liveReload.metric !== "reload_latency_ms" || liveReload.workload !== "web-app" ||
    liveReload.signal?.kind !== "monotonic_http_counter" ||
    liveReload.signal?.definition !== "GET readiness path returns a numeric value greater than the value observed before the edit" ||
    liveReload.budget?.sample_count !== 3 || liveReload.budget?.startup_timeout_ms !== 30_000 ||
    liveReload.budget?.reload_timeout_ms !== 30_000 || liveReload.budget?.poll_interval_ms !== 20 ||
    liveReload.edit?.from !== "reload-before" || liveReload.edit?.to !== "reload-after" ||
    liveReload.phases?.cold !== "first measured edit after a fresh process reaches readiness" ||
    liveReload.phases?.warm !== "measured edit after two unmeasured edits in the same fresh process" ||
    JSON.stringify([...liveReloadRunnerIds].sort()) !== JSON.stringify(["bun", "entr+cc", "jet-dev", "nodemon", "vite"]) ||
    !equalStringArrays(liveReload.fairness ?? [], ["same source edit", "same observable readiness signal", "fresh process per sample", "median cold and warm reload samples"]) ||
    memorySafetyFuzz.schema !== "gauntlet-axis-memory-safety-fuzz-v1" ||
    memorySafetyFuzz.metric !== "memory_safety_findings" ||
    memorySafetyFuzz.corpus?.path !== "fuzz-input.bin" || memorySafetyFuzz.corpus?.generator !== "xorshift32-v1" ||
    memorySafetyFuzz.corpus?.seed !== 2272 || memorySafetyFuzz.corpus?.case_count !== 128 ||
    memorySafetyFuzz.corpus?.bytes_per_case !== 64 || memorySafetyFuzz.budget?.wall_timeout_ms !== 30_000 ||
    memorySafetyFuzz.budget?.cpu_seconds !== 10 || memorySafetyFuzz.budget?.memory_mb !== 512 ||
    memorySafetyFuzz.oracle?.algorithm !== "memory-safety-case-summary-v1" ||
    memorySafetyFuzz.oracle?.output !== "cases {case_count} valid {valid} boundary {boundary} oob {oob} use_after_free {use_after_free} wrong_output {wrong_output} bytes {byte_count} checksum {u32_sum} semantic {semantic}\n" ||
    JSON.stringify([...memorySafetyRunnerIds].sort()) !== JSON.stringify(["c", "jet-default", "rust", "zig"]) ||
    !equalStringArrays(memorySafetyFuzz.fairness ?? [], ["same generated input file", "same timeout and resource budget", "sanitizer or equivalent finding evidence", "deduplicate each finding before close"])) {
    throw new Error("gauntlet report is missing a required comparison axis");
  }
  if (matrix) {
    const matrixCells = (matrix.cells ?? []).map((cell) => cell.id);
    if (new Set(matrixCells).size !== matrixCells.length || corpus.matrix_cell_count !== matrixCells.length || !equalStringArrays(corpus.allowed_uncovered_cells, MATRIX_UNCOVERED_DEFAULTS)) {
      throw new Error("measurement manifest matrix denominator does not match the approved matrix");
    }
    if (corpus.allowed_uncovered_cells.some((cell) => !matrixCells.includes(cell))) {
      throw new Error("measurement manifest allows an unknown uncovered matrix cell");
    }
  }
  const entries = [];
  for (const row of manifest.entries) {
    if (typeof row.name !== "string" || !Object.hasOwn(row, "python") || typeof row.jet !== "string") throw new Error("measurement manifest row is incomplete");
    const entryDir = manifestSource(entriesDir, row.name);
    const jet = await manifestSourceMetrics(entryDir, row.jet);
    const python = row.python === null ? null : await manifestSourceMetrics(entryDir, row.python);
    const comparison = python ? {
      loc_ratio: python.loc === 0 ? null : jet.loc / python.loc,
      source_token_delta: jet.source_tokens - python.source_tokens,
      loc_pass: jet.loc <= contract.loc_ratio_max * python.loc,
      token_pass: jet.source_tokens < python.source_tokens,
    } : null;
    entries.push({ name: row.name, jet, python, comparison });
  }
  const pairs = entries.filter((entry) => entry.python);
  const sum = (language, metric) => pairs.reduce((total, entry) => total + entry[language][metric], 0);
  const jetLoc = sum("jet", "loc");
  const pythonLoc = sum("python", "loc");
  const jetTokens = sum("jet", "source_tokens");
  const pythonTokens = sum("python", "source_tokens");
  return {
    contract,
    entries,
    aggregate: {
      eligible_entries: pairs.length,
      jet: { loc: jetLoc, source_tokens: jetTokens },
      python: { loc: pythonLoc, source_tokens: pythonTokens },
      loc_ratio: pythonLoc === 0 ? null : jetLoc / pythonLoc,
      source_token_delta: jetTokens - pythonTokens,
      loc_pass: jetLoc <= contract.loc_ratio_max * pythonLoc,
      token_pass: jetTokens < pythonTokens,
    },
    coverage: {
      entry_count: entries.length,
      expected_entry_count: corpus.entry_count,
      python_pair_count: pairs.length,
      expected_python_pair_count: corpus.python_pair_count,
      denominator_pass: entries.length === corpus.entry_count && pairs.length === corpus.python_pair_count,
    },
  };
}

function validateEntryShape(item, matrix) {
  const entry = item.entry;
  const issues = [];
  if (!item.nameDeclared) issues.push(`${item.directoryName}: entry.json must declare name`);
  if (entry.name !== item.directoryName) issues.push(`${item.directoryName}: entry.name is ${JSON.stringify(entry.name)}`);
  if (!ENTRY_MODES.includes(entry.mode)) issues.push(`${entry.name}: unsupported mode ${entry.mode ?? "missing"}`);
  if (!Array.isArray(entry.languages) || entry.languages.length === 0) {
    issues.push(`${entry.name}: languages must be a non-empty array`);
  } else {
    if (new Set(entry.languages).size !== entry.languages.length) issues.push(`${entry.name}: duplicate language declaration`);
    if (!entry.languages.includes("jet")) issues.push(`${entry.name}: missing default Jet rail`);
    for (const language of entry.languages) {
      if (typeof language !== "string" || !LANGUAGE_FILES[baseLanguage(language)]) issues.push(`${entry.name}: unsupported language ${language}`);
    }
  }
  const nonApplicable = entry.non_applicable;
  if (nonApplicable !== undefined) {
    if (!nonApplicable || typeof nonApplicable !== "object" || Array.isArray(nonApplicable)) {
      issues.push(`${entry.name}: non_applicable must be an object`);
    } else {
      for (const [language, fact] of Object.entries(nonApplicable)) {
        if (!(entry.languages ?? []).includes(language)) issues.push(`${entry.name}/${language}: non_applicable language is not declared`);
        if (!fact || typeof fact !== "object" || Array.isArray(fact)) {
          issues.push(`${entry.name}/${language}: non_applicable fact must be an object`);
          continue;
        }
        for (const field of ["reason", "evidence"]) {
          if (typeof fact[field] !== "string" || fact[field].trim().length === 0) {
            issues.push(`${entry.name}/${language}: non_applicable fact missing ${field}`);
          }
        }
      }
    }
  }
  if (!Array.isArray(entry.cells) || entry.cells.length === 0) {
    issues.push(`${entry.name}: cells must be a non-empty array`);
  } else {
    const knownCells = new Set((matrix.cells ?? []).map((cell) => cell.id));
    for (const cell of entry.cells) if (!knownCells.has(cell)) issues.push(`${entry.name}: unknown matrix cell ${cell}`);
  }
  const authoring = entry.authoring ?? {};
  const expert = entry.expert ?? {};
  for (const language of entry.languages ?? []) {
    if (nonApplicable?.[language]) continue;
    const record = authoring[language] ?? expert[language];
    if (!record || typeof record !== "object") {
      issues.push(`${entry.name}/${language}: missing authoring or sourced provenance`);
      continue;
    }
    if (record.sourced === true) {
      for (const field of ["author", "source", "license"]) {
        if (typeof record[field] !== "string" || record[field].length === 0) issues.push(`${entry.name}/${language}: sourced provenance missing ${field}`);
      }
    } else {
      for (const field of ["author", "notes"]) {
        if (typeof record[field] !== "string") issues.push(`${entry.name}/${language}: authoring provenance missing ${field}`);
      }
      for (const field of ["turns", "retries"]) {
        if (!Number.isInteger(record[field]) || record[field] < 0) issues.push(`${entry.name}/${language}: authoring provenance missing valid ${field}`);
      }
      if (!Array.isArray(record.diagnosticsHit)) issues.push(`${entry.name}/${language}: authoring provenance missing diagnosticsHit`);
    }
  }
  return issues;
}
async function validateCorpus(entriesDir, loaded, skipped, matrix, manifest, fullScope) {
  const issues = [];
  const items = loaded.map((item) => ({ ...item, directoryName: path.basename(item.dir) }));
  for (const item of items) issues.push(...validateEntryShape(item, matrix));
  if (!fullScope) return issues;
  if (skipped.length) issues.push(`full corpus has skipped entries: ${skipped.map((item) => item.name).join(", ")}`);
  if (!manifest?.corpus) {
    issues.push("full corpus is missing the measurement denominator manifest");
    return issues;
  }
  const actualNames = items.map((item) => item.directoryName);
  if (actualNames.length !== manifest.corpus.entry_count || !equalStringArrays(actualNames, manifest.corpus.entry_names)) {
    issues.push("full corpus entry names/count do not match the frozen measurement denominator");
  }
  const matrixIds = (matrix.cells ?? []).map((cell) => cell.id);
  const covered = sortedUnique(items.flatMap((item) => item.entry.cells ?? []));
  const ownersByCell = new Map();
  for (const item of items) {
    for (const cell of item.entry.cells ?? []) ownersByCell.set(cell, (ownersByCell.get(cell) ?? 0) + 1);
  }
  const unexpectedUncovered = matrixIds.filter((id) => !covered.includes(id) && !manifest.corpus.allowed_uncovered_cells.includes(id));
  const unknownCoverage = covered.filter((id) => !matrixIds.includes(id));
  if (unexpectedUncovered.length) issues.push(`full matrix cells are uncovered: ${unexpectedUncovered.join(", ")}`);
  if (unknownCoverage.length) issues.push(`corpus declares unknown matrix cells: ${unknownCoverage.join(", ")}`);
  const duplicateCoverage = [...ownersByCell.entries()].filter(([, count]) => count > 1).map(([cell]) => cell);
  if (duplicateCoverage.length) issues.push(`matrix cells have multiple corpus owners: ${duplicateCoverage.join(", ")}`);
  const rowsByName = new Map((manifest.entries ?? []).map((row) => [row.name, row]));
  for (const item of items) {
    const row = rowsByName.get(item.directoryName);
    if (!row) {
      issues.push(`${item.directoryName}: missing measurement manifest row`);
      continue;
    }
    if ((row.python === null) !== !(item.entry.languages ?? []).includes("python")) {
      issues.push(`${item.directoryName}: Python manifest pairing disagrees with declared rails`);
    }
    for (const language of item.entry.languages ?? []) {
      if (item.entry.non_applicable?.[language]) continue;
      const sourceFile = LANGUAGE_FILES[baseLanguage(language)];
      if (!(await exists(path.join(item.dir, language, sourceFile)))) issues.push(`${item.directoryName}/${language}: declared source is missing`);
    }
  }
  return issues;
}

async function discoverJetArtifact(dir) {
  const preferred = path.join(dir, "build", "main");
  if (await exists(preferred)) return preferred;
  const found = [];
  async function walk(current) {
    for (const item of await fs.readdir(current, { withFileTypes: true })) {
      if (item.name === ".jet" || item.name === "zig-cache" || item.name === "zig-global-cache") continue;
      const full = path.join(current, item.name);
      if (item.isDirectory()) await walk(full);
      else if (item.name === "main" || item.name === "main.exe") {
        try {
          const stat = await fs.stat(full);
          if ((stat.mode & 0o111) !== 0) found.push(full);
        } catch { /* file disappeared */ }
      }
    }
  }
  await walk(dir);
  return found[0] ?? null;
}

function buildCommand(language, jetBin, sourceDir) {
  if (language === "jet") return [jetBin, "build", "main.jet"];
  if (language === "rust") return ["rustc", "--edition=2021", "-O", "main.rs", "-o", "main-rust"];
  if (language === "c") return ["gcc", "-O2", "main.c", "-o", "main-c", "-lm"];
  if (language === "zig") return ["zig", "build-exe", "-O", "ReleaseFast", "--cache-dir", "zig-cache", "--global-cache-dir", "zig-global-cache", "main.zig"];
  if (language === "go") return ["env", "GO111MODULE=off", `GOCACHE=${path.join(sourceDir, "go-cache")}`, "go", "build", "-o", "main-go", "main.go"];
  return null;
}

function runCommand(language, sourceDir, artifact, args) {
  language = baseLanguage(language);
  if (language === "jet") return [artifact, ...args];
  if (language === "python") return ["python3", "main.py", ...args];
  if (language === "js" || language === "node") return ["node", "main.mjs", ...args];
  return [artifact, ...args];
}

async function verify(cwd, command, expected, { full = false, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  const result = await runProcess(cwd, command, { full, timeoutMs });
  const error = mismatch(expected, result.stdout);
  if (result.code !== 0) return `exit ${result.code}${result.stderr ? `: ${result.stderr.toString("utf8").trim().slice(0, 300)}` : ""}`;
  return error;
}

async function verifySequence(cwd, commands, expected, reset = null, { full = false, timeoutMs = DEFAULT_TIMEOUT_MS } = {}) {
  if (reset) await reset();
  const output = [];
  for (const command of commands) {
    const result = await runProcess(cwd, command, { full, timeoutMs });
    output.push(result.stdout);
    if (result.code !== 0) return `exit ${result.code}${result.stderr ? `: ${result.stderr.toString("utf8").trim().slice(0, 300)}` : ""}`;
  }
  return mismatch(expected, Buffer.concat(output));
}

async function makeStateReset(sourceDir) {
  const baseline = `${sourceDir}.baseline`;
  await fs.rm(baseline, { recursive: true, force: true });
  await fs.cp(sourceDir, baseline, { recursive: true });
  return async () => {
    for (const item of await fs.readdir(sourceDir)) await fs.rm(path.join(sourceDir, item), { recursive: true, force: true });
    await fs.cp(baseline, sourceDir, { recursive: true });
  };
}

async function measureSequenceRuns(cwd, commands, count, reset, { full = false } = {}) {
  const samples = [];
  for (let i = 0; i < count; i += 1) {
    await reset();
    samples.push(await timedSequence(cwd, commands, { full }));
  }
  return summarizeSamples(samples);
}

async function buildAndMeasure(language, sourceDir, jetBin, overrideCommand = null) {
  const base = baseLanguage(language);
  const command = overrideCommand ?? buildCommand(base, jetBin, sourceDir);
  if (!command) return { supported: true, command: null, build: null, artifact: null };
  const cold = await timedProcess(sourceDir, command);
  const warm = cold.exit_code === 0 ? await timedProcess(sourceDir, command) : null;
  const artifact = base === "jet" ? await discoverJetArtifact(sourceDir) : path.join(sourceDir, {
    rust: "main-rust", c: "main-c", zig: "main", go: "main-go",
  }[base]);
  const failure = cold.exit_code !== 0 ? `cold build exit ${cold.exit_code}` : warm?.exit_code !== 0 ? `warm build exit ${warm?.exit_code}` : !await exists(artifact) ? "build produced no executable" : null;
  return { supported: true, command, build: { cold, warm }, artifact, failure };
}

function commandFromSpec(value, language, jetBin, fallback = null) {
  if (value === undefined || value === null) return fallback;
  const command = Array.isArray(value) ? [...value] : String(value).trim().split(/\s+/);
  if (command[0] === "jet") command[0] = jetBin;
  if (language === "jet" && command[0] === "build") command.unshift(jetBin);
  return command;
}

function languageSpecValue(value, language) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return value;
  return value[language] ?? value.default;
}

async function configuredBuildAndMeasure(language, sourceDir, jetBin, entry) {
  const configured = languageSpecValue(entry.spec?.build ?? entry.build, language);
  const command = commandFromSpec(configured, language, jetBin, null);
  if (!command) return { supported: true, command: null, build: null, artifact: null, failure: null };
  const full = entry.spec?.fullShell === true;
  const cold = await timedProcess(sourceDir, command, { full });
  const warm = cold.exit_code === 0 ? await timedProcess(sourceDir, command, { full }) : null;
  const failure = cold.exit_code !== 0 ? `cold build exit ${cold.exit_code}` : warm?.exit_code !== 0 ? `warm build exit ${warm?.exit_code}` : null;
  return { supported: true, command, build: { cold, warm }, artifact: null, failure };
}

async function copyRelativeFile(sourceDir, stageDir, relative) {
  const source = path.join(sourceDir, relative);
  if (!(await exists(source))) return false;
  const target = path.join(stageDir, relative);
  await fs.mkdir(path.dirname(target), { recursive: true });
  await fs.copyFile(source, target);
  return true;
}

async function artifactBytes(sourceDir) {
  const roots = [];
  for (const name of ["build", "dist", "out"]) if (await exists(path.join(sourceDir, name))) roots.push(path.join(sourceDir, name));
  if (!roots.length) roots.push(sourceDir);
  let total = 0;
  async function walk(dir, rootOnly) {
    for (const item of await fs.readdir(dir, { withFileTypes: true })) {
      const full = path.join(dir, item.name);
      if (item.isDirectory()) await walk(full, rootOnly);
      else if ((item.name.endsWith(".wasm") || item.name.endsWith(".js")) && (rootOnly || !["main.mjs", "runner.mjs"].includes(item.name))) total += (await fs.stat(full)).size;
    }
  }
  for (const root of roots) await walk(root, roots[0] !== sourceDir);
  return total === 0 ? null : total;
}

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolve(port));
    });
  });
}

async function waitForReady(child, port, service) {
  const protocol = service.protocol ?? "http";
  const started = performance.now();
  while (performance.now() - started < 10000) {
    if (child.exitCode !== null) throw new Error(`service exited ${child.exitCode}${child.stderrText() ? `: ${child.stderrText()}` : ""}`);
    if (protocol === "line") {
      const ready = service.ready ?? { send: "ready\n", expect: "ready\n" };
      const result = await lineProbe(port, ready, 500);
      if (result.ok && result.body === ready.expect) return { seconds: (performance.now() - started) / 1000, result };
    } else {
      const result = await httpProbe(port, { method: "GET", path: service.readyPath }, 500);
      if (result.ok) return { seconds: (performance.now() - started) / 1000, result };
    }
  }
  throw new Error(`service did not answer ${protocol === "line" ? "line readiness probe" : service.readyPath}`);
}

async function runService(language, sourceDir, artifact, entry, commandForOverride = null) {
  const service = entry.spec?.service ?? entry.service ?? {};
  const protocol = service.protocol ?? "http";
  const probes = service.probe ?? [];
  if (!service.portArg || probes.length === 0 || !["http", "line"].includes(protocol) || (protocol === "http" && !service.readyPath) || (protocol === "line" && !service.ready)) return { failure: "service requires portArg, readiness, and probe" };
  const commandFor = commandForOverride ?? ((port) => runCommand(language, sourceDir, artifact, [String(port)]));
  let child = null;
  let startupSeconds = null;
  let failure = null;
  try {
    const port = await freePort();
    child = startProcess(sourceDir, commandFor(port), { full: entry.spec?.fullShell === true });
    startupSeconds = (await waitForReady(child, port, service)).seconds;
    const verification = await probeSequence(port, probes, protocol);
    if (!verification.ok) failure = `probe ${verification.index} failed: ${verification.reason}`;
    const firstExit = await waitForExit(child, 1000);
    if (firstExit.code !== 0 && !failure) failure = `verification service exit ${firstExit.code ?? firstExit.signal}`;
    if (firstExit.code === null) {
      if (!failure) failure = "verification shutdown did not produce a clean exit";
      stopProcess(child);
      await waitForExit(child);
    }
  } catch (error) {
    failure = error.message;
    if (child) { stopProcess(child); await waitForExit(child); }
  }
  if (failure) return { failure, startupSeconds };

  const port = await freePort();
  child = startProcess(sourceDir, commandFor(port), { full: entry.spec?.fullShell === true });
  let ready;
  try {
    ready = await waitForReady(child, port, service);
  } catch (error) {
    stopProcess(child);
    await waitForExit(child);
    return { failure: error.message, startupSeconds };
  }
  let rssKb = await readRssKb(child.pid);
  const rssTimer = setInterval(() => {
    readRssKb(child.pid).then((value) => {
      if (Number.isFinite(value)) rssKb = Math.max(rssKb ?? 0, value);
    });
  }, 20);
  const latencies = [];
  const repeatProbes = probes.slice(0, -1);
  let measurementFailure = null;
  for (let repeat = 0; repeat < 50 && !measurementFailure; repeat += 1) {
    for (const probe of repeatProbes) {
      const result = await serviceProbe(port, probe, protocol);
      latencies.push(result.latencyMs);
      if (!probeMatches(probe, result, protocol)) {
        measurementFailure = `probe failed during measurement: ${result.error ?? "response mismatch"}`;
        break;
      }
    }
  }
  if (!measurementFailure) {
    const shutdown = await serviceProbe(port, probes[probes.length - 1], protocol);
    latencies.push(shutdown.latencyMs);
    if (!probeMatches(probes.at(-1), shutdown, protocol)) measurementFailure = `shutdown probe failed: ${shutdown.error ?? "response mismatch"}`;
  }
  clearInterval(rssTimer);
  const finalRssKb = await readRssKb(child.pid);
  if (Number.isFinite(finalRssKb)) rssKb = Math.max(rssKb ?? 0, finalRssKb);
  const exit = await waitForExit(child, 5000);
  if (exit.code === null) { stopProcess(child); await waitForExit(child); }
  return {
    failure: measurementFailure ?? (exit.code !== 0 ? `service clean exit ${exit.code ?? exit.signal}` : null),
    startupSeconds,
    latencyMs: { median: median(latencies), p99: percentile(latencies, 0.99) },
    rssKb,
    cleanExit: exit.code === 0,
    exitCode: exit.code,
    readySeconds: ready.seconds,
  };
}

function serviceMetrics(service) {
  return {
    startupSeconds: service.startupSeconds ?? null,
    latencyMs: service.latencyMs ?? { median: null, p99: null },
    rssKb: service.rssKb ?? null,
    cleanExit: service.cleanExit ?? false,
    exitCode: service.exitCode ?? null,
  };
}

async function startPeer(sourceDir, peer) {
  const port = peer.port;
  const child = startProcess(sourceDir, ["python3", peer.script, String(port)]);
  try {
    await waitForTcp(port);
    return child;
  } catch (error) {
    stopProcess(child);
    await waitForExit(child);
    throw error;
  }
}

async function measureRuns(cwd, command, count, { full = false, reset = null } = {}) {
  const samples = [];
  for (let i = 0; i < count; i += 1) {
    if (reset) await reset();
    samples.push(await timedProcess(cwd, command, { full }));
  }
  return summarizeSamples(samples);
}

function ratioVerdict(ratio) {
  if (!Number.isFinite(ratio)) return null;
  if (ratio < 1) return "win";
  if (ratio <= 1.05) return "parity";
  return "loss";
}

function comparisons(entry, languages, rows, tiers = {}) {
  const jet = rows.jet;
  const metrics = comparisonMetrics(entry.mode);
  const policy = tierPolicy(entry.mode);
  const requiredTiers = Object.entries(policy)
    .filter(([, value]) => value.required)
    .map(([tier]) => tier);
  const jetTiersReady = requiredTiers.every((tier) => tiers[tier]?.status === "ok");
  const output = {};
  for (const language of languages.filter((item) => item !== "jet")) {
    const peer = rows[language];
    const comparison = {
      status: peer?.status ?? "unavailable",
      applicable: peer?.status !== "not_applicable",
      basis: peer?.status === "not_applicable" ? "declared-non-applicability" : "jet-aot-and-run",
      reason: peer?.status === "not_applicable" ? peer.reason : undefined,
      evidence: peer?.status === "not_applicable" ? peer.evidence : undefined,
      jet_tiers_ready: jetTiersReady,
      primary_metric: primaryMetric(entry.mode),
      metrics: {},
      tiers: {},
      verdicts: {},
    };
    if (peer?.status === "not_applicable") {
      output[language] = comparison;
      continue;
    }
    for (const tier of Object.keys(policy)) {
      const tierReady = tiers[tier]?.status === "ok";
      const jetMetrics = tier === "aot" ? jet?.metrics : tiers[tier]?.metrics;
      const tierComparison = { status: tiers[tier]?.status ?? "unavailable", metrics: {} };
      for (const metric of metrics) {
        const jetValue = Number.isFinite(jetMetrics?.[metric]) ? jetMetrics[metric] : null;
        const peerValue = Number.isFinite(peer?.metrics?.[metric]) ? peer.metrics[metric] : null;
        const ratio = tierReady && peer?.status === "ok" && peerValue !== null && peerValue !== 0 && jetValue !== null
          ? jetValue / peerValue
          : null;
        tierComparison.metrics[metric] = {
          jet: jetValue,
          peer: peerValue,
          ratio,
          verdict: ratioVerdict(ratio),
        };
      }
      comparison.tiers[tier] = tierComparison;
      comparison.verdicts[tier] = tierComparison.metrics[comparison.primary_metric]?.verdict ?? null;
    }
    comparison.metrics = comparison.tiers.aot?.metrics ?? {};
    output[language] = comparison;
  }
  return output;
}

function emptyJetTiers(entry, dev, reason) {
  const policy = tierPolicy(entry.mode);
  return Object.fromEntries(["aot", "run", "dev"].map((tier) => {
    const tierPolicyValue = policy[tier];
    if (!tierPolicyValue) return [tier, { applicable: false, required: false, status: "not_applicable", metrics: {} }];
    if (tier === "dev" && !dev) return [tier, unavailableTier(tierPolicyValue.required, "jet dev is unavailable")];
    return [tier, unavailableTier(tierPolicyValue.required, reason)];
  }));
}

function jetTierCommands(entry, tier, jetBin) {
  const prefix = tier === "run"
    ? [jetBin, "run", "main.jet", "--"]
    : [jetBin, "dev", "--watch=off", "main.jet", "--"];
  if (entry.mode === "batch-steps") return (entry.spec?.steps ?? []).map((args) => [...prefix, ...args]);
  return [[...prefix, ...(entry.spec?.args ?? [])]];
}

async function stageEntry(entryDir, entry, runDir, jetBin, selectedRuns, dev) {
  const entryStage = path.join(runDir, entry.name);
  const languages = entry.languages ?? [];
  const provenance = {
    entry_json_sha256: await fileSha256(path.join(entryDir, "entry.json")),
    corpus_tree_sha256: await treeSha256(entryDir),
  };
  const finish = (result) => ({ ...result, provenance });
  const failedRows = (reason) => Object.fromEntries(languages.map((language) => [language, {
    language,
    status: "broken",
    disqualified: true,
    reason,
    metrics: {},
    diagnostics: [],
  }]));
  await fs.mkdir(entryStage, { recursive: true });
  const serviceMode = entry.mode === "service";
  const expectedPath = path.join(entryDir, entry.spec?.expected ?? "expected.out");
  if (!serviceMode && !(await exists(expectedPath))) return finish({ entry, status: "broken", reason: "missing expected output", languages, rows: failedRows("missing expected output"), comparisons: {}, jet_tiers: emptyJetTiers(entry, dev, "missing expected output") });
  if (!serviceMode) provenance.expected_sha256 = await fileSha256(expectedPath);
  const expected = serviceMode ? Buffer.alloc(0) : await fs.readFile(expectedPath);
  const fixture = entry.spec?.fixtureGen;
  const commonFixtures = path.join(entryStage, "fixtures");
  let generatedFixture = null;
  if (fixture) {
    const fixtureDir = path.join(entryDir, path.dirname(fixture.script));
    if (!(await exists(fixtureDir))) {
      const reason = `missing fixture directory ${fixture.script}`;
      return finish({ entry, status: "broken", reason, languages, rows: failedRows(reason), comparisons: {}, jet_tiers: emptyJetTiers(entry, dev, reason) });
    }
    await fs.cp(fixtureDir, commonFixtures, { recursive: true });
    const output = fixture.out;
    await fs.mkdir(path.dirname(path.join(entryStage, output)), { recursive: true });
    const fixtureCache = process.env.JET_GAUNTLET_FIXTURE_CACHE;
    if (fixtureCache) {
      const cached = path.join(fixtureCache, entry.name, output);
      if (!(await exists(cached))) {
        const reason = `fixture cache is missing ${entry.name}/${output}`;
        return finish({ entry, status: "broken", reason, languages, rows: failedRows(reason), comparisons: {}, jet_tiers: emptyJetTiers(entry, dev, reason) });
      }
      await fs.cp(cached, path.join(entryStage, output), { recursive: true });
    } else {
      const generated = await runProcess(entryStage, ["python3", fixture.script, output]);
      if (generated.code !== 0) {
        const reason = `fixture generator exit ${generated.code}: ${generated.stderr.toString("utf8").trim().slice(0, 300)}`;
        return finish({ entry, status: "broken", reason, languages, rows: failedRows(reason), comparisons: {}, jet_tiers: emptyJetTiers(entry, dev, reason) });
      }
    }
    generatedFixture = path.join(entryStage, output);
    provenance.fixture_sha256 = await pathSha256(generatedFixture);
  }

  const rows = {};
  const resets = {};
  for (const language of languages) {
    const sourceDir = path.join(entryDir, language);
    const sourceFile = LANGUAGE_FILES[baseLanguage(language)];
    const row = { language, status: "broken", metrics: {}, diagnostics: [] };
    rows[language] = row;
    const nonApplicable = entry.non_applicable?.[language];
    if (nonApplicable) {
      row.status = "not_applicable";
      row.applicable = false;
      row.disqualified = false;
      row.reason = nonApplicable.reason;
      row.evidence = nonApplicable.evidence;
      row.verification = { status: "not_applicable", kind: "declared_non_applicability", reason: nonApplicable.reason };
      row.provenance = {
        source: null,
        source_sha256: null,
        base_language: baseLanguage(language),
        authoring: null,
        non_applicability: nonApplicable,
      };
      continue;
    }
    if (!sourceFile) {
      row.reason = "unsupported language";
      row.disqualified = true;
      continue;
    }
    if (!(await exists(path.join(sourceDir, sourceFile)))) {
      row.reason = `missing ${language}/${sourceFile}`;
      row.disqualified = true;
      continue;
    }
    const stagedSource = path.join(entryStage, language);
    await fs.cp(sourceDir, stagedSource, { recursive: true });
    if (fixture) await fs.cp(commonFixtures, path.join(stagedSource, "fixtures"), { recursive: true });
    for (const item of await fs.readdir(entryDir, { withFileTypes: true })) {
      if (!item.isFile() || item.name === "entry.json" || item.name === path.basename(expectedPath)) continue;
      await fs.copyFile(path.join(entryDir, item.name), path.join(stagedSource, item.name));
    }
    let fixtureReset = null;
    if (fixture && generatedFixture) {
      const fixtureTarget = path.join(stagedSource, fixture.out);
      fixtureReset = async () => {
        await fs.rm(fixtureTarget, { recursive: true, force: true });
        await fs.mkdir(path.dirname(fixtureTarget), { recursive: true });
        await fs.cp(generatedFixture, fixtureTarget, { recursive: true });
      };
      await fixtureReset();
    }
    if (entry.spec?.peer && !(await copyRelativeFile(entryDir, stagedSource, entry.spec.peer.script))) {
      row.reason = `missing peer script ${entry.spec.peer.script}`;
      row.disqualified = true;
      continue;
    }
    if (!serviceMode) await fs.copyFile(expectedPath, path.join(stagedSource, path.basename(expectedPath)));
    const webMode = entry.mode === "web" || entry.mode === "web-app";
    const buildOverride = webMode ? null : commandFromSpec(languageSpecValue(entry.spec?.build ?? null, language), language, jetBin, null);
    const build = webMode ? await configuredBuildAndMeasure(language, stagedSource, jetBin, entry) : await buildAndMeasure(language, stagedSource, jetBin, buildOverride);
    row.build = build.build ? { command: build.command, ...build.build } : null;
    row.metrics = await sourceMetrics(sourceDir, sourceFile);
    row.provenance = {
      source: path.relative(entryDir, path.join(sourceDir, sourceFile)).replaceAll(path.sep, "/"),
      source_sha256: row.metrics.source_sha256,
      base_language: baseLanguage(language),
      authoring: entry.authoring?.[language] ?? entry.expert?.[language] ?? null,
      peer_script: entry.spec?.peer?.script ?? null,
      peer_script_sha256: entry.spec?.peer ? await fileSha256(path.join(entryDir, entry.spec.peer.script)) : null,
    };
    row.disqualified = false;
    if (build.failure) {
      row.reason = build.failure;
      row.disqualified = true;
      row.verification = { status: "failed", kind: "build", reason: build.failure };
      const missingToolchain = build.build?.cold?.exit_code === 127 && (build.build.cold.error || /command not found/i.test(build.build.cold.stderr ?? ""));
      if (missingToolchain) {
        row.missing_toolchain = true;
        console.warn(`WARN ${entry.name}/${language}: ${build.failure}; toolchain may be missing`);
      }
      continue;
    }
    const artifact = build.artifact;

    if (entry.mode === "batch-steps") {
      const steps = entry.spec?.steps ?? [];
      const commands = steps.map((args) => runCommand(language, stagedSource, artifact, args));
      const reset = await makeStateReset(stagedSource);
      resets[language] = reset;
      const verification = await verifySequence(stagedSource, commands, expected, reset);
      if (verification) {
        row.reason = verification;
        row.disqualified = true;
        row.verification = { status: "failed", kind: "byte_exact_stdout", reason: verification };
        continue;
      }
      row.status = "ok";
      row.verification = { status: "passed", kind: "byte_exact_stdout" };
      row.command = commands;
      const runs = selectedRuns ?? (entry.perf ? 7 : 3);
      row.runtime = await measureSequenceRuns(stagedSource, commands, runs, reset);
      if (row.runtime.samples.some((sample) => sample.exit_code !== 0)) {
        row.status = "broken";
        row.reason = "measured run exited nonzero";
        row.disqualified = true;
        row.verification = { status: "failed", kind: "timed_run", reason: row.reason };
        continue;
      }
      const binary = artifact && await exists(artifact) ? (await fs.stat(artifact)).size : null;
      row.metrics = { ...row.metrics, ...runtimeMetrics(row.runtime), ...buildMetrics(row.build), binary_bytes: binary };
      continue;
    }

    if (entry.mode === "service") {
      const service = await runService(language, stagedSource, artifact, entry);
      row.command = runCommand(language, stagedSource, artifact, ["<port>"]);
      row.metrics = {
        ...row.metrics,
        ...buildMetrics(row.build),
        runtime_wall_seconds: null,
        runtime_first_stdout_seconds: null,
        runtime_peak_rss_kb: service.rssKb ?? null,
        binary_bytes: artifact && await exists(artifact) ? (await fs.stat(artifact)).size : null,
        service_startup_seconds: service.startupSeconds ?? null,
        service_latency_ms_p50: service.latencyMs?.median ?? null,
        service_latency_ms_p99: service.latencyMs?.p99 ?? null,
        startupSeconds: service.startupSeconds ?? null,
        latencyMs: service.latencyMs ?? { median: null, p99: null },
        rssKb: service.rssKb ?? null,
        cleanExit: service.cleanExit ?? false,
        exitCode: service.exitCode ?? null,
      };
      if (service.failure) {
        row.reason = service.failure;
        row.disqualified = true;
        row.verification = { status: "failed", kind: "service_probe_sequence", reason: service.failure };
        continue;
      }
      row.status = "ok";
      row.verification = { status: "passed", kind: "service_probe_sequence" };
      continue;
    }

    if (webMode) {
      const configuredRun = languageSpecValue(entry.spec?.run ?? entry.run, language);
      const command = commandFromSpec(configuredRun, language, jetBin, ["node", "runner.mjs"]);
      const full = entry.spec?.fullShell === true;
      const verification = await verify(stagedSource, command, expected, { full });
      if (verification) {
        row.reason = verification;
        row.disqualified = true;
        row.verification = { status: "failed", kind: "byte_exact_stdout", reason: verification };
        continue;
      }
      row.status = "ok";
      row.verification = { status: "passed", kind: "byte_exact_stdout" };
      row.command = command;
      const runs = selectedRuns ?? (entry.perf ? 7 : 3);
      row.runtime = await measureRuns(stagedSource, command, runs, { full });
      if (row.runtime.samples.some((sample) => sample.exit_code !== 0)) {
        row.status = "broken";
        row.reason = "measured run exited nonzero";
        row.disqualified = true;
        row.verification = { status: "failed", kind: "timed_run", reason: row.reason };
        continue;
      }
      row.metrics = {
        ...row.metrics,
        ...runtimeMetrics(row.runtime),
        ...buildMetrics(row.build),
        binary_bytes: await artifactBytes(stagedSource),
        firstResultSeconds: row.runtime.median.time_to_first_stdout_seconds,
        artifactBytes: row.metrics.binary_bytes,
      };
      continue;
    }

    let peerChild = null;
    const stopPeer = async () => {
      if (peerChild) {
        stopProcess(peerChild);
        await waitForExit(peerChild);
        peerChild = null;
      }
    };
    if (entry.spec?.peer) {
      try {
        peerChild = await startPeer(stagedSource, entry.spec.peer);
      } catch (error) {
        row.reason = `peer unavailable: ${error.message}`;
        row.disqualified = true;
        continue;
      }
    }
    if (fixtureReset) resets[language] = fixtureReset;
    const command = runCommand(language, stagedSource, artifact, entry.spec?.args ?? []);
    if (fixtureReset) await fixtureReset();
    const verification = await verify(stagedSource, command, expected);
    if (verification) {
      row.reason = verification;
      row.disqualified = true;
      row.verification = { status: "failed", kind: "byte_exact_stdout", reason: verification };
      await stopPeer();
      continue;
    }
    row.status = "ok";
    row.verification = { status: "passed", kind: "byte_exact_stdout" };
    row.command = command;
    const runs = selectedRuns ?? (entry.perf ? 7 : 3);
    row.runtime = await measureRuns(stagedSource, command, runs, { reset: fixtureReset });
    if (row.runtime.samples.some((sample) => sample.exit_code !== 0)) {
      row.status = "broken";
      row.reason = "measured run exited nonzero";
      row.disqualified = true;
      row.verification = { status: "failed", kind: "timed_run", reason: row.reason };
      await stopPeer();
      continue;
    }
    const binary = artifact && await exists(artifact) ? (await fs.stat(artifact)).size : null;
    row.metrics = { ...row.metrics, ...runtimeMetrics(row.runtime), ...buildMetrics(row.build), binary_bytes: binary };
    await stopPeer();
  }

  const jetRow = rows.jet;
  const tiers = {};
  const jetDir = path.join(entryStage, "jet");
  const jetSourceAvailable = await exists(jetDir);
  const jetFixtureReset = generatedFixture && jetSourceAvailable ? async () => {
    const target = path.join(jetDir, fixture.out);
    await fs.rm(target, { recursive: true, force: true });
    await fs.cp(generatedFixture, target, { recursive: true });
  } : null;
  const TIER_TIMEOUT_MS = timeoutFromEnv("JET_GAUNTLET_TIER_TIMEOUT_MS", Math.min(DEFAULT_TIMEOUT_MS, 180_000));
  for (const tier of ["aot", "run", "dev"]) {
    const policy = tierPolicy(entry.mode)[tier];
    if (!policy) {
      tiers[tier] = { applicable: false, required: false, status: "not_applicable", metrics: {} };
      continue;
    }
    if (tier === "dev" && !dev) {
      tiers[tier] = unavailableTier(policy.required, "jet dev is unavailable");
      continue;
    }
    if (tier === "aot") {
      tiers[tier] = {
        applicable: true,
        required: policy.required,
        status: jetRow?.status === "ok" ? "ok" : "broken",
        reason: jetRow?.status === "ok" ? undefined : (jetRow?.reason ?? "Jet AOT row is unavailable"),
        command: { build: jetRow?.build?.command ?? null, run: jetRow?.command ?? null },
        verification: jetRow?.verification ?? null,
        metrics: { ...(jetRow?.metrics ?? {}) },
      };
      continue;
    }
    if (!jetSourceAvailable) {
      tiers[tier] = unavailableTier(policy.required, "Jet source was not staged");
      continue;
    }
    if (entry.mode === "service") {
      const commandFor = (port) => tier === "run"
        ? [jetBin, "run", "main.jet", "--", String(port)]
        : [jetBin, "dev", "--watch=off", "main.jet", "--", String(port)];
      const service = await runService("jet", jetDir, null, entry, commandFor);
      tiers[tier] = {
        applicable: true,
        required: policy.required,
        status: service.failure ? "broken" : "ok",
        reason: service.failure,
        command: commandFor("<port>"),
        verification: { status: service.failure ? "failed" : "passed", kind: "service_probe_sequence", reason: service.failure },
        metrics: {
          runtime_wall_seconds: null,
          runtime_first_stdout_seconds: null,
          runtime_peak_rss_kb: service.rssKb ?? null,
          service_startup_seconds: service.startupSeconds ?? null,
          service_latency_ms_p50: service.latencyMs?.median ?? null,
          service_latency_ms_p99: service.latencyMs?.p99 ?? null,
          binary_bytes: null,
        },
      };
      continue;
    }
    const commands = jetTierCommands(entry, tier, jetBin);
    const command = commands.length === 1 ? commands[0] : commands;
    const reset = entry.mode === "batch-steps" ? (resets.jet ?? (async () => {})) : jetFixtureReset;
    if (jetFixtureReset) await jetFixtureReset();
    const tierVerification = commands.length > 1
      ? await verifySequence(jetDir, commands, expected, reset, { timeoutMs: TIER_TIMEOUT_MS })
      : await verify(jetDir, command, expected, { timeoutMs: TIER_TIMEOUT_MS });
    const tierRow = {
      applicable: true,
      required: policy.required,
      command,
      verification: { status: tierVerification ? "failed" : "passed", kind: "byte_exact_stdout", reason: tierVerification },
      metrics: {},
    };
    if (tierVerification) {
      tierRow.status = "broken";
      tierRow.reason = tierVerification;
    } else {
      tierRow.status = "ok";
      tierRow.runtime = commands.length > 1
        ? await measureSequenceRuns(jetDir, commands, selectedRuns ?? (entry.perf ? 7 : 3), reset ?? (async () => {}))
        : await measureRuns(jetDir, command, selectedRuns ?? (entry.perf ? 7 : 3), { reset: jetFixtureReset });
      if (tierRow.runtime.samples.some((sample) => sample.exit_code !== 0)) {
        tierRow.status = "broken";
        tierRow.reason = "measured run exited nonzero";
        tierRow.verification = { status: "failed", kind: "timed_run", reason: tierRow.reason };
      } else {
        tierRow.metrics = { ...runtimeMetrics(tierRow.runtime), binary_bytes: null };
      }
    }
    tiers[tier] = tierRow;
  }
  const requiredTiersReady = Object.entries(tierPolicy(entry.mode))
    .filter(([, policy]) => policy.required)
    .every(([tier]) => tiers[tier]?.status === "ok");
  const status = languages.every((language) => ["ok", "not_applicable"].includes(rows[language]?.status)) && requiredTiersReady ? "ok" : "broken";
  return finish({ entry, status, stage: entryStage, languages, rows, comparisons: comparisons(entry, languages, rows, tiers), jet_tiers: tiers });
}

async function loadEntries(entriesDir, selected) {
  const loaded = [];
  const skipped = [];
  if (!(await exists(entriesDir))) return { loaded, skipped };
  for (const item of (await fs.readdir(entriesDir, { withFileTypes: true })).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
    if (!item.isDirectory() || (selected && item.name !== selected)) continue;
    const dir = path.join(entriesDir, item.name);
    const file = path.join(dir, "entry.json");
    try {
      const entry = JSON.parse(await fs.readFile(file, "utf8"));
      const nameDeclared = Object.hasOwn(entry, "name");
      entry.name ??= item.name;
      if (!ENTRY_MODES.includes(entry.mode)) {
        console.warn(`WARN ${entry.name}: skipped mode ${entry.mode ?? "missing"}`);
        skipped.push({ name: entry.name, reason: `mode ${entry.mode ?? "missing"} is not batch` });
      } else loaded.push({ dir, entry, directoryName: item.name, nameDeclared });
    } catch (error) {
      console.warn(`WARN ${item.name}: skipped invalid entry.json: ${error.message}`);
      skipped.push({ name: item.name, reason: `invalid entry.json: ${error.message}` });
    }
  }
  return { loaded, skipped };
}

async function readLiveTowerCards() {
  if (process.env.JET_GAUNTLET_DISABLE_TOWER_IO === "1") {
    return { status: "unavailable", reason: "Tower I/O disabled by caller" };
  }
  const towerPath = path.join(repoDir, "plugins/tower/.tower/tower.json");
  try {
    const store = JSON.parse(await fs.readFile(towerPath, "utf8"));
    if (!Array.isArray(store.cards)) return { status: "unavailable", reason: "Tower store has no cards array" };
    const cards = new Map(store.cards.map((card) => [card.num, card]));
    return { status: "available", revision: store.rev ?? null, cards };
  } catch (error) {
    return { status: "unavailable", reason: `cannot read Tower store: ${error.message}` };
  }
}

function liveLossOwner(number, tower) {
  if (!Number.isInteger(number)) return { status: "missing", reason: "no owner card is declared" };
  if (tower.status !== "available") return { status: "unavailable", card: number, reason: tower.reason };
  const card = tower.cards.get(number);
  if (!card) return { status: "stale", card: number, reason: "declared owner card is absent" };
  if (["done", "cancelled", "frozen"].includes(card.phase)) {
    return { status: "stale", card: number, title: card.title, phase: card.phase, reason: "declared owner card is terminal" };
  }
  return { status: "live", card: number, title: card.title, phase: card.phase, assignee: card.assignee ?? null };
}

function cellVerdict(verdicts) {
  if (!verdicts.length || verdicts.some((verdict) => verdict === null)) return "unmeasured";
  if (verdicts.includes("loss")) return "loss";
  if (verdicts.includes("parity")) return "parity";
  return "win";
}

function buildScoreboard(matrix, results, manifest, tower) {
  const entriesByCell = new Map();
  for (const result of results) {
    for (const cell of result.entry.cells ?? []) {
      const values = entriesByCell.get(cell) ?? [];
      values.push(result);
      entriesByCell.set(cell, values);
    }
  }
  const declaredOwners = manifest?.loss_owners ?? {};
  const cells = (matrix.cells ?? []).map((cell) => {
    const candidates = entriesByCell.get(cell.id) ?? [];
    const records = candidates.map((result) => {
      const metric = primaryMetric(result.entry.mode);
      const jetTierPolicy = tierPolicy(result.entry.mode);
      const requiredTiers = Object.entries(jetTierPolicy)
        .filter(([, policy]) => policy.required)
        .map(([tier]) => tier);
      const peers = (result.entry.languages ?? [])
        .filter((language) => language !== "jet")
        .map((language) => {
          const comparison = result.comparisons?.[language];
          const item = comparison?.metrics?.[metric];
          const tierVerdicts = Object.fromEntries(requiredTiers.map((tier) => [tier, comparison?.verdicts?.[tier] ?? null]));
          return {
            language,
            applicable: comparison?.applicable !== false,
            status: comparison?.status ?? result.rows?.[language]?.status ?? "unavailable",
            jet: item?.jet ?? null,
            peer: item?.peer ?? null,
            ratio: item?.ratio ?? null,
            verdict: item?.verdict ?? null,
            tier_verdicts: tierVerdicts,
            all_required_tiers_verdict: cellVerdict(Object.values(tierVerdicts)),
          };
        });
      const jetTierStatuses = Object.fromEntries(Object.keys(jetTierPolicy).map((tier) => [tier, result.jet_tiers?.[tier]?.status ?? "unavailable"]));
      const tiersReady = requiredTiers.every((tier) => jetTierStatuses[tier] === "ok");
      const applicablePeers = peers.filter((peer) => peer.applicable !== false);
      const verdict = result.rows?.jet?.status === "ok" && tiersReady
        ? (applicablePeers.length > 0 ? cellVerdict(applicablePeers.map((peer) => peer.all_required_tiers_verdict)) : "unmeasured")
        : "unmeasured";
      const owner = verdict === "loss" ? liveLossOwner(declaredOwners[result.entry.name], tower) : null;
      return {
        entry: result.entry.name,
        mode: result.entry.mode,
        status: result.status,
        primary_metric: metric,
        jet: result.rows?.jet?.metrics?.[metric] ?? null,
        jet_tiers: jetTierStatuses,
        tiers_ready: tiersReady,
        peers,
        verdict,
        loss_owner: owner,
      };
    });
    const verdict = records.length === 1 ? records[0].verdict : "unmeasured";
    return {
      id: cell.id,
      domain: cell.domain,
      kind: cell.kind,
      entries: records,
      verdict,
      loss_owners: records.filter((record) => record.verdict === "loss").map((record) => ({ entry: record.entry, ...record.loss_owner })),
    };
  });
  const verdicts = cells.map((cell) => cell.verdict);
  const allowedUncovered = new Set(manifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS);
  return {
    contract: "gauntlet-scoreboard-v1",
    primary_metric_by_mode: MODE_PRIMARY_METRIC,
    verdict_policy: { win: "all declared peer ratios < 1", parity: "no loss and at least one ratio <= 1.05", loss: "any declared peer ratio > 1.05", unmeasured: "missing row, tier, metric, or byte verification" },
    cells,
    summary: {
      cells: cells.length,
      win: verdicts.filter((verdict) => verdict === "win").length,
      parity: verdicts.filter((verdict) => verdict === "parity").length,
      loss: verdicts.filter((verdict) => verdict === "loss").length,
      unmeasured: verdicts.filter((verdict) => verdict === "unmeasured").length,
      unmeasured_allowed: cells.filter((cell) => cell.verdict === "unmeasured" && allowedUncovered.has(cell.id)).length,
      unmeasured_required: cells.filter((cell) => cell.verdict === "unmeasured" && !allowedUncovered.has(cell.id)).length,
    },
    loss_owners: {
      declared: declaredOwners,
      tower: tower.status === "available" ? { status: tower.status, revision: tower.revision } : { status: tower.status, reason: tower.reason },
      unresolved: cells.flatMap((cell) => cell.loss_owners.filter((owner) => owner.status !== "live")),
    },
  };
}

const AXIS_OUTPUT_LIMIT = 4_000;

function axisRepoPath(relative) {
  if (typeof relative !== "string" || relative.length === 0 || path.isAbsolute(relative)) {
    throw new Error(`axis source must be a relative repository path: ${relative}`);
  }
  const resolved = path.resolve(repoDir, relative);
  const remainder = path.relative(repoDir, resolved);
  if (!remainder || remainder.startsWith("..") || path.isAbsolute(remainder)) {
    throw new Error(`axis source escapes repository: ${relative}`);
  }
  return resolved;
}

function axisStagePath(stageDir, relative) {
  if (typeof relative !== "string" || relative.length === 0 || path.isAbsolute(relative)) {
    throw new Error(`axis target must be a relative path: ${relative}`);
  }
  const resolved = path.resolve(stageDir, relative);
  const remainder = path.relative(stageDir, resolved);
  if (!remainder || remainder.startsWith("..") || path.isAbsolute(remainder)) {
    throw new Error(`axis target escapes runner stage: ${relative}`);
  }
  return resolved;
}

function axisCommand(command, replacements) {
  if (!Array.isArray(command) || command.length === 0 || command.some((part) => typeof part !== "string")) {
    throw new Error("axis command must be a non-empty string array");
  }
  return command.map((part) => {
    const expanded = Object.entries(replacements).reduce(
      (value, [needle, replacement]) => value.replaceAll(`{${needle}}`, String(replacement)),
      part,
    );
    return expanded === "jet" && replacements.jet_bin ? replacements.jet_bin : expanded;
  });
}

async function stageAxisFiles(stageDir, files) {
  if (!Array.isArray(files) || files.length === 0) throw new Error("axis runner declares no files");
  const targets = new Set();
  const staged = [];
  for (const file of files) {
    if (!file || typeof file.source !== "string" || typeof file.target !== "string") {
      throw new Error("axis runner file must declare source and target");
    }
    const source = axisRepoPath(file.source);
    const target = axisStagePath(stageDir, file.target);
    const targetKey = path.relative(stageDir, target).replaceAll(path.sep, "/");
    if (targets.has(targetKey)) throw new Error(`axis runner declares duplicate target: ${targetKey}`);
    targets.add(targetKey);
    const stat = await fs.stat(source);
    if (!stat.isFile()) throw new Error(`axis source is not a file: ${file.source}`);
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.copyFile(source, target);
    staged.push({
      source: path.relative(repoDir, source).replaceAll(path.sep, "/"),
      target: targetKey,
      bytes: stat.size,
      sha256: await fileSha256(source),
    });
  }
  return staged;
}

async function probeAxisTool(cwd, tool, jetBin) {
  if (tool === "jet") {
    const resolved = path.resolve(jetBin);
    try {
      const stat = await fs.stat(resolved);
      if (!stat.isFile()) return { tool, status: "probe_failed", reason: `Jet binary is not a file: ${resolved}` };
      return { tool, status: "available", resolved, sha256: await fileSha256(resolved) };
    } catch (error) {
      if (error.code === "ENOENT") return { tool, status: "unavailable", reason: `Jet binary is absent: ${resolved}` };
      return { tool, status: "probe_failed", reason: `could not inspect Jet binary ${resolved}: ${error.message}` };
    }
  }
  const result = await runProcess(cwd, ["sh", "-c", `command -v ${shellQuote(tool)}`], { timeoutMs: 10_000 });
  const output = result.stdout.toString("utf8").trim();
  if (result.code === 0 && output) {
    const resolved = output.split(/\r?\n/, 1)[0];
    const versionFlag = tool === "entr" ? "-V" : "--version";
    const versionResult = await runProcess(cwd, [tool, versionFlag], { timeoutMs: 10_000 });
    const versionOutput = versionResult.stdout.toString("utf8").trim() || versionResult.stderr.toString("utf8").trim();
    return {
      tool,
      status: "available",
      resolved,
      version: versionOutput.split(/\r?\n/, 1)[0].slice(0, 300),
      version_exit_code: versionResult.code,
    };
  }
  if (result.code === 1 && !output && !result.stderr.toString("utf8").trim()) {
    return { tool, status: "unavailable", reason: `${tool} is absent from the declared tool environment` };
  }
  return {
    tool,
    status: "probe_failed",
    reason: `could not determine whether ${tool} is available (exit ${result.code})`,
    exit_code: result.code,
    stderr: result.stderr.toString("utf8").trim().slice(0, AXIS_OUTPUT_LIMIT),
  };
}

async function probeAxisTools(cwd, tools, jetBin) {
  if (!Array.isArray(tools) || tools.length === 0) throw new Error("axis runner declares no tools");
  const probes = [];
  for (const tool of tools) probes.push(await probeAxisTool(cwd, tool, jetBin));
  return probes;
}

async function freeTcpPort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  if (!Number.isInteger(port) || port < 1) throw new Error("could not allocate a local TCP port");
  return port;
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function childFailure(child) {
  if (child?.spawnError) return `process spawn failed: ${child.spawnError.message}`;
  if (child && child.exitCode !== null) return `process exited before readiness (exit ${child.exitCode ?? child.signalCode ?? "unknown"})`;
  return null;
}

async function waitForAxisReady(child, port, readiness, timeoutMs, pollIntervalMs, previousValue = null) {
  const started = performance.now();
  const pathName = readiness.path;
  const expectedStatus = readiness.status ?? 200;
  let lastReason = "readiness endpoint did not return a numeric counter";
  while (performance.now() - started < timeoutMs) {
    const failure = childFailure(child);
    if (failure) throw new Error(failure);
    const remaining = Math.max(1, timeoutMs - (performance.now() - started));
    const result = await httpProbe(port, { path: pathName }, Math.min(1_000, remaining));
    if (result.ok && result.status === expectedStatus) {
      const value = Number(result.body.trim());
      if (Number.isFinite(value) && (previousValue === null || value > previousValue)) {
        return { value, body: result.body, status: result.status, waited_ms: performance.now() - started };
      }
      lastReason = previousValue === null
        ? `readiness body was not numeric: ${JSON.stringify(result.body)}`
        : `readiness counter ${JSON.stringify(result.body)} did not exceed ${previousValue}`;
    } else if (!result.ok) {
      lastReason = result.error;
    } else {
      lastReason = `readiness status ${result.status}, expected ${expectedStatus}`;
    }
    await sleep(Math.min(pollIntervalMs, Math.max(1, timeoutMs - (performance.now() - started))));
  }
  throw new Error(`${pathName} was not ready within ${timeoutMs}ms: ${lastReason}`);
}

async function normalizeAxisMarker(file, from, to) {
  const text = await fs.readFile(file, "utf8");
  const fromCount = text.split(from).length - 1;
  const toCount = text.split(to).length - 1;
  if (fromCount === 1 && toCount === 0) return text;
  if (fromCount === 0 && toCount === 1) {
    const baseline = text.replace(to, from);
    await fs.writeFile(file, baseline);
    return baseline;
  }
  throw new Error(`axis edit markers are not unique in ${path.basename(file)} (from=${fromCount}, to=${toCount})`);
}

async function applyAxisEdit(file, from, to) {
  const before = await fs.readFile(file, "utf8");
  const count = before.split(from).length - 1;
  if (count !== 1 || before.split(to).length - 1 !== 0) {
    throw new Error(`axis edit did not find exactly one baseline marker in ${path.basename(file)}`);
  }
  const after = before.replace(from, to);
  await fs.writeFile(file, after);
  return {
    file: path.basename(file),
    from,
    to,
    before_sha256: sha256(before),
    after_sha256: sha256(after),
  };
}

async function restoreAxisEdit(file, from, to) {
  const text = await fs.readFile(file, "utf8").catch(() => null);
  if (text === null || text === undefined) return;
  if (text.split(from).length - 1 === 1 && text.split(to).length - 1 === 0) return;
  if (text.split(to).length - 1 === 1 && text.split(from).length - 1 === 0) {
    await fs.writeFile(file, text.replace(to, from));
    return;
  }
  throw new Error(`axis edit cannot restore ${path.basename(file)} to its baseline marker`);
}

async function runLiveReloadSample({ runner, stageDir, editPath, phase, index, jetBin, budget }) {
  const port = await freeTcpPort();
  const command = axisCommand(runner.command, { port, jet_bin: jetBin });
  const sample = {
    phase,
    index,
    status: "failed",
    fresh_process: true,
    command,
    port,
  };
  let child = null;
  try {
    await normalizeAxisMarker(editPath, budget.edit_from, budget.edit_to);
    child = startProcess(stageDir, command);
    sample.pid = child.pid ?? null;
    const initial = await waitForAxisReady(child, port, runner.readiness, budget.startup_timeout_ms, budget.poll_interval_ms);
    sample.readiness = { path: runner.readiness.path, initial: initial.value, status: initial.status, startup_wait_ms: initial.waited_ms };
    let previous = initial.value;
    if (phase === "warm") {
      await applyAxisEdit(editPath, budget.edit_from, budget.edit_to);
      const warmedAfter = await waitForAxisReady(child, port, runner.readiness, budget.reload_timeout_ms, budget.poll_interval_ms, previous);
      previous = warmedAfter.value;
      await applyAxisEdit(editPath, budget.edit_to, budget.edit_from);
      const warmedBefore = await waitForAxisReady(child, port, runner.readiness, budget.reload_timeout_ms, budget.poll_interval_ms, previous);
      previous = warmedBefore.value;
    }
    await normalizeAxisMarker(editPath, budget.edit_from, budget.edit_to);
    const started = performance.now();
    const edit = await applyAxisEdit(editPath, budget.edit_from, budget.edit_to);
    sample.edit = edit;
    const ready = await waitForAxisReady(child, port, runner.readiness, budget.reload_timeout_ms, budget.poll_interval_ms, previous);
    sample.status = "complete";
    sample.readiness.after = ready.value;
    sample.reload_latency_ms = performance.now() - started;
    sample.readiness.counter_delta = ready.value - previous;
  } catch (error) {
    sample.reason = error.message;
    if (child?.spawnError) sample.spawn_error = child.spawnError.message;
    if (child?.stderrText) sample.stderr = child.stderrText();
  } finally {
    if (child) {
      stopProcess(child);
      sample.process_exit = await waitForExit(child, 5_000);
      if (sample.process_exit.signal === "TIMEOUT") {
        try { process.kill(-child.pid, "SIGKILL"); } catch {}
        try { child.kill("SIGKILL"); } catch {}
        sample.process_exit = await waitForExit(child, 1_000);
        sample.cleanup_forced = true;
      }
      if (!sample.stderr && child.stderrText) sample.stderr = child.stderrText();
    }
    try {
      await restoreAxisEdit(editPath, budget.edit_from, budget.edit_to);
    } catch (error) {
      sample.restore_error = error.message;
      if (sample.status === "complete") {
        sample.status = "failed";
        sample.reason = error.message;
      }
    }
  }
  return sample;
}

function liveReloadSummary(samples) {
  const phases = {};
  for (const phase of ["cold", "warm"]) {
    const phaseSamples = samples.filter((sample) => sample.phase === phase);
    const successful = phaseSamples.filter((sample) => sample.status === "complete");
    phases[phase] = {
      sample_count: phaseSamples.length,
      successful_samples: successful.length,
      median_reload_latency_ms: median(successful.map((sample) => sample.reload_latency_ms)),
      samples: phaseSamples,
    };
  }
  return phases;
}

async function runLiveReloadRunner(axis, runner, axisDir, jetBin) {
  const runnerDir = path.join(axisDir, runner.id.replaceAll(/[^A-Za-z0-9_.-]/g, "_"));
  await fs.mkdir(runnerDir, { recursive: true });
  const files = await stageAxisFiles(runnerDir, runner.files);
  const probes = await probeAxisTools(runnerDir, runner.tools, jetBin);
  const result = {
    id: runner.id,
    workload: axis.workload,
    tools: probes,
    source_files: files,
    edit_file: runner.edit_file,
    readiness: runner.readiness,
    declared_command: runner.command,
    status: "unmeasured",
    measurements: [],
  };
  const unavailable = probes.find((probe) => probe.status === "unavailable");
  const probeFailure = probes.find((probe) => probe.status === "probe_failed");
  if (unavailable) {
    result.status = "unavailable";
    result.reason = unavailable.reason;
    return result;
  }
  if (probeFailure) {
    result.status = "failed";
    result.reason = probeFailure.reason;
    return result;
  }
  const editPath = axisStagePath(runnerDir, runner.edit_file);
  const budget = { ...axis.budget, edit_from: axis.edit.from, edit_to: axis.edit.to };
  await normalizeAxisMarker(editPath, budget.edit_from, budget.edit_to);
  for (const phase of ["cold", "warm"]) {
    for (let index = 1; index <= budget.sample_count; index += 1) {
      result.measurements.push(await runLiveReloadSample({ runner, stageDir: runnerDir, editPath, phase, index, jetBin, budget }));
    }
  }
  result.summary = liveReloadSummary(result.measurements);
  const expected = budget.sample_count * 2;
  const successful = result.measurements.filter((sample) => sample.status === "complete").length;
  result.status = successful === expected ? "complete" : "failed";
  if (result.status !== "complete") result.reason = `${expected - successful}/${expected} reload samples failed`;
  return result;
}


function xorshift32(value) {
  let state = value >>> 0;
  state ^= state << 13;
  state ^= state >>> 17;
  state ^= state << 5;
  return state >>> 0;
}

function generateFuzzCorpus(spec) {
  const seed = spec.seed >>> 0;
  const caseCount = spec.case_count;
  const bytesPerCase = spec.bytes_per_case;
  if (!Number.isInteger(seed) || !Number.isInteger(caseCount) || caseCount < 1 ||
    !Number.isInteger(bytesPerCase) || bytesPerCase < 1) {
    throw new Error("fuzz corpus must declare a positive integer shape and seed");
  }
  const data = Buffer.alloc(caseCount * bytesPerCase);
  let state = seed;
  for (let index = 0; index < data.length; index += 1) {
    state = xorshift32(state);
    data[index] = state & 0xff;
  }
  let checksum = 0;
  for (const byte of data) checksum = (checksum + byte) >>> 0;
  return {
    seed,
    case_count: caseCount,
    bytes_per_case: bytesPerCase,
    bytes: data.length,
    sha256: sha256(data),
    checksum,
    data,
  };
}

function formatFuzzOracle(template, corpus) {
  if (typeof template !== "string") throw new Error("fuzz oracle output must be a string");
  return template
    .replaceAll("{byte_count}", String(corpus.bytes))
    .replaceAll("{u32_sum}", String(corpus.checksum));
}

function boundedCommand(command, budget) {
  if (!Number.isInteger(budget.wall_timeout_ms) || budget.wall_timeout_ms < 1 ||
    !Number.isFinite(budget.cpu_seconds) || budget.cpu_seconds <= 0 ||
    !Number.isInteger(budget.memory_mb) || budget.memory_mb < 1) {
    throw new Error("invalid memory-safety resource budget");
  }
  const cpuSeconds = Math.max(1, Math.ceil(budget.cpu_seconds));
  return [
    "sh",
    "-c",
    `ulimit -t ${cpuSeconds} || exit 125; exec ${command.map(shellQuote).join(" ")}`,
  ];
}

function processEvidence(result) {
  return {
    code: result.code,
    signal: result.signal ?? null,
    timed_out: result.timedOut,
    resource_exceeded: result.resourceExceeded ?? null,
    stdout: result.stdout.toString("utf8").slice(0, AXIS_OUTPUT_LIMIT),
    stderr: result.stderr.toString("utf8").slice(0, AXIS_OUTPUT_LIMIT),
  };
}

function normalizeFindingText(text) {
  return text
    .replaceAll(repoDir, "<repo>")
    .replace(/0x[0-9a-f]+/gi, "0xADDRESS")
    .replace(/:\d+(?::\d+)?/g, ":LINE")
    .replace(/\b0x[0-9a-f]+\b/gi, "0xADDRESS")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 500);
}

function detectMemoryFindings(output, evidence) {
  const text = output.replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, "");
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const findings = new Map();
  for (const pattern of evidence.patterns ?? []) {
    const matches = lines.filter((line) => line.includes(pattern));
    for (const line of sortedUnique(matches)) {
      const finding = {
        kind: evidence.kind,
        pattern,
        excerpts: [line],
        signature: normalizeFindingText(line),
      };
      findings.set(`${finding.kind}\0${finding.signature}`, finding);
    }
  }
  return [...findings.values()];
}

function recordFinding(receipts, axisId, runner, finding) {
  const dedupeKey = sha256(`${axisId}\0${finding.kind}\0${finding.signature}`);
  let receipt = receipts.get(dedupeKey);
  if (!receipt) {
    receipt = {
      id: dedupeKey.slice(0, 16),
      dedupe_key: dedupeKey,
      axis: axisId,
      kind: finding.kind,
      signature: finding.signature,
      rails: [],
      occurrences: [],
      tower_tracking: { status: "pending", card: null },
    };
    receipts.set(dedupeKey, receipt);
  }
  if (!receipt.rails.includes(runner.id)) receipt.rails.push(runner.id);
  receipt.occurrences.push({
    runner: runner.id,
    language: runner.language,
    pattern: finding.pattern,
    excerpts: finding.excerpts,
  });
  receipt.rails.sort((left, right) => left.localeCompare(right));
  return receipt;
}

async function runMemoryCommand(cwd, command, budget) {
  const wrapped = boundedCommand(command, budget);
  const result = await runProcess(cwd, wrapped, { timeoutMs: budget.wall_timeout_ms, resourceBudget: budget });
  return { command: wrapped, process: processEvidence(result), raw: result };
}

async function runMemorySafetyRunner(axis, runner, axisDir, corpus, expectedOutput, jetBin) {
  const runnerDir = path.join(axisDir, runner.id.replaceAll(/[^A-Za-z0-9_.-]/g, "_"));
  await fs.mkdir(runnerDir, { recursive: true });
  const files = await stageAxisFiles(runnerDir, runner.files);
  const inputPath = axisStagePath(runnerDir, axis.corpus.path);
  await fs.mkdir(path.dirname(inputPath), { recursive: true });
  await fs.writeFile(inputPath, corpus.data);
  const inputSha = await fileSha256(inputPath);
  const probes = await probeAxisTools(runnerDir, runner.tools, jetBin);
  const result = {
    id: runner.id,
    language: runner.language,
    tools: probes,
    source_files: files,
    corpus: {
      path: axis.corpus.path,
      bytes: corpus.bytes,
      generated_sha256: corpus.sha256,
      copied_sha256: inputSha,
      matches_generator: inputSha === corpus.sha256,
    },
    resource_budget: axis.budget,
    resource_enforcement: {
      cpu: "RLIMIT_CPU via ulimit -t",
      memory: "process-tree VmRSS monitor",
      wall: "harness process-group timeout",
    },
    evidence: runner.evidence,
    declared_compile: runner.compile ?? null,
    declared_run: runner.run,
    status: "unmeasured",
  };
  const unavailable = probes.find((probe) => probe.status === "unavailable");
  const probeFailure = probes.find((probe) => probe.status === "probe_failed");
  if (unavailable) {
    result.status = "unavailable";
    result.reason = unavailable.reason;
    return result;
  }
  if (probeFailure) {
    result.status = "failed";
    result.reason = probeFailure.reason;
    return result;
  }
  try {
    if (runner.compile) {
      const compile = await runMemoryCommand(runnerDir, axisCommand(runner.compile, { jet_bin: jetBin }), axis.budget);
      result.compile = compile;
      delete result.compile.raw;
      if (compile.process.code !== 0) {
        result.status = "failed";
        result.reason = compile.process.resource_exceeded ?? `compile exited ${compile.process.code}`;
        return result;
      }
    }
    const run = await runMemoryCommand(runnerDir, axisCommand(runner.run, { jet_bin: jetBin }), axis.budget);
    result.run = run;
    delete result.run.raw;
    const combinedOutput = `${run.process.stdout}\n${run.process.stderr}`;
    const findings = detectMemoryFindings(combinedOutput, runner.evidence);
    result.output = {
      expected: expectedOutput,
      actual_stdout: run.process.stdout,
      exact_stdout: run.process.stdout === expectedOutput,
    };
    result.findings = findings;
    result.finding = findings[0] ?? null;
    result.input_unchanged = await fileSha256(inputPath) === inputSha;
    if (findings.length) {
      result.status = "finding";
      return result;
    }
    if (run.process.resource_exceeded) {
      result.status = "failed";
      result.reason = run.process.resource_exceeded;
    } else if (run.process.timed_out) {
      result.status = "failed";
      result.reason = `run exceeded ${axis.budget.wall_timeout_ms}ms wall budget`;
    } else if (run.process.code !== 0) {
      result.status = "failed";
      result.reason = `run exited ${run.process.code}`;
    } else if (!result.output.exact_stdout) {
      result.status = "failed";
      result.reason = "run stdout did not match the declared oracle";
    } else if (!result.input_unchanged) {
      result.status = "failed";
      result.reason = "runner modified the shared fuzz input";
    } else {
      result.status = "complete";
    }
  } catch (error) {
    result.status = "failed";
    result.reason = `runner execution failed: ${error.message}`;
  }
  return result;
}


function unmeasuredAxis(id, axis, reason) {
  return {
    id,
    required: axis?.status === "required",
    status: "unmeasured",
    reason,
    schema: axis?.schema ?? null,
    contract: axis ?? null,
    measurements: [],
    publication: { status: "blocked", blockers: [reason] },
  };
}

async function runAxes(manifest, runDir, jetBin, fullScope, runId) {
  const contracts = manifest?.axes ?? {};
  const axes = {};
  for (const [id, axis] of Object.entries(contracts)) {
    if (!fullScope) {
      axes[id] = unmeasuredAxis(id, axis, "axis measurements require the full corpus scope");
      continue;
    }
    try {
      if (id === "live_reload") axes[id] = await runLiveReloadAxisAdapter(axis, runDir, jetBin);
      else if (id === "memory_safety_fuzz") axes[id] = await runMemorySafetyFuzzAxisAdapter(axis, {
        runDir,
        jetBin,
        runId,
        targetCommit: process.env.GITHUB_SHA ?? process.env.SOURCE_COMMIT,
        repoDir,
        outputLimit: AXIS_OUTPUT_LIMIT,
        stageAxisFiles,
        axisStagePath,
        probeAxisTools,
        axisCommand,
        runMemoryCommand,
        fileSha256,
      });
      else axes[id] = unmeasuredAxis(id, axis, "no runner is implemented for this required axis");
    } catch (error) {
      axes[id] = {
        id,
        required: axis.status === "required",
        status: "failed",
        reason: `axis harness error: ${error.message}`,
        contract: axis,
        measurements: [],
        publication: { status: "blocked", blockers: [error.message] },
      };
    }
  }
  return axes;
}

function publicationState({ fullScope, loaded, skipped, matrix, manifest, sourceMeasurements, results, scoreboard, axes, validationIssues }) {
  const blockers = [...new Set(validationIssues)];
  if (!fullScope) blockers.push("run scope is partial; full matrix publication requires no --entry");
  if (skipped.length) blockers.push(`skipped entries: ${skipped.map((item) => item.name).join(", ")}`);
  if (manifest && sourceMeasurements && !sourceMeasurements.coverage.denominator_pass) blockers.push("source measurement denominator is incomplete");
  if (manifest && sourceMeasurements && !sourceMeasurements.aggregate.loc_pass) blockers.push("source LOC contract failed");
  if (manifest && sourceMeasurements && !sourceMeasurements.aggregate.token_pass) blockers.push("source token contract failed");
  const allowed = new Set(manifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS);
  const covered = new Set(results.flatMap((result) => result.entry.cells ?? []));
  const unexpectedUncovered = (matrix.cells ?? []).map((cell) => cell.id).filter((id) => !covered.has(id) && !allowed.has(id));
  if (unexpectedUncovered.length) blockers.push(`unexpected uncovered matrix cells: ${unexpectedUncovered.join(", ")}`);
  const expectedEntryCount = manifest?.corpus?.entry_count ?? loaded.length;
  if (fullScope && loaded.length !== expectedEntryCount) blockers.push(`entry denominator is ${loaded.length}/${expectedEntryCount}`);
  if (scoreboard.summary.unmeasured_required > 0) blockers.push(`${scoreboard.summary.unmeasured_required} required matrix cells are unmeasured`);
  for (const [id, axis] of Object.entries(axes)) {
    if (axis.required && axis.status !== "complete") blockers.push(`${id} axis is ${axis.status}`);
    if (axis.required && axis.publication?.status !== "ready" && !(axis.publication?.blockers?.length)) {
      blockers.push(`${id} axis publication is ${axis.publication?.status ?? "unreported"}`);
    }
    for (const blocker of axis.publication?.blockers ?? []) blockers.push(`${id}: ${blocker}`);
  }
  for (const owner of scoreboard.loss_owners.unresolved) blockers.push(`${owner.entry}: loss owner ${owner.card ?? "is not declared"} is not live`);
  const uniqueBlockers = [...new Set(blockers)];
  return {
    scope: fullScope ? "full_matrix" : "partial_entry",
    status: uniqueBlockers.length ? "incomplete" : "complete",
    complete: uniqueBlockers.length === 0,
    blockers: uniqueBlockers,
    allowed_uncovered_cells: [...allowed].sort((left, right) => left.localeCompare(right)),
  };
}

async function copyJetBinary(options, runDir) {
  if (options.jetBin) return path.resolve(process.cwd(), options.jetBin);
  const source = path.join(repoDir, "target/debug/jet");
  if (!(await exists(source))) throw new Error(`missing default Jet binary: ${source}`);
  const destination = path.join(runDir, "jet-bin", "jet");
  await fs.mkdir(path.dirname(destination), { recursive: true });
  await fs.copyFile(source, destination);
  await fs.chmod(destination, 0o755);
  return destination;
}

async function devAvailable(jetBin, runDir) {
  const result = await runProcess(runDir, [jetBin, "dev", "--help"]);
  return result.code === 0;
}

async function toolchainFingerprint(runDir, jetBin) {
  const commands = {
    python: ["python3", "--version"],
    rust: ["rustc", "--version"],
    c: ["gcc", "--version"],
    zig: ["zig", "version"],
    go: ["go", "version"],
    js: ["node", "--version"],
    node: ["node", "--version"],
  };
  const versions = {};
  for (const [language, command] of Object.entries(commands)) {
    const result = await runProcess(runDir, command, { timeoutMs: 10_000 });
    const output = result.stdout.toString("utf8").trim() || result.stderr.toString("utf8").trim();
    versions[language] = {
      command,
      status: result.code === 0 ? "ok" : "unavailable",
      version: output.split(/\r?\n/, 1)[0].slice(0, 300),
      exit_code: result.code,
    };
  }
  versions.jet = { command: [jetBin], status: "identified_by_binary_sha256" };
  return versions;
}

function dateStamp() {
  return new Date().toISOString().slice(0, 10);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const matrixPath = path.join(repoDir, "gauntlet/matrix.json");
  const matrixText = await fs.readFile(matrixPath, "utf8");
  const matrix = JSON.parse(matrixText);
  const entriesDir = path.resolve(process.cwd(), options.entriesDir ?? path.join(repoDir, "gauntlet/entries"));
  const defaultEntriesDir = path.resolve(path.join(repoDir, "gauntlet/entries"));
  const fullScope = entriesDir === defaultEntriesDir && options.entry === null;
  const manifestPath = path.join(repoDir, "gauntlet/measurement-manifest.json");
  const sourceManifest = entriesDir === defaultEntriesDir
    ? JSON.parse(await fs.readFile(manifestPath, "utf8"))
    : null;
  const sourceMeasurements = sourceManifest ? await measureSourceManifest(entriesDir, sourceManifest, matrix) : null;
  const runId = `${dateStamp().replaceAll("-", "")}-${process.pid}-${Date.now().toString(36)}`;
  const runDir = path.join(process.env.HOME ?? ".", ".cache/jet-gauntlet/work", runId);
  await fs.mkdir(runDir, { recursive: true });
  const jetBin = await copyJetBinary(options, runDir);
  const toolchains = await toolchainFingerprint(runDir, jetBin);
  const dev = await devAvailable(jetBin, runDir);
  if (!dev) console.warn("WARN jet dev unavailable; skipping Jet dev tier");
  const { loaded, skipped } = await loadEntries(entriesDir, options.entry);
  const validationIssues = await validateCorpus(entriesDir, loaded, skipped, matrix, sourceManifest, fullScope);
  for (const issue of validationIssues) console.warn(`WARN corpus: ${issue}`);
  const results = [];
  for (const item of loaded) {
    console.error(`gauntlet: entry ${item.entry.name} [${(item.entry.languages ?? []).join(",")}] ...`);
    try {
      results.push(await stageEntry(item.dir, item.entry, runDir, jetBin, options.runs, dev));
    } catch (error) {
      console.error(`gauntlet: entry ${item.entry.name} CRASHED: ${error.message}`);
      results.push({
        entry: item.entry,
        status: "broken",
        reason: `harness error: ${error.message}`,
        languages: item.entry.languages ?? [],
        rows: {},
        comparisons: {},
        jet_tiers: emptyJetTiers(item.entry, dev, `harness error: ${error.message}`),
        provenance: {
          entry_json_sha256: await fileSha256(path.join(item.dir, "entry.json")),
          corpus_tree_sha256: await treeSha256(item.dir),
        },
      });
    }
    console.error(`gauntlet: entry ${item.entry.name} done`);
  }
  const covered = new Set(results.flatMap((result) => result.entry.cells ?? []));
  const uncovered = (matrix.cells ?? []).map((cell) => cell.id).filter((id) => !covered.has(id));
  const tower = await readLiveTowerCards();
  const scoreboard = buildScoreboard(matrix, results, sourceManifest, tower);
  const axes = await runAxes(sourceManifest, runDir, jetBin, fullScope, runId);
  const publication = publicationState({ fullScope, loaded, skipped, matrix, manifest: sourceManifest, sourceMeasurements, results, scoreboard, axes, validationIssues });
  const expectedEntryNames = sourceManifest?.corpus?.entry_names ?? [];
  const allowedUncovered = new Set(sourceManifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS);
  const unexpectedUncovered = uncovered.filter((cell) => !allowedUncovered.has(cell));
  const denominator = {
    entry_names_pass: fullScope && skipped.length === 0 && loaded.length === expectedEntryNames.length && equalStringArrays(loaded.map((item) => item.directoryName ?? path.basename(item.dir)), expectedEntryNames),
    matrix_coverage_pass: unexpectedUncovered.length === 0,
    source_pairs_pass: sourceMeasurements?.coverage?.denominator_pass ?? false,
  };
  denominator.pass = denominator.entry_names_pass && denominator.matrix_coverage_pass && denominator.source_pairs_pass;
  const report = {
    contract: "gauntlet-report-v1",
    generated: new Date().toISOString(),
    run_id: runId,
    options: { entry: options.entry, jet_bin: jetBin, runs: options.runs, scope: fullScope ? "full_matrix" : "partial_entry" },
    matrix_version: matrix.version,
    matrix_rails: matrix.rails,
    entries_dir: entriesDir,
    skipped,
    validation: { issues: validationIssues },
    coverage: {
      expected_entry_count: sourceManifest?.corpus?.entry_count ?? null,
      expected_entry_names: expectedEntryNames,
      observed_entry_count: loaded.length,
      observed_result_count: results.length,
      expected_matrix_cell_count: matrix.cells?.length ?? 0,
      declared_covered_cells: [...covered].sort((left, right) => left < right ? -1 : left > right ? 1 : 0),
      uncovered_cells: uncovered,
      allowed_uncovered_cells: sourceManifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS,
      denominator,
    },
    uncovered_cells: uncovered,
    source_measurements: sourceMeasurements,
    axes,
    scoreboard,
    publication,
    provenance: {
      matrix_sha256: sha256(matrixText),
      measurement_manifest_sha256: sourceManifest ? sha256(await fs.readFile(manifestPath)) : null,
      corpus_tree_sha256: await treeSha256(entriesDir),
      jet_binary_sha256: await fileSha256(jetBin),
      toolchains,
      host: { platform: process.platform, arch: process.arch, node: process.version },
    },
    reproducibility: {
      expected_output: "byte-exact UTF-8 stdout or declared service probe sequence",
      source_metric_token_definition: sourceManifest?.contract?.token_definition ?? null,
      tier_policy_by_mode: Object.fromEntries(Object.entries(TIER_POLICY).map(([mode, policy]) => [mode, Object.keys(policy)])),
      ratio_verdicts: { win: "<1", parity: "<=1.05", loss: ">1.05" },
      missing_metric_verdict: "unmeasured",
      run_count: options.runs ?? "entry perf policy (7 for perf, 3 otherwise)",
    },
    entries: results,
  };
  const resultDir = path.join(repoDir, "gauntlet/results");
  await fs.mkdir(resultDir, { recursive: true });
  const resultPath = path.join(resultDir, `${dateStamp()}.json`);
  await fs.writeFile(resultPath, `${JSON.stringify(report, null, 2)}\n`);

  console.log("entry\tlanguage\tstatus\truntime_s\tcold_build_s\tjet_verdicts\tmode_metrics");
  for (const result of results) {
    for (const language of result.languages ?? ["-"]) {
      const row = result.rows?.[language];
      const comparison = result.comparisons?.[language];
      const primaryVerdict = comparison?.metrics?.[comparison.primary_metric]?.verdict ?? null;
      const verdicts = primaryVerdict ?? "-";
      const metrics = result.entry.mode === "service"
        ? `startup=${row?.metrics?.startupSeconds ?? "-"},latency_ms=${row?.metrics?.latencyMs?.median ?? "-"}/${row?.metrics?.latencyMs?.p99 ?? "-"},rss_kb=${row?.metrics?.rssKb ?? "-"},clean_exit=${row?.metrics?.cleanExit ?? "-"}`
        : result.entry.mode === "web" || result.entry.mode === "web-app"
          ? `artifact_bytes=${row?.metrics?.artifactBytes ?? "-"},first_result_s=${row?.metrics?.firstResultSeconds ?? "-"}`
          : "-";
      console.log(`${result.entry.name}\t${language}\t${row?.status ?? result.status}\t${row?.metrics?.runtime_wall_seconds ?? "-"}\t${row?.metrics?.cold_build_seconds ?? "-"}\t${verdicts || "-"}\t${metrics}`);
    }
  }
  for (const item of skipped) console.log(`${item.name}\t-\tskipped\t-\t-\t-`);
  if (sourceMeasurements) {
    console.log("source\tjet_loc\tpython_loc\tloc_ratio\tjet_source_tokens\tpython_source_tokens\tsource_token_delta");
    for (const item of sourceMeasurements.entries) {
      console.log(`${item.name}\t${item.jet.loc}\t${item.python?.loc ?? "-"}\t${item.comparison?.loc_ratio ?? "-"}\t${item.jet.source_tokens}\t${item.python?.source_tokens ?? "-"}\t${item.comparison?.source_token_delta ?? "-"}`);
    }
    const total = sourceMeasurements.aggregate;
    console.log(`source-total\t${total.jet.loc}\t${total.python.loc}\t${total.loc_ratio}\t${total.jet.source_tokens}\t${total.python.source_tokens}\t${total.source_token_delta}`);
  }
  console.log(`results\t${resultPath}`);
  console.log(`uncovered\t${uncovered.length}`);
  console.log(`publication\t${publication.status}`);
  for (const [id, axis] of Object.entries(axes)) {
    console.log(`axis\t${id}\t${axis.status}`);
    for (const blocker of axis.publication?.blockers ?? []) console.log(`axis-blocker\t${id}\t${blocker}`);
  }
  if (!publication.complete) process.exitCode = 1;
}

main().catch((error) => {
  console.error(`harness: ${error.message}`);
  process.exitCode = 1;
});
