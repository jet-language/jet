#!/usr/bin/env node

/**
 * Future positive-observation producer for comptime proof (#2938).
 *
 * The production program owns the observation values.  It writes the same
 * typed JSONL journal consumed by the adapter observation rail; this runner
 * only authenticates the journal against the real command, cache directory,
 * and source identity.  No output-line heuristic or marker can become a
 * semantic observation.  Missing journal fields stay unverified.
 */

import {
  accessSync,
  constants as fsConstants,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  canonical,
  canonicalJson,
  digestBytes,
  digestText,
  replay,
  validateManifest,
} from "../../../scripts/agent/compiler-proof.mjs";
import {
  OBSERVATION_FIELDS as JOURNAL_OBSERVATION_FIELDS,
  NULLABLE_OBSERVATION_FIELDS,
  readComparisonJournal,
  writeComparisonCorpus,
} from "../observations/comparison-journal.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const STAGE = "comptime";
const CHECKER_PATH = "proof/compiler/comptime/check.mjs";
const OBSERVER_PATH = "proof/compiler/comptime/observe.mjs";
const OBSERVATIONS_PATH = "proof/compiler/comptime/observations.json";
const CONTRACT_PATH = "proof/compiler/comptime/contract.json";
const RELATION_PATH = "proof/compiler/obligations.json";
const MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const RUNNER_PATH = "scripts/agent/jet-env";
const OBSERVATION_FIELDS = JOURNAL_OBSERVATION_FIELDS;
const IDENTITY_FIELDS = Object.freeze([
  "source_sha256",
  "program_sha256",
  "dependency_closure_sha256",
  "configuration_sha256",
  "evaluator_sha256",
  "compiler_identity",
  "checker_sha256",
  "target",
  "obligations_sha256",
]);
const CACHE_EVENTS = Object.freeze([
  "source_dependency_changed",
  "imported_fact_changed",
  "compiler_abi_changed",
  "target_profile_effect_changed",
  "evaluator_artifact_changed",
  "configuration_changed",
  "interrupted_before_publish",
  "failed_without_outputs",
]);
const DEFAULT_FIXTURE = "proof/compiler/comptime/fixtures/mixed-reader.jet";
const COMPILER_PATH = "target/debug/jet";
const DEFAULT_SCRATCH = join(homedir(), ".cache", "jet-test-scratch");
const TIMEOUT_MS = 120000;
const MODES = Object.freeze(["aot", "jet_run", "interpreter"]);
const OPERATION_ID = "mixed-reader-mutation-fallible";

function fail(message, details = undefined) {
  const error = new Error(message);
  error.details = details;
  throw error;
}

function projectPath(value, label) {
  if (typeof value !== "string" || value.length === 0 || value.includes("\\") || value.includes("\0")) {
    fail(`${label} must be a non-empty project-relative path`);
  }
  const normalized = value.replaceAll("\\", "/");
  if (normalized.startsWith("/") || normalized.split("/").includes("..")) fail(`${label} escapes the project`);
  const absolute = resolve(ROOT, normalized);
  if (absolute !== ROOT && !absolute.startsWith(`${ROOT}${sep}`)) fail(`${label} escapes the project`);
  return normalized;
}

function absoluteProjectPath(value, label) {
  const project = projectPath(value, label);
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail(`${label} is missing: ${project}`);
  return absolute;
}

function digestFile(value, label = "file") {
  return digestBytes(readFileSync(absoluteProjectPath(value, label)));
}

function readJson(value, label) {
  try {
    return JSON.parse(readFileSync(absoluteProjectPath(value, label), "utf8"));
  } catch (error) {
    fail(`${label} is not valid JSON: ${error.message}`);
  }
}

