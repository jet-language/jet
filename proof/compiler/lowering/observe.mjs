#!/usr/bin/env node

/**
 * Future positive-observation producer for the lowering and optimization proof rails.
 *
 * The producer captures compiler-owned canonical pass records through the
 * inspect projection and the append-only emitter-process journal, then
 * executes the real Jet runner in each route mode. Static pass records come
 * from compiler boundaries; runtime dimensions come from the shared typed
 * comparison journal and never get copied onto static nodes.
 */

import { accessSync, constants as fsConstants, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync, readFileSync, cpSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { replay } from "../../../scripts/agent/compiler-proof.mjs";

import {
  BASE_PREMISES,
  OBSERVATION_FIELDS,
  OBSERVATION_IDENTITY_FIELDS,
  OBSERVER_PATHS,
  REPLAY_ROUTES,
  RUNTIME_ROUTES,
  ROUTE_PROTOCOL,
  ROUTE_SCHEMA,
  SCENARIOS,
  TARGETS,
  canonicalJson,
  digestText,
  operationRegistry,
  readJson,
  readSource,
} from "./common.mjs";
import { readComparisonJournal } from "../observations/comparison-journal.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const RUNNER_PATH = "scripts/agent/jet-env";
const COMPILER_PATH = "target/debug/jet";
const CHECKER_PATHS = Object.freeze({
  lowering: "proof/compiler/lowering/check.mjs",
  optimization: "proof/compiler/optimization/check.mjs",
});
const CONTRACT_PATHS = Object.freeze({
  lowering: "proof/compiler/lowering/contract.json",
  optimization: "proof/compiler/optimization/contract.json",
});
const WITNESS_PATHS = Object.freeze({
  lowering: "proof/compiler/lowering/witnesses.json",
  optimization: "proof/compiler/optimization/witnesses.json",
});
const DEFAULT_FIXTURES = Object.freeze({
  lowering: "proof/compiler/lowering/fixtures/legal-shared-route.jet",
  optimization: "proof/compiler/optimization/fixtures/legal-checked-loop.jet",
});
const OBSERVATIONS_PATHS = Object.freeze({
  lowering: "proof/compiler/lowering/observations.json",
  optimization: "proof/compiler/optimization/observations.json",
});
const TIMEOUT_MS = 120000;

function fail(message, details = undefined) {
  const error = new Error(message);
  error.details = details;
  throw error;
}

function projectPath(value, label) {
  if (typeof value !== "string" || value.length === 0 || value.includes("\\") || value.includes("\0")) {
    fail(`${label} must be a project-relative path`);
  }
  const target = resolve(ROOT, value);
  const prefix = `${ROOT}${sep}`;
  if (target !== ROOT && !target.startsWith(prefix)) fail(`${label} escapes the project root`);
  return target;
}

function safeScratch(value, stage) {
  const candidate = resolve(value ?? process.env.JET_TEST_SCRATCH_DIR ?? join(homedir(), ".cache", `jet-${stage}-observations`));
  const cacheRoot = resolve(homedir(), ".cache");
  if (candidate === "/tmp" || candidate.startsWith("/tmp/") || (candidate !== cacheRoot && !candidate.startsWith(`${cacheRoot}${sep}`))) {
    fail("observation scratch must be under $HOME/.cache and never /tmp", { scratch: candidate });
  }
  mkdirSync(candidate, { recursive: true });
  return candidate;
}

function parseArgs(argv) {
  const options = {
    json: false,
    stage: "lowering",
    fixture: undefined,
    output: undefined,
    scratch: undefined,
    formal: false,
    operation: undefined,
    replayPath: "proof/compiler/toolchain/manifest.json",
    claimId: undefined,
    dischargeOutput: undefined,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") options.json = true;
    else if (arg === "--formal") options.formal = true;
    else if (["--stage", "--fixture", "--output", "--scratch", "--operation", "--replay-path", "--claim-id", "--discharge-output"].includes(arg)) {
      if (!argv[index + 1] || argv[index + 1].startsWith("--")) fail(`${arg} requires a value`);
      const key = {
        "--stage": "stage",
        "--fixture": "fixture",
        "--output": "output",
        "--scratch": "scratch",
        "--operation": "operation",
        "--replay-path": "replayPath",
        "--claim-id": "claimId",
        "--discharge-output": "dischargeOutput",
      }[arg];
      options[key] = argv[++index];
    } else if (arg === "--help" || arg === "-h") options.help = true;
    else fail(`unknown argument ${arg}`);
  }
  if (!Object.hasOwn(DEFAULT_FIXTURES, options.stage)) fail(`unknown stage: ${options.stage}`);
  options.fixture ??= DEFAULT_FIXTURES[options.stage];
  options.output ??= OBSERVATIONS_PATHS[options.stage];
  options.dischargeOutput ??= `proof/compiler/${options.stage}/discharge.json`;
  return options;
}

function expectedArgs(mode, entry) {
  if (mode === "aot") return ["jet", "run", "--release", entry];
  if (mode === "jet_run") return ["jet", "run", entry];
  if (mode === "interpreter") return ["jet", "run", "--interpret", entry];
  if (mode === "check") return ["jet", "check", entry];
  fail(`unsupported observation route mode ${mode}`);
}

function parseCanonicalRoutes(payload, stage, mode, operations) {
  const routeRows = payload && Array.isArray(payload.canonical_routes)
    ? payload.canonical_routes
    : null;
  if (!routeRows) return { rows: [], errors: [`${mode} inspect decisions did not emit canonical_routes`] };
  const rowsById = new Map();
  const errors = [];
  for (const pass of routeRows) {
    if (!pass || typeof pass !== "object" || Array.isArray(pass)) {
      errors.push(`${mode} canonical route pass is not an object`);
      continue;
    }
    if (pass.schema !== "jet.canonical-pass-record.v1"
      || pass.protocol !== "jet.canonical-pass.v1"
      || pass.stage !== stage
      || typeof pass.operation_id !== "string"
      || !operations.has(pass.operation_id)
      || !Number.isInteger(pass.occurrence)
      || pass.occurrence < 1
      || !Number.isInteger(pass.order)
      || pass.order < 1) {
      errors.push(`${mode} canonical route pass identity is invalid`);
      continue;
    }
    if (!pass.source || typeof pass.source !== "object" || typeof pass.source.path !== "string"
      || !pass.input || typeof pass.input !== "object"
      || !pass.output || typeof pass.output !== "object"
      || !Array.isArray(pass.premises)
      || typeof pass.disposition !== "string"
      || pass.disposition.length === 0) {
      errors.push(`${mode} canonical route pass ${pass.operation_id} is incomplete`);
      continue;
    }
    const existing = rowsById.get(pass.operation_id) ?? [];
    existing.push(pass);
    rowsById.set(pass.operation_id, existing);
  }
  const rows = [...operations.keys()].map((operationId) => {
    const passes = (rowsById.get(operationId) ?? []).sort((left, right) => left.order - right.order);
    return {
      operation_id: operationId,
      route: {
        schema: ROUTE_SCHEMA,
        protocol: ROUTE_PROTOCOL,
        stage,
        mode,
        operation_id: operationId,
        passes,
        coverage: {
          observed: passes.length > 0,
          occurrence_count: passes.length,
        },
      },
    };
  });
  return { rows, errors };
}

function readCanonicalProcessJournal(path) {
  if (typeof path !== "string" || !existsSync(path)) return { records: [], errors: [], processes: [] };
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (error) {
    return { records: [], errors: [`canonical process journal read failed: ${error.message}`], processes: [] };
  }
  const records = [];
  const processes = [];
  const errors = [];
  for (const [index, line] of text.split(/\r?\n/u).entries()) {
    if (line.trim().length === 0) continue;
    let payload;
    try {
      payload = JSON.parse(line);
    } catch (error) {
      errors.push(`canonical process journal line ${index + 1} is invalid JSON: ${error.message}`);
      continue;
    }
    if (payload.schema !== "jet.canonical-pass-journal.v1"
      || payload.protocol !== "jet.canonical-pass.v1"
      || typeof payload.process !== "string"
      || !Array.isArray(payload.records)) {
      errors.push(`canonical process journal line ${index + 1} has invalid identity`);
      continue;
    }
    processes.push(payload.process);
    for (const record of payload.records) {
      if (!record || typeof record !== "object" || Array.isArray(record)) {
        errors.push(`canonical process journal line ${index + 1} contains a non-object record`);
        continue;
      }
      records.push({
        ...record,
        source: {
          ...(record.source ?? {}),
          process: payload.process,
        },
      });
    }
  }
  return { records, errors, processes };
}

function mergeCanonicalRoutePayload(payload, journal, stage, operations) {
  const inspectRows = payload && Array.isArray(payload.canonical_routes)
    ? payload.canonical_routes
    : [];
  const persistedEmitterRows = journal.records.filter((record) =>
    record.source?.process === "emit"
    && record.stage === stage
    && operations.has(record.operation_id)
    && record.operation_id === "mir.rust-emitter");
  const emitterRows = persistedEmitterRows;
  return {
    ...(payload && typeof payload === "object" && !Array.isArray(payload) ? payload : {}),
    canonical_routes: [
      ...inspectRows.filter((record) => record.operation_id !== "mir.rust-emitter"),
      ...emitterRows,
    ],
  };
}
function parseMirIdentity(stdout, mode) {
  const lines = stdout.split(/\r?\n/u).filter((line) => line.startsWith("// jet-mir-identity:"));
  if (lines.length === 0) return { status: "unavailable", reason: `${mode} emit produced no canonical mir-v1 identity` };
  if (lines.length !== 1) return { status: "failed", reason: `${mode} emit produced multiple canonical mir-v1 identities` };
  const canonical = lines[0].slice("// jet-mir-identity:".length).trim();
  try {
    const identity = JSON.parse(canonical);
    if (JSON.stringify(identity) !== canonical
      || identity.schema !== "mir-v1"
      || typeof identity.semantic_hash !== "string"
      || identity.semantic_hash !== identity.optimized_hash
      || typeof identity.identity_digest !== "string"
      || !Array.isArray(identity.function_ids)
      || !Array.isArray(identity.source_map)
      || !Array.isArray(identity.target_facts)) {
      return { status: "failed", reason: `${mode} emit produced malformed canonical mir-v1 identity` };
    }
    return { status: "observed", identity };
  } catch (error) {
    return { status: "failed", reason: `${mode} emit produced invalid canonical mir-v1 JSON: ${error.message}` };
  }
}

function captureCanonicalMir(caseRoot, entry, mode, environment) {
  const emitArgs = ["jet", "emit", "--rust", "--metadata", entry];
  const emitted = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", ...emitArgs], {
    cwd: caseRoot,
    env: { ...environment, JET_ADAPTER_CANONICAL_PASS_PROCESS: "emit" },
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 32 * 1024 * 1024,
  });
  const emitStdout = `${emitted.stdout ?? ""}`;
  const emitStderr = `${emitted.stderr ?? ""}`;
  const identity = emitted.status === 0 && !emitted.error
    ? parseMirIdentity(emitStdout, mode)
    : { status: "failed", reason: emitted.error?.message ?? `${mode} emit exited ${String(emitted.status)}` };
  const decisionArgs = ["jet", "inspect", "decisions", "--json", entry];
  const decisions = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", ...decisionArgs], {
    cwd: caseRoot,
    env: { ...environment, JET_ADAPTER_CANONICAL_PASS_PROCESS: "inspect" },
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 32 * 1024 * 1024,
  });
  const decisionStdout = `${decisions.stdout ?? ""}`;
  const decisionStderr = `${decisions.stderr ?? ""}`;
  let decisionPayload;
  if (decisions.status === 0 && !decisions.error) {
    try {
      decisionPayload = JSON.parse(decisionStdout);
    } catch (error) {
      decisionPayload = { status: "failed", reason: `${mode} inspect decisions produced invalid JSON: ${error.message}` };
    }
  } else {
    decisionPayload = { status: "unavailable", reason: decisions.error?.message ?? `${mode} inspect decisions exited ${String(decisions.status)}` };
  }
  const decisionObserved = decisionPayload
    && typeof decisionPayload === "object"
    && !Array.isArray(decisionPayload)
    && decisionPayload.status !== "failed"
    && decisionPayload.status !== "unavailable";
  const canonicalJournal = readCanonicalProcessJournal(environment.JET_ADAPTER_CANONICAL_PASS_PATH);
  return {
    schema: "jet.canonical-mir-capture.v1",
    status: identity.status === "observed" && decisionObserved ? "observed" : identity.status === "failed" || decisionPayload.status === "failed" ? "failed" : "unavailable",
    emit: {
      command: [RUNNER_PATH, "full", ...emitArgs],
      ...identity,
      exit_code: Number.isInteger(emitted.status) ? emitted.status : null,
      stdout_sha256: digestText(emitStdout),
      stderr_sha256: digestText(emitStderr),
    },
    decisions: {
      command: [RUNNER_PATH, "full", ...decisionArgs],
      payload: decisionPayload,
      exit_code: Number.isInteger(decisions.status) ? decisions.status : null,
      stdout_sha256: digestText(decisionStdout),
      stderr_sha256: digestText(decisionStderr),
    },
    canonical_process_journal: canonicalJournal,
  };
}

