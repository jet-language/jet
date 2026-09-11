#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import { cpus, homedir, hostname, release, totalmem } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_DIR = path.resolve(SCRIPT_DIR, "../..");
const DEFAULT_MANIFEST = path.join(REPO_DIR, "tools/agent-eval/dx/manifest.json");
const DEFAULT_ADAPTERS = path.join(REPO_DIR, "tools/agent-eval/adapters.json");
const DEFAULT_CAPSULE = path.join(REPO_DIR, "tools/agent-eval/jet-context-capsule.md");
const DEFAULT_SCRATCH = path.join(homedir(), ".cache", "jet-test-scratch", "fresh-agent-domain");
const DEFAULT_OUTPUT = path.join(DEFAULT_SCRATCH, "scoreboard.json");

const MANIFEST_SCHEMA = "jet.fresh-agent-domain.manifest.v1";
const SCOREBOARD_SCHEMA = "jet.fresh-agent-domain.scoreboard.v1";
const PLAN_SCHEMA = "jet.fresh-agent-domain.plan.v1";
const RESPONSE_SCHEMA = "jet.fresh-agent-domain.response.v1";
const RESULT_SCHEMA = "jet.fresh-agent-domain.result.v1";
const REQUIRED_PHASES = Object.freeze(["first_run", "first_edit", "first_repair", "first_panel"]);
const OUTCOMES = Object.freeze(["pass", "unavailable", "wrong", "incomplete"]);
const FAILURE_CATEGORIES = Object.freeze(["setup", "compiler", "task", "environment"]);
const CONTEXT_MODES = Object.freeze(["capsule", "control"]);
const MAX_CAPTURE_BYTES = 2 * 1024 * 1024;
const REPORT_STATUSES = Object.freeze(["incomplete", "unavailable", "recorded-with-failures", "recorded"]);
const FAILURE_KINDS = Object.freeze(["blocked", "timeout", "malformed_output", "check_fail", "run_fail"]);
const LEGACY_TASK_IDS = Object.freeze(["hello", "cli", "data-transform", "http"]);
const DEFAULT_TIMEOUT_MS = 10 * 60 * 1000;
const USAGE_EXIT = 64;
const UNAVAILABLE_EXIT = 78;

export class UsageError extends Error {}
export class SetupError extends Error {}

function utf8Compare(left, right) {
  return Buffer.compare(Buffer.from(String(left), "utf8"), Buffer.from(String(right), "utf8"));
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort(utf8Compare).map((key) => [key, canonicalize(value[key])]));
  }
  return value;
}

export function stableJson(value) {
  return JSON.stringify(canonicalize(value));
}

function prettyJson(value) {
  return `${JSON.stringify(canonicalize(value), null, 2)}\n`;
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function hashJson(value) {
  return sha256(Buffer.from(stableJson(value), "utf8"));
}

function nonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function finiteMs(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function safeId(value, label) {
  if (!nonEmptyString(value) || !/^[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(value)) {
    throw new SetupError(`${label} must be a non-empty identifier using letters, numbers, '.', '_' or '-'`);
  }
  return value;
}

function absolute(value, base = process.cwd()) {
  return path.resolve(base, value);
}

function isTmpPath(value) {
  const resolved = absolute(value);
  return resolved === "/tmp" || resolved.startsWith(`${path.parse("/tmp").root}tmp${path.sep}`);
}

function assertNotTmp(value, label) {
  if (isTmpPath(value)) throw new SetupError(`${label} must not use /tmp: ${value}`);
}

function relativePath(value, root = REPO_DIR) {
  const rel = path.relative(root, value).split(path.sep).join("/");
  return rel && !rel.startsWith("../") && rel !== ".." ? rel : value;
}

async function readText(file, label) {
  try {
    return await fs.readFile(file, "utf8");
  } catch (error) {
    throw new SetupError(`cannot read ${label} ${file}: ${error.message}`);
  }
}

async function readJson(file, label) {
  const text = await readText(file, label);
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new SetupError(`invalid JSON in ${label} ${file}: ${error.message}`);
  }
}

async function fileDescriptor(file, label) {
  const bytes = await fs.readFile(file).catch((error) => {
    throw new SetupError(`cannot read ${label} ${file}: ${error.message}`);
  });
  return { path: relativePath(file), bytes: bytes.length, sha256: sha256(bytes) };
}

function utf8Prefix(text, budget) {
  const bytes = Buffer.from(text, "utf8");
  if (bytes.length <= budget) return text;
  for (let end = budget; end >= 0; end -= 1) {
    const value = bytes.subarray(0, end).toString("utf8");
    if (Buffer.byteLength(value, "utf8") === end) return value;
  }
  return "";
}

function defaultScratch() {
  return absolute(process.env.JET_FRESH_AGENT_SCRATCH_DIR
    || process.env.JET_TEST_SCRATCH
    || DEFAULT_SCRATCH);
}

function defaultOutput() {
  return path.join(defaultScratch(), "scoreboard.json");
}

function usage() {
  return `Usage: node scripts/agent/fresh-agent-domain-run.mjs [mode] [options]

This tool plans or records cold-agent runs for the eight critical Jet areas.
It never starts an agent unless --run is present. Scratch and transcripts are
kept on disk under ~/.cache/jet-test-scratch by default; /tmp is rejected.

Modes (choose one):
  --help                 Show this help.
  --list                 List the eight domains in deterministic order.
  --check                Validate the manifest, inputs, adapter commands, and an existing output.
  --dry-run              Print the deterministic run plan without creating scratch or invoking agents.
  --run                  Execute missing domain/context/adapter rows. Required to invoke agents.

Run options:
  --resume               Resume an existing output after identity validation; prior evidence is kept.
  --manifest PATH        Canonical DX manifest (default: tools/agent-eval/dx/manifest.json)
  --adapters PATH        OMP adapter config (default: tools/agent-eval/adapters.json)
  --capsule PATH         Complete context capsule
  --llms PATH            Explicit digest fixture; default reads jet inspect digest
  --mode capsule|control|both
  --adapter ID|both       Adapter family to use (default: both)
  --output PATH           Scoreboard output (default: ~/.cache/jet-test-scratch/fresh-agent-domain/scoreboard.json)
  --scratch PATH          Disk-backed scratch root
  --run-id ID             Stable run identity (default: generated timestamp and pid)
  --timeout-ms N          Per-agent command timeout (default: 600000)
  --json                  Emit machine-readable output for list/check/dry-run
  -h, --help              Show this help

A run writes one immutable attempt directory per row. Existing output is a
setup error unless --resume is explicit. Resume only appends attempts and never
replaces a completed result. Result outcomes are pass, unavailable, wrong, or
incomplete; every non-pass result carries setup, compiler, task, or environment
failure classification.\n`;
}

function parsePath(argv, index, token) {
  const value = argv[index + 1];
  if (!value || value.startsWith("--")) throw new UsageError(`${token} needs a path`);
  return absolute(value);
}

export function parseArgs(argv = []) {
  const options = {
    action: null,
    manifest: DEFAULT_MANIFEST,
    adapters: DEFAULT_ADAPTERS,
    capsule: DEFAULT_CAPSULE,
    llms: null,
    output: defaultOutput(),
    scratch: defaultScratch(),
    modes: [...CONTEXT_MODES],
    adapterSelection: "both",
    timeoutMs: DEFAULT_TIMEOUT_MS,
    runId: null,
    resume: false,
    json: false,
  };
  const actions = new Set(["help", "list", "check", "dry-run", "run"]);
  const pathFlags = new Map([
    ["--manifest", "manifest"],
    ["--adapters", "adapters"],
    ["--capsule", "capsule"],
    ["--llms", "llms"],
    ["--output", "output"],
    ["--scratch", "scratch"],
  ]);
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "-h" || token === "--help") {
      if (options.action && options.action !== "help") throw new UsageError("--help cannot be combined with another mode");
      options.action = "help";
      continue;
    }
    const action = token.startsWith("--") ? token.slice(2) : null;
    if (actions.has(action)) {
      if (options.action && options.action !== action) throw new UsageError(`modes --${options.action} and --${action} cannot be combined`);
      options.action = action;
      continue;
    }
    if (token === "--resume") {
      options.resume = true;
      continue;
    }
    if (token === "--json") {
      options.json = true;
      continue;
    }
    if (token === "--mode") {
      const value = argv[++index];
      if (!value || !["capsule", "control", "both"].includes(value)) throw new UsageError("--mode needs capsule, control, or both");
      options.modes = value === "both" ? [...CONTEXT_MODES] : [value];
      continue;
    }
    if (token === "--adapter") {
      const value = argv[++index];
      if (!value || !/^([A-Za-z0-9._-]+|both)(,[A-Za-z0-9._-]+)*$/u.test(value)) throw new UsageError("--adapter needs an adapter id, both, or a comma-separated id list");
      options.adapterSelection = value;
      continue;
    }
    if (token === "--run-id") {
      const value = argv[++index];
      if (!value) throw new UsageError("--run-id needs an identifier");
      options.runId = safeId(value, "--run-id");
      continue;
    }
    if (token === "--timeout-ms") {
      const value = Number(argv[++index]);
      if (!Number.isInteger(value) || value < 1) throw new UsageError("--timeout-ms needs a positive integer");
      options.timeoutMs = value;
      continue;
    }
    const pathKey = pathFlags.get(token);
    if (pathKey) {
      options[pathKey] = parsePath(argv, index, token);
      index += 1;
      continue;
    }
    throw new UsageError(`unknown option: ${token}`);
  }
  if (options.resume && options.action !== "run") throw new UsageError("--resume requires --run");
  if (options.action === "help" && options.resume) throw new UsageError("--help cannot be combined with --resume");
  if (!options.action) throw new UsageError("no mode selected; pass --help, --list, --check, --dry-run, or --run");
  return options;
}