function readEvidenceJournal(path, schema, mode) {
  if (!existsSync(path)) {
    return { events: [], errors: [], status: "unavailable", reason: `no ${schema} evidence was emitted` };
  }
  let lines;
  try {
    lines = readFileSync(path, "utf8").split(/\r?\n/u).filter(Boolean);
  } catch (error) {
    return { events: [], errors: [`${schema} evidence could not be read: ${error.message}`], status: "failed" };
  }
  const events = [];
  const errors = [];
  for (const line of lines) {
    let event;
    try {
      event = JSON.parse(line);
    } catch (error) {
      errors.push(`${schema} evidence emitted invalid JSON: ${error.message}`);
      continue;
    }
    if (!event || typeof event !== "object" || Array.isArray(event)
        || event.schema !== schema
        || event.mode !== mode
        || event.operation_id !== OPERATION_ID) {
      errors.push(`${schema} evidence identity is invalid`);
      continue;
    }
    events.push(event);
  }
  return {
    events,
    errors,
    status: errors.length > 0 ? "failed" : events.length > 0 ? "observed" : "unavailable",
    ...(errors.length > 0 ? { reason: errors.join("; ") } : {}),
  };
}

function safeScratch(value) {
  const candidate = resolve(value ?? process.env.JET_TEST_SCRATCH_DIR ?? DEFAULT_SCRATCH);
  const cacheRoot = resolve(homedir(), ".cache");
  if (candidate === "/tmp" || candidate.startsWith("/tmp/") || (candidate !== cacheRoot && !candidate.startsWith(`${cacheRoot}${sep}`))) {
    fail("observation scratch must be under $HOME/.cache and never /tmp", { scratch: candidate });
  }
  mkdirSync(candidate, { recursive: true });
  return candidate;
}

function parseArgs(argv) {
  const options = { json: false, fixture: DEFAULT_FIXTURE, output: OBSERVATIONS_PATH, scratch: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") options.json = true;
    else if (arg === "--fixture" || arg === "--output" || arg === "--scratch") {
      const value = argv[++index];
      if (!value || value.startsWith("--")) fail(`${arg} requires a value`);
      options[arg.slice(2)] = value;
    } else if (arg === "--help" || arg === "-h") options.help = true;
    else fail(`unknown argument ${arg}`);
  }
  return options;
}

function expectedArgs(mode, entry) {
  if (mode === "aot") return ["jet", "run", "--release", entry];
  if (mode === "jet_run") return ["jet", "run", entry];
  if (mode === "interpreter") return ["jet", "run", "--interpret", entry];
  fail(`unsupported comptime observation mode ${mode}`);
}


