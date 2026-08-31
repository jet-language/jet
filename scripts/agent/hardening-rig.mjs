#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  closeSync,
  existsSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  DEFAULT_BATCH_SIZE,
  DEFAULT_CORPUS_LIMIT,
  MAX_BATCH_SIZE,
  MAX_TIMEOUT_MS,
  MUTATION_ARMS,
  batchMutations,
  bundleIdentity,
  checkJetSource,
  discoverCorpusSeeds,
  executeCase,
  makeResultBundle,
  serializeBundles,
  tierCommand,
} from "./hardening-oracle-layer.mjs";
import { hardeningDedupKey as buildHardeningDedupKey, redTeamMain } from "./hardening-red-team.mjs";
import { buildDashboard } from "./hardening-dashboard.mjs";

import {
  checkGrammarNegativeControls,
  diagnosticRegistryHash,
  deriveConstructManifest,
  constructManifestHash,
  generateTypedPrograms,
  runGrammarPrograms,
} from "./hardening-grammar-layer.mjs";
import {
  generatePropertyCases,
  propertyLayerSummary,
  runPropertyCases,
} from "./hardening-property-layer.mjs";
import {
  MUTATION_CATALOG,
  runMutationSensitivity,
} from "./hardening-mutation-layer.mjs";
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(SCRIPT_DIR, "../..");
const ROOT = resolve(process.env.JET_HARDENING_ROOT || DEFAULT_ROOT);
const HOME_DIR = process.env.HOME || os.homedir();
const CACHE_ROOT = resolve(process.env.JET_HARDENING_CACHE || join(HOME_DIR, ".cache/jet-hardening/v1"));
const SCRATCH_ROOT = resolve(
  process.env.JET_HARDENING_SCRATCH || process.env.JET_TEST_SCRATCH || join(HOME_DIR, ".cache/jet-test-scratch/hardening-rig"),
);
const TARGET_ROOT = join(ROOT, "target");
const JET_BINARY = join(TARGET_ROOT, "debug", "jet");
const JET_ENV = resolve(process.env.JET_HARDENING_JET_ENV || join(ROOT, "scripts/agent/jet-env"));
const TMP_GUARD = resolve(process.env.JET_HARDENING_TMP_GUARD || join(ROOT, "scripts/agent/tmp-guard.sh"));
const PROOF_PARALLEL = resolve(
  process.env.JET_HARDENING_PROOF_PARALLEL || join(ROOT, "scripts/agent/proof-parallel.sh"),
);
const UNIT_SOURCE_ROOT = join(SCRIPT_DIR, "user-service");
const SERVICE_NAME = "jet-hardening-rig.service";
const TIMER_NAME = "jet-hardening-rig.timer";
const STATE_PATH = join(CACHE_ROOT, "state.json");
const RESULT_PATH = join(CACHE_ROOT, "result.json");
const CYCLE_ROOT = join(CACHE_ROOT, "cycles");
const FAILURE_PATH = join(CACHE_ROOT, "failure.json");
const FAILURE_LOG_PATH = join(CACHE_ROOT, "logs/failure.log");
const INTERESTING_ROOT = join(CACHE_ROOT, "interesting");
const RIG_LEASE_PATH = join(CACHE_ROOT, "rig.lock");
const BUILD_LEASE_PATH = join(TARGET_ROOT, ".jet-hardening-build.lock");

const GIB = 1024 ** 3;
const MIN_MEMORY_GIB = 16;
const TARGET_CAP_BYTES = 80 * GIB;
const DEFAULT_ORACLE_TIMEOUT_MS = 30_000;
const DEFAULT_ORACLE_MAX_CASES = 128;
const TOWER_CLI = resolve(
  process.env.JET_HARDENING_TOWER_CLI || join(ROOT, "plugins/tower/tower.mjs"),
);
const TOWER_DATA = process.env.JET_HARDENING_TOWER_DATA
  ? resolve(ROOT, process.env.JET_HARDENING_TOWER_DATA)
  : null;
const CACHE_CAP_BYTES = 4 * GIB;
const INTERESTING_CAP_BYTES = 512 * 1024 ** 2;
const LOG_CAP_BYTES = 1024 ** 2;
const DEFAULT_TIMEOUT_MS = 95 * 60 * 1000;
const DEFAULT_BUILD_TIMEOUT_MS = 95 * 60 * 1000;
const DEFAULT_PROOF_TARGETS = ["dev_corpus_gate"];
const DEFAULT_SHARDS = ["fuzz_sema", "sema_soundness_differential"];
const MAX_TRANSITIONS = 96;
const MAX_CAPTURE_BYTES = 256 * 1024;
const TEST_MODE = process.env.JET_HARDENING_TEST_MODE === "1";

let currentRun = null;
let handlingSignal = false;
let requestedSignal = null;

class Refusal extends Error {
  constructor(reason, details = {}) {
    super(reason);
    this.reason = reason;
    this.details = details;
  }
}

class ChildFailure extends Error {
  constructor(label, result) {
    super(`${label} failed`);
    this.label = label;
    this.result = result;
  }
}

function now() {
  return new Date().toISOString();
}

function runId() {
  return `${now().replace(/[-:.TZ]/g, "").slice(0, 14)}-${process.pid}-${randomUUID().slice(0, 8)}`;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function sha256File(path) {
  try {
    return sha256(readFileSync(path));
  } catch {
    return null;
  }
}

function readJson(path) {
  if (!existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    return { __error: `unreadable JSON: ${error.message}` };
  }
}

function atomicWrite(path, contents) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.tmp-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  try {
    writeFileSync(temporary, contents, { mode: 0o600 });
    const fd = openSync(temporary, "r");
    try {
      fsyncSync(fd);
    } finally {
      closeSync(fd);
    }
    renameSync(temporary, path);
  } catch (error) {
    try {
      unlinkSync(temporary);
    } catch {
      // The original error is more useful than cleanup failure.
    }
    throw error;
  }
}

function atomicJson(path, value) {
  atomicWrite(path, `${JSON.stringify(value, null, 2)}\n`);
}

function cleanAtomicTemps() {
  for (const [directory, ownedTemp] of [
    [CACHE_ROOT, (name) => ["state.json", "result.json", "failure.json"].some((base) => name.startsWith(`${base}.tmp-`))],
    [CYCLE_ROOT, (name) => name.startsWith("cycle-") && name.includes(".json.tmp-")],
  ]) {
    if (!existsSync(directory)) continue;
    for (const name of readdirSync(directory)) {
      if (!ownedTemp(name)) continue;
      try {
        unlinkSync(join(directory, name));
      } catch {
        // A concurrent invocation owns it; its lease decides whether that is safe.
      }
    }
  }
}

function readProcStartTicks(pid) {
  try {
    const text = readFileSync(`/proc/${pid}/stat`, "utf8");
    const end = text.lastIndexOf(") ");
    if (end < 0) return null;
    const fields = text.slice(end + 2).trim().split(/\s+/);
    return fields[19] || null;
  } catch {
    return null;
  }
}

function processMatches(owner) {
  if (!owner || !Number.isInteger(owner.pid) || owner.pid <= 1) return false;
  try {
    process.kill(owner.pid, 0);
  } catch {
    return false;
  }
  if (!owner.start_ticks) return true;
  return readProcStartTicks(owner.pid) === String(owner.start_ticks);
}

function processGroupEntries(groupPid) {
  if (process.platform !== "linux") return [];
  const entries = [];
  let names;
  try {
    names = readdirSync("/proc").filter((name) => /^\d+$/.test(name));
  } catch {
    return entries;
  }
  for (const name of names) {
    try {
      const text = readFileSync(`/proc/${name}/stat`, "utf8");
      const end = text.lastIndexOf(") ");
      if (end < 0) continue;
      const fields = text.slice(end + 2).trim().split(/\s+/);
      if (Number(fields[2]) === groupPid) {
        entries.push({ pid: Number(name), state: fields[0] });
      }
    } catch {
      // Process exited during the scan.
    }
  }
  return entries;
}

function groupHasLiveProcess(groupPid) {
  const entries = processGroupEntries(groupPid);
  if (process.platform !== "linux") {
    try {
      process.kill(-groupPid, 0);
      return true;
    } catch {
      return false;
    }
  }
  return entries.some((entry) => entry.state !== "Z");
}

