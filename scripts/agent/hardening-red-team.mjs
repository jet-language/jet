#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  createHash,
  createPrivateKey,
  createPublicKey,
  generateKeyPairSync,
  sign,
  verify,
} from "node:crypto";
import {
  closeSync,
  existsSync,
  fsyncSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  MUTATOR_VERSION,
  SCHEMA_VERSION as ORACLE_SCHEMA_VERSION,
  bundleIdentity,
  canonicalJson,
  executeCommand,
  oracleCatalog,
  validateResultBundle,
} from "./hardening-oracle-layer.mjs";

export const RED_TEAM_SCHEMA_VERSION = 1;
export const RED_TEAM_SESSION_SCHEMA = "jet.hardening.red-team.session.v1";
export const RED_TEAM_PACKET_SCHEMA = "jet.hardening.red-team.context.v1";
export const RED_TEAM_LANE_SCHEMA = "jet.hardening.red-team.lane.v1";
export const RED_TEAM_RECEIPT_SCHEMA = "jet.hardening.red-team.receipt.v1";
export const RED_TEAM_LANE_COUNT = 8;
export const RED_TEAM_WAVE_COUNT = 4;
export const RED_TEAM_MAX_ACTIVE = 2;
export const RED_TEAM_MODEL = "gpt-5.6-luna";
export const RED_TEAM_REASONING = "max";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(SCRIPT_DIR, "../..");
const HOME_DIR = process.env.HOME || os.homedir();
const DEFAULT_CACHE = resolve(
  process.env.JET_HARDENING_CACHE || join(HOME_DIR, ".cache/jet-hardening/v1"),
);
const DEFAULT_BINARY = (root) => join(root, "target/debug/jet");
const MAX_SOURCE_BYTES = 512 * 1024;
const MAX_CAPTURE_BYTES = 256 * 1024;
const DEFAULT_LANE_TIMEOUT_MS = 10 * 60 * 1000;
const MAX_LANE_TIMEOUT_MS = 10 * 60 * 1000;
const SHA256_PATTERN = /^sha256:[0-9a-f]{64}$/;
const COMMIT_PATTERN = /^[0-9a-f]{40}$/i;
const LANE_ID_PATTERN = /^lane-[1-8]$/;
const CONTEXT_ID_PATTERN = /^[A-Za-z0-9_.:/-]{1,160}$/;
const FINDING_ID_PATTERN = /^[A-Za-z0-9_.:/-]{1,160}$/;
const EXECUTION_GATE = "OWNER_REQUIRED_FOR_REAL_EIGHT_LANE_EXECUTION";

const LANE_BRIEFS = Object.freeze([
  {
    lane_id: "lane-1",
    wave: 1,
    surface: "tier-seams-nested-places",
    attack_surface: "Tier seams and nested places.",
    brief: "Exercise nested place reads and writes through every applicable execution tier.",
  },
  {
    lane_id: "lane-2",
    wave: 1,
    surface: "numeric-representation-extremes",
    attack_surface: "Numeric and representation extremes.",
    brief: "Exercise numeric boundaries, packed representations, and serialization extremes.",
  },
  {
    lane_id: "lane-3",
    wave: 2,
    surface: "optimization-dev-release-interpreter",
    attack_surface: "Dev, release, forced-interpreter, and optimization paths.",
    brief: "Compare optimization, dev, release, and forced-interpreter behavior for one meaning.",
  },
  {
    lane_id: "lane-4",
    wave: 2,
    surface: "process-input-and-resources",
    attack_surface: "stdin, argv, env, files, exit, and resource limits.",
    brief: "Exercise process boundaries and bounded resource failures with value-consuming checks.",
  },
  {
    lane_id: "lane-5",
    wave: 3,
    surface: "core-host-effects-exclusions",
    attack_surface: "Core host/effect surfaces and exclusions.",
    brief: "Probe host and effect Core surfaces, including every named exclusion boundary.",
  },
  {
    lane_id: "lane-6",
    wave: 3,
    surface: "concurrency-cancellation",
    attack_surface: "Concurrency and cancellation.",
    brief: "Exercise cancellation, joining, ordering, and cleanup under bounded concurrency.",
  },
  {
    lane_id: "lane-7",
    wave: 4,
    surface: "parser-sema-tir-boundaries",
    attack_surface: "Parser, sema, TIR, and construct boundaries.",
    brief: "Cross construct boundaries from parser admission through sema and TIR lowering.",
  },
  {
    lane_id: "lane-8",
    wave: 4,
    surface: "cross-domain-compositions",
    attack_surface: "Cross-domain compositions.",
    brief: "Compose independent Core and language domains to expose seam interactions.",
  },
]);

const WAVE_LANES = Object.freeze([
  Object.freeze(["lane-1", "lane-2"]),
  Object.freeze(["lane-3", "lane-4"]),
  Object.freeze(["lane-5", "lane-6"]),
  Object.freeze(["lane-7", "lane-8"]),
]);
export const RED_TEAM_LANE_BRIEFS = Object.freeze(LANE_BRIEFS.map(clone));
export const RED_TEAM_WAVES = Object.freeze(WAVE_LANES.map((lanes, index) => ({ wave: index + 1, lanes: [...lanes] })));

export class RedTeamProtocolError extends Error {
  constructor(message, code = "E_RED_TEAM") {
    super(message);
    this.name = "RedTeamProtocolError";
    this.code = code;
  }
}


function fail(message, code = "E_RED_TEAM") {
  throw new RedTeamProtocolError(message, code);
}

function clone(value) {
  return value == null ? value : JSON.parse(JSON.stringify(value));
}

function deepFreeze(value) {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) return value;
  Object.freeze(value);
  for (const child of Object.values(value)) deepFreeze(child);
  return value;
}

function requiredString(value, label) {
  if (typeof value !== "string" || !value.trim()) fail(`${label} must be a non-empty string`, "E_SCHEMA");
  return value;
}

function digest(value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(String(value), "utf8");
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function validDigest(value) {
  return typeof value === "string" && SHA256_PATTERN.test(value);
}

function validCommit(value) {
  return typeof value === "string" && COMMIT_PATTERN.test(value);
}

function now() {
  return new Date().toISOString();
}

function safeRelative(root, path) {
  const rel = relative(resolve(root), resolve(path));
  if (!rel || rel.startsWith("..") || resolve(rel) === ".") return rel || ".";
  return rel;
}

function requiredObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`, "E_SCHEMA");
  return value;
}

function boundedInteger(value, label, minimum, maximum) {
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    fail(`${label} must be an integer from ${minimum} through ${maximum}`, "E_SCHEMA");
  }
  return value;
}

function validateSnapshot(snapshot, label) {
  requiredObject(snapshot, label);
  requiredString(snapshot.path, `${label}.path`);
  if (isAbsolute(snapshot.path) || snapshot.path.split(/[\\/]/).includes("..")) fail(`${label}.path must stay inside the frozen checkout`, "E_MANIFEST");
  if (!validDigest(snapshot.sha256)) fail(`${label}.sha256 is invalid`, "E_MANIFEST");
  if (snapshot.source_snapshot_hash !== undefined && !validDigest(snapshot.source_snapshot_hash)) {
    fail(`${label}.source_snapshot_hash is invalid`, "E_MANIFEST");
  }
}

function validateRigConfig(config) {
  requiredObject(config, "manifest rig_config");
  boundedInteger(config.schema_version, "manifest rig_config.schema_version", 1, 1);
  boundedInteger(config.suite_concurrency, "manifest rig_config.suite_concurrency", 1, RED_TEAM_MAX_ACTIVE);
  boundedInteger(config.cargo_build_jobs, "manifest rig_config.cargo_build_jobs", 1, 64);
  requiredString(config.seed, "manifest rig_config.seed");
  requiredString(config.variants, "manifest rig_config.variants");
  for (const key of ["proof_targets", "deterministic_shards"]) {
    if (config[key] === undefined) continue;
    if (!Array.isArray(config[key]) || config[key].some((item) => typeof item !== "string" || !item.trim())) {
      fail(`manifest rig_config.${key} must be a list of names`, "E_MANIFEST");
    }
  }
  for (const [key, minimum, maximum] of [
    ["oracle_batch_size", 1, 512],
    ["oracle_max_cases", 1, 4096],
    ["oracle_timeout_ms", 1, MAX_LANE_TIMEOUT_MS],
  ]) {
    if (config[key] !== undefined) boundedInteger(config[key], `manifest rig_config.${key}`, minimum, maximum);
  }
}

function validateOracleVersions(versions) {
  requiredObject(versions, "manifest oracle_versions");
  if (versions.layer_schema_version !== ORACLE_SCHEMA_VERSION) fail("manifest oracle layer schema is not frozen", "E_MANIFEST");
  requiredString(versions.mutator_version, "manifest oracle_versions.mutator_version");
  if (!Array.isArray(versions.adapters) || versions.adapters.length === 0) fail("manifest oracle adapters are missing", "E_MANIFEST");
  const domains = new Set();
  for (const [index, adapter] of versions.adapters.entries()) {
    requiredObject(adapter, `manifest oracle_versions.adapters[${index}]`);
    for (const key of ["domain", "oracle", "version", "provenance"]) {
      requiredString(adapter[key], `manifest oracle_versions.adapters[${index}].${key}`);
    }
    if (domains.has(adapter.domain)) fail(`manifest oracle_versions repeats domain ${adapter.domain}`, "E_MANIFEST");
    domains.add(adapter.domain);
    if (adapter.independence_class !== undefined) requiredString(adapter.independence_class, `manifest oracle_versions.adapters[${index}].independence_class`);
    if (adapter.normalization !== undefined && (!Array.isArray(adapter.normalization) || adapter.normalization.some((item) => typeof item !== "string" || !item))) {
      fail(`manifest oracle_versions.adapters[${index}].normalization must be a named list`, "E_MANIFEST");
    }
  }
  if (!validDigest(versions.catalog_sha256) || versions.catalog_sha256 !== digest(canonicalJson(versions.adapters))) {
    fail("manifest oracle catalog digest does not match its adapters", "E_MANIFEST");
  }
}

function targetMatches(manifest, target) {
  return Boolean(target)
    && target.commit === manifest.target.commit
    && target.binary_sha256 === manifest.target.binary_sha256
    && target.root === manifest.target.root
    && target.binary_path === manifest.target.binary_path
    && target.platform === manifest.target.platform
    && target.arch === manifest.target.arch
    && target.registry_snapshot?.sha256 === manifest.registry_snapshot.sha256
    && target.public_surface_snapshot?.sha256 === manifest.public_surface_snapshot.sha256;
}

function atomicWrite(path, contents) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.tmp-${process.pid}-${Date.now()}`;
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
      // Preserve the original write error.
    }
    throw error;
  }
}

function writeJson(path, value) {
  atomicWrite(path, `${JSON.stringify(value, null, 2)}\n`);
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`cannot read JSON ${path}: ${error.message}`, "E_INPUT");
  }
}

function binaryHash(path) {
  if (!existsSync(path)) fail(`frozen binary is missing: ${path}`, "E_TARGET");
  try {
    if (!statSync(path).isFile()) fail(`frozen binary is not a regular file: ${path}`, "E_TARGET");
    return digest(readFileSync(path));
  } catch (error) {
    if (error instanceof RedTeamProtocolError) throw error;
    fail(`cannot hash frozen binary: ${error.message}`, "E_TARGET");
  }
}

function gitSnapshot(root) {
  const status = spawnSync("git", ["-C", root, "status", "--porcelain=v1", "--untracked-files=all"], {
    encoding: "utf8",
  });
  if (status.status !== 0) fail(`git status failed: ${String(status.stderr || "").trim() || "unknown error"}`, "E_TARGET");
  const dirty = String(status.stdout || "").trim();
  if (dirty) fail("red-team target must be a clean checkout", "E_TARGET");
  const commit = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (commit.status !== 0) fail("git commit identity unavailable", "E_TARGET");
  return { commit: String(commit.stdout || "").trim(), dirty_paths: [] };
}

