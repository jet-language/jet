#!/usr/bin/env node

/**
 * Future positive-observation producer for compiler adapters (#2941).
 *
 * This producer is the only stage allowed to create positive adapter observation
 * certificates. It asks the source-bound adapter checker for the current
 * operation identities, runs the supplied Jet program through every selected
 * applicable route, and accepts only explicit observation markers emitted by
 * that program. A marker is finite evidence for one operation; it is never a
 * semantic proof and never widens applicability.
 */

import { accessSync, constants as fsConstants, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { createServer } from "node:net";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalJson, digestBytes, digestText } from "../../../scripts/agent/compiler-proof.mjs";
import { readComparisonJournal, writeComparisonCorpus } from "../observations/comparison-journal.mjs";
import { CdpDriver } from "../../../scripts/canvas-test/driver.mjs";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const STAGE = "adapters";
const CHECKER_PATH = "proof/compiler/adapters/check.mjs";
const OBSERVER_PATH = "proof/compiler/adapters/observe.mjs";
const OBSERVATIONS_PATH = "proof/compiler/adapters/observations.json";
const RUNNER_PATH = "scripts/agent/jet-env";
const COMPILER_PATH = "target/debug/jet";
const DEFAULT_SCRATCH = join(homedir(), ".cache", "jet-test-scratch");
const MARKER = "JET_ADAPTER_OBSERVATION_V1:";
const TIMEOUT_MS = 120000;
const MODES = Object.freeze(["aot", "jet_run", "interpreter", "web", "comptime"]);
const OBSERVATION_FIELDS = Object.freeze([
  "result",
  "typed_failure",
  "mutation",
  "input_consumption",
  "ordered_effects",
  "cleanup",
  "resource_premises",
  "schedule",
  "allowed_schedules",
]);
const ADAPTER_DIMENSIONS = Object.freeze([
  "dispatch",
  "control_transfer",
  "calling_convention",
  "representation",
  "width",
  "layout",
  "handles",
  "callbacks",
  "results",
  "errors",
  "cleanup",
  "task_completion",
  "source_identity",
  "target_identity",
  "tool_configuration",
]);
const IDENTITY_FIELDS = Object.freeze([
  "program_sha256",
  "source_sha256",
  "shared_operation_sha256",
  "adapter_artifact_sha256",
  "target_sha256",
  "tool_configuration_sha256",
  "checker_sha256",
  "obligations_sha256",
]);

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
  if (normalized.startsWith("/") || normalized.split("/").includes("..")) {
    fail(`${label} must stay within the project`, { path: value });
  }
  const absolute = resolve(ROOT, normalized);
  if (absolute !== ROOT && !absolute.startsWith(`${ROOT}${sep}`)) fail(`${label} escapes the project`, { path: value });
  return normalized;
}

function absoluteProjectPath(value, label, { file = false } = {}) {
  const project = projectPath(value, label);
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail(`${label} is missing: ${project}`);
  if (file) {
    if (!statSync(absolute).isFile()) fail(`${label} must be a file: ${project}`);
    try {
      accessSync(absolute, fsConstants.R_OK);
    } catch (error) {
      fail(`${label} is not readable: ${project}`, { error: String(error) });
    }
  }
  return { project, absolute };
}

function digestFile(path, label = "file") {
  const { absolute } = absoluteProjectPath(path, label, { file: true });
  return digestBytes(readFileSync(absolute));
}