function executeMode(caseRoot, entry, stage, mode, operations, scratch) {
  const args = expectedArgs(mode, entry);
  const modeScratch = join(scratch, `${mode}-${basename(caseRoot)}`);
  const journalPath = join(modeScratch, "journal.json");
  const canonicalPath = join(modeScratch, "canonical-pass.jsonl");
  const environment = {
    ...process.env,
    JET_RUN_CACHE_DIR: join(modeScratch, "run-cache"),
    JET_DEV_ORACLE_CACHE_DIR: join(modeScratch, "oracle-cache"),
    JET_TEST_SCRATCH_DIR: scratch,
    JET_ADAPTER_MODE: mode,
    JET_ADAPTER_STAGE: stage,
    JET_ADAPTER_OPERATION_IDS: [...operations.keys()].sort().join(","),
    JET_ADAPTER_CANONICAL_PASS_PATH: canonicalPath,
    LC_ALL: "C",
    LANG: "C",
    NO_COLOR: "1",
    CLICOLOR: "0",
  };
  const irCapture = captureCanonicalMir(caseRoot, entry, mode, environment);
  const child = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", ...args], {
    cwd: caseRoot,
    env: { ...environment, JET_ADAPTER_CANONICAL_PASS_PROCESS: "runtime" },
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 32 * 1024 * 1024,
  });
  const stdout = `${child.stdout ?? ""}`;
  const stderr = `${child.stderr ?? ""}`;
  const processJournal = irCapture.canonical_process_journal;
  const canonicalPayload = mergeCanonicalRoutePayload(
    irCapture.decisions.payload,
    processJournal,
    stage,
    operations,
  );
  const canonical = parseCanonicalRoutes(canonicalPayload, stage, mode, operations);
  const journal = readComparisonJournal(journalPath, {
    mode,
    operationIds: [...operations.keys()],
    nullableFields: ["typed_failure", "mutation"],
  });
  const journalRows = [...journal.records.values()].map((record) => ({
    operation_id: record.operation_id,
    observations: record.observations,
    capture: {
      kind: "typed-comparison-observation-journal",
      path: journalPath,
      operation_id: record.operation_id,
      mode,
      record_sha256: record.record_sha256,
      universal_proof: false,
    },
  }));
  const journalRowsById = new Map(journalRows.map((row) => [row.operation_id, row]));
  const rows = canonical.rows.map((row) => ({
    ...row,
    observations: journalRowsById.get(row.operation_id)?.observations ?? {},
    ...(journalRowsById.get(row.operation_id)?.capture === undefined
      ? {}
      : { capture: journalRowsById.get(row.operation_id).capture }),
  }));
  for (const row of journalRows) {
    if (!operations.has(row.operation_id)) continue;
    if (!rows.some((candidate) => candidate.operation_id === row.operation_id)) {
      rows.push({
        operation_id: row.operation_id,
        route: {
          schema: ROUTE_SCHEMA,
          protocol: ROUTE_PROTOCOL,
          stage,
          mode,
          operation_id: row.operation_id,
          passes: [],
          coverage: { observed: false, occurrence_count: 0 },
        },
        observations: row.observations,
        capture: row.capture,
      });
    }
  }
  const errors = [...canonical.errors, ...processJournal.errors, ...journal.errors];
  const expectedExit = 0;
  const failed = Boolean(child.error)
    || child.status !== expectedExit
    || errors.length > 0
    || irCapture.status !== "observed";
  const observed = rows.some((row) => row.route?.passes?.length > 0
    || Object.keys(row.observations ?? {}).length > 0);
  return {
    mode,
    command: [RUNNER_PATH, "full", ...args],
    ir_capture: irCapture,
    status: failed ? "failed" : observed ? "observed" : "unverified",
    records: rows,
    observed_dimensions: [...new Set(rows.flatMap((row) => OBSERVATION_FIELDS.filter((field) => Object.hasOwn(row.observations, field))))],
    unknown_dimensions: OBSERVATION_FIELDS.filter((field) => !rows.some((row) => Object.hasOwn(row.observations, field))),
    exit_code: Number.isInteger(child.status) ? child.status : null,
    signal: child.signal ?? null,
    timed_out: child.error?.code === "ETIMEDOUT",
    stdout_sha256: digestText(stdout),
    stderr_sha256: digestText(stderr),
    canonical_route_count: canonicalPayload.canonical_routes?.length ?? 0,
    canonical_process_count: processJournal.processes.length,
    ...(errors.length > 0 ? { reason: errors.join("; ") } : {}),
    ...(child.error ? { reason: child.error.message, error_code: child.error.code ?? null } : {}),
    ...(child.status !== expectedExit && !child.error ? { reason: `Jet exited ${String(child.status)}; expected ${String(expectedExit)}` } : {}),
  };
}