function validateScenario(scenario, label) {
  assert(scenario && typeof scenario === "object" && !Array.isArray(scenario), `${label}.scenario is missing`);
  assert(LEGACY_TASK_IDS.includes(scenario.base_task_id), `${label}.scenario.base_task_id must preserve a #2324 task id`);
  safeId(scenario.fixture_variant, `${label}.scenario.fixture_variant`);
  assert(Array.isArray(scenario.fixture_files) && scenario.fixture_files.length > 0 && scenario.fixture_files.every(nonEmptyString), `${label}.scenario.fixture_files is invalid`);
  assert(Array.isArray(scenario.required_capabilities) && scenario.required_capabilities.length > 0 && scenario.required_capabilities.every(nonEmptyString), `${label}.scenario.required_capabilities is invalid`);
  for (const phase of REQUIRED_PHASES) {
    const contract = scenario[phase];
    assert(contract && typeof contract === "object" && nonEmptyString(contract.observation) && nonEmptyString(contract.marker), `${label}.scenario.${phase} is incomplete`);
  }
}

function validateMethodTask(task, index) {
  assert(task && typeof task === "object" && !Array.isArray(task), `method_tasks[${index}] is invalid`);
  assert(task.id === LEGACY_TASK_IDS[index], `method_tasks[${index}] must preserve #2324 task order`);
  safeId(task.fixture_id, `method_tasks[${index}].fixture_id`);
  assert(["batch", "http"].includes(task.mode), `method_tasks[${index}].mode is invalid`);
  assert(nonEmptyString(task.objective), `method_tasks[${index}].objective is missing`);
}

function validatePhaseContract(protocol) {
  if (!protocol || typeof protocol !== "object" || !Array.isArray(protocol.required_phases)
    || protocol.required_phases.length !== REQUIRED_PHASES.length
    || protocol.required_phases.some((phase, index) => phase !== REQUIRED_PHASES[index])) {
    throw new SetupError("manifest protocol.required_phases must be first_run, first_edit, first_repair, first_panel in that order");
  }
  if (!Array.isArray(protocol.outcomes) || protocol.outcomes.join("\0") !== OUTCOMES.join("\0")) {
    throw new SetupError("manifest protocol.outcomes does not match the result contract");
  }
  if (!Array.isArray(protocol.failure_categories) || protocol.failure_categories.join("\0") !== FAILURE_CATEGORIES.join("\0")) {
    throw new SetupError("manifest protocol.failure_categories does not match the result contract");
  }
  if (!Array.isArray(protocol.failure_kinds) || stableJson(protocol.failure_kinds) !== stableJson([...FAILURE_KINDS])) {
    throw new SetupError("manifest protocol.failure_kinds does not distinguish blocked, timeout, malformed_output, check_fail, and run_fail");
  }
}

export function validateManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) throw new SetupError("manifest must be an object");
  if (manifest.schema !== MANIFEST_SCHEMA || manifest.schema_version !== 1) throw new SetupError(`manifest schema must be ${MANIFEST_SCHEMA}`);
  if (manifest.card !== "#2482") throw new SetupError("manifest card must be #2482");
  if (manifest.runner !== "scripts/agent/fresh-agent-domain-run.mjs") throw new SetupError("manifest runner identity is wrong");
  if (manifest.response_schema !== RESPONSE_SCHEMA || manifest.result_schema !== RESULT_SCHEMA || manifest.scoreboard_schema !== SCOREBOARD_SCHEMA) {
    throw new SetupError("manifest result and response schemas do not match the runner");
  }
  if (!manifest.context || typeof manifest.context !== "object"
    || manifest.context.capsule !== "tools/agent-eval/jet-context-capsule.md"
    || manifest.context.control !== "jet inspect digest"
    || stableJson(manifest.context.modes) !== stableJson([...CONTEXT_MODES])
    || manifest.context.control_rule !== "The control is the exact UTF-8 prefix of jet inspect digest with the capsule byte budget.") {
    throw new SetupError("manifest context paths, modes, or control rule are invalid");
  }
  if (!manifest.protocol || manifest.protocol.fresh_process !== true || manifest.protocol.hidden_context !== false) {
    throw new SetupError("manifest protocol must require a fresh process and forbid hidden context");
  }
  if (!manifest.adapters || typeof manifest.adapters !== "object"
    || manifest.adapters.config !== "tools/agent-eval/adapters.json"
    || manifest.adapters.transport !== "command"
    || !Array.isArray(manifest.adapters.required_families)
    || manifest.adapters.required_families.length !== 2
    || manifest.adapters.required_families.some((family) => !nonEmptyString(family))) {
    throw new SetupError("manifest adapters contract is invalid");
  }
  if (new Set(manifest.adapters.required_families).size !== manifest.adapters.required_families.length) {
    throw new SetupError("manifest adapter families must be unique");
  }
  validatePhaseContract(manifest.protocol);
  if (!Array.isArray(manifest.domains) || manifest.domains.length < 8) throw new SetupError("manifest must contain at least eight domains");
  const domainCount = manifest.domains.length;
  const orders = new Set();
  const ids = new Set();
  const taskIds = new Set();
  const fixtures = new Set();
  for (const [index, domain] of manifest.domains.entries()) {
    if (!domain || typeof domain !== "object") throw new SetupError(`domain ${index} must be an object`);
    if (!Number.isInteger(domain.order) || domain.order < 1 || domain.order > domainCount || orders.has(domain.order)) throw new SetupError(`domain ${index} has a duplicate or invalid order`);
    orders.add(domain.order);
    for (const [field, set] of [["id", ids], ["task_id", taskIds], ["fixture_id", fixtures]]) {
      safeId(domain[field], `domain ${index}.${field}`);
      if (set.has(domain[field])) throw new SetupError(`duplicate domain ${field} ${domain[field]}`);
      set.add(domain[field]);
    }
    for (const field of ["slug", "title", "task"]) if (!nonEmptyString(domain[field])) throw new SetupError(`domain ${domain.id} needs ${field}`);
    if (!Array.isArray(domain.success_facts) || domain.success_facts.length === 0 || domain.success_facts.some((value) => !nonEmptyString(value))) {
      throw new SetupError(`domain ${domain.id} needs non-empty success_facts`);
    }
    validateScenario(domain.scenario, domain.id);
  }
  if (orders.size !== domainCount || [...orders].some((value, index) => value !== index + 1)) throw new SetupError("domain orders must be contiguous and deterministic");
  if (!Array.isArray(manifest.method_tasks) || manifest.method_tasks.length !== LEGACY_TASK_IDS.length
    || stableJson(manifest.method_tasks.map((task) => task.id)) !== stableJson([...LEGACY_TASK_IDS])) {
    throw new SetupError("manifest must preserve #2324 task ids hello, cli, data-transform, http");
  }
  for (const [index, task] of manifest.method_tasks.entries()) validateMethodTask(task, index);
  if (!Array.isArray(manifest.fresh_domains) || manifest.fresh_domains.length !== domainCount) {
    throw new SetupError("fresh-agent domain inventory is incomplete");
  }
  return [...manifest.domains].sort((left, right) => left.order - right.order || utf8Compare(left.id, right.id));
}

export async function loadAdapterConfig(file = DEFAULT_ADAPTERS) {
  return validateAdapterConfig(await readJson(file, "adapter config"));
}

function adapterFamilies(config) {
  return config.map((adapter) => adapter.family).sort(utf8Compare);
}

function validateRequiredFamilies(manifest, adapters) {
  const expected = [...manifest.adapters.required_families].sort(utf8Compare);
  const actual = adapterFamilies(adapters);
  if (expected.length !== actual.length || expected.some((family, index) => family !== actual[index])) {
    throw new SetupError("adapter config does not provide the manifest required families");
  }
}

function validateAdapterConfig(config) {
  if (!config || typeof config !== "object" || config.schema !== "jet.cold-agent.adapters.v1" || !Array.isArray(config.adapters)) {
    throw new SetupError("adapter config has the wrong schema");
  }
  const ids = new Set();
  const families = new Set();
  for (const adapter of config.adapters) {
    if (!adapter || !nonEmptyString(adapter.id) || !nonEmptyString(adapter.family) || ids.has(adapter.id) || families.has(adapter.family)) {
      throw new SetupError("adapter ids and families must be unique non-empty strings");
    }
    ids.add(adapter.id);
    families.add(adapter.family);
    if (adapter.default_transport !== "command" || !adapter.command || !Array.isArray(adapter.command.default_argv) || adapter.command.default_argv.length === 0
      || adapter.command.default_argv.some((part) => !nonEmptyString(part))) throw new SetupError(`${adapter.id} needs a command transport and command.default_argv`);
    if (adapter.command.input !== "prompt-argument" && adapter.command.input !== "json-stdin") throw new SetupError(`${adapter.id} has an invalid command input mode`);
    if (!nonEmptyString(adapter.command.command_env)) throw new SetupError(`${adapter.id} needs command.command_env`);
    if (adapter.command.version_argv !== undefined
      && (!Array.isArray(adapter.command.version_argv) || adapter.command.version_argv.length === 0 || adapter.command.version_argv.some((part) => !nonEmptyString(part)))) {
      throw new SetupError(`${adapter.id} command.version_argv must be a non-empty argv array`);
    }
  }
  return [...config.adapters].sort((left, right) => utf8Compare(left.id));
}