function digestValue(value) {
  return digestText(canonicalJson(value));
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

function parseList(value, label) {
  const values = value.split(",").map((item) => item.trim()).filter((item) => item.length > 0);
  if (values.length === 0) fail(`${label} must contain at least one value`);
  return values;
}

function parseArgs(argv) {
  const options = {
    json: false,
    program: undefined,
    operations: [],
    modes: undefined,
    output: OBSERVATIONS_PATH,
    scratch: undefined,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") options.json = true;
    else if (arg === "--program" || arg === "--operation" || arg === "--operations" || arg === "--mode" || arg === "--output" || arg === "--scratch") {
      if (!argv[index + 1] || argv[index + 1].startsWith("--")) fail(`${arg} requires a value`);
      const value = argv[++index];
      if (arg === "--program") options.program = value;
      else if (arg === "--operation" || arg === "--operations") options.operations.push(...parseList(value, arg));
      else if (arg === "--mode") options.modes = [...(options.modes ?? []), ...parseList(value, arg)];
      else options[arg.slice(2)] = value;
    } else if (arg === "--help" || arg === "-h") options.help = true;
    else fail(`unknown argument ${arg}`);
  }
  if (options.help) return options;
  if (!options.program) fail("--program is required");
  if (options.operations.length === 0) fail("at least one --operation is required");
  if (options.modes && options.modes.some((mode) => !MODES.includes(mode))) {
    fail("--mode names an unsupported adapter mode", { modes: options.modes, supported: MODES });
  }
  options.operations = [...new Set(options.operations)];
  options.modes = options.modes ? [...new Set(options.modes)] : undefined;
  return options;
}

function expectedArgs(mode, entry) {
  if (mode === "aot") return ["jet", "run", "--release", entry];
  if (mode === "jet_run") return ["jet", "run", entry];
  if (mode === "interpreter") return ["jet", "run", "--interpret", entry];
  if (mode === "comptime") return ["jet", "run", entry];
  if (mode === "web") return ["jet", "dev", entry, "--target=web"];
  fail(`unsupported adapter observation mode ${mode}`);
}

function markerObservation(value, operationId, mode) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return { error: `${mode} marker for ${operationId} is not an object` };
  }
  const nullableWhenAbsent = new Set(["typed_failure", "mutation"]);
  const missing = OBSERVATION_FIELDS.filter((field) => !Object.hasOwn(value, field)
    || value[field] === undefined
    || (value[field] === null && !nullableWhenAbsent.has(field)));
  if (missing.length > 0) return { error: `${mode} marker for ${operationId} omits fields: ${missing.join(", ")}` };
  return { observations: value };
}

function parseMarkers(stdout, operationIds, mode) {
  const lines = stdout.split(/\r?\n/u);
  const markerLines = lines.filter((line) => line.startsWith(MARKER));
  if (markerLines.length === 0) return { records: new Map(), marker_count: 0 };
  const records = new Map();
  for (const line of markerLines) {
    let payload;
    try {
      payload = JSON.parse(line.slice(MARKER.length));
    } catch (error) {
      return { error: `${mode} emitted invalid observation JSON: ${error.message}`, marker_count: markerLines.length };
    }
    if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
      return { error: `${mode} observation marker is not an object`, marker_count: markerLines.length };
    }
    const operationId = payload.operation_id;
    if (typeof operationId !== "string" || operationId.length === 0) {
      return { error: `${mode} observation marker must name operation_id`, marker_count: markerLines.length };
    }
    if (!operationIds.has(operationId)) {
      return { error: `${mode} observation marker names an operation not requested by this run: ${operationId}`, marker_count: markerLines.length };
    }
    if (records.has(operationId)) return { error: `${mode} emitted duplicate observation marker for ${operationId}`, marker_count: markerLines.length };
    const value = payload.observations ?? Object.fromEntries(OBSERVATION_FIELDS.map((field) => [field, payload[field]]));
    const checked = markerObservation(value, operationId, mode);
    if (checked.error) return { error: checked.error, marker_count: markerLines.length };
    records.set(operationId, { observations: checked.observations, marker_sha256: digestValue(payload) });
  }
  return { records, marker_count: markerLines.length };
}
function parseJournal(path, operationIds, mode) {
  const parsed = readComparisonJournal(path, { mode, operationIds });
  const records = new Map([...parsed.records.entries()].map(([operationId, record]) => [
    operationId,
    {
      observations: record.observations,
      marker_sha256: record.record_sha256,
      capture: {
        kind: "typed-comparison-observation-journal",
        path,
        operation_id: operationId,
        mode,
        universal_proof: false,
      },
    },
  ]));
  return {
    records,
    journal_count: parsed.journal_count,
    ...(parsed.errors.length > 0 ? { error: parsed.errors.join("; ") } : {}),
  };
}