function registryPath(root, configured) {
  const candidates = configured
    ? [resolve(root, configured)]
    : [
        join(root, ".jet/hardening-manifest.json"),
        join(root, ".jet/core-conformance-inventory.json"),
        join(root, "tests/conformance/manifest.json"),
        join(root, "tests/conformance/manifest.tsv"),
      ];
  return candidates.find((path) => existsSync(path)) || null;
}

function registrySnapshot(root, configured) {
  const path = registryPath(root, configured);
  if (!path) fail("red-team registry/public-surface snapshot is missing", "E_REGISTRY");
  const bytes = readFileSync(path);
  let parsed = null;
  if (path.endsWith(".json")) {
    try {
      parsed = JSON.parse(bytes.toString("utf8"));
    } catch (error) {
      fail(`red-team registry is not valid JSON: ${error.message}`, "E_REGISTRY");
    }
  }
  const hash = digest(bytes);
  const sourceSnapshotHash = parsed?.source_snapshot?.hash || parsed?.sourceSnapshot?.hash || hash;
  return {
    path: safeRelative(root, path),
    sha256: hash,
    source_snapshot_hash: sourceSnapshotHash,
  };
}

function defaultRigConfig() {
  const number = (name, fallback, minimum, maximum) => {
    const raw = process.env[name];
    if (raw == null || raw === "") return fallback;
    const value = Number(raw);
    if (!Number.isInteger(value) || value < minimum || value > maximum) {
      fail(`${name} must be an integer from ${minimum} through ${maximum}`, "E_CONFIG");
    }
    return value;
  };
  return {
    schema_version: 1,
    suite_concurrency: 2,
    cargo_build_jobs: 4,
    seed: process.env.JET_HARDENING_SEED || "2336",
    variants: process.env.JET_HARDENING_VARIANTS || "50",
    proof_targets: String(process.env.JET_HARDENING_PROOF_TARGETS || "dev_corpus_gate")
      .split(",").map((item) => item.trim()).filter(Boolean),
    deterministic_shards: String(process.env.JET_HARDENING_SHARDS || "fuzz_sema,sema_soundness_differential")
      .split(",").map((item) => item.trim()).filter(Boolean),
    oracle_batch_size: number("JET_HARDENING_ORACLE_BATCH_SIZE", 32, 1, 512),
    oracle_max_cases: number("JET_HARDENING_ORACLE_MAX_CASES", 128, 1, 4096),
    oracle_timeout_ms: number("JET_HARDENING_ORACLE_TIMEOUT_MS", 30_000, 1, 600_000),
  };
}

function oracleVersions() {
  const adapters = oracleCatalog().map((item) => ({
    domain: item.domain,
    oracle: item.oracle,
    version: item.version,
    provenance: item.provenance,
  }));
  return {
    layer_schema_version: ORACLE_SCHEMA_VERSION,
    mutator_version: MUTATOR_VERSION,
    adapters,
    catalog_sha256: digest(canonicalJson(adapters)),
  };
}

function defaultResourceLimits(rigConfig = {}) {
  return {
    max_active_lanes: RED_TEAM_MAX_ACTIVE,
    lane_timeout_ms: Number(rigConfig.red_team_lane_timeout_ms || DEFAULT_LANE_TIMEOUT_MS),
    capture_bytes: MAX_CAPTURE_BYTES,
    target_cap_gib: 80,
    cache_cap_gib: 4,
    interesting_cap_mib: 512,
    log_cap_mib: 1,
    scratch: "disk-backed cache scratch only",
    cleanup: "process-group, agent, alternate-target, scratch, and bounded-log cleanup is mandatory",
  };
}

function laneBriefs() {
  return LANE_BRIEFS.map(clone);
}

function quota() {
  return {
    lanes: RED_TEAM_LANE_COUNT,
    waves: RED_TEAM_WAVE_COUNT,
    lanes_per_wave: RED_TEAM_MAX_ACTIVE,
    full_quota_required: true,
    min_attempts_per_lane: 1,
  };
}

function unsignedManifest(manifest) {
  const copy = clone(manifest);
  delete copy.manifest_sha256;
  return copy;
}

export function sessionManifestDigest(manifest) {
  return digest(canonicalJson(unsignedManifest(manifest)));
}

function validateLaneBriefs(briefs) {
  if (!Array.isArray(briefs) || briefs.length !== RED_TEAM_LANE_COUNT) fail("manifest must contain all eight lane briefs", "E_MANIFEST");
  const seen = new Set();
  for (const brief of briefs) {
    if (!brief || !LANE_ID_PATTERN.test(brief.lane_id) || seen.has(brief.lane_id)) fail("manifest lane briefs must name each lane once", "E_MANIFEST");
    seen.add(brief.lane_id);
    const expected = LANE_BRIEFS.find((item) => item.lane_id === brief.lane_id);
    if (!expected || canonicalJson(brief) !== canonicalJson(expected)) fail(`lane ${brief.lane_id} does not match its ratified attack slice`, "E_MANIFEST");
  }
  if (seen.size !== RED_TEAM_LANE_COUNT) fail("manifest lane briefs omit a lane", "E_MANIFEST");
}

export function validateSessionManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) fail("red-team session manifest must be an object", "E_MANIFEST");
  if (manifest.schema !== RED_TEAM_SESSION_SCHEMA || manifest.schema_version !== RED_TEAM_SCHEMA_VERSION) {
    fail("unsupported red-team session manifest schema", "E_MANIFEST");
  }
  requiredString(manifest.session_id, "manifest session_id");
  requiredString(manifest.created_at, "manifest created_at");
  if (Number.isNaN(Date.parse(manifest.created_at))) fail("manifest created_at is not an ISO timestamp", "E_MANIFEST");
  const target = manifest.target;
  requiredObject(target, "manifest target");
  if (!validCommit(target.commit)) fail("manifest target commit is not a full commit hash", "E_MANIFEST");
  if (!validDigest(target.binary_sha256)) fail("manifest target binary hash is invalid", "E_MANIFEST");
  requiredString(target.root, "manifest target root");
  requiredString(target.binary_path, "manifest target binary path");
  if (isAbsolute(target.binary_path) || target.binary_path.split(/[\\/]/).includes("..")) fail("manifest target binary path must stay inside the frozen checkout", "E_MANIFEST");
  requiredString(target.platform, "manifest target platform");
  requiredString(target.arch, "manifest target arch");
  validateSnapshot(manifest.registry_snapshot, "manifest registry_snapshot");
  validateSnapshot(manifest.public_surface_snapshot, "manifest public_surface_snapshot");
  validateRigConfig(manifest.rig_config);
  if (!validDigest(manifest.rig_config_sha256) || manifest.rig_config_sha256 !== digest(canonicalJson(manifest.rig_config))) {
    fail("manifest rig config digest does not match its config", "E_MANIFEST");
  }
  validateOracleVersions(manifest.oracle_versions);
  requiredObject(manifest.agent_policy, "manifest agent_policy");
  if (String(manifest.agent_policy.model).toLowerCase() !== RED_TEAM_MODEL
    || manifest.agent_policy.reasoning_effort !== RED_TEAM_REASONING
    || manifest.agent_policy.fresh_context !== true) {
    fail("manifest agent policy is not fresh Luna-max", "E_MANIFEST");
  }
  validateLaneBriefs(manifest.lane_briefs);
  const expectedWaves = WAVE_LANES.map((lanes, index) => ({ wave: index + 1, lanes: [...lanes] }));
  if (!Array.isArray(manifest.waves) || canonicalJson(manifest.waves) !== canonicalJson(expectedWaves)) {
    fail("manifest waves must be the four frozen waves of two", "E_MANIFEST");
  }
  const expectedQuota = manifest.quota;
  if (!expectedQuota || expectedQuota.lanes !== RED_TEAM_LANE_COUNT || expectedQuota.waves !== RED_TEAM_WAVE_COUNT
    || expectedQuota.lanes_per_wave !== RED_TEAM_MAX_ACTIVE || expectedQuota.full_quota_required !== true
    || expectedQuota.min_attempts_per_lane !== 1) {
    fail("manifest quota must require eight lanes in four waves of two", "E_MANIFEST");
  }
  const limits = manifest.resource_limits;
  if (!limits || limits.max_active_lanes !== RED_TEAM_MAX_ACTIVE || !Number.isInteger(limits.lane_timeout_ms)
    || limits.lane_timeout_ms < 1 || limits.lane_timeout_ms > MAX_LANE_TIMEOUT_MS) {
    fail("manifest resource limits must cap active lanes at two", "E_MANIFEST");
  }
  boundedInteger(limits.capture_bytes, "manifest resource_limits.capture_bytes", 1, MAX_CAPTURE_BYTES);
  boundedInteger(limits.target_cap_gib, "manifest resource_limits.target_cap_gib", 1, 80);
  boundedInteger(limits.cache_cap_gib, "manifest resource_limits.cache_cap_gib", 1, 4);
  boundedInteger(limits.interesting_cap_mib, "manifest resource_limits.interesting_cap_mib", 1, 512);
  boundedInteger(limits.log_cap_mib, "manifest resource_limits.log_cap_mib", 1, 1);
  requiredString(limits.scratch, "manifest resource_limits.scratch");
  requiredString(limits.cleanup, "manifest resource_limits.cleanup");
  if (manifest.current_defect_cards_hidden !== true) fail("manifest must hide current defect cards before discovery", "E_MANIFEST");
  if (manifest.discovery_rule !== "known Tower defect cards are unavailable until all independent lane receipts are recorded") {
    fail("manifest discovery rule is not frozen", "E_MANIFEST");
  }
  if (manifest.execution_gate !== EXECUTION_GATE) fail("manifest execution gate is not frozen", "E_MANIFEST");
  if (manifest.commit !== manifest.target.commit || manifest.binary_sha256 !== manifest.target.binary_sha256) {
    fail("manifest duplicate target identity is inconsistent", "E_MANIFEST");
  }
  if (!validDigest(manifest.manifest_sha256) || sessionManifestDigest(manifest) !== manifest.manifest_sha256) {
    fail("manifest digest does not cover the frozen session inputs", "E_MANIFEST");
  }
  return true;
}

export function createSessionManifest({
  root = DEFAULT_ROOT,
  session_id = undefined,
  commit = undefined,
  binary_sha256 = undefined,
  binary_path = process.env.JET_HARDENING_BINARY,
  registry_snapshot = undefined,
  public_surface_snapshot = undefined,
  rig_config = undefined,
  oracle_versions = undefined,
  created_at = undefined,
  resource_limits = undefined,
  lane_briefs: requestedBriefs = undefined,
} = {}) {
  const resolvedRoot = resolve(root);
  const targetPath = resolve(binary_path || DEFAULT_BINARY(resolvedRoot));
  const identity = commit ? { commit } : gitSnapshot(resolvedRoot);
  const hash = binary_sha256 || binaryHash(targetPath);
  if (!validDigest(hash)) fail("manifest binary_sha256 must be a sha256 digest", "E_TARGET");
  const registry = registry_snapshot || registrySnapshot(resolvedRoot, process.env.JET_HARDENING_REGISTRY);
  const config = clone(rig_config || defaultRigConfig());
  const publicSnapshot = clone(registry);
  const manifest = {
    schema: RED_TEAM_SESSION_SCHEMA,
    schema_version: RED_TEAM_SCHEMA_VERSION,
    session_id: session_id || `red-team-${Date.now()}-${process.pid}`,
    created_at: created_at || now(),
    target: {
      root: safeRelative(resolvedRoot, resolvedRoot) === "." ? "." : resolvedRoot,
      commit: identity.commit,
      binary_path: safeRelative(resolvedRoot, targetPath),
      binary_sha256: hash,
      platform: process.platform,
      arch: process.arch,
    },
    commit: identity.commit,
    binary_sha256: hash,
    registry_snapshot: clone(registry),
    public_surface_snapshot: clone(public_surface_snapshot || publicSnapshot),
    rig_config: config,
    rig_config_sha256: digest(canonicalJson(config)),
    oracle_versions: clone(oracle_versions || oracleVersions()),
    lane_briefs: clone(requestedBriefs || laneBriefs()),
    waves: WAVE_LANES.map((lanes, index) => ({ wave: index + 1, lanes: [...lanes] })),
    quota: quota(),
    resource_limits: clone(resource_limits || defaultResourceLimits(config)),
    agent_policy: {
      model: RED_TEAM_MODEL,
      reasoning_effort: RED_TEAM_REASONING,
      fresh_context: true,
    },
    current_defect_cards_hidden: true,
    discovery_rule: "known Tower defect cards are unavailable until all independent lane receipts are recorded",
    execution_gate: EXECUTION_GATE,
  };
  manifest.manifest_sha256 = sessionManifestDigest(manifest);
  validateSessionManifest(manifest);
  return deepFreeze(manifest);
}