function delay(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function killProcessGroup(groupPid) {
  if (!Number.isInteger(groupPid) || groupPid <= 1 || groupPid === process.pid) return;
  if (!groupHasLiveProcess(groupPid)) return;
  try {
    process.kill(-groupPid, "SIGTERM");
  } catch {
    // The group can exit between the scan and signal.
  }
  await delay(250);
  if (groupHasLiveProcess(groupPid)) {
    try {
      process.kill(-groupPid, "SIGKILL");
    } catch {
      // The group can exit between the scan and signal.
    }
    await delay(100);
  }
}

function captureAppend(previous, chunk) {
  const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
  if (previous.length >= MAX_CAPTURE_BYTES) return previous;
  return Buffer.concat([previous, bytes.subarray(0, MAX_CAPTURE_BYTES - previous.length)]);
}

function resolveCommand(path) {
  return path.includes("/") ? resolve(path) : path;
}

function commandText(program, args) {
  return [program, ...args].map((arg) => JSON.stringify(arg)).join(" ");
}

async function runCommand(label, program, args, environment, timeoutMs = DEFAULT_TIMEOUT_MS, stdin = undefined) {
  const command = commandText(program, args);
  const childEnv = { ...process.env, ...environment };
  const input = stdin === undefined || stdin === null
    ? null
    : Buffer.isBuffer(stdin) || stdin instanceof Uint8Array
      ? Buffer.from(stdin)
      : Buffer.from(String(stdin), "utf8");
  const child = spawn(resolveCommand(program), args, {
    cwd: ROOT,
    detached: true,
    env: childEnv,
    stdio: [input === null ? "ignore" : "pipe", "pipe", "pipe"],
  });
  const record = {
    label,
    command,
    pid: child.pid || null,
    stdout: Buffer.alloc(0),
    stderr: Buffer.alloc(0),
    timed_out: false,
    exit: null,
    signal: null,
  };
  if (child.pid) currentRun?.children.set(child.pid, record);
  child.stdout?.on("data", (chunk) => {
    record.stdout = captureAppend(record.stdout, chunk);
  });
  child.stderr?.on("data", (chunk) => {
    record.stderr = captureAppend(record.stderr, chunk);
  });
  child.stdout?.on("error", () => {});
  child.stderr?.on("error", () => {});
  child.stdin?.on("error", () => {});
  if (input !== null) child.stdin?.end(input);

  let killPromise = null;
  child.once("exit", () => {
    if (child.pid && !killPromise) killPromise = killProcessGroup(child.pid);
  });
  const timer = setTimeout(() => {
    record.timed_out = true;
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {
      // The process can exit between the timeout and group signal.
    }
    try {
      child.kill("SIGTERM");
    } catch {
      // The process can exit between the timeout and direct signal.
    }
    child.stdin?.destroy();
    child.stdout?.destroy();
    child.stderr?.destroy();
    killPromise = killProcessGroup(child.pid);
  }, timeoutMs);
  const closed = await new Promise((resolvePromise) => {
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      resolvePromise(value);
    };
    child.once("error", (error) => finish({ error }));
    child.once("close", (exit, signal) => finish({ exit, signal }));
  });
  clearTimeout(timer);
  if (killPromise) await killPromise;

  record.exit = closed.exit ?? null;
  record.signal = closed.signal ?? null;
  if (child.pid) {
    await killProcessGroup(child.pid);
    currentRun?.children.delete(child.pid);
  }
  if (closed.error) record.error = closed.error.message;
  return {
    ...record,
    stdout_bytes: record.stdout.length,
    stderr_bytes: record.stderr.length,
    stdout_sha256: sha256(record.stdout),
    stderr_sha256: sha256(record.stderr),
    stdout_base64: record.stdout.toString("base64"),
    stderr_base64: record.stderr.toString("base64"),
    ok: !record.error && !record.timed_out && record.exit === 0 && !record.signal,
  };
}

function leaseOwner(path) {
  return readJson(join(path, "owner.json"));
}

function leaseDescription(owner) {
  if (!owner || owner.__error) return "lease owner unreadable";
  return `${owner.kind || "lease"} pid ${owner.pid || "?"}`;
}

function staleLeasePath(path, owner) {
  const pid = Number.isInteger(owner?.pid) ? owner.pid : "unknown";
  return `${path}.stale-${pid}-${process.pid}-${Date.now()}`;
}

function acquireLease(path, kind, onStale) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  try {
    mkdirSync(path, { mode: 0o700 });
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const owner = leaseOwner(path);
    if (processMatches(owner)) {
      throw new Refusal(`${kind} lease is active`, { lease: path, owner: leaseDescription(owner) });
    }
    if (!owner || owner.__error || !owner.pid) {
      throw new Refusal(`${kind} lease is present but cannot be verified`, { lease: path });
    }
    const quarantine = staleLeasePath(path, owner);
    try {
      renameSync(path, quarantine);
      rmSync(quarantine, { recursive: true, force: true });
      onStale?.({ path, owner });
      mkdirSync(path, { mode: 0o700 });
    } catch {
      throw new Refusal(`${kind} lease changed while recovering stale state`, { lease: path });
    }
  }
  const owner = {
    schema_version: 1,
    owner: "hardening-rig",
    kind,
    pid: process.pid,
    start_ticks: readProcStartTicks(process.pid),
    started: now(),
  };
  atomicJson(join(path, "owner.json"), owner);
  return {
    path,
    owner,
    release() {
      const current = leaseOwner(path);
      if (!current || current.pid !== owner.pid || current.start_ticks !== owner.start_ticks) return;
      rmSync(path, { recursive: true, force: true });
    },
  };
}

function seedStaleLease(path, kind) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  try {
    mkdirSync(path, { mode: 0o700 });
  } catch (error) {
    if (error.code === "EEXIST") throw new Refusal(`${kind} lease already exists for stale-lease simulation`, { lease: path });
    throw error;
  }
  atomicJson(join(path, "owner.json"), {
    schema_version: 1,
    owner: "hardening-rig",
    kind,
    pid: 2147483647,
    start_ticks: "0",
    started: "1970-01-01T00:00:00.000Z",
  });
}

function pathIsWithin(child, parent) {
  const rel = relative(resolve(parent), resolve(child));
  return rel !== "" && rel !== ".." && !rel.startsWith("../");
}

function scratchMarker(path) {
  return readJson(join(path, "owner.json"));
}

function cleanupStaleScratch() {
  if (!existsSync(SCRATCH_ROOT)) return [];
  const removed = [];
  for (const name of readdirSync(SCRATCH_ROOT)) {
    if (!name.startsWith("cycle-")) continue;
    const path = join(SCRATCH_ROOT, name);
    let isDirectory = false;
    try {
      isDirectory = lstatSync(path).isDirectory();
    } catch {
      continue;
    }
    if (!isDirectory) continue;
    const marker = scratchMarker(path);
    if (!marker || marker.owner !== "hardening-rig" || !marker.pid || processMatches(marker)) continue;
    rmSync(path, { recursive: true, force: true });
    removed.push(path);
  }
  return removed;
}

function createScratch(run) {
  if (!diskBacked(SCRATCH_ROOT).ok) throw new Refusal("scratch path is not disk-backed", { scratch: SCRATCH_ROOT });
  mkdirSync(SCRATCH_ROOT, { recursive: true, mode: 0o700 });
  const path = mkdtempSync(join(SCRATCH_ROOT, `cycle-${run.id}-`));
  atomicJson(join(path, "owner.json"), {
    schema_version: 1,
    owner: "hardening-rig",
    run_id: run.id,
    pid: process.pid,
    start_ticks: readProcStartTicks(process.pid),
    created: now(),
  });
  return path;
}

function removeScratch(path, run) {
  if (!path || !pathIsWithin(path, SCRATCH_ROOT) || !path.split("/").pop()?.startsWith("cycle-")) return false;
  if (run.scratch_removed && !existsSync(path)) return true;
  const marker = scratchMarker(path);
  if (!marker || marker.owner !== "hardening-rig" || marker.run_id !== run.id) return false;
  rmSync(path, { recursive: true, force: true });
  run.scratch_removed = !existsSync(path);
  return run.scratch_removed;
}

function diskBacked(path) {
  const resolved = resolve(path);
  if (resolved === "/tmp" || resolved.startsWith("/tmp/")) {
    return { ok: false, reason: "path is under RAM-backed /tmp" };
  }
  try {
    mkdirSync(resolved, { recursive: true, mode: 0o700 });
  } catch (error) {
    return { ok: false, reason: `cannot create path: ${error.message}` };
  }
  const result = spawnSync("df", ["-P", "-T", "--", resolved], { encoding: "utf8" });
  if (result.status !== 0) return { ok: false, reason: "df could not identify filesystem" };
  const lines = String(result.stdout || "").trim().split(/\n/);
  const fields = lines.length > 1 ? lines[lines.length - 1].trim().split(/\s+/) : [];
  const filesystemType = fields[1] || "unknown";
  if (["tmpfs", "ramfs", "devtmpfs"].includes(filesystemType)) {
    return { ok: false, reason: `filesystem is ${filesystemType}` };
  }
  return { ok: true, filesystem: filesystemType };
}

function sizeBytes(path) {
  if (!existsSync(path)) return 0;
  const item = lstatSync(path);
  if (!item.isDirectory()) return item.size;
  let total = 0;
  for (const name of readdirSync(path)) total += sizeBytes(join(path, name));
  return total;
}

function sizeReport() {
  const report = {};
  for (const [key, path] of [
    ["target_bytes", TARGET_ROOT],
    ["cache_bytes", CACHE_ROOT],
    ["interesting_bytes", INTERESTING_ROOT],
    ["log_bytes", FAILURE_LOG_PATH],
  ]) {
    try {
      report[key] = sizeBytes(path);
    } catch (error) {
      report[key] = null;
      report[`${key}_error`] = error.message;
    }
  }
  report.target_gib = report.target_bytes == null ? null : report.target_bytes / GIB;
  report.cache_gib = report.cache_bytes == null ? null : report.cache_bytes / GIB;
  report.interesting_mib = report.interesting_bytes == null ? null : report.interesting_bytes / 1024 ** 2;
  report.log_mib = report.log_bytes == null ? null : report.log_bytes / 1024 ** 2;
  return report;
}

function capViolations(report) {
  const violations = [];
  if (report.target_bytes == null || report.target_bytes > TARGET_CAP_BYTES) violations.push("target over 80GiB");
  if (report.cache_bytes == null || report.cache_bytes > CACHE_CAP_BYTES) violations.push("cache over 4GiB");
  if (report.interesting_bytes == null || report.interesting_bytes > INTERESTING_CAP_BYTES) {
    violations.push("interesting corpus over 512MiB");
  }
  if (report.log_bytes == null || report.log_bytes > LOG_CAP_BYTES) violations.push("failure log over 1MiB");
  return violations;
}

function rotateFailureLog() {
  if (!existsSync(FAILURE_LOG_PATH)) return;
  const bytes = readFileSync(FAILURE_LOG_PATH);
  if (bytes.length <= LOG_CAP_BYTES) return;
  const marker = Buffer.from("[hardening-rig log rotated]\n");
  atomicWrite(FAILURE_LOG_PATH, Buffer.concat([marker, bytes.subarray(Math.max(0, bytes.length - LOG_CAP_BYTES + marker.length))]));
}

function writeFailureLog(record) {
  const line = Buffer.from(`${JSON.stringify(record)}\n`);
  atomicWrite(FAILURE_LOG_PATH, line.subarray(0, LOG_CAP_BYTES));
}