function selectedAdapterIds(selection, adapters) {
  if (selection === "both") return adapters.map((adapter) => adapter.id);
  const ids = selection.split(",");
  if (new Set(ids).size !== ids.length) throw new UsageError("--adapter repeats an adapter id");
  return ids;
}
function normalizeCanonicalManifest(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) throw new SetupError("manifest must be an object");
  if (raw.schema !== "jet.dx.benchmark-manifest.v1" || raw.schema_version !== 1) {
    throw new SetupError("fresh-agent runner requires the canonical tools/agent-eval/dx/manifest.json");
  }
  const fresh = raw.fresh_agent;
  if (!fresh || typeof fresh !== "object" || fresh.card !== "#2482") {
    throw new SetupError("canonical manifest has no fresh_agent card #2482 section");
  }
  const taskById = new Map((raw.tasks || []).map((task) => [task.id, task]));
  const taskByTaskId = new Map((raw.tasks || []).map((task) => [task.task_id, task]));
  const domainById = new Map((raw.domains || []).map((domain) => [domain.id, domain]));
  const methodTasks = fresh.method_tasks || raw.method_tasks;
  const freshDomains = (fresh.domains || []).map((reference) => {
    const canonicalDomain = domainById.get(reference.id || reference.task_id);
    const task = taskById.get(reference.task_id) || taskByTaskId.get(reference.task_id);
    if (!task || !canonicalDomain) throw new SetupError(`canonical fresh-agent task is missing: ${reference.task_id}`);
    const scenario = reference.scenario || canonicalDomain.scenario || task.scenario;
    if (!scenario) throw new SetupError(`canonical fresh-agent scenario is missing: ${reference.task_id}`);
    return {
      order: reference.order,
      id: reference.id || canonicalDomain.id,
      slug: reference.slug || canonicalDomain.slug || task.slug,
      title: reference.title || canonicalDomain.title || task.title,
      fixture_id: reference.fixture_id || task.fixture_id,
      task_id: task.task_id,
      base_task_id: reference.base_task_id || scenario.base_task_id,
      task: task.task,
      success_facts: task.success_facts,
      scenario,
    };
  });
  return {
    schema: MANIFEST_SCHEMA,
    schema_version: 1,
    card: fresh.card,
    runner: fresh.runner,
    response_schema: fresh.response_schema,
    result_schema: fresh.result_schema,
    scoreboard_schema: fresh.scoreboard_schema,
    context: fresh.context,
    protocol: fresh.protocol,
    source_execution: fresh.source_execution ?? null,
    baseline: fresh.baseline ?? null,
    adapters: fresh.adapters,
    method_tasks: methodTasks,
    fresh_domains: freshDomains,
    domains: freshDomains,
  };
}

async function loadManifest(file) {
  const raw = await readJson(file, "canonical DX manifest");
  return validateManifest(normalizeCanonicalManifest(raw));
}

function commandFromEnvironment(adapter, env = process.env) {
  const value = String(env[adapter.command.command_env] ?? "").trim();
  if (!value) return { argv: [...adapter.command.default_argv], source: "manifest" };
  let argv;
  try {
    argv = JSON.parse(value);
  } catch {
    throw new SetupError(`${adapter.id}: ${adapter.command.command_env} must be a JSON argv array`);
  }
  if (!Array.isArray(argv) || argv.length === 0 || argv.some((part) => !nonEmptyString(part))) {
    throw new SetupError(`${adapter.id}: ${adapter.command.command_env} must be a non-empty JSON argv array`);
  }
  const hiddenFlags = new Set(["--session", "--tools", "--skills", "--rules", "--extensions", "--context", "--system-prompt"]);
  if (argv.some((part) => hiddenFlags.has(part))) throw new SetupError(`${adapter.id}: command override enables hidden context`);
  if (argv.some((part) => secretEnvironmentName(part))) throw new SetupError(`${adapter.id}: command override contains a provider secret flag`);
  return { argv, source: "environment" };
}

function adapterDescriptor(adapter, command) {
  const modelIndex = command.argv.indexOf("--model");
  const versionArgv = Array.isArray(adapter.command.version_argv) && adapter.command.version_argv.length > 0
    ? [...adapter.command.version_argv]
    : [command.argv[0], "--version"];
  return {
    id: adapter.id,
    family: adapter.family,
    transport: "command",
    input: adapter.command.input,
    command_source: command.source,
    command_sha256: hashJson(command.argv),
    model: modelIndex >= 0 ? command.argv[modelIndex + 1] ?? null : null,
    version_argv: versionArgv,
    tool_version: nonEmptyString(adapter.command.tool_version) ? adapter.command.tool_version : null,
    tool_version_status: nonEmptyString(adapter.command.tool_version) ? "observed-in-config" : "pending-run",
    tool_version_reason: nonEmptyString(adapter.command.tool_version) ? null : "version command must run before scoring this adapter",
  };
}

function contextDescriptor(mode, capsule, control, budgetBytes) {
  const text = mode === "capsule" ? capsule : control;
  return {
    mode,
    bytes: Buffer.byteLength(text, "utf8"),
    sha256: sha256(Buffer.from(text, "utf8")),
    budget_bytes: budgetBytes,
  };
}

async function loadInputs(options) {
  const capsuleText = await readText(options.capsule, "capsule");
  let llmsText;
  if (options.llms) {
    llmsText = await readText(options.llms, "control digest");
  } else {
    const result = await spawnTracked([path.join(REPO_DIR, "scripts/agent/jet-env"), "jet", "inspect", "digest"], REPO_DIR, { env: process.env, input: "", timeoutMs: 180000 });
    if (result.exit_code !== 0 || result.timed_out || result.output_limit || result.spawn_error) {
      throw new SetupError(`cannot generate control digest: ${result.spawn_error || result.stderr.toString("utf8") || "digest command failed"}`);
    }
    llmsText = result.stdout.toString("utf8");
  }
  const budgetBytes = Buffer.byteLength(capsuleText, "utf8");
  const llmsBytes = Buffer.byteLength(llmsText, "utf8");
  if (llmsBytes < budgetBytes) throw new SetupError(`control digest is ${llmsBytes} bytes, shorter than the capsule budget ${budgetBytes}`);
  const controlText = utf8Prefix(llmsText, budgetBytes);
  if (Buffer.byteLength(controlText, "utf8") !== budgetBytes) throw new SetupError("control context did not preserve the capsule byte budget");
  return {
    capsuleText,
    controlText,
    budgetBytes,
    capsule: { ...(await fileDescriptor(options.capsule, "capsule")), bytes: budgetBytes },
    llms: options.llms
      ? await fileDescriptor(options.llms, "control digest")
      : { path: "jet inspect digest", bytes: llmsBytes, sha256: sha256(Buffer.from(llmsText, "utf8")) },
    contexts: {
      capsule: contextDescriptor("capsule", capsuleText, controlText, budgetBytes),
      control: contextDescriptor("control", capsuleText, controlText, budgetBytes),
    },
  };
}

function taskIdentity(domain) {
  return {
    task_id: domain.task_id,
    base_task_id: domain.base_task_id,
    domain: domain.id,
    slug: domain.slug,
    fixture_id: domain.fixture_id,
    scenario_sha256: hashJson(domain.scenario),
    prompt_contract_sha256: hashJson({ task_id: domain.task_id, base_task_id: domain.base_task_id, domain: domain.id, fixture_id: domain.fixture_id, task: domain.task, success_facts: domain.success_facts, scenario: domain.scenario, phases: REQUIRED_PHASES }),
  };
}
function promptFor(domain, mode, context) {
  return [
    "You are a fresh Jet agent solving one isolated domain task.",
    "You have no repository, session, tool, skill, rule, extension, or maintainer context.",
    "Do not guess an observation. If a prerequisite cannot be exercised, report unavailable with the exact reason.",
    "Use the domain fixture and marker contract below; do not replace it with a generic hello-world or an unrelated tool.",
    "Return exactly one JSON object and no Markdown or commentary.",
    `Response schema: ${RESPONSE_SCHEMA}`,
    "The object must contain schema, task_id, domain, fixture_id, context_mode, phases, and diagnostics.",
    "Each phase must contain status (pass, unavailable, wrong, or incomplete), command.argv, observed or unavailable_reason, elapsed_ms, and resource.",
    "A pass phase must also contain source: {mode: batch|http, contents: UTF-8 Jet source, expected_stdout: exact UTF-8 output or null}; the harness checks and runs this source.",
    `Task ID: ${domain.task_id}`,
    "Command evidence must include cwd, exit_code or signal, elapsed_ms, stdout_sha256, and stderr_sha256. Never claim a pass without those facts.",
    `#2324 base task ID: ${domain.base_task_id}`,
    `Domain: ${domain.title} (${domain.id})`,
    `Fixture ID: ${domain.fixture_id}`,
    `Fixture variant: ${domain.scenario.fixture_variant}`,
    `Fixture files: ${domain.scenario.fixture_files.join(", ")}`,
    `Required capabilities: ${domain.scenario.required_capabilities.join("; ")}`,
    `Context mode: ${mode}`,
    "",
    domain.task,
    "",
    ...REQUIRED_PHASES.flatMap((phase) => [
      `${phase} observation: ${domain.scenario[phase].observation}`,
      `${phase} marker: ${domain.scenario[phase].marker}`,
    ]),
    "",
    `Required phase order: ${REQUIRED_PHASES.join(", ")}`,
    `Required success facts: ${domain.success_facts.join("; ")}`,
    "",
    "## Context",
    context,
  ].join("\n");
}
function environmentIdentity(env = process.env) {
  const names = Object.keys(env).sort(utf8Compare);
  const values = Object.fromEntries(names.map((name) => [
    name,
    /KEY|TOKEN|SECRET|PASSWORD|AUTH/u.test(name) ? "<redacted>" : String(env[name] ?? ""),
  ]));
  return {
    platform: process.platform,
    arch: process.arch,
    node: process.version,
    os_release: release(),
    cpu_model: cpus()[0]?.model || "unknown",
    cpu_count: cpus().length,
    memory_bytes: totalmem(),
    hostname_sha256: sha256(hostname()),
    env_names: names,
    env_sha256: hashJson(values),
    secrets_redacted: true,
  };
}


function planIdentity({ manifest, manifestFile, adaptersFile, inputs, adapters, modes, domains, environment = environmentIdentity() }) {
  const identity = {
    manifest_sha256: hashJson(manifest),
    manifest_path: relativePath(manifestFile),
    adapters_file: { ...adaptersFile },
    capsule: { ...inputs.capsule },
    llms: { ...inputs.llms },
    contexts: modes.map((mode) => inputs.contexts[mode]),
    adapters: adapters.map((adapter) => adapter.descriptor),
    tasks: domains.map(taskIdentity),
    environment,
  };
  return { ...identity, sha256: hashJson(identity) };
}


function publicRowKey(adapterId, mode, domainId) {
  return `${adapterId}/${mode}/${domainId}`;
}