export function writeSessionManifest(path, manifest) {
  validateSessionManifest(manifest);
  const destination = resolve(path);
  if (existsSync(destination)) {
    const existing = readJson(destination);
    validateSessionManifest(existing);
    if (sessionManifestDigest(existing) !== sessionManifestDigest(manifest)) {
      fail(`session manifest already exists with a different frozen identity: ${destination}`, "E_MANIFEST_LOCKED");
    }
    return deepFreeze(existing);
  }
  writeJson(destination, manifest);
  return manifest;
}

export function readSessionManifest(path) {
  const manifest = readJson(path);
  validateSessionManifest(manifest);
  return deepFreeze(manifest);
}

function packetUnsigned(packet) {
  const copy = clone(packet);
  delete copy.context_digest;
  return copy;
}

export function contextPacketDigest(packet) {
  return digest(canonicalJson(packetUnsigned(packet)));
}

export function makeContextPacket(manifest, laneId) {
  validateSessionManifest(manifest);
  const brief = manifest.lane_briefs.find((item) => item.lane_id === laneId);
  if (!brief) fail(`unknown red-team lane ${laneId}`, "E_PACKET");
  const packet = {
    schema: RED_TEAM_PACKET_SCHEMA,
    schema_version: RED_TEAM_SCHEMA_VERSION,
    session_id: manifest.session_id,
    manifest_sha256: manifest.manifest_sha256,
    lane_id: brief.lane_id,
    wave: brief.wave,
    attack_surface: brief.attack_surface,
    brief: brief.brief,
    mission: "Find a new P0 without relying on current defect cards; consume every tested value observably.",
    target: clone(manifest.target),
    public_surface_snapshot: clone(manifest.public_surface_snapshot),
    agent_policy: clone(manifest.agent_policy),
    rig_config_sha256: manifest.rig_config_sha256,
    oracle_versions: clone(manifest.oracle_versions),
    quota: clone(manifest.quota),
    resource_limits: clone(manifest.resource_limits),
    visibility: {
      current_defect_cards: "hidden",
      known_findings: "omitted",
      reveal_after: "all-eight-independent-receipts",
    },
    forbidden_inputs: ["current Tower defect cards", "other lane receipts", "post-discovery dedup results"],
  };
  packet.context_digest = contextPacketDigest(packet);
  return deepFreeze(packet);
}

export function makeContextPackets(manifest) {
  validateSessionManifest(manifest);
  return manifest.lane_briefs.map((brief) => makeContextPacket(manifest, brief.lane_id));
}
export function validateContextPacket(packet, manifest = undefined) {
  if (!packet || packet.schema !== RED_TEAM_PACKET_SCHEMA || packet.schema_version !== RED_TEAM_SCHEMA_VERSION) fail("invalid red-team context packet schema", "E_PACKET");
  requiredString(packet.session_id, "context packet session_id");
  if (!LANE_ID_PATTERN.test(packet.lane_id)) fail("context packet lane_id is invalid", "E_PACKET");
  if (manifest && packet.session_id !== manifest.session_id) fail("context packet belongs to another session", "E_PACKET");
  if (manifest && packet.manifest_sha256 !== manifest.manifest_sha256) fail("context packet manifest is not frozen", "E_PACKET");
  if (manifest) {
    const brief = manifest.lane_briefs.find((item) => item.lane_id === packet.lane_id);
    if (!brief || packet.wave !== brief.wave || packet.attack_surface !== brief.attack_surface || packet.brief !== brief.brief) {
      fail("context packet does not match its frozen lane brief", "E_PACKET");
    }
    if (!targetMatches(manifest, {
      ...packet.target,
      registry_snapshot: manifest.registry_snapshot,
      public_surface_snapshot: packet.public_surface_snapshot,
    })) fail("context packet target is not frozen", "E_PACKET");
    if (canonicalJson(packet.agent_policy) !== canonicalJson(manifest.agent_policy)
      || packet.rig_config_sha256 !== manifest.rig_config_sha256
      || canonicalJson(packet.oracle_versions) !== canonicalJson(manifest.oracle_versions)
      || canonicalJson(packet.quota) !== canonicalJson(manifest.quota)
      || canonicalJson(packet.resource_limits) !== canonicalJson(manifest.resource_limits)) {
      fail("context packet does not carry the frozen session inputs", "E_PACKET");
    }
    if (packet.public_surface_snapshot?.sha256 !== manifest.public_surface_snapshot.sha256) {
      fail("context packet public surface is not frozen", "E_PACKET");
    }
  }
  if (packet.visibility?.current_defect_cards !== "hidden" || packet.visibility?.known_findings !== "omitted") {
    fail("context packet exposes current defect cards", "E_PACKET");
  }
  if (!packet.target || !packet.public_surface_snapshot || !packet.agent_policy) {
    fail("context packet is missing frozen session inputs", "E_PACKET");
  }
  for (const forbidden of ["defect_cards", "known_defects", "known_findings", "tower_cards"]) {
    if (Object.hasOwn(packet, forbidden)) fail(`context packet contains forbidden ${forbidden}`, "E_PACKET");
  }
  if (packet.context_digest !== contextPacketDigest(packet)) fail("context packet digest changed", "E_PACKET");
  return true;
}

function stripComments(source) {
  return String(source)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/[^\n]*/g, "");
}