function executeMode(caseRoot, entry, mode, cacheRoot) {
  const journalPath = join(cacheRoot, mode, "journal.json");
  const evaluatorPath = `${journalPath}.evaluator.jsonl`;
  const cachePath = `${journalPath}.cache.jsonl`;
  mkdirSync(join(cacheRoot, mode), { recursive: true });
  const command = [RUNNER_PATH, "full", ...expectedArgs(mode, entry)];
  const environment = {
    ...process.env,
    JET_COMPTIME_MODE: mode,
    JET_COMPTIME_OPERATION_ID: OPERATION_ID,
    JET_COMPTIME_OBSERVATION_PATH: journalPath,
    // The adapter rail owns the JSONL format and collector implementation.
    JET_ADAPTER_MODE: mode,
    JET_ADAPTER_OPERATION_IDS: OPERATION_ID,
    JET_ADAPTER_OBSERVATION_PATH: journalPath,
    JET_COMPTIME_EVALUATOR_OBSERVATION_PATH: evaluatorPath,
    JET_COMPTIME_CACHE_OBSERVATION_PATH: cachePath,
    JET_RUN_CACHE_DIR: join(cacheRoot, "run-cache"),
    JET_DEV_ORACLE_CACHE_DIR: join(cacheRoot, "oracle-cache"),
    JET_TEST_SCRATCH_DIR: cacheRoot,
    LC_ALL: "C",
    LANG: "C",
    NO_COLOR: "1",
    CLICOLOR: "0",
  };
  const child = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", ...expectedArgs(mode, entry)], {
    cwd: caseRoot,
    env: environment,
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 16 * 1024 * 1024,
  });
  const stdout = `${child.stdout ?? ""}`;
  const stderr = `${child.stderr ?? ""}`;
  const evaluator = readEvidenceJournal(evaluatorPath, "jet.comptime-evaluator-event.v1", mode);
  const cache = readEvidenceJournal(cachePath, "jet.comptime-cache-event.v1", mode);
  const journal = readComparisonJournal(journalPath, {
    mode,
    operationIds: [OPERATION_ID],
    nullableFields: NULLABLE_OBSERVATION_FIELDS,
  });
  const record = journal.records.get(OPERATION_ID);
  const failed = Boolean(child.error) || child.status !== 0;
  const status = failed
    ? (child.error?.code === "ETIMEDOUT" ? "timeout" : "failed")
    : record && journal.errors.length === 0 && evaluator.events.length > 0 && evaluator.errors.length === 0 && cache.events.length > 0 && cache.errors.length === 0
      ? "observed"
      : "unverified";
  return {
    mode,
    command,
    status,
    observations: record?.observations ?? {},
    missing_dimensions: record ? [] : [...OBSERVATION_FIELDS],
    journal_sha256: record?.record_sha256 ?? null,
    journal_count: journal.journal_count,
    journal_errors: journal.errors,
    evaluator_events: evaluator.events,
    evaluator_status: evaluator.status,
    evaluator_errors: evaluator.errors,
    cache_events: cache.events,
    cache_status: cache.status,
    cache_errors: cache.errors,
    exit_code: Number.isInteger(child.status) ? child.status : null,
    signal: child.signal ?? null,
    timed_out: child.error?.code === "ETIMEDOUT",
    stdout_sha256: digestText(stdout),
    stderr_sha256: digestText(stderr),
    ...(journal.errors.length > 0 ? { reason: journal.errors.join("; ") } : {}),
    ...(child.error ? { error_code: child.error.code ?? null, reason: child.error.message } : {}),
    ...(child.status !== 0 && !child.error ? { reason: `Jet exited ${String(child.status)}` } : {}),
  };
}


function runCanonicalComparison(caseRoot, modeResults, cacheRoot) {
  const pairs = [["aot", "jet_run"], ["aot", "interpreter"]];
  const comparisons = [];
  for (const [referenceMode, candidateMode] of pairs) {
    const reference = modeResults[referenceMode]?.observations;
    const candidate = modeResults[candidateMode]?.observations;
    if (modeResults[referenceMode]?.status !== "observed" || modeResults[candidateMode]?.status !== "observed") {
      comparisons.push({ reference_mode: referenceMode, candidate_mode: candidateMode, status: "unavailable", reason: "both modes need complete typed journals before canonical comparison" });
      continue;
    }
    const corpusPath = join(cacheRoot, `comparison-${referenceMode}-${candidateMode}.json`);
    const corpus = writeComparisonCorpus(corpusPath, { operationId: OPERATION_ID, referenceMode, candidateMode, reference, candidate });
    const command = [RUNNER_PATH, "full", "jet", "test-compare", corpusPath, "--json"];
    const child = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", "jet", "test-compare", corpusPath, "--json"], {
      cwd: caseRoot,
      env: { ...process.env, JET_TEST_SCRATCH_DIR: cacheRoot, JET_RUN_CACHE_DIR: join(cacheRoot, "comparison-cache"), JET_DEV_ORACLE_CACHE_DIR: join(cacheRoot, "comparison-oracle"), LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
      encoding: "utf8",
      timeout: TIMEOUT_MS,
      maxBuffer: 16 * 1024 * 1024,
    });
    const output = `${child.stdout ?? ""}`.trim().split(/\r?\n/u).filter(Boolean).at(-1);
    let report = null;
    try { report = output ? JSON.parse(output) : null; } catch { report = null; }
    const status = child.error?.code === "ETIMEDOUT"
      ? "timeout"
      : child.error || child.status !== 0
        ? "unavailable"
        : report?.status === "matched" ? "matched" : report?.status === "mismatch" ? "mismatch" : "unavailable";
    comparisons.push({ reference_mode: referenceMode, candidate_mode: candidateMode, status, corpus: corpusPath, corpus_sha256: digestFile(corpusPath, "comparison corpus"), command, exit_code: Number.isInteger(child.status) ? child.status : null, report, universal_proof: false });
  }
  return comparisons;
}