function archiveCycle(result) {
  if (!result?.run_id || !/^[A-Za-z0-9_.-]+$/.test(result.run_id)) {
    throw new Error("cycle result has no safe run id for archival");
  }
  atomicJson(join(CYCLE_ROOT, `cycle-${result.run_id}.json`), result);
}

function loadState() {
  const state = readJson(STATE_PATH);
  if (!state) {
    return {
      schema_version: 1,
      blocked: false,
      transitions: [],
      last_cycle: null,
      built_commit: null,
      built_binary_sha256: null,
    };
  }
  if (state.__error) throw new Refusal("hardening rig state is unreadable", { path: STATE_PATH, error: state.__error });
  return state;
}

function transition(run, phase, fields = {}) {
  const event = { at: now(), run_id: run.id, phase, ...fields };
  run.transitions.push(event);
  const state = loadState();
  const transitions = [...(Array.isArray(state.transitions) ? state.transitions : []), event].slice(-MAX_TRANSITIONS);
  atomicJson(STATE_PATH, {
    ...state,
    schema_version: 1,
    updated: event.at,
    transitions,
    last_phase: phase,
  });
}

function gitIdentity() {
  const status = spawnSync("git", ["-C", ROOT, "status", "--porcelain=v1", "--untracked-files=all"], {
    encoding: "utf8",
  });
  if (status.status !== 0) throw new Refusal("git status failed", { stderr: status.stderr?.trim() || "unknown git error" });
  const commit = spawnSync("git", ["-C", ROOT, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (commit.status !== 0) throw new Refusal("git commit identity unavailable");
  return {
    clean: !String(status.stdout || "").trim(),
    dirty_paths: String(status.stdout || "").trim().split(/\n/).filter(Boolean),
    commit: String(commit.stdout || "").trim(),
  };
}

function manifestIdentity() {
  const configured = process.env.JET_HARDENING_MANIFEST;
  const candidates = configured
    ? [resolve(ROOT, configured)]
    : [
        join(ROOT, ".jet/hardening-manifest.json"),
        join(ROOT, ".jet/core-conformance-inventory.json"),
        join(ROOT, "tests/conformance/manifest.json"),
        join(ROOT, "tests/conformance/manifest.tsv"),
      ];
  for (const path of candidates) {
    if (!existsSync(path)) continue;
    return { path: relative(ROOT, path), sha256: sha256File(path), present: true };
  }
  return { path: null, sha256: null, present: false };
}

function csvEnv(name, fallback) {
  const value = process.env[name];
  if (value == null) return fallback;
  if (!value.trim()) return [];
  return value.split(",").map((item) => item.trim()).filter(Boolean);
}

function validateNames(values, label) {
  for (const value of values) {
    if (!/^[A-Za-z0-9_.\-/]+$/.test(value) || value.startsWith("-")) {
      throw new Refusal(`${label} contains an invalid name`, { value });
    }
  }
}
function boundedIntegerEnv(name, fallback, minimum, maximum) {
  const raw = process.env[name];
  if (raw == null || raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    throw new Refusal(`${name} must be an integer from ${minimum} through ${maximum}`, { value: raw });
  }
  return value;
}

function booleanEnv(name, fallback = false) {
  const value = process.env[name];
  if (value == null) return fallback;
  if (["1", "true", "yes"].includes(value.toLowerCase())) return true;
  if (["0", "false", "no"].includes(value.toLowerCase())) return false;
  throw new Refusal(`${name} must be true or false`, { value });
}

function oracleIncludeDifferential() {
  const configured = process.env.JET_HARDENING_INCLUDE_DIFFERENTIAL;
  if (configured == null) return true;
  if (["1", "true"].includes(configured.toLowerCase())) return true;
  if (["0", "false"].includes(configured.toLowerCase())) return false;
  throw new Refusal("JET_HARDENING_INCLUDE_DIFFERENTIAL must be true or false", { value: configured });
}

function oracleSeedPaths() {
  return {
    conformance: join(ROOT, "tests/conformance/corpus"),
    differential_manifest: join(ROOT, "tests/fuzz/sema/differential/manifest.tsv"),
  };
}


function config() {
  const proofTargets = csvEnv("JET_HARDENING_PROOF_TARGETS", DEFAULT_PROOF_TARGETS);
  const shards = csvEnv("JET_HARDENING_SHARDS", DEFAULT_SHARDS);
  const mutationDisabledKillers = csvEnv("JET_HARDENING_MUTATION_DISABLED_KILLERS", []);
  validateNames(proofTargets, "proof targets");
  validateNames(shards, "shards");
  validateNames(mutationDisabledKillers, "mutation disabled killers");
  const seed = process.env.JET_HARDENING_SEED || "2336";
  const variants = process.env.JET_HARDENING_VARIANTS || "50";
  const value = {
    schema_version: 1,
    suite_concurrency: 2,
    cargo_build_jobs: 4,
    min_free_gib: MIN_MEMORY_GIB,
    target_cap_gib: 80,
    cache_cap_gib: 4,
    interesting_cap_mib: 512,
    log_cap_mib: 1,
    incremental: 0,
    proof_targets: proofTargets,
    deterministic_shards: shards,
    seed,
    variants,
    oracle_batch_size: boundedIntegerEnv(
      "JET_HARDENING_ORACLE_BATCH_SIZE",
      DEFAULT_BATCH_SIZE,
      1,
      MAX_BATCH_SIZE,
    ),
    oracle_max_cases: boundedIntegerEnv(
      "JET_HARDENING_ORACLE_MAX_CASES",
      DEFAULT_ORACLE_MAX_CASES,
      1,
      DEFAULT_CORPUS_LIMIT,
    ),
    oracle_timeout_ms: boundedIntegerEnv(
      "JET_HARDENING_ORACLE_TIMEOUT_MS",
      DEFAULT_ORACLE_TIMEOUT_MS,
      1,
      MAX_TIMEOUT_MS,
    ),
    oracle_include_differential: oracleIncludeDifferential(),
    property_enabled: booleanEnv("JET_HARDENING_PROPERTY", false),
    property_max_cases: boundedIntegerEnv("JET_HARDENING_PROPERTY_MAX_CASES", 128, 1, 4096),
    grammar_enabled: booleanEnv("JET_HARDENING_GRAMMAR", false),
    grammar_max_cases: boundedIntegerEnv("JET_HARDENING_GRAMMAR_MAX_CASES", 128, 1, 1024),
    mutation_enabled: booleanEnv("JET_HARDENING_MUTATION", false),
    mutation_max_cases: boundedIntegerEnv("JET_HARDENING_MUTATION_MAX_CASES", MUTATION_CATALOG.length, 1, MUTATION_CATALOG.length),
    mutation_disabled_killers: mutationDisabledKillers,
  };
  return { ...value, hash: sha256(JSON.stringify(value)) };
}

function cycleEnvironment(scratch, cfg) {
  return {
    CARGO_BUILD_JOBS: String(cfg.cargo_build_jobs),
    CARGO_INCREMENTAL: "0",
    CARGO_TARGET_DIR: TARGET_ROOT,
    FUZZ_SEED: cfg.seed,
    FUZZ_VARIANTS: cfg.variants,
    JET_MIN_FREE_GB: String(cfg.min_free_gib),
    JET_TARGET_CAP_GB: String(cfg.target_cap_gib),
    JET_TEST_SCRATCH: scratch,
    JET_TEST_SCRATCH_DIR: scratch,
    JET_DEV_ORACLE_CACHE_DIR: join(scratch, "oracle-cache"),
    TMPDIR: scratch,
    TMP: scratch,
    TEMP: scratch,
  };
}

function childSummary(commandResult) {
  if (!commandResult) return null;
  return {
    label: commandResult.label,
    command: commandResult.command,
    pid: commandResult.pid,
    exit: commandResult.exit,
    signal: commandResult.signal,
    timed_out: commandResult.timed_out,
    ok: commandResult.ok,
    stdout_bytes: commandResult.stdout_bytes,
    stderr_bytes: commandResult.stderr_bytes,
    stdout_sha256: commandResult.stdout_sha256,
    stderr_sha256: commandResult.stderr_sha256,
  };
}

function failureBundle(result, childResult, error) {
  const bundle = {
    ...result,
    classification: childResult?.timed_out ? "timeout" : "failure",
    failure: error?.message || "cycle failed",
    failure_command: childResult?.command || null,
    stdout_bytes_base64: childResult?.stdout_base64 || "",
    stderr_bytes_base64: childResult?.stderr_base64 || "",
  };
  const encoded = Buffer.from(JSON.stringify(bundle));
  if (encoded.length <= 1024 * 1024) return bundle;
  return {
    ...result,
    classification: "failure",
    failure: error?.message || "cycle failed",
    failure_command: childResult?.command || null,
    stdout_bytes_base64: "",
    stderr_bytes_base64: "",
    output_truncated: true,
  };
}

function baseResult(run, cfg, identity, manifest) {
  return {
    schema_version: 1,
    run_id: run.id,
    started: run.started,
    finished: null,
    status: "RUNNING",
    commit: identity?.commit || null,
    binary_sha256: sha256File(JET_BINARY),
    host: os.hostname(),
    target: `${process.platform}-${process.arch}`,
    registry_snapshot: manifest,
    config: cfg,
    config_sha256: cfg.hash,
    seed: cfg.seed,
    mutation_arm: null,
    source: null,
    expected_relation: null,
    actual_relation: null,
    tier_commands: [],
    transitions: run.transitions,
    preflight: [],
    build: null,
    proof: null,
    oracle: null,
    property: null,
    grammar: null,
    mutation: null,
    tower: null,
    cleanup: null,
  };
}

function simulatedGuard(simulate) {
  const values = {
    dirty: "simulated dirty checkout",
    busy: "simulated active delivery or build lease",
    "tmp-guard": "simulated tmp-guard refusal",
    memory: "simulated available memory below 16GiB",
    target: "simulated target over 80GiB",
    cache: "simulated cache over 4GiB",
  };
  return values[simulate] || null;
}

async function preflight(run, cfg, simulate) {
  const checks = [];
  const check = (name, ok, reason, details = {}) => {
    const row = { name, ok, reason: reason || null, ...details };
    checks.push(row);
    if (!ok) throw new Refusal(reason || `${name} failed`, row);
  };

  const state = loadState();
  check("state", !state.blocked, state.blocked_reason || "previous resource overage blocks later cycles", { blocked: state.blocked });
  check("target-selection", !process.env.CARGO_TARGET_DIR || resolve(process.env.CARGO_TARGET_DIR) === TARGET_ROOT,
    "alternate CARGO_TARGET_DIR requested", { target: process.env.CARGO_TARGET_DIR || TARGET_ROOT });
  const scratch = diskBacked(SCRATCH_ROOT);
  check("scratch", scratch.ok, scratch.reason, { path: SCRATCH_ROOT, filesystem: scratch.filesystem || null });
  const stale = cleanupStaleScratch();
  if (stale.length) checks.push({ name: "stale-scratch", ok: true, recovered: stale.length });

  const synthetic = simulatedGuard(simulate);
  if (synthetic && simulate !== "stale-lease") throw new Refusal(synthetic, { simulated: true });

  if (simulate !== "tmp-guard") {
    check("tmp-guard", existsSync(TMP_GUARD), "tmp-guard is missing", { command: TMP_GUARD });
    const guard = await runCommand("tmp-guard", TMP_GUARD, [], cycleEnvironment(SCRATCH_ROOT, cfg), 60_000);
    checks.push({ name: "tmp-guard", ...childSummary(guard) });
    if (!guard.ok) throw new Refusal("tmp-guard failed", { command: guard.command, exit: guard.exit, stderr: guard.stderr_base64 });
  }

  if (simulate !== "dirty") {
    const identity = gitIdentity();
    checks.push({ name: "git", clean: identity.clean, commit: identity.commit, dirty_paths: identity.dirty_paths });
    check("git", identity.clean, "checkout is dirty", { commit: identity.commit, dirty_paths: identity.dirty_paths });
    run.identity = identity;
  } else {
    throw new Refusal("simulated dirty checkout", { simulated: true });
  }

  const externalLeasePaths = [
    ...["JET_HARDENING_DELIVERY_LEASE", "JET_HARDENING_BUILD_LEASE"].flatMap((name) => {
      const value = process.env[name];
      if (!value || ["1", "true", "active"].includes(value.toLowerCase())) return value ? [join(ROOT, `.${name.toLowerCase()}`)] : [];
      return [value];
    }),
    join(ROOT, ".jet/delivery-lease"),
    join(ROOT, ".jet/build-lease"),
    join(ROOT, ".claude/builder-claim"),
  ].filter(Boolean).map((path) => resolve(path));
  const activeExternal = externalLeasePaths.find((path) => existsSync(path));
  check("delivery-build-lease", !activeExternal && simulate !== "busy", "active delivery or build lease", {
    lease: activeExternal || null,
  });

  const resources = sizeReport();
  const availableGib = TEST_MODE && process.env.JET_HARDENING_MEM_AVAILABLE_GB
    ? Number(process.env.JET_HARDENING_MEM_AVAILABLE_GB)
    : (() => {
        try {
          const meminfo = readFileSync("/proc/meminfo", "utf8");
          const match = meminfo.match(/^MemAvailable:\s+(\d+) kB$/m);
          return match ? Number(match[1]) / 1024 ** 2 : null;
        } catch {
          return null;
        }
      })();
  checks.push({ name: "memory", available_gib: availableGib });
  check("memory", simulate !== "memory" && availableGib != null && availableGib >= MIN_MEMORY_GIB,
    `available memory below ${MIN_MEMORY_GIB}GiB`, { available_gib: availableGib, minimum_gib: MIN_MEMORY_GIB });
  check("target-cap", simulate !== "target" && resources.target_bytes != null && resources.target_bytes <= TARGET_CAP_BYTES,
    "target over 80GiB", { target_gib: resources.target_gib, cap_gib: 80 });
  check("cache-cap", simulate !== "cache" && resources.cache_bytes != null && resources.cache_bytes <= CACHE_CAP_BYTES,
    "cache over 4GiB", { cache_gib: resources.cache_gib, cap_gib: 4 });
  check("interesting-cap", resources.interesting_bytes != null && resources.interesting_bytes <= INTERESTING_CAP_BYTES,
    "interesting corpus over 512MiB", { interesting_mib: resources.interesting_mib, cap_mib: 512 });
  rotateFailureLog();
  const rotated = sizeReport();
  check("log-cap", rotated.log_bytes != null && rotated.log_bytes <= LOG_CAP_BYTES,
    "failure log over 1MiB", { log_mib: rotated.log_mib, cap_mib: 1 });
  run.preflight = checks;
  return run.identity;
}

function proofCommand(cfg) {
  const names = [...cfg.proof_targets, ...cfg.deterministic_shards];
  if (!names.length) return null;
  return {
    program: PROOF_PARALLEL,
    args: ["-j", String(cfg.suite_concurrency), ...names],
    text: commandText(PROOF_PARALLEL, ["-j", String(cfg.suite_concurrency), ...names]),
  };
}

function boundedOracleSelection(discovered, cfg) {
  const armCount = Math.min(MUTATION_ARMS.length, cfg.oracle_max_cases);
  const arms = MUTATION_ARMS.slice(0, armCount);
  const seedLimit = Math.max(1, Math.floor(cfg.oracle_max_cases / armCount));
  return {
    arms,
    seeds: discovered.seeds.slice(0, seedLimit),
    omitted_seed_count: Math.max(0, discovered.seeds.length - seedLimit),
  };
}

function oracleSourcePath(run, caseInput) {
  const directory = join(run.scratch, "oracle");
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const caseId = String(caseInput.case_id || sha256(caseInput.source)).replace(/[^A-Za-z0-9_-]/g, "_");
  const path = join(directory, `${caseId}.jet`);
  writeFileSync(path, caseInput.source, { mode: 0o600 });
  return path;
}

async function executeOracleTier(run, request, environment, cfg) {
  const sourcePath = oracleSourcePath(run, request);
  const command = tierCommand(request.tier, sourcePath, { root: ROOT, jetEnv: JET_ENV });
  const execution = await runCommand(
    `oracle:${request.case_id || "case"}:${request.tier}`,
    command.program,
    command.args,
    environment,
    cfg.oracle_timeout_ms,
    request.stdin,
  );
  return {
    ...execution,
    tier: request.tier,
    tier_command: command.tier_command,
    source_path: sourcePath,
    source_sha256: sha256(request.source),
  };
}

function findingClassification(caseResult) {
  const loud = caseResult.tier_results.some((observation) => (
    observation.error
    || observation.timed_out
    || observation.signal
    || (observation.exit !== null && observation.exit !== 0)
  ));
  const defaultJetRunDivergence = caseResult.differences.includes("jet_run");
  return {
    classification: defaultJetRunDivergence ? "default-jet-run-divergence" : loud ? "loud-failure" : "silent-data",
    silentWrongData: !loud,
    defaultJetRunDivergence,
    loudFailure: loud,
  };
}

function findingBundle(run, caseInput, caseResult, manifest, cfg) {
  const input = caseResult.result_bundle_input;
  if (!input) throw new Error(`oracle returned no finding input for ${caseInput.case_id}`);
  const selected = caseResult.tier_results.find((observation) => observation.tier === input.tier)
    || caseResult.tier_results[0];
  const classification = findingClassification(caseResult);
  return makeResultBundle({
    run_id: run.id,
    stable_surface_id: caseInput.stable_surface_id,
    tier: input.tier,
    tier_command: selected.tier_command,
    seed: caseInput.seed,
    mutation_arm: caseInput.mutation_arm,
    mutator_version: caseInput.mutator_version,
    source: caseInput.source,
    stdout: selected.stdout,
    stderr: selected.stderr,
    exit: selected.exit,
    signal: selected.signal,
    timeout: selected.timed_out,
    expected_relation: input.expected_relation,
    actual_relation: input.actual_relation,
    normalization: caseInput.normalization || [],
    oracle: caseInput.oracle,
    commit: run.identity.commit,
    binary_sha256: run.result?.binary_sha256 || sha256File(JET_BINARY) || "sha256:unknown-binary",
    registry_snapshot_hash: manifest.sha256 || "sha256:unknown-registry",
    config_hash: cfg.hash,
    classification: classification.classification,
    tower_action: "create-or-update",
    tier_observations: caseResult.observations,
    applicable_tiers: caseInput.applicable_tiers,
  });
}

function towerPayload(caseInput, caseResult, bundle) {
  const classification = findingClassification(caseResult);
  const hardeningSeam = caseInput.semantic_primitive || caseInput.root_seam || "unclassified";
  const hardeningWrongTierMask = caseResult.differences;
  const hardeningInputPartition = caseInput.mutation_arm;
  const commands = caseResult.tier_results.map((observation) => `${observation.tier}: ${observation.tier_command}`);
  const hardeningKey = buildHardeningDedupKey({
    bundle,
    hardening_seam: hardeningSeam,
    violated_relation: bundle.expected_relation,
    wrong_tier_mask: hardeningWrongTierMask,
    input_partition: hardeningInputPartition,
  });
  return {
    title: `Layer-1 hardening finding: ${caseInput.stable_surface_id} (${caseInput.mutation_arm})`,
    body: "Confirmed by the bounded layer-1 differential oracle.",
    hardeningSeam,
    hardeningRelation: bundle.expected_relation,
    hardeningWrongTierMask,
    hardeningInputPartition,
    hardeningDedupKey: hardeningKey,
    hardeningEvidence: {
      source: bundle.source,
      commands,
      expectedRelation: bundle.expected_relation,
      actualRelation: bundle.actual_relation,
      seed: bundle.seed,
      targetCommit: bundle.commit,
      bundleDigest: bundleIdentity(bundle),
      classification: bundle.classification,
      stdoutBytes: bundle.stdout_bytes,
      stderrBytes: bundle.stderr_bytes,
      exit: bundle.exit,
      signal: bundle.signal,
      timeout: bundle.timeout,
      normalization: bundle.normalization,
    },
    source: bundle.source,
    commands,
    expectedRelation: bundle.expected_relation,
    actualRelation: bundle.actual_relation,
    seed: bundle.seed,
    targetCommit: bundle.commit,
    stdoutBytes: bundle.stdout_bytes,
    stderrBytes: bundle.stderr_bytes,
    exit: bundle.exit,
    signal: bundle.signal,
    timeout: bundle.timeout,
    normalization: bundle.normalization,
    classification: classification.classification,
    silentWrongData: classification.silentWrongData,
    defaultJetRunDivergence: classification.defaultJetRunDivergence,
    loudFailure: classification.loudFailure,
    tier: bundle.tier,
    oracle: bundle.oracle,
  };
}

async function runLayerOne(run, cfg, manifest, environment) {
  const paths = oracleSeedPaths();
  const includeDifferential = cfg.oracle_include_differential && existsSync(paths.differential_manifest);
  transition(run, "oracle_discover", {
    conformance_root: relative(ROOT, paths.conformance),
    differential_manifest: includeDifferential ? relative(ROOT, paths.differential_manifest) : null,
  });
  const discovered = discoverCorpusSeeds(ROOT, { includeDifferential });
  const selected = boundedOracleSelection(discovered, cfg);
  const summary = {
    engine: "hardening-oracle-layer",
    schema_version: 1,
    status: "SKIPPED",
    include_differential: includeDifferential,
    discovered_seed_count: discovered.seeds.length,
    rejected_seed_count: discovered.rejected.length,
    selected_seed_count: selected.seeds.length,
    omitted_seed_count: selected.omitted_seed_count,
    arms: selected.arms,
    max_cases: cfg.oracle_max_cases,
    batch_size: cfg.oracle_batch_size,
    timeout_ms: cfg.oracle_timeout_ms,
    rejected: [...discovered.rejected],
    attempted: 0,
    valid_case_count: 0,
    batch_count: 0,
    cases: [],
    findings: [],
    finding_payloads: [],
    serialized_bundles: "",
    bundle_sha256: sha256(""),
  };
  if (!selected.seeds.length) {
    transition(run, "oracle_skipped", { reason: "no checked-in value-consuming seeds" });
    return summary;
  }

  transition(run, "oracle_batch", {
    seed_count: selected.seeds.length,
    arms: selected.arms,
    max_cases: cfg.oracle_max_cases,
    batch_size: cfg.oracle_batch_size,
  });
  const batch = batchMutations(selected.seeds, {
    batchSize: cfg.oracle_batch_size,
    arms: selected.arms,
    maxCases: cfg.oracle_max_cases,
  });
  summary.attempted = batch.attempted;
  summary.valid_case_count = batch.valid_case_count;
  summary.batch_count = batch.batches.length;
  summary.cases = batch.cases.map((caseInput) => ({
    case_id: caseInput.case_id,
    stable_surface_id: caseInput.stable_surface_id,
    seed: caseInput.seed,
    domain: caseInput.domain,
    mutation_arm: caseInput.mutation_arm,
    mutator_version: caseInput.mutator_version,
    source_sha256: `sha256:${sha256(caseInput.source)}`,
    skeleton: caseInput.skeleton,
    observer_fingerprint: caseInput.observer_fingerprint,
    type_skeleton: caseInput.type_skeleton,
    normalization: caseInput.normalization,
    oracle: caseInput.oracle,
    expected_relation: caseInput.expected_relation,
    applicable_tiers: caseInput.applicable_tiers,
  }));
  summary.rejected.push(...batch.rejected);
  if (!batch.cases.length) {
    transition(run, "oracle_skipped", { reason: "no valid bounded mutations", rejected: batch.rejected.length });
    return summary;
  }

  transition(run, "oracle_execute", { case_count: batch.cases.length, batch_count: batch.batches.length });
  const findings = [];
  const finding_payloads = [];
  for (const caseInput of batch.cases) {
    const caseResult = await executeCase(caseInput, {
      executor: (request) => executeOracleTier(run, request, environment, cfg),
      validate: false,
      applicable_tiers: caseInput.applicable_tiers,
      normalization: caseInput.normalization,
      stdin: caseInput.stdin || "",
    });
    const failedTiers = caseResult.tier_results.filter((observation) => (
      observation.error
      || observation.timed_out
      || observation.signal
      || (observation.exit !== null && observation.exit !== 0)
    ));
    if (failedTiers.length) {
      caseResult.ok = false;
      caseResult.differences = [...new Set([
        ...caseResult.differences,
        ...failedTiers.map((observation) => observation.tier),
      ])];
      caseResult.result_bundle_input ||= {
        tier: failedTiers[0].tier,
        expected_relation: caseResult.expected_relation,
        actual_relation: caseResult.actual_relation,
        tier_observations: caseResult.observations,
      };
    }
    if (caseResult.ok) continue;
    const bundle = findingBundle(run, caseInput, caseResult, manifest, cfg);
    findings.push(bundle);
    finding_payloads.push({
      bundle_identity: bundleIdentity(bundle),
      payload: towerPayload(caseInput, caseResult, bundle),
    });
  }
  const serialized = serializeBundles(findings);
  summary.status = findings.length ? "FINDINGS" : "PASS";
  summary.findings = findings;
  summary.finding_payloads = finding_payloads;
  summary.serialized_bundles = serialized;
  summary.bundle_sha256 = sha256(serialized);
  transition(run, findings.length ? "oracle_findings" : "oracle_pass", {
    attempted: summary.attempted,
    valid_case_count: summary.valid_case_count,
    finding_count: findings.length,
  });
  return summary;
}
function layerPath(name, fallback = null) {
  const value = process.env[name];
  return value ? resolve(ROOT, value) : fallback;
}

function readLayerRows(path) {
  if (!path || !existsSync(path)) return null;
  const value = readJson(path);
  if (Array.isArray(value)) return value;
  if (value && Array.isArray(value.rows)) return value.rows;
  if (value?.manifest && Array.isArray(value.manifest.rows)) return value.manifest.rows;
  return null;
}

function layerSurfaces(manifest) {
  const configured = layerPath(
    "JET_HARDENING_PROPERTY_MANIFEST",
    manifest?.path ? resolve(ROOT, manifest.path) : null,
  );
  return readLayerRows(configured) || [];
}

function layerValue(result) {
  const bytes = Buffer.isBuffer(result?.stdout)
    ? result.stdout
    : Buffer.isBuffer(result?.stdout_bytes)
      ? result.stdout_bytes
      : typeof result?.stdout_base64 === "string"
        ? Buffer.from(result.stdout_base64, "base64")
        : Buffer.from(String(result?.stdout || ""), "utf8");
  const text = bytes.toString("utf8").trim();
  if (!text) return "";
  const lines = text.split(/\r?\n/).filter(Boolean);
  const candidate = lines.length === 1 ? lines[0] : lines[lines.length - 1];
  try { return JSON.parse(candidate); } catch { return candidate; }
}

async function executeGeneratedTier(run, request, environment, cfg) {
  const result = await executeOracleTier(run, request, environment, cfg);
  const normalized_value = layerValue(result);
  const stderr = Buffer.isBuffer(result.stderr) ? result.stderr.toString("utf8") : String(result.stderr || "");
  const rustText = [stderr, result.error, result.stdout, result.stderr_bytes]
    .filter((value) => value !== undefined && value !== null)
    .map((value) => Buffer.isBuffer(value) ? value.toString("utf8") : String(value))
    .join(" ");
  const rustRejected = request.layer === "grammar"
    && request.value_consuming === true
    && request.tier === "aot"
    && !result.ok
    && /(?:\brustc\b|generated Rust|generated code.*(?:rejected|error)|error\[[A-Z]\d+\])/i.test(rustText);
  return {
    ...result,
    normalized_value,
    relation: canonicalRelation(normalized_value),
    ...(request.layer === "grammar" ? {
      tir: request.tier === "interpreter"
        ? { constructed: result.ok === true, evaluated: result.ok === true }
        : { constructed: result.ok === true },
      ...(rustRejected ? { rust: { accepted: false, error: rustText } } : {}),
    } : {}),
  };
}

function compilerStageObservation(result, accepted) {
  return {
    accepted: accepted === true,
    ok: accepted === true,
    ...(result?.error ? { error: String(result.error) } : {}),
    ...(result?.json ? { diagnostics: result.json } : {}),
  };
}

async function checkGeneratedGrammarStages(request, environment, cfg) {
  const checked = await checkJetSource(request.source, {
    root: ROOT,
    jet_env: JET_ENV,
    cwd: ROOT,
    env: environment,
    timeout_ms: cfg.oracle_timeout_ms,
    capture_limit: MAX_CAPTURE_BYTES,
  });
  const parser = compilerStageObservation(checked.parse, checked.parse?.ok === true);
  const sema = checked.parse?.ok === true
    ? compilerStageObservation(checked.check, checked.check?.ok === true)
    : null;
  return {
    parser,
    ...(sema ? { sema } : {}),
    source_sha256: sha256(request.source),
  };
}

function canonicalRelation(value) {
  try { return JSON.stringify(value); } catch { return String(value); }
}

function disabledLayerSummary(schema, reason) {
  return {
    schema,
    schema_version: 1,
    status: "DISABLED",
    reason,
    attempted: 0,
    valid_case_count: 0,
    rejected: [{ kind: "configuration", reason }],
    findings: [],
    serialized_bundles: "",
    bundle_sha256: sha256(""),
  };
}

async function runLayerTwo(run, cfg, manifest, environment) {
  if (!cfg.property_enabled) return disabledLayerSummary("jet.hardening.property.v1", "property layer is opt-in");
  const surfaces = layerSurfaces(manifest);
  if (surfaces.length === 0) return disabledLayerSummary("jet.hardening.property.v1", "property manifest has no rows");
  const generated = generatePropertyCases({
    surfaces,
    seed: cfg.seed,
    maxCases: cfg.property_max_cases,
  });
  const result = await runPropertyCases(generated.cases, {
    maxCases: cfg.property_max_cases,
    executor: (request) => executeGeneratedTier(run, request, environment, cfg),
    metadata: {
      run_id: run.id,
      commit: run.identity.commit,
      binary_sha256: run.result.binary_sha256 || sha256File(JET_BINARY) || "sha256:unknown-binary",
      registry_snapshot_hash: manifest.sha256 || "sha256:unknown-registry",
      config_hash: cfg.hash,
    },
  });
  return propertyLayerSummary(generated, result);
}

function grammarSources() {
  const syntax = layerPath(
    "JET_HARDENING_GRAMMAR_SYNTAX",
    join(ROOT, "crates/jet-foundation/src/Syntax.rs"),
  );
  const parser = process.env.JET_HARDENING_GRAMMAR_PARSER
    ? csvEnv("JET_HARDENING_GRAMMAR_PARSER", []).map((path) => resolve(ROOT, path))
    : undefined;
  const sema = process.env.JET_HARDENING_GRAMMAR_SEMA
    ? csvEnv("JET_HARDENING_GRAMMAR_SEMA", []).map((path) => resolve(ROOT, path))
    : undefined;
  return { syntaxSource: syntax, parserSources: parser, semaSources: sema };
}

async function runLayerThree(run, cfg, environment, manifest) {
  if (!cfg.grammar_enabled) return disabledLayerSummary("jet.hardening.grammar.v1", "grammar layer is opt-in");
  const negative_controls = checkGrammarNegativeControls();
  const constructManifest = deriveConstructManifest({ root: ROOT, ...grammarSources() });
  const generated = generateTypedPrograms(constructManifest, {
    seed: cfg.seed,
    maxCases: cfg.grammar_max_cases,
    includeNearValid: booleanEnv("JET_HARDENING_GRAMMAR_NEAR_VALID", true),
  });
  const grammarManifestHash = constructManifestHash(constructManifest);
  const result = await runGrammarPrograms(generated.programs, {
    maxCases: cfg.grammar_max_cases,
    executor: (request) => executeGeneratedTier(run, request, environment, cfg),
    stageExecutor: (request) => checkGeneratedGrammarStages(request, environment, cfg),
    metadata: {
      run_id: run.id,
      commit: run.identity.commit,
      binary_sha256: run.result.binary_sha256 || sha256File(JET_BINARY) || "sha256:unknown-binary",
      registry_snapshot_hash: diagnosticRegistryHash(),
      config_hash: cfg.hash,
    },
  });
  return {
    ...result,
    denominator: generated.denominator,
    manifest: constructManifest,
    generated_rejected: generated.rejected,
    generation: {
      seed: generated.seed,
      max_cases: generated.max_cases,
      attempted: generated.attempted,
      valid_case_count: generated.valid_case_count,
      manifest_sha256: generated.manifest_sha256,
      programs_sha256: generated.programs_sha256,
      ordered_case_ids: generated.programs.map((program) => program.case_id),
    },
    negative_controls,
  };
}

async function runLayerFour(run, cfg, environment, manifest) {
  if (!cfg.mutation_enabled) return disabledLayerSummary("jet.hardening.mutation.v1", "mutation layer is opt-in");
  if (!run.buildLease) throw new Error("mutation layer requires the clean checkout/build lease");
  const adapterPath = layerPath("JET_HARDENING_MUTATION_ADAPTER");
  if (!adapterPath) throw new Error("JET_HARDENING_MUTATION_ADAPTER is required when mutation layer is enabled");
  const adapter = await import(adapterPath);
  const baselineSource = adapter.baseline_source || WITNESS_MUTATION_SOURCE;
  const baseline = {
    source_sha256: sha256(baselineSource),
    target_sha256: sha256File(JET_BINARY) || "sha256:unknown-binary",
    current: typeof adapter.current === "function" ? adapter.current : undefined,
  };
  run.mutation_baseline = baseline;
  run.mutation_context = null;
  run.mutation_cleanup = async () => {
    await adapter.interrupt?.();
    const context = run.mutation_context;
    if (!context) return;
    await adapter.restore(context.mutant, context.input, baseline);
    await adapter.removeWorkspace(context.mutant, context.input);
    run.mutation_context = null;
  };
  return runMutationSensitivity({
    catalog: MUTATION_CATALOG,
    seed: cfg.seed,
    maxMutants: cfg.mutation_max_cases,
    baseline,
    apply: adapter.apply,
    build: adapter.build,
    prove: adapter.prove,
    restore: adapter.restore,
    removeWorkspace: adapter.removeWorkspace || null,
    workspaceRequired: true,
    disabledKillers: cfg.mutation_disabled_killers,
    onMutantStart: (mutant, input) => {
      run.mutation_context = { mutant, input };
    },
    onMutantEnd: () => {
      run.mutation_context = null;
    },
    metadata: {
      run_id: run.id,
      commit: run.identity.commit,
      binary_sha256: baseline.target_sha256,
      registry_snapshot_hash: manifest.sha256 || "sha256:unknown-registry",
      config_hash: cfg.hash,
    },
  });
}

const WITNESS_MUTATION_SOURCE = `fn run() {
    value :: 1
    print(value)
}
`;

function layerFindingEntries(layer, layerResult) {
  if (layer === "4" && Array.isArray(layerResult?.gap_cards)) {
    return layerResult.gap_cards.map((card) => {
      const payload = card.payload || {};
      return {
        bundle_identity: card.identity,
        payload: {
          title: card.title,
          body: card.reason,
          hardeningLayer: layer,
          hardeningSeam: payload.seam || "unclassified",
          mutantId: payload.mutant_id || null,
          expectedLayer: payload.expected_layer || null,
          astMutation: payload.ast_mutation || null,
          missingProof: payload.missing_proof || null,
          gapCard: payload,
          mutationScore: layerResult.mutation_score,
          survivorIds: layerResult.survivor_ids,
          classification: "hardening-gap-survivor",
        },
      };
    });
  }
  return (layerResult?.findings || layerResult?.bundles || []).map((bundle) => ({
    bundle_identity: bundleIdentity(bundle),
    payload: {
      title: `Layer-${layer} hardening finding: ${bundle.stable_surface_id}`,
      body: `Confirmed by the bounded ${layer} hardening layer.`,
      hardeningLayer: layer,
      hardeningSeam: bundle.seam || bundle.law_id || bundle.construct_id || bundle.mutant_id || "unclassified",
      source: bundle.source,
      expectedRelation: bundle.expected_relation,
      actualRelation: bundle.actual_relation,
      seed: bundle.seed,
      targetCommit: bundle.commit,
      classification: bundle.classification,
      oracle: bundle.oracle,
      proof: bundle.proof || null,
    },
  }));
}

function towerDryRun() {
  return TEST_MODE || process.env.JET_HARDENING_DRY_RUN === "1";
}

async function writeTowerFinding(run, entry, environment, cfg) {
  if (towerDryRun()) {
    return {
      status: "SKIPPED",
      reason: TEST_MODE ? "test mode" : "dry run",
      bundle_identity: entry.bundle_identity,
    };
  }
  if (!existsSync(TOWER_CLI)) throw new Error(`Tower CLI is missing: ${TOWER_CLI}`);
  const args = [];
  if (TOWER_DATA) args.push("--data", TOWER_DATA);
  args.push("card", "add", "--stdin", "--json", "--by", "hardening-rig");
  const command = await runCommand(
    "tower:hardening-card",
    process.execPath,
    [TOWER_CLI, ...args],
    environment,
    cfg.oracle_timeout_ms,
    JSON.stringify(entry.payload),
  );
  if (!command.ok) throw new ChildFailure("tower", command);
  let card;
  try {
    card = JSON.parse(command.stdout.toString("utf8"));
  } catch (error) {
    throw new Error(`Tower hardening response is not JSON: ${error.message}`);
  }
  if (!card || card.error) throw new Error(`Tower hardening write failed: ${card?.message || "invalid response"}`);
  return {
    status: "WRITTEN",
    bundle_identity: entry.bundle_identity,
    card_id: card.id || null,
    card_num: card.num || null,
    action: card.action || null,
    command: childSummary(command),
  };
}

async function writeTowerFindings(run, oracle, environment, cfg) {
  const actions = [];
  for (const entry of oracle.finding_payloads) {
    actions.push(await writeTowerFinding(run, entry, environment, cfg));
  }
  return actions;
}

async function runCycle(options) {
  const id = runId();
  const run = {
    id,
    started: now(),
    transitions: [],
    children: new Map(),
    preflight: [],
    scratch: null,
    scratch_removed: false,
    rigLease: null,
    buildLease: null,
    identity: null,
    result: null,
    config: config(),
  };
  currentRun = run;
  cleanAtomicTemps();
  mkdirSync(CACHE_ROOT, { recursive: true, mode: 0o700 });
  const manifest = manifestIdentity();
  let result = baseResult(run, run.config, null, manifest);
  run.result = result;
  let childResult = null;
  let refusal = null;
  try {
    transition(run, "preflight");
    try {
      const identity = await preflight(run, run.config, options.simulate);
      result.commit = identity.commit;
      result.preflight = run.preflight;
    } catch (error) {
      if (error instanceof Refusal) {
        refusal = error;
        result.status = "SKIPPED";
        result.refusal = { reason: error.reason, details: error.details };
        transition(run, "refused", { status: "SKIPPED", reason: error.reason });
        return finalizeCycle(run, result, null, null, refusal);
      }
      throw error;
    }

    if (options.simulate === "stale-lease") seedStaleLease(RIG_LEASE_PATH, "rig");
    try {
      run.rigLease = acquireLease(RIG_LEASE_PATH, "rig", ({ owner }) => {
        transition(run, "stale_lease_recovered", { owner: leaseDescription(owner), killed_pid: false });
      });
    } catch (error) {
      if (error instanceof Refusal) {
        refusal = error;
        result.status = "SKIPPED";
        result.refusal = { reason: error.reason, details: error.details };
        transition(run, "refused", { status: "SKIPPED", reason: error.reason });
        return finalizeCycle(run, result, null, null, refusal);
      }
      throw error;
    }
    transition(run, "rig_lease_acquired", { lease: RIG_LEASE_PATH });

    try {
      run.buildLease = acquireLease(BUILD_LEASE_PATH, "build", ({ owner }) => {
        transition(run, "stale_build_lease_recovered", { owner: leaseDescription(owner), killed_pid: false });
      });
    } catch (error) {
      if (error instanceof Refusal) {
        refusal = error;
        result.status = "SKIPPED";
        result.refusal = { reason: error.reason, details: error.details };
        transition(run, "refused", { status: "SKIPPED", reason: error.reason });
        return finalizeCycle(run, result, null, null, refusal);
      }
      throw error;
    }
    transition(run, "build_lease_acquired", { lease: BUILD_LEASE_PATH });

    run.scratch = createScratch(run);
    const environment = cycleEnvironment(run.scratch, run.config);
    const state = loadState();
    const currentBinarySha256 = sha256File(JET_BINARY);
    const mustBuild = options.simulate === "build-failure"
      || state.built_commit !== run.identity.commit
      || !existsSync(JET_BINARY)
      || (state.built_binary_sha256 && state.built_binary_sha256 !== currentBinarySha256);
    transition(run, "snapshot_identity", {
      commit: run.identity.commit,
      registry_snapshot: manifest,
      binary_sha256: sha256File(JET_BINARY),
      build_required: mustBuild,
    });

    if (mustBuild) {
      transition(run, "build");
      const buildCommand = options.simulate === "build-failure"
        ? ["sh", "-c", "exit 7"]
        : ["cargo", "build", "-p", "jet"];
      childResult = await runCommand("build", JET_ENV, buildCommand, environment,
        Number(process.env.JET_HARDENING_BUILD_TIMEOUT_MS || DEFAULT_BUILD_TIMEOUT_MS));
      result.build = childSummary(childResult);
      if (!childResult.ok || options.simulate === "build-failure") throw new ChildFailure("build", childResult);
      if (!existsSync(JET_BINARY)) throw new Error(`build completed without ${JET_BINARY}`);
      result.binary_sha256 = sha256File(JET_BINARY);
      atomicJson(STATE_PATH, {
        ...loadState(),
        built_commit: run.identity.commit,
        built_binary_sha256: result.binary_sha256,
      });
    } else {
      result.build = { skipped: true, reason: "same clean commit already built", binary_sha256: sha256File(JET_BINARY) };
    }

    const proof = proofCommand(run.config);
    result.tier_commands = proof ? [proof.text] : [];
    transition(run, "proof", { command: proof?.text || null, concurrency: run.config.suite_concurrency });
    if (options.simulate === "timeout" || options.simulate === "signal" || options.simulate === "test-failure") {
      const simulated = await runCommand(
        "simulated-proof",
        "sh",
        ["-c", options.simulate === "test-failure" ? "exit 7" : options.simulate === "signal"
          ? "sleep 30 & kill -TERM $$"
          : "sleep 30"],
        environment,
        options.simulate === "timeout" ? 100 : DEFAULT_TIMEOUT_MS,
      );
      childResult = simulated;
      result.proof = childSummary(simulated);
      if (!simulated.ok) throw new ChildFailure("proof", simulated);
    } else if (proof) {
      if (!existsSync(PROOF_PARALLEL)) throw new Error(`proof runner is missing: ${PROOF_PARALLEL}`);
      childResult = await runCommand("proof", proof.program, proof.args, environment,
        Number(process.env.JET_HARDENING_PROOF_TIMEOUT_MS || DEFAULT_TIMEOUT_MS));
      result.proof = childSummary(childResult);
      if (!childResult.ok) throw new ChildFailure("proof", childResult);
    } else {
      result.proof = { skipped: true, reason: "no named proof targets or shards" };
    }

    const oracle = await runLayerOne(run, run.config, manifest, environment);
    result.oracle = oracle;
    if (oracle.findings.length) {
      transition(run, "tower_card_findings", { finding_count: oracle.findings.length });
      const towerActions = await writeTowerFindings(run, oracle, environment, run.config);
      result.tower = {
        status: towerDryRun() ? "SKIPPED" : "WRITTEN",
        reason: towerDryRun() ? (TEST_MODE ? "test mode" : "dry run") : null,
        actions: towerActions,
      };
      result.status = "RED";
      result.failure_stage = "oracle";
      const findingError = new Error(`layer-1 oracle confirmed ${oracle.findings.length} finding(s)`);
      transition(run, "record_findings", {
        status: result.status,
        finding_count: oracle.findings.length,
        tower_actions: result.tower.actions.length,
      });
      return finalizeCycle(run, result, childResult, findingError, null);
    }
    const property = await runLayerTwo(run, run.config, manifest, environment);
    result.property = property;
    const grammar = await runLayerThree(run, run.config, environment, manifest);
    result.grammar = grammar;
    const mutation = await runLayerFour(run, run.config, environment, manifest);
    result.mutation = mutation;
    const layerFindings = [
      ...layerFindingEntries("2", property),
      ...layerFindingEntries("3", grammar),
      ...layerFindingEntries("4", mutation),
    ];
    if (layerFindings.length) {
      transition(run, "layer_findings", { finding_count: layerFindings.length });
      const towerActions = await writeTowerFindings(run, { finding_payloads: layerFindings }, environment, run.config);
      result.tower = {
        status: towerDryRun() ? "SKIPPED" : "WRITTEN",
        reason: towerDryRun() ? (TEST_MODE ? "test mode" : "dry run") : null,
        actions: towerActions,
      };
      result.status = "RED";
      result.failure_stage = "hardening-layers";
      const findingError = new Error(`hardening layers confirmed ${layerFindings.length} finding(s)`);
      transition(run, "record_findings", {
        status: result.status,
        finding_count: layerFindings.length,
        tower_actions: result.tower.actions.length,
      });
      return finalizeCycle(run, result, childResult, findingError, null);
    }
    result.tower = { status: "SKIPPED", reason: "no confirmed findings", actions: [] };

    result.status = "PASS";
    transition(run, "record_result", { status: result.status });
    return finalizeCycle(run, result, childResult, null, null);

  } catch (error) {
    if (error instanceof ChildFailure) {
      childResult = error.result;
      result.failure_stage = error.label;
    }
    if (run.signal) {
      result.failure_stage = "signal";
      result.signal = run.signal;
    }
    result.status = "RED";
    transition(run, "failure", {
      status: "RED",
      reason: error.message,
      stage: result.failure_stage || null,
      timed_out: Boolean(childResult?.timed_out),
    });
    return finalizeCycle(run, result, childResult, error, null);
  } finally {
    currentRun = null;
  }
}

async function finalizeCycle(run, result, childResult, error, refusal) {
  let cleanupError = null;
  try {
    await killOwnedChildren(run);
    if (run.scratch) {
      const removed = removeScratch(run.scratch, run);
      result.cleanup = { scratch: run.scratch, scratch_removed: removed, children: run.children.size === 0 };
    } else {
      result.cleanup = { scratch: null, scratch_removed: true, children: run.children.size === 0 };
    }
  } catch (cleanupFailure) {
    cleanupError = cleanupFailure;
    result.cleanup = { scratch_removed: false, children: false, error: cleanupFailure.message };
    result.status = "RED";
  }
  try {
    run.buildLease?.release();
    run.rigLease?.release();
  } catch (leaseError) {
    cleanupError ||= leaseError;
    result.status = "RED";
  }
  result.finished = now();
  result.transitions = [...run.transitions];
  if (cleanupError) {
    transition(run, "cleanup_failure", { error: cleanupError.message });
  }

  if (error && !(error instanceof Refusal)) {
    const bundle = failureBundle(result, childResult, error);
    atomicJson(FAILURE_PATH, bundle);
    writeFailureLog(bundle);
  }
  rotateFailureLog();
  const resources = sizeReport();
  const violations = capViolations(resources);
  result.resources = resources;
  result.resource_violations = violations;
  if (violations.length) {
    result.status = "RED";
    const state = loadState();
    atomicJson(STATE_PATH, {
      ...state,
      blocked: true,
      blocked_reason: violations.join("; "),
      blocked_at: now(),
    });
    transition(run, "resource_overage", { status: "RED", violations });
  }
  transition(run, "status", { status: result.status, violations });
  result.transitions = [...run.transitions];
  try {
    archiveCycle(result);
  } catch (archiveFailure) {
    result.status = "RED";
    result.archive_error = archiveFailure.message;
    transition(run, "archive_failure", { status: "RED", error: archiveFailure.message });
    result.transitions = [...run.transitions];
  }
  const state = loadState();
  atomicJson(STATE_PATH, {
    ...state,
    last_cycle: {
      run_id: run.id,
      status: result.status,
      commit: result.commit,
      finished: result.finished,
      refusal: refusal?.reason || null,
      resource_violations: violations,
    },
  });
  atomicJson(RESULT_PATH, result);
  return result;
}

async function killOwnedChildren(run) {
  await Promise.all([...run.children.keys()].map(async (pid) => {
    await killProcessGroup(pid);
    run.children.delete(pid);
  }));
}

function userUnitDirectory() {
  return resolve(process.env.XDG_CONFIG_HOME || join(HOME_DIR, ".config"), "systemd/user");
}

function renderUnit(name) {
  const templatePath = join(UNIT_SOURCE_ROOT, name);
  if (!existsSync(templatePath)) throw new Error(`unit template is missing: ${templatePath}`);
  return readFileSync(templatePath, "utf8").replaceAll("@REPO@", ROOT);
}

function machineOutput(value, json) {
  if (json) {
    process.stdout.write(`${JSON.stringify(value, null, 2)}\n`);
    return;
  }
  if (value.command === "status") {
    const target = value.target || {};
    const manifest = value.manifest || {};
    const conformance = value.conformance || {};
    const fuzz = value.fuzz || {};
    const redTeam = value.red_team || {};
    const tower = value.tower || {};
    const resources = value.resources || {};
    process.stdout.write(`HARDENING RIG  ${value.status}\n`);
    process.stdout.write(`target commit  ${target.commit || "unknown"} (${target.clean ? "clean" : "dirty"})\n`);
    process.stdout.write(`binary         ${target.binary_sha256 || "unknown"}\n`);
    process.stdout.write(`manifest       ${manifest.hash || "unknown"} ${manifest.stale ? "STALE" : "current"}\n`);
    process.stdout.write(`conformance    ${conformance.status || "RED"} ${JSON.stringify(conformance.totals || {})}\n`);
    for (const exclusion of conformance.exclusions || []) {
      process.stdout.write(`exclusion      ${exclusion.stable_id || "unknown"} ${exclusion.ratified ? "ratified" : "UNRATIFIED"} ${exclusion.reason || "missing reason"}\n`);
    }
    process.stdout.write(`fuzz           ${fuzz.status || "RED"} ${fuzz.clean_days || 0} clean days, ${fuzz.valid_cases || 0} valid cases, floor ${fuzz.lowest_row ?? "unknown"}/${fuzz.row_floor?.required ?? 100}\n`);
    process.stdout.write(`fuzz domains   ${JSON.stringify(fuzz.domain_distribution || {})}\n`);
    process.stdout.write(`fuzz findings  ${fuzz.silent_findings || 0} silent, seed ${fuzz.last_seed || "unknown"}${fuzz.invalidation_cause ? `, invalidated by ${fuzz.invalidation_cause}` : ""}\n`);
    process.stdout.write(`red team       ${redTeam.status || "RED"} ${redTeam.quota?.completed_lanes || 0}/${redTeam.quota?.lanes || 8} lanes, ${redTeam.unique_p0 || 0} unique P0\n`);
    process.stdout.write(`Tower P0       ${tower.open_p0 ?? "unknown"} ${tower.refs?.length ? tower.refs.join(",") : ""}\n`);
    process.stdout.write(`resources      ${resources.status || "RED"} memory ${resources.memory_available_gib ?? "unknown"}GiB, free ${resources.free_space_gib ?? "unknown"}GiB\n`);
    process.stdout.write(`target/cache   ${resources.target_gib ?? "unknown"}GiB / 80GiB, ${resources.cache_gib ?? "unknown"}GiB / 4GiB\n`);
    if (value.reasons?.length) process.stdout.write(`reasons        ${value.reasons.join("; ")}\n`);
    return;
  }
  process.stdout.write(`${value.status || value.state || value.command}\n`);
}

function statusReport() {
  const state = readJson(STATE_PATH);
  const result = readJson(RESULT_PATH);
  const resources = sizeReport();
  let memoryAvailableGib = null;
  try {
    const meminfo = readFileSync("/proc/meminfo", "utf8");
    const match = meminfo.match(/^MemAvailable:\s+(\d+) kB$/m);
    if (match) memoryAvailableGib = Number(match[1]) / 1024 ** 2;
  } catch {
    // Status remains useful when procfs is unavailable.
  }
  resources.memory_available_gib = memoryAvailableGib;
  const malformedState = state?.__error || null;
  const violations = capViolations(resources);
  const report = buildDashboard({
    root: ROOT,
    evidenceRoot: process.env.JET_HARDENING_EVIDENCE_DIR
      || process.env.JET_HARDENING_EVIDENCE
      || CACHE_ROOT,
    cacheRoot: CACHE_ROOT,
    targetRoot: TARGET_ROOT,
    binaryPath: JET_BINARY,
    towerCli: TOWER_CLI,
    towerData: TOWER_DATA,
    resources,
    capViolations: violations,
    state: malformedState ? { unreadable: malformedState } : state || null,
    result: result?.__error ? null : result || null,
  });
  report.scratch = SCRATCH_ROOT;
  report.cap_violations = report.resources.cap_violations;
  report.caps = report.resources.caps;
  return report;
}

function systemctl(args) {
  const program = process.env.JET_HARDENING_SYSTEMCTL || "systemctl";
  return spawnSync(program, ["--user", ...args], { cwd: ROOT, encoding: "utf8" });
}

function lifecycle(command, json) {
  if (command === "install") {
    const directory = userUnitDirectory();
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    const servicePath = join(directory, SERVICE_NAME);
    const timerPath = join(directory, TIMER_NAME);
    atomicWrite(servicePath, renderUnit(SERVICE_NAME));
    atomicWrite(timerPath, renderUnit(TIMER_NAME));
    const reload = systemctl(["daemon-reload"]);
    const value = {
      command,
      status: reload.status === 0 ? "INSTALLED" : "INSTALLED_RELOAD_FAILED",
      service: servicePath,
      timer: timerPath,
      daemon_reload_exit: reload.status,
      stderr: String(reload.stderr || "").trim(),
    };
    machineOutput(value, json);
    return reload.status === 0 ? 0 : 1;
  }
  if (command === "start") {
    const result = systemctl(["enable", "--now", TIMER_NAME]);
    const value = { command, status: result.status === 0 ? "STARTED" : "START_FAILED", exit: result.status, stderr: String(result.stderr || "").trim() };
    machineOutput(value, json);
    return result.status === 0 ? 0 : 1;
  }
  if (command === "stop") {
    const result = systemctl(["disable", "--now", TIMER_NAME, SERVICE_NAME]);
    const value = { command, status: result.status === 0 ? "STOPPED" : "STOP_FAILED", exit: result.status, stderr: String(result.stderr || "").trim() };
    machineOutput(value, json);
    return result.status === 0 ? 0 : 1;
  }
  throw new Error(`unknown lifecycle command: ${command}`);
}

function help() {
  process.stdout.write([
    "usage: hardening-rig.mjs <cycle|status|install|start|stop> [--json]",
    "",
    "cycle runs one bounded pass. The timer owns repetition.",
    "cycle options: --simulate=stale-lease|dirty|busy|memory|target|cache|timeout|signal|test-failure|build-failure",
  ].join("\n") + "\n");
}

async function handleSignal(signal) {
  if (handlingSignal) return;
  handlingSignal = true;
  requestedSignal = signal;
  if (currentRun) {
    const run = currentRun;
    run.signal = signal;
    await killOwnedChildren(run);
    let mutationCleanupError = null;
    try {
      await run.mutation_cleanup?.();
    } catch (error) {
      mutationCleanupError = error;
    }
    const result = baseResult(run, run.config, run.identity, manifestIdentity());
    result.status = "RED";
    result.failure_stage = "signal";
    result.signal = signal;
    result.finished = now();
    result.cleanup = {
      signal,
      children: true,
      mutation_restored: mutationCleanupError === null,
      mutation_cleanup_error: mutationCleanupError?.message || null,
      scratch_removed: run.scratch ? removeScratch(run.scratch, run) : true,
    };
    atomicJson(FAILURE_PATH, failureBundle(result, null, new Error(`received ${signal}`)));
    writeFailureLog(result);
    try {
      run.buildLease?.release();
      run.rigLease?.release();
      transition(run, "signal", { status: "RED", signal });
      try {
        archiveCycle(result);
      } catch {
        // Preserve the signal result even when the cache cannot accept another record.
      }
      atomicJson(RESULT_PATH, result);
    } catch {
      // Preserve signal exit even if state storage is unavailable.
    }
  }
  process.exit(128 + ({ SIGHUP: 1, SIGINT: 2, SIGTERM: 15 }[signal] || 1));
}

process.on("SIGHUP", () => void handleSignal("SIGHUP"));
process.on("SIGINT", () => void handleSignal("SIGINT"));
process.on("SIGTERM", () => void handleSignal("SIGTERM"));
async function main() {
  const args = process.argv.slice(2);
  const commandIndex = args.findIndex((arg) => !arg.startsWith("--"));
  const command = commandIndex < 0 ? "status" : args[commandIndex];
  const json = args.includes("--json");
  const simulationArg = args.find((arg) => arg.startsWith("--simulate="));
  const simulationIndex = args.indexOf("--simulate");
  const simulate = simulationArg
    ? simulationArg.slice("--simulate=".length)
    : simulationIndex >= 0
      ? args[simulationIndex + 1]
      : process.env.JET_HARDENING_SIMULATE || null;
  if (command === "red-team") return redTeamMain(args.slice(commandIndex + 1));
  if (command === "help" || command === "--help" || command === "-h") {
    help();
    return 0;
  }
  if (command === "status") {
    machineOutput(statusReport(), json);
    return 0;
  }
  if (["install", "start", "stop"].includes(command)) return lifecycle(command, json);
  if (command === "cycle") {
    const result = await runCycle({ simulate });
    machineOutput(result, json);
    return result.status === "PASS" || result.status === "SKIPPED" ? 0 : 1;
  }
  throw new Error(`unknown command: ${command}`);
}

main().then((code) => {
  if (!requestedSignal) process.exit(code);
}).catch((error) => {
  if (requestedSignal) return;
  process.stderr.write(`hardening-rig: ${error.stack || error.message}\n`);
  process.exit(1);
});
