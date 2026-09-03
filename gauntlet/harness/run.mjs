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
import { projectStatus } from "./status.mjs";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../..");
const envRunner = path.join(repoDir, "scripts/agent/jet-env");
const timer = path.join(harnessDir, "timer.py");

const ENTRY_MODES = ["batch", "batch-steps", "service", "web", "web-app"];
const MATRIX_UNCOVERED_DEFAULTS = [];
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
const RATIO_VERDICTS = {
  rust: { win: "<1", parity: "<=1.05", loss: ">1.05" },
  non_rust: { win: "<1", parity: null, loss: ">=1" },
};
const METRIC_APPLICABILITY_POLICY = {
  default: "required",
  not_applicable: "explicit_structural_reason",
  missing: "unmeasured_and_publication_blocked",
};
const STRUCTURAL_NOT_APPLICABLE_BASES = new Set(["no_compile_phase"]);
const PEER_MEASUREMENT_POLICY = {
  ratio_tiers: ["aot", "run"],
  trace_only_tiers: [],
  sample_binding: "immutable_peer_row_reused_per_declared_jet_tier",
};
const LOSS_OWNER_CATEGORY_BY_METRIC = {
  runtime_wall_seconds: "runtime",
  runtime_first_stdout_seconds: "runtime",
  service_latency_ms_p50: "latency",
  service_latency_ms_p99: "latency",
  service_startup_seconds: "runtime",
  runtime_peak_rss_kb: "rss",
  peak_rss_bytes: "rss",
  cold_build_seconds: "build",
  warm_build_seconds: "build",
  binary_bytes: "binary",
  loc: "source",
  source_bytes: "source",
  tokens: "source",
  source_tokens: "source",
};
const AOT_ONLY_METRICS = new Set(["cold_build_seconds", "warm_build_seconds", "binary_bytes"]);


const SOURCE_METRICS = ["loc", "source_bytes", "tokens", "source_tokens"];

function sourceMetricValues(metrics) {
  return Object.fromEntries(SOURCE_METRICS.map((metric) => [metric, metrics?.[metric] ?? null]));
}

function metricComparableAtTier(metric, tier) {
  return tier === "aot" || !AOT_ONLY_METRICS.has(metric);
}
const VALID_RESULT_STATUSES = new Set(["ok", "not_applicable", "broken", "unavailable", "failed", "inconclusive"]);
const VALID_PASS_VERIFICATION_KINDS = new Set(["byte_exact_stdout", "service_probe_sequence"]);
const FORBIDDEN_SELECTION_KEYS = new Set([
  "selected",
  "selected_peer",
  "selected_metric",
  "selected_best",
  "selected_value",
  "best",
  "winner",
  "average",
  "mean",
  "avg",
  "aggregate",
  "aggregates",
  "selection",
  "chosen",
]);

function positiveMetricValue(metric, value) {
  return typeof value === "number" && Number.isFinite(value) && value > 0;
}

function selectionOrAggregateIssues(value, prefix, issues, seen = new Set()) {
  if (!value || typeof value !== "object" || seen.has(value)) return;
  seen.add(value);
  for (const [key, nested] of Object.entries(value)) {
    const normalizedKey = key.replaceAll("-", "_").toLowerCase();
    if (FORBIDDEN_SELECTION_KEYS.has(normalizedKey)) {
      issues.push(`${prefix}.${key}: selected or aggregate measurements are not allowed`);
    }
    selectionOrAggregateIssues(nested, `${prefix}.${key}`, issues, seen);
  }
}