function publicCaptureObservation(stdout, child, mode, operationId) {
  const outputLines = stdout.split(/\r?\n/u).filter((line) => line.length > 0);
  const lifecycle = [
    `process:${mode}:started`,
    `process:${mode}:exited:${String(child.status)}`,
  ];
  const unavailable = (field) => ({
    status: "unavailable",
    reason: `the public ${mode} runner does not expose ${field} for this capture`,
  });
  const observations = {
    result: outputLines.length > 0
      ? { status: "observed", value: outputLines.join("\n") }
      : unavailable("result"),
    typed_failure: unavailable("typed_failure"),
    mutation: unavailable("mutation"),
    input_consumption: unavailable("input_consumption"),
    ordered_effects: outputLines.length > 0
      ? outputLines.map((line) => `stdout:${line}`)
      : unavailable("ordered_effects"),
    cleanup: lifecycle,
    resource_premises: unavailable("resource_premises"),
    schedule: lifecycle,
    allowed_schedules: unavailable("allowed_schedules"),
  };
  return {
    observations,
    marker_sha256: digestValue({ mode, operation_id: operationId, observations }),
    capture: {
      kind: "public-runner-comparison-observation",
      operation_id: operationId,
      mode,
      stdout_sha256: digestText(stdout),
      exit_code: child.status,
      lifecycle,
      unavailable_dimensions: OBSERVATION_FIELDS.filter((field) => observations[field]?.status === "unavailable"),
      universal_proof: false,
    },
  };
}

function unavailableDimensions(observations) {
  const nullableWhenAbsent = new Set(["typed_failure", "mutation"]);
  return OBSERVATION_FIELDS.filter((field) => {
    const value = observations?.[field];
    return value === undefined
      || (value === null && !nullableWhenAbsent.has(field))
      || (value && typeof value === "object" && value.status === "unavailable");
  });
}


function parsedRecords(parsed, journal, child, stdout, mode, knownOperationIds) {
  if (parsed.marker_count !== 0) return parsed.records ?? new Map();
  if (journal.journal_count !== 0 || journal.records.size !== 0) return journal.records;
  if (child.status !== 0 || knownOperationIds.size !== 1) return parsed.records;
  const operationId = [...knownOperationIds][0];
  const capture = publicCaptureObservation(stdout, child, mode, operationId);
  return new Map([[operationId, capture]]);
}
function reservePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address !== null ? address.port : null;
      server.close((error) => error ? reject(error) : resolvePort(port));
    });
  });
}

async function waitForWebServer(port) {
  const deadline = Date.now() + TIMEOUT_MS;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/`, { cache: "no-store" });
      if (response.status === 200) {
        await response.text();
        return;
      }
      lastError = new Error(`web server returned ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 80));
  }
  throw lastError ?? new Error("web server did not become ready");
}

async function stopWebServer(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  await new Promise((resolveExit) => {
    const timer = setTimeout(() => {
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      resolveExit();
    }, 2000);
    child.once("close", () => {
      clearTimeout(timer);
      resolveExit();
    });
  });
}