function sortedRows(rows) {
  return [...rows].sort((left, right) => utf8Compare(left.key, right.key));
}

function resultResource(before, after) {
  const user = Number.isFinite(after?.userCPUTime) && Number.isFinite(before?.userCPUTime)
    ? Math.max(0, (after.userCPUTime - before.userCPUTime) / 1000) : null;
  const system = Number.isFinite(after?.systemCPUTime) && Number.isFinite(before?.systemCPUTime)
    ? Math.max(0, (after.systemCPUTime - before.systemCPUTime) / 1000) : null;
  const rss = Number.isFinite(after?.maxRSS) ? after.maxRSS * 1024 : null;
  return {
    user_cpu_ms: user,
    system_cpu_ms: system,
    max_rss_bytes: rss,
    available: user !== null || system !== null || rss !== null,
  };
}

function hiddenContextName(name) {
  return /^(?:OMP|JET)_(?:SESSION|TOOLS|SKILLS|RULES|EXTENSIONS|CONTEXT|SYSTEM_PROMPT)(?:_|$)/u.test(name)
    || /(?:^|_)(?:MAINTAINER_CONTEXT|SYSTEM_PROMPT|AGENT_CONTEXT)(?:_|$)/u.test(name);
}

function secretEnvironmentName(name) {
  return /(?:API_KEY|ACCESS_TOKEN|SECRET|PASSWORD|AUTH_TOKEN|PRIVATE_KEY)$/u.test(name);
}

function cleanRunnerEnvironment(env = process.env) {
  return Object.fromEntries(Object.entries(env).filter(([name]) => !hiddenContextName(name) && !secretEnvironmentName(name)));
}

function redactedEnvironment(env, scratch) {
  const names = Object.keys(env).sort(utf8Compare);
  const values = Object.fromEntries(names.map((name) => [
    name,
    secretEnvironmentName(name) ? "<redacted>" : String(env[name] ?? ""),
  ]));
  values.TMPDIR = scratch;
  values.TMP = scratch;
  values.TEMP = scratch;
  return {
    names,
    removed_hidden_context: Object.keys(process.env).filter((name) => hiddenContextName(name)).sort(utf8Compare),
    removed_secret_values: Object.keys(process.env).filter((name) => secretEnvironmentName(name)).sort(utf8Compare),
    sha256: hashJson(values),
    secrets_redacted: true,
  };
}

function killTree(child, signal = "SIGTERM") {
  if (!child || child.exitCode !== null) return;
  try {
    if (process.platform === "win32") child.kill(signal);
    else process.kill(-child.pid, signal);
  } catch {
    try { child.kill(signal); } catch { /* process already exited */ }
  }
}

function captureChunk(chunks, chunk, total) {
  const remaining = MAX_CAPTURE_BYTES - total;
  if (remaining > 0) chunks.push(chunk.subarray(0, remaining));
  return total + chunk.length;
}