function materializeCertificate(cert, witness) {
  return {
    ...witness.certificate_defaults,
    ...cert,
    origin: { ...witness.certificate_defaults.origin, ...cert.origin },
    legality: { ...witness.certificate_defaults.legality, ...cert.legality },
    profitability: { ...witness.certificate_defaults.profitability, ...cert.profitability },
    target_contract: { ...witness.certificate_defaults.target_contract, ...cert.target_contract },
  };
}

function identityFor(operation, witness, contract, stage) {
  const cert = witness.certificates.find((candidate) => candidate.operation_id === operation.id);
  const materialized = materializeCertificate(cert, witness);
  return {
    operation_id: operation.id,
    stage,
    source_sha256: operation.source_sha256,
    compiler_source_sha256: digestText(canonicalJson(witness.binding.authorities)),
    model_sha256: witness.binding.model.sha256,
    checker_sha256: witness.binding.checker.sha256,
    configuration_sha256: witness.binding.contract.sha256,
    target_sha256: digestText(canonicalJson(materialized.target_contract)),
    obligations_sha256: witness.binding.obligations.sha256,
    driver_sha256: witness.binding.driver.sha256,
    collector_sha256: readSource("proof/compiler/observations/comparison-journal.mjs", `${stage} observation collector`).sha256,
    contract_execution_sha256: digestText(canonicalJson(contract.execution)),
  };
}