async function executeWebMode(program, knownOperationIds, requiredOperationIds, cacheRoot) {
  const mode = "web";
  const port = await reservePort();
  const command = [RUNNER_PATH, "full", "jet", "dev", program, "--target=web", `--port=${port}`];
  const journalPath = join(cacheRoot, mode, "journal.json");
  mkdirSync(join(cacheRoot, mode), { recursive: true });
  const environment = {
    ...process.env,
    JET_ADAPTER_MODE: mode,
    JET_ADAPTER_OPERATION_IDS: [...knownOperationIds].sort().join(","),
    JET_ADAPTER_OBSERVATION_PATH: journalPath,
    JET_RUN_CACHE_DIR: join(cacheRoot, mode, "run"),
    JET_DEV_ORACLE_CACHE_DIR: join(cacheRoot, mode, "oracle"),
    JET_TEST_SCRATCH_DIR: cacheRoot,
    LC_ALL: "C",
    LANG: "C",
    NO_COLOR: "1",
    CLICOLOR: "0",
  };
  const child = spawn(resolve(ROOT, RUNNER_PATH), ["full", "jet", "dev", program, "--target=web", `--port=${port}`], {
    cwd: ROOT,
    env: environment,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let serverStdout = "";
  let serverStderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => { serverStdout += chunk; });
  child.stderr.on("data", (chunk) => { serverStderr += chunk; });
  const browser = new CdpDriver({ chromeTempRoot: cacheRoot });
  let browserOutput = "";
  let browserError;
  try {
    await waitForWebServer(port);
    await browser.launch();
    await browser.send("Page.addScriptToEvaluateOnNewDocument", {
      source: "window.__jetAdapterOutput=[]; for (const method of ['log','error','warn']) { const original=console[method]; console[method]=(...args)=>{ window.__jetAdapterOutput.push(args.map((value)=>String(value)).join(' ')); original.apply(console,args); }; }",
    }, browser.pageSession);
    await browser.navigate(`http://127.0.0.1:${port}/`);
    const deadline = Date.now() + TIMEOUT_MS;
    while (Date.now() < deadline) {
      const lines = await browser.evaluate("window.__jetAdapterOutput || []");
      browserOutput = Array.isArray(lines) ? lines.join("\\n") : "";
      if (browserOutput.length > 0 || existsSync(journalPath)) break;
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 80));
    }
  } catch (error) {
    browserError = error;
  } finally {
    await browser.close().catch((error) => { browserError ??= error; });
    await stopWebServer(child);
  }
  const childFacts = {
    status: browserError ? 1 : 0,
    signal: null,
    error: browserError,
  };
  const parsed = parseMarkers(browserOutput, knownOperationIds, mode);
  const journal = parsed.marker_count === 0 ? parseJournal(journalPath, knownOperationIds, mode) : { records: new Map(), journal_count: 0 };
  const records = parsedRecords(parsed, journal, childFacts, browserOutput, mode, knownOperationIds);
  const errors = parsed.error ?? journal.error;
  let status = "unverified";
  let reason;
  if (browserError) {
    status = "unavailable";
    reason = browserError.message;
  } else if (errors) {
    status = "failed";
    reason = errors;
  } else if (records.size === 0) {
    reason = "web runner emitted no browser observation and no typed comparison journal";
  } else if ([...requiredOperationIds].some((operationId) => !records.has(operationId))) {
    reason = "web runner did not emit a record for every applicable requested operation";
  } else {
    status = "observed";
  }
  const stdout = `${serverStdout}${browserOutput}`;
  return {
    mode,
    command,
    status,
    observations: Object.fromEntries([...records.entries()].map(([id, value]) => [id, value.observations])),
    marker_sha256: Object.fromEntries([...records.entries()].map(([id, value]) => [id, value.marker_sha256])),
    observed_operation_ids: [...records.keys()].sort(),
    marker_count: parsed.marker_count ?? 0,
    journal_count: journal.journal_count ?? 0,
    captures: Object.fromEntries([...records.entries()].filter(([, value]) => value.capture).map(([id, value]) => [id, {
      ...value.capture,
      kind: "headless-browser-web-observation",
      port,
      browser_output_sha256: digestText(browserOutput),
    }])),
    unavailable_dimensions: Object.fromEntries([...records.entries()].map(([id, value]) => [id, unavailableDimensions(value.observations)])),
    exit_code: browserError ? 1 : 0,
    signal: null,
    timed_out: false,
    stdout_sha256: digestText(stdout),
    stderr_sha256: digestText(serverStderr),
    ...(reason ? { reason } : {}),
    ...(browserError?.code ? { error_code: browserError.code } : {}),
  };
}

