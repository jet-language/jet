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
const FAILURE_PATH = join(CACHE_ROOT, "failure.json");
const FAILURE_LOG_PATH = join(CACHE_ROOT, "logs/failure.log");
const INTERESTING_ROOT = join(CACHE_ROOT, "interesting");
const RIG_LEASE_PATH = join(CACHE_ROOT, "rig.lock");
const BUILD_LEASE_PATH = join(TARGET_ROOT, ".jet-hardening-build.lock");

const GIB = 1024 ** 3;
const MIN_MEMORY_GIB = 16;
const TARGET_CAP_BYTES = 80 * GIB;
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
  if (!existsSync(CACHE_ROOT)) return;
  for (const name of readdirSync(CACHE_ROOT)) {
    if (!name.includes(".tmp-") || !["state.json", "result.json", "failure.json"].some((base) => name.startsWith(`${base}.tmp-`))) {
      continue;
    }
    try {
      unlinkSync(join(CACHE_ROOT, name));
    } catch {
      // A concurrent invocation owns it; its lease decides whether that is safe.
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

async function runCommand(label, program, args, environment, timeoutMs = DEFAULT_TIMEOUT_MS) {
  const command = commandText(program, args);
  const childEnv = { ...process.env, ...environment };
  const child = spawn(resolveCommand(program), args, {
    cwd: ROOT,
    detached: true,
    env: childEnv,
    stdio: ["ignore", "pipe", "pipe"],
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

  let killPromise = null;
  const timer = setTimeout(() => {
    record.timed_out = true;
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

function config() {
  const proofTargets = csvEnv("JET_HARDENING_PROOF_TARGETS", DEFAULT_PROOF_TARGETS);
  const shards = csvEnv("JET_HARDENING_SHARDS", DEFAULT_SHARDS);
  validateNames(proofTargets, "proof targets");
  validateNames(shards, "shards");
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
  transition(run, "status", { status: result.status, violations });
  result.transitions = [...run.transitions];
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
    process.stdout.write(`HARDENING RIG  ${value.status}\n`);
    process.stdout.write(`cycle  ${value.last_cycle?.status || "IDLE"}\n`);
    if (value.last_cycle?.refusal) process.stdout.write(`refusal  ${value.last_cycle.refusal}\n`);
    process.stdout.write(`memory  ${value.resources.memory_available_gib ?? "unknown"}GiB available\n`);
    process.stdout.write(`target  ${value.resources.target_gib ?? "unknown"}GiB / 80GiB\n`);
    process.stdout.write(`cache   ${value.resources.cache_gib ?? "unknown"}GiB / 4GiB\n`);
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
  const status = malformedState || state?.blocked || violations.length || result?.status === "RED" ? "RED" : "NOT READY";
  return {
    command: "status",
    status,
    root: ROOT,
    target: TARGET_ROOT,
    binary: JET_BINARY,
    cache: CACHE_ROOT,
    scratch: SCRATCH_ROOT,
    state: malformedState ? { unreadable: malformedState } : state || null,
    last_cycle: state?.last_cycle || null,
    last_result: result || null,
    manifest: manifestIdentity(),
    resources,
    cap_violations: violations,
    caps: { target_gib: 80, cache_gib: 4, interesting_mib: 512, log_mib: 1 },
  };
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
    const result = baseResult(run, run.config, run.identity, manifestIdentity());
    result.status = "RED";
    result.failure_stage = "signal";
    result.signal = signal;
    result.finished = now();
    result.cleanup = { signal, children: true, scratch_removed: run.scratch ? removeScratch(run.scratch, run) : true };
    atomicJson(FAILURE_PATH, failureBundle(result, null, new Error(`received ${signal}`)));
    writeFailureLog(result);
    try {
      run.buildLease?.release();
      run.rigLease?.release();
      transition(run, "signal", { status: "RED", signal });
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
  const command = args.find((arg) => !arg.startsWith("--")) || "status";
  const json = args.includes("--json");
  const simulationArg = args.find((arg) => arg.startsWith("--simulate="));
  const simulationIndex = args.indexOf("--simulate");
  const simulate = simulationArg
    ? simulationArg.slice("--simulate=".length)
    : simulationIndex >= 0
      ? args[simulationIndex + 1]
      : process.env.JET_HARDENING_SIMULATE || null;
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