function certificateFor(operation, modeResults, stage) {
  const execution = modeResults.map((result) => ({
    mode: result.mode,
    status: result.status,
    exit_code: result.exit_code,
    canonical_route_count: result.canonical_route_count,
    stdout_sha256: result.stdout_sha256,
    stderr_sha256: result.stderr_sha256,
  }));
  const allModesObserved = modeResults
    .filter((result) => RUNTIME_ROUTES.includes(result.mode))
    .every((result) => result.records.some((record) => record.operation_id === operation.id
      && Array.isArray(record.route?.passes)
      && record.route.passes.length > 0
      && OBSERVATION_FIELDS.every((field) => Object.hasOwn(record.observations, field))));
  return {
    schema: "jet.compiler-transformation-observation.v1",
    status: allModesObserved ? "matched" : "observed",
    universal_proof: false,
    producer: "compiler-proof observation workflow",
    observer: OBSERVER_PATHS[stage],
    observer_sha256: readSource(OBSERVER_PATHS[stage], `${stage} observer`).sha256,
    execution_id: digestText(canonicalJson({ operation_id: operation.id, execution })),
    checker: CHECKER_PATHS[stage],
    runner: RUNNER_PATH,
    method: "canonical_route_observation",
    protocol: ROUTE_PROTOCOL,
    operation_id: operation.id,
  };
}