function executeMode(mode, program, knownOperationIds, requiredOperationIds, cacheRoot) {
  const command = [RUNNER_PATH, "full", ...expectedArgs(mode, program)];
  const journalPath = join(cacheRoot, mode, "journal.json");
  mkdirSync(join(cacheRoot, mode), { recursive: true });
  const environment = {
    ...process.env,
    JET_ADAPTER_MODE: mode,
    JET_ADAPTER_OPERATION_IDS: [...knownOperationIds].sort().join(","),
    JET_ADAPTER_OBSERVATION_PATH: journalPath,
    JET_RUN_CACHE_DIR: join(cacheRoot, mode, "run"),
    JET_DEV_ORACLE_CACHE_DIR: join(cacheRoot, mode, "oracle"),
    JET_TEST_SCRATCH_DIR: cacheRoot,
    LC_ALL: "C",
    LANG: "C",
    NO_COLOR: "1",
    CLICOLOR: "0",
  };
  const child = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", ...expectedArgs(mode, program)], {
    cwd: ROOT,
    env: environment,
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 16 * 1024 * 1024,
  });
  const stdout = `${child.stdout ?? ""}`;
  const stderr = `${child.stderr ?? ""}`;
  const parsed = parseMarkers(stdout, knownOperationIds, mode);
  const journal = parsed.marker_count === 0 ? parseJournal(journalPath, knownOperationIds, mode) : { records: new Map(), journal_count: 0 };
  const records = parsedRecords(parsed, journal, child, stdout, mode, knownOperationIds);
  let status = "unverified";
  let reason;
  if (child.error?.code === "ETIMEDOUT") {
    status = "timeout";
    reason = `${mode} execution exceeded ${TIMEOUT_MS}ms`;
  } else if (child.error) {
    status = "unavailable";
    reason = child.error.message;
  } else if (child.status !== 0) {
    status = "failed";
  } else if (parsed.error || journal.error) {
    status = "failed";
    reason = parsed.error ?? journal.error;
  } else if (records.size === 0) {
    reason = `${mode} emitted no explicit observation and has no single selected operation for public capture`;
  } else if ([...requiredOperationIds].some((operationId) => !records.has(operationId))) {
    reason = `${mode} did not emit a marker for every applicable requested operation`;
  } else {
    status = "observed";
  }
  return {
    mode,
    command,
    status,
    observations: Object.fromEntries([...records.entries()].map(([id, value]) => [id, value.observations])),
    marker_sha256: Object.fromEntries([...records.entries()].map(([id, value]) => [id, value.marker_sha256])),
    observed_operation_ids: [...records.keys()].sort(),
    marker_count: parsed.marker_count ?? 0,
    journal_count: journal.journal_count ?? 0,
    captures: Object.fromEntries([...records.entries()].filter(([, value]) => value.capture).map(([id, value]) => [id, value.capture])),
    unavailable_dimensions: Object.fromEntries([...records.entries()].map(([id, value]) => [id, unavailableDimensions(value.observations)])),
    exit_code: Number.isInteger(child.status) ? child.status : null,
    signal: child.signal ?? null,
    timed_out: child.error?.code === "ETIMEDOUT",
    stdout_sha256: digestText(stdout),
    stderr_sha256: digestText(stderr),
    ...(reason ? { reason } : {}),
    ...(child.error?.code ? { error_code: child.error.code } : {}),
  };
}

function runChecker() {
  const command = [process.execPath, resolve(ROOT, CHECKER_PATH), "--json"];
  const child = spawnSync(process.execPath, [resolve(ROOT, CHECKER_PATH), "--json"], {
    cwd: ROOT,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 32 * 1024 * 1024,
  });
  const stdout = `${child.stdout ?? ""}`.trim();
  if (child.error?.code === "ETIMEDOUT") fail("checker timed out before identity rows were available", { command, timeout_ms: TIMEOUT_MS });
  if (!stdout) fail("adapter checker produced no JSON identity rows", { command, stderr: `${child.stderr ?? ""}`, exit_code: child.status });
  let result;
  try {
    result = JSON.parse(stdout);
  } catch (error) {
    fail("adapter checker output is not JSON", { command, error: String(error), stdout });
  }
  if (!Array.isArray(result.obligations?.rows)) fail("adapter checker did not return identity-bound obligation rows", { command, checker_status: result.status });
  return { result, command, exit_code: Number.isInteger(child.status) ? child.status : null, stdout_sha256: digestText(stdout) };
}