function cacheLookupStatus(result) {
  const event = (result?.cache_events ?? []).find((candidate) => candidate.phase === "lookup");
  return event?.status ?? "unknown";
}

function cacheKeys(result) {
  return [...new Set((result?.cache_events ?? []).map((event) => event.key).filter((key) => typeof key === "string"))].sort();
}

function cacheTransition(id, changedIdentity, expectedPublication, first, second) {
  return {
    id,
    changed_identity: changedIdentity,
    expected_lookup: "miss",
    expected_publication: expectedPublication,
    status: "unverified",
    observed_lookup: cacheLookupStatus(second),
    first_execution_sha256: digestText(canonicalJson(first)),
    second_execution_sha256: digestText(canonicalJson(second)),
    first_cache_events: first?.cache_events ?? [],
    second_cache_events: second?.cache_events ?? [],
    first_cache_status: first?.cache_status ?? "unavailable",
    second_cache_status: second?.cache_status ?? "unavailable",
    receipt_status: first?.cache_events?.length > 0 && second?.cache_events?.length > 0 ? "observed" : "unavailable",
    cache_key_transition: {
      baseline_keys: cacheKeys(first),
      changed_keys: cacheKeys(second),
      changed: JSON.stringify(cacheKeys(first)) !== JSON.stringify(cacheKeys(second)),
    },
    universal_proof: false,
  };
}

function runCacheInvalidation(caseRoot, entry, cacheRoot, baseline) {
  const sourcePath = join(caseRoot, entry);
  const before = readFileSync(sourcePath, "utf8");
  const after = before.replace("[U8]{7, 9, 11}", "[U8]{8, 9, 11}");
  if (after === before) fail("source invalidation probe could not create a distinct source");
  writeFileSync(sourcePath, after, "utf8");
  try {
    const changed = executeMode(caseRoot, entry, "jet_run", cacheRoot);
    const sourceEvent = cacheTransition("source_dependency_changed", "source_bytes_sha256", "invalidated", baseline, changed);
    const unsupported = CACHE_EVENTS.filter((id) => id !== sourceEvent.id).map((id) => ({
      id,
      changed_identity: id,
      expected_lookup: "miss",
      expected_publication: ["interrupted_before_publish", "failed_without_outputs"].includes(id) ? "none" : "invalidated",
      status: "unverified",
      observed_lookup: "unknown",
      reason: "the selected production command did not expose this cache event receipt",
      universal_proof: false,
    }));
    return {
      schema: "jet.comptime-cache-observations.v1",
      status: "unverified",
      events: [
        { ...sourceEvent, before_source_sha256: digestBytes(Buffer.from(before, "utf8")), after_source_sha256: digestBytes(Buffer.from(after, "utf8")), changed_execution: changed },
        ...unsupported,
      ].sort((left, right) => left.id.localeCompare(right.id)),
      receipts: {
        baseline_cache_events: baseline?.cache_events ?? [],
        changed_cache_events: changed.cache_events ?? [],
        baseline_status: baseline?.cache_status ?? "unavailable",
        changed_status: changed.cache_status ?? "unavailable",
      },
    };
  } finally {
    writeFileSync(sourcePath, before, "utf8");
  }
}

