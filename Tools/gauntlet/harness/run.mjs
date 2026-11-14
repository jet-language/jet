#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { killProcessTree, processesEnded, processTreePids, processTreeRssKb, runLiveReloadAxis as runLiveReloadAxisAdapter, signalProcessTree } from "./live-reload.mjs";
import { runMemorySafetyFuzzAxis as runMemorySafetyFuzzAxisAdapter } from "./memory-safety-fuzz.mjs";
import { compileThroughputContractIssues, runCompileThroughputAxisAdapter } from "./compile-throughput.mjs";
import { buildSystemContractIssues, runBuildSystemAxisAdapter } from "./build-system.mjs";
import { applyStatusGate, projectStatus, rebuildStatusFromResults } from "./status.mjs";
import { buildStandings, ceilingMeasurements as standingsCeilingMeasurements, entryStandings, planCells, planEntry, PEER_KINDS, standingsLines, standingsPolicy } from "./standings.mjs";
import { stateRoot } from "../../agent/roots.mjs";
const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../../..");
const envRunner = path.join(repoDir, "Tools/agent/JetEnv.jet");
const timer = path.join(harnessDir, "timer.py");
const launcherSource = path.join(harnessDir, "launch.c");
const refBuild = path.join(repoDir, "Tools/perf/asm-parity/ref-build.mjs");

const TOOL_VERSION_COMMANDS = Object.freeze({
  python3: ["python3", "--version"],
  lua: ["lua", "-v"],
  luajit: ["luajit", "-v"],
  rustc: ["rustc", "--version"],
  gcc: ["gcc", "--version"],
  zig: ["zig", "version"],
  go: ["go", "version"],
  node: ["node", "--version"],
  bun: ["bun", "--version"],
  vite: ["vite", "--version"],
  as: ["as", "--version"],
  perf: ["perf", "--version"],
  strip: ["strip", "--version"],
  ldd: ["ldd", "--version"],
  entr: ["entr", "-V"],
});

function toolVersionCommand(tool) {
  return [...(TOOL_VERSION_COMMANDS[tool] ?? [tool, "--version"])];
}

const ENTRY_MODES = ["batch", "batch-steps", "service", "web", "web-app"];
// A tool entry pairs a repository script with its Jet twin (AGENTS.md,
// Tooling in other languages); runToolEntry runs its parity cases.
const TOOL_MODE = "tool";
const TOOL_INTERPRETERS = Object.freeze({
  ".mjs": ["node"],
  ".js": ["node"],
  ".cjs": ["node"],
  ".ts": ["node"],
  ".py": ["python3"],
  ".sh": ["bash"],
  ".bash": ["bash"],
});
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
const WEB_WASM_LANGUAGES = new Set(["jet", "rust"]);
const WEB_ARTIFACT_ASSET_PATTERN = /\.(?:js|mjs|wasm)$/i;
const TIMER_SCHEMA = "jet.gauntlet.timer.v2";
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
// One comparator for every peer, Rust included: Jet/peer below 1 wins, anything else loses.
const RATIO_VERDICTS = { win: "<1", loss: ">=1" };
// The AGENTS.md Performance gate paragraphs the gate encodes, word for word.
const STRICT_PERFORMANCE_POLICY_TEXT = [
  "Jet's performance target has two stages. The first stage is the peer floor, and it is a hard, structural requirement: a Jet program must be faster and use less peak memory than the same program in every other language. Jet/peer must be below `1.00` for every matched peer, for each required cell and for both wall time and peak memory. Rust is judged like every other peer, with no parity band. A loss to any peer on either metric is a codegen, runtime or Core failure. It is fixed generally and structurally, so that any Jet program beats the other languages, never only the measured one. The second and final stage is the machine ceiling: hand-written, maximally optimized machine code for the same workload, which Jet must match or beat in both cycles and bytes (Jet/ceiling at most `1.00`). Jet aims as close to the ceiling as it can. Closing the remaining ceiling gap is the hill-climbing stage: until the owner opens it, a cell passes the gate on the peer floor, and its ceiling ratio is measured and reported but does not fail the gate.",
  "",
  "Every required cell needs a ceiling reference: an asm-parity cell (Tools/perf/asm-parity) that names its gauntlet cell and does the work with the best known technique for the workload (SIMD, algorithmic, or syscall-level), not a naive transliteration. A cell without a reference is uncovered and fails the gate once hill climbing opens the ceiling stage. A better technique replaces the reference and raises the target.",
  "",
  "Required cells cover the foundations (numerics, text, files, concurrency, networking, build time, and run time) plus one real workload in each critical area: web, games, CLI and scripts, data analysis, backend services, GUI applications, and embedded. A niche becomes required only when Jet ships a first-party battery for it; never claim a niche win without one.",
  "",
  "Apply the comparator separately to each cell and metric. Never average away a loss, substitute an easier workload or tier, drop a peer, or accept wrong, unavailable, uncovered, mismatched, or inconclusive evidence. A performance card, milestone, dashboard, or release gate stays open while any required cell fails. Never trade semantics, diagnostics, determinism, safety, or I9 parity for a score. Manifests and gates must encode this policy; prose alone is not evidence. Historical receipts stay immutable under the policy they were recorded with and never weaken the current gate.",
].join("\n");

// The target has two stages. `peer_floor` is the per-cell, per-metric strict win
// over every matched peer (RATIO_VERDICTS); it alone passes or fails a cell.
// `machine_ceiling` is the final one: Jet against a hand-written, maximally
// optimized machine-code reference (an asm-parity cell,
// Tools/perf/asm-parity/cells.json + <cell>/ref.S, whose `gauntlet` field names
// the matrix cells it bounds), Jet/ceiling <= 1.00 on every runtime and size
// metric. Until the owner opens hill climbing, the ceiling ratio is measured
// and reported (`ceiling_gate`) and fails no cell. `metrics` maps a metric's
// loss-owner category (LOSS_OWNER_CATEGORY_BY_METRIC) to the asm-parity
// measure that judges it.
const CEILING_VERDICTS = Object.freeze({ meet: "<=1", miss: ">1" });
const CEILING_POLICY = Object.freeze({
  stages: Object.freeze([
    { id: "peer_floor", role: "initial", against: "every_matched_peer", comparators: RATIO_VERDICTS },
    { id: "machine_ceiling", role: "final", against: "hand_written_machine_code", comparators: CEILING_VERDICTS },
  ]),
  closes_cell: "peer_floor",
  peer_win_closes_cell: true,
  ceiling_gate: "reported_until_hill_climbing_opens",
  required_cells: "every_required_cell",
  metrics: Object.freeze({ runtime: "cycles", latency: "cycles", binary: "bytes" }),
  references: "Tools/perf/asm-parity/cells.json",
  reference_source: "ref.S",
  reference_link: "gauntlet",
  missing_reference: "uncovered",
  reference_technique: "best_known_for_the_workload",
  better_technique: "replaces_reference_and_raises_target",
});
// Both peer-floor metrics: every matched peer and Jet must carry a reading.
const PEER_FLOOR_METRICS = Object.freeze(["runtime_wall_seconds", "runtime_peak_rss_kb"]);

const STRICT_PERFORMANCE_POLICY = Object.freeze({
  schema: "jet.strict-performance-policy.v3",
  id: "AGENTS.md#strict-performance-gate",
  version: 3,
  comparison: "per_cell_and_metric",
  comparators: RATIO_VERDICTS,
  failure_statuses: Object.freeze([
    "missing",
    "wrong",
    "unavailable",
    "uncovered",
    "mismatched",
    "inconclusive",
  ]),
  foundations: Object.freeze([
    { id: "numerics", required: true, cells: ["numerics.float-kernel", "numerics.fft", "numerics.tensor-map", "numerics.int-kernel"], metrics: "all" },
    { id: "text", required: true, cells: ["text.kernel", "text.regex-kernel", "text.regex-find-all-large", "text.report-cli", "text.script"], metrics: "all" },
    { id: "files", required: true, cells: ["files.script", "files.orchestration"], metrics: "all" },
    { id: "concurrency", required: true, cells: ["concurrency.app", "concurrency.service"], metrics: "all" },
    { id: "networking", required: true, cells: ["netserv.client", "netserv.service"], metrics: "all" },
    { id: "build_time", required: true, cells: "all", metrics: ["cold_build_seconds", "warm_build_seconds"] },
    { id: "run_time", required: true, cells: "all", metrics: "runtime" },
  ]),
  critical_areas: Object.freeze([
    { id: "web", required: true, cells: ["webfront.widget", "webfront.app"], metrics: "all" },
    { id: "games", required: false, activation: "first_party_battery", cells: [], metrics: "all" },
    { id: "cli_and_scripts", required: true, cells: ["cli.app", "cli.hello", "text.report-cli", "text.script", "formats.csv-cli", "files.script"], metrics: "all" },
    { id: "data_analysis", required: true, cells: ["formats.csv-cli", "numerics.script", "numerics.notebook"], metrics: "all" },
    { id: "backend_services", required: true, cells: ["concurrency.service", "netserv.service"], metrics: "all" },
    { id: "ai_ml_applications", required: false, activation: "first_party_battery", cells: [], metrics: "all" },
    { id: "gui_applications", required: false, activation: "first_party_battery", cells: [], metrics: "all" },
    { id: "embedded", required: true, cells: ["embedded.kernel", "embedded.data"], metrics: "all" },
  ]),
  first_party_battery_required: true,
  historical_receipts: "immutable_recorded_policy",
  ceiling: CEILING_POLICY,
  text: STRICT_PERFORMANCE_POLICY_TEXT,
});
const METRIC_APPLICABILITY_POLICY = {
  default: "required",
  not_applicable: "explicit_structural_reason",
  missing: "unmeasured_and_publication_blocked",
};
const STRUCTURAL_NOT_APPLICABLE_BASES = new Set(["no_compile_phase", "no_wasm_artifact"]);
// The run tier times `jet run` from launch to answer, and its ratio peers are
// the interpreters that answer a script the same way, each through its own
// command (runCommand). A compiled peer's row is a prebuilt binary, so compiled
// peers are judged on the AOT tier, whose metrics include the build times, and
// never on the run tier. The run tier is timed in each cache state: `edited`
// (the script changed since its last run, Jet's shipped libraries prebuilt) and
// `repeat` (nothing changed) gate the cell; `fresh_install` (the first run with
// an empty cache) is reported only. On an entry that names a `run_reference`,
// a minimal hand-written program for the same answer, Jet's run tier must also
// finish within `run_reference_margin_seconds` of that program in every gated
// cache state.
const RUN_TIER_PEERS = Object.freeze(["lua", "luajit", "python", "node"]);
const RUN_CACHE_STATES = Object.freeze({ gated: Object.freeze(["edited", "repeat"]), reported: Object.freeze(["fresh_install"]) });
const PEER_MEASUREMENT_POLICY = {
  ratio_tiers: ["aot", "run"],
  trace_only_tiers: [],
  sample_binding: "immutable_peer_row_reused_per_declared_jet_tier",
  run_tier_peers: RUN_TIER_PEERS,
  compiled_peer_tiers: ["aot"],
  run_cache_states: RUN_CACHE_STATES,
  run_reference_margin_seconds: 0.0005,
};

// The tiers a peer is compared on: the run-tier peers on every declared tier,
// a compiled peer or an expert pair on the AOT tier only.
function peerTierPolicy(entry, language) {
  const policy = tierPolicy(entry.mode);
  if (!language.endsWith("-expert") && RUN_TIER_PEERS.includes(language)) return policy;
  return Object.fromEntries(Object.entries(policy).filter(([tier]) => PEER_MEASUREMENT_POLICY.compiled_peer_tiers.includes(tier)));
}
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
  artifact_bytes: "binary",
  wasm_bytes: "binary",
  loc: "source",
  source_bytes: "source",
  tokens: "source",
  source_tokens: "source",
};
const AOT_ONLY_METRICS = new Set(["cold_build_seconds", "warm_build_seconds", "binary_bytes", "artifact_bytes", "wasm_bytes"]);


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
  lua: "main.lua",
  luajit: "main.lua",
  bun: "main.mjs",
  vite: "main.mjs",
  asm: "ref.S",
};
// LuaJIT runs the entry's Lua program, Bun the Node program and Vite bundles
// the web (js) program; the asm ceiling's directory is its asm-parity
// reference. Every other language has its own directory.
const LANGUAGE_SOURCE_DIRS = { luajit: "lua", bun: "node", vite: "js" };