function spawnTracked(argv, cwd, { env, input, timeoutMs }) {
  const startedAt = new Date().toISOString();
  const started = process.hrtime.bigint();
  const usageBefore = process.resourceUsage?.() ?? null;
  let child;
  try {
    child = spawn(argv[0], argv.slice(1), {
      cwd,
      env,
      detached: process.platform !== "win32",
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
  } catch (error) {
    return Promise.resolve({
      argv,
      cwd,
      started_at: startedAt,
      finished_at: new Date().toISOString(),
      elapsed_ms: Number(process.hrtime.bigint() - started) / 1e6,
      exit_code: null,
      signal: null,
      timed_out: false,
      output_limit: false,
      spawn_error: error.message,
      stdout: Buffer.alloc(0),
      stderr: Buffer.alloc(0),
      stdout_bytes: 0,
      stderr_bytes: 0,
      resource: resultResource(usageBefore, process.resourceUsage?.() ?? null),
    });
  }
  return new Promise((resolve) => {
    const stdout = [];
    const stderr = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let timedOut = false;
    let outputLimit = false;
    let settled = false;
    let timer = null;
    const finish = (exitCode, signal, spawnError = null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const usageAfter = process.resourceUsage?.() ?? null;
      resolve({
        argv,
        cwd,
        started_at: startedAt,
        finished_at: new Date().toISOString(),
        elapsed_ms: Number(process.hrtime.bigint() - started) / 1e6,
        exit_code: exitCode,
        signal,
        timed_out: timedOut,
        output_limit: outputLimit,
        spawn_error: spawnError,
        stdout: Buffer.concat(stdout),
        stderr: Buffer.concat(stderr),
        stdout_bytes: stdoutBytes,
        stderr_bytes: stderrBytes,
        resource: resultResource(usageBefore, usageAfter),
      });
    };
    child.stdout.on("data", (chunk) => {
      stdoutBytes = captureChunk(stdout, chunk, stdoutBytes);
      if (stdoutBytes > MAX_CAPTURE_BYTES && !outputLimit) {
        outputLimit = true;
        killTree(child);
      }
    });
    child.stderr.on("data", (chunk) => {
      stderrBytes = captureChunk(stderr, chunk, stderrBytes);
      if (stderrBytes > MAX_CAPTURE_BYTES && !outputLimit) {
        outputLimit = true;
        killTree(child);
      }
    });
    child.once("error", (error) => finish(null, null, error.message));
    child.once("close", (code, signal) => finish(code, signal));
    if (timeoutMs > 0) {
      timer = setTimeout(() => {
        timedOut = true;
        killTree(child);
      }, timeoutMs);
      timer.unref?.();
    }
    child.stdin.end(input ?? "");
  });
}

function baseFailure(category, code, message, phase = null, kind = null) {
  return { category, code, message, phase, kind: kind && FAILURE_KINDS.includes(kind) ? kind : null };
}

function emptyResource() {
  return { user_cpu_ms: null, system_cpu_ms: null, max_rss_bytes: null, available: false };
}

function emptyAttempt({ adapter, mode, domain, attemptId, key, prompt, context }) {
  return {
    schema: RESULT_SCHEMA,
    attempt_id: attemptId,
    key,
    adapter: { id: adapter.id, family: adapter.family },
    context: contextDescriptor(mode, context.capsuleText, context.controlText, context.budgetBytes),
    domain: { id: domain.id, title: domain.title, slug: domain.slug, fixture_id: domain.fixture_id, base_task_id: domain.base_task_id, scenario: domain.scenario },
    task: { ...taskIdentity(domain), prompt_sha256: sha256(Buffer.from(prompt, "utf8")) },
    outcome: "incomplete",
    failure: null,
    diagnostics: [],
    score: null,
    phases: {},
    command: null,
    elapsed_ms: 0,
    resource: emptyResource(),
    artifacts: {},
  };
}

function normalizePhaseStatus(value) {
  const status = String(value ?? "").trim().toLowerCase();
  return OUTCOMES.includes(status) ? status : null;
}

function commandEvidence(phase) {
  const command = phase?.command;
  if (!command || typeof command !== "object" || !Array.isArray(command.argv)
    || command.argv.length === 0 || command.argv.some((part) => typeof part !== "string" || part.length === 0)) return null;
  return {
    argv: [...command.argv],
    cwd: nonEmptyString(command.cwd) ? command.cwd : null,
    exit_code: Number.isInteger(command.exit_code) ? command.exit_code : null,
    signal: typeof command.signal === "string" ? command.signal : null,
    elapsed_ms: finiteMs(command.elapsed_ms) ? command.elapsed_ms : null,
    stdout_sha256: /^[0-9a-f]{64}$/u.test(command.stdout_sha256 ?? "") ? command.stdout_sha256 : null,
    stderr_sha256: /^[0-9a-f]{64}$/u.test(command.stderr_sha256 ?? "") ? command.stderr_sha256 : null,
  };
}

function validAgentResource(resource) {
  if (!resource || typeof resource !== "object" || Array.isArray(resource)
    || typeof resource.available !== "boolean") return false;
  for (const field of ["user_cpu_ms", "system_cpu_ms", "max_rss_bytes"]) {
    if (resource[field] !== null && (!Number.isFinite(resource[field]) || resource[field] < 0)) return false;
  }
  return true;
}
function sourceContract(value, label) {
  if (value === undefined || value === null) return null;
  if (!value || typeof value !== "object" || !nonEmptyString(value.contents) || value.contents.length > MAX_CAPTURE_BYTES
    || (value.mode !== undefined && !["batch", "http"].includes(value.mode))
    || ((value.mode || "batch") === "batch" && typeof value.expected_stdout !== "string")
    || (value.expected_stdout !== undefined && typeof value.expected_stdout !== "string")) {
    throw new SetupError(`${label}.source must contain bounded UTF-8 contents, mode, and exact expected_stdout for batch`);
  }
  return {
    mode: value.mode || "batch",
    contents: value.contents,
    contents_sha256: sha256(Buffer.from(value.contents, "utf8")),
    expected_stdout: value.expected_stdout ?? null,
  };
}
function responseScore(value) {
  if (value === undefined || value === null) return null;
  if (!value || typeof value !== "object") return null;
  const score = {};
  for (const field of ["compile", "run", "total"]) {
    if (!Number.isFinite(value[field])) return null;
    score[field] = value[field];
  }
  return score;
}

function classifyResponse(raw, { adapter, mode, domain, prompt, context, attemptId, key }) {
  const result = emptyAttempt({ adapter, mode, domain, attemptId, key, prompt, context });
  let response;
  try {
    response = JSON.parse(raw);
  } catch {
    result.outcome = "incomplete";
    result.failure = baseFailure("task", "E_RESPONSE_JSON", "agent response was not one JSON object", null, "malformed_output");
    return result;
  }
  if (!response || typeof response !== "object" || Array.isArray(response)) {
    result.failure = baseFailure("task", "E_RESPONSE_OBJECT", "agent response was not an object", null, "malformed_output");
    return result;
  }
  if (response.schema !== undefined && response.schema !== RESPONSE_SCHEMA) {
    result.failure = baseFailure("task", "E_RESPONSE_SCHEMA", `agent response schema is not ${RESPONSE_SCHEMA}`, null, "malformed_output");
    return result;
  }
  result.score = responseScore(response.score);
  if (response.score !== undefined && response.score !== null && result.score === null) {
    result.failure = baseFailure("task", "E_SCORE_SHAPE", "score must contain numeric compile, run, and total fields", null, "malformed_output");
    return result;
  }
  result.task.response_task_id = response.task_id ?? null;
  result.task.response_domain = response.domain ?? null;
  result.task.response_fixture_id = response.fixture_id ?? null;
  result.task.response_context_mode = response.context_mode ?? null;
  if (response.task_id !== domain.task_id || response.domain !== domain.id || response.fixture_id !== domain.fixture_id || response.context_mode !== mode) {
    result.outcome = "wrong";
    result.failure = baseFailure("task", "E_TASK_IDENTITY", "agent response task, domain, fixture, or context identity does not match the prompt", null, "run_fail");
    return result;
  }
  const responseDiagnostics = normalizeDiagnostics(response.diagnostics);
  if (responseDiagnostics === null) {
    result.failure = baseFailure("task", "E_DIAGNOSTICS_SHAPE", "diagnostics must be typed objects with code, message, category, and optional kind", null, "malformed_output");
    return result;
  }
  result.diagnostics = responseDiagnostics;
  if (!response.phases || typeof response.phases !== "object" || Array.isArray(response.phases)) {
    result.failure = baseFailure("task", "E_PHASES_MISSING", "response has no phase object", null, "malformed_output");
    return result;
  }
  let sawUnavailable = false;
  let sawWrong = false;
  let sawIncomplete = false;
  let firstFailure = null;
  for (const phaseId of REQUIRED_PHASES) {
    const phase = response.phases[phaseId];
    const status = normalizePhaseStatus(phase?.status);
    const command = commandEvidence(phase);
    const diagnostics = normalizeDiagnostics(phase?.diagnostics, phaseId);
    const observed = phase?.observed ?? phase?.evidence ?? phase?.output ?? null;
    const unavailableReason = phase?.unavailable_reason ?? phase?.reason ?? null;
    const elapsed = phase?.elapsed_ms;
    const resource = phase?.resource;
    const failureKind = phase?.failure_kind ?? phase?.kind ?? null;
    const validFailureKind = failureKind === null || FAILURE_KINDS.includes(failureKind);
    const validResource = validAgentResource(resource);
    let source = null;
    let sourceInvalid = false;
    try { source = sourceContract(phase?.source, `${phaseId}`); } catch { sourceInvalid = true; }
    const missingObservation = observed === null || observed === "";
    const missingSource = status === "pass" && source === null;
    const missingEvidence = !status
      || !command
      || !validFailureKind
      || !finiteMs(elapsed)
      || !validResource
      || sourceInvalid
      || missingSource
      || (status === "unavailable" ? missingObservation && !nonEmptyString(unavailableReason) : missingObservation);
    result.phases[phaseId] = {
      status: status ?? "incomplete",
      command,
      observed,
      unavailable_reason: nonEmptyString(unavailableReason) ? unavailableReason : null,
      elapsed_ms: finiteMs(elapsed) ? elapsed : null,
      resource: validResource ? resource : null,
      failure_kind: validFailureKind && failureKind ? failureKind : null,
      diagnostics: diagnostics ?? [],
      source,
    };
    if (missingEvidence) {
      sawIncomplete = true;
      if (!firstFailure) firstFailure = baseFailure("task", "E_PHASE_EVIDENCE", `${phaseId} lacks required status, command, observation or unavailable reason, elapsed, resource, or failure kind evidence`, phaseId, "malformed_output");
      continue;
    }
    if (diagnostics === null) {
      sawIncomplete = true;
      if (!firstFailure) firstFailure = baseFailure("task", "E_PHASE_DIAGNOSTICS", `${phaseId} diagnostics are not typed`, phaseId, "malformed_output");
      continue;
    }
    if (status === "unavailable") {
      sawUnavailable = true;
      const category = FAILURE_CATEGORIES.includes(phase.failure_category) ? phase.failure_category : "environment";
      if (!firstFailure) firstFailure = baseFailure(category, "E_PHASE_UNAVAILABLE", `${phaseId} is unavailable: ${String(unavailableReason ?? "no reason recorded")}`, phaseId, failureKind || "blocked");
    } else if (status === "wrong") {
      sawWrong = true;
      const category = phase.failure_category ?? diagnostics.find((item) => item.category === "compiler")?.category ?? "task";
      const failureCategory = FAILURE_CATEGORIES.includes(category) ? category : "task";
      if (!firstFailure) firstFailure = baseFailure(failureCategory, "E_PHASE_WRONG", `${phaseId} reported a wrong result`, phaseId, failureKind || (phaseId === "first_repair" ? "check_fail" : "run_fail"));
    } else if (status === "incomplete") {
      sawIncomplete = true;
      if (!firstFailure) firstFailure = baseFailure("task", "E_PHASE_INCOMPLETE", `${phaseId} reported incomplete evidence`, phaseId, failureKind || "malformed_output");
    }
  }
  const compilerDiagnostic = result.diagnostics.find((item) => item.category === "compiler" && item.severity !== "info");
  if (compilerDiagnostic) {
    sawWrong = true;
    if (!firstFailure) firstFailure = baseFailure("compiler", compilerDiagnostic.code, compilerDiagnostic.message, compilerDiagnostic.phase, compilerDiagnostic.kind || "check_fail");
  }
  result.outcome = sawWrong ? "wrong" : sawUnavailable ? "unavailable" : sawIncomplete ? "incomplete" : "pass";
  result.failure = result.outcome === "pass" ? null : firstFailure;
  if (response.outcome !== undefined && normalizePhaseStatus(response.outcome) !== result.outcome) {
    result.outcome = "wrong";
    result.failure = baseFailure("task", "E_OUTCOME_MISMATCH", "top-level response outcome disagrees with phase outcomes", null, "malformed_output");
  }
  return result;
}

async function writeExclusive(file, content, mode = 0o600) {
  await fs.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  try {
    await fs.writeFile(file, content, { encoding: "utf8", flag: "wx", mode });
  } catch (error) {
    throw new SetupError(`refusing to overwrite evidence ${file}: ${error.message}`);
  }
}

async function writeJsonAtomic(file, value) {
  const temp = `${file}.${process.pid}.${Math.random().toString(16).slice(2)}.partial`;
  try {
    await fs.writeFile(temp, prettyJson(value), { encoding: "utf8", mode: 0o600 });
    await fs.rename(temp, file);
  } catch (error) {
    await fs.rm(temp, { force: true }).catch(() => {});
    throw new SetupError(`cannot write scoreboard ${file}: ${error.message}`);
  }
}

async function exists(file) {
  return fs.access(file).then(() => true).catch(() => false);
}

function reportStatus(rows, expectedRows) {
  if (rows.length < expectedRows) return "incomplete";
  if (rows.some((row) => row.outcome === "unavailable")) return "unavailable";
  if (rows.some((row) => row.outcome === "incomplete")) return "incomplete";
  if (rows.some((row) => row.outcome === "wrong")) return "recorded-with-failures";
  return "recorded";
}

function summarize(rows, expectedRows) {
  const summary = { expected: expectedRows, recorded: rows.length, pass: 0, unavailable: 0, wrong: 0, incomplete: 0, failure_categories: {} };
  for (const row of rows) {
    if (Object.hasOwn(summary, row.outcome)) summary[row.outcome] += 1;
    if (row.failure?.category) summary.failure_categories[row.failure.category] = (summary.failure_categories[row.failure.category] ?? 0) + 1;
  }
  summary.failure_categories = Object.fromEntries(Object.entries(summary.failure_categories).sort(([a], [b]) => utf8Compare(a, b)));
  return summary;
}

function scoreRegressions(rows) {
  const groups = new Map();
  for (const row of rows) {
    const base = row.key.replace(/\/(?:capsule|control)\//u, "/");
    const mode = row.context?.mode;
    if (!["capsule", "control"].includes(mode) || !row.score) continue;
    const item = groups.get(base) || {};
    item[mode] = row.score;
    groups.set(base, item);
  }
  const regressions = [];
  for (const [key, pair] of groups) {
    if (!pair.capsule || !pair.control) continue;
    for (const field of ["compile", "run", "total"]) {
      if (pair.capsule[field] < pair.control[field]) regressions.push({ key, field, capsule: pair.capsule[field], control: pair.control[field] });
    }
  }
  return regressions;
}

function makeReport({ run, identity, manifest, inputs, adapters, modes, domains, scratch, rows = [], attempts = [], status = null, errors = [] }) {
  const expectedRows = adapters.length * modes.length * domains.length;
  const sorted = sortedRows(rows);
  const regressions = scoreRegressions(sorted);
  const finalStatus = status ?? (regressions.length > 0 ? "recorded-with-failures" : reportStatus(sorted, expectedRows));
  return {
    schema: SCOREBOARD_SCHEMA,
    schema_version: 1,
    card: manifest.card,
    status: finalStatus,
    run,
    identity,
    policy: {
      fresh_process: true,
      hidden_context: false,
      no_automatic_run: true,
      required_phases: [...REQUIRED_PHASES],
      outcomes: [...OUTCOMES],
      failure_categories: [...FAILURE_CATEGORIES],
      failure_kinds: [...FAILURE_KINDS],
    },
    inputs: {
      manifest: { path: relativePath(manifest.__file), sha256: identity.manifest_sha256 },
      adapters: { ...identity.adapters_file },
      capsule: { ...inputs.capsule },
      llms: { ...inputs.llms },
      contexts: modes.map((mode) => inputs.contexts[mode]),
    },
    method_tasks: manifest.method_tasks.map((task) => ({ id: task.id, fixture_id: task.fixture_id, mode: task.mode })),
    baseline: manifest.baseline ?? null,
    adapters: adapters.map((adapter) => adapter.descriptor),
    contexts: [...modes],
    domains: domains.map((domain) => ({ order: domain.order, id: domain.id, slug: domain.slug, title: domain.title, fixture_id: domain.fixture_id, task_id: domain.task_id, base_task_id: domain.base_task_id, scenario: domain.scenario })),
    scratch: { root: scratch.root, run_dir: scratch.runDir, disk_only: true },
    expected_rows: expectedRows,
    rows: sorted,
    attempts: [...attempts].sort((left, right) => utf8Compare(left.attempt_id, right.attempt_id)),
    summary: summarize(sorted, expectedRows),
    score_regressions: regressions,
    errors: [...new Set([...errors, ...regressions.map((item) => `capsule ${item.field} score is lower than control for ${item.key}`)])].sort(utf8Compare),
  };
}

function validateArtifactRef(value) {
  return value && typeof value === "object" && nonEmptyString(value.path) && !isTmpPath(value.path) && Number.isInteger(value.bytes)
    && value.bytes >= 0 && /^[0-9a-f]{64}$/u.test(value.sha256);
}

function validateStoredResult(result, label) {
  if (!result || typeof result !== "object" || result.schema !== RESULT_SCHEMA || !nonEmptyString(result.attempt_id)
    || !nonEmptyString(result.key) || !OUTCOMES.includes(result.outcome)) throw new SetupError(`${label} has an invalid result identity`);
  if (result.score !== null && responseScore(result.score) === null) throw new SetupError(`${label} score fields are invalid`);
  if (!result.domain || typeof result.domain !== "object" || !nonEmptyString(result.domain.id)
    || !nonEmptyString(result.domain.fixture_id) || !nonEmptyString(result.domain.base_task_id)) throw new SetupError(`${label} domain identity is missing`);
  validateScenario(result.domain.scenario, `${label}.domain`);
  if (!result.task || result.task.task_id === undefined || !/^[0-9a-f]{64}$/u.test(result.task.prompt_contract_sha256 ?? "")) {
    throw new SetupError(`${label} task identity is missing`);
  }
  if (result.outcome === "pass" && result.failure !== null) throw new SetupError(`${label} pass has a failure classification`);
  if (result.outcome !== "pass" && (!result.failure || !FAILURE_CATEGORIES.includes(result.failure.category) || !nonEmptyString(result.failure.code) || !nonEmptyString(result.failure.message)
    || (result.failure.kind !== null && !FAILURE_KINDS.includes(result.failure.kind)))) {
    throw new SetupError(`${label} non-pass result lacks a typed failure classification`);
  }
  if (result.command !== null && result.command !== undefined) {
    const command = result.command;
    if (!Array.isArray(command.argv) || command.argv.length === 0 || command.argv.some((part) => !nonEmptyString(part))
      || !nonEmptyString(command.cwd) || !finiteMs(command.elapsed_ms) || !command.environment
      || !/^[0-9a-f]{64}$/u.test(command.argv_sha256 ?? "")
      || !/^[0-9a-f]{64}$/u.test(command.stdout?.sha256 ?? "")
      || !/^[0-9a-f]{64}$/u.test(command.stderr?.sha256 ?? "")) throw new SetupError(`${label} command transcript is incomplete`);
  }
  if (result.artifacts && typeof result.artifacts === "object") {
    for (const field of ["prompt", "command", "stdout", "stderr"]) {
      if (result.artifacts[field] !== undefined && !validateArtifactRef(result.artifacts[field])) throw new SetupError(`${label} has an invalid ${field} artifact`);
    }
  }
}

export function validateReport(report) {
  if (!report || typeof report !== "object" || report.schema !== SCOREBOARD_SCHEMA || report.schema_version !== 1) throw new SetupError("scoreboard has the wrong schema");
  if (!REPORT_STATUSES.includes(report.status)) throw new SetupError("scoreboard status is invalid");
  if (!report.identity || !/^[0-9a-f]{64}$/u.test(report.identity.sha256 ?? "") || !report.identity.environment) throw new SetupError("scoreboard identity or environment is missing");
  if (!Number.isInteger(report.expected_rows) || report.expected_rows < 1) throw new SetupError("scoreboard expected_rows is invalid");
  if (!Array.isArray(report.rows) || !Array.isArray(report.attempts)) throw new SetupError("scoreboard rows and attempts must be arrays");
  const keys = new Set();
  for (const row of report.rows) {
    validateStoredResult(row, `scoreboard row ${row?.key ?? "<unknown>"}`);
    if (keys.has(row.key)) throw new SetupError("scoreboard rows contain a duplicate result");
    keys.add(row.key);
  }
  const sorted = sortedRows(report.rows);
  if (stableJson(report.rows) !== stableJson(sorted)) throw new SetupError("scoreboard rows are not in deterministic order");
  const attemptIds = new Set();
  for (const attempt of report.attempts) {
    validateStoredResult(attempt, `scoreboard attempt ${attempt?.attempt_id ?? "<unknown>"}`);
    if (attemptIds.has(attempt.attempt_id)) throw new SetupError("scoreboard attempts contain a duplicate result");
    attemptIds.add(attempt.attempt_id);
  }
  const sortedAttempts = [...report.attempts].sort((left, right) => utf8Compare(left.attempt_id, right.attempt_id));
  if (stableJson(report.attempts) !== stableJson(sortedAttempts)) throw new SetupError("scoreboard attempts are not in deterministic order");
  return report;
}

function attemptOrdinal(attempts, key) {
  return attempts.filter((attempt) => attempt.key === key).length + 1;
}

function attemptId(adapter, mode, domain, ordinal) {
  return `${adapter.id}-${mode}-${domain.id}-a${String(ordinal).padStart(3, "0")}`;
}

function safeAttemptDirectory(adapter, mode, domain, ordinal) {
  return `${adapter.id}-${mode}-${domain.id}-a${String(ordinal).padStart(3, "0")}`;
}

async function createRunScratch(root, runId, identitySha256) {
  assertNotTmp(root, "scratch root");
  await fs.mkdir(root, { recursive: true, mode: 0o700 });
  const runDir = await fs.mkdtemp(path.join(root, `run-${runId}-`));
  await writeExclusive(path.join(runDir, "run.json"), prettyJson({
    schema: "jet.fresh-agent-domain.run.v1",
    run_id: runId,
    identity_sha256: identitySha256,
    pid: process.pid,
  }));
  return runDir;
}

async function validateRunScratch(runDir, runId, identitySha256) {
  assertNotTmp(runDir, "existing run scratch");
  const marker = await readJson(path.join(runDir, "run.json"), "run scratch marker");
  if (marker.schema !== "jet.fresh-agent-domain.run.v1" || marker.run_id !== runId || marker.identity_sha256 !== identitySha256) {
    throw new SetupError("existing run scratch marker identity differs; refusing to mix evidence");
  }
}

function runnerEnv(scratch, env = process.env) {
  return {
    ...cleanRunnerEnvironment(env),
    TMPDIR: scratch,
    TMP: scratch,
    TEMP: scratch,
    JET_TEST_SCRATCH: scratch,
    JET_TEST_SCRATCH_DIR: scratch,
    JET_NIX_TMP_CLEANED: "1",
    JET_FRESH_AGENT: "1",
  };
}

function sourceCommandEvidence(transcript, argv, cwd) {
  return {
    argv,
    cwd: path.relative(REPO_DIR, cwd) || ".",
    exit_code: transcript.exit_code,
    signal: transcript.signal,
    elapsed_ms: transcript.elapsed_ms,
    stdout_sha256: sha256(transcript.stdout),
    stderr_sha256: sha256(transcript.stderr),
  };
}

async function executeGeneratedSources(result, attemptDir, domain, env) {
  for (const phaseId of REQUIRED_PHASES) {
    const phase = result.phases[phaseId];
    const source = phase?.source;
    if (!source) continue;
    const phaseDir = path.join(attemptDir, "source", phaseId);
    await fs.mkdir(phaseDir, { recursive: true, mode: 0o700 });
    const sourcePath = path.join(phaseDir, "candidate.jet");
    const packagePath = path.join(phaseDir, "package.jet");
    await writeExclusive(sourcePath, source.contents);
    await writeExclusive(packagePath, `name: "fresh-agent-${domain.id}"\nversion: "0.1.0"\nedition: "2026"\nauthority: { holds: { allow: [Env, Exec, FS, Net, IO, Mem.Alloc, Panic, Time] } }\n`);
    if (source.mode === "http") {
      phase.source_execution = { status: "unavailable", reason: "loopback HTTP probe adapter is not available for this source-only phase", failure_kind: "blocked", source_path: path.relative(REPO_DIR, sourcePath) };
      if (result.outcome === "pass") {
        result.outcome = "unavailable";
        result.failure = baseFailure("environment", "E_HTTP_ADAPTER", phase.source_execution.reason, phaseId, "blocked");
      }
      continue;
    }
    const launcher = path.join(REPO_DIR, "scripts/agent/jet-env");
    const checkArgv = [launcher, "jet", "check", "candidate.jet"];
    const check = await spawnTracked(checkArgv, phaseDir, { env, input: "", timeoutMs: 180000 });
    const checkEvidence = sourceCommandEvidence(check, checkArgv, phaseDir);
    const execution = { status: "failed", source_path: path.relative(REPO_DIR, sourcePath), check: checkEvidence, run: null, expected_stdout_sha256: source.expected_stdout === null ? null : sha256(Buffer.from(source.expected_stdout, "utf8")) };
    if (check.exit_code !== 0 || check.timed_out || check.spawn_error || check.output_limit) {
      execution.reason = check.spawn_error || check.stderr.toString("utf8") || "generated Jet source failed jet check";
      execution.failure_kind = check.timed_out ? "timeout" : check.spawn_error ? "blocked" : "check_fail";
    } else {
      const runArgv = [launcher, "jet", "run", "--profile=debug", "candidate.jet"];
      const run = await spawnTracked(runArgv, phaseDir, { env, input: "", timeoutMs: 180000 });
      execution.run = sourceCommandEvidence(run, runArgv, phaseDir);
      const expected = source.expected_stdout;
      const outputMatches = expected === null || run.stdout.toString("utf8") === expected;
      execution.status = run.exit_code === 0 && !run.timed_out && !run.spawn_error && !run.output_limit && outputMatches ? "pass" : "failed";
      execution.reason = execution.status === "pass" ? null : run.spawn_error || run.stderr.toString("utf8") || (outputMatches ? "generated Jet source failed jet run" : "generated Jet stdout mismatch");
      execution.failure_kind = execution.status === "pass" ? null : run.timed_out ? "timeout" : run.spawn_error ? "blocked" : outputMatches ? "run_fail" : "malformed_output";
    }
    phase.source_execution = execution;
    if (execution.status !== "pass" && result.outcome === "pass") {
      result.outcome = "wrong";
      result.failure = baseFailure("compiler", execution.failure_kind === "check_fail" ? "E_SOURCE_CHECK" : "E_SOURCE_RUN", execution.reason, phaseId, execution.failure_kind);
    }
  }
  return result;
}

async function runOne({ adapter, command, mode, domain, inputs, runDir, options, attempts }) {
  const key = publicRowKey(adapter.id, mode, domain.id);
  const ordinal = attemptOrdinal(attempts, key);
  const id = attemptId(adapter, mode, domain, ordinal);
  const attemptDir = path.join(runDir, safeAttemptDirectory(adapter, mode, domain, ordinal));
  await fs.mkdir(attemptDir, { recursive: false, mode: 0o700 });
  const context = inputs[mode === "capsule" ? "capsuleText" : "controlText"];
  const prompt = promptFor(domain, mode, context);
  const promptPath = path.join(attemptDir, "prompt.txt");
  await writeExclusive(promptPath, prompt);
  const request = JSON.stringify({
    schema: "jet.cold-agent.request.v1",
    adapter: adapter.id,
    family: adapter.family,
    task: domain.task_id,
    domain: domain.id,
    context: mode,
    prompt,
    prompt_sha256: sha256(Buffer.from(prompt, "utf8")),
  }) + "\n";
  const argv = adapter.input === "prompt-argument" ? [...command.argv, prompt] : [...command.argv];
  const env = runnerEnv(attemptDir, process.env);
  const transcript = await spawnTracked(argv, attemptDir, {
    env,
    input: adapter.input === "prompt-argument" ? "" : request,
    timeoutMs: options.timeoutMs,
  });
  const stdoutPath = path.join(attemptDir, "stdout.txt");
  const stderrPath = path.join(attemptDir, "stderr.txt");
  await writeExclusive(stdoutPath, transcript.stdout.toString("utf8"));
  await writeExclusive(stderrPath, transcript.stderr.toString("utf8"));
  const transcriptMeta = {
    schema: "jet.fresh-agent-domain.command-transcript.v1",
    argv: transcript.argv,
    argv_sha256: hashJson(transcript.argv),
    cwd: relativePath(attemptDir),
    input_mode: adapter.input,
    environment: redactedEnvironment(env, attemptDir),
    started_at: transcript.started_at,
    finished_at: transcript.finished_at,
    elapsed_ms: transcript.elapsed_ms,
    exit_code: transcript.exit_code,
    signal: transcript.signal,
    timed_out: transcript.timed_out,
    output_limit: transcript.output_limit,
    spawn_error: transcript.spawn_error,
    stdout: { path: relativePath(stdoutPath), bytes: transcript.stdout_bytes ?? transcript.stdout.length, captured_bytes: transcript.stdout.length, truncated: (transcript.stdout_bytes ?? transcript.stdout.length) > transcript.stdout.length, sha256: sha256(transcript.stdout) },
    stderr: { path: relativePath(stderrPath), bytes: transcript.stderr_bytes ?? transcript.stderr.length, captured_bytes: transcript.stderr.length, truncated: (transcript.stderr_bytes ?? transcript.stderr.length) > transcript.stderr.length, sha256: sha256(transcript.stderr) },
    resource: transcript.resource,
  };
  await writeExclusive(path.join(attemptDir, "command.json"), prettyJson(transcriptMeta));
  let result;
  if (transcript.spawn_error || transcript.timed_out || transcript.output_limit || transcript.exit_code !== 0) {
    result = emptyAttempt({ adapter, mode, domain, attemptId: id, key, task: domain, prompt, context: inputs });
    const code = transcript.spawn_error ? "E_AGENT_SPAWN" : transcript.timed_out ? "E_AGENT_TIMEOUT" : transcript.output_limit ? "E_AGENT_OUTPUT_LIMIT" : "E_AGENT_EXIT";
    const kind = transcript.spawn_error ? "blocked" : transcript.timed_out ? "timeout" : transcript.output_limit ? "malformed_output" : "run_fail";
    result.outcome = transcript.spawn_error ? "unavailable" : transcript.exit_code !== 0 ? "wrong" : "incomplete";
    result.failure = baseFailure("environment", code, transcript.spawn_error || (transcript.timed_out ? "agent command timed out" : transcript.output_limit ? "agent output exceeded capture limit" : `agent command exited ${transcript.exit_code}`), null, kind);
  } else {
    result = classifyResponse(transcript.stdout.toString("utf8"), {
      adapter, mode, domain, task: domain, prompt, context: inputs, attemptId: id, key,
    });
  }
  if (result.outcome === "pass" || Object.values(result.phases).some((phase) => phase.source)) {
    result = await executeGeneratedSources(result, attemptDir, domain, runnerEnv(attemptDir, process.env));
  }
  result.command = transcriptMeta;
  result.elapsed_ms = transcript.elapsed_ms;
  result.resource = transcript.resource;
  result.artifacts = {
    attempt_dir: relativePath(attemptDir),
    prompt: { path: relativePath(promptPath), bytes: Buffer.byteLength(prompt, "utf8"), sha256: sha256(Buffer.from(prompt, "utf8")) },
    command: { path: relativePath(path.join(attemptDir, "command.json")), bytes: Buffer.byteLength(prettyJson(transcriptMeta), "utf8"), sha256: hashJson(transcriptMeta) },
    stdout: transcriptMeta.stdout,
    stderr: transcriptMeta.stderr,
  };
  await writeExclusive(path.join(attemptDir, "result.json"), prettyJson(result));
  return result;
}

function unavailableResult({ adapter, mode, domain, inputs, category, code, message, key, attemptId: id }) {
  const context = inputs[mode === "capsule" ? "capsuleText" : "controlText"];
  const prompt = promptFor(domain, mode, context);
  const result = emptyAttempt({ adapter, mode, domain, attemptId: id, key, task: domain, prompt, context: inputs });
  result.outcome = "unavailable";
  result.failure = baseFailure(category, code, message, null, "blocked");
  result.elapsed_ms = 0;
  result.resource = emptyResource();
  return result;
}

function buildPlan({ manifest, manifestFile, adaptersFile, inputs, adapters, modes, domains, options }) {
  const identity = planIdentity({ manifest, manifestFile, adaptersFile, inputs, adapters, modes, domains });
  const rows = [];
  for (const adapter of adapters) {
    for (const mode of modes) {
      for (const domain of domains) {
        const prompt = promptFor(domain, mode, mode === "capsule" ? inputs.capsuleText : inputs.controlText);
        rows.push({
          key: publicRowKey(adapter.id, mode, domain.id),
          adapter: adapter.id,
          family: adapter.family,
          tool_version: adapter.descriptor.tool_version,
          tool_version_status: adapter.descriptor.tool_version_status,
          mode,
          domain: domain.id,
          task_id: domain.task_id,
          fixture_id: domain.fixture_id,
          task_sha256: taskIdentity(domain).prompt_contract_sha256,
          prompt_sha256: sha256(Buffer.from(prompt, "utf8")),
          context_sha256: inputs.contexts[mode].sha256,
        });
      }
    }
  }
  rows.sort((left, right) => utf8Compare(left.key, right.key));
  return {
    schema: PLAN_SCHEMA,
    schema_version: 1,
    card: manifest.card,
    run: false,
    explicit_run_flag_required: true,
    identity,
    adapters: adapters.map((adapter) => adapter.descriptor),
    contexts: modes.map((mode) => inputs.contexts[mode]),
    domains: domains.map((domain) => ({ order: domain.order, id: domain.id, slug: domain.slug, title: domain.title, fixture_id: domain.fixture_id, task_id: domain.task_id })),
    rows,
    scratch_root: options.scratch,
    output: options.output,
  };
}

async function prepare(options) {
  assertNotTmp(options.scratch, "scratch root");
  assertNotTmp(options.output, "output path");
  const manifest = normalizeCanonicalManifest(await readJson(options.manifest, "canonical DX manifest"));
  const domains = validateManifest(manifest);
  const config = await readJson(options.adapters, "adapter config");
  const allAdapters = validateAdapterConfig(config);
  validateRequiredFamilies(manifest, allAdapters);
  const wanted = new Set(selectedAdapterIds(options.adapterSelection, allAdapters));
  const selected = allAdapters.filter((adapter) => wanted.has(adapter.id));
  if (selected.length !== wanted.size) throw new UsageError(`unknown adapter in --adapter: ${[...wanted].filter((id) => !allAdapters.some((adapter) => adapter.id === id)).join(", ")}`);
  const resolved = [];
  const adapterErrors = [];
  for (const adapter of selected) {
    try {
      const command = commandFromEnvironment(adapter);
      resolved.push({
        ...adapter,
        input: adapter.command.input,
        command,
        descriptor: adapterDescriptor(adapter, command),
      });
    } catch (error) {
      adapterErrors.push({ adapter, error });
      resolved.push({
        ...adapter,
        input: adapter.command.input,
        command: null,
        descriptor: { id: adapter.id, family: adapter.family, transport: "command", input: adapter.command.input, command_source: "unavailable", command_sha256: null, model: null },
      });
    }
  }
  const inputs = await loadInputs(options);
  const adaptersFile = await fileDescriptor(options.adapters, "adapter config");
  const identity = planIdentity({ manifest, manifestFile: options.manifest, adaptersFile, inputs, adapters: resolved, modes: options.modes, domains });
  return { manifest, domains, adapters: resolved, adapterErrors, inputs, adaptersFile, identity };
}

async function observeAdapterVersions(prepared, options) {
  for (const adapter of prepared.adapters) {
    if (!adapter.command || adapter.descriptor.tool_version_status === "observed-in-config") continue;
    const argv = adapter.descriptor.version_argv;
    const transcript = await spawnTracked(argv, REPO_DIR, { env: runnerEnv(options.scratch, process.env), input: "", timeoutMs: 15_000 });
    const text = `${transcript.stdout.toString("utf8")}\n${transcript.stderr.toString("utf8")}`.trim().replace(/\s+/gu, " ");
    if (transcript.exit_code === 0 && text) {
      adapter.descriptor.tool_version = text;
      adapter.descriptor.tool_version_status = "observed";
      adapter.descriptor.tool_version_reason = null;
    } else {
      adapter.descriptor.tool_version_status = transcript.timed_out ? "timeout" : transcript.spawn_error ? "unavailable" : "failed";
      adapter.descriptor.tool_version_reason = transcript.spawn_error || (transcript.timed_out ? "version command timed out" : "version command produced no output");
    }
  }
  prepared.identity = planIdentity({ manifest: prepared.manifest, manifestFile: options.manifest, adaptersFile: prepared.adaptersFile, inputs: prepared.inputs, adapters: prepared.adapters, modes: options.modes, domains: prepared.domains });
}

function identityMatches(existing, identity) {
  return existing?.identity?.sha256 === identity.sha256;
}
async function runCampaign(options, prepared) {
  await observeAdapterVersions(prepared, options);
  const outputExists = await exists(options.output);
  let existing = null;
  if (outputExists && !options.resume) throw new SetupError(`output already exists; use --resume after checking identity: ${options.output}`);
  if (options.resume) {
    if (!outputExists) throw new SetupError(`--resume requires an existing output: ${options.output}`);
    existing = validateReport(await readJson(options.output, "scoreboard"));
  }
  const runId = existing?.run?.id ?? options.runId ?? `cold-${new Date().toISOString().replace(/[-:.TZ]/gu, "").slice(0, 14)}-${process.pid}`;
  safeId(runId, "run id");
  let runDir;
  if (existing) {
    runDir = existing.scratch?.run_dir;
    if (!runDir || isTmpPath(runDir) || !(await exists(runDir))) throw new SetupError("existing run scratch is missing; refusing to create a replacement");
    await validateRunScratch(runDir, runId, prepared.identity.sha256);
  } else {
    runDir = await createRunScratch(options.scratch, runId, prepared.identity.sha256);
  }
  const scratch = { root: options.scratch, runDir };
  let attempts = existing?.attempts ? [...existing.attempts] : [];
  let rows = existing?.rows ? [...existing.rows] : [];
  const startedAt = existing?.run?.started_at ?? new Date().toISOString();
  const run = {
    id: runId,
    started_at: startedAt,
    resumed: Boolean(existing),
    pid: process.pid,
    timeout_ms: options.timeoutMs,
    finished_at: null,
    elapsed_ms: null,
  };
  let report = makeReport({ run, identity: prepared.identity, manifest: prepared.manifest, inputs: prepared.inputs, adapters: prepared.adapters, modes: options.modes, domains: prepared.domains, scratch, rows, attempts, status: "incomplete" });
  if (!outputExists) {
    await fs.mkdir(path.dirname(options.output), { recursive: true, mode: 0o700 });
    await writeExclusive(options.output, prettyJson(report));
  }
  const existingByKey = new Map(rows.map((row) => [row.key, row]));
  const selectedAdapters = prepared.adapters;
  for (const adapter of selectedAdapters) {
    for (const mode of options.modes) {
      for (const domain of prepared.domains) {
        const publicKey = publicRowKey(adapter.id, mode, domain.id);
        const prior = existingByKey.get(publicKey);
        if (prior && (prior.outcome === "pass" || prior.outcome === "wrong")) continue;
        const ordinal = attemptOrdinal(attempts, publicKey);
        const id = attemptId(adapter, mode, domain, ordinal);
        let result;
        if (!adapter.command) {
          const issue = prepared.adapterErrors.find((item) => item.adapter.id === adapter.id)?.error;
          result = unavailableResult({ adapter, mode, domain, inputs: prepared.inputs, category: "environment", code: "E_ADAPTER_UNAVAILABLE", message: issue?.message ?? "adapter command is unavailable", key: publicKey, attemptId: id });
        } else {
          try {
            result = await runOne({ adapter, command: adapter.command, mode, domain, inputs: prepared.inputs, runDir, options, attempts });
          } catch (error) {
            result = unavailableResult({ adapter, mode, domain, inputs: prepared.inputs, category: error instanceof SetupError ? "setup" : "environment", code: error instanceof SetupError ? "E_ATTEMPT_SETUP" : "E_ATTEMPT_FAILURE", message: error.message, key: publicKey, attemptId: id });
          }
        }
        result.tool_version = adapter.descriptor.tool_version;
        result.tool_version_status = adapter.descriptor.tool_version_status;
        result.key = publicKey;
        result.adapter = { id: adapter.id, family: adapter.family, descriptor: adapter.descriptor };
        result.context.mode = mode;
        result.domain = { ...result.domain, order: domain.order, id: domain.id, title: domain.title, slug: domain.slug, fixture_id: domain.fixture_id, base_task_id: domain.base_task_id, scenario: domain.scenario };
        result.attempt_id = id;
        attempts.push(result);
        const next = new Map(rows.map((row) => [row.key, row]));
        next.set(publicKey, result);
        rows = [...next.values()];
        report = makeReport({ run, identity: prepared.identity, manifest: prepared.manifest, inputs: prepared.inputs, adapters: prepared.adapters, modes: options.modes, domains: prepared.domains, scratch, rows, attempts, status: "incomplete" });
        await writeJsonAtomic(options.output, report);
      }
    }
  }
  run.finished_at = new Date().toISOString();
  const runStart = Date.parse(run.started_at);
  run.elapsed_ms = Number.isFinite(runStart) ? Math.max(0, Date.now() - runStart) : null;
  report = makeReport({ run, identity: prepared.identity, manifest: prepared.manifest, inputs: prepared.inputs, adapters: prepared.adapters, modes: options.modes, domains: prepared.domains, scratch, rows, attempts, status: null });
  await writeJsonAtomic(options.output, report);
  return report;
}
function listPayload(domains) {
  return {
    schema: "jet.fresh-agent-domain.list.v1",
    card: "#2482",
    count: domains.length,
    domains: domains.map((domain) => ({
      order: domain.order,
      id: domain.id,
      slug: domain.slug,
      title: domain.title,
      fixture_id: domain.fixture_id,
      task_id: domain.task_id,
      base_task_id: domain.base_task_id,
      scenario: domain.scenario,
    })),
  };
}

function printPayload(payload, json) {
  if (json) process.stdout.write(prettyJson(payload));
  else process.stdout.write(payload);
}

async function check(options) {
  const prepared = await prepare(options);
  let output = null;
  if (await exists(options.output)) output = validateReport(await readJson(options.output, "scoreboard"));
  const payload = {
    schema: "jet.fresh-agent-domain.check.v1",
    card: "#2482",
    status: "ok",
    manifest: { path: relativePath(options.manifest), domains: prepared.domains.length, sha256: prepared.identity.manifest_sha256 },
    inputs: { capsule: prepared.inputs.capsule, llms: prepared.inputs.llms, control_budget_bytes: prepared.inputs.budgetBytes },
    adapters: prepared.adapters.map((adapter) => ({ ...adapter.descriptor, command_available: Boolean(adapter.command) })),
    output: output ? { path: relativePath(options.output), status: output.status, rows: output.rows.length } : { path: relativePath(options.output), status: "absent", rows: 0 },
    identity_sha256: prepared.identity.sha256,
  };
  if (options.json) printPayload(payload, true);
  else printPayload(`CHECK OK\ndomains: ${prepared.domains.length}\nadapters: ${prepared.adapters.map((adapter) => `${adapter.id}=${adapter.command ? "available" : "unavailable"}`).join(", ")}\noutput: ${payload.output.status}\n`, false);
  return 0;
}

async function dryRun(options) {
  const prepared = await prepare(options);
  const payload = buildPlan({ manifest: prepared.manifest, manifestFile: options.manifest, adaptersFile: prepared.adaptersFile, inputs: prepared.inputs, adapters: prepared.adapters, modes: options.modes, domains: prepared.domains, options });
  if (options.json) printPayload(payload, true);
  else printPayload(`DRY RUN\nidentity: ${payload.identity.sha256}\ndomains: ${prepared.domains.length}\nrows: ${payload.rows.length}\nscratch: ${options.scratch}\noutput: ${options.output}\npass --run to invoke agents\n`, false);
  return 0;
}

async function list(options) {
  const domains = await loadManifest(options.manifest);
  const payload = listPayload(domains);
  if (options.json) printPayload(payload, true);
  else printPayload(domains.map((domain) => `${String(domain.order).padStart(2, "0")} ${domain.id.padEnd(12, " ")} ${domain.title} [${domain.fixture_id}]`).join("\n") + "\n", false);
  return 0;
}

export async function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs(argv);
    if (options.action === "help") {
      process.stdout.write(usage());
      return 0;
    }
    if (options.action === "list") return await list(options);
    if (options.action === "check") return await check(options);
    if (options.action === "dry-run") return await dryRun(options);
    if (options.action === "run") {
      const prepared = await prepare(options);
      const report = await runCampaign(options, prepared);
      process.stdout.write(`${report.status} ${report.rows.length}/${report.expected_rows} rows; output ${options.output}\n`);
      return report.status === "recorded" ? 0 : report.status === "recorded-with-failures" ? 1 : UNAVAILABLE_EXIT;
    }
    throw new UsageError("no mode selected");
  } catch (error) {
    const label = error instanceof UsageError ? "USAGE" : "SETUP";
    process.stderr.write(`${label}: ${error.message}\n`);
    if (error instanceof UsageError) process.stderr.write(usage());
    return error instanceof UsageError ? USAGE_EXIT : UNAVAILABLE_EXIT;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  process.exitCode = await main();
}