function aggregateRecords(stage, expected, witness, contract, modeResults) {
  const byOperation = new Map();
  for (const result of modeResults) {
    for (const row of result.records) {
      const operation = expected.find((candidate) => candidate.id === row.operation_id);
      if (!operation) continue;
      const record = byOperation.get(operation.id) ?? {
        operation_id: operation.id,
        stage,
        identity: identityFor(operation, witness, contract, stage),
        identity_fields: [...OBSERVATION_IDENTITY_FIELDS],
        observation_order: [...OBSERVATION_FIELDS],
        scenarios: [...SCENARIOS],
        premises: [...BASE_PREMISES],
        routes: {},
        observations: {},
        runtime_captures: {},
      };
      record.routes[result.mode] = row.route;
      record.observations[result.mode] = row.observations;
      if (row.capture !== undefined) record.runtime_captures[result.mode] = row.capture;
      byOperation.set(operation.id, record);
    }
  }
  for (const record of byOperation.values()) {
    const operation = expected.find((candidate) => candidate.id === record.operation_id);
    record.certificate = certificateFor(operation, modeResults, stage);
  }
  return [...byOperation.values()];
}

function buildPayload(stage, fixture, expected, witness, contract, modeResults, records, compilerAvailable) {
  const complete = records.length === expected.length && records.every((record) => record.routes.check
    && record.observations.check
    && Array.isArray(record.routes.check.passes)
    && record.routes.check.passes.length > 0
    && RUNTIME_ROUTES.every((mode) => {
      const route = record.routes[mode];
      const observations = record.observations[mode];
      return route
        && Array.isArray(route.passes)
        && route.passes.length > 0
        && observations
        && OBSERVATION_FIELDS.every((field) => Object.hasOwn(observations, field));
    }));
  return {
    schema: `jet.${stage}-observations.v1`,
    schema_version: 1,
    stage,
    producer: "compiler-proof observation workflow",
    observer: OBSERVER_PATHS[stage],
    protocol: ROUTE_PROTOCOL,
    observer_sha256: readSource(OBSERVER_PATHS[stage], `${stage} observer`).sha256,
    runner: RUNNER_PATH,
    compiler: COMPILER_PATH,
    checker: CHECKER_PATHS[stage],
    target_choices: [...TARGETS],
    status: complete
      ? "matched"
      : compilerAvailable
        ? modeResults.some((result) => result.status === "failed")
          ? "failed"
          : records.length > 0 ? "observed" : "unverified"
        : "unavailable",
    fixture,
    fixture_sha256: readSource(fixture, "observation fixture").sha256,
    identity_fields: [...OBSERVATION_IDENTITY_FIELDS],
    observation_fields: [...OBSERVATION_FIELDS],
    operation_ids: expected.map((operation) => operation.id),
    records,
    modes: [...REPLAY_ROUTES],
    mode_results: modeResults,
    missing_operations: expected.map((operation) => operation.id).filter((id) => !records.some((record) => record.operation_id === id)),
    universal_proof: false,
    formal: { status: "unavailable", path: `proof/compiler/${stage}/discharge.json` },
  };
}