function baseLanguage(language) {
  return language.endsWith("-expert") ? language.slice(0, -"-expert".length) : language;
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function parseArgs(argv) {
  const options = { runs: null, entry: null, jetBin: null, entriesDir: null, axis: null, ceilingResults: null, languages: null, merge: null, dryRun: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--entry" || arg === "--jet-bin" || arg === "--runs" || arg === "--entries-dir" || arg === "--axis" || arg === "--ceiling-results" || arg === "--languages" || arg === "--merge") {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      const value = argv[++i];
      if (arg === "--entry") options.entry = value;
      if (arg === "--jet-bin") options.jetBin = value;
      if (arg === "--entries-dir") options.entriesDir = value;
      if (arg === "--runs") options.runs = Number.parseInt(value, 10);
      if (arg === "--axis") options.axis = value;
      if (arg === "--ceiling-results") options.ceilingResults = value;
      if (arg === "--languages") options.languages = sortedUnique(value.split(",").map((language) => language.trim()).filter(Boolean));
      if (arg === "--merge") options.merge = value;
      continue;
    }
    if (arg === "--dry-run") {
      options.dryRun = true;
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: node Tools/gauntlet/harness/run.mjs [--dry-run] [--entry name] [--jet-bin path] [--runs n] [--entries-dir path] [--axis live_reload|memory_safety_fuzz|compile_throughput] [--ceiling-results asm-parity/results.json] [--languages jet,c,... [--merge earlier-results.json]]");
      process.exit(0);
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (options.runs !== null && (!Number.isInteger(options.runs) || options.runs < 1)) {
    throw new Error("--runs must be a positive integer");
  }
  if (options.axis !== null && !["live_reload", "memory_safety_fuzz", "compile_throughput"].includes(options.axis)) {
    throw new Error("--axis must be live_reload, memory_safety_fuzz or compile_throughput");
  }
  if (options.languages !== null) {
    if (options.languages.length === 0) throw new Error("--languages needs at least one language");
    const unknown = options.languages.filter((language) => !LANGUAGE_FILES[baseLanguage(language)]);
    if (unknown.length) throw new Error(`--languages names unknown languages: ${unknown.join(", ")}`);
    if (options.axis !== null) throw new Error("--languages selects entry rows; an --axis run has none");
  }
  if (options.merge !== null && options.languages === null) throw new Error("--merge needs --languages: it supplies the rows this run does not measure");
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

async function runProcess(cwd, args, {
  input = undefined,
  full = false,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  resourceBudget = null,
  env = undefined,
} = {}) {
  return new Promise((resolve) => {
    const child = spawn(envRunner, [...(full ? ["full"] : []), ...args], {
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
      killProcessTree(child);
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
  if (sample.schema !== TIMER_SCHEMA) {
    sample.error = sample.error ?? `timer schema is ${JSON.stringify(sample.schema)}; expected ${TIMER_SCHEMA}`;
    sample.exit_code = sample.exit_code ?? result.code;
  }
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
  if (sample.schema !== TIMER_SCHEMA) {
    sample.error = sample.error ?? `timer schema is ${JSON.stringify(sample.schema)}; expected ${TIMER_SCHEMA}`;
    sample.exit_code = sample.exit_code ?? result.code;
  }
  return sample;
}

// Builds launch.c, the launcher every measured command runs through (the
// timer's commands and each service), into `runDir` once per run, and names
// it to the timers through JET_GAUNTLET_LAUNCHER.
async function buildLauncher(runDir) {
  const built = path.join(runDir, "launch");
  const result = await runProcess(runDir, ["cc", "-O2", "-o", built, launcherSource]);
  if (result.code !== 0) throw new Error(`cannot build the launcher from ${launcherSource}: ${result.stderr.toString("utf8").trim().slice(0, 300)}`);
  process.env.JET_GAUNTLET_LAUNCHER = built;
}

// Starts `args` in the dev shell. With a `report`, it runs through the
// launcher, which writes it (see launch.c): the start line before the
// command's exec, the end line, with its peak memory, once it has exited.
function startProcess(cwd, args, { full = false, env = undefined, report = null } = {}) {
  const launched = report === null ? args : [process.env.JET_GAUNTLET_LAUNCHER, report, ...args];
  const child = spawn(envRunner, [...(full ? ["full"] : []), ...launched], {
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

// The fields of a launcher report's `kind` line ("start" or "end") as
// numbers, or null while the report lacks it.
async function launchReport(report, kind) {
  const text = await fs.readFile(report, "utf8").catch(() => "");
  const line = text.split("\n").find((row) => row.startsWith(`${kind} `));
  return line === undefined ? null : line.split(" ").slice(1).map(Number);
}

// Stops `child` and every process of its tree: SIGTERM to each, then SIGKILL to
// the ones still running after graceMs. Resolves to the child's exit and
// whether the kill was forced.
async function stopProcess(child, graceMs = 5000) {
  if (child.exitCode !== null || child.signalCode !== null) return { exit: { code: child.exitCode, signal: child.signalCode }, forced: false };
  const pids = await processTreePids(child.pid);
  signalProcessTree(pids, "SIGTERM");
  let forced = false;
  if (!(await processesEnded(pids, graceMs))) {
    forced = true;
    signalProcessTree(pids, "SIGKILL");
    await processesEnded(pids, 1000);
  }
  return { exit: await waitForExit(child, 1000), forced };
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
function percentile(values, quantile) {
  const numbers = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!numbers.length) return null;
  const index = Math.min(numbers.length - 1, Math.max(0, Math.ceil(quantile * numbers.length) - 1));
  return numbers[index];
}
function finiteValues(samples, field) {
  return (Array.isArray(samples) ? samples : [])
    .map((sample) => sample?.[field])
    .filter((value) => Number.isFinite(value));
}

function noiseSummary(values) {
  const finite = values.filter((value) => Number.isFinite(value)).sort((left, right) => left - right);
  if (!finite.length) return { count: 0, min: null, max: null, range: null, relative_range: null };
  const middle = median(finite);
  const min = finite[0];
  const max = finite.at(-1);
  return {
    count: finite.length,
    min,
    max,
    range: max - min,
    relative_range: middle > 0 ? (max - min) / middle : null,
  };
}

function summarizeSamples(samples) {
  const metrics = ["wall_seconds", "peak_rss_kb", "time_to_first_stdout_seconds"];
  const rawMetrics = ["wall_time_ns", "time_to_first_stdout_ns"];
  const valid = Array.isArray(samples) && samples.length > 0 &&
    samples.every((sample) => sample && sample.schema === TIMER_SCHEMA && sample.exit_code === 0 &&
      metrics.every((metric) => positiveMetricValue(metric, sample[metric])) &&
      rawMetrics.every((metric) => positiveMetricValue(metric, sample[metric])));
  const medians = Object.fromEntries(metrics.map((metric) => [
    metric,
    valid ? median(samples.map((sample) => sample[metric])) : null,
  ]));
  const rawMedians = Object.fromEntries(rawMetrics.map((metric) => [
    metric,
    valid ? median(samples.map((sample) => sample[metric])) : null,
  ]));
  const noise = Object.fromEntries(metrics.concat(rawMetrics).map((metric) => [metric, noiseSummary(finiteValues(samples, metric))]));
  return {
    samples,
    valid,
    protocol: TIMER_SCHEMA,
    raw_sample_fields: rawMetrics,
    median: medians,
    median_raw: rawMedians,
    noise: {
      method: "sample_range_over_median",
      metrics: noise,
    },
    confidence: {
      status: "not_computed",
      method: "paired_confidence_required_for_strict_surface_claims",
      level: null,
      reason: "These are independent process samples; no confidence interval is inferred from a median.",
    },
  };
}

function validBuildSample(sample) {
  return sample?.schema === TIMER_SCHEMA &&
    sample?.exit_code === 0 &&
    positiveMetricValue("wall_seconds", sample.wall_seconds) &&
    positiveMetricValue("wall_time_ns", sample.wall_time_ns) &&
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
function policyTerritoryCells(area, matrixCellIds) {
  if (area?.cells === "all") return [...matrixCellIds];
  return Array.isArray(area?.cells) ? area.cells : [];
}

function strictPerformancePolicyIssues(matrix) {
  const policy = matrix?.strict_performance_policy;
  if (!isObject(policy)) return ["matrix is missing the strict performance policy"];
  const matrixCells = Array.isArray(matrix?.cells) ? matrix.cells : [];
  const issues = [];
  if (policy.schema !== STRICT_PERFORMANCE_POLICY.schema) {
    issues.push(`matrix strict performance policy schema is ${JSON.stringify(policy.schema)}`);
  }
  if (policy.id !== STRICT_PERFORMANCE_POLICY.id || policy.version !== STRICT_PERFORMANCE_POLICY.version) {
    issues.push("matrix strict performance policy identity is not ratified");
  }
  if (policy.comparison !== STRICT_PERFORMANCE_POLICY.comparison) {
    issues.push("matrix strict performance policy is not per-cell and per-metric");
  }
  if (JSON.stringify(policy.comparators) !== JSON.stringify(STRICT_PERFORMANCE_POLICY.comparators)) {
    issues.push("matrix strict performance policy comparator differs from the ratified law");
  }
  if (!equalStringArrays(policy.failure_statuses, STRICT_PERFORMANCE_POLICY.failure_statuses)) {
    issues.push("matrix strict performance policy failure statuses are incomplete");
  }
  for (const category of ["foundations", "critical_areas"]) {
    if (!Array.isArray(policy[category])) {
      issues.push(`matrix strict performance policy ${category} are missing`);
      continue;
    }
    const ids = new Set();
    for (const area of policy[category]) {
      if (!isObject(area) || typeof area.id !== "string" || area.id.length === 0) {
        issues.push(`matrix strict performance policy has an invalid ${category} area`);
        continue;
      }
      if (ids.has(area.id)) issues.push(`matrix strict performance policy duplicates ${category} area ${area.id}`);
      ids.add(area.id);
      if (area.required !== true && area.activation !== "first_party_battery") {
        issues.push(`${category}.${area.id}: inactive territory must require a first-party battery activation`);
      }
      const cells = policyTerritoryCells(area, matrixCells.map((cell) => cell.id));
      if (area.required === true && cells.length === 0) {
        issues.push(`${category}.${area.id}: required territory has no first-party battery cells`);
      }
      if (area.required !== true && cells.length > 0) {
        issues.push(`${category}.${area.id}: inactive territory cannot claim battery cells`);
      }
      for (const cell of cells) {
        if (typeof cell !== "string" || !matrixCells.some((candidate) => candidate.id === cell)) {
          issues.push(`${category}.${area.id}: unknown or invalid matrix battery cell ${cell}`);
        }
      }
    }
  }
  if (JSON.stringify(policy.foundations) !== JSON.stringify(STRICT_PERFORMANCE_POLICY.foundations)) {
    issues.push("matrix strict performance policy foundation battery map differs from the ratified law");
  }
  if (JSON.stringify(policy.critical_areas) !== JSON.stringify(STRICT_PERFORMANCE_POLICY.critical_areas)) {
    issues.push("matrix strict performance policy critical-area battery map differs from the ratified law");
  }
  if (policy.first_party_battery_required !== true) {
    issues.push("matrix strict performance policy does not require a first-party battery before a niche claim");
  }
  if (policy.historical_receipts !== STRICT_PERFORMANCE_POLICY.historical_receipts) {
    issues.push("matrix strict performance policy does not preserve historical receipts");
  }
  if (JSON.stringify(policy.ceiling) !== JSON.stringify(STRICT_PERFORMANCE_POLICY.ceiling)) {
    issues.push("matrix strict performance policy machine-ceiling stage differs from the ratified law");
  }
  if (policy.text !== STRICT_PERFORMANCE_POLICY.text) {
    issues.push("matrix strict performance policy text differs from AGENTS.md");
  }
  const matrixIds = matrixCells.map((cell) => cell.id);
  if (new Set(matrixIds).size !== matrixIds.length || matrixIds.some((id) => typeof id !== "string" || id.length === 0)) {
    issues.push("matrix strict performance policy cannot address an invalid or duplicate matrix cell");
  }
  const covered = requiredPolicyCells(policy, matrixIds);
  for (const id of matrixIds) {
    if (!covered.has(id)) issues.push(`${id}: matrix cell has no required strict-performance territory battery`);
  }
  return [...new Set(issues)];
}

// The cells some required territory claims: every one of them is judged against
// its peers and against its machine ceiling.
function requiredPolicyCells(policy, matrixIds) {
  return new Set([
    ...(Array.isArray(policy?.foundations) ? policy.foundations : []),
    ...(Array.isArray(policy?.critical_areas) ? policy.critical_areas : []),
  ].filter((area) => area?.required === true)
    .flatMap((area) => policyTerritoryCells(area, matrixIds)));
}

// The machine-ceiling references (CEILING_POLICY.references): every asm-parity
// cell names the matrix cells it bounds in `gauntlet` ([] for a microbenchmark
// that bounds none) and ships its hand-written reference beside cells.json.
async function ceilingReferenceIssues(matrix, references) {
  if (!Array.isArray(references)) return [`${CEILING_POLICY.references}: machine-ceiling references are missing`];
  const matrixIds = new Set((Array.isArray(matrix?.cells) ? matrix.cells : []).map((cell) => cell.id));
  const referenceDir = path.dirname(path.join(repoDir, CEILING_POLICY.references));
  const issues = [];
  const names = new Set();
  for (const reference of references) {
    const name = reference?.name;
    if (typeof name !== "string" || name.length === 0) {
      issues.push(`${CEILING_POLICY.references}: a machine-ceiling reference has no name`);
      continue;
    }
    if (names.has(name)) issues.push(`${CEILING_POLICY.references}: duplicate machine-ceiling reference ${name}`);
    names.add(name);
    if (!Array.isArray(reference[CEILING_POLICY.reference_link])) {
      issues.push(`${name}: machine-ceiling reference does not name its gauntlet cells`);
    } else {
      for (const cell of reference[CEILING_POLICY.reference_link]) {
        if (!matrixIds.has(cell)) issues.push(`${name}: machine-ceiling reference names unknown matrix cell ${cell}`);
      }
    }
    if (!await exists(path.join(referenceDir, name, CEILING_POLICY.reference_source))) {
      issues.push(`${name}: machine-ceiling reference has no ${CEILING_POLICY.reference_source}`);
    }
  }
  return issues;
}

// The runtime and size metrics a mode's machine ceiling judges.
function ceilingMetrics(mode) {
  return comparisonMetrics(mode).filter((metric) => Object.hasOwn(CEILING_POLICY.metrics, LOSS_OWNER_CATEGORY_BY_METRIC[metric]));
}

function ceilingRatioVerdict(ratio) {
  if (typeof ratio !== "number" || !Number.isFinite(ratio) || ratio <= 0) return null;
  return ratio <= 1 ? "meet" : "miss";
}

// One verdict for several judged rows: any wrong, unmeasured or missed row
// fails the metric; it meets only when every row meets.
function ceilingReduce(verdicts) {
  for (const verdict of ["wrong", "uncovered", "unmeasured", "miss"]) {
    if (verdicts.includes(verdict)) return verdict;
  }
  return verdicts.length ? "meet" : "unmeasured";
}

// A matrix cell against its machine ceiling. `measurements` is an asm-parity
// results.json (Tools/perf/asm-parity/asm-parity.mjs): each reference's `asm`
// row and every Jet row it measured. A cell no reference names is uncovered.
function ceilingCell(cell, references, measurements, required) {
  const metrics = ceilingMetrics(matrixCellMode(cell));
  const names = (Array.isArray(references) ? references : [])
    .filter((reference) => Array.isArray(reference?.[CEILING_POLICY.reference_link]) && reference[CEILING_POLICY.reference_link].includes(cell.id))
    .map((reference) => reference.name);
  const rows = Array.isArray(measurements?.rows) ? measurements.rows : [];
  const metricResults = Object.fromEntries(metrics.map((metric) => {
    const measure = CEILING_POLICY.metrics[LOSS_OWNER_CATEGORY_BY_METRIC[metric]];
    if (!names.length) {
      return [metric, { measure, verdict: "uncovered", rows: [], reason: "no hand-written machine-code reference names this cell" }];
    }
    const ratioKey = measure === "cycles" ? "cycles_x" : "size_x";
    const judged = names.flatMap((name) => {
      const own = rows.filter((row) => row?.cell === name);
      const asm = own.find((row) => row.variant === "asm");
      const jet = own.filter((row) => row.variant !== "asm");
      if (!asm || !jet.length) return [{ reference: name, variant: null, ratio: null, verdict: "unmeasured" }];
      return jet.map((row) => ({
        reference: name,
        variant: row.variant,
        ratio: typeof row[ratioKey] === "number" ? row[ratioKey] : null,
        verdict: asm.output !== "same" || row.output !== "same" ? "wrong" : (ceilingRatioVerdict(row[ratioKey]) ?? "unmeasured"),
      }));
    });
    const verdict = ceilingReduce(judged.map((row) => row.verdict));
    const reason = verdict === "meet" ? null
      : verdict === "unmeasured" ? "machine-ceiling reference was not measured (asm-parity results are missing)"
      : verdict === "wrong" ? "machine-ceiling output differs from the cell's expected output"
      : "Jet/ceiling is above 1.00";
    return [metric, { measure, verdict, rows: judged, reason }];
  }));
  return {
    required,
    status: names.length ? "covered" : "uncovered",
    references: names,
    metrics: metricResults,
    verdict: ceilingReduce(Object.values(metricResults).map((item) => item.verdict)),
  };
}

function matrixPeerLanguages(matrix, cell = null) {
  const rails = matrix?.rails ?? {};
  const incumbent = cell?.domain && isObject(rails.incumbents) ? rails.incumbents[cell.domain] : null;
  const runTier = cell === null || Object.hasOwn(tierPolicy(matrixCellMode(cell)), "run");
  const peers = [
    ...(Array.isArray(rails.always) ? rails.always : []),
    ...(Array.isArray(rails.perf) ? rails.perf : []),
    ...(typeof incumbent === "string" ? [incumbent] : (cell ? [] : Object.values(isObject(rails.incumbents) ? rails.incumbents : {}))),
  ].filter((language) => typeof language === "string" && baseLanguage(language) !== "jet");
  // The run rail (PEER_MEASUREMENT_POLICY.run_tier_peers) joins every cell with a run tier.
  const run = runTier && Array.isArray(rails.run) ? rails.run : [];
  return sortedUnique([...(peers.length ? peers : ["rust"]), ...run]);
}

function matrixCellMode(cell) {
  const id = String(cell?.id ?? "");
  if (id === "webfront.app") return "web-app";
  if (cell?.kind === "service" || id.startsWith("netserv.") || id === "concurrency.service") return "service";
  if (cell?.kind === "web" || id === "webfront.widget") return "web";
  if (cell?.kind === "cli" || cell?.kind === "scripting") return "batch-steps";
  return "batch";
}

function missingMatrixCellFailures(matrix, cell) {
  const mode = matrixCellMode(cell);
  const tiers = Object.fromEntries(Object.keys(tierPolicy(mode)).map((tier) => [
    tier,
    { status: "unmeasured", jet: null, peer: null, ratio: null, verdict: null },
  ]));
  return matrixPeerLanguages(matrix, cell).flatMap((peer) => comparisonMetrics(mode).map((metric) => ({
    metric,
    peer,
    verdict: "unmeasured",
    reason: "matrix cell has no corpus entry; peer and metric evidence are missing",
    tiers,
  })));
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
      "artifact_bytes",
      "wasm_bytes",
      "loc",
      "source_bytes",
      "tokens",
      "source_tokens",
    ];
  }
  return COMPARISON_METRICS;
}

function metricApplicability(entry, language, metric) {
  const base = baseLanguage(language);
  const languageFacts = entry?.metric_applicability?.[language];
  const fact = languageFacts && typeof languageFacts === "object" && Object.hasOwn(languageFacts, metric)
    ? languageFacts[metric]
    : languageFacts && typeof languageFacts === "object" && Object.hasOwn(languageFacts, "*")
      ? languageFacts["*"]
      : entry?.non_applicable?.[language];
  if (fact !== undefined) {
    if (!fact || typeof fact !== "object" || Array.isArray(fact)) {
      return { status: "invalid", reason: "metric applicability must be an object" };
    }
    if (fact.status !== undefined && fact.status !== "not_applicable") {
      return { status: "invalid", reason: "metric applicability status must be not_applicable" };
    }
    // A matched peer cannot opt out of one peer-floor metric; a peer that is
    // not applicable as a whole (`*` or `non_applicable`) is not matched.
    if (PEER_FLOOR_METRICS.includes(metric) && Object.hasOwn(languageFacts ?? {}, metric)) {
      return { status: "invalid", reason: `uncovered: ${metric} is a peer-floor metric and is required for Jet and every matched peer` };
    }
    return {
      status: "not_applicable",
      basis: fact.basis ?? null,
      reason: fact.reason ?? null,
      evidence: fact.evidence ?? null,
    };
  }
  if ((entry?.mode === "web" || entry?.mode === "web-app") &&
    (metric === "wasm_bytes" || metric === "binary_bytes") &&
    !WEB_WASM_LANGUAGES.has(base)) {
    return {
      status: "not_applicable",
      basis: "no_wasm_artifact",
      reason: `${language} web rail is script-only and has no Wasm binary artifact.`,
      evidence: "Only Jet and Rust web rails declare Wasm artifacts; script rails are measured by artifact_bytes.",
    };
  }
  return { status: "required" };
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
  const requiredMetricsByMode = Object.fromEntries(Object.keys(TIER_POLICY).map((mode) => [mode, comparisonMetrics(mode)]));
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
    JSON.stringify(reportContract.required_metrics_by_mode) !== JSON.stringify(requiredMetricsByMode) ||
    JSON.stringify(reportContract.axis_schemas) !== JSON.stringify({
      live_reload: "gauntlet-axis-live-reload-v1",
      memory_safety_fuzz: "gauntlet-axis-memory-safety-fuzz-v1",
      compile_throughput: "gauntlet-axis-compile-throughput-v1",
      build_system: "gauntlet-axis-build-system-v1",
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
    !equalStringArrays(memorySafetyFuzz.fairness ?? [], ["same generated input file", "same timeout and resource budget", "sanitizer or equivalent finding evidence", "deduplicate each finding before close"]) ||
    compileThroughputContractIssues(manifest.axes.compile_throughput).length ||
    buildSystemContractIssues(manifest.axes.build_system).length) {
    throw new Error("gauntlet report is missing a required comparison axis");
  }
  if (matrix) {
    if (JSON.stringify(matrix.metric_applicability) !== JSON.stringify(METRIC_APPLICABILITY_POLICY)) {
      throw new Error("matrix is missing the metric applicability policy");
    }
    const strictIssues = strictPerformancePolicyIssues(matrix);
    if (strictIssues.length) {
      throw new Error(`matrix strict performance policy is invalid: ${strictIssues.join("; ")}`);
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
  issues.push(...strictPerformancePolicyIssues(matrix));
  if (JSON.stringify(matrix?.rails?.run) !== JSON.stringify(RUN_TIER_PEERS)) {
    issues.push(`matrix run rail must be the run tier's peers: ${RUN_TIER_PEERS.join(", ")}`);
  }
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
    const preferred = path.join(dir, ".jet", "build", name);
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
  if (language === "asm") return ["node", refBuild, "ref.S", "main-asm"];
  return null;
}

function runCommand(language, sourceDir, artifact, args) {
  language = baseLanguage(language);
  if (language === "jet") return [artifact, ...args];
  if (language === "python") return ["python3", "main.py", ...args];
  if (language === "lua") return ["lua", "main.lua", ...args];
  if (language === "luajit") return ["luajit", "main.lua", ...args];
  if (language === "js" || language === "node") return ["node", "main.mjs", ...args];
  if (language === "bun") return ["bun", "main.mjs", ...args];
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
  await fs.cp(sourceDir, baseline, { recursive: true, preserveTimestamps: true });
  return async () => {
    for (const item of await fs.readdir(sourceDir)) await fs.rm(path.join(sourceDir, item), { recursive: true, force: true });
    await fs.cp(baseline, sourceDir, { recursive: true, preserveTimestamps: true });
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
    rust: "main-rust", c: "main-c", zig: "main", go: "main-go", asm: "main-asm",
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

// Vite has no entry-local build spec: it bundles the web program's runner
// (with everything it imports) into dist/runner.mjs, which node runs.
const VITE_WEB_BUILD = Object.freeze(["vite", "build", "--ssr", "runner.mjs", "--outDir", "dist", "--minify", "esbuild", "--logLevel", "warn"]);
const VITE_WEB_RUN = Object.freeze(["node", "dist/runner.mjs"]);

async function configuredBuildAndMeasure(language, sourceDir, jetBin, entry) {
  const configured = languageSpecValue(entry.spec?.build ?? entry.build, language) ?? (language === "vite" ? [...VITE_WEB_BUILD] : null);
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

// A web entry's run command. Bun runs the Node program's command with bun in
// place of node; Vite runs its bundle.
function webRunCommand(entry, language, jetBin) {
  const configured = (name) => commandFromSpec(languageSpecValue(entry.spec?.run ?? entry.run, name), name, jetBin, ["node", "runner.mjs"]);
  if (language === "vite") return [...VITE_WEB_RUN];
  if (language === "bun") {
    const node = configured("node");
    return node[0] === "node" ? ["bun", ...node.slice(1)] : node;
  }
  return configured(language);
}

async function copyRelativeFile(sourceDir, stageDir, relative) {
  const source = path.join(sourceDir, relative);
  if (!(await exists(source))) return false;
  const target = path.join(stageDir, relative);
  await fs.mkdir(path.dirname(target), { recursive: true });
  await fs.copyFile(source, target);
  return true;
}

async function webArtifactSizes(sourceDir, command = []) {
  const isAsset = (name) => WEB_ARTIFACT_ASSET_PATTERN.test(name)
    && !name.endsWith(".map")
    && !/(?:^|[._-])(?:manifest|log)(?:[._-]|$)/i.test(name);
  const files = new Map();
  const record = async (file) => {
    const identity = path.resolve(file);
    if (files.has(identity)) return;
    const name = path.basename(identity);
    if (!isAsset(name)) return;
    const bytes = (await fs.stat(identity)).size;
    files.set(identity, {
      path: path.relative(sourceDir, identity).split(path.sep).join("/"),
      bytes,
      kind: name.toLowerCase().endsWith(".wasm") ? "wasm" : "script",
    });
  };
  const roots = [];
  for (const name of ["build", "dist", "out"]) {
    const root = path.join(sourceDir, name);
    if (await exists(root)) roots.push(root);
  }
  async function walk(dir) {
    for (const item of await fs.readdir(dir, { withFileTypes: true })) {
      const full = path.join(dir, item.name);
      if (item.isDirectory()) await walk(full);
      else await record(full);
    }
  }
  for (const root of roots) await walk(root);

  if (!roots.length) {
    const declaredEntry = command
      .find((value) => typeof value === "string" && !value.startsWith("-") && WEB_ARTIFACT_ASSET_PATTERN.test(value));
    if (declaredEntry) {
      const entry = path.resolve(sourceDir, declaredEntry);
      const relative = path.relative(sourceDir, entry);
      if (!relative.startsWith("..") && !path.isAbsolute(relative) && await exists(entry)) {
        const harnessEntry = path.basename(entry) === "runner.mjs" ? entry : null;
        const imports = [
          /\bimport\s*(?:(?:[\s\S]*?\sfrom\s*)?["']([^"']+)["'])/g,
          /\bexport\s+(?:[\s\S]*?\sfrom\s*)["']([^"']+)["']/g,
          /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
          /\bnew\s+URL\(\s*["']([^"']+)["']\s*,\s*import\.meta\.url\s*\)/g,
        ];
        const candidates = async (base, specifier) => {
          if (!specifier.startsWith(".")) return null;
          const target = path.resolve(path.dirname(base), specifier);
          const relativeTarget = path.relative(sourceDir, target);
          if (relativeTarget.startsWith("..") || path.isAbsolute(relativeTarget)) return null;
          const paths = [target, `${target}.js`, `${target}.mjs`, `${target}.wasm`,
            path.join(target, "index.js"), path.join(target, "index.mjs")];
          for (const candidate of paths) {
            if (await exists(candidate)) return candidate;
          }
          return null;
        };
        const seen = new Set();
        async function visit(file) {
          const identity = path.resolve(file);
          if (seen.has(identity)) return;
          seen.add(identity);
          if (identity !== harnessEntry) await record(identity);
          if (!/\.(?:js|mjs)$/.test(path.basename(identity))) return;
          const source = await fs.readFile(identity, "utf8");
          for (const pattern of imports) {
            for (const match of source.matchAll(pattern)) {
              const dependency = await candidates(identity, match[1]);
              if (dependency) await visit(dependency);
            }
          }
        }
        await visit(entry);
      }
    }
  }
  const measuredFiles = [...files.values()].sort((left, right) => left.path.localeCompare(right.path));
  if (!measuredFiles.length) return null;
  return {
    artifact_bytes: measuredFiles.reduce((total, file) => total + file.bytes, 0),
    wasm_bytes: measuredFiles.filter((file) => file.kind === "wasm").reduce((total, file) => total + file.bytes, 0),
    files: measuredFiles,
  };
}

async function measureRuns(cwd, command, count, { full = false, reset = null, receipt = null } = {}) {
  const samples = [];
  for (let i = 0; i < count; i += 1) {
    if (reset) await reset();
    const sample = await timedProcess(cwd, command, { full });
    if (receipt) {
      samples.push({
        ...sample,
        rail: receipt.rail,
        artifact_bytes: receipt.artifact_bytes,
        wasm_bytes: receipt.wasm_bytes,
      });
    } else {
      samples.push(sample);
    }
  }
  const summary = summarizeSamples(samples);
  if (receipt) {
    summary.artifact = {
      artifact_bytes: receipt.artifact_bytes ?? null,
      wasm_bytes: receipt.wasm_bytes ?? null,
      files: receipt.files ?? [],
    };
  }
  return summary;
}

// User-mode cycles of `commands` run in sequence (a batch-steps sample runs
// every step), counted by `perf stat -e cycles:u` over the process tree; the
// median of `count` samples, each after `reset`. A missing perf, a failed run
// or an uncounted event is the row's reason, never a dropped sample.
async function measureCycles(cwd, commands, count, { full = false, reset = null } = {}) {
  const samples = [];
  for (let i = 0; i < count; i += 1) {
    if (reset) await reset();
    let total = 0;
    for (const command of commands) {
      const result = await runProcess(cwd, ["perf", "stat", "-x,", "-e", "cycles:u", "--", ...command], { full });
      const stderr = result.stderr.toString("utf8");
      const line = stderr.split(/\r?\n/).reverse().find((text) => (text.split(",")[2] ?? "").startsWith("cycles"));
      if (result.code === 127 && !line) return { status: "unavailable", value: null, samples, reason: `perf unavailable: ${stderr.trim().slice(0, 200)}` };
      if (result.code !== 0) return { status: "failed", value: null, samples, reason: `perf stat run exit ${result.code}: ${stderr.trim().slice(-300)}` };
      const cycles = Number(line?.split(",")[0]);
      if (!Number.isFinite(cycles) || cycles <= 0) return { status: "unmeasured", value: null, samples, reason: `perf stat counted no cycles: ${line ?? stderr.trim().slice(-200)}` };
      total += cycles;
    }
    samples.push(total);
  }
  return { status: "ok", value: median(samples), samples, statistic: "median", counter: "cycles:u" };
}

// A standings run counts every measured row's cycles beside its timed runs.
async function countCycles(row, cwd, commands, count, options) {
  row.cycles = await measureCycles(cwd, commands, count, options);
  row.metrics.runtime_cycles = row.cycles.value;
}

// An interpreted peer's script: its program directory's script files, the
// harness fixtures left out.
async function scriptFiles(dir) {
  const files = [];
  async function walk(current) {
    for (const item of await fs.readdir(current, { withFileTypes: true })) {
      const full = path.join(current, item.name);
      if (item.isDirectory()) {
        if (!["fixtures", "node_modules", "__pycache__"].includes(item.name)) await walk(full);
      } else if (/\.(?:py|mjs|cjs|js)$/.test(item.name)) {
        files.push({ path: path.relative(dir, full).split(path.sep).join("/"), bytes: (await fs.stat(full)).size });
      }
    }
  }
  await walk(dir);
  files.sort((left, right) => left.path.localeCompare(right.path));
  return { bytes: files.reduce((total, file) => total + file.bytes, 0), files };
}

// ---- deployed size (#6834 c6) ------------------------------------------------
// One stated size basis (measurement-manifest.json standings.size_basis). A
// compiled program is charged the static, symbol-stripped build of the same
// program where its language can build that way and that build reproduces the
// expected output; otherwise its stripped binary plus every shared library it
// loads, libc and the dynamic loader included. Jet and asm build static.
// Interpreted peers are charged their script plus the interpreter and every
// shared library it loads, on the same terms.
const STATIC_SIZE_BUILDS = Object.freeze({
  c: (dir, out) => ["gcc", "-O2", "-static", "-s", "main.c", "-o", out, "-lm"],
  rust: (dir, out) => ["rustc", "--edition=2021", "-O", "-C", "target-feature=+crt-static", "-C", "strip=symbols", "main.rs", "-o", out],
  zig: (dir, out) => ["zig", "build-exe", "-O", "ReleaseFast", "-fstrip", "--cache-dir", "zig-cache", "--global-cache-dir", "zig-global-cache", `-femit-bin=${out}`, "main.zig"],
  go: (dir, out) => ["env", "GO111MODULE=off", "CGO_ENABLED=0", `GOCACHE=${path.join(dir, "go-cache")}`, "go", "build", "-ldflags=-s -w", "-o", out, "main.go"],
});

// Whether an ELF64 executable loads through a dynamic loader (PT_INTERP), and
// whether it still carries a symbol table or debug sections.
async function elfLinkage(file) {
  const name = path.basename(file);
  try {
    const data = await fs.readFile(file);
    if (data.length < 64 || data.toString("latin1", 0, 4) !== "\x7fELF") return { error: `${name} is not an ELF executable` };
    if (data[4] !== 2 || data[5] !== 1) return { error: `${name} is not a little-endian ELF64 executable` };
    const phoff = Number(data.readBigUInt64LE(32));
    const phentsize = data.readUInt16LE(54);
    let dynamic = false;
    for (let index = 0; index < data.readUInt16LE(56); index += 1) {
      if (data.readUInt32LE(phoff + index * phentsize) === 3) dynamic = true;
    }
    const shoff = Number(data.readBigUInt64LE(40));
    const shentsize = data.readUInt16LE(58);
    const shnum = data.readUInt16LE(60);
    const shstrndx = data.readUInt16LE(62);
    const sections = [];
    if (shoff > 0 && shstrndx < shnum) {
      const strings = Number(data.readBigUInt64LE(shoff + shstrndx * shentsize + 24));
      for (let index = 0; index < shnum; index += 1) {
        const start = strings + data.readUInt32LE(shoff + index * shentsize);
        sections.push(data.toString("latin1", start, data.indexOf(0, start)));
      }
    }
    return { dynamic, stripped: !sections.some((section) => section === ".symtab" || section.startsWith(".debug")), bytes: data.length };
  } catch (error) {
    return { error: `${name} could not be read as ELF: ${error.message}` };
  }
}

// Every shared library `file` loads, by ldd, libc and the dynamic loader
// included; the vDSO has no file and costs nothing to deploy.
async function sharedLibraries(cwd, file) {
  const result = await runProcess(cwd, ["ldd", file], { timeoutMs: 30_000 });
  const text = `${result.stdout.toString("utf8")}\n${result.stderr.toString("utf8")}`;
  const lines = text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  if (result.code === 127) return { status: "unavailable", reason: `ldd unavailable: ${lines.join(" ").slice(0, 200)}` };
  if (lines.some((line) => /not a dynamic executable|statically linked/.test(line))) return { status: "ok", libraries: [] };
  if (result.code !== 0) return { status: "unmeasured", reason: `ldd ${path.basename(file)} exit ${result.code}: ${lines.join(" ").slice(0, 200)}` };
  const missing = lines.find((line) => /=> not found/.test(line));
  if (missing) return { status: "unmeasured", reason: `ldd ${path.basename(file)}: ${missing}` };
  const libraries = new Map();
  for (const line of lines) {
    const match = line.match(/^(\S+)(?:\s+=>\s+(\S+))?\s+\(0x[0-9a-f]+\)$/);
    const library = match?.[2] ?? (match?.[1].startsWith("/") ? match[1] : null);
    if (!library) continue;
    const real = await fs.realpath(library);
    if (!libraries.has(real)) libraries.set(real, { path: real, bytes: (await fs.stat(real)).size });
  }
  return { status: "ok", libraries: [...libraries.values()].sort((left, right) => left.path.localeCompare(right.path)) };
}

const libraryBytes = (libraries) => libraries.reduce((total, library) => total + library.bytes, 0);

// The deployed size of a compiled row's program on the stated basis. `check`
// runs a candidate binary the way the row was verified and returns a failure
// or null; a static build or stripped copy counts only once it passes. Size
// builds go to <dir>.size so a state reset of `dir` cannot remove them.
async function deployedSize(language, dir, artifact, { override = false, check }) {
  const sizeDir = `${dir}.size`;
  await fs.mkdir(sizeDir, { recursive: true });
  const measured = await elfLinkage(artifact);
  if (measured.error) return { status: "unmeasured", reason: measured.error };
  let file = artifact;
  let build = "measured binary";
  let staticBuild = null;
  const variant = STATIC_SIZE_BUILDS[baseLanguage(language)];
  if (variant && override) staticBuild = "not used: the entry overrides the build command";
  else if (variant) {
    // Relative to `dir`, the build's cwd, so the recorded command names no stage path.
    const out = path.join("..", path.basename(sizeDir), `static-${baseLanguage(language)}`);
    const built = path.join(dir, out);
    const command = variant(dir, out);
    const result = await runProcess(dir, command);
    const linkage = result.code === 0 ? await elfLinkage(built) : null;
    const stderr = result.stderr.toString("utf8").trim();
    const firstError = stderr.split("\n").find((line) => /\berror\b/i.test(line))?.trim();
    const failure = result.code !== 0 ? `\`${command.join(" ")}\` exit ${result.code}: ${[firstError, stderr.slice(-200)].filter(Boolean).join(" … ")}`
      : linkage.error ? linkage.error
        : linkage.dynamic ? "the static build still loads a dynamic loader"
          : await check(built);
    if (failure) staticBuild = `not used: ${failure}`;
    else {
      file = built;
      build = "static stripped build";
      staticBuild = command.join(" ");
    }
  }
  let linkage = await elfLinkage(file);
  if (linkage.error) return { status: "unmeasured", reason: linkage.error };
  if (!linkage.stripped) {
    const stripped = path.join(sizeDir, `${path.basename(file)}.stripped`);
    const result = await runProcess(dir, ["strip", "-o", stripped, file]);
    if (result.code !== 0) {
      return { status: result.code === 127 ? "unavailable" : "unmeasured", reason: `strip ${path.basename(file)} exit ${result.code}: ${result.stderr.toString("utf8").trim().slice(0, 200)}` };
    }
    const failure = await check(stripped);
    if (failure) return { status: "unmeasured", reason: `the stripped binary fails its check: ${failure}` };
    file = stripped;
    build = `${build}, stripped`;
    linkage = await elfLinkage(file);
    if (linkage.error) return { status: "unmeasured", reason: linkage.error };
  }
  const shared = linkage.dynamic ? await sharedLibraries(dir, file) : { status: "ok", libraries: [] };
  if (shared.status !== "ok") return { status: shared.status, reason: shared.reason };
  return {
    status: "ok",
    basis: linkage.dynamic ? "stripped + shared libraries" : "static stripped",
    linkage: linkage.dynamic ? "dynamic" : "static",
    measured: { linkage: measured.dynamic ? "dynamic" : "static", stripped: measured.stripped, bytes: measured.bytes },
    build,
    file_bytes: linkage.bytes,
    libraries: shared.libraries,
    bytes: linkage.bytes + libraryBytes(shared.libraries),
    ...(staticBuild ? { static_build: staticBuild } : {}),
  };
}

// The runtime an interpreted peer ships with: the interpreter executable the
// dev shell resolves and every shared library it loads.
async function interpreterFootprint(cwd, interpreter) {
  const script = 'exe=$(command -v "$1") || exit 127; readlink -f "$exe"';
  const result = await runProcess(cwd, ["sh", "-c", script, "sh", interpreter], { timeoutMs: 30_000 });
  const exe = result.stdout.toString("utf8").trim();
  if (result.code !== 0 || !exe) {
    return { interpreter, status: "unavailable", reason: `\`command -v ${interpreter}\` exit ${result.code}: ${result.stderr.toString("utf8").trim().slice(0, 200)}` };
  }
  const linkage = await elfLinkage(exe);
  if (linkage.error) return { interpreter, path: exe, status: "unmeasured", reason: `${exe} is not the interpreter binary: ${linkage.error}` };
  const shared = linkage.dynamic ? await sharedLibraries(cwd, exe) : { status: "ok", libraries: [] };
  if (shared.status !== "ok") return { interpreter, path: exe, status: shared.status, reason: shared.reason };
  return {
    interpreter,
    status: "ok",
    path: exe,
    exe_bytes: linkage.bytes,
    libraries: shared.libraries,
    bytes: linkage.bytes + libraryBytes(shared.libraries),
  };
}

// The interpreter each interpreted standings peer runs.
const STANDINGS_INTERPRETERS = Object.freeze({ python: "python3", node: "node", bun: "bun", lua: "lua", luajit: "luajit" });

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

// Polls the service started through the launcher until it answers its
// readiness probe. Its startup runs from the launcher's start line (just
// before the service's exec; node's hrtime and the launcher both read
// CLOCK_MONOTONIC) to the answer, so it holds neither JetEnv's nor the
// launcher's own startup.
async function waitForReady(child, port, service, report) {
  const protocol = service.protocol ?? "http";
  const started = performance.now();
  const answered = async (result) => {
    const readyNs = process.hrtime.bigint();
    const start = await launchReport(report, "start");
    if (start === null) throw new Error("service answered before its launcher's report began");
    return { seconds: Number(readyNs - BigInt(start[1])) / 1e9, result };
  };
  while (performance.now() - started < 10000) {
    if (child.exitCode !== null) throw new Error(`service exited ${child.exitCode}${child.stderrText() ? `: ${child.stderrText()}` : ""}`);
    if (protocol === "line") {
      const ready = service.ready ?? { send: "ready\n", expect: "ready\n" };
      const result = await lineProbe(port, ready, 500);
      if (result.ok && result.body === ready.expect) return answered(result);
    } else {
      const result = await httpProbe(port, { method: "GET", path: service.readyPath }, 500);
      if (result.ok) return answered(result);
    }
  }
  throw new Error(`service did not answer ${protocol === "line" ? "line readiness probe" : service.readyPath}`);
}

// Measures a service: a verification run, then a measured run whose startup,
// probe latencies and peak memory are the result. Both run through the
// launcher; the peak is the launcher's, each service process's exact VmHWM
// at its exit (the largest), read from its report once the service exits.
async function runService(language, sourceDir, artifact, entry, commandForOverride = null) {
  const service = entry.spec?.service ?? entry.service ?? {};
  const protocol = service.protocol ?? "http";
  const probes = service.probe ?? [];
  if (!service.portArg || probes.length === 0 || !["http", "line"].includes(protocol) || (protocol === "http" && !service.readyPath) || (protocol === "line" && !service.ready)) return { failure: "service requires portArg, readiness, and probe" };
  const commandFor = commandForOverride ?? ((port) => runCommand(language, sourceDir, artifact, [String(port)]));
  let child = null;
  let startupSeconds = null;
  let failure = null;
  const reports = await fs.mkdtemp(path.join(os.tmpdir(), "jet-service-"));
  const verifyReport = path.join(reports, "verify");
  const report = path.join(reports, "measure");
  try {
    const port = await freePort();
    child = startProcess(sourceDir, commandFor(port), { full: entry.spec?.fullShell === true, report: verifyReport });
    startupSeconds = (await waitForReady(child, port, service, verifyReport)).seconds;
    const verification = await probeSequence(port, probes, protocol);
    if (!verification.ok) failure = `probe ${verification.index} failed: ${verification.reason}`;
    const firstExit = await waitForExit(child, 1000);
    if (firstExit.code !== 0 && !failure) failure = `verification service exit ${firstExit.code ?? firstExit.signal}`;
    if (firstExit.code === null) {
      if (!failure) failure = "verification shutdown did not produce a clean exit";
      await stopProcess(child);
    }
  } catch (error) {
    failure = error.message;
    if (child) await stopProcess(child);
  }
  if (failure) {
    await fs.rm(reports, { recursive: true, force: true });
    return { failure, startupSeconds };
  }

  const port = await freePort();
  child = startProcess(sourceDir, commandFor(port), { full: entry.spec?.fullShell === true, report });
  let ready;
  try {
    ready = await waitForReady(child, port, service, report);
  } catch (error) {
    await stopProcess(child);
    await fs.rm(reports, { recursive: true, force: true });
    return { failure: error.message, startupSeconds };
  }
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
  }
  const exit = await waitForExit(child, 5000);
  if (exit.code === null) await stopProcess(child);
  const rssKb = (await launchReport(report, "end"))?.[3] ?? null;
  await fs.rm(reports, { recursive: true, force: true });
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
    startup_samples_seconds: [startupSeconds],
    startup_noise: noiseSummary([startupSeconds]),
    startup_confidence: {
      status: "not_computed",
      method: "independent_service_startup_sample",
      level: null,
      reason: "Only one fresh-process startup sample is retained; no confidence interval is inferred.",
    },
    latencyMs: { median: median(latencies), p99: percentile(latencies, 0.99) },
    latency_samples_ms: latencies,
    latency_noise: noiseSummary(latencies),
    latency_confidence: {
      status: "not_computed",
      method: "independent_service_probe_samples",
      level: null,
      reason: "Probe samples are retained, but strict sub-millisecond claims require paired confidence evidence.",
    },
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
    await stopProcess(child);
    throw error;
  }
}


function ratioVerdict(ratio) {
  if (!Number.isFinite(ratio) || ratio <= 0) return null;
  return ratio < 1 ? "win" : "loss";
}

function subMillisecondMetric(metric, jet, peer) {
  const limit = metric.endsWith("_seconds") ? 0.001
    : metric.startsWith("service_latency_ms_") ? 1
      : null;
  return limit !== null && [jet, peer].some((value) => positiveMetricValue(metric, value) && value < limit);
}

function runtimeMetricEvidence(runtime, metric) {
  if (!runtime || runtime.protocol !== TIMER_SCHEMA || !Array.isArray(runtime.samples)) return null;
  const field = metric === "runtime_first_stdout_seconds" ? "time_to_first_stdout_seconds" : "wall_seconds";
  const rawField = metric === "runtime_first_stdout_seconds" ? "time_to_first_stdout_ns" : "wall_time_ns";
  return {
    protocol: TIMER_SCHEMA,
    raw_samples: runtime.samples.map((sample) => ({
      value: Number.isFinite(sample?.[field]) ? sample[field] : null,
      raw_ns: Number.isFinite(sample?.[rawField]) ? sample[rawField] : null,
    })),
    median: runtime.median?.[field] ?? null,
    noise: runtime.noise?.metrics?.[field] ?? null,
    confidence: runtime.confidence ?? null,
  };
}

function serviceMetricEvidence(metrics, metric) {
  if (!metrics) return null;
  const latencyMetric = metric.startsWith("service_latency_ms_");
  const values = latencyMetric ? metrics.service_latency_samples_ms : metrics.service_startup_samples_seconds;
  if (!Array.isArray(values)) return null;
  return {
    protocol: "jet.gauntlet.service-probe-v1",
    raw_samples: values,
    median: metrics[metric] ?? null,
    noise: latencyMetric ? (metrics.service_latency_noise ?? null) : (metrics.service_startup_noise ?? null),
    confidence: latencyMetric ? (metrics.service_latency_confidence ?? null) : (metrics.service_startup_confidence ?? null),
  };
}

function comparisonEvidence(metric, jetValue, peerValue, jetRuntime, peerRuntime, jetMetrics, peerMetrics) {
  if (!subMillisecondMetric(metric, jetValue, peerValue)) return null;
  return {
    metric,
    jet: metric.startsWith("service_latency_ms_") || metric === "service_startup_seconds"
      ? serviceMetricEvidence(jetMetrics, metric)
      : runtimeMetricEvidence(jetRuntime, metric),
    peer: metric.startsWith("service_latency_ms_") || metric === "service_startup_seconds"
      ? serviceMetricEvidence(peerMetrics, metric)
      : runtimeMetricEvidence(peerRuntime, metric),
  };
}

// One metric cell of one tier: Jet's value against the peer row's value.
function metricComparisonCell(entry, language, metric, tier, { tierReady, unavailableReason, jetMetrics, jetRuntime, peer, jetConfiguration }) {
  const applicability = metricApplicability(entry, language, metric);
  if (applicability.status === "invalid") {
    return {
      status: "unmeasured",
      applicability,
      jet: null,
      peer: null,
      ratio: null,
      verdict: null,
      reason: applicability.reason,
    };
  }
  if (applicability.status === "not_applicable") {
    return {
      status: "not_applicable",
      applicability,
      jet: null,
      peer: null,
      ratio: null,
      verdict: null,
    };
  }
  if (!metricComparableAtTier(metric, tier)) {
    return {
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
  }
  const jetValue = positiveMetricValue(metric, jetMetrics?.[metric]) ? jetMetrics[metric] : null;
  const peerMetrics = peer?.metrics ?? {};
  const peerValue = positiveMetricValue(metric, peerMetrics[metric]) ? peerMetrics[metric] : null;
  const ratio = tierReady && peer?.status === "ok" && peerValue !== null && jetValue !== null
    ? jetValue / peerValue
    : null;
  const evidence = comparisonEvidence(metric, jetValue, peerValue, jetRuntime, peer?.runtime, jetMetrics, peerMetrics);
  const evidenceReady = !evidence ||
    (evidence.jet?.confidence?.status === "available" && evidence.peer?.confidence?.status === "available" &&
      evidence.jet?.noise && evidence.peer?.noise &&
      evidence.jet.raw_samples?.length > 0 && evidence.peer.raw_samples?.length > 0);
  const verdict = evidenceReady ? ratioVerdict(ratio) : null;
  return {
    status: verdict === null ? (evidence && !evidenceReady ? "inconclusive" : "unmeasured") : "measured",
    jet: jetValue,
    peer: peerValue,
    ratio,
    verdict,
    evidence,
    noise: evidence ? { jet: evidence.jet?.noise ?? null, peer: evidence.peer?.noise ?? null } : null,
    confidence: evidence ? { jet: evidence.jet?.confidence ?? null, peer: evidence.peer?.confidence ?? null } : null,
    reason: verdict === null
      ? (evidence && !evidenceReady ? "sub-millisecond comparison lacks paired confidence evidence"
        : !tierReady ? (unavailableReason ?? `matched Jet ${jetConfiguration} ${tier} tier is unavailable`)
          : peer?.status !== "ok" ? "peer row is unavailable"
            : jetValue === null ? `matched Jet ${jetConfiguration} metric is missing or invalid`
              : peerValue === null ? "peer metric is missing or invalid"
                : "invalid metric ratio")
      : null,
  };
}

// The run tier's cell judges every gated cache state: it carries the first
// state without a verdict, or else the state with the highest Jet/peer ratio,
// so the cell wins only when every gated state wins.
function runTierMetricCell(entry, language, metric, runTier, context) {
  const ordered = RUN_CACHE_STATES.gated.map((state) => {
    const measured = runTier?.cache_states?.[state];
    return [state, metricComparisonCell(entry, language, metric, "run", {
      ...context,
      tierReady: runTier?.status === "ok" && measured?.status === "ok",
      unavailableReason: runTier?.status === "ok" ? `matched Jet ${context.jetConfiguration} run tier has no measured ${state} cache state` : undefined,
      jetMetrics: measured?.metrics,
      jetRuntime: measured?.runtime,
    })];
  });
  const [state, cell] = ordered.find(([, item]) => item.status !== "measured" && item.status !== "not_applicable") ??
    ordered.reduce((worst, candidate) => ((candidate[1].ratio ?? 0) > (worst[1].ratio ?? 0) ? candidate : worst));
  return {
    ...cell,
    cache_state: state,
    cache_states: Object.fromEntries(ordered.map(([name, item]) => [name, {
      status: item.status,
      jet: item.jet,
      peer: item.peer,
      ratio: item.ratio,
      verdict: item.verdict,
      reason: item.reason ?? null,
    }])),
  };
}

function comparisons(entry, languages, rows, tiers = {}) {
  const metrics = comparisonMetrics(entry.mode);
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
    const comparisonPolicy = peerTierPolicy(entry, language);
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
        : expertPeer ? "jet-expert-aot-and-matched-peer"
          : Object.hasOwn(comparisonPolicy, "run") ? "jet-aot-and-run" : "jet-aot",
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
      const context = {
        tierReady: comparisonTiers[tier]?.status === "ok",
        jetMetrics: tier === "aot" ? matchedJet?.metrics : comparisonTiers[tier]?.metrics,
        jetRuntime: tier === "aot" ? matchedJet?.runtime : comparisonTiers[tier]?.runtime,
        peer,
        jetConfiguration,
      };
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
        tierComparison.metrics[metric] = tier === "run"
          ? runTierMetricCell(entry, language, metric, comparisonTiers.run, context)
          : metricComparisonCell(entry, language, metric, tier, context);
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

function serviceTierMetrics(service) {
  return {
    runtime_wall_seconds: null,
    runtime_first_stdout_seconds: null,
    runtime_peak_rss_kb: service.rssKb ?? null,
    service_startup_seconds: service.startupSeconds ?? null,
    service_startup_samples_seconds: service.startup_samples_seconds ?? [],
    service_startup_noise: service.startup_noise ?? null,
    service_startup_confidence: service.startup_confidence ?? null,
    service_latency_ms_p50: service.latencyMs?.median ?? null,
    service_latency_ms_p99: service.latencyMs?.p99 ?? null,
    service_latency_samples_ms: service.latency_samples_ms ?? [],
    service_latency_noise: service.latency_noise ?? null,
    service_latency_confidence: service.latency_confidence ?? null,
    binary_bytes: null,
  };
}

// Times the run tier in each cache state (PEER_MEASUREMENT_POLICY.run_cache_states).
// `measure(stateCommand, prepare)` runs the state's samples, calling `prepare`
// before each one and running every command through `stateCommand`. `repeat`
// goes first, while run.jet is still as the tier's verification run left it;
// `edited` rewrites run.jet with a new trailing comment before every sample;
// `fresh_install` gives every sample an empty cache home. run.jet gets its
// bytes and times back afterward.
async function measureRunCacheStates(jetDir, measure) {
  const script = path.join(jetDir, "run.jet");
  const original = await fs.readFile(script, "utf8");
  const originalStat = await fs.stat(script);
  const cacheHome = `${jetDir}.fresh-cache`;
  const states = {};
  try {
    for (const state of ["repeat", "edited", "fresh_install"]) {
      let edits = 0;
      const stateCommand = (command) => state === "fresh_install" ? ["env", `XDG_CACHE_HOME=${cacheHome}`, ...command] : command;
      const prepare = async () => {
        if (state === "edited") {
          edits += 1;
          await fs.writeFile(script, `${original}\n// gauntlet edit ${edits}\n`);
        } else if (state === "fresh_install") {
          await fs.rm(cacheHome, { recursive: true, force: true });
          await fs.mkdir(cacheHome, { recursive: true });
        }
      };
      states[state] = { cache_state: state, gated: RUN_CACHE_STATES.gated.includes(state), ...(await measure(stateCommand, prepare)) };
    }
  } finally {
    await fs.writeFile(script, original);
    await fs.utimes(script, originalStat.atime, originalStat.mtime);
    await fs.rm(cacheHome, { recursive: true, force: true });
  }
  return states;
}

// The run tier is broken when a gated cache state is; otherwise its row carries
// the slower gated state's runtime and metrics, so it never reads faster than
// either. Comparisons judge every gated state on its own (runTierMetricCell).
function settleRunTier(tierRow, metric, sourceMetrics, verificationKind) {
  const gated = RUN_CACHE_STATES.gated.map((state) => tierRow.cache_states?.[state]);
  const failed = gated.find((state) => state?.status !== "ok");
  if (failed) {
    tierRow.status = "broken";
    tierRow.reason = `${failed?.cache_state ?? "gated"} cache state: ${failed?.reason ?? "unmeasured"}`;
    tierRow.verification = { status: "failed", kind: verificationKind, reason: tierRow.reason };
    return;
  }
  const slower = gated.reduce((worst, state) => ((state.metrics[metric] ?? 0) > (worst.metrics[metric] ?? 0) ? state : worst));
  tierRow.cache_state = slower.cache_state;
  tierRow.runtime = slower.runtime;
  tierRow.metrics = { ...sourceMetrics, ...slower.metrics };
}

// `standings`, on a standings run, names the standings peers the entry does
// not declare and the asm-parity reference ({ name, dir }) that is its
// ceiling: they are staged, verified and measured like the declared rows into
// `standings_rows`, which the scoreboard and status never read, and every
// measured row also counts its cycles and an interpreted row its script
// bytes. Without it (null) the entry stages its declared rows alone.
// `measure`, on a --languages run, is the set of rows (languages, standings
// peers, and asm for the ceiling) the run measures; every other row that has
// a program is recorded unmeasured (`not_measured`, with its program's tree
// hash) and Jet's tiers too when Jet is not in it, for mergeEarlierResult to
// fill from an earlier run. Null measures every row.
async function stageEntry(entryDir, entry, runDir, jetBin, selectedRuns, dev, standings = null, measure = null) {
  const entryStage = path.join(runDir, entry.name);
  const notMeasured = measure ? `not measured: this run measures ${[...measure].join(", ")} only` : null;
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
  const standingsRows = {};
  const standingsOnly = [
    ...(standings?.peers ?? []).filter((peer) => !languages.includes(peer)),
    ...(standings?.ceiling ? ["asm"] : []),
  ];
  for (const language of [...languages, ...standingsOnly]) {
    const sourceDir = language === "asm" ? standings.ceiling.dir : path.join(entryDir, LANGUAGE_SOURCE_DIRS[baseLanguage(language)] ?? language);
    const sourceFile = LANGUAGE_FILES[baseLanguage(language)];
    const row = { language, status: "broken", metrics: {}, diagnostics: [] };
    if (languages.includes(language)) rows[language] = row;
    else standingsRows[language] = row;
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
    const sourceTreeSha256 = await treeSha256(sourceDir);
    if (measure && !measure.has(language)) {
      row.status = "unavailable";
      row.reason = notMeasured;
      row.not_measured = true;
      row.provenance = { source_tree_sha256: sourceTreeSha256, base_language: baseLanguage(language) };
      continue;
    }
    const stagedSource = path.join(entryStage, language);
    await fs.cp(sourceDir, stagedSource, { recursive: true });
    const fixtureState = path.join(stagedSource, "fixture-state");
    if (await exists(fixtureState)) {
      await fs.cp(fixtureState, path.join(stagedSource, ".jet"), { recursive: true });
      await fs.rm(fixtureState, { recursive: true, force: true });
    }
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
    // #6834 c6: a standings run charges every compiled row its deployed size.
    const deployed = standings && PEER_KINDS[baseLanguage(language)] === "compiled"
      ? (check) => deployedSize(language, stagedSource, build.artifact, { override: buildOverride !== null, check })
      : null;
    row.metrics = await sourceMetrics(sourceDir, sourceFile);
    if (standings && PEER_KINDS[baseLanguage(language)] === "interpreted") row.script = await scriptFiles(sourceDir);
    row.provenance = {
      source: path.relative(language === "asm" ? repoDir : entryDir, path.join(sourceDir, sourceFile)).replaceAll(path.sep, "/"),
      source_sha256: row.metrics.source_sha256,
      source_tree_sha256: sourceTreeSha256,
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
      if (standings) await countCycles(row, stagedSource, commands, runs, { reset });
      if (deployed) row.size = await deployed((file) => verifySequence(stagedSource, steps.map((args) => runCommand(language, stagedSource, file, args)), expected, reset));
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
        service_startup_samples_seconds: service.startup_samples_seconds ?? [],
        service_startup_noise: service.startup_noise ?? null,
        service_startup_confidence: service.startup_confidence ?? null,
        service_latency_ms_p50: service.latencyMs?.median ?? null,
        service_latency_ms_p99: service.latencyMs?.p99 ?? null,
        service_latency_samples_ms: service.latency_samples_ms ?? [],
        service_latency_noise: service.latency_noise ?? null,
        service_latency_confidence: service.latency_confidence ?? null,
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
      if (deployed) row.size = await deployed(async (file) => (await runService(language, stagedSource, file, entry)).failure ?? null);
      continue;
    }

    if (webMode) {
      const command = webRunCommand(entry, language, jetBin);
      const full = entry.spec?.fullShell === true;
      const verification = await verify(stagedSource, command, expected, { full });
      if (verification) {
        row.reason = verification;
        row.disqualified = true;
        row.verification = { status: "failed", kind: "byte_exact_stdout", reason: verification };
        continue;
      }
      const webArtifact = await webArtifactSizes(stagedSource, command);
      row.status = "ok";
      row.verification = { status: "passed", kind: "byte_exact_stdout" };
      row.command = command;
      const runs = selectedRuns ?? (entry.perf ? 7 : 3);
      row.runtime = await measureRuns(stagedSource, command, runs, {
        full,
        receipt: {
          rail: language,
          artifact_bytes: webArtifact?.artifact_bytes ?? null,
          wasm_bytes: webArtifact?.wasm_bytes ?? null,
          files: webArtifact?.files ?? [],
        },
      });
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
        binary_bytes: WEB_WASM_LANGUAGES.has(baseLanguage(language)) ? (webArtifact?.wasm_bytes ?? null) : null,
        artifact_bytes: webArtifact?.artifact_bytes ?? null,
        wasm_bytes: WEB_WASM_LANGUAGES.has(baseLanguage(language)) ? (webArtifact?.wasm_bytes ?? null) : null,
      };
      if (standings) await countCycles(row, stagedSource, [command], runs, { full });
      continue;
    }

    let peerChild = null;
    const stopPeer = async () => {
      if (peerChild) {
        await stopProcess(peerChild);
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
    if (standings) await countCycles(row, stagedSource, [command], runs, { reset: fixtureReset });
    if (deployed) {
      row.size = await deployed(async (file) => {
        if (fixtureReset) await fixtureReset();
        return verify(stagedSource, runCommand(language, stagedSource, file, entry.spec?.args ?? []), expected);
      });
    }
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
  let jetPeerChild = null;
  let jetPeerFailure = null;
  const stopJetPeer = async () => {
    if (jetPeerChild) {
      await stopProcess(jetPeerChild);
      jetPeerChild = null;
    }
  };
  try {
    if (entry.spec?.peer && jetSourceAvailable) {
      try {
        jetPeerChild = await startPeer(jetDir, entry.spec.peer);
      } catch (error) {
        jetPeerFailure = `peer unavailable: ${error.message}`;
      }
    }

  for (const tier of Object.keys(tierPolicy(entry.mode))) {
    const policy = tierPolicy(entry.mode)[tier];
    if (measure && !measure.has("jet")) {
      tiers[tier] = { ...unavailableTier(policy.required, notMeasured), not_measured: true };
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
    if (jetPeerFailure) {
      tiers[tier] = unavailableTier(policy.required, jetPeerFailure);
      continue;
    }

    if (entry.mode === "service") {
      const commandFor = (port) => tier === "run"
        ? [jetBin, "run", "run.jet", "--", String(port)]
        : [jetBin, "dev", "--watch=off", "run.jet", "--", String(port)];
      const tierRow = {
        applicable: true,
        required: policy.required,
        command: commandFor("<port>"),
        metrics: {},
      };
      if (tier === "run") {
        tierRow.cache_states = await measureRunCacheStates(jetDir, async (stateCommand, prepare) => {
          await prepare();
          const service = await runService("jet", jetDir, null, entry, (port) => stateCommand(commandFor(port)));
          return {
            status: service.failure ? "broken" : "ok",
            reason: service.failure,
            command: stateCommand(commandFor("<port>")),
            runtime: null,
            metrics: serviceTierMetrics(service),
          };
        });
        tierRow.status = "ok";
        tierRow.verification = { status: "passed", kind: "service_probe_sequence" };
        settleRunTier(tierRow, primaryMetric(entry.mode), sourceMetricValues(jetRow?.metrics), "service_probe_sequence");
      } else {
        const service = await runService("jet", jetDir, null, entry, commandFor);
        tierRow.status = service.failure ? "broken" : "ok";
        tierRow.reason = service.failure;
        tierRow.verification = { status: service.failure ? "failed" : "passed", kind: "service_probe_sequence", reason: service.failure };
        tierRow.metrics = { ...sourceMetricValues(jetRow?.metrics), ...serviceTierMetrics(service) };
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
    const runs = selectedRuns ?? (entry.perf ? 7 : 3);
    if (tierVerification) {
      tierRow.status = "broken";
      tierRow.reason = tierVerification;
    } else if (tier === "run") {
      tierRow.status = "ok";
      tierRow.cache_states = await measureRunCacheStates(jetDir, async (stateCommand, prepare) => {
        const stateCommands = commands.map(stateCommand);
        const beforeSample = async () => {
          if (reset) await reset();
          await prepare();
        };
        const runtime = stateCommands.length > 1
          ? await measureSequenceRuns(jetDir, stateCommands, runs, beforeSample)
          : await measureRuns(jetDir, stateCommands[0], runs, { reset: beforeSample });
        return {
          status: runtime.valid === true ? "ok" : "broken",
          reason: runtime.valid === true ? undefined : "measured run samples are invalid",
          command: stateCommands.length === 1 ? stateCommands[0] : stateCommands,
          runtime,
          metrics: { ...runtimeMetrics(runtime), binary_bytes: null },
        };
      });
      settleRunTier(tierRow, primaryMetric(entry.mode), sourceMetricValues(jetRow?.metrics), "timed_run");
    } else {
      tierRow.status = "ok";
      tierRow.runtime = commands.length > 1
        ? await measureSequenceRuns(jetDir, commands, runs, reset ?? (async () => {}))
        : await measureRuns(jetDir, command, runs, { reset: jetFixtureReset });
      if (tierRow.runtime.valid !== true) {
        tierRow.status = "broken";
        tierRow.reason = "measured run samples are invalid";
        tierRow.verification = { status: "failed", kind: "timed_run", reason: tierRow.reason };
      } else {
        tierRow.metrics = { ...sourceMetricValues(jetRow?.metrics), ...runtimeMetrics(tierRow.runtime), binary_bytes: null };
      }
    }
    tiers[tier] = tierRow;
  }
  } finally {
    await stopJetPeer();
  }
  return finish({ entry, status: entryStatus(entry, languages, rows, tiers), stage: entryStage, languages, rows, ...(standings ? { standings_rows: standingsRows } : {}), comparisons: comparisons(entry, languages, rows, tiers), jet_tiers: tiers });
}

// An entry is ok when every declared row is ok or not applicable and every
// required Jet tier is ok.
function entryStatus(entry, languages, rows, tiers) {
  const requiredTiersReady = Object.entries(tierPolicy(entry.mode))
    .filter(([, policy]) => policy.required)
    .every(([tier]) => tiers[tier]?.status === "ok");
  return languages.every((language) => ["ok", "not_applicable"].includes(rows[language]?.status)) && requiredTiersReady ? "ok" : "broken";
}

// A --languages run's result with every unmeasured row taken from `earlier`,
// the same entry's result in an earlier run's report (`source`, from
// loadMergeSource): declared rows, standings rows and the asm ceiling, and
// Jet's tiers with its row. A row carries over only when the earlier run
// measured the same thing: the same program tree, and the entry's mode, spec,
// expected output and fixture unchanged; a carried row names the run it came
// from. Any other unmeasured row stays unavailable with the reason.
function mergeEarlierResult(result, earlier, source) {
  const entry = result.entry;
  const inputsReason = !earlier
    ? `the merged run ${source.run_id} has no ${entry.name} entry`
    : earlier.entry?.mode !== entry.mode || JSON.stringify(earlier.entry?.spec ?? null) !== JSON.stringify(entry.spec ?? null) ||
      earlier.provenance?.expected_sha256 !== result.provenance?.expected_sha256 || earlier.provenance?.fixture_sha256 !== result.provenance?.fixture_sha256
      ? `${entry.name}'s mode, spec, expected output or fixture changed since the merged run ${source.run_id}`
      : null;
  const carry = (row) => {
    if (!row?.not_measured) return row;
    const previous = earlier?.rows?.[row.language] ?? earlier?.standings_rows?.[row.language] ?? null;
    const reason = inputsReason ??
      (!previous || previous.not_measured ? `the merged run ${source.run_id} did not measure ${row.language}` :
        previous.provenance?.source_tree_sha256 !== row.provenance.source_tree_sha256 ? `the ${row.language} program changed since the merged run ${source.run_id}` : null);
    return reason ? { ...row, reason: `${row.reason}; not carried: ${reason}` } : { ...previous, carried_from: source.run_id };
  };
  const carryMap = (map) => Object.fromEntries(Object.entries(map).map(([language, row]) => [language, carry(row)]));
  const rows = carryMap(result.rows ?? {});
  const standingsRows = result.standings_rows ? carryMap(result.standings_rows) : null;
  const jetCarried = Boolean(result.rows?.jet?.not_measured) && Boolean(rows.jet?.carried_from);
  const tiers = jetCarried
    ? Object.fromEntries(Object.entries(earlier.jet_tiers ?? {}).map(([tier, row]) => [tier, { ...row, carried_from: source.run_id }]))
    : result.jet_tiers;
  const languages = result.languages ?? [];
  return {
    ...result,
    status: entryStatus(entry, languages, rows, tiers),
    rows,
    ...(standingsRows ? { standings_rows: standingsRows } : {}),
    comparisons: comparisons(entry, languages, rows, tiers),
    jet_tiers: tiers,
  };
}

// The earlier run a --merge names: its report, indexed by entry name.
async function loadMergeSource(file) {
  const report = JSON.parse(await fs.readFile(file, "utf8"));
  if (report?.contract !== "gauntlet-report-v1" || !Array.isArray(report.entries)) throw new Error(`--merge ${file} is not a gauntlet report`);
  return {
    path: file,
    run_id: report.run_id ?? null,
    generated: report.generated ?? null,
    commit: report.provenance?.commit ?? null,
    jet_binary_sha256: report.provenance?.jet_binary_sha256 ?? null,
    toolchains: report.provenance?.toolchains ?? null,
    entries: new Map(report.entries.map((result) => [result.entry?.name, result])),
  };
}

// Comparison entries (ENTRY_MODES) go to `loaded`; tool entries (TOOL_MODE)
// go to `tools`, which never join the matrix denominator; anything else is
// `skipped`.
async function loadEntries(entriesDir, selected) {
  const loaded = [];
  const tools = [];
  const skipped = [];
  if (!(await exists(entriesDir))) return { loaded, tools, skipped };
  for (const item of (await fs.readdir(entriesDir, { withFileTypes: true })).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
    if (!item.isDirectory() || (selected && item.name !== selected)) continue;
    const dir = path.join(entriesDir, item.name);
    const file = path.join(dir, "entry.json");
    try {
      const entry = JSON.parse(await fs.readFile(file, "utf8"));
      const nameDeclared = Object.hasOwn(entry, "name");
      entry.name ??= item.name;
      if (entry.mode === TOOL_MODE) tools.push({ dir, entry, directoryName: item.name });
      else if (!ENTRY_MODES.includes(entry.mode)) {
        console.warn(`WARN ${entry.name}: skipped mode ${entry.mode ?? "missing"}`);
        skipped.push({ name: entry.name, reason: `mode ${entry.mode ?? "missing"} is not batch` });
      } else loaded.push({ dir, entry, directoryName: item.name, nameDeclared });
    } catch (error) {
      console.warn(`WARN ${item.name}: skipped invalid entry.json: ${error.message}`);
      skipped.push({ name: item.name, reason: `invalid entry.json: ${error.message}` });
    }
  }
  return { loaded, tools, skipped };
}

// The command that runs a tool's original: the interpreter its extension
// names, else the file itself through its `#!` line.
function toolCommand(script) {
  return [...(TOOL_INTERPRETERS[path.extname(script)] ?? []), script];
}

// `argv` with a case's `env` set inside JetEnv, where it wins and where JetEnv
// never sees it. JetEnv applies the dev shell's variables over the caller's the
// way `nix develop` does, so the shell's CC would replace a case's `CC`, and it
// refuses a scratch variable on /tmp, which a case may set to prove the program
// refuses it; env(1) inside JetEnv sets the case's variables last.
function withCaseEnv(env, argv) {
  const pairs = Object.entries(env).map(([name, value]) => `${name}=${value}`);
  return pairs.length === 0 ? argv : ["env", "--", ...pairs, ...argv];
}

// Runs a tool entry's parity cases (Tools/agent/ScriptPairing.jet checks the
// entry's shape). Each case runs the original (`tool.original`) and the Jet
// twin (`tool.jet`), both paths under `root` (the repository), with the
// case's `args` and `env` (variables over the harness environment and over
// the dev shell's, which JetEnv applies: the case's always win, and JetEnv
// itself never sees them, so what a case proves is the program's behaviour);
// each must exit with the case's `exit` (0 when it names none) and do the
// job. A case with a `check` (a Jet program in the entry) proves the job:
// after each program, `jetBin CHECK STDOUT` runs in the case directory, with
// the case's `env` too, STDOUT being a file holding the program's output, and
// must exit 0 and print the case's `expected` file byte for byte; a case
// without one compares the program's own output with `expected` byte for
// byte. A `jetBin` given as a path is also the
// `jet` the scripts run: its directory goes first on PATH and it is $JET,
// which the dev shell's `jet` runs (a case's `env` may still set either); a
// bare name is left to the caller's PATH. Every run starts in its own fresh
// copy, under `workDir`, of the entry's `tool.fixtures` directory (an empty
// directory when it has none; symbolic links are copied as they are, so a
// relative one still points inside the copy), so a tool that writes files
// changes neither the entry nor the other run.
async function runToolEntry(dir, entry, jetBin, workDir, root = repoDir) {
  const tool = isObject(entry.tool) ? entry.tool : {};
  const issues = [];
  for (const field of ["original", "jet"]) {
    if (typeof tool[field] !== "string" || tool[field] === "") issues.push(`tool.${field} is not a path`);
    else if (!(await exists(path.join(root, tool[field])))) issues.push(`tool.${field} \`${tool[field]}\` does not exist`);
  }
  if (!Array.isArray(tool.cases) || tool.cases.length === 0) issues.push("tool.cases is empty");
  const provenance = {
    entry_json_sha256: await fileSha256(path.join(dir, "entry.json")),
    corpus_tree_sha256: await treeSha256(dir),
  };
  if (issues.length) return { name: entry.name, original: tool.original ?? null, jet: tool.jet ?? null, status: "broken", reason: issues.join("; "), cases: [], provenance };
  const fixtures = typeof tool.fixtures === "string" ? path.join(dir, tool.fixtures) : null;
  const named = jetBin.includes(path.sep) ? path.resolve(jetBin) : null;
  const inner = named === null ? {} : { PATH: `${path.dirname(named)}${path.delimiter}${process.env.PATH ?? ""}`, JET: named };
  const programs = {
    original: toolCommand(path.join(root, tool.original)),
    // `--` ends jet's own options, so a case's args reach the twin verbatim,
    // a leading `--` included.
    jet: [jetBin, path.join(root, tool.jet), "--"],
  };
  const cases = [];
  for (const [index, item] of tool.cases.entries()) {
    const args = Array.isArray(item?.args) ? item.args.map(String) : [];
    const env = isObject(item?.env) ? Object.fromEntries(Object.entries(item.env).map(([name, value]) => [name, String(value)])) : {};
    const exitCode = Number.isInteger(item?.exit) ? item.exit : 0;
    const expectedPath = path.join(dir, String(item?.expected ?? ""));
    const expected = await exists(expectedPath) ? await fs.readFile(expectedPath) : null;
    const row = { case: index + 1, args, env, exit: exitCode, expected: item?.expected ?? null };
    for (const [side, command] of Object.entries(programs)) {
      if (expected === null) {
        row[side] = { status: "fail", reason: `expected output \`${item?.expected ?? ""}\` does not exist` };
        continue;
      }
      const cwd = path.join(workDir, `case-${index + 1}-${side}`);
      await fs.rm(cwd, { recursive: true, force: true });
      if (fixtures === null) await fs.mkdir(cwd, { recursive: true });
      else await fs.cp(fixtures, cwd, { recursive: true, verbatimSymlinks: true });
      const result = await runProcess(cwd, withCaseEnv(env, [...command, ...args]), { env: inner });
      let reason = result.code !== exitCode
        ? `exit ${result.code}, not ${exitCode}: ${result.stderr.toString("utf8").trim().slice(0, 300)}`
        : null;
      if (reason === null && typeof item?.check === "string" && item.check !== "") {
        const printed = `${cwd}.stdout`;
        await fs.writeFile(printed, result.stdout);
        const checked = await runProcess(cwd, withCaseEnv(env, [jetBin, path.join(dir, item.check), printed]), { env: inner });
        await fs.rm(printed, { force: true });
        reason = checked.code !== 0
          ? `its check \`${item.check}\` fails (exit ${checked.code}): ${checked.stderr.toString("utf8").trim().slice(0, 300)}`
          : mismatch(expected, checked.stdout);
      } else if (reason === null) {
        reason = mismatch(expected, result.stdout);
      }
      row[side] = reason === null ? { status: "pass" } : { status: "fail", reason };
    }
    row.status = row.original.status === "pass" && row.jet.status === "pass" ? "pass" : "fail";
    cases.push(row);
  }
  const status = cases.every((row) => row.status === "pass") ? "pass" : "fail";
  return { name: entry.name, original: tool.original, jet: tool.jet, status, cases, provenance };
}

async function readLiveTowerCards() {
  if (process.env.JET_GAUNTLET_DISABLE_TOWER_IO === "1") {
    return { status: "unavailable", reason: "Tower I/O disabled by caller" };
  }
  const towerPath = path.join(repoDir, "Tools/tower/.tower/tower.json");
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
  if (comparable.some((verdict) => !["win", "loss"].includes(verdict))) {
    return "unmeasured";
  }
  if (comparable.includes("loss")) return "loss";
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

// An entry's `run_reference` names the language row holding a minimal
// hand-written program for the same answer. Jet's run tier, in every gated
// cache state, must finish within PEER_MEASUREMENT_POLICY.run_reference_margin_seconds
// of that program's wall time. Null when the entry names no reference.
function runReferenceVerdict(entry, rows, tiers) {
  const reference = entry?.run_reference;
  if (typeof reference !== "string") return null;
  const margin = PEER_MEASUREMENT_POLICY.run_reference_margin_seconds;
  const row = rows?.[reference];
  const referenceWall = row?.status === "ok" && positiveMetricValue("runtime_wall_seconds", row.metrics?.runtime_wall_seconds)
    ? row.metrics.runtime_wall_seconds
    : null;
  const cacheStates = Object.fromEntries(RUN_CACHE_STATES.gated.map((state) => {
    const measured = tiers?.run?.status === "ok" ? tiers.run.cache_states?.[state] : null;
    const jet = measured?.status === "ok" && positiveMetricValue("runtime_wall_seconds", measured.metrics?.runtime_wall_seconds)
      ? measured.metrics.runtime_wall_seconds
      : null;
    const limit = referenceWall === null ? null : referenceWall + margin;
    return [state, {
      jet,
      reference: referenceWall,
      limit,
      verdict: jet === null || limit === null ? "unmeasured" : (jet <= limit ? "win" : "loss"),
    }];
  }));
  const states = Object.entries(cacheStates);
  const unmeasured = states.find(([, item]) => item.verdict === "unmeasured");
  const lost = states.filter(([, item]) => item.verdict === "loss");
  const verdict = unmeasured ? "unmeasured" : (lost.length ? "loss" : "win");
  return {
    reference,
    margin_seconds: margin,
    cache_states: cacheStates,
    verdict,
    reason: unmeasured
      ? `uncovered: no ${unmeasured[0]} run-tier reading for jet or no ${reference} reference reading`
      : lost.length
        ? lost.map(([state, item]) => `${state} run tier ${item.jet.toFixed(6)}s is more than ${margin}s over ${reference} ${item.reference.toFixed(6)}s`).join("; ")
        : null,
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
      ? ratioVerdict(item.ratio)
      : null;
    const valid = item?.status === "measured" &&
      expectedVerdict !== null &&
      item.verdict === expectedVerdict;
    if (!item || !valid) {
      const unread = PEER_FLOOR_METRICS.includes(metric)
        ? [["jet", item?.jet], [language, item?.peer]].filter(([, value]) => !positiveMetricValue(metric, value)).map(([name]) => name)
        : [];
      return [tier, {
        ...(item ?? {}),
        status: "unmeasured",
        jet: item?.jet ?? null,
        peer: item?.peer ?? null,
        ratio: item?.ratio ?? null,
        verdict: null,
        reason: unread.length
          ? `uncovered: no ${tier} ${metric} reading for ${unread.join(" and ")}`
          : item?.reason ?? "missing or invalid metric comparison",
      }];
    }
    if (item.verdict === "loss") {
      return [tier, { ...item, status: "measured", reason: `${tier} Jet/${language} ${item.ratio.toFixed(3)} is at or above 1.00` }];
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
  if (matrix && Array.isArray(entry.cells) && Array.isArray(matrix.cells)) {
    const expectedMatrixPeers = new Set(entry.cells.flatMap((cellId) => {
      const matrixCell = matrix.cells.find((candidate) => candidate?.id === cellId);
      return matrixCell ? matrixPeerLanguages(matrix, matrixCell) : [];
    }));
    for (const peer of expectedMatrixPeers) {
      if (!languages.includes(peer)) issues.push(`${name}/${peer}: required matrix rail is missing`);
    }
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
        for (const state of RUN_CACHE_STATES.gated) {
          if (tierResult.cache_states?.[state]?.status !== "ok") issues.push(`${name}/jet/run/${state}: gated cache state is missing or not measured`);
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
    const expectedComparisonPolicy = peerTierPolicy(entry, language);
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
        if (subMillisecondMetric(metric, item?.jet, item?.peer)) {
          const evidenceReady = item?.evidence?.jet?.confidence?.status === "available" &&
            item?.evidence?.peer?.confidence?.status === "available" &&
            item?.evidence?.jet?.noise && item?.evidence?.peer?.noise &&
            item?.evidence?.jet?.raw_samples?.length > 0 && item?.evidence?.peer?.raw_samples?.length > 0;
          if (!evidenceReady || item?.status !== "measured" || item?.verdict === null) {
            issues.push(`${name}/${language}/${tier}/${metric}: sub-millisecond comparison lacks strict confidence evidence`);
            continue;
          }
        }
        if (item?.status !== "measured" || !positiveMetricValue(metric, item?.jet) ||
          !positiveMetricValue(metric, item?.peer) || !Number.isFinite(item?.ratio) ||
          item.ratio !== item.jet / item.peer || item.verdict !== ratioVerdict(item.ratio)) {
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
// `ceiling` carries the machine-ceiling references (Tools/perf/asm-parity/cells.json)
// and the asm-parity results.json that measured them (null when not measured).
function buildScoreboard(matrix, results, manifest, tower, ceiling = { references: [], measurements: null }) {
  const inputResults = Array.isArray(results) ? results : [];
  const resultValidationIssues = inputResults.flatMap((result) => validateResultShape(result, matrix));
  const policyValidationIssues = strictPerformancePolicyIssues(matrix);
  const allowedUncovered = new Set(manifest?.corpus?.allowed_uncovered_cells ?? MATRIX_UNCOVERED_DEFAULTS);
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
  const requiredCells = requiredPolicyCells(matrix?.strict_performance_policy, (Array.isArray(matrix?.cells) ? matrix.cells : []).map((cell) => cell.id));
  const cells = (Array.isArray(matrix?.cells) ? matrix.cells : []).map((cell) => {
    const candidates = entriesByCell.get(cell.id) ?? [];
    const records = candidates.map((result) => {
      const metric = primaryMetric(result.entry.mode);
      const metrics = comparisonMetrics(result.entry.mode);
      const jetTierPolicy = tierPolicy(result.entry.mode);
      const requiredTiers = Object.entries(jetTierPolicy)
        .filter(([, policy]) => policy.required)
        .map(([tier]) => tier);
      const declaredLanguages = (Array.isArray(result.entry.languages) ? result.entry.languages : [])
        .filter((language) => typeof language === "string" && language !== "jet" && language !== "jet-expert");
      // A run-rail peer the entry has no program for stays in the record, unmeasured.
      const runRail = Array.isArray(matrix?.rails?.run) ? matrix.rails.run : [];
      const missingRunPeers = Object.hasOwn(jetTierPolicy, "run")
        ? runRail.filter((language) => !declaredLanguages.includes(language))
        : [];
      const peers = [...declaredLanguages, ...missingRunPeers]
        .map((language) => {
          const comparison = result.comparisons?.[language];
          const comparisonPolicy = comparison?.tier_policy ?? peerTierPolicy(result.entry, language);
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
            .filter(([, item]) => !["win", "not_applicable"].includes(item.verdict))
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
      const runReference = runReferenceVerdict(result.entry, result.rows, result.jet_tiers);
      const metricVerdicts = Object.fromEntries(metrics.map((item) => [
        item,
        cellVerdict([
          ...applicablePeers.map((peer) => peer.metric_verdicts[item] ?? null),
          ...(runReference && item === "runtime_wall_seconds" ? [runReference.verdict] : []),
        ]),
      ]));
      const metricFailures = Object.entries(metricVerdicts)
        .filter(([, verdict]) => !["win", "not_applicable"].includes(verdict))
        .map(([item, verdict]) => {
          const peerFailures = applicablePeers.flatMap((peer) => peer.metric_failures.filter((failure) => failure.metric === item))
            .concat(runReference && item === "runtime_wall_seconds" && runReference.verdict !== "win"
              ? [{ metric: item, peer: runReference.reference, verdict: runReference.verdict, reason: runReference.reason, tiers: { run: runReference.cache_states }, owner: runReference.verdict === "loss" ? liveLossOwner(declaredOwners[result.entry.name], item, towerState) : null }]
              : []);
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
        run_reference: runReference,
        metric_verdicts: metricVerdicts,
        metric_failures: metricFailures,
        loss_owners: lossOwners,
        verdict,
      };
    });
    const structuralMetricNames = comparisonMetrics(matrixCellMode(cell));
    const structuralFailureReason = candidates.length === 0
      ? "matrix cell has no corpus entry; peer and metric evidence are missing"
      : `matrix cell has ${candidates.length} corpus entries; exactly one is required`;
    const structuralFailures = (candidates.length !== 1 && !allowedUncovered.has(cell.id))
      ? (candidates.length === 0
        ? missingMatrixCellFailures(matrix, cell)
        : matrixPeerLanguages(matrix, cell).flatMap((peer) => structuralMetricNames.map((metric) => ({
          metric,
          peer,
          verdict: "unmeasured",
          reason: structuralFailureReason,
          tiers: {},
        }))))
      : [];
    const structuralMetricVerdicts = candidates.length === 1
      ? records[0].metric_verdicts
      : (allowedUncovered.has(cell.id)
        ? {}
        : Object.fromEntries(structuralMetricNames.map((metric) => [metric, "unmeasured"])));
    // The peer floor passes or fails the cell; the machine ceiling is measured
    // and reported only (CEILING_POLICY.ceiling_gate).
    const peerVerdict = records.length === 1 ? records[0].verdict : "unmeasured";
    const ceilingRecord = ceilingCell(cell, ceiling?.references, ceiling?.measurements ?? null, requiredCells.has(cell.id));
    return {
      id: cell.id,
      domain: cell.domain,
      kind: cell.kind,
      entries: records,
      metric_verdicts: structuralMetricVerdicts,
      metric_failures: records.flatMap((record) => record.metric_failures).concat(structuralFailures),
      peer_verdict: peerVerdict,
      ceiling: ceilingRecord,
      ceiling_verdict: ceilingRecord.verdict,
      verdict: peerVerdict,
      loss_owners: records.flatMap((record) => record.loss_owners),
    };
  });
  const verdicts = cells.map((cell) => cell.verdict);
  const metricVerdicts = cells.flatMap((cell) => Object.values(cell.metric_verdicts));
  return {
    strict_performance_policy: STRICT_PERFORMANCE_POLICY,
    contract: "gauntlet-scoreboard-v1",
    primary_metric_by_mode: MODE_PRIMARY_METRIC,
    verdict_policy: { ...RATIO_VERDICTS, unmeasured: "missing row, tier, metric, or byte verification" },
    validation_issues: [...new Set([...resultValidationIssues, ...policyValidationIssues])],
    cells,
    summary: {
      cells: cells.length,
      win: verdicts.filter((verdict) => verdict === "win").length,
      loss: verdicts.filter((verdict) => verdict === "loss").length,
      unmeasured: verdicts.filter((verdict) => verdict === "unmeasured").length,
      unmeasured_allowed: cells.filter((cell) => cell.verdict === "unmeasured" && allowedUncovered.has(cell.id)).length,
      unmeasured_required: cells.filter((cell) => cell.verdict === "unmeasured" && !allowedUncovered.has(cell.id)).length,
      metric_win: metricVerdicts.filter((verdict) => verdict === "win").length,
      metric_loss: metricVerdicts.filter((verdict) => verdict === "loss").length,
      metric_unmeasured: metricVerdicts.filter((verdict) => verdict === "unmeasured").length,
      metric_not_applicable: metricVerdicts.filter((verdict) => verdict === "not_applicable").length,
      ceiling_meet: cells.filter((cell) => cell.ceiling_verdict === "meet").length,
      ceiling_miss: cells.filter((cell) => cell.ceiling_verdict === "miss").length,
      ceiling_uncovered: cells.filter((cell) => cell.ceiling_verdict === "uncovered").length,
      ceiling_unmeasured: cells.filter((cell) => !["meet", "miss", "uncovered"].includes(cell.ceiling_verdict)).length,
    },
    ceiling_coverage: {
      references: CEILING_POLICY.references,
      covered: cells.filter((cell) => cell.ceiling.status === "covered").map((cell) => ({ cell: cell.id, references: cell.ceiling.references })),
      uncovered: cells.filter((cell) => cell.ceiling.status === "uncovered").map((cell) => cell.id),
      measured: Boolean(ceiling?.measurements),
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
    const versionResult = await runProcess(cwd, toolVersionCommand(tool), { timeoutMs: 10_000 });
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
      const stopped = await stopProcess(child);
      sample.process_exit = stopped.exit;
      if (stopped.forced) sample.cleanup_forced = true;
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
      else if (id === "compile_throughput") axes[id] = await runCompileThroughputAxisAdapter(axis, { runDir, jetBin, repoDir });
      else if (id === "build_system") axes[id] = await runBuildSystemAxisAdapter(axis, { runDir, jetBin, repoDir });
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
  blockers.push(...strictPerformancePolicyIssues(matrix));
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
      const peerFailures = Array.isArray(failure.peers) ? failure.peers : [];
      if (peerFailures.length > 0) {
        for (const peerFailure of peerFailures) {
          const reasons = [peerFailure.reason, ...(failure.reasons ?? [])].filter(Boolean);
          const detail = reasons.length ? ` (${[...new Set(reasons)].join("; ")})` : "";
          blockers.push(`${cell.id}/${peerFailure.peer ?? "peer"}/${failure.metric ?? "metric"}: metric ${peerFailure.verdict ?? failure.verdict ?? "unmeasured"}${detail}`);
        }
      } else {
        const reasons = failure.reason ? ` (${failure.reason})` : "";
        blockers.push(`${cell.id}/${failure.peer ?? "peer"}/${failure.metric ?? "metric"}: metric ${failure.verdict ?? "unmeasured"}${reasons}`);
      }
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

// The compiler under measurement: --jet-bin, else $JET (what `Tools/agent/JetEnv.jet jet`
// runs), else `jet` on PATH. It runs in place, never copied: a Jet-built compiler reads
// its runtime pack beside itself (jet-runtime/), and the run records its SHA-256. A
// missing compiler is an error.
async function resolveJetBinary(options) {
  if (options.jetBin) return path.resolve(process.cwd(), options.jetBin);
  const candidates = process.env.JET
    ? [path.resolve(process.cwd(), process.env.JET)]
    : (process.env.PATH ?? "").split(path.delimiter).filter(Boolean).map((directory) => path.join(directory, "jet"));
  for (const candidate of candidates) {
    if (await exists(candidate)) return await fs.realpath(candidate);
  }
  throw new Error(`missing Jet compiler: ${process.env.JET ? `JET=${process.env.JET} does not exist` : "JET is unset and no jet is on PATH"} (set JET, put jet on PATH, or pass --jet-bin)`);
}

async function devAvailable(jetBin, runDir) {
  const result = await runProcess(runDir, [jetBin, "dev", "--help"]);
  return result.code === 0;
}

async function toolchainFingerprint(runDir, jetBin) {
  const commands = {
    python: toolVersionCommand("python3"),
    rust: toolVersionCommand("rustc"),
    c: toolVersionCommand("gcc"),
    zig: toolVersionCommand("zig"),
    go: toolVersionCommand("go"),
    js: toolVersionCommand("node"),
    node: toolVersionCommand("node"),
    lua: toolVersionCommand("lua"),
    luajit: toolVersionCommand("luajit"),
    bun: toolVersionCommand("bun"),
    vite: toolVersionCommand("vite"),
    asm: toolVersionCommand("as"),
    perf: toolVersionCommand("perf"),
    strip: toolVersionCommand("strip"),
    ldd: toolVersionCommand("ldd"),
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

// The commit a run measured: the checkout's HEAD, null outside a git checkout.
// Tower's Gauntlet page names it beside the run's date.
async function checkoutCommit() {
  const head = await runProcess(repoDir, ["git", "rev-parse", "HEAD"], { timeoutMs: 10_000 });
  return head.code === 0 ? head.stdout.toString("utf8").trim() || null : null;
}

// Each comparison entry's standings plan: its programs for every standings
// peer, the asm-parity reference naming one of its cells, and the toolchains.
async function standingsPlans(loaded, policy, references, toolchains) {
  const plans = [];
  const referenceDir = path.dirname(CEILING_POLICY.references);
  for (const item of loaded) {
    const cells = item.entry.cells ?? [];
    const entryReferences = (Array.isArray(references) ? references : [])
      .filter((reference) => Array.isArray(reference?.[CEILING_POLICY.reference_link]) && reference[CEILING_POLICY.reference_link].some((cell) => cells.includes(cell)));
    const programs = {};
    for (const peer of policy.peers) {
      const dir = LANGUAGE_SOURCE_DIRS[peer] ?? peer;
      const file = LANGUAGE_FILES[peer];
      programs[peer] = { path: `${dir}/${file}`, exists: Boolean(file) && await exists(path.join(item.dir, dir, file)) };
    }
    if (entryReferences.length === 1) {
      const program = `${referenceDir}/${entryReferences[0].name}/${CEILING_POLICY.reference_source}`;
      programs[policy.ceiling] = { path: program, exists: await exists(path.join(repoDir, program)) };
    }
    const plan = planEntry(item.entry, policy, { programs, references: entryReferences, toolchains });
    const ceiling = plan.peers[policy.ceiling];
    plans.push({
      item,
      plan,
      stage: {
        peers: policy.peers.filter((peer) => plan.peers[peer].plan === "measure"),
        ceiling: ceiling.plan === "measure" ? { name: ceiling.reference, dir: path.join(repoDir, referenceDir, ceiling.reference) } : null,
      },
    });
  }
  return plans;
}

// The standings peers' toolchains plus the measuring tools: perf counts
// cycles, strip and ldd give the deployed size.
function standingsToolchains(policy, toolchains) {
  return Object.fromEntries([...policy.peers, policy.ceiling, "perf", "strip", "ldd"].filter((tool) => toolchains[tool]).map((tool) => [tool, toolchains[tool]]));
}

// --dry-run: every entry x peer x metric standings cell the run would fill,
// with its planned status, and nothing built or measured.
async function dryRun(options, matrix, entriesDir) {
  const manifest = JSON.parse(await fs.readFile(path.join(repoDir, "Tools/gauntlet/measurement-manifest.json"), "utf8"));
  const policy = standingsPolicy(matrix, manifest);
  for (const issue of policy.issues) console.warn(`WARN standings: ${issue}`);
  const references = JSON.parse(await fs.readFile(path.join(repoDir, CEILING_POLICY.references), "utf8"));
  const toolchains = await toolchainFingerprint(repoDir, null);
  const { loaded, tools, skipped } = await loadEntries(entriesDir, options.entry);
  const plans = await standingsPlans(loaded, policy, references, toolchains);
  const cells = planCells(plans.map(({ plan }) => plan));
  for (const [basis, definition] of Object.entries(policy.size_basis)) console.log(`size-basis\t${basis}\t${definition}`);
  console.log(["cell", "entry", "mode", "peer", "role", "metric", "status", "program", "size basis (jet / peer)", "reason"].join("\t"));
  for (const cell of cells) {
    console.log(["cell", cell.entry, cell.mode, cell.peer, cell.role, cell.metric, cell.status, cell.program ?? "-", cell.size_basis ? `${cell.jet_size_basis} / ${cell.size_basis}` : "-", cell.reason ?? "-"].join("\t"));
  }
  for (const [tool, row] of Object.entries(standingsToolchains(policy, toolchains))) {
    console.log(`toolchain\t${tool}\t${row.status}\t${row.version || "-"}`);
  }
  for (const item of skipped) console.log(`skipped\t${item.name}\t${item.reason ?? "-"}`);
  const counts = {};
  for (const cell of cells) counts[cell.status] = (counts[cell.status] ?? 0) + 1;
  console.log(`dry-run\tentries ${loaded.length}\tpeers ${policy.peers.length}+${policy.ceiling}\tmetrics 5\tcells ${cells.length}\t${Object.entries(counts).map(([status, count]) => `${status} ${count}`).join(", ")}\ttool entries ${tools.length} (judged by PortBars, not standings)`);
  if (policy.issues.length) process.exitCode = 1;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const matrixPath = path.join(repoDir, "Tools/gauntlet/matrix.json");
  const matrixText = await fs.readFile(matrixPath, "utf8");
  const matrix = JSON.parse(matrixText);
  const entriesDir = path.resolve(process.cwd(), options.entriesDir ?? path.join(repoDir, "Tools/gauntlet/entries"));
  if (options.dryRun) return dryRun(options, matrix, entriesDir);
  const defaultEntriesDir = path.resolve(path.join(repoDir, "Tools/gauntlet/entries"));
  const axisOnly = options.axis !== null;
  // A --languages run measures some rows only; merged into an earlier run's
  // other rows (--merge) it is a whole report again and publishes as one.
  const measure = options.languages === null ? null : new Set(options.languages);
  const mergeSource = options.merge === null ? null : await loadMergeSource(path.resolve(process.cwd(), options.merge));
  const fullScope = !axisOnly && entriesDir === defaultEntriesDir && options.entry === null && (measure === null || mergeSource !== null);
  const manifestPath = path.join(repoDir, "Tools/gauntlet/measurement-manifest.json");
  const sourceManifest = entriesDir === defaultEntriesDir
    ? JSON.parse(await fs.readFile(manifestPath, "utf8"))
    : null;
  const sourceMeasurements = !axisOnly && sourceManifest ? await measureSourceManifest(entriesDir, sourceManifest, matrix) : null;
  const runId = `${dateStamp().replaceAll("-", "")}-${process.pid}-${Date.now().toString(36)}`;
  const runDir = path.join(stateRoot(), "gauntlet/work", runId);
  await fs.mkdir(runDir, { recursive: true });
  await buildLauncher(runDir);
  const jetBin = await resolveJetBinary(options);
  const toolchains = await toolchainFingerprint(runDir, jetBin);
  const dev = await devAvailable(jetBin, runDir);
  if (!dev) console.warn("WARN jet dev unavailable; skipping Jet dev tier");
  const { loaded, tools, skipped } = axisOnly
    ? { loaded: [], tools: [], skipped: [] }
    : await loadEntries(entriesDir, options.entry);
  // The machine-ceiling references and, when given, the asm-parity run that measured them.
  const ceilingReferences = JSON.parse(await fs.readFile(path.join(repoDir, CEILING_POLICY.references), "utf8"));
  const ceilingMeasurementsGiven = options.ceilingResults === null
    ? null
    : JSON.parse(await fs.readFile(path.resolve(process.cwd(), options.ceilingResults), "utf8"));
  const standingsSpec = standingsPolicy(matrix, sourceManifest ?? JSON.parse(await fs.readFile(manifestPath, "utf8")));
  const plans = await standingsPlans(loaded, standingsSpec, ceilingReferences, toolchains);
  const validationIssues = axisOnly
    ? []
    : [
      ...await validateCorpus(entriesDir, loaded, skipped, matrix, sourceManifest, fullScope),
      ...await ceilingReferenceIssues(matrix, ceilingReferences),
      ...standingsSpec.issues.map((issue) => `standings: ${issue}`),
    ];
  for (const issue of validationIssues) console.warn(`WARN corpus: ${issue}`);
  const results = [];
  for (const item of loaded) {
    console.error(`gauntlet: entry ${item.entry.name} [${(item.entry.languages ?? []).join(",")}] ...`);
    try {
      const result = await stageEntry(item.dir, item.entry, runDir, jetBin, options.runs, dev, plans.find((plan) => plan.item === item).stage, measure);
      results.push(mergeSource ? mergeEarlierResult(result, mergeSource.entries.get(item.entry.name) ?? null, mergeSource) : result);
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
  // Interpreted peers count their runtime in their size: measure each
  // interpreter's footprint once per run.
  const runtimes = {};
  if (!axisOnly) {
    for (const [peer, interpreter] of Object.entries(STANDINGS_INTERPRETERS)) {
      if (standingsSpec.peers.includes(peer)) runtimes[peer] = await interpreterFootprint(runDir, interpreter);
    }
  }
  const standings = axisOnly ? null : buildStandings(
    plans.map(({ item, plan }) => entryStandings(plan, results.find((result) => result.entry === item.entry), { runtimes })),
    standingsSpec,
    { runtimes, toolchains: standingsToolchains(standingsSpec, toolchains) },
  );
  // Without an asm-parity results file, this run's own asm and Jet rows are
  // the scoreboard's machine-ceiling measurements.
  const ceilingMeasurements = ceilingMeasurementsGiven ?? (standings ? standingsCeilingMeasurements(standings.entries) : null);
  const toolResults = [];
  for (const item of tools) {
    console.error(`gauntlet: tool ${item.entry.name} ...`);
    try {
      toolResults.push(await runToolEntry(item.dir, item.entry, jetBin, path.join(runDir, "tools", item.directoryName)));
    } catch (error) {
      console.error(`gauntlet: tool ${item.entry.name} CRASHED: ${error.message}`);
      toolResults.push({ name: item.entry.name, original: item.entry.tool?.original ?? null, jet: item.entry.tool?.jet ?? null, status: "broken", reason: `harness error: ${error.message}`, cases: [] });
    }
    console.error(`gauntlet: tool ${item.entry.name} done`);
  }
  // `--entry` naming a tool entry runs only its parity cases, and the exit
  // status is their verdict. A partial run of program entries cannot
  // publish, so its exit status is its own verdict: the corpus checks found
  // nothing, every language row of every entry it ran is ok, and its tools
  // pass. Full and axis runs exit by the publication rule.
  const toolOnly = options.entry !== null && loaded.length === 0 && tools.length > 0;
  const partial = !fullScope && !axisOnly && !toolOnly;
  const covered = axisOnly ? new Set() : new Set(results.flatMap((result) => result.entry.cells ?? []));
  const uncovered = axisOnly ? [] : (matrix.cells ?? []).map((cell) => cell.id).filter((id) => !covered.has(id));
  const tower = axisOnly ? null : await readLiveTowerCards();
  const scoreboard = axisOnly
    ? { strict_performance_policy: STRICT_PERFORMANCE_POLICY, primary_metric_by_mode: MODE_PRIMARY_METRIC, verdict_policy: RATIO_VERDICTS, summary: {}, cells: [] }
    : buildScoreboard(matrix, results, sourceManifest, tower, { references: ceilingReferences, measurements: ceilingMeasurements });
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
    options: { entry: options.entry, axis: options.axis, jet_bin: jetBin, runs: options.runs, ceiling_results: options.ceilingResults, languages: options.languages, merge: options.merge, scope: axisOnly ? `axis_${options.axis}` : (fullScope ? "full_matrix" : "partial_entry") },
    ...(mergeSource ? { merged_from: { path: mergeSource.path, run_id: mergeSource.run_id, generated: mergeSource.generated, commit: mergeSource.commit, jet_binary_sha256: mergeSource.jet_binary_sha256, toolchains: mergeSource.toolchains } } : {}),
    matrix_version: matrix.version,
    matrix_rails: matrix.rails,
    strict_performance_policy: STRICT_PERFORMANCE_POLICY,
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
    standings,
    publication,
    provenance: {
      matrix_sha256: sha256(matrixText),
      measurement_manifest_sha256: sourceManifest ? sha256(await fs.readFile(manifestPath)) : null,
      corpus_tree_sha256: await treeSha256(entriesDir),
      commit: await checkoutCommit(),
      jet_binary_sha256: await fileSha256(jetBin),
      toolchains,
      host: { platform: process.platform, arch: process.arch, node: process.version },
    },
    reproducibility: {
      expected_output: "byte-exact UTF-8 stdout or declared service probe sequence",
      source_metric_token_definition: sourceManifest?.contract?.token_definition ?? null,
      tier_policy_by_mode: Object.fromEntries(Object.entries(TIER_POLICY).map(([mode, policy]) => [mode, Object.keys(policy)])),
      ratio_verdicts: RATIO_VERDICTS,
      strict_performance_policy: STRICT_PERFORMANCE_POLICY,
      metric_applicability: METRIC_APPLICABILITY_POLICY,
      peer_measurement: PEER_MEASUREMENT_POLICY,
      missing_metric_verdict: "unmeasured",
      run_count: options.runs ?? "entry perf policy (7 for perf, 3 otherwise)",
    },
    entries: results,
    tools: toolResults,
  };
  // One file per run. Full-matrix runs own the bare <date>.json, and a merged
  // run <date>-merged-<languages>.json beside the run it merged; partial and
  // axis runs carry their scope so a later partial run never overwrites the
  // day's full report (status.mjs merges every file in this directory).
  // A run with standings is copied to the machine's shared results directory,
  // where Tower's Gauntlet page finds a run made in any checkout.
  const resultDir = path.join(repoDir, "Tools/gauntlet/results");
  await fs.mkdir(resultDir, { recursive: true });
  const languagesTag = options.languages === null ? null : `languages-${options.languages.join("-")}`;
  const resultName = axisOnly
    ? `${dateStamp()}-axis-${options.axis}.json`
    : fullScope
      ? `${dateStamp()}${mergeSource ? `-merged-${languagesTag}` : ""}.json`
      : `${dateStamp()}-${[options.entry, languagesTag].filter(Boolean).join("-")}.json`;
  const resultPath = path.join(resultDir, resultName);
  const reportText = `${JSON.stringify(report, null, 2)}\n`;
  await fs.writeFile(resultPath, reportText);
  if (standings) {
    const sharedResultDir = path.join(stateRoot(), "gauntlet/results");
    await fs.mkdir(sharedResultDir, { recursive: true });
    await fs.writeFile(path.join(sharedResultDir, resultName), reportText);
  }
  const statusPath = path.join(repoDir, "Tools/gauntlet/status.json");
  if (fullScope) {
    const status = projectStatus(report, resultPath);
    applyStatusGate(status, matrix);
    await fs.writeFile(statusPath, `${JSON.stringify(status, null, 2)}\n`);
    console.log(`status\t${statusPath}`);
  } else if (axisOnly) {
    const status = await rebuildStatusFromResults(resultDir);
    applyStatusGate(status, matrix);
    await fs.writeFile(statusPath, `${JSON.stringify(status, null, 2)}\n`);
    console.log(`status\t${statusPath}`);
  } else {
    console.log(`status\tskipped partial scope; ${statusPath} was not overwritten`);
  }
  for (const result of results) {
    for (const language of result.languages ?? []) {
      const row = result.rows?.[language];
      const comparison = result.comparisons?.[language];
      const primaryVerdict = comparison?.metrics?.[comparison.primary_metric]?.verdict ?? null;
      const verdicts = primaryVerdict ?? "-";
      const metrics = result.entry.mode === "service"
        ? `startup=${row?.metrics?.startupSeconds ?? "-"},latency_ms=${row?.metrics?.latencyMs?.median ?? "-"}/${row?.metrics?.latencyMs?.p99 ?? "-"},rss_kb=${row?.metrics?.rssKb ?? "-"},clean_exit=${row?.metrics?.cleanExit ?? "-"}`
        : result.entry.mode === "web" || result.entry.mode === "web-app"
          ? `artifact_bytes=${row?.metrics?.artifact_bytes ?? "-"},wasm_bytes=${row?.metrics?.wasm_bytes ?? "-"},first_result_s=${row?.metrics?.runtime_first_stdout_seconds ?? "-"}`
          : "-";
      console.log(`${result.entry.name}\t${language}\t${row?.status ?? result.status}\t${row?.metrics?.runtime_wall_seconds ?? "-"}\t${row?.metrics?.cold_build_seconds ?? "-"}\t${verdicts || "-"}\t${metrics}`);
    }
  }
  for (const item of skipped) console.log(`${item.name}\t-\tskipped\t-\t-\t-`);
  for (const tool of toolResults) {
    console.log(`tool\t${tool.name}\t${tool.status}\t${tool.original ?? "-"}\t${tool.jet ?? "-"}`);
    if (tool.reason) console.log(`tool-reason\t${tool.name}\t${tool.reason}`);
    for (const row of tool.cases) {
      for (const side of ["original", "jet"]) {
        if (row[side].status !== "pass") console.log(`tool-case\t${tool.name}\t${row.case}\t${side}\t${row[side].reason}`);
      }
    }
  }
  if (sourceMeasurements) {
    console.log("source\tjet_loc\tpython_loc\tloc_ratio\tjet_source_tokens\tpython_source_tokens\tsource_token_delta");
    for (const item of sourceMeasurements.entries) {
      console.log(`${item.name}\t${item.jet.loc}\t${item.python?.loc ?? "-"}\t${item.comparison?.loc_ratio ?? "-"}\t${item.jet.source_tokens}\t${item.python?.source_tokens ?? "-"}\t${item.comparison?.source_token_delta ?? "-"}`);
    }
    const total = sourceMeasurements.aggregate;
    console.log(`source-total\t${total.jet.loc}\t${total.python.loc}\t${total.loc_ratio}\t${total.jet.source_tokens}\t${total.python.source_tokens}\t${total.source_token_delta}`);
  }
  if (standings) {
    for (const [basis, definition] of Object.entries(standings.size_basis)) console.log(`standings-size-basis\t${basis}\t${definition}`);
    for (const line of standingsLines(standings)) console.log(`standings\t${line}`);
    const { floor, ceiling } = standings.summary;
    console.log(`standings-summary\tfloor pass ${floor.pass} fail ${floor.fail} other ${floor.other}\tceiling meet ${ceiling.meet} miss ${ceiling.miss} other ${ceiling.other}`);
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
  const toolsPass = toolResults.every((tool) => tool.status === "pass");
  // A --languages run answers only for the rows it measured: a row it left
  // unmeasured or carried from --merge is no verdict of this run.
  const measuredHere = (row) => measure === null || !(row?.not_measured || row?.carried_from);
  const tiersReady = (result) => Object.entries(tierPolicy(result.entry.mode))
    .every(([tier, policy]) => !policy.required || !measuredHere(result.jet_tiers?.[tier]) || result.jet_tiers?.[tier]?.status === "ok");
  const rowsOk = results.every((result) => (measure === null ? result.status !== "broken" : !result.reason && tiersReady(result)) &&
    (result.languages ?? []).every((language) => !measuredHere(result.rows?.[language]) || result.rows?.[language]?.status === "ok"));
  if (toolOnly) process.exitCode = toolsPass ? 0 : 1;
  else if (partial) process.exitCode = validationIssues.length === 0 && skipped.length === 0 && results.length > 0 && rowsOk && toolsPass ? 0 : 1;
  else if (!publication.complete || !toolsPass) process.exitCode = 1;
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
  mergeEarlierResult,
  validateResultShape,
  buildScoreboard,
  publicationState,
  httpProbe,
  probeMatches,
  probeAxisTools,
  stageEntry,
  loadEntries,
  runToolEntry,
  STRICT_PERFORMANCE_POLICY,
  strictPerformancePolicyIssues,
  CEILING_POLICY,
  ceilingReferenceIssues,
  matrixPeerLanguages,
  LOSS_OWNER_CATEGORY_BY_METRIC,
  standingsPlans,
  elfLinkage,
};