function operationRows(checker, operationIds) {
  const rows = new Map((checker.result.obligations.rows ?? []).filter((row) => typeof row?.id === "string").map((row) => [row.id, row]));
  const selected = new Map();
  for (const operationId of operationIds) {
    const row = rows.get(operationId);
    if (!row) fail(`requested adapter operation is absent from the checked denominator: ${operationId}`);
    const capabilities = row.implementation?.capabilities ?? (row.implementation?.capability ? [row.implementation.capability] : []);
    const applicable = capabilities.filter((capability) => capability?.required && capability.applicable && capability.bound && ["implemented-unqualified", "passed"].includes(capability.disposition) && MODES.includes(capability.mode));
    selected.set(operationId, { row, modes: [...new Set(applicable.map((capability) => capability.mode))].sort() });
  }
  return selected;
}

function identityFor(row, mode) {
  const identity = row.implementation?.identity;
  const value = identity?.[mode] && typeof identity[mode] === "object" ? identity[mode] : identity;
  if (value === null || typeof value !== "object" || Array.isArray(value)) return null;
  if (IDENTITY_FIELDS.some((field) => typeof value[field] !== "string" || value[field].length === 0)) return null;
  return value;
}


function canonicalCompare(operationId, modes, modeResults, cacheRoot) {
  const referenceMode = modes[0];
  const comparisons = [];
  let status = "matched";
  let reason;
  for (const candidateMode of modes.slice(1)) {
    const reference = modeResults[referenceMode]?.observations?.[operationId];
    const candidate = modeResults[candidateMode]?.observations?.[operationId];
    if (!reference || !candidate) {
      status = "unavailable";
      reason ??= `${operationId} lacks observations for canonical comparison ${referenceMode}->${candidateMode}`;
      comparisons.push({ reference_mode: referenceMode, candidate_mode: candidateMode, status: "unavailable", reason });
      continue;
    }
    const corpusPath = join(cacheRoot, `adapter-compare-${digestValue({ operationId, referenceMode, candidateMode })}.json`);
    let corpus;
    try {
      corpus = writeComparisonCorpus(corpusPath, { operationId, referenceMode, candidateMode, reference, candidate, source: `adapter:${operationId}` });
      const child = spawnSync(resolve(ROOT, RUNNER_PATH), ["full", "jet", "test-compare", corpusPath, "--json"], {
        cwd: ROOT,
        env: {
          ...process.env,
          JET_TEST_SCRATCH_DIR: cacheRoot,
          JET_RUN_CACHE_DIR: join(cacheRoot, "compare-run-cache"),
          JET_DEV_ORACLE_CACHE_DIR: join(cacheRoot, "compare-oracle-cache"),
          LC_ALL: "C",
          LANG: "C",
          NO_COLOR: "1",
          CLICOLOR: "0",
        },
        encoding: "utf8",
        timeout: TIMEOUT_MS,
        maxBuffer: 16 * 1024 * 1024,
      });
      const output = `${child.stdout ?? ""}`.trim().split(/\r?\n/u).filter(Boolean).at(-1);
      const report = output ? JSON.parse(output) : null;
      const pairStatus = child.error?.code === "ETIMEDOUT"
        ? "timeout"
        : report?.status === "mismatch"
          ? "mismatch"
          : child.error || child.status !== 0
            ? "unavailable"
            : report?.status === "matched"
              ? "matched"
              : "unavailable";
      comparisons.push({
        reference_mode: referenceMode,
        candidate_mode: candidateMode,
        status: pairStatus,
        corpus: corpusPath,
        corpus_sha256: digestFile(corpusPath, "comparison corpus"),
        command: [RUNNER_PATH, "full", "jet", "test-compare", corpusPath, "--json"],
        exit_code: Number.isInteger(child.status) ? child.status : null,
        report,
        universal_proof: false,
      });
      if (pairStatus !== "matched") {
        status = pairStatus === "mismatch" ? "mismatch" : "unavailable";
        reason ??= report?.reason ?? `${referenceMode}->${candidateMode} canonical comparison returned ${pairStatus}`;
      }
    } catch (error) {
      status = "unavailable";
      reason ??= `${referenceMode}->${candidateMode} canonical comparison unavailable: ${error.message}`;
      comparisons.push({ reference_mode: referenceMode, candidate_mode: candidateMode, status: "unavailable", corpus: corpusPath, reason, universal_proof: false });
    }
  }
  return { status: modes.length > 0 ? status : "unavailable", reason, comparisons, universal_proof: false };
}