async function produceFormal(options, payload, expected, stage) {
  if (!options.formal) return null;
  if (!options.operation || !options.claimId) fail("--formal requires --operation and --claim-id");
  const record = payload.records.find((candidate) => candidate.operation_id === options.operation);
  const operation = expected.find((candidate) => candidate.id === options.operation);
  if (!record || !operation) fail(`formal operation is not observed: ${options.operation}`);
  const complete = Boolean(
    record.routes.check
      && record.observations.check
      && Array.isArray(record.routes.check.passes)
      && record.routes.check.passes.length > 0,
  ) && RUNTIME_ROUTES.every((mode) => {
    const route = record.routes[mode];
    const observations = record.observations[mode];
    return route
      && Array.isArray(route.passes)
      && route.passes.length > 0
      && observations
      && OBSERVATION_FIELDS.every((field) => Object.hasOwn(observations, field));
  });
  if (!complete) fail(`formal operation is missing complete route observations: ${options.operation}`);
  const replayAuthority = {
    authority_key: `compiler-proof.${stage}`,
    obligation_id: operation.id,
  };
  const replayReport = replay(options.replayPath, options.claimId, replayAuthority);
  const canonicalGoal = replayReport.authority;
  const discharge = {
    schema: `jet.${stage}-formal-discharge.v1`,
    schema_version: 1,
    stage,
    producer: "compiler-proof observation workflow",
    universal_proof: false,
    records: [{
      operation_id: operation.id,
      canonical_goal: canonicalGoal,
      authority: replayAuthority,
      goal_sha256: digestText(canonicalJson(canonicalGoal)),
      replay_ref: { path: options.replayPath, claim_id: options.claimId },
      replay: replayReport,
    }],
  };
  const outputPath = projectPath(options.dischargeOutput, "formal discharge output");
  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(discharge, null, 2)}\n`, "utf8");
  return discharge;
}

function produce(options) {
  const stage = options.stage;
  const fixture = options.fixture;
  const fixtureAbsolute = projectPath(fixture, "fixture");
  const contract = readJson(CONTRACT_PATHS[stage], `${stage} contract`);
  const witness = readJson(WITNESS_PATHS[stage], `${stage} witnesses`);
  const expected = operationRegistry(stage);
  const scratch = safeScratch(options.scratch, stage);
  const caseRoot = mkdtempSync(join(scratch, `${stage}-observe-`));
  const entry = basename(fixtureAbsolute);
  const compilerAbsolute = projectPath(COMPILER_PATH, "compiler");
  let compilerAvailable = existsSync(compilerAbsolute);
  if (compilerAvailable) {
    try { accessSync(compilerAbsolute, fsConstants.X_OK); } catch { compilerAvailable = false; }
  }
  try {
    cpSync(fixtureAbsolute, join(caseRoot, entry));
    const operationMap = new Map(expected.map((operation) => [operation.id, operation]));
    const modeResults = compilerAvailable
      ? REPLAY_ROUTES.map((mode) => executeMode(caseRoot, entry, stage, mode, operationMap, scratch))
      : REPLAY_ROUTES.map((mode) => ({ mode, status: "unavailable", command: [RUNNER_PATH, "full", ...expectedArgs(mode, entry)], records: [], observed_dimensions: [], unknown_dimensions: [...OBSERVATION_FIELDS], reason: "target/debug/jet is missing or not executable" }));
    const records = aggregateRecords(stage, expected, witness, contract, modeResults);
    return { payload: buildPayload(stage, fixture, expected, witness, contract, modeResults, records, compilerAvailable), expected };
  } finally {
    rmSync(caseRoot, { recursive: true, force: true });
  }
}

function usage() {
  return "Usage: observe.mjs [--json] [--stage lowering|optimization] [--fixture PATH] [--output PATH] [--scratch PATH] [--formal --operation ID --claim-id ID]";
}

export async function main(argv = process.argv.slice(2)) {
  try {
    const options = parseArgs(argv);
    if (options.help) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    const { payload, expected } = produce(options);
    const formal = await produceFormal(options, payload, expected, options.stage);
    if (formal) payload.formal = { status: "supplied", path: options.dischargeOutput };
    const outputPath = projectPath(options.output, "observation output");
    mkdirSync(dirname(outputPath), { recursive: true });
    writeFileSync(outputPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
    if (options.json) process.stdout.write(`${canonicalJson({ ...payload, ...(formal ? { formal } : {}) })}\n`);
    else process.stdout.write(`${options.stage} observations: ${payload.status}; records=${payload.records.length}\n`);
    return 0;
  } catch (error) {
    const result = {
      schema: `jet.${parseArgsSafeStage(process.argv.slice(2))}-observations.v1`,
      schema_version: 1,
      stage: parseArgsSafeStage(process.argv.slice(2)),
      status: "unavailable",
      error: { message: error.message, ...(error.details === undefined ? {} : { details: error.details }) },
      universal_proof: false,
    };
    if (process.argv.includes("--json")) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stderr.write(`${result.error.message}\n`);
    return 3;
  }
}

function parseArgsSafeStage(argv) {
  const index = argv.indexOf("--stage");
  return index >= 0 && argv[index + 1] === "optimization" ? "optimization" : "lowering";
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = await main();

export {
  OBSERVATION_FIELDS,
  OBSERVATION_IDENTITY_FIELDS,
  produce,
  parseCanonicalRoutes,
};