function identityFor(contract, fixture, dependencies, manifest) {
  validateManifest(manifest);
  const claim = manifest.claims.find((candidate) => candidate.id === contract.identity.claim_id);
  if (!claim) fail(`proof claim ${contract.identity.claim_id} is missing`);
  const rows = dependencies.map((dependency) => ({ path: dependency.path, role: dependency.role, sha256: digestFile(dependency.path) })).sort((left, right) => left.path.localeCompare(right.path));
  return {
    source_sha256: digestFile(fixture),
    program_sha256: digestFile(fixture),
    dependency_closure_sha256: digestText(canonicalJson(rows)),
    configuration_sha256: digestText(canonicalJson({ stage: STAGE, execution: contract.execution, cache: contract.cache, policy: contract.policy })),
    evaluator_sha256: digestText(canonicalJson(contract.evaluator)),
    compiler_identity: claim.identity_sha256,
    checker_sha256: digestFile(CHECKER_PATH, "comptime checker"),
    target: null,
    obligations_sha256: digestFile(RELATION_PATH, "obligation relation"),
  };
}

function trustedGoalReplay(contract, manifest) {
  const authorityKey = contract.identity?.authority_key;
  const claimId = contract.identity?.claim_id;
  const obligationId = contract.identity?.obligation_id;
  if (typeof authorityKey !== "string" || authorityKey.length === 0) {
    return { status: "unavailable", reason: "comptime contract has no approved replay authority key", universal_proof: false };
  }
  if (typeof claimId !== "string" || claimId.length === 0) {
    return { status: "unavailable", reason: "comptime contract has no replay claim identity", universal_proof: false };
  }
  if (typeof obligationId !== "string" || obligationId.length === 0) {
    return { status: "unavailable", reason: "comptime contract has no explicit replay obligation identity", authority_key: authorityKey, universal_proof: false };
  }
  const approved = manifest.approved_authorities?.[authorityKey];
  if (!approved || typeof approved !== "object" || Array.isArray(approved)) {
    return { status: "unavailable", reason: `approved replay authority ${authorityKey} is absent from the pinned manifest`, authority_key: authorityKey, universal_proof: false };
  }
  try {
    const report = replay(MANIFEST_PATH, claimId, { authority_key: authorityKey, obligation_id: obligationId });
    return {
      status: report.status === "proved" ? "proved" : "unavailable",
      authority_key: authorityKey,
      obligation_id: obligationId,
      report,
      universal_proof: false,
    };
  } catch (error) {
    return {
      status: "unavailable",
      authority_key: authorityKey,
      obligation_id: obligationId,
      reason: error.message,
      universal_proof: false,
    };
  }
}
function produce(options) {
  const fixture = projectPath(options.fixture, "fixture");
  const contract = readJson(CONTRACT_PATH, "comptime contract");
  const manifest = readJson(MANIFEST_PATH, "proof manifest");
  const dependencies = contract.dependencies.map(({ path, role }) => ({ path, role }));
  const scratch = safeScratch(options.scratch);
  const caseRoot = mkdtempSync(join(scratch, "comptime-observe-"));
  const entry = "mixed-reader.jet";
  cpSync(resolve(ROOT, fixture), join(caseRoot, entry));
  const cacheRoot = join(caseRoot, "cache");
  mkdirSync(cacheRoot, { recursive: true });
  try {
    const compilerPath = resolve(ROOT, COMPILER_PATH);
    let compilerAvailable = existsSync(compilerPath);
    if (compilerAvailable) {
      try { accessSync(compilerPath, fsConstants.X_OK); } catch { compilerAvailable = false; }
    }
    const modeResults = compilerAvailable
      ? Object.fromEntries(MODES.map((mode) => [mode, executeMode(caseRoot, entry, mode, cacheRoot)]))
      : Object.fromEntries(MODES.map((mode) => [mode, { mode, status: "unavailable", command: [RUNNER_PATH, "full", ...expectedArgs(mode, entry)], observations: {}, missing_dimensions: [...OBSERVATION_FIELDS], reason: "target/debug/jet is missing or not executable" }]));
    const comparisons = compilerAvailable ? runCanonicalComparison(caseRoot, modeResults, cacheRoot) : MODES.slice(1).map((mode) => ({ reference_mode: "aot", candidate_mode: mode, status: "unavailable", reason: "compiler unavailable" }));
    const cacheInvalidation = compilerAvailable ? runCacheInvalidation(caseRoot, entry, cacheRoot, modeResults.jet_run) : { schema: "jet.comptime-cache-observations.v1", status: "unavailable", events: [], reason: "compiler unavailable; cache probe was not started", universal_proof: false };
    const identityBase = identityFor(contract, fixture, dependencies, manifest);
    const matched = comparisons.every((comparison) => comparison.status === "matched");
    const records = MODES.filter((mode) => modeResults[mode].status === "observed" && matched).map((mode) => ({
      operation_id: OPERATION_ID,
      mode,
      identity: { ...identityBase, target: mode },
      identity_fields: [...IDENTITY_FIELDS],
      dimensions: [...OBSERVATION_FIELDS],
      observations: canonical(modeResults[mode].observations),
      evaluator_events: canonical(modeResults[mode].evaluator_events ?? []),
      cache_events: canonical(modeResults[mode].cache_events ?? []),
      comparison: { status: "matched", relation: "ordered_effects", pairs: comparisons, universal_proof: false },
      certificate: {
        schema: "jet.comptime-observation-certificate.v1",
        status: "verified",
        universal_proof: false,
        producer: OBSERVER_PATH,
        execution_id: digestText(canonicalJson({
          mode,
          journal: modeResults[mode].journal_sha256,
          evaluator_events: modeResults[mode].evaluator_events ?? [],
          cache_events: modeResults[mode].cache_events ?? [],
          comparisons,
        })),
        checker: CHECKER_PATH,
        runner: RUNNER_PATH,
        command: modeResults[mode].command,
        exit_code: modeResults[mode].exit_code,
        journal_sha256: modeResults[mode].journal_sha256,
      },
    }));
    const payload = {
      schema: "jet.comptime-observations.v1",
      schema_version: 1,
      stage: STAGE,
      producer: OBSERVER_PATH,
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      fixture,
      fixture_sha256: digestFile(fixture),
      identity_fields: [...IDENTITY_FIELDS],
      observation_fields: [...OBSERVATION_FIELDS],
      status: records.length === MODES.length ? "verified" : compilerAvailable ? "unverified" : "unavailable",
      records,
      modes: modeResults,
      comparisons,
      cache_invalidation: cacheInvalidation,
      trusted_goal_replay: trustedGoalReplay(contract, manifest),
      universal_proof: false,
    };
    const output = resolve(ROOT, projectPath(options.output, "observation output"));
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
    return payload;
  } finally {
    rmSync(caseRoot, { recursive: true, force: true });
  }
}

function usage() {
  return "Usage: observe.mjs [--json] [--fixture PATH] [--output PATH] [--scratch PATH]";
}

export async function main(argv = process.argv.slice(2)) {
  try {
    const options = parseArgs(argv);
    if (options.help) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    const result = produce(options);
    if (options.json) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stdout.write(`comptime observations: ${result.status}; records=${result.records.length}\n`);
    return 0;
  } catch (error) {
    const result = { schema: "jet.comptime-observations.v1", schema_version: 1, stage: STAGE, status: "unavailable", error: { message: error.message, ...(error.details === undefined ? {} : { details: error.details }) }, universal_proof: false };
    if (process.argv.includes("--json")) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stderr.write(`${result.error.message}\n`);
    return 3;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = await main();

export { CACHE_EVENTS, IDENTITY_FIELDS, OBSERVATION_FIELDS, produce };