export function isValueConsumingSource(source) {
  if (typeof source !== "string" || !source.trim() || Buffer.byteLength(source, "utf8") > MAX_SOURCE_BYTES) return false;
  const code = stripComments(source);
  const observers = [...code.matchAll(/\b(?:print|write|assert|panic)\s*\(([^)\n]*)\)/g)];
  return observers.some((match) => {
    const argument = match[1].trim();
    if (!argument || /^(?:true|false|null|[-+]?\d+(?:\.\d+)?|"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')$/.test(argument)) return false;
    return /[A-Za-z_][A-Za-z0-9_.]*|\(|\[|[+\-*\/%<>=!&|]/.test(argument);
  });
}

function sourceRecord(value, index, label = "source_programs") {
  if (!value || typeof value !== "object") fail(`${label}[${index}] must be an object`, "E_LANE");
  const source = requiredString(value.source, `${label}[${index}].source`);
  if (value.value_consuming !== true || !isValueConsumingSource(source)) {
    fail(`${label}[${index}] is not a value-consuming program`, "E_LANE");
  }
  const observer = requiredString(value.observer || "value observer", `${label}[${index}].observer`);
  if (observer !== "value observer" && !stripComments(source).includes(observer)) {
    fail(`${label}[${index}] observer is not present in source`, "E_LANE");
  }
  const sourceSha = digest(source);
  if (value.source_sha256 !== sourceSha) fail(`${label}[${index}] source digest does not match`, "E_LANE");
  return {
    id: requiredString(value.id || `${label.slice(0, -1)}-${index + 1}`, `${label}[${index}].id`),
    source,
    source_sha256: sourceSha,
    value_consuming: true,
    observer,
  };
}

function listRecord(value, index, label) {
  if (typeof value === "string") return { id: requiredString(value, `${label}[${index}].id`), value };
  if (!value || typeof value !== "object") fail(`${label}[${index}] must be an object`, "E_LANE");
  const id = requiredString(value.id || `${label.slice(0, -1)}-${index + 1}`, `${label}[${index}].id`);
  return { ...clone(value), id };
}

function requireRecordList(report, key) {
  if (!Array.isArray(report[key])) fail(`lane receipt ${key} must be an array`, "E_LANE");
  return report[key];
}

function ensureUniqueRecordIds(records, label) {
  const ids = new Set();
  for (const record of records) {
    if (ids.has(record.id)) fail(`${label} repeats id ${record.id}`, "E_LANE");
    ids.add(record.id);
  }
  return ids;
}

function validateCleanupRecord(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} cleanup is missing`, "E_CLEANUP");
  for (const key of ["scratch_paths", "alternate_targets"]) {
    if (value[key] !== undefined && !Array.isArray(value[key])) fail(`${label} cleanup ${key} must be an array`, "E_CLEANUP");
  }
  for (const key of ["unbounded_logs", "complete"]) {
    if (value[key] !== undefined && typeof value[key] !== "boolean") fail(`${label} cleanup ${key} must be boolean`, "E_CLEANUP");
  }
  const cleanup = cleanupSummary(value);
  for (const key of ["scratch_paths", "alternate_targets"]) {
    if (cleanup[key].some((item) => typeof item !== "string" || !item.trim())) {
      fail(`${label} cleanup ${key} contains an invalid path`, "E_CLEANUP");
    }
  }
  return cleanup;
}

function findingSeverity(finding) {
  const bundle = finding.bundle || {};
  const classification = String(finding.classification || bundle.classification || "").toLowerCase().replace(/_/g, "-");
  if (finding.p0 === true || isDefaultJetRunDivergence(finding) || finding.silent_wrong_data === true
    || ["silent", "silent-data", "wrong-data", "silent-wrong-data", "default-jet-run-divergence"].includes(classification)) return "P0";
  if (finding.severity && /^P[0-3]$/.test(finding.severity)) return finding.severity;
  if (finding.priority && /^P[0-3]$/.test(finding.priority)) return finding.priority;
  return "P1";
}

function isDefaultJetRunDivergence(finding) {
  const bundle = finding?.bundle || finding || {};
  if (finding?.default_jet_run_divergence === true || finding?.default_jet_run_divergence === "true"
    || bundle.default_jet_run_divergence === true || bundle.default_jet_run_divergence === "true"
    || bundle.defaultJetRunDivergence === true || bundle.defaultJetRunDivergence === "true") return true;
  return (bundle.tier || finding?.tier) === "jet_run"
    && bundle.actual_relation !== undefined
    && bundle.actual_relation !== bundle.expected_relation
    && bundle.exit === 0
    && !bundle.signal
    && bundle.timeout !== true;
}


function validateFinding(finding, index, manifest) {
  if (!finding || typeof finding !== "object") fail(`unique_findings[${index}] must be an object`, "E_LANE");
  const findingId = requiredString(finding.finding_id || finding.id, `unique_findings[${index}].finding_id`);
  if (!FINDING_ID_PATTERN.test(findingId)) fail(`unique_findings[${index}] finding_id is invalid`, "E_LANE");
  if (!finding.bundle || typeof finding.bundle !== "object") fail(`finding ${findingId} has no hardening bundle`, "E_LANE");
  try {
    validateResultBundle(finding.bundle);
  } catch (error) {
    fail(`finding ${findingId} has an invalid hardening bundle: ${error.message}`, "E_LANE");
  }
  const identity = bundleIdentity(finding.bundle);
  if (finding.bundle_identity !== identity) fail(`finding ${findingId} bundle identity does not match`, "E_LANE");
  if (finding.bundle.commit !== manifest.target.commit || finding.bundle.binary_sha256 !== manifest.target.binary_sha256) {
    fail(`finding ${findingId} targets a different frozen binary`, "E_LANE");
  }
  if (finding.bundle.registry_snapshot_hash !== manifest.registry_snapshot.sha256) {
    fail(`finding ${findingId} targets a different registry snapshot`, "E_LANE");
  }
  const severity = findingSeverity(finding);
  const reproducerId = requiredString(finding.reproducer_id, `finding ${findingId}.reproducer_id`);
  return {
    ...clone(finding),
    finding_id: findingId,
    severity,
    bundle_identity: identity,
    load_bearing: finding.load_bearing !== false,
    reproducer_id: reproducerId,
  };
}

export function validateLaneReceipt(report, manifest) {
  validateSessionManifest(manifest);
  if (!report || typeof report !== "object" || report.schema !== RED_TEAM_LANE_SCHEMA || report.schema_version !== RED_TEAM_SCHEMA_VERSION) {
    fail("invalid red-team lane receipt schema", "E_LANE");
  }
  if (report.session_id !== manifest.session_id) fail("lane receipt belongs to another session", "E_LANE");
  if (!LANE_ID_PATTERN.test(report.lane_id)) fail("lane receipt lane_id is invalid", "E_LANE");
  const expectedBrief = manifest.lane_briefs.find((brief) => brief.lane_id === report.lane_id);
  if (!expectedBrief || report.wave !== expectedBrief.wave) fail(`lane ${report.lane_id} has the wrong wave`, "E_LANE");
  requiredString(report.context_id, "lane receipt context_id");
  if (!CONTEXT_ID_PATTERN.test(report.context_id)) fail("lane receipt context_id is invalid", "E_LANE");
  requiredString(report.agent_id, "lane receipt agent_id");
  if (report.fresh_context !== true || String(report.model || "").toLowerCase() !== RED_TEAM_MODEL
    || report.reasoning_effort !== RED_TEAM_REASONING) {
    fail(`lane ${report.lane_id} is not a fresh Luna-max context`, "E_LANE");
  }
  if (report.known_defects_visible !== false || Object.hasOwn(report, "known_defects") || Object.hasOwn(report, "tower_cards")) {
    fail(`lane ${report.lane_id} was given current defect cards`, "E_LANE");
  }
  for (const forbidden of ["defect_cards", "known_defects", "known_findings", "tower_cards", "dedup_results"]) {
    if (Object.hasOwn(report, forbidden)) fail(`lane ${report.lane_id} was given current ${forbidden}`, "E_LANE");
  }
  if (report.complete !== true || report.stopped_early === true) fail(`lane ${report.lane_id} stopped before completing its quota`, "E_LANE");
  const packet = makeContextPacket(manifest, report.lane_id);
  if (report.packet_digest !== packet.context_digest) fail(`lane ${report.lane_id} did not use its independent context packet`, "E_LANE");
  if (!report.target || report.target.commit !== manifest.target.commit || report.target.binary_sha256 !== manifest.target.binary_sha256
    || report.target.root !== manifest.target.root
    || report.target.binary_path !== manifest.target.binary_path
    || report.target.platform !== manifest.target.platform
    || report.target.arch !== manifest.target.arch
    || (report.registry_snapshot?.sha256 !== undefined && report.registry_snapshot.sha256 !== manifest.registry_snapshot.sha256)
    || (report.public_surface_snapshot?.sha256 !== undefined && report.public_surface_snapshot.sha256 !== manifest.public_surface_snapshot.sha256)) {
    fail(`lane ${report.lane_id} target does not match the frozen binary`, "E_LANE");
  }
  const cleanup = validateCleanupRecord(report.cleanup, `lane ${report.lane_id}`);
  const sourcePrograms = requireRecordList(report, "source_programs").map((value, index) => sourceRecord(value, index));
  const attempts = requireRecordList(report, "attempts").map((value, index) => listRecord(value, index, "attempts"));
  const validCases = requireRecordList(report, "valid_cases").map((value, index) => listRecord(value, index, "valid_cases"));
  const duplicates = requireRecordList(report, "duplicates").map((value, index) => listRecord(value, index, "duplicates"));
  const falsePositives = requireRecordList(report, "false_positives").map((value, index) => listRecord(value, index, "false_positives"));
  const reproducers = requireRecordList(report, "minimized_reproducers").map((value, index) => sourceRecord(value, index, "minimized_reproducers"));
  const uniqueFindingInputs = requireRecordList(report, "unique_findings");
  if (!sourcePrograms.length || !attempts.length || !validCases.length) fail(`lane ${report.lane_id} has no value-consuming quota evidence`, "E_LANE");
  const sourceIds = new Set(sourcePrograms.map((item) => item.id));
  ensureUniqueRecordIds(sourcePrograms, `lane ${report.lane_id} source programs`);
  ensureUniqueRecordIds(attempts, `lane ${report.lane_id} attempts`);
  ensureUniqueRecordIds(validCases, `lane ${report.lane_id} valid cases`);
  ensureUniqueRecordIds(duplicates, `lane ${report.lane_id} duplicates`);
  ensureUniqueRecordIds(falsePositives, `lane ${report.lane_id} false positives`);
  ensureUniqueRecordIds(reproducers, `lane ${report.lane_id} reproducers`);
  for (const attempt of attempts) {
    if (!attempt.program_id || !sourceIds.has(attempt.program_id)) fail(`lane ${report.lane_id} has an attempt for an unknown source`, "E_LANE");
  }
  const attemptIds = new Set(attempts.map((item) => item.id));
  for (const validCase of validCases) {
    const attemptId = validCase.attempt_id;
    if (!attemptIds.has(attemptId)) fail(`lane ${report.lane_id} has a valid case without an attempt`, "E_LANE");
  }
  const reproducerIds = new Set(reproducers.map((item) => item.id));
  const uniqueFindings = uniqueFindingInputs.map((value, index) => validateFinding(value, index, manifest));
  const findingIds = new Set();
  const bundleIds = new Set();
  for (const finding of uniqueFindings) {
    if (findingIds.has(finding.finding_id) || bundleIds.has(finding.bundle_identity)) fail(`lane ${report.lane_id} repeats a unique finding`, "E_LANE");
    findingIds.add(finding.finding_id);
    bundleIds.add(finding.bundle_identity);
    if (!reproducerIds.has(finding.reproducer_id)) fail(`finding ${finding.finding_id} has no minimized reproducer`, "E_LANE");
  }
  const counts = report.counts;
  if (!counts || typeof counts !== "object") fail(`lane ${report.lane_id} is missing counts`, "E_LANE");
  const expectedCounts = {
    source_programs: sourcePrograms.length,
    attempts: attempts.length,
    valid_cases: validCases.length,
    duplicates: duplicates.length,
    false_positives: falsePositives.length,
    minimized_reproducers: reproducers.length,
    unique_findings: uniqueFindings.length,
  };
  for (const [key, expected] of Object.entries(expectedCounts)) {
    if (counts[key] !== expected) fail(`lane ${report.lane_id} count ${key} is not an evidence count`, "E_LANE");
  }
  return {
    ...clone(report),
    source_programs: sourcePrograms,
    attempts,
    valid_cases: validCases,
    duplicates,
    false_positives: falsePositives,
    minimized_reproducers: reproducers,
    unique_findings: uniqueFindings,
    cleanup,
    counts: expectedCounts,
    semantic_change: report.semantic_change === true,
    registry_changed: report.registry_changed === true,
  };
}

export function makeLaneReceipt(manifest, input = {}) {
  validateSessionManifest(manifest);
  const laneId = input.lane_id || input.laneId;
  if (!LANE_ID_PATTERN.test(laneId || "")) fail("lane receipt lane_id is required", "E_LANE");
  const packet = makeContextPacket(manifest, laneId);
  const source = input.source || `fn run() { value :: 1\n    print(value)\n}\n`;
  const sourcePrograms = input.source_programs || [{
    id: "program-1",
    source,
    source_sha256: digest(source),
    value_consuming: true,
    observer: "print(value)",
  }];
  const attempts = input.attempts || [{ id: "attempt-1", program_id: sourcePrograms[0].id, valid: true }];
  const validCases = input.valid_cases || [{ id: "case-1", attempt_id: attempts[0].id }];
  const duplicates = input.duplicates || [];
  const falsePositives = input.false_positives || [];
  const reproducers = input.minimized_reproducers || [];
  const uniqueFindings = input.unique_findings || [];
  const report = {
    schema: RED_TEAM_LANE_SCHEMA,
    schema_version: RED_TEAM_SCHEMA_VERSION,
    session_id: manifest.session_id,
    packet_digest: packet.context_digest,
    lane_id: laneId,
    wave: manifest.lane_briefs.find((brief) => brief.lane_id === laneId).wave,
    context_id: input.context_id || `${manifest.session_id}/${laneId}/fresh`,
    agent_id: input.agent_id || `${laneId}-agent`,
    model: input.model || RED_TEAM_MODEL,
    reasoning_effort: input.reasoning_effort || RED_TEAM_REASONING,
    fresh_context: true,
    known_defects_visible: false,
    target: clone(manifest.target),
    started_at: input.started_at || manifest.created_at,
    finished_at: input.finished_at || manifest.created_at,
    complete: input.complete !== false,
    stopped_early: input.stopped_early === true,
    semantic_change: input.semantic_change === true,
    registry_changed: input.registry_changed === true,
    cleanup: clone(input.cleanup || {
      active_agents: 0,
      active_processes: 0,
      scratch_paths: [],
      alternate_targets: [],
      unbounded_logs: false,
      complete: true,
    }),
    source_programs: clone(sourcePrograms),
    attempts: clone(attempts),
    valid_cases: clone(validCases),
    duplicates: duplicates.map((item, index) => listRecord(item, index, "duplicates")),
    false_positives: falsePositives.map((item, index) => listRecord(item, index, "false_positives")),
    minimized_reproducers: reproducers.map((item, index) => ({
      ...clone(item),
      id: item.id || `reproducer-${index + 1}`,
      source_sha256: item.source_sha256 || digest(item.source),
      value_consuming: item.value_consuming ?? true,
      observer: item.observer || "value observer",
    })),
    unique_findings: clone(uniqueFindings),
  };
  report.counts = {
    source_programs: report.source_programs.length,
    attempts: report.attempts.length,
    valid_cases: report.valid_cases.length,
    duplicates: report.duplicates.length,
    false_positives: report.false_positives.length,
    minimized_reproducers: report.minimized_reproducers.length,
    unique_findings: report.unique_findings.length,
  };
  validateLaneReceipt(report, manifest);
  return report;
}

function currentTarget(root, binaryPath, registryConfigured, publicSurfaceConfigured = registryConfigured) {
  const identity = gitSnapshot(root);
  const targetPath = resolve(binaryPath || DEFAULT_BINARY(root));
  const registry = registrySnapshot(root, registryConfigured);
  const publicSurface = registrySnapshot(root, publicSurfaceConfigured);
  return {
    commit: identity.commit,
    binary_sha256: binaryHash(targetPath),
    binary_path: safeRelative(root, targetPath),
    registry_snapshot: registry,
    public_surface_snapshot: publicSurface,
    platform: process.platform,
    arch: process.arch,
  };
}

export function targetDrift(manifest, snapshot) {
  const reasons = [];
  if (!snapshot || snapshot.commit !== manifest.target.commit) reasons.push("target commit changed");
  if (!snapshot || snapshot.binary_sha256 !== manifest.target.binary_sha256) reasons.push("target binary changed");
  if (!snapshot || snapshot.registry_snapshot?.sha256 !== manifest.registry_snapshot.sha256) reasons.push("registry snapshot changed");
  if (snapshot?.public_surface_snapshot && snapshot.public_surface_snapshot.sha256 !== manifest.public_surface_snapshot.sha256) {
    reasons.push("public surface snapshot changed");
  }
  if (snapshot?.binary_path !== undefined && snapshot.binary_path !== manifest.target.binary_path) reasons.push("target binary path changed");
  if (!snapshot || snapshot.platform !== manifest.target.platform || snapshot.arch !== manifest.target.arch) reasons.push("target platform changed");
  return reasons;
}

function laneFindings(lanes) {
  const byBundle = new Map();
  const duplicates = [];
  const severityRank = { P0: 0, P1: 1, P2: 2, P3: 3 };
  for (const lane of lanes) {
    for (const finding of lane.unique_findings) {
      if (byBundle.has(finding.bundle_identity)) {
        duplicates.push({ bundle_identity: finding.bundle_identity, lane_id: lane.lane_id, finding_id: finding.finding_id });
        const existing = byBundle.get(finding.bundle_identity);
        if ((severityRank[finding.severity] ?? 99) < (severityRank[existing.severity] ?? 99)) existing.severity = finding.severity;
        existing.load_bearing ||= finding.load_bearing;
        existing.silent_wrong_data ||= finding.silent_wrong_data === true;
        existing.default_jet_run_divergence ||= finding.default_jet_run_divergence === true;
        continue;
      }
      byBundle.set(finding.bundle_identity, { ...finding, discovered_by: lane.lane_id });
    }
  }
  return { findings: [...byBundle.values()], duplicates };
}

function p0Finding(finding) {
  return finding.severity === "P0"
    || isDefaultJetRunDivergence(finding)
    || finding.silent_wrong_data === true
    || finding.silent_wrong_data === "true";
}

function bytesFrom(value) {
  if (typeof value === "string" && value.startsWith("base64:")) return Buffer.from(value.slice(7), "base64");
  return Buffer.from(String(value ?? ""), "utf8");
}

async function defaultReplayFinding(finding, manifest, options) {
  const bundle = finding.bundle;
  const root = resolve(options.root || manifest.target.root || DEFAULT_ROOT);
  const targetPath = resolve(root, manifest.target.binary_path);
  const snapshot = currentTarget(root, targetPath, manifest.registry_snapshot.path, manifest.public_surface_snapshot.path);
  const scratchRoot = resolve(process.env.JET_HARDENING_SCRATCH
    || process.env.JET_TEST_SCRATCH
    || join(os.homedir(), ".cache/jet-test-scratch/hardening-rig-red-team"));
  mkdirSync(scratchRoot, { recursive: true, mode: 0o700 });
  const scratch = mkdtempSync(join(scratchRoot, "replay-"));
  const sourcePath = join(scratch, "case.jet");
  try {
    writeFileSync(sourcePath, bundle.source, { mode: 0o600 });
    const tierFlags = {
      aot: ["--release"],
      jet_run: [],
      interpreter: ["--interpret"],
    }[bundle.tier];
    const result = await executeCommand({
      program: targetPath,
      args: ["run", ...tierFlags, sourcePath],
      root,
      cwd: root,
      env: { NO_COLOR: "1", JETPACK_ENV: "1", ...(options.env || {}) },
      stdin: finding.stdin || "",
      timeout_ms: manifest.resource_limits.lane_timeout_ms,
      capture_limit: manifest.resource_limits.capture_bytes,
      label: `replay:${finding.finding_id}:${bundle.tier}`,
    });
    const expectedStdout = bytesFrom(bundle.stdout_bytes);
    const expectedStderr = bytesFrom(bundle.stderr_bytes);
    const confirmed = result.stdout.equals(expectedStdout)
      && result.stderr.equals(expectedStderr)
      && result.exit === bundle.exit
      && (result.signal || null) === (bundle.signal || null)
      && result.timeout === bundle.timeout;
    return {
      confirmed,
      target: snapshot,
      tier: bundle.tier,
      binary_path: manifest.target.binary_path,
      stdout_sha256: result.stdout_sha256,
      stderr_sha256: result.stderr_sha256,
      exit: result.exit,
      signal: result.signal,
      timeout: result.timeout,
      bundle_identity: finding.bundle_identity,
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function normalizeReplay(replay, finding, manifest) {
  if (!replay || replay.confirmed !== true) fail(`finding ${finding.finding_id} did not replay on the frozen binary`, "E_REPLAY");
  const target = replay.target || {};
  if (target.commit !== manifest.target.commit || target.binary_sha256 !== manifest.target.binary_sha256) {
    fail(`finding ${finding.finding_id} replay used a different target`, "E_REPLAY");
  }
  if (target.binary_path !== undefined && target.binary_path !== manifest.target.binary_path) {
    fail(`finding ${finding.finding_id} replay used a different binary path`, "E_REPLAY");
  }
  if (target.registry_snapshot?.sha256 !== undefined
    && target.registry_snapshot.sha256 !== manifest.registry_snapshot.sha256) {
    fail(`finding ${finding.finding_id} replay used a different registry snapshot`, "E_REPLAY");
  }
  if (target.public_surface_snapshot?.sha256 !== undefined
    && target.public_surface_snapshot.sha256 !== manifest.public_surface_snapshot.sha256) {
    fail(`finding ${finding.finding_id} replay used a different public surface snapshot`, "E_REPLAY");
  }
  if (replay.tier !== undefined && replay.tier !== finding.bundle.tier) {
    fail(`finding ${finding.finding_id} replay used a different tier`, "E_REPLAY");
  }
  return {
    ...clone(replay),
    confirmed: true,
    bundle_identity: finding.bundle_identity,
    target_commit: manifest.target.commit,
    target_binary_sha256: manifest.target.binary_sha256,
  };
}

function normalizeSeam(value) {
  const aliases = {
    prelude: "prelude-semantic-function",
    "semantic-function": "prelude-semantic-function",
    "semantic-equality": "interpreter-equality",
    "indexed-place": "tir-place-lowering",
    "packed-int": "packed-int-representation",
    "aot-emit": "aot-emission",
    "release-emission": "aot-emission",
    input: "input-transport",
    "stdin-transport": "input-transport",
    unclassified: "unclassified.semantic-primitive",
  };
  const seam = String(value || "unclassified.semantic-primitive").trim().toLowerCase().replace(/\s+/g, "-").replace(/_/g, "-");
  const normalized = aliases[seam] || seam;
  return [
    "prelude-semantic-function",
    "interpreter-equality",
    "tir-place-lowering",
    "packed-int-representation",
    "aot-emission",
    "input-transport",
    "unclassified.semantic-primitive",
  ].includes(normalized) ? normalized : "unclassified.semantic-primitive";
}

function token(value, fallback) {
  const result = String(value ?? "").trim().toLowerCase().replace(/\s+/g, "-");
  return result || fallback;
}

function wrongTierMaskFor(finding) {
  const bundle = finding.bundle || finding;
  if (finding.wrong_tier_mask !== undefined) return finding.wrong_tier_mask;
  const observed = bundle.tier_observations
    ?.filter((item) => item.relation !== bundle.actual_relation)
    .map((item) => item.tier);
  return observed?.length ? observed : [bundle.tier];
}

function normalizedTierMask(value) {
  const values = Array.isArray(value) ? value : String(value ?? "").split(/[,+]/);
  return values.map((item) => token(item, "unknown")).filter(Boolean);
}

export function hardeningDedupKey(finding) {
  const bundle = finding.bundle || finding;
  const wrongTiers = wrongTierMaskFor(finding);
  return [
    "hardening:v1",
    `seam=${encodeURIComponent(normalizeSeam(finding.hardening_seam || finding.semantic_primitive || finding.root_seam))}`,
    `relation=${encodeURIComponent(token(finding.violated_relation || bundle.expected_relation, "unknown-relation"))}`,
    `tiers=${encodeURIComponent([...new Set(normalizedTierMask(wrongTiers))].sort().join(","))}`,
    `partition=${encodeURIComponent(token(finding.input_partition || finding.mutation_arm || bundle.mutation_arm, "unknown-partition"))}`,
  ].join("|");
}

function towerStable(value) {
  if (Array.isArray(value)) return `[${value.map(towerStable).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${towerStable(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function evidenceSource(bundle) {
  return String(bundle.source).trim();
}

function towerEvidenceDigest(finding, key, manifest) {
  const bundle = finding.bundle;
  const core = {
    schema_version: 1,
    repro_schema: "jet.hardening.repro.v1",
    finding_id: finding.finding_id,
    stable_key: key,
    source: evidenceSource(bundle),
    commands: bundle.tier_observations.map((observation) => `${observation.tier}: ${bundle.tier_command}`),
    expected_relation: bundle.expected_relation,
    actual_relation: bundle.actual_relation,
    seed: bundle.seed,
    target_commit: manifest.target.commit,
    classification: bundle.classification,
    stdout_bytes: bundle.stdout_bytes,
    stderr_bytes: bundle.stderr_bytes,
    exit: bundle.exit,
    signal: bundle.signal || null,
    timeout: bundle.timeout === true,
    normalization: bundle.normalization || [],
  };
  return digest(towerStable(core));
}
export function findingTowerPayload(finding, manifest, replay = undefined) {
  const bundle = finding.bundle;
  const source = evidenceSource(bundle);
  const key = hardeningDedupKey(finding);
  const wrongTierMask = wrongTierMaskFor(finding);
  const inputPartition = finding.input_partition || finding.mutation_arm || bundle.mutation_arm;
  const commands = bundle.tier_observations.map((observation) => `${observation.tier}: ${bundle.tier_command}`);
  const evidenceDigest = towerEvidenceDigest(finding, key, manifest);
  const severity = findingSeverity(finding);
  return {
    title: `Red-team hardening finding: ${finding.finding_id}`,
    body: [
      "Confirmed by the bounded fresh-context red-team protocol.",
      `Session: ${manifest.session_id}`,
      `Bundle identity: ${finding.bundle_identity}`,
      "Assimilation route: #2338 root-seam dedup.",
    ].join("\n"),
    kind: "task",
    track: "sidequest",
    phase: "ready",
    priority: severity,
    tags: ["hardening-rig", "red-team", "luna-max"],
    hardeningDedupKey: key,
    hardeningWrongTierMask: wrongTierMask,
    hardeningInputPartition: inputPartition,
    hardeningSeam: normalizeSeam(finding.hardening_seam || finding.semantic_primitive || finding.root_seam),
    hardeningRelation: bundle.expected_relation,
    hardeningFindingId: finding.finding_id,
    hardeningEvidence: {
      source,
      commands,
      expectedRelation: bundle.expected_relation,
      actualRelation: bundle.actual_relation,
      seed: bundle.seed,
      targetCommit: manifest.target.commit,
      bundleDigest: evidenceDigest,
      classification: bundle.classification,
      stdoutBytes: bundle.stdout_bytes,
      stderrBytes: bundle.stderr_bytes,
      exit: bundle.exit,
      signal: bundle.signal,
      timeout: bundle.timeout,
      normalization: bundle.normalization,
      replay,
    },
    bundleIdentity: finding.bundle_identity,
    sessionId: manifest.session_id,
    targetCommit: manifest.target.commit,
  };
}

export async function assimilateFindings(findings, manifest, {
  root = DEFAULT_ROOT,
  tower_cli = process.env.JET_HARDENING_TOWER_CLI,
  tower_data = process.env.JET_HARDENING_TOWER_DATA,
  dry_run = process.env.JET_HARDENING_DRY_RUN === "1",
  command = executeCommand,
} = {}) {
  if (!Array.isArray(findings)) fail("findings must be an array", "E_ASSIMILATION");
  validateSessionManifest(manifest);
  const actions = [];
  for (const finding of findings) {
    const payload = findingTowerPayload(finding, manifest, finding.replay);
    if (dry_run) {
      actions.push({ status: "SKIPPED", reason: "dry run", route: "#2338", bundle_identity: finding.bundle_identity });
      continue;
    }
    const cli = resolve(tower_cli || join(root, "plugins/tower/tower.mjs"));
    if (!existsSync(cli)) fail(`Tower CLI is missing: ${cli}`, "E_ASSIMILATION");
    const args = tower_data ? [cli, "--data", resolve(root, tower_data)] : [cli];
    args.push("card", "add", "--stdin", "--json", "--by", "hardening-rig");
    const result = await command({
      program: process.execPath,
      args,
      cwd: root,
      stdin: JSON.stringify(payload),
      timeout_ms: manifest.resource_limits.lane_timeout_ms,
      capture_limit: manifest.resource_limits.capture_bytes,
      label: `red-team:tower:${finding.finding_id}`,
    });
    if (!result.ok || result.stdout_truncated) fail(`Tower assimilation failed for ${finding.finding_id}`, "E_ASSIMILATION");
    let response;
    try {
      response = JSON.parse(result.stdout.toString("utf8"));
    } catch (error) {
      fail(`Tower assimilation response is not JSON: ${error.message}`, "E_ASSIMILATION");
    }
    if (!response || response.error) fail(`Tower assimilation rejected ${finding.finding_id}`, "E_ASSIMILATION");
    actions.push({
      status: "WRITTEN",
      route: "#2338",
      finding_id: finding.finding_id,
      bundle_identity: finding.bundle_identity,
      card_id: response.id || null,
      card_num: response.num || null,
      action: response.action || null,
      command: [process.execPath, ...args].join(" "),
    });
  }
  return actions;
}

function validateAssimilationActions(actions, findings) {
  if (!Array.isArray(actions) || actions.length !== findings.length) {
    fail("confirmed red-team findings were not all routed through #2338", "E_ASSIMILATION");
  }
  const expected = new Set(findings.map((finding) => finding.finding_id));
  const seen = new Set();
  for (const action of actions) {
    if (!action || action.route !== "#2338" || action.status === "SKIPPED") {
      fail("red-team finding assimilation did not complete through #2338", "E_ASSIMILATION");
    }
    if (!expected.has(action.finding_id) || seen.has(action.finding_id)) {
      fail("red-team finding assimilation returned an unexpected finding", "E_ASSIMILATION");
    }
    seen.add(action.finding_id);
  }
}

function stopRequested(signal) {
  return signal?.aborted === true;
}

function cleanupSummary(value = {}) {
  const count = (key) => value[key] === undefined ? 0 : value[key];
  const summary = {
    active_agents: count("active_agents"),
    active_processes: count("active_processes"),
    scratch_paths: Array.isArray(value.scratch_paths) ? [...value.scratch_paths] : [],
    alternate_targets: Array.isArray(value.alternate_targets) ? [...value.alternate_targets] : [],
    unbounded_logs: value.unbounded_logs === true,
    complete: value.complete !== false,
  };
  if (!Number.isInteger(summary.active_agents) || summary.active_agents < 0
    || !Number.isInteger(summary.active_processes) || summary.active_processes < 0) {
    fail("cleanup receipt has invalid active counts", "E_CLEANUP");
  }
  return summary;
}

function cleanupProblems(cleanup) {
  const problems = [];
  if (cleanup.active_agents !== 0) problems.push("active agents remain");
  if (cleanup.active_processes !== 0) problems.push("active processes remain");
  if (cleanup.scratch_paths.length) problems.push("scratch remains");
  if (cleanup.alternate_targets.length) problems.push("alternate target remains");
  if (cleanup.unbounded_logs) problems.push("unbounded log remains");
  if (!cleanup.complete) problems.push("cleanup incomplete");
  return problems;
}
function laneCleanupSummary(lanes) {
  return lanes.reduce((summary, lane) => {
    const value = lane.cleanup;
    if (!value || typeof value !== "object") return summary;
    summary.active_agents += Number(value.active_agents || 0);
    summary.active_processes += Number(value.active_processes || 0);
    if (Array.isArray(value.scratch_paths)) summary.scratch_paths.push(...value.scratch_paths);
    if (Array.isArray(value.alternate_targets)) summary.alternate_targets.push(...value.alternate_targets);
    summary.unbounded_logs ||= value.unbounded_logs === true;
    summary.complete &&= value.complete !== false;
    return summary;
  }, {
    active_agents: 0,
    active_processes: 0,
    scratch_paths: [],
    alternate_targets: [],
    unbounded_logs: false,
    complete: true,
  });
}

function validateSignedLaneShape(lane, receipt, manifest = undefined) {
  if (manifest) {
    validateLaneReceipt(lane, manifest);
    return;
  }
  if (!lane || lane.schema !== RED_TEAM_LANE_SCHEMA || lane.schema_version !== RED_TEAM_SCHEMA_VERSION) {
    fail("red-team verdict contains an invalid lane receipt", "E_VERDICT");
  }
  if (lane.session_id !== receipt.session_id || !LANE_ID_PATTERN.test(lane.lane_id || "")) {
    fail("red-team verdict lane identity is invalid", "E_VERDICT");
  }
  const brief = LANE_BRIEFS.find((item) => item.lane_id === lane.lane_id);
  if (!brief || lane.wave !== brief.wave) fail("red-team verdict lane wave is invalid", "E_VERDICT");
  requiredString(lane.context_id, "red-team verdict lane context_id");
  requiredString(lane.agent_id, "red-team verdict lane agent_id");
  if (lane.fresh_context !== true || String(lane.model || "").toLowerCase() !== RED_TEAM_MODEL
    || lane.reasoning_effort !== RED_TEAM_REASONING || lane.known_defects_visible !== false
    || lane.complete !== true || lane.stopped_early === true) {
    fail(`red-team verdict lane ${lane.lane_id} is not a complete hidden-card Luna-max receipt`, "E_VERDICT");
  }
  if (!validDigest(lane.packet_digest)) fail(`red-team verdict lane ${lane.lane_id} packet digest is invalid`, "E_VERDICT");
  const session = receipt.session;
  if (!lane.target || lane.target.commit !== session.commit || lane.target.binary_sha256 !== session.binary_sha256) {
    fail(`red-team verdict lane ${lane.lane_id} targets a different binary`, "E_VERDICT");
  }
  for (const key of ["source_programs", "attempts", "valid_cases", "duplicates", "false_positives", "minimized_reproducers", "unique_findings"]) {
    if (!Array.isArray(lane[key])) fail(`red-team verdict lane ${lane.lane_id} is missing ${key}`, "E_VERDICT");
  }
  if (!lane.source_programs.length || !lane.attempts.length || !lane.valid_cases.length) {
    fail(`red-team verdict lane ${lane.lane_id} has no value-consuming quota evidence`, "E_VERDICT");
  }
  const sourcePrograms = lane.source_programs.map((item, index) => sourceRecord(item, index));
  const sourceIds = new Set(sourcePrograms.map((item) => item.id));
  ensureUniqueRecordIds(sourcePrograms, `red-team verdict lane ${lane.lane_id} source programs`);
  const attempts = lane.attempts.map((item, index) => listRecord(item, index, "attempts"));
  const attemptIds = new Set(attempts.map((item) => item.id));
  ensureUniqueRecordIds(attempts, `red-team verdict lane ${lane.lane_id} attempts`);
  for (const attempt of attempts) {
    if (!sourceIds.has(attempt.program_id)) fail(`red-team verdict lane ${lane.lane_id} has an attempt for unknown source`, "E_VERDICT");
  }
  const validCases = lane.valid_cases.map((item, index) => listRecord(item, index, "valid_cases"));
  ensureUniqueRecordIds(validCases, `red-team verdict lane ${lane.lane_id} valid cases`);
  for (const validCase of validCases) {
    if (!attemptIds.has(validCase.attempt_id)) fail(`red-team verdict lane ${lane.lane_id} has a valid case without an attempt`, "E_VERDICT");
  }
  const duplicates = lane.duplicates.map((item, index) => listRecord(item, index, "duplicates"));
  const falsePositives = lane.false_positives.map((item, index) => listRecord(item, index, "false_positives"));
  const reproducers = lane.minimized_reproducers.map((item, index) => sourceRecord(item, index, "minimized_reproducers"));
  ensureUniqueRecordIds(duplicates, `red-team verdict lane ${lane.lane_id} duplicates`);
  ensureUniqueRecordIds(falsePositives, `red-team verdict lane ${lane.lane_id} false positives`);
  ensureUniqueRecordIds(reproducers, `red-team verdict lane ${lane.lane_id} reproducers`);
  const pseudoManifest = {
    target: { commit: session.commit, binary_sha256: session.binary_sha256 },
    registry_snapshot: { sha256: session.registry_sha256 },
  };
  const findings = lane.unique_findings.map((item, index) => validateFinding(item, index, pseudoManifest));
  const findingIds = new Set();
  const bundleIds = new Set();
  for (const finding of findings) {
    if (findingIds.has(finding.finding_id) || bundleIds.has(finding.bundle_identity)) {
      fail(`red-team verdict lane ${lane.lane_id} repeats a unique finding`, "E_VERDICT");
    }
    findingIds.add(finding.finding_id);
    bundleIds.add(finding.bundle_identity);
    if (!reproducers.some((item) => item.id === finding.reproducer_id)) {
      fail(`red-team verdict finding ${finding.finding_id} has no minimized reproducer`, "E_VERDICT");
    }
  }
  const counts = lane.counts;
  if (!counts || typeof counts !== "object" || Array.isArray(counts)) fail(`red-team verdict lane ${lane.lane_id} counts are missing`, "E_VERDICT");
  const expectedCounts = {
    source_programs: sourcePrograms.length,
    attempts: attempts.length,
    valid_cases: validCases.length,
    duplicates: duplicates.length,
    false_positives: falsePositives.length,
    minimized_reproducers: reproducers.length,
    unique_findings: findings.length,
  };
  for (const [key, expected] of Object.entries(expectedCounts)) {
    if (counts[key] !== expected) fail(`red-team verdict lane ${lane.lane_id} count ${key} is inconsistent`, "E_VERDICT");
  }
}

function validateSignedVerdictShape(receipt, manifest = undefined) {
  if (receipt.receipt_kind !== "fresh-context-red-team-verdict") return;
  if (!["PASS", "FAILED", "STALE"].includes(receipt.status)) fail("red-team verdict status is invalid", "E_VERDICT");
  const strict = manifest !== undefined || Object.hasOwn(receipt, "manifest_sha256");
  if (!strict) {
    if (Array.isArray(receipt.findings)) {
      const p0Count = receipt.findings.filter(p0Finding).length;
      if (receipt.p0_count !== undefined && receipt.p0_count !== p0Count) fail("red-team verdict P0 count is inconsistent", "E_VERDICT");
      if (receipt.status === "PASS" && p0Count !== 0) fail("red-team verdict cannot pass with a new P0", "E_VERDICT");
      if (receipt.unique_finding_count !== undefined && receipt.unique_finding_count !== receipt.findings.length) {
        fail("red-team verdict finding count is inconsistent", "E_VERDICT");
      }
    }
    return;
  }
  if (!validDigest(receipt.manifest_sha256) || !receipt.session_id || !receipt.session) {
    fail("red-team verdict is missing frozen session identity", "E_VERDICT");
  }
  if (manifest) {
    validateSessionManifest(manifest);
    if (receipt.manifest_sha256 !== manifest.manifest_sha256 || receipt.session_id !== manifest.session_id) {
      fail("red-team verdict belongs to another frozen session", "E_VERDICT");
    }
  }
  const session = receipt.session;
  if (!validCommit(session.commit) || !validDigest(session.binary_sha256)
    || !validDigest(session.registry_sha256) || !validDigest(session.public_surface_sha256)) {
    fail("red-team verdict frozen session identity is invalid", "E_VERDICT");
  }
  if (manifest && (session.commit !== manifest.target.commit || session.binary_sha256 !== manifest.target.binary_sha256
    || session.registry_sha256 !== manifest.registry_snapshot.sha256
    || session.public_surface_sha256 !== manifest.public_surface_snapshot.sha256)) {
    fail("red-team verdict target does not match its manifest", "E_VERDICT");
  }
  if (!receipt.execution_gate || !Array.isArray(receipt.lanes) || !Array.isArray(receipt.findings)
    || !Array.isArray(receipt.finding_duplicates) || !Array.isArray(receipt.replayed_findings)
    || !Array.isArray(receipt.assimilation) || !Array.isArray(receipt.stale_reasons)
    || !Array.isArray(receipt.failure_reasons) || !receipt.quota || !receipt.cleanup
    || !receipt.independent_discovery) {
    fail("red-team verdict is missing complete session evidence", "E_VERDICT");
  }
  if (receipt.quota.lanes !== RED_TEAM_LANE_COUNT || receipt.quota.waves !== RED_TEAM_WAVE_COUNT
    || receipt.quota.lanes_per_wave !== RED_TEAM_MAX_ACTIVE || receipt.quota.full_quota_required !== true) {
    fail("red-team verdict quota is not eight lanes in four waves of two", "E_VERDICT");
  }
  boundedInteger(receipt.max_active_lanes, "red-team verdict max_active_lanes", 0, RED_TEAM_MAX_ACTIVE);
  if (typeof receipt.started_at !== "string" || typeof receipt.finished_at !== "string") fail("red-team verdict timestamps are missing", "E_VERDICT");
  const laneIds = new Set();
  for (const lane of receipt.lanes) {
    validateSignedLaneShape(lane, receipt, manifest);
    if (laneIds.has(lane.lane_id)) fail("red-team verdict repeats a lane", "E_VERDICT");
    laneIds.add(lane.lane_id);
  }
  if (!Array.isArray(receipt.lane_agents) || receipt.lane_agents.length !== receipt.lanes.length) {
    fail("red-team verdict lane agent evidence is incomplete", "E_VERDICT");
  }
  const laneAgentIds = new Set();
  const laneContexts = new Set();
  for (const laneAgent of receipt.lane_agents) {
    const lane = receipt.lanes.find((item) => item.lane_id === laneAgent.lane_id);
    if (!lane || lane.agent_id !== laneAgent.agent_id || lane.context_id !== laneAgent.context_id) {
      fail("red-team verdict lane agent evidence does not match lanes", "E_VERDICT");
    }
    if (laneAgentIds.has(laneAgent.agent_id) || laneContexts.has(laneAgent.context_id)) fail("red-team verdict repeats a lane identity", "E_VERDICT");
    laneAgentIds.add(laneAgent.agent_id);
    laneContexts.add(laneAgent.context_id);
  }
  const pseudoManifest = {
    target: { commit: session.commit, binary_sha256: session.binary_sha256 },
    registry_snapshot: { sha256: session.registry_sha256 },
  };
  const findings = receipt.findings.map((item, index) => validateFinding(item, index, pseudoManifest));
  const p0Count = findings.filter(p0Finding).length;
  if (receipt.p0_count !== p0Count || receipt.unique_finding_count !== findings.length) {
    fail("red-team verdict finding counts are inconsistent", "E_VERDICT");
  }
  if (receipt.status === "PASS") {
    if (receipt.lanes.length !== RED_TEAM_LANE_COUNT || laneIds.size !== RED_TEAM_LANE_COUNT) fail("red-team verdict quota is incomplete", "E_VERDICT");
    for (const laneId of LANE_BRIEFS.map((item) => item.lane_id)) if (!laneIds.has(laneId)) fail(`red-team verdict is missing ${laneId}`, "E_VERDICT");
    if (p0Count !== 0) fail("red-team verdict cannot pass with a new P0", "E_VERDICT");
    if (receipt.stale_reasons.length || receipt.failure_reasons.length) fail("red-team PASS verdict has failure or stale reasons", "E_VERDICT");
    const loadBearing = findings.filter((finding) => finding.load_bearing !== false).length;
    if (receipt.replayed_findings.length !== loadBearing || receipt.assimilation.length !== findings.length) {
      fail("red-team PASS verdict is missing replay or #2338 assimilation evidence", "E_VERDICT");
    }
    const cleanup = validateCleanupRecord(receipt.cleanup, "red-team verdict");
    if (cleanupProblems(cleanup).length) fail("red-team PASS verdict has cleanup residue", "E_VERDICT");
  }
}

function receiptUnsigned(receipt) {
  const copy = clone(receipt);
  delete copy.signature;
  delete copy.signed_payload_sha256;
  return copy;
}

export function signReceipt(payload, {
  signer_id = "hardening-red-team-signer",
  reviewer_id = "hardening-red-team-reviewer",
  private_key = undefined,
  key_pair = undefined,
} = {}) {
  requiredString(signer_id, "receipt signer_id");
  requiredString(reviewer_id, "receipt reviewer_id");
  if (signer_id === reviewer_id) fail("receipt signer and reviewer must be distinct", "E_SELF_REVIEW");
  const pair = key_pair || (private_key
    ? { privateKey: createPrivateKey(private_key), publicKey: createPublicKey(createPrivateKey(private_key)) }
    : generateKeyPairSync("ed25519"));
  const privateKey = pair.privateKey || pair.private_key;
  const publicKey = pair.publicKey || pair.public_key;
  if (!privateKey || !publicKey) fail("receipt signing key pair is incomplete", "E_SIGNATURE");
  const bodyPayload = clone(payload);
  delete bodyPayload.signature;
  delete bodyPayload.signed_payload_sha256;
  const body = {
    ...bodyPayload,
    schema: RED_TEAM_RECEIPT_SCHEMA,
    schema_version: RED_TEAM_SCHEMA_VERSION,
    signer_id,
    reviewer_id,
  };
  const signedPayload = canonicalJson(body);
  const signature = sign(null, Buffer.from(signedPayload, "utf8"), privateKey).toString("base64");
  const publicKeyPem = publicKey.export({ type: "spki", format: "pem" }).toString();
  return {
    ...body,
    signed_payload_sha256: digest(signedPayload),
    signature: {
      algorithm: "ed25519",
      encoding: "base64",
      signer_id,
      public_key_pem: publicKeyPem,
      value: signature,
    },
  };
}

export function verifySignedReceipt(receipt, manifest = undefined) {
  if (!receipt || receipt.schema !== RED_TEAM_RECEIPT_SCHEMA || receipt.schema_version !== RED_TEAM_SCHEMA_VERSION) {
    fail("invalid red-team receipt schema", "E_SIGNATURE");
  }
  const signature = receipt.signature;
  if (!signature || signature.algorithm !== "ed25519" || signature.encoding !== "base64") fail("receipt signature metadata is invalid", "E_SIGNATURE");
  if (typeof receipt.signer_id !== "string" || typeof receipt.reviewer_id !== "string"
    || signature.signer_id !== receipt.signer_id || receipt.signer_id === receipt.reviewer_id) {
    fail("receipt signer/reviewer identity is invalid", "E_SELF_REVIEW");
  }
  if (Array.isArray(receipt.lane_agents)) {
    const identities = new Set(receipt.lane_agents.flatMap((lane) => [lane.agent_id, lane.context_id]));
    if (identities.has(receipt.reviewer_id) || identities.has(receipt.signer_id)) {
      fail("lane agent cannot sign or review its own red-team verdict", "E_SELF_REVIEW");
    }
  }
  const body = receiptUnsigned(receipt);
  const signedPayload = canonicalJson(body);
  if (receipt.signed_payload_sha256 !== digest(signedPayload)) fail("receipt signed payload digest changed", "E_SIGNATURE");
  let valid = false;
  try {
    valid = verify(null, Buffer.from(signedPayload, "utf8"), createPublicKey(signature.public_key_pem), Buffer.from(signature.value, "base64"));
  } catch (error) {
    fail(`receipt signature cannot be verified: ${error.message}`, "E_SIGNATURE");
  }
  if (!valid) fail("receipt signature is invalid", "E_SIGNATURE");
  validateSignedVerdictShape(receipt, manifest);
  return true;
}

function laneAgents(lanes) {
  return lanes.map((lane) => ({ lane_id: lane.lane_id, agent_id: lane.agent_id, context_id: lane.context_id }));
}

function ensureDistinctReview(lanes, reviewerId, signerId) {
  const identities = new Set(lanes.flatMap((lane) => [lane.agent_id, lane.context_id]));
  if (identities.has(reviewerId) || identities.has(signerId)) fail("lane agent cannot sign or review its own red-team verdict", "E_SELF_REVIEW");
}

function resultPayload({
  manifest,
  lanes,
  findings,
  findingDuplicates,
  replayed,
  assimilation,
  status,
  staleReasons,
  failureReasons,
  cleanup,
  maxActive,
  startedAt,
  finishedAt,
  executionGate,
}) {
  return {
    schema: RED_TEAM_RECEIPT_SCHEMA,
    schema_version: RED_TEAM_SCHEMA_VERSION,
    receipt_kind: "fresh-context-red-team-verdict",
    status,
    session_id: manifest.session_id,
    manifest_sha256: manifest.manifest_sha256,
    session: {
      commit: manifest.target.commit,
      binary_sha256: manifest.target.binary_sha256,
      registry_sha256: manifest.registry_snapshot.sha256,
      public_surface_sha256: manifest.public_surface_snapshot.sha256,
    },
    quota: clone(manifest.quota),
    execution_gate: executionGate || manifest.execution_gate,
    lanes: lanes.map((lane) => clone(lane)),
    lane_agents: laneAgents(lanes),
    findings,
    finding_duplicates: findingDuplicates,
    replayed_findings: replayed,
    assimilation,
    stale_reasons: [...new Set(staleReasons)],
    failure_reasons: [...new Set(failureReasons)],
    p0_count: findings.filter(p0Finding).length,
    unique_finding_count: findings.length,
    max_active_lanes: maxActive,
    started_at: startedAt,
    finished_at: finishedAt,
    cleanup,
    independent_discovery: {
      current_defect_cards_hidden_until: "all-eight-independent-receipts",
      revealed_after_discovery: lanes.length === RED_TEAM_LANE_COUNT,
    },
  };
}

async function runLaneWave(packets, laneRunner, state) {
  if (packets.length > RED_TEAM_MAX_ACTIVE) fail("red-team runner exceeded two active lanes", "E_CONCURRENCY");
  const outcomes = await Promise.allSettled(packets.map(async (packet) => {
    validateContextPacket(packet);
    state.active += 1;
    state.max_active = Math.max(state.max_active, state.active);
    if (state.active > RED_TEAM_MAX_ACTIVE) fail("red-team runner exceeded two active lanes", "E_CONCURRENCY");
    try {
      return await laneRunner(packet);
    } finally {
      state.active -= 1;
    }
  }));
  return outcomes;
}

export async function runRedTeamSession({
  manifest,
  root = DEFAULT_ROOT,
  lane_runner = undefined,
  lane_receipts = undefined,
  replay_finding = undefined,
  assimilate = undefined,
  cleanup = undefined,
  current_target = undefined,
  check_target = true,
  signal = undefined,
  signer_id = "hardening-red-team-signer",
  reviewer_id = "hardening-red-team-reviewer",
  signing = undefined,
  execution_gate = undefined,
} = {}) {
  validateSessionManifest(manifest);
  manifest = deepFreeze(clone(manifest));
  const startedAt = now();
  const state = { active: 0, max_active: 0 };
  const packets = makeContextPackets(manifest);
  const lanes = [];
  const errors = [];
  const staleReasons = [];
  let findings = [];
  let findingDuplicates = [];
  let replayed = [];
  let assimilationActions = [];
  let cleanupResult = null;
  const resolveTarget = async () => {
    if (typeof current_target === "function") return current_target();
    if (current_target) return current_target;
    return currentTarget(resolve(root), join(resolve(root), manifest.target.binary_path), manifest.registry_snapshot.path, manifest.public_surface_snapshot.path);
  };
  try {
    let targetReady = true;
    if (check_target) {
      const snapshot = await resolveTarget();
      staleReasons.push(...targetDrift(manifest, snapshot));
      targetReady = staleReasons.length === 0;
    }
    if (!targetReady) {
      errors.push("target preflight failed; no lanes started");
    } else if (lane_receipts !== undefined) {
      if (!Array.isArray(lane_receipts)) fail("lane_receipts must be an array", "E_LANE");
      for (const report of lane_receipts) {
        try {
          lanes.push(validateLaneReceipt(report, manifest));
        } catch (error) {
          errors.push(error.message);
        }
      }
    } else {
      if (typeof lane_runner !== "function") fail("red-team runner needs a lane runner", "E_RUNNER");
      for (let wave = 0; wave < WAVE_LANES.length; wave += 1) {
        if (stopRequested(signal)) {
          errors.push("session cancelled before full quota");
          break;
        }
        const wavePackets = packets.filter((packet) => packet.wave === wave + 1);
        const outcomes = await runLaneWave(wavePackets, lane_runner, state);
        for (let index = 0; index < outcomes.length; index += 1) {
          const outcome = outcomes[index];
          if (outcome.status === "rejected") {
            errors.push(`${wavePackets[index].lane_id}: ${outcome.reason?.message || outcome.reason}`);
            continue;
          }
          try {
            lanes.push(validateLaneReceipt(outcome.value, manifest));
          } catch (error) {
            errors.push(error.message);
          }
        }
      }
    }
    if (check_target && targetReady) staleReasons.push(...targetDrift(manifest, await resolveTarget()));
    const byLane = new Map(lanes.map((lane) => [lane.lane_id, lane]));
    if (lanes.length !== RED_TEAM_LANE_COUNT || byLane.size !== RED_TEAM_LANE_COUNT) errors.push("full eight-lane quota is incomplete");
    for (const packet of packets) if (!byLane.has(packet.lane_id)) errors.push(`${packet.lane_id}: missing lane receipt`);
    const laneIdentities = new Set();
    const contextIdentities = new Set();
    const agentIdentities = new Set();
    for (const lane of lanes) {
      if (laneIdentities.has(lane.lane_id)) errors.push(`${lane.lane_id}: duplicate lane receipt`);
      laneIdentities.add(lane.lane_id);
      if (contextIdentities.has(lane.context_id)) errors.push(`${lane.lane_id}: duplicate fresh context`);
      contextIdentities.add(lane.context_id);
      if (agentIdentities.has(lane.agent_id)) errors.push(`${lane.lane_id}: duplicate lane agent`);
      agentIdentities.add(lane.agent_id);
    }
    if (state.active !== 0) errors.push("red-team lanes remain active");
    if (state.max_active > RED_TEAM_MAX_ACTIVE) errors.push("red-team runner exceeded two active lanes");
    ensureDistinctReview(lanes, reviewer_id, signer_id);
    const collected = laneFindings(lanes);
    findings = collected.findings;
    findingDuplicates = collected.duplicates;
    for (const lane of lanes) if (lane.semantic_change) staleReasons.push(`${lane.lane_id}: semantic change reported`);
    for (const lane of lanes) if (lane.registry_changed) staleReasons.push(`${lane.lane_id}: registry changed during discovery`);
    const assimilationHook = assimilate || ((items, frozen) => assimilateFindings(items, frozen, { root }));
    if (findings.some((finding) => finding.load_bearing)) {
      const replay = replay_finding || ((finding) => defaultReplayFinding(finding, manifest, { root }));
      for (const finding of findings) {
        if (stopRequested(signal)) {
          errors.push("session cancelled before finding replay");
          break;
        }
        if (finding.load_bearing) {
          try {
            const replayedFinding = normalizeReplay(await replay(finding, manifest), finding, manifest);
            replayed.push(replayedFinding);
            finding.replay = replayedFinding;
          } catch (error) {
            errors.push(error.message);
          }
        }
      }
    }
    if (findings.length) {
      // Confirmed findings always route through #2338 before the verdict is signed.
      const confirmed = findings.filter((finding) => !finding.load_bearing || finding.replay?.confirmed === true);
      if (confirmed.length) {
        try {
          assimilationActions = await assimilationHook(confirmed, manifest);
          validateAssimilationActions(assimilationActions, confirmed);
        } catch (error) {
          errors.push(error.message);
        }
      }
    }
  } catch (error) {
    errors.push(error.message);
  } finally {
    try {
      const reported = laneCleanupSummary(lanes);
      const cleanedValue = await cleanup?.({ manifest, packets, lanes, signal }) || {};
      const cleaned = Object.keys(cleanedValue).length
        ? validateCleanupRecord(cleanedValue, "session")
        : {};
      cleanupResult = cleanupSummary({
        active_agents: Math.max(reported.active_agents, cleaned.active_agents ?? 0),
        active_processes: Math.max(reported.active_processes, cleaned.active_processes ?? 0),
        scratch_paths: [...new Set([...reported.scratch_paths, ...(cleaned.scratch_paths || [])])],
        alternate_targets: [...new Set([...reported.alternate_targets, ...(cleaned.alternate_targets || [])])],
        unbounded_logs: reported.unbounded_logs || cleaned.unbounded_logs === true,
        complete: (cleaned.complete !== false) && reported.complete,
      });
    } catch (error) {
      cleanupResult = cleanupSummary({ complete: false });
      errors.push(error.message);
    }
    errors.push(...cleanupProblems(cleanupResult));
  }
  const status = staleReasons.length
    ? "STALE"
    : errors.length || findings.some(p0Finding) || lanes.length !== RED_TEAM_LANE_COUNT
      ? "FAILED"
      : "PASS";
  const finishedAt = now();
  const payload = resultPayload({
    manifest,
    lanes,
    findings,
    findingDuplicates,
    replayed,
    assimilation: assimilationActions,
    status,
    staleReasons,
    failureReasons: errors,
    cleanup: cleanupResult,
    maxActive: state.max_active,
    startedAt,
    finishedAt,
    executionGate: execution_gate,
  });
  ensureDistinctReview(lanes, reviewer_id, signer_id);
  const receipt = signReceipt(payload, { signer_id, reviewer_id, ...(signing || {}) });
  verifySignedReceipt(receipt);
  return receipt;
}

function defaultPaths(root) {
  const cache = resolve(process.env.JET_HARDENING_RED_TEAM_CACHE || DEFAULT_CACHE);
  return {
    cache,
    manifest: resolve(process.env.JET_HARDENING_RED_TEAM_MANIFEST || join(cache, "red-team/session.json")),
    receipt: resolve(process.env.JET_HARDENING_RED_TEAM_RECEIPT || join(cache, "red-team/receipt.json")),
    root,
  };
}

function machineOutput(value, json) {
  if (json) process.stdout.write(`${JSON.stringify(value, null, 2)}\n`);
  else process.stdout.write(`${value.status || value.execution_gate || "OK"}\n`);
}

function commandArgs(args) {
  const values = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) continue;
    const [key, inline] = arg.slice(2).split("=", 2);
    values[key.replaceAll("-", "_")] = inline ?? (args[index + 1]?.startsWith("--") ? true : args[++index]);
  }
  return values;
}

function commandLaneRunner(options, paths, manifest) {
  const runner = options.runner || process.env.JET_HARDENING_RED_TEAM_RUNNER;
  if (!runner) fail("real red-team execution needs JET_HARDENING_RED_TEAM_RUNNER", "E_RUNNER");
  const runnerArgs = String(options.runner_args || process.env.JET_HARDENING_RED_TEAM_RUNNER_ARGS || "")
    .split(" ").filter(Boolean);
  return async (packet) => {
    const result = await executeCommand({
      program: runner,
      args: runnerArgs,
      cwd: paths.root,
      stdin: JSON.stringify(packet),
      timeout_ms: manifest.resource_limits.lane_timeout_ms,
      capture_limit: manifest.resource_limits.capture_bytes,
      label: `red-team:${packet.lane_id}`,
    });
    if (!result.ok || result.stdout_truncated) fail(`${packet.lane_id} runner failed or exceeded capture bound`, "E_RUNNER");
    try {
      return JSON.parse(result.stdout.toString("utf8"));
    } catch (error) {
      fail(`${packet.lane_id} runner did not return JSON: ${error.message}`, "E_RUNNER");
    }
  };
}

export async function redTeamMain(argv = process.argv.slice(2)) {
  const args = [...argv];
  const command = args.find((arg) => !arg.startsWith("--")) || "help";
  const options = commandArgs(args);
  const json = args.includes("--json");
  const root = resolve(options.root || process.env.JET_HARDENING_ROOT || DEFAULT_ROOT);
  const paths = defaultPaths(root);
  if (command === "help" || command === "--help" || command === "-h") {
    process.stdout.write([
      "usage: hardening-red-team.mjs <plan|packets|run|verify> [--json]",
      "plan creates and freezes the clean target/session manifest; it never starts lanes.",
      "packets emits the eight independent context packets with defect cards hidden.",
      "run requires --execute-real and JET_HARDENING_RED_TEAM_RUNNER; owner gate is explicit.",
      "verify checks the signed receipt without reading Tower storage.",
    ].join("\n") + "\n");
    return 0;
  }
  if (command === "plan") {
    const manifest = writeSessionManifest(paths.manifest, createSessionManifest({ root }));
    machineOutput({ status: "PLANNED", execution_gate: manifest.execution_gate, manifest_path: paths.manifest, manifest }, json);
    return 0;
  }
  if (command === "packets") {
    const manifest = readSessionManifest(paths.manifest);
    const packets = makeContextPackets(manifest);
    machineOutput({ status: "PACKETS_READY", packets }, json);
    return 0;
  }
  if (command === "verify") {
    const receipt = readJson(options.receipt || paths.receipt);
    const manifest = readSessionManifest(options.manifest || paths.manifest);
    verifySignedReceipt(receipt, manifest);
    machineOutput({ status: receipt.status, verified: true, receipt_path: options.receipt || paths.receipt }, json);
    return 0;
  }
  if (command === "run") {
    const executeReal = options.execute_real === true || options.execute_real === "true" || process.env.JET_HARDENING_RED_TEAM_EXECUTE === "1";
    if (!executeReal) {
      machineOutput({ status: "BLOCKED", execution_gate: "OWNER_REQUIRED_FOR_REAL_EIGHT_LANE_EXECUTION", reason: "real eight-lane execution is owner-gated and was not started" }, json);
      return 2;
    }
    const manifest = readSessionManifest(paths.manifest);
    const runner = commandLaneRunner(options, paths, manifest);
    const receipt = await runRedTeamSession({
      manifest,
      root,
      lane_runner: runner,
      replay_finding: undefined,
      assimilate: (findings, frozen) => assimilateFindings(findings, frozen, { root }),
      execution_gate: "OWNER_AUTHORIZED_REAL_EIGHT_LANE_EXECUTION",
    });
    writeJson(options.receipt || paths.receipt, receipt);
    machineOutput({ status: receipt.status, receipt_path: options.receipt || paths.receipt, receipt }, json);
    return receipt.status === "PASS" ? 0 : 1;
  }
  fail(`unknown red-team command: ${command}`, "E_USAGE");
}

const invokedPath = process.argv[1] && resolve(process.argv[1]);
const modulePath = resolve(fileURLToPath(import.meta.url));
if (invokedPath === modulePath) {
  redTeamMain().then((code) => process.exit(code)).catch((error) => {
    process.stderr.write(`hardening-red-team: ${error.stack || error.message}\n`);
    process.exit(1);
  });
}