function compareObservations(selected, modeResults, options) {
  const comparisons = [];
  const records = [];
  let allMatched = true;
  for (const [operationId, selection] of selected.entries()) {
    const modes = options.modes ?? selection.modes;
    let status = modes.length === selection.modes.length && modes.every((mode) => selection.modes.includes(mode))
      ? "matched"
      : "unavailable";
    let reason = status === "unavailable" ? `${operationId} observation selection does not cover every checked applicable mode` : undefined;
    const modeComparisons = [];
    for (const mode of modes) {
      if (!selection.modes.includes(mode)) {
        status = "unavailable";
        reason ??= `${operationId}@${mode} is not an applicable checked adapter mode`;
        modeComparisons.push({ mode, status: "unavailable", reason });
        continue;
      }
      const result = modeResults[mode];
      const observations = result?.observations?.[operationId];
      const missing = unavailableDimensions(observations);
      if (!result || result.status !== "observed" || !observations || missing.length > 0) {
        status = "unavailable";
        reason ??= result?.reason ?? `${mode} has unavailable adapter observation dimensions for ${operationId}: ${missing.join(", ")}`;
        modeComparisons.push({ mode, status: result?.status ?? "unavailable", unavailable_dimensions: missing, reason });
        continue;
      }
      modeComparisons.push({ mode, status: "observed", observations_sha256: digestValue(observations), unavailable_dimensions: [] });
    }
    if (status === "matched") {
      const canonical = canonicalCompare(operationId, modes, modeResults, options.scratchRoot);
      status = canonical.status;
      reason ??= canonical.reason;
      comparisons.push({ operation_id: operationId, modes, status, ...(reason ? { reason } : {}), mode_results: modeComparisons, canonical, universal_proof: false });
    } else {
      comparisons.push({ operation_id: operationId, modes, status, ...(reason ? { reason } : {}), mode_results: modeComparisons, universal_proof: false });
    }
    if (status !== "matched") {
      allMatched = false;
      continue;
    }
    const operationRecords = [];
    for (const mode of modes) {
      const result = modeResults[mode];
      const identity = identityFor(selection.row, mode);
      const observations = result.observations[operationId];
      const certificate = {
        schema: "jet.adapter-observation-certificate.v1",
        status: "verified",
        universal_proof: false,
        producer: OBSERVER_PATH,
        execution_id: digestValue({ operation_id: operationId, mode, program: options.program, program_sha256: options.program_sha256, command: result.command, exit_code: result.exit_code, stdout_sha256: result.stdout_sha256, stderr_sha256: result.stderr_sha256, observations }),
        checker: CHECKER_PATH,
        runner: RUNNER_PATH,
        command: result.command,
        exit_code: result.exit_code,
        stdout_sha256: result.stdout_sha256,
        stderr_sha256: result.stderr_sha256,
        observation_sha256: digestValue(observations),
        identity_sha256: digestValue(identity),
      };
      operationRecords.push({ operation_id: operationId, mode, identity, identity_fields: [...IDENTITY_FIELDS], dimensions: [...ADAPTER_DIMENSIONS], observations, certificate });
    }
    records.push(...operationRecords);
  }
  return { comparisons, records, allMatched };
}