const LANGUAGE_FILES = {
  jet: "run.jet",
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
  const options = { runs: null, entry: null, jetBin: null, entriesDir: null, axis: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--entry" || arg === "--jet-bin" || arg === "--runs" || arg === "--entries-dir" || arg === "--axis") {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      const value = argv[++i];
      if (arg === "--entry") options.entry = value;
      if (arg === "--jet-bin") options.jetBin = value;
      if (arg === "--entries-dir") options.entriesDir = value;
      if (arg === "--runs") options.runs = Number.parseInt(value, 10);
      if (arg === "--axis") options.axis = value;
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: node gauntlet/harness/run.mjs [--entry name] [--jet-bin path] [--runs n] [--entries-dir path] [--axis live_reload]");
      process.exit(0);
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (options.runs !== null && (!Number.isInteger(options.runs) || options.runs < 1)) {
    throw new Error("--runs must be a positive integer");
  }
  if (options.axis !== null && !["live_reload", "memory_safety_fuzz"].includes(options.axis)) {
    throw new Error("--axis must be live_reload or memory_safety_fuzz");
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

async function runProcess(cwd, args, {
  input = undefined,
  full = false,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  resourceBudget = null,
  env = undefined,
} = {}) {
  return new Promise((resolve) => {
    const child = spawn(envRunner, [...(full ? ["full"] : []), "sh", "-c", args.map(shellQuote).join(" ")], {
      cwd,
      env: env === undefined ? process.env : { ...process.env, ...env },
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

function startProcess(cwd, args, { full = false, env = undefined } = {}) {
  const child = spawn(envRunner, [...(full ? ["full"] : []), "sh", "-c", args.map(shellQuote).join(" ")], {
    cwd,
    env: env === undefined ? process.env : { ...process.env, ...env },
    stdio: ["ignore", "ignore", "pipe"],
    detached: true,
  });
  const stderr = [];
  child.spawnError = null;
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
      response.on("end", () => {
        const bodyText = Buffer.concat(chunks).toString("utf8");
        const latencyMs = performance.now() - started;
        const headers = { ...response.headers };
        resolve({
          ok: true,
          status: response.statusCode,
          headers,
          body: bodyText,
          latencyMs,
        });
      });
    });
    request.on("timeout", () => request.destroy(new Error("HTTP probe timeout")));
    request.on("error", (error) => resolve({ ok: false, error: error.message, latencyMs: performance.now() - started }));
    if (body !== undefined) request.write(body);
    request.end();
  });
}

function isHeaderMap(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value) &&
    Object.keys(value).length > 0 &&
    Object.entries(value).every(([name, expected]) =>
      typeof name === "string" && name.length > 0 && name === name.trim() && typeof expected === "string");
}

function normalizedHeaderValue(name, value) {
  const trimmed = value.trim();
  if (name.toLowerCase() !== "content-type") return trimmed;
  const parts = trimmed.split(";");
  const mediaType = parts.shift().trim().toLowerCase();
  const parameters = parts.map((part) => {
    const separator = part.indexOf("=");
    if (separator < 0) return part.trim().toLowerCase();
    const key = part.slice(0, separator).trim().toLowerCase();
    const parameterValue = part.slice(separator + 1).trim();
    return `${key}=${parameterValue}`;
  }).sort();
  return [mediaType, ...parameters].join(";");
}

function expectedHeadersMatch(expected, actual) {
  if (!isHeaderMap(expected) || !actual || typeof actual !== "object" || Array.isArray(actual)) return false;
  const actualHeaders = new Map(Object.entries(actual).map(([name, value]) => [name.toLowerCase(), value]));
  return Object.entries(expected).every(([name, expectedValue]) => {
    const actualValue = actualHeaders.get(name.toLowerCase());
    return typeof actualValue === "string" && normalizedHeaderValue(name, actualValue) === normalizedHeaderValue(name, expectedValue);
  });
}

function headerMismatch(probe, result) {
  if (!isHeaderMap(probe.expectHeaders)) return "missing expected response headers";
  const actual = result.headers && typeof result.headers === "object" ? result.headers : {};
  const actualHeaders = new Map(Object.entries(actual).map(([name, value]) => [name.toLowerCase(), value]));
  for (const [name, expected] of Object.entries(probe.expectHeaders)) {
    const value = actualHeaders.get(name.toLowerCase());
    if (value === undefined) return `header ${name} is missing, expected ${JSON.stringify(expected)}`;
    if (typeof value !== "string" || normalizedHeaderValue(name, value) !== normalizedHeaderValue(name, expected)) {
      return `header ${name} was ${JSON.stringify(value)}, expected ${JSON.stringify(expected)}`;
    }
  }
  return "response headers did not match expected values";
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
    (probe.expectBody === undefined || result.body === probe.expectBody) &&
    expectedHeadersMatch(probe.expectHeaders, result.headers);
}

function probeMismatch(probe, result, protocol) {
  if (!result.ok) return result.error;
  if (protocol === "line") return `body ${JSON.stringify(result.body)}, expected ${JSON.stringify(probe.expect)}`;
  if (probe.expectStatus !== undefined && result.status !== probe.expectStatus) return `status ${result.status}, expected ${probe.expectStatus}`;
  if (probe.expectBody !== undefined && result.body !== probe.expectBody) return `body ${JSON.stringify(result.body)}, expected ${JSON.stringify(probe.expectBody)}`;
  return headerMismatch(probe, result);
}

async function probeSequence(port, probes, protocol = "http") {
  for (let index = 0; index < probes.length; index += 1) {
    const probe = probes[index];
    const result = await serviceProbe(port, probe, protocol);
    if (!probeMatches(probe, result, protocol)) return { ok: false, index, result, reason: probeMismatch(probe, result, protocol) };
  }
  return { ok: true };
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
  const valid = Array.isArray(samples) && samples.length > 0 &&
    samples.every((sample) => sample && sample.exit_code === 0 && metrics.every((metric) => positiveMetricValue(metric, sample[metric])));
  const medians = Object.fromEntries(metrics.map((metric) => [
    metric,
    valid ? median(samples.map((sample) => sample[metric])) : null,
  ]));
  return { samples, valid, median: medians };
}

function validBuildSample(sample) {
  return sample?.exit_code === 0 &&
    positiveMetricValue("wall_seconds", sample.wall_seconds) &&
    positiveMetricValue("peak_rss_kb", sample.peak_rss_kb);
}

function mismatch(expected, actual) {
  const limit = Math.min(expected.length, actual.length);
  let index = 0;
  while (index < limit && expected[index] === actual[index]) index += 1;
  if (index === expected.length && index === actual.length) return null;
  const show = (buffer) => JSON.stringify(buffer.subarray(index, index + 80).toString("utf8"));
  return `byte ${index}: expected ${show(expected)}, got ${show(actual)} (length ${expected.length}/${actual.length})`;
}

function unavailableTierTrace(reason = "compiler-owned tier trace channel is unavailable; --trace-tiers shares workload stderr") {
  return {
    status: "failed",
    channel: "unavailable",
    rows: [],
    native_rows: 0,
    interp_rows: 0,
    whole_program_deopt: null,
    invocations: [],
    reason,
  };
}

function tracedCommand(command) {
  const traced = [...command];
  if (traced.includes("--trace-tiers")) return traced;
  const separator = traced.indexOf("--");
  if (separator < 0) traced.push("--trace-tiers");
  else traced.splice(separator, 0, "--trace-tiers");
  return traced;
}

function validTierRow(row) {
  return row && typeof row === "object" && !Array.isArray(row) &&
    typeof row.function === "string" && row.function.length > 0 &&
    (row.tier === "native" || row.tier === "interp") &&
    (row.reason === null || typeof row.reason === "string") &&
    typeof row.millis === "number" && Number.isFinite(row.millis) && row.millis >= 0;
}

function parseTierFacts(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return { error: "sidecar is not a JSON object" };
  if (!Array.isArray(value.rows) || value.rows.length === 0 || !value.rows.every(validTierRow)) {
    return { error: "sidecar rows are missing or malformed" };
  }
  const nativeRows = value.rows.filter((row) => row.tier === "native").length;
  const interpRows = value.rows.filter((row) => row.tier === "interp").length;
  if (!Number.isInteger(value.native_rows) || value.native_rows < 0 || value.native_rows !== nativeRows) {
    return { error: "sidecar native_rows does not match rows" };
  }
  if (!Number.isInteger(value.interp_rows) || value.interp_rows < 0 || value.interp_rows !== interpRows) {
    return { error: "sidecar interp_rows does not match rows" };
  }
  if (typeof value.whole_program_deopt !== "boolean") {
    return { error: "sidecar whole_program_deopt is not boolean" };
  }
  return {
    rows: value.rows,
    native_rows: nativeRows,
    interp_rows: interpRows,
    whole_program_deopt: value.whole_program_deopt,
  };
}

async function readTierFacts(sidecar) {
  let text;
  try {
    text = await fs.readFile(sidecar, "utf8");
  } catch (error) {
    return { error: `compiler-owned tier trace sidecar is absent: ${error.message}` };
  }
  let value;
  try {
    value = JSON.parse(text);
  } catch (error) {
    return { error: `compiler-owned tier trace sidecar is malformed: ${error.message}` };
  }
  return parseTierFacts(value);
}

function tierTraceResult(invocations, reason = null) {
  const rows = invocations.flatMap((invocation) => invocation.rows);
  const hasFacts = rows.length > 0;
  return {
    status: reason ? "failed" : "passed",
    channel: "compiler_owned_sidecar",
    rows,
    native_rows: invocations.reduce((total, invocation) => total + invocation.native_rows, 0),
    interp_rows: invocations.reduce((total, invocation) => total + invocation.interp_rows, 0),
    whole_program_deopt: hasFacts
      ? invocations.some((invocation) => invocation.whole_program_deopt === true)
      : null,
    invocations,
    ...(reason ? { reason } : {}),
  };
}

function emptyTierInvocation(command, exitCode) {
  return {
    command: [...command],
    exit_code: exitCode,
    rows: [],
    native_rows: 0,
    interp_rows: 0,
    whole_program_deopt: null,
  };
}

async function collectTierTrace(cwd, commands, expected, reset = null, {
  full = false,
  timeoutMs = DEFAULT_TIMEOUT_MS,
} = {}) {
  const sidecar = path.join(cwd, ".jet-tier-trace.json");
  try {
    await fs.rm(sidecar, { force: true });
  } catch (error) {
    return unavailableTierTrace(`could not remove stale tier trace sidecar: ${error.message}`);
  }
  if (!Array.isArray(commands) || commands.length === 0) {
    return unavailableTierTrace("compiler-owned tier trace has no commands");
  }
  const invocations = [];
  const output = [];
  let reason = null;
  if (reset) {
    try {
      await reset();
    } catch (error) {
      return unavailableTierTrace(`tier trace reset failed: ${error.message}`);
    }
  }
  try {
    for (const command of commands) {
      await fs.rm(sidecar, { force: true });
      const result = await runProcess(cwd, tracedCommand(command), {
        full,
        timeoutMs,
        env: { JET_TRACE_TIERS_PATH: sidecar },
      });
      const facts = await readTierFacts(sidecar);
      const invocation = facts.error
        ? emptyTierInvocation(command, result.code)
        : { command: [...command], exit_code: result.code, ...facts };
      invocations.push(invocation);
      output.push(result.stdout);
      if (facts.error) {
        reason = facts.error;
        break;
      }
      if (result.code !== 0) {
        reason = `trace invocation exited ${result.code}`;
        break;
      }
    }
    if (!reason) {
      const mismatchReason = mismatch(expected, Buffer.concat(output));
      if (mismatchReason) reason = `trace output mismatch: ${mismatchReason}`;
    }
    return tierTraceResult(invocations, reason);
  } catch (error) {
    return tierTraceResult(invocations, `tier trace collection failed: ${error.message}`);
  } finally {
    await fs.rm(sidecar, { force: true });
  }
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
  return Array.isArray(left) && Array.isArray(right) && left.length === right.length && left.every((value, index) => value === right[index]);
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
      "cold_build_seconds",
      "warm_build_seconds",
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

function metricApplicability(entry, language, metric) {
  const languageFacts = entry?.metric_applicability?.[language];
  const fact = languageFacts && typeof languageFacts === "object" && Object.hasOwn(languageFacts, metric)
    ? languageFacts[metric]
    : languageFacts && typeof languageFacts === "object" && Object.hasOwn(languageFacts, "*")
      ? languageFacts["*"]
      : entry?.non_applicable?.[language];
  if (fact === undefined) return { status: "required" };
  if (!fact || typeof fact !== "object" || Array.isArray(fact)) {
    return { status: "invalid", reason: "metric applicability must be an object" };
  }
  if (fact.status !== undefined && fact.status !== "not_applicable") {
    return { status: "invalid", reason: "metric applicability status must be not_applicable" };
  }
  return {
    status: "not_applicable",
    basis: fact.basis ?? null,
    reason: fact.reason ?? null,
    evidence: fact.evidence ?? null,
  };
}

function applicabilityFactIssues(entry, language, scope, fact) {
  const prefix = `${entry.name}/${language}${scope ? `/${scope}` : ""}`;
  const issues = [];
  if (baseLanguage(language) === "jet") issues.push(`${prefix}: Jet cannot be not_applicable`);
  if (!fact || typeof fact !== "object" || Array.isArray(fact)) {
    issues.push(`${prefix}: not_applicable fact must be an object`);
    return issues;
  }
  if (fact.status !== undefined && fact.status !== "not_applicable") {
    issues.push(`${prefix}: not_applicable fact status must be not_applicable`);
  }
  if (!STRUCTURAL_NOT_APPLICABLE_BASES.has(fact.basis)) {
    issues.push(`${prefix}: not_applicable fact requires a structural basis`);
  }
  for (const field of ["reason", "evidence"]) {
    if (typeof fact[field] !== "string" || fact[field].trim().length === 0) {
      issues.push(`${prefix}: not_applicable fact missing ${field}`);
    }
  }
  return issues;
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
  if (contract.token_metric !== "source_tokens") {
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
    new Set(corpus.allowed_uncovered_cells).size !== corpus.allowed_uncovered_cells.length || corpus.allowed_uncovered_cells.length !== 0) {
    throw new Error("measurement manifest entry denominator does not match its named corpus");
  }
  const reportContract = manifest.report_contract;
  const tierPolicyByMode = Object.fromEntries(Object.entries(TIER_POLICY).map(([mode, policy]) => [mode, Object.keys(policy)]));
  if (reportContract?.id !== "gauntlet-report-v1" || reportContract.scope !== "full_matrix" ||
    !equalStringArrays(reportContract.required_jet_tiers ?? [], ["aot", "run"]) ||
    !equalStringArrays(reportContract.optional_jet_tiers ?? [], ["dev"]) ||
    JSON.stringify(reportContract.tier_policy_by_mode) !== JSON.stringify(tierPolicyByMode) ||
    JSON.stringify(reportContract.primary_metric_by_mode) !== JSON.stringify(MODE_PRIMARY_METRIC) ||
    JSON.stringify(reportContract.ratio_verdicts) !== JSON.stringify(RATIO_VERDICTS) ||
    reportContract.metric_applicability?.default !== METRIC_APPLICABILITY_POLICY.default ||
    reportContract.metric_applicability?.not_applicable !== METRIC_APPLICABILITY_POLICY.not_applicable ||
    reportContract.metric_applicability?.missing !== METRIC_APPLICABILITY_POLICY.missing ||
    !equalStringArrays(reportContract.aot_only_metrics ?? [], [...AOT_ONLY_METRICS]) ||
    reportContract.missing_metric_verdict !== "unmeasured" || reportContract.output_verification !== "byte_exact_utf8_or_declared_probe_sequence" ||
    reportContract.loss_owner_required_for !== "any_comparable_metric_loss" ||
    JSON.stringify(reportContract.peer_measurement) !== JSON.stringify(PEER_MEASUREMENT_POLICY) ||
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
    if (JSON.stringify(matrix.metric_applicability) !== JSON.stringify(METRIC_APPLICABILITY_POLICY)) {
      throw new Error("matrix is missing the metric applicability policy");
    }
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

function validateHttpServiceProbes(entry, issues) {
  if (entry.mode !== "service") return;
  const service = entry.spec?.service ?? entry.service;
  if ((service?.protocol ?? "http") !== "http" || !Array.isArray(service?.probe)) return;
  for (const [index, probe] of service.probe.entries()) {
    const prefix = `${entry.name}/service/probe[${index}]`;
    if (!probe || typeof probe !== "object" || Array.isArray(probe)) {
      issues.push(`${prefix}: HTTP probe must be an object`);
      continue;
    }
    if (!isHeaderMap(probe.expectHeaders)) {
      issues.push(`${prefix}: expectHeaders must be a non-empty string map`);
    }
  }
}

function validateEntryShape(item, matrix) {
  const entry = item.entry;
  const issues = [];
  validateHttpServiceProbes(entry, issues);
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
        issues.push(...applicabilityFactIssues(entry, language, "", fact));
      }
    }
  }
  const metricApplicable = entry.metric_applicability;
  if (metricApplicable !== undefined) {
    if (!metricApplicable || typeof metricApplicable !== "object" || Array.isArray(metricApplicable)) {
      issues.push(`${entry.name}: metric_applicability must be an object`);
    } else {
      for (const [language, facts] of Object.entries(metricApplicable)) {
        if (!(entry.languages ?? []).includes(language)) issues.push(`${entry.name}/${language}: metric_applicability language is not declared`);
        if (!facts || typeof facts !== "object" || Array.isArray(facts)) {
          issues.push(`${entry.name}/${language}: metric_applicability must map metrics to facts`);
          continue;
        }
        for (const [metric, fact] of Object.entries(facts)) {
          if (metric !== "*" && !comparisonMetrics(entry.mode).includes(metric)) {
            issues.push(`${entry.name}/${language}/${metric}: unknown comparable metric`);
          }
          issues.push(...applicabilityFactIssues(entry, language, metric, fact));
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
  if (JSON.stringify(matrix?.metric_applicability) !== JSON.stringify(METRIC_APPLICABILITY_POLICY)) {
    issues.push("matrix is missing the metric applicability policy");
  }
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
  for (const name of ["run", "main"]) {
    const preferred = path.join(dir, "build", name);
    if (await exists(preferred)) return preferred;
  }
  const found = [];
  async function walk(current) {
    for (const item of await fs.readdir(current, { withFileTypes: true })) {
      if (item.name === ".jet" || item.name === "zig-cache" || item.name === "zig-global-cache") continue;
      const full = path.join(current, item.name);
      if (item.isDirectory()) await walk(full);
      else if (item.name === "run" || item.name === "run.exe" || item.name === "main" || item.name === "main.exe") {
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
  if (language === "jet") return [jetBin, "build", "run.jet"];
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
  const failure = cold.exit_code !== 0 ? `cold build exit ${cold.exit_code}` :
    !validBuildSample(cold) ? "cold build measurement invalid" :
      warm?.exit_code !== 0 ? `warm build exit ${warm?.exit_code}` :
        !validBuildSample(warm) ? "warm build measurement invalid" :
          !await exists(artifact) ? "build produced no executable" : null;
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
  const failure = cold.exit_code !== 0 ? `cold build exit ${cold.exit_code}` :
    !validBuildSample(cold) ? "cold build measurement invalid" :
      warm?.exit_code !== 0 ? `warm build exit ${warm?.exit_code}` :
        !validBuildSample(warm) ? "warm build measurement invalid" : null;
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
  let rssKb = await processTreeRssKb(child.pid);
  let rssSamplePromise = Promise.resolve();
  const sampleRss = () => {
    rssSamplePromise = rssSamplePromise.then(async () => {
      const value = await processTreeRssKb(child.pid);
      if (Number.isFinite(value)) rssKb = Math.max(rssKb ?? 0, value);
    }).catch(() => {});
  };
  const rssTimer = setInterval(sampleRss, 20);
  const latencies = [];
  const repeatProbes = probes.slice(0, -1);
  let measurementFailure = null;
  try {
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
  } catch (error) {
    measurementFailure = error.message;
  } finally {
    clearInterval(rssTimer);
    await rssSamplePromise;
  }
  const finalRssKb = await processTreeRssKb(child.pid);
  if (Number.isFinite(finalRssKb)) rssKb = Math.max(rssKb ?? 0, finalRssKb);
  const exit = await waitForExit(child, 5000);
  if (exit.code === null) { stopProcess(child); await waitForExit(child); }
  const measurementInvalid = !positiveMetricValue("service_startup_seconds", startupSeconds) ||
    !positiveMetricValue("service_peak_rss_kb", rssKb) ||
    latencies.length === 0 ||
    !latencies.every((value) => positiveMetricValue("service_latency_ms", value));
  const finalFailure = measurementFailure ??
    (measurementInvalid ? "service timing or RSS measurement invalid" :
      exit.code !== 0 ? `service clean exit ${exit.code ?? exit.signal}` : null);
  return {
    failure: finalFailure,
    startupSeconds,
    latencyMs: { median: median(latencies), p99: percentile(latencies, 0.99) },
    rssKb,
    cleanExit: exit.code === 0,
    exitCode: exit.code,
    readySeconds: ready.seconds,
  };
}

async function collectServiceTierTrace(sourceDir, entry, commandForOverride = null, {
  full = entry.spec?.fullShell === true,
  timeoutMs = DEFAULT_TIMEOUT_MS,
} = {}) {
  const service = entry.spec?.service ?? entry.service ?? {};
  const protocol = service.protocol ?? "http";
  const probes = service.probe ?? [];
  if (!service.portArg || probes.length === 0 || !["http", "line"].includes(protocol) ||
      (protocol === "http" && !service.readyPath) ||
      (protocol === "line" && !service.ready)) {
    return unavailableTierTrace("service requires portArg, readiness, and probe");
  }
  const commandFor = commandForOverride ?? ((port) => ["jet", "run", "run.jet", "--", String(port)]);
  const displayCommand = commandFor("<port>");
  const sidecar = path.join(sourceDir, ".jet-tier-trace.json");
  try {
    await fs.rm(sidecar, { force: true });
  } catch (error) {
    return unavailableTierTrace(`could not remove stale tier trace sidecar: ${error.message}`);
  }
  const invocations = [];
  let child = null;
  let exitCode = null;
  let reason = null;
  try {
    const port = await freePort();
    child = startProcess(sourceDir, tracedCommand(commandFor(port)), {
      full,
      env: { JET_TRACE_TIERS_PATH: sidecar },
    });
    await waitForReady(child, port, service);
    const verification = await probeSequence(port, probes, protocol);
    if (!verification.ok) reason = `probe ${verification.index} failed: ${verification.reason}`;
    let exit = await waitForExit(child, 5000);
    if (exit.code === null) {
      if (!reason) reason = "trace service shutdown did not produce a clean exit";
      stopProcess(child);
      exit = await waitForExit(child);
    }
    exitCode = exit.code;
    if (exitCode !== 0 && !reason) reason = `trace service exited ${exitCode ?? exit.signal}`;
  } catch (error) {
    reason = `tier trace service failed: ${error.message}`;
    if (child) {
      stopProcess(child);
      const exit = await waitForExit(child);
      exitCode = exit.code;
    }
  }
  try {
    const facts = await readTierFacts(sidecar);
    const invocation = facts.error
      ? emptyTierInvocation(displayCommand, exitCode)
      : { command: [...displayCommand], exit_code: exitCode, ...facts };
    invocations.push(invocation);
    if (facts.error && !reason) reason = facts.error;
  } finally {
    await fs.rm(sidecar, { force: true });
  }
  return tierTraceResult(invocations, reason);
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

function ratioVerdict(ratio, peerLanguage = null) {
  if (!Number.isFinite(ratio) || ratio <= 0) return null;
  if (baseLanguage(peerLanguage ?? "") === "rust") {
    if (ratio < 1) return "win";
    if (ratio <= 1.05) return "parity";
    return "loss";
  }
  return ratio < 1 ? "win" : "loss";
}

function comparisons(entry, languages, rows, tiers = {}) {
  const metrics = comparisonMetrics(entry.mode);
  const policy = tierPolicy(entry.mode);
  const rowMap = rows && typeof rows === "object" && !Array.isArray(rows) ? rows : {};
  const defaultJet = rowMap.jet;
  const output = {};
  for (const language of (Array.isArray(languages) ? languages : [])
    .filter((item) => typeof item === "string" && item !== "jet" && item !== "jet-expert")) {
    const peer = rowMap[language];
    const expertPeer = language.endsWith("-expert");
    const jetConfiguration = expertPeer ? "jet-expert" : "jet";
    const matchedJet = expertPeer ? rowMap["jet-expert"] : defaultJet;
    const comparisonTiers = expertPeer
      ? {
          aot: {
            status: matchedJet?.status === "ok" ? "ok" : (matchedJet?.status ?? "unavailable"),
            metrics: matchedJet?.metrics ?? {},
          },
        }
      : tiers;
    const comparisonPolicy = expertPeer ? { aot: { required: true } } : policy;
    const comparisonRequiredTiers = Object.entries(comparisonPolicy)
      .filter(([, value]) => value.required)
      .map(([tier]) => tier);
    const matchedJetTiersReady = comparisonRequiredTiers.every((tier) => comparisonTiers[tier]?.status === "ok");
    const declaredPeerNonApplicable = entry?.non_applicable?.[language];
    const undeclaredPeerNonApplicable = peer?.status === "not_applicable" && !declaredPeerNonApplicable;
    const comparison = {
      status: undeclaredPeerNonApplicable ? "invalid" : (peer?.status ?? "unavailable"),
      applicable: peer?.status === "not_applicable" ? declaredPeerNonApplicable === undefined : true,
      basis: peer?.status === "not_applicable"
        ? (declaredPeerNonApplicable ? "declared-non-applicability" : "undeclared-non-applicability")
        : expertPeer ? "jet-expert-aot-and-matched-peer" : "jet-aot-and-run",
      jet_configuration: jetConfiguration,
      jet_tiers_ready: matchedJetTiersReady,
      required_tiers: comparisonRequiredTiers,
      tier_policy: comparisonPolicy,
      peer_sample: {
        binding: PEER_MEASUREMENT_POLICY.sample_binding,
        source: `rows.${language}.metrics`,
        language,
        source_sha256: peer?.metrics?.source_sha256 ?? null,
      },
      primary_metric: primaryMetric(entry.mode),
      metrics: {},
      tiers: {},
      verdicts: {},
    };
    if (undeclaredPeerNonApplicable) {
      comparison.reason = "peer is marked not_applicable without a declared structural reason";
      output[language] = comparison;
      continue;
    }
    if (peer?.status === "not_applicable") {
      comparison.reason = peer.reason;
      comparison.evidence = peer.evidence;
      output[language] = comparison;
      continue;
    }
    for (const tier of Object.keys(comparisonPolicy)) {
      const tierReady = comparisonTiers[tier]?.status === "ok";
      const jetMetrics = tier === "aot" ? matchedJet?.metrics : comparisonTiers[tier]?.metrics;
      const tierComparison = {
        metrics: {},
        peer_sample: {
          binding: PEER_MEASUREMENT_POLICY.sample_binding,
          source: `rows.${language}.metrics`,
          language,
          source_sha256: peer?.metrics?.source_sha256 ?? null,
        },
      };
      for (const metric of metrics) {
        const applicability = metricApplicability(entry, language, metric);
        if (applicability.status === "invalid") {
          tierComparison.metrics[metric] = {
            status: "unmeasured",
            applicability,
            jet: null,
            peer: null,
            ratio: null,
            verdict: null,
            reason: applicability.reason,
          };
          continue;
        }
        if (applicability.status === "not_applicable") {
          tierComparison.metrics[metric] = {
            status: "not_applicable",
            applicability,
            jet: null,
            peer: null,
            ratio: null,
            verdict: null,
          };
          continue;
        }
        if (!metricComparableAtTier(metric, tier)) {
          tierComparison.metrics[metric] = {
            status: "not_applicable",
            applicability: {
              status: "not_applicable",
              basis: "no_compile_phase",
              reason: `${metric} is a compile-only measurement and the interpreted ${tier} tier has no compile phase.`,
              evidence: "Build, rebuild, and binary metrics are measured only from the AOT artifact; interpreted tiers have no compile phase.",
            },
            jet: null,
            peer: null,
            ratio: null,
            verdict: null,
          };
          continue;
        }
        const jetValue = positiveMetricValue(metric, jetMetrics?.[metric]) ? jetMetrics[metric] : null;
        const peerValue = positiveMetricValue(metric, peer?.metrics?.[metric]) ? peer.metrics[metric] : null;
        const ratio = tierReady && peer?.status === "ok" && peerValue !== null && jetValue !== null
          ? jetValue / peerValue
          : null;
        const verdict = ratioVerdict(ratio, language);
        tierComparison.metrics[metric] = {
          status: verdict === null ? "unmeasured" : "measured",
          jet: jetValue,
          peer: peerValue,
          ratio,
          verdict,
          reason: verdict === null
            ? (!tierReady ? `matched Jet ${jetConfiguration} ${tier} tier is unavailable`
              : peer?.status !== "ok" ? "peer row is unavailable"
                : jetValue === null ? `matched Jet ${jetConfiguration} metric is missing or invalid`
                  : peerValue === null ? "peer metric is missing or invalid"
                    : "invalid metric ratio")
            : null,
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
  return Object.fromEntries(Object.keys(policy).map((tier) => {
    const tierPolicyValue = policy[tier];
    if (tier === "dev" && !dev) return [tier, unavailableTier(tierPolicyValue.required, "jet dev is unavailable")];
    return [tier, unavailableTier(tierPolicyValue.required, reason)];
  }));
}

function jetTierCommands(entry, tier, jetBin) {
  const prefix = tier === "run"
    ? [jetBin, "run", "run.jet", "--"]
    : [jetBin, "dev", "--watch=off", "run.jet", "--"];
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
      if (row.runtime.valid !== true) {
        row.status = "broken";
        row.reason = "measured run samples are invalid";
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
      if (row.runtime.valid !== true) {
        row.status = "broken";
        row.reason = "measured run samples are invalid";
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
    if (row.runtime.valid !== true) {
      row.status = "broken";
      row.reason = "measured run samples are invalid";
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
  const traceRequests = [];
  for (const tier of Object.keys(tierPolicy(entry.mode))) {
    const policy = tierPolicy(entry.mode)[tier];
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
        ? [jetBin, "run", "run.jet", "--", String(port)]
        : [jetBin, "dev", "--watch=off", "run.jet", "--", String(port)];
      const service = await runService("jet", jetDir, null, entry, commandFor);
      const tierRow = {
        applicable: true,
        required: policy.required,
        status: service.failure ? "broken" : "ok",
        reason: service.failure,
        command: commandFor("<port>"),
        verification: { status: service.failure ? "failed" : "passed", kind: "service_probe_sequence", reason: service.failure },
        metrics: {
          ...sourceMetricValues(jetRow?.metrics),
          runtime_wall_seconds: null,
          runtime_first_stdout_seconds: null,
          runtime_peak_rss_kb: service.rssKb ?? null,
          service_startup_seconds: service.startupSeconds ?? null,
          service_latency_ms_p50: service.latencyMs?.median ?? null,
          service_latency_ms_p99: service.latencyMs?.p99 ?? null,
          binary_bytes: null,
        },
      };
      if (!service.failure && tier === "run") {
        traceRequests.push({ kind: "service", tierRow, commandFor });
      }
      tiers[tier] = tierRow;
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
      if (tierRow.runtime.valid !== true) {
        tierRow.status = "broken";
        tierRow.reason = "measured run samples are invalid";
        tierRow.verification = { status: "failed", kind: "timed_run", reason: tierRow.reason };
      } else {
        tierRow.metrics = { ...sourceMetricValues(jetRow?.metrics), ...runtimeMetrics(tierRow.runtime), binary_bytes: null };
        if (tier === "run") {
          traceRequests.push({ kind: "batch", tierRow, commands, expected, reset });
        }
      }
    }
    tiers[tier] = tierRow;
  }
  for (const request of traceRequests) {
    const trace = request.kind === "service"
      ? await collectServiceTierTrace(jetDir, entry, request.commandFor)
      : await collectTierTrace(jetDir, request.commands, request.expected, request.reset, { timeoutMs: TIER_TIMEOUT_MS });
    request.tierRow.trace = trace;
    if (trace.status !== "passed") {
      request.tierRow.status = "broken";
      request.tierRow.reason = trace.reason;
      request.tierRow.verification = {
        status: "failed",
        kind: request.kind === "service" ? "service_probe_sequence" : "byte_exact_stdout",
        reason: trace.reason,
      };
    }
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

function liveLossOwner(declaredOwners, metric, tower) {
  const category = LOSS_OWNER_CATEGORY_BY_METRIC[metric] ?? null;
  const number = category && declaredOwners && typeof declaredOwners === "object" && !Array.isArray(declaredOwners)
    ? declaredOwners[category]
    : null;
  if (!category) return { status: "missing", metric, category: null, reason: "no owner category is declared for this metric" };
  if (!Number.isInteger(number)) return { status: "missing", metric, category, reason: `no owner card is declared for ${category} metrics` };
  const towerState = tower && typeof tower === "object" ? tower : { status: "unavailable", reason: "Tower state is missing" };
  if (towerState.status !== "available" || !(towerState.cards instanceof Map)) {
    return { status: "unavailable", metric, category, card: number, reason: towerState.reason ?? "Tower cards are unavailable" };
  }
  const card = towerState.cards.get(number);
  if (!card) return { status: "stale", metric, category, card: number, reason: "declared owner card is absent" };
  if (["done", "cancelled", "frozen"].includes(card.phase)) {
    return { status: "stale", metric, category, card: number, title: card.title, phase: card.phase, reason: "declared owner card is terminal" };
  }
  return { status: "live", metric, category, card: number, title: card.title, phase: card.phase, assignee: card.assignee ?? null };
}

function cellVerdict(verdicts) {
  const comparable = verdicts.filter((verdict) => verdict !== "not_applicable");
  if (!comparable.length) return "not_applicable";
  if (comparable.some((verdict) => !["win", "parity", "loss"].includes(verdict))) {
    return "unmeasured";
  }
  if (comparable.includes("loss")) return "loss";
  if (comparable.includes("parity")) return "parity";
  return "win";
}

function unmeasuredMetricComparison(declaredTiers, applicability, reason) {
  const tiers = Object.fromEntries(declaredTiers.map((tier) => [tier, {
    status: "unmeasured",
    applicability,
    jet: null,
    peer: null,
    ratio: null,
    verdict: null,
    reason,
  }]));
  return {
    status: "unmeasured",
    applicability,
    tiers,
    verdict: "unmeasured",
    reason,
  };
}

function scoreboardMetricComparison(entry, language, comparison, metric, declaredTiers) {
  const ratioTiers = declaredTiers.filter((tier) => PEER_MEASUREMENT_POLICY.ratio_tiers.includes(tier) && metricComparableAtTier(metric, tier));
  const declared = metricApplicability(entry, language, metric);
  if (declared.status === "invalid") {
    return unmeasuredMetricComparison(declaredTiers, declared, declared.reason);
  }
  const declaredNotApplicable = declared.status === "not_applicable";
  const comparisonNotApplicable = comparison?.applicable === false || comparison?.status === "not_applicable";
  if (comparisonNotApplicable && !declaredNotApplicable) {
    return unmeasuredMetricComparison(
      declaredTiers,
      { status: "invalid", reason: "comparison marked metric not_applicable without a declared structural reason" },
      "comparison marked metric not_applicable without a declared structural reason",
    );
  }
  if (declaredNotApplicable) {
    const tiers = Object.fromEntries(declaredTiers.map((tier) => [tier, {
      status: "not_applicable",
      applicability: declared,
      jet: null,
      peer: null,
      ratio: null,
      verdict: null,
    }]));
    return {
      status: "not_applicable",
      applicability: declared,
      tiers,
      verdict: "not_applicable",
      reason: null,
    };
  }
  const tiers = Object.fromEntries(declaredTiers.map((tier) => {
    const item = comparison?.tiers?.[tier]?.metrics?.[metric];
    if (item?.status === "not_applicable") {
      if (!metricComparableAtTier(metric, tier)) {
        return [tier, { ...item, status: item.status, verdict: null }];
      }
      return [tier, {
        ...item,
        status: "unmeasured",
        verdict: null,
        reason: "metric marked not_applicable without a declared structural reason",
      }];
    }
    if (item?.status === "trace_only" || item?.status === "not_comparable") {
      return [tier, {
        ...item,
        status: "unmeasured",
        verdict: null,
        reason: `${metric} is not a ratio-measured metric on ${tier}`,
      }];
    }
    const expectedVerdict = positiveMetricValue(metric, item?.jet) && positiveMetricValue(metric, item?.peer) &&
      Number.isFinite(item?.ratio) && item.ratio === item.jet / item.peer
      ? ratioVerdict(item.ratio, language)
      : null;
    const valid = item?.status === "measured" &&
      expectedVerdict !== null &&
      item.verdict === expectedVerdict;
    if (!item || !valid) {
      return [tier, {
        ...(item ?? {}),
        status: "unmeasured",
        jet: item?.jet ?? null,
        peer: item?.peer ?? null,
        ratio: item?.ratio ?? null,
        verdict: null,
        reason: item?.reason ?? "missing or invalid metric comparison",
      }];
    }
    return [tier, { ...item, status: "measured" }];
  }));
  const verdict = cellVerdict(ratioTiers.map((tier) => tiers[tier]?.verdict ?? null));
  const reasons = ratioTiers.map((tier) => tiers[tier]?.reason).filter(Boolean);
  return {
    status: verdict === "unmeasured" ? "unmeasured" : "measured",
    applicability: declared,
    tiers,
    verdict,
    reason: reasons.length ? [...new Set(reasons)].join("; ") : null,
  };
}


function validateResultShape(result, matrix = null) {
  const issues = [];
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    return ["result is not an object"];
  }
  const entry = result.entry;
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
    return ["result is missing entry identity"];
  }
  const name = typeof entry.name === "string" ? entry.name : "<unnamed>";
  const languages = Array.isArray(entry.languages) ? entry.languages : [];
  const expectedRows = new Set(languages);
  const rows = result.rows;
  const rowMap = rows && typeof rows === "object" && !Array.isArray(rows) ? rows : {};
  const expectedPeers = languages.filter((language) => typeof language === "string" && language !== "jet" && language !== "jet-expert");
  const comparisonMap = result.comparisons && typeof result.comparisons === "object" && !Array.isArray(result.comparisons)
    ? result.comparisons
    : {};
  const tierMap = result.jet_tiers && typeof result.jet_tiers === "object" && !Array.isArray(result.jet_tiers)
    ? result.jet_tiers
    : {};
  const policy = tierPolicy(entry.mode);
  const metrics = comparisonMetrics(entry.mode);

  selectionOrAggregateIssues(result.rows, `${name}.rows`, issues);
  selectionOrAggregateIssues(result.comparisons, `${name}.comparisons`, issues);
  selectionOrAggregateIssues(result.jet_tiers, `${name}.jet_tiers`, issues);
  if (!ENTRY_MODES.includes(entry.mode)) issues.push(`${name}: unsupported result mode ${JSON.stringify(entry.mode)}`);
  if (!VALID_RESULT_STATUSES.has(result.status)) {
    issues.push(`${name}: invalid result status ${JSON.stringify(result.status)}`);
  } else if (result.status !== "ok") {
    issues.push(`${name}: result status ${result.status} is not publishable`);
  }
  if (!Array.isArray(entry.languages) || languages.length === 0) {
    issues.push(`${name}: result has no declared language rows`);
  } else if (new Set(languages).size !== languages.length) {
    issues.push(`${name}: result declares duplicate languages`);
  }
  if (!Array.isArray(entry.cells) || entry.cells.length === 0) {
    issues.push(`${name}: result has no declared matrix cells`);
  } else if (new Set(entry.cells).size !== entry.cells.length) {
    issues.push(`${name}: result declares duplicate matrix cells`);
  }
  for (const language of languages) {
    if (typeof language !== "string") {
      issues.push(`${name}: result declares non-string language ${JSON.stringify(language)}`);
      continue;
    }
    const row = rowMap[language];
    if (!row || typeof row !== "object" || Array.isArray(row)) {
      issues.push(`${name}/${language}: missing result row`);
      continue;
    }
    if (row.language !== language) issues.push(`${name}/${language}: row identity is ${JSON.stringify(row.language)}`);
    if (!VALID_RESULT_STATUSES.has(row.status)) {
      issues.push(`${name}/${language}: invalid row status ${JSON.stringify(row.status)}`);
      continue;
    }
    if (row.status === "not_applicable") {
      if (!entry.non_applicable?.[language]) {
        issues.push(`${name}/${language}: row is not_applicable without a declared structural reason`);
      }
      if (row.verification?.status !== "not_applicable") {
        issues.push(`${name}/${language}: not_applicable row is missing not_applicable verification`);
      }
      continue;
    }
    if (row.status !== "ok") {
      issues.push(`${name}/${language}: row status ${row.status} is not publishable`);
      continue;
    }
    if (row.disqualified === true) issues.push(`${name}/${language}: successful row is disqualified`);
    if (!row.verification || row.verification.status !== "passed" ||
      !VALID_PASS_VERIFICATION_KINDS.has(row.verification.kind)) {
      issues.push(`${name}/${language}: successful row is missing valid passed verification`);
    }
    if (!row.metrics || typeof row.metrics !== "object" || Array.isArray(row.metrics)) {
      issues.push(`${name}/${language}: successful row is missing metrics`);
      continue;
    }
    if (!row.provenance || row.provenance.base_language !== baseLanguage(language)) {
      issues.push(`${name}/${language}: source identity does not match declared language`);
    }
    if (typeof row.metrics.source_sha256 !== "string" || row.metrics.source_sha256.length === 0 ||
      row.provenance?.source_sha256 !== row.metrics.source_sha256) {
      issues.push(`${name}/${language}: source hash does not match measured source`);
    }
    for (const metric of metrics) {
      if (metricApplicability(entry, language, metric).status === "required" &&
        !positiveMetricValue(metric, row.metrics[metric])) {
        issues.push(`${name}/${language}/${metric}: required row metric is missing or invalid`);
      }
    }
  }
  for (const language of Object.keys(rowMap)) {
    if (!expectedRows.has(language)) issues.push(`${name}/${language}: result row is not declared`);
  }

  for (const [tier, tierPolicyValue] of Object.entries(policy)) {
    const tierResult = tierMap[tier];
    if (!tierResult || typeof tierResult !== "object" || Array.isArray(tierResult)) {
      issues.push(`${name}/jet/${tier}: missing Jet tier result`);
      continue;
    }
    if (!VALID_RESULT_STATUSES.has(tierResult.status)) {
      issues.push(`${name}/jet/${tier}: invalid tier status ${JSON.stringify(tierResult.status)}`);
      continue;
    }
    if (tierPolicyValue.required && tierResult.status !== "ok") {
      issues.push(`${name}/jet/${tier}: required tier status ${tierResult.status}`);
    }
    if (tierResult.status === "not_applicable") {
      issues.push(`${name}/jet/${tier}: declared Jet tier cannot be not_applicable`);
      continue;
    }
    if (tierResult.status === "ok") {
      if (!tierResult.verification || tierResult.verification.status !== "passed" ||
        !VALID_PASS_VERIFICATION_KINDS.has(tierResult.verification.kind)) {
        issues.push(`${name}/jet/${tier}: successful tier is missing valid passed verification`);
      }
      if (!tierResult.metrics || typeof tierResult.metrics !== "object" || Array.isArray(tierResult.metrics)) {
        issues.push(`${name}/jet/${tier}: successful tier is missing metrics`);
        continue;
      }
      for (const metric of metrics) {
        if (metricComparableAtTier(metric, tier) && !positiveMetricValue(metric, tierResult.metrics[metric])) {
          issues.push(`${name}/jet/${tier}/${metric}: required tier metric is missing or invalid`);
        }
      }
      if (tier === "run") {
        const trace = tierResult.trace;
        const validTrace = trace?.status === "passed" &&
          trace.channel === "compiler_owned_sidecar" &&
          trace.whole_program_deopt === false &&
          trace.native_rows > 0 && Number.isInteger(trace.native_rows) &&
          trace.interp_rows === 0 &&
          Array.isArray(trace.rows) && trace.rows.length > 0 &&
          Array.isArray(trace.invocations) && trace.invocations.length > 0 &&
          trace.invocations.every((invocation) =>
            Array.isArray(invocation.rows) && invocation.rows.length > 0 &&
            Number.isInteger(invocation.native_rows) && invocation.native_rows > 0 &&
            invocation.interp_rows === 0 &&
            invocation.whole_program_deopt === false);
        if (!validTrace) {
          issues.push(`${name}/jet/run: successful tier is missing valid native tier trace; whole-program deopts fail closed`);
        }
      }
    }
  }
  for (const tier of Object.keys(tierMap)) {
    if (!Object.hasOwn(policy, tier)) issues.push(`${name}/jet/${tier}: tier is not declared for this mode`);
  }

  if (!result.comparisons || typeof result.comparisons !== "object" || Array.isArray(result.comparisons)) {
    issues.push(`${name}: missing peer comparison rows`);
  }
  for (const language of Object.keys(comparisonMap)) {
    if (!expectedPeers.includes(language)) issues.push(`${name}/${language}: comparison row is not declared`);
  }
  for (const language of expectedPeers) {
    const comparison = comparisonMap[language];
    const row = rowMap[language];
    if (!comparison || typeof comparison !== "object" || Array.isArray(comparison)) {
      issues.push(`${name}/${language}: missing peer comparison row`);
      continue;
    }
    if (!VALID_RESULT_STATUSES.has(comparison.status)) {
      issues.push(`${name}/${language}: invalid comparison status ${JSON.stringify(comparison.status)}`);
    }
    const expectedRowStatus = row?.status ?? "unavailable";
    if (comparison.status !== expectedRowStatus) {
      issues.push(`${name}/${language}: comparison status does not match peer row`);
    }
    const expectedJetConfiguration = language.endsWith("-expert") ? "jet-expert" : "jet";
    if (comparison.jet_configuration !== expectedJetConfiguration) {
      issues.push(`${name}/${language}: comparison uses ${JSON.stringify(comparison.jet_configuration)} instead of ${expectedJetConfiguration}`);
    }
    const expectedApplicable = expectedRowStatus === "not_applicable" ? false : true;
    if (comparison.applicable !== expectedApplicable) {
      issues.push(`${name}/${language}: comparison applicability does not match peer row`);
    }
    const expectedSampleSource = `rows.${language}.metrics`;
    const sampleScopes = [
      ["comparison", comparison.peer_sample],
      ...Object.entries(comparison.tiers ?? {}).map(([tier, value]) => [`${tier} tier`, value?.peer_sample]),
    ];
    for (const [scope, sample] of sampleScopes) {
      if (!sample || sample.binding !== PEER_MEASUREMENT_POLICY.sample_binding ||
        sample.source !== expectedSampleSource ||
        sample.language !== language ||
        sample.source_sha256 !== (row?.metrics?.source_sha256 ?? null)) {
        issues.push(`${name}/${language}/${scope}: peer sample identity does not match its row`);
      }
    }
    const expectedComparisonPolicy = language.endsWith("-expert") ? { aot: { required: true } } : policy;
    const comparisonPolicy = comparison.tier_policy && typeof comparison.tier_policy === "object" &&
      !Array.isArray(comparison.tier_policy) ? comparison.tier_policy : {};
    if (JSON.stringify(comparisonPolicy) !== JSON.stringify(expectedComparisonPolicy)) {
      issues.push(`${name}/${language}: comparison tier policy does not match the declared mode`);
    }
    for (const tier of Object.keys(comparison.tiers ?? {})) {
      if (!Object.hasOwn(expectedComparisonPolicy, tier)) issues.push(`${name}/${language}/${tier}: comparison tier is not declared`);
    }
    if (comparison.status === "not_applicable" && entry.non_applicable?.[language]) continue;
    for (const [tier, tierPolicyValue] of Object.entries(expectedComparisonPolicy)) {
      if (!tierPolicyValue.required && !PEER_MEASUREMENT_POLICY.ratio_tiers.includes(tier)) continue;
      const tierComparison = comparison.tiers?.[tier];
      if (!tierComparison || typeof tierComparison !== "object" || Array.isArray(tierComparison)) {
        if (tierPolicyValue.required || PEER_MEASUREMENT_POLICY.ratio_tiers.includes(tier)) {
          issues.push(`${name}/${language}/${tier}: missing peer tier comparison`);
        }
        continue;
      }
      if (!PEER_MEASUREMENT_POLICY.ratio_tiers.includes(tier)) continue;
      for (const metric of metrics) {
        const item = tierComparison.metrics?.[metric];
        const applicability = metricApplicability(entry, language, metric);
        const expectedNotApplicable = applicability.status === "not_applicable" || !metricComparableAtTier(metric, tier);
        if (expectedNotApplicable) {
          if (item?.status !== "not_applicable") {
            issues.push(`${name}/${language}/${tier}/${metric}: required not_applicable cell is missing`);
          }
          continue;
        }
        if (item?.status !== "measured" || !positiveMetricValue(metric, item?.jet) ||
          !positiveMetricValue(metric, item?.peer) || !Number.isFinite(item?.ratio) ||
          item.ratio !== item.jet / item.peer || item.verdict !== ratioVerdict(item.ratio, language)) {
          issues.push(`${name}/${language}/${tier}/${metric}: missing or invalid independently measured cell`);
        }
      }
    }
  }
  if (matrix) {
    const knownCells = new Set((matrix.cells ?? []).map((cell) => cell.id));
    for (const cell of Array.isArray(entry.cells) ? entry.cells : []) {
      if (!knownCells.has(cell)) issues.push(`${name}: result declares unknown matrix cell ${cell}`);
    }
  }
  return [...new Set(issues)];
}
function buildScoreboard(matrix, results, manifest, tower) {
  const inputResults = Array.isArray(results) ? results : [];
  const resultValidationIssues = inputResults.flatMap((result) => validateResultShape(result, matrix));
  const canonicalResults = inputResults.filter((result) => result?.entry && typeof result.entry === "object")
    .map((result) => ({
      ...result,
      comparisons: comparisons(result.entry, result.entry.languages, result.rows, result.jet_tiers),
    }));
  const entriesByCell = new Map();
  for (const result of canonicalResults) {
    for (const cell of (Array.isArray(result.entry.cells) ? result.entry.cells : [])) {
      const values = entriesByCell.get(cell) ?? [];
      values.push(result);
      entriesByCell.set(cell, values);
    }
  }
  const declaredOwners = manifest?.loss_owners ?? {};
  const towerState = tower && typeof tower === "object" ? tower : { status: "unavailable", reason: "Tower state is missing" };
  const cells = (Array.isArray(matrix?.cells) ? matrix.cells : []).map((cell) => {
    const candidates = entriesByCell.get(cell.id) ?? [];
    const records = candidates.map((result) => {
      const metric = primaryMetric(result.entry.mode);
      const metrics = comparisonMetrics(result.entry.mode);
      const jetTierPolicy = tierPolicy(result.entry.mode);
      const requiredTiers = Object.entries(jetTierPolicy)
        .filter(([, policy]) => policy.required)
        .map(([tier]) => tier);
      const peers = (Array.isArray(result.entry.languages) ? result.entry.languages : [])
        .filter((language) => typeof language === "string" && language !== "jet" && language !== "jet-expert")
        .map((language) => {
          const comparison = result.comparisons?.[language];
          const comparisonPolicy = comparison?.tier_policy ?? jetTierPolicy;
          const comparisonTiers = Object.keys(comparisonPolicy);
          const metricComparisons = Object.fromEntries(metrics.map((item) => [
            item,
            scoreboardMetricComparison(result.entry, language, comparison, item, comparisonTiers),
          ]));
          const metricVerdicts = Object.fromEntries(metrics.map((item) => [item, metricComparisons[item].verdict]));
          const primaryComparison = metricComparisons[metric];
          const primaryAot = primaryComparison?.tiers?.aot ?? null;
          const tierVerdicts = Object.fromEntries(comparisonTiers.map((tier) => [
            tier,
            primaryComparison?.tiers?.[tier]?.verdict ?? null,
          ]));
          const metricFailures = Object.entries(metricComparisons)
            .filter(([, item]) => !["win", "parity", "not_applicable"].includes(item.verdict))
            .map(([item, value]) => {
              const owner = value.verdict === "loss"
                ? liveLossOwner(declaredOwners[result.entry.name], item, towerState)
                : null;
              return { metric: item, peer: language, verdict: value.verdict, reason: value.reason, tiers: value.tiers, owner };
            });
          const ratioTierVerdicts = PEER_MEASUREMENT_POLICY.ratio_tiers
            .filter((tier) => comparisonTiers.includes(tier))
            .map((tier) => tierVerdicts[tier] ?? null);
          const primaryVerdict = cellVerdict(ratioTierVerdicts);
          return {
            language,
            applicable: comparison?.applicable !== false,
            status: comparison?.status ?? result.rows?.[language]?.status ?? "unavailable",
            jet: primaryAot?.jet ?? null,
            peer: primaryAot?.peer ?? null,
            trace_only_tiers: PEER_MEASUREMENT_POLICY.trace_only_tiers.filter((tier) => comparisonTiers.includes(tier)),
            jet_configuration: comparison?.jet_configuration ?? "jet",
            required_tiers: comparison?.required_tiers ?? comparisonTiers.filter((tier) => comparisonPolicy[tier]?.required),
            tier_policy: comparisonPolicy,
            peer_sample: comparison?.peer_sample ?? null,
            ratio: primaryAot?.ratio ?? null,
            verdict: primaryVerdict === "not_applicable" ? null : primaryVerdict,
            tier_verdicts: tierVerdicts,
            all_required_tiers_verdict: primaryVerdict,
            metric_comparisons: metricComparisons,
            metric_verdicts: metricVerdicts,
            metric_failures: metricFailures,
          };
        });
      const jetTierStatuses = Object.fromEntries(Object.keys(jetTierPolicy).map((tier) => [tier, result.jet_tiers?.[tier]?.status ?? "unavailable"]));
      const tiersReady = requiredTiers.every((tier) => jetTierStatuses[tier] === "ok");
      const applicablePeers = peers.filter((peer) => peer.applicable !== false);
      const metricVerdicts = Object.fromEntries(metrics.map((item) => [
        item,
        cellVerdict(applicablePeers.map((peer) => peer.metric_verdicts[item] ?? null)),
      ]));
      const metricFailures = Object.entries(metricVerdicts)
        .filter(([, verdict]) => !["win", "parity", "not_applicable"].includes(verdict))
        .map(([item, verdict]) => {
          const peerFailures = applicablePeers.flatMap((peer) => peer.metric_failures.filter((failure) => failure.metric === item));
          return {
            metric: item,
            verdict,
            reasons: [...new Set(peerFailures.map((failure) => failure.reason).filter(Boolean))],
            peers: peerFailures,
          };
        });
      const lossOwners = metricFailures.flatMap((failure) => failure.peers
        .filter((peerFailure) => peerFailure.verdict === "loss")
        .map((peerFailure) => ({ entry: result.entry.name, peer: peerFailure.peer, ...peerFailure.owner })));
      const verdict = result.rows?.jet?.status === "ok" && tiersReady
        ? (applicablePeers.length > 0 ? cellVerdict(Object.values(metricVerdicts)) : "unmeasured")
        : "unmeasured";
      const primaryVerdict = metricVerdicts[metric] ?? "unmeasured";
      return {
        entry: result.entry.name,
        mode: result.entry.mode,
        status: result.status,
        primary_metric: metric,
        primary_verdict: primaryVerdict,
        jet: result.rows?.jet?.metrics?.[metric] ?? null,
        jet_tiers: jetTierStatuses,
        tiers_ready: tiersReady,
        peers,
        metric_verdicts: metricVerdicts,
        metric_failures: metricFailures,
        loss_owners: lossOwners,
        verdict,
      };
    });
    const verdict = records.length === 1 ? records[0].verdict : "unmeasured";
    return {
      id: cell.id,
      domain: cell.domain,
      kind: cell.kind,
      entries: records,
      metric_verdicts: records.length === 1 ? records[0].metric_verdicts : {},
      metric_failures: records.flatMap((record) => record.metric_failures),
      verdict,
      loss_owners: records.flatMap((record) => record.loss_owners),
    };
  });
  const verdicts = cells.map((cell) => cell.verdict);
  const allowedUncovered = new Set(manifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS);
  const metricVerdicts = cells.flatMap((cell) => Object.values(cell.metric_verdicts));
  return {
    contract: "gauntlet-scoreboard-v1",
    primary_metric_by_mode: MODE_PRIMARY_METRIC,
    verdict_policy: { rust: RATIO_VERDICTS.rust, non_rust: RATIO_VERDICTS.non_rust, unmeasured: "missing row, tier, metric, or byte verification" },
    validation_issues: [...new Set(resultValidationIssues)],
    cells,
    summary: {
      cells: cells.length,
      win: verdicts.filter((verdict) => verdict === "win").length,
      parity: verdicts.filter((verdict) => verdict === "parity").length,
      loss: verdicts.filter((verdict) => verdict === "loss").length,
      unmeasured: verdicts.filter((verdict) => verdict === "unmeasured").length,
      unmeasured_allowed: cells.filter((cell) => cell.verdict === "unmeasured" && allowedUncovered.has(cell.id)).length,
      unmeasured_required: cells.filter((cell) => cell.verdict === "unmeasured" && !allowedUncovered.has(cell.id)).length,
      metric_win: metricVerdicts.filter((verdict) => verdict === "win").length,
      metric_parity: metricVerdicts.filter((verdict) => verdict === "parity").length,
      metric_loss: metricVerdicts.filter((verdict) => verdict === "loss").length,
      metric_unmeasured: metricVerdicts.filter((verdict) => verdict === "unmeasured").length,
      metric_not_applicable: metricVerdicts.filter((verdict) => verdict === "not_applicable").length,
    },
    loss_owners: {
      declared: declaredOwners,
      tower: towerState.status === "available" ? { status: towerState.status, revision: towerState.revision } : { status: towerState.status, reason: towerState.reason },
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
    if (versionResult.code !== 0) {
      return {
        tool,
        status: "probe_failed",
        resolved,
        reason: `${tool} version probe failed (exit ${versionResult.code})`,
        version: versionOutput.split(/\r?\n/, 1)[0].slice(0, 300),
        exit_code: versionResult.code,
        stderr: versionResult.stderr.toString("utf8").trim().slice(0, AXIS_OUTPUT_LIMIT),
      };
    }
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

async function runAxes(manifest, runDir, jetBin, fullScope, runId, selectedAxis = null) {
  const contracts = manifest?.axes ?? {};
  const axes = {};
  for (const [id, axis] of Object.entries(contracts)) {
    if (selectedAxis !== null && id !== selectedAxis) continue;
    if (!fullScope && selectedAxis === null) {
      axes[id] = unmeasuredAxis(id, axis, "axis measurements require the full corpus scope");
      continue;
    }
    try {
      if (id === "live_reload") axes[id] = await runLiveReloadAxisAdapter(axis, runDir, jetBin, { envRunner, envRunnerArgs: ["full"] });
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
function publicationState({ fullScope, loaded, skipped, matrix, manifest, sourceMeasurements, results, scoreboard, axes, validationIssues, axisId = null }) {
  const loadedEntries = Array.isArray(loaded) ? loaded : [];
  const skippedEntries = Array.isArray(skipped) ? skipped : [];
  const measuredResults = Array.isArray(results) ? results : [];
  const matrixCells = Array.isArray(matrix?.cells) ? matrix.cells : [];
  const scoreboardCells = Array.isArray(scoreboard?.cells) ? scoreboard.cells : [];
  const summary = scoreboard?.summary && typeof scoreboard.summary === "object" ? scoreboard.summary : {};
  if (axisId !== null) {
    const axis = axes?.[axisId];
    const axisBlockers = Array.isArray(validationIssues) ? [...validationIssues] : [];
    if (!axis) {
      axisBlockers.push(`${axisId} axis is missing`);
    } else {
      if (axis.status !== "complete") axisBlockers.push(`${axisId} axis is ${axis.status ?? "unreported"}`);
      if (axis.publication?.status !== "ready") {
        axisBlockers.push(`${axisId} axis publication is ${axis.publication?.status ?? "unreported"}`);
      }
      axisBlockers.push(...(axis.publication?.blockers ?? []));
    }
    const uniqueAxisBlockers = [...new Set(axisBlockers.filter((blocker) => typeof blocker === "string" && blocker.length > 0))];
    return {
      scope: `axis_${axisId}`,
      status: uniqueAxisBlockers.length ? "incomplete" : "complete",
      complete: uniqueAxisBlockers.length === 0,
      blockers: uniqueAxisBlockers,
      allowed_uncovered_cells: [],
    };
  }
  const blockers = Array.isArray(validationIssues) ? [...validationIssues] : [];
  blockers.push(...measuredResults.flatMap((result) => validateResultShape(result, matrix)));
  blockers.push(...(Array.isArray(scoreboard?.validation_issues) ? scoreboard.validation_issues : []));
  if (!fullScope) blockers.push("run scope is partial; full matrix publication requires no --entry");
  if (skippedEntries.length) blockers.push(`skipped entries: ${skippedEntries.map((item) => item.name).join(", ")}`);
  if (!sourceMeasurements) {
    blockers.push("source measurements are missing");
  } else if (manifest && !sourceMeasurements.coverage?.denominator_pass) {
    blockers.push("source measurement denominator is incomplete");
  }

  const expectedResultNames = loadedEntries.map((item) => item.directoryName ?? item.entry?.name ?? null);
  const actualResultNames = measuredResults.map((result) => result?.entry?.name ?? null);
  if (measuredResults.length !== loadedEntries.length) {
    blockers.push(`result denominator is ${measuredResults.length}/${loadedEntries.length}`);
  }
  if (new Set(actualResultNames).size !== actualResultNames.length) {
    blockers.push("result entry identities are duplicated");
  }
  if (expectedResultNames.length !== actualResultNames.length ||
    expectedResultNames.some((name, index) => name !== actualResultNames[index])) {
    blockers.push("result entry identities do not match the loaded corpus order");
  }
  const matrixIds = new Set(matrixCells.map((cell) => cell.id));
  if (scoreboardCells.length !== matrixCells.length) {
    blockers.push(`scoreboard cell denominator is ${scoreboardCells.length}/${matrixCells.length}`);
  }
  for (const cell of scoreboardCells) {
    if (!matrixIds.has(cell.id)) blockers.push(`scoreboard declares unknown matrix cell ${cell.id}`);
  }
  const requiredAxisIds = Object.entries(manifest?.axes ?? {})
    .filter(([, axis]) => axis?.status === "required")
    .map(([id]) => id);
  const allowedCells = manifest?.corpus?.allowed_uncovered_cells;
  const allowed = new Set(Array.isArray(allowedCells) ? allowedCells : MATRIX_UNCOVERED_DEFAULTS);

  const scoreboardByCell = new Map(scoreboardCells.map((cell) => [cell.id, cell]));
  for (const cell of matrixCells) {
    const record = scoreboardByCell.get(cell.id);
    const candidates = record?.entries ?? [];
    if (candidates.length !== 1 && !allowed.has(cell.id)) {
      blockers.push(`${cell.id}: expected exactly one published entry, found ${candidates.length}`);
    }
    for (const failure of record?.metric_failures ?? []) {
      const reasons = failure.reasons?.length ? ` (${failure.reasons.join("; ")})` : "";
      blockers.push(`${cell.id}/${failure.metric}: metric ${failure.verdict}${reasons}`);
    }
  }
  const covered = new Set(measuredResults.flatMap((result) => Array.isArray(result?.entry?.cells) ? result.entry.cells : []));
  const unexpectedUncovered = matrixCells.map((cell) => cell.id).filter((id) => !covered.has(id) && !allowed.has(id));
  if (unexpectedUncovered.length) blockers.push(`unexpected uncovered matrix cells: ${unexpectedUncovered.join(", ")}`);
  const expectedEntryCount = manifest?.corpus?.entry_count ?? loadedEntries.length;
  if (fullScope && loadedEntries.length !== expectedEntryCount) blockers.push(`entry denominator is ${loadedEntries.length}/${expectedEntryCount}`);
  if (summary.unmeasured_required > 0) blockers.push(`${summary.unmeasured_required} required matrix cells are unmeasured`);
  if (summary.metric_unmeasured > 0) blockers.push(`${summary.metric_unmeasured} required metric cells are unmeasured`);

  const axisResults = axes && typeof axes === "object" ? axes : {};
  for (const id of requiredAxisIds) {
    if (!Object.hasOwn(axisResults, id)) blockers.push(`${id} axis is missing`);
  }
  for (const [id, axis] of Object.entries(axisResults)) {
    const axisRecord = axis && typeof axis === "object" ? axis : {};
    const required = axisRecord.required === true || requiredAxisIds.includes(id);
    if (required && axisRecord.status !== "complete") blockers.push(`${id} axis is ${axisRecord.status ?? "unreported"}`);
    if (required && axisRecord.publication?.status !== "ready") {
      blockers.push(`${id} axis publication is ${axisRecord.publication?.status ?? "unreported"}`);
    }
    for (const blocker of axisRecord.publication?.blockers ?? []) blockers.push(`${id}: ${blocker}`);
  }
  for (const owner of scoreboard?.loss_owners?.unresolved ?? []) {
    blockers.push(`${owner.entry}/${owner.peer ?? "peer"}/${owner.metric ?? "metric"} (${owner.category ?? "unknown category"}): loss owner ${owner.card ?? "is not declared"} is not live`);
  }
  const uniqueBlockers = [...new Set(blockers.filter((blocker) => typeof blocker === "string" && blocker.length > 0))];
  return {
    scope: fullScope ? "full_matrix" : "partial_entry",
    status: uniqueBlockers.length ? "incomplete" : "complete",
    complete: uniqueBlockers.length === 0,
    blockers: uniqueBlockers,
    allowed_uncovered_cells: [...allowed].sort((left, right) => left.localeCompare(right)),
  };
}

// The run and dev tiers execute inside the compiler process (Cranelift hosts,
// interpreter ambient), so a debug-profile compiler measures unoptimized
// Prelude hosts rather than Jet. The default binary is therefore the release
// build; a missing one is an error, never a silent fallback to target/debug.
async function copyJetBinary(options, runDir) {
  if (options.jetBin) return path.resolve(process.cwd(), options.jetBin);
  const source = path.join(repoDir, "target/release/jet");
  if (!(await exists(source))) {
    throw new Error(`missing default Jet binary: ${source} (build it with \`scripts/agent/jet-env cargo build --release --bin jet\` or pass --jet-bin)`);
  }
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
  const axisOnly = options.axis !== null;
  const fullScope = !axisOnly && entriesDir === defaultEntriesDir && options.entry === null;
  const manifestPath = path.join(repoDir, "gauntlet/measurement-manifest.json");
  const sourceManifest = entriesDir === defaultEntriesDir
    ? JSON.parse(await fs.readFile(manifestPath, "utf8"))
    : null;
  const sourceMeasurements = !axisOnly && sourceManifest ? await measureSourceManifest(entriesDir, sourceManifest, matrix) : null;
  const runId = `${dateStamp().replaceAll("-", "")}-${process.pid}-${Date.now().toString(36)}`;
  const runDir = path.join(process.env.HOME ?? ".", ".cache/jet-gauntlet/work", runId);
  await fs.mkdir(runDir, { recursive: true });
  const jetBin = await copyJetBinary(options, runDir);
  const toolchains = await toolchainFingerprint(runDir, jetBin);
  const dev = await devAvailable(jetBin, runDir);
  if (!dev) console.warn("WARN jet dev unavailable; skipping Jet dev tier");
  const { loaded, skipped } = axisOnly
    ? { loaded: [], skipped: [] }
    : await loadEntries(entriesDir, options.entry);
  const validationIssues = axisOnly
    ? []
    : await validateCorpus(entriesDir, loaded, skipped, matrix, sourceManifest, fullScope);
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
  const covered = axisOnly ? new Set() : new Set(results.flatMap((result) => result.entry.cells ?? []));
  const uncovered = axisOnly ? [] : (matrix.cells ?? []).map((cell) => cell.id).filter((id) => !covered.has(id));
  const tower = axisOnly ? null : await readLiveTowerCards();
  const scoreboard = axisOnly
    ? { primary_metric_by_mode: MODE_PRIMARY_METRIC, verdict_policy: RATIO_VERDICTS, summary: {}, cells: [] }
    : buildScoreboard(matrix, results, sourceManifest, tower);
  const axes = await runAxes(sourceManifest, runDir, jetBin, fullScope || axisOnly, runId, options.axis);
  const publication = publicationState({ fullScope, loaded, skipped, matrix, manifest: sourceManifest, sourceMeasurements, results, scoreboard, axes, validationIssues, axisId: options.axis });
  const expectedEntryNames = sourceManifest?.corpus?.entry_names ?? [];
  const allowedUncovered = axisOnly ? new Set() : new Set(sourceManifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS);
  const unexpectedUncovered = uncovered.filter((cell) => !allowedUncovered.has(cell));
  const denominator = {
    entry_names_pass: axisOnly || (fullScope && skipped.length === 0 && loaded.length === expectedEntryNames.length && equalStringArrays(loaded.map((item) => item.directoryName ?? path.basename(item.dir)), expectedEntryNames)),
    matrix_coverage_pass: axisOnly || unexpectedUncovered.length === 0,
    source_pairs_pass: axisOnly || (sourceMeasurements?.coverage?.denominator_pass ?? false),
  };
  denominator.pass = denominator.entry_names_pass && denominator.matrix_coverage_pass && denominator.source_pairs_pass;
  const report = {
    contract: "gauntlet-report-v1",
    generated: new Date().toISOString(),
    run_id: runId,
    options: { entry: options.entry, axis: options.axis, jet_bin: jetBin, runs: options.runs, scope: axisOnly ? `axis_${options.axis}` : (fullScope ? "full_matrix" : "partial_entry") },
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
      ratio_verdicts: RATIO_VERDICTS,
      metric_applicability: METRIC_APPLICABILITY_POLICY,
      peer_measurement: PEER_MEASUREMENT_POLICY,
      missing_metric_verdict: "unmeasured",
      run_count: options.runs ?? "entry perf policy (7 for perf, 3 otherwise)",
    },
    entries: results,
  };
  // One file per run. Full-matrix runs own the bare <date>.json; partial and
  // axis runs carry their scope so a later partial run never overwrites the
  // day's full report (status.mjs merges every file in this directory).
  const resultDir = path.join(repoDir, "gauntlet/results");
  await fs.mkdir(resultDir, { recursive: true });
  const resultName = axisOnly
    ? `${dateStamp()}-axis-${options.axis}.json`
    : (fullScope ? `${dateStamp()}.json` : `${dateStamp()}-${options.entry}.json`);
  const resultPath = path.join(resultDir, resultName);
  await fs.writeFile(resultPath, `${JSON.stringify(report, null, 2)}\n`);
  const statusPath = path.join(repoDir, "gauntlet/status.json");
  if (fullScope || axisOnly) {
    const status = projectStatus(report, resultPath);
    await fs.writeFile(statusPath, `${JSON.stringify(status, null, 2)}\n`);
    console.log(`status\t${statusPath}`);
  } else {
    console.log(`status\tskipped partial scope; ${statusPath} was not overwritten`);
  }

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
    if (id === options.axis) {
      console.log("axis-rival\tcold_reload_ms\twarm_reload_ms\tverdict");
      for (const [rival, comparison] of Object.entries(axis.comparisons ?? {})) {
        console.log(`${rival}\t${comparison.cold?.jet ?? "-"}:${comparison.cold?.peer ?? "-"}\t${comparison.warm?.jet ?? "-"}:${comparison.warm?.peer ?? "-"}\t${comparison.verdict ?? "-"}`);
      }
    }
  }
  if (!publication.complete) process.exitCode = 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`harness: ${error.message}`);
    process.exitCode = 1;
  });
}

export {
  comparisons,
  ratioVerdict,
  metricApplicability,
  validateEntryShape,
  validateResultShape,
  buildScoreboard,
  publicationState,
  processTreeRssKb,
  httpProbe,
  probeMatches,
  collectTierTrace,
  collectServiceTierTrace,
};