async function produce(options) {
  const { project: program } = absoluteProjectPath(options.program, "program", { file: true });
  const scratch = safeScratch(options.scratch);
  const scratchRoot = mkdtempSync(join(scratch, "adapter-observe-"));
  try {
    const checker = runChecker();
    const selected = operationRows(checker, options.operations);
    const selectedModes = options.modes ?? [...new Set([...selected.values()].flatMap((selection) => selection.modes))].sort();
    const program_sha256 = digestFile(program, "program");
    const operationIds = new Set(selected.keys());
    const requiredByMode = new Map(MODES.map((mode) => [
      mode,
      new Set([...selected.entries()].filter(([, selection]) => selection.modes.includes(mode)).map(([operationId]) => operationId)),
    ]));
    const modeResults = {};
    for (const mode of MODES) {
      if (!selectedModes.includes(mode) || requiredByMode.get(mode).size === 0) {
        modeResults[mode] = { mode, status: "unrequested", command: [RUNNER_PATH, "full", ...expectedArgs(mode, program)], observations: {}, observed_operation_ids: [], marker_count: 0, universal_proof: false };
      } else {
        modeResults[mode] = mode === "web"
          ? await executeWebMode(program, operationIds, requiredByMode.get(mode), scratchRoot)
          : executeMode(mode, program, operationIds, requiredByMode.get(mode), scratchRoot);
      }
    }
    const comparison = compareObservations(selected, modeResults, { ...options, program, program_sha256, scratchRoot });
    const status = comparison.allMatched ? "verified" : comparison.comparisons.some((entry) => entry.status === "mismatch") ? "mismatch" : modeResults && Object.values(modeResults).some((result) => ["failed", "timeout"].includes(result.status)) ? "failed" : "unavailable";
    const payload = {
      schema: "jet.adapters-observations.v1",
      schema_version: 1,
      stage: STAGE,
      producer: OBSERVER_PATH,
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      program,
      program_sha256,
      checker: {
        path: CHECKER_PATH,
        command: checker.command,
        exit_code: checker.exit_code,
        stdout_sha256: checker.stdout_sha256,
        status: checker.result.status ?? "unknown",
      },
      operation_ids: options.operations,
      modes: selectedModes,
      identity_fields: [...IDENTITY_FIELDS],
      observation_fields: [...OBSERVATION_FIELDS],
      dimensions: [...ADAPTER_DIMENSIONS],
      status,
      records: comparison.records,
      comparisons: comparison.comparisons,
      execution: {
        scratch: "$HOME/.cache/jet-test-scratch",
        timeout_ms: TIMEOUT_MS,
        command_policy: "scripts/agent/jet-env full; aot, jet_run, interpreter, and comptime execute independently; comptime uses a real jet run so @ bindings in the supplied program execute at compile time; web starts jet dev and captures the served page through CdpDriver; production may emit a typed comparison observation journal at JET_ADAPTER_OBSERVATION_PATH, otherwise public stdout/lifecycle capture remains explicitly dimension-limited; no dimension is synthesized",
      },
      universal_proof: false,
    };
    const outputPath = resolve(ROOT, projectPath(options.output, "observation output"));
    mkdirSync(dirname(outputPath), { recursive: true });
    writeFileSync(outputPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
    return payload;
  } finally {
    rmSync(scratchRoot, { recursive: true, force: true });
  }
}

function usage() {
  return "Usage: observe.mjs --program PATH --operation ID[,ID...] [--mode aot,jet_run,interpreter,web,comptime] [--output PATH] [--scratch PATH] [--json]";
}

export async function main(argv = process.argv.slice(2)) {
  try {
    const options = parseArgs(argv);
    if (options.help) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    const result = await produce(options);
    if (options.json) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stdout.write(`adapter observations: ${result.status}; records=${result.records.length}\n`);
    return result.status === "verified" ? 0 : 3;
  } catch (error) {
    const result = {
      schema: "jet.adapters-observations.v1",
      schema_version: 1,
      stage: STAGE,
      producer: OBSERVER_PATH,
      status: "unavailable",
      error: { message: error.message, ...(error.details === undefined ? {} : { details: error.details }) },
      records: [],
      universal_proof: false,
    };
    if (process.argv.includes("--json")) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stderr.write(`${result.error.message}\n`);
    return 3;
  }
}

if (process.argv[1] && import.meta.url === new URL(process.argv[1], "file:").href) process.exitCode = await main();

export { ADAPTER_DIMENSIONS, IDENTITY_FIELDS, MODES, OBSERVATION_FIELDS, produce };
