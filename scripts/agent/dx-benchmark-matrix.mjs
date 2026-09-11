#!/usr/bin/env node

/**
 * Manifest-driven E14 developer-experience measurements.
 *
 * The runner is deliberately evidence-first. A command that cannot be started,
 * a marker that is not observed, or an expected artifact that is absent is
 * represented as unmeasured or failed. It never becomes a zero or a score.
 */

import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import { constants as fsConstants, existsSync as pathExists } from "node:fs";
import os from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DEFAULT_MANIFEST_PATH = join(ROOT, "tools/agent-eval/dx/manifest.json");
const DEFAULT_OUTPUT = join(process.env.XDG_CACHE_HOME || join(os.homedir(), ".cache"), "jet-test-scratch", "dx-benchmark-matrix", "results.json");
const DEFAULT_SCRATCH = join(process.env.XDG_CACHE_HOME || join(os.homedir(), ".cache"), "jet-test-scratch", "dx-benchmark-matrix");
const SCRIPT_PATH = "scripts/agent/dx-benchmark-matrix.mjs";
const MANIFEST_SCHEMA = "jet.dx.benchmark-manifest.v1";
const SCHEMA = "jet.dx.benchmark-matrix.v2";
const SCHEMA_VERSION = 2;
const ROW_SCHEMA = "jet.dx.benchmark-row.v1";
const DIMENSION_IDS = Object.freeze(["cold", "warm", "edit", "error", "test", "debug", "package"]);
const COMPARISON_LEAVES = Object.freeze([
  "cold", "warm", "edit", "error.diagnosis", "error.repair", "error.total",
  "test.cold", "test.warm", "debug", "package.cold", "package.warm",
]);
const SAMPLE_STATES = Object.freeze(["measured", "unmeasured", "failed"]);
const ROW_STATES = Object.freeze(["pending", ...SAMPLE_STATES]);
const COMPARISON_STATES = Object.freeze(["win", "parity", "loss", "unmeasured", "failed"]);
const REQUIRED_DOMAIN_IDS = Object.freeze(["web", "web-frameworks", "mobile", "systems", "games", "backend", "data", "cli", "live", "ai-ml", "gui", "embedded"]);
const MAX_OUTPUT_BYTES = 64 * 1024;
const DEFAULT_TIMEOUT_MS = 30_000;
const RESIDENT_TIMEOUT_MS = 20_000;
const UNMEASURED = null;

const DIMENSION_DEFINITIONS = Object.freeze({
  cold: Object.freeze({ unit: "ms", lower_is_better: true }),
  warm: Object.freeze({ unit: "ms", lower_is_better: true }),
  edit: Object.freeze({ unit: "ms", lower_is_better: true }),
  error: Object.freeze({ unit: "ms", lower_is_better: true }),
  test: Object.freeze({ unit: "ms", lower_is_better: true }),
  debug: Object.freeze({ unit: "ms", lower_is_better: true }),
  package: Object.freeze({ unit: "ms", lower_is_better: true }),
});

function utf8Compare(left, right) {
  return Buffer.compare(Buffer.from(String(left), "utf8"), Buffer.from(String(right), "utf8"));
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort(utf8Compare).map((key) => [key, canonical(value[key])]));
  }
  return value;
}

export function stableJson(value) {
  return JSON.stringify(canonical(value));
}

function hashBytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

function hashJson(value) {
  return hashBytes(Buffer.from(stableJson(value), "utf8"));
}
function secretEnvironmentName(name) {
  return /(?:API[_-]?KEY|ACCESS[_-]?TOKEN|SECRET|PASSWORD|AUTH[_-]?TOKEN|PRIVATE[_-]?KEY|CREDENTIAL)/iu.test(name);
}

function environmentIdentity(env = process.env) {
  const names = Object.keys(env).sort(utf8Compare);
  const values = names.map((name) => [name, secretEnvironmentName(name) ? "<redacted>" : String(env[name])]);
  return {
    platform: process.platform,
    arch: process.arch,
    node: process.version,
    os_release: os.release(),
    cpu_model: os.cpus()[0]?.model || "unknown",
    cpu_count: os.cpus().length,
    memory_bytes: os.totalmem(),
    hostname_sha256: hashBytes(Buffer.from(os.hostname(), "utf8")),
    environment_names: names,
    environment_sha256: hashJson(values),
    secrets_redacted: names.filter(secretEnvironmentName),
  };
}

function nonEmpty(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function finiteMs(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function fail(message) {
  throw new Error(`dx benchmark matrix: ${message}`);
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function safeId(value, label) {
  assert(nonEmpty(value) && /^[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(value), `${label} must be a safe identifier`);
  return value;
}

function relativePath(value) {
  const result = relative(ROOT, value).split("\\").join("/");
  return result && !result.startsWith("../") && result !== ".." ? result : value;
}

function isTmpPath(value) {
  const absolute = resolve(String(value));
  return absolute === "/tmp" || absolute.startsWith("/tmp/");
}

function assertSafeScratch(value, label) {
  const absolute = resolve(value);
  assert(!isTmpPath(absolute), `${label} must not use /tmp`);
  const target = resolve(ROOT, "target");
  assert(absolute !== target && !absolute.startsWith(`${target}/`), `${label} must not use the compiler target`);
  return absolute;
}

async function readText(file, label) {
  try {
    return await fs.readFile(file, "utf8");
  } catch (error) {
    fail(`cannot read ${label} ${file}: ${error.message}`);
  }
}

async function readJson(file, label) {
  const text = await readText(file, label);
  try {
    return JSON.parse(text);
  } catch (error) {
    fail(`invalid JSON in ${label} ${file}: ${error.message}`);
  }
}

async function fileDescriptor(file, label) {
  const bytes = await fs.readFile(file).catch((error) => fail(`cannot read ${label} ${file}: ${error.message}`));
  return { path: relativePath(file), bytes: bytes.length, sha256: hashBytes(bytes) };
}

function manifestForHash(manifest) {
  const copy = JSON.parse(JSON.stringify(manifest));
  delete copy.__file;
  return copy;
}

export function manifestDigest(manifest) {
  return hashJson(manifestForHash(manifest));
}

function commandShape(command, label) {
  assert(command && typeof command === "object" && !Array.isArray(command), `${label} must be an object`);
  safeId(command.id, `${label}.id`);
  assert(nonEmpty(command.program), `${label}.program is missing`);
  assert(Array.isArray(command.args) && command.args.every((value) => typeof value === "string"), `${label}.args must be strings`);
  assert(nonEmpty(command.cwd), `${label}.cwd is missing`);
  assert(Number.isInteger(command.timeout_ms) && command.timeout_ms > 0, `${label}.timeout_ms is invalid`);
  if (command.marker !== undefined && command.marker !== null) assert(nonEmpty(command.marker), `${label}.marker is invalid`);
  if (command.resident !== undefined) assert(typeof command.resident === "boolean", `${label}.resident is invalid`);
}

function validateFixture(task) {
  const fixture = task.fixture;
  assert(fixture && typeof fixture === "object" && !Array.isArray(fixture), `${task.id}.fixture is missing`);
  assert(fixture.files && typeof fixture.files === "object" && !Array.isArray(fixture.files), `${task.id}.fixture.files is missing`);
  const paths = Object.entries(fixture.files);
  assert(paths.length > 0, `${task.id}.fixture.files is empty`);
  for (const [file, contents] of paths) {
    assert(nonEmpty(file) && !isAbsolute(file) && !file.split(/[\\/]/u).includes(".."), `${task.id}.fixture file path is unsafe`);
    assert(typeof contents === "string", `${task.id}.fixture file ${file} is not text`);
  }
  for (const section of ["edit", "defect", "fix"]) {
    const item = fixture[section];
    assert(item && typeof item === "object", `${task.id}.fixture.${section} is missing`);
    assert(nonEmpty(item.file) && Object.hasOwn(fixture.files, item.file), `${task.id}.fixture.${section}.file is not declared`);
    assert(nonEmpty(item.needle), `${task.id}.fixture.${section}.needle is missing`);
    assert(typeof item.replacement === "string", `${task.id}.fixture.${section}.replacement is missing`);
  }
  assert(fixture.state && typeof fixture.state === "object", `${task.id}.fixture.state is missing`);
  for (const key of ["before", "after", "reset"]) assert(nonEmpty(fixture.state[key]), `${task.id}.fixture.state.${key} is missing`);
}

function validateTask(task, index) {
  assert(task && typeof task === "object" && !Array.isArray(task), `tasks[${index}] is invalid`);
  safeId(task.id, `tasks[${index}].id`);
  safeId(task.task_id, `tasks[${index}].task_id`);
  safeId(task.fixture_id, `tasks[${index}].fixture_id`);
  assert(Number.isInteger(task.order) && task.order === index + 1, `${task.id}.order is not deterministic`);
  for (const field of ["slug", "title", "objective", "task"]) assert(nonEmpty(task[field]), `${task.id}.${field} is missing`);
  assert(Array.isArray(task.success_facts) && task.success_facts.length > 0 && task.success_facts.every(nonEmpty), `${task.id}.success_facts is invalid`);
  assert(task.markers && typeof task.markers === "object", `${task.id}.markers is missing`);
  for (const dimension of ["cold", "warm", "edit", "error", "test", "debug", "repair"]) assert(nonEmpty(task.markers[dimension]), `${task.id}.markers.${dimension} is missing`);
  assert(task.neutral && typeof task.neutral === "object", `${task.id}.neutral is missing`);
  for (const field of ["id", "input", "expected", "equivalence"]) assert(nonEmpty(task.neutral[field]), `${task.id}.neutral.${field} is missing`);
  validateFixture(task);
}

function validateReferenceApps(manifest, profileIds) {
  if (manifest.reference_apps === undefined) return;
  assert(Array.isArray(manifest.reference_apps) && manifest.reference_apps.length > 0, "reference_apps must be a non-empty array");
  const ids = new Set();
  for (const [index, app] of manifest.reference_apps.entries()) {
    const label = `reference_apps[${index}]`;
    assert(app && typeof app === "object" && !Array.isArray(app), `${label} is invalid`);
    safeId(app.id, `${label}.id`);
    assert(!ids.has(app.id), `duplicate reference app ${app.id}`);
    ids.add(app.id);
    assert(/^#\d+$/u.test(app.card || ""), `${label}.card must be a Tower card id`);
    assert(nonEmpty(app.root) && !isAbsolute(app.root) && !app.root.split(/[\\/]/u).includes(".."), `${label}.root is unsafe`);
    assert(nonEmpty(app.entry) && !isAbsolute(app.entry) && !app.entry.split(/[\\/]/u).includes(".."), `${label}.entry is unsafe`);
    assert(nonEmpty(app.target), `${label}.target is missing`);

    const source = app.source;
    assert(source && typeof source === "object" && Array.isArray(source.files) && source.files.length > 0, `${label}.source.files is missing`);
    for (const file of source.files) assert(nonEmpty(file) && !isAbsolute(file) && !file.split(/[\\/]/u).includes(".."), `${label}.source file is unsafe`);
    assert(source.loc && typeof source.loc === "object", `${label}.source.loc is missing`);
    assert(source.loc.unit === "lines" && source.loc.count === "derived" && nonEmpty(source.loc.policy), `${label}.source.loc policy is invalid`);

    const comparison = app.comparison;
    assert(comparison && typeof comparison === "object", `${label}.comparison is missing`);
    safeId(comparison.peer_profile, `${label}.comparison.peer_profile`);
    assert(!profileIds || profileIds.has(comparison.peer_profile), `${label}.comparison.peer_profile is unknown`);
    assert(comparison.peer_root === "$PROJECT", `${label}.comparison.peer_root must use the fresh project root`);
    assert(Array.isArray(comparison.peer_extensions) && comparison.peer_extensions.length > 0 && comparison.peer_extensions.every(nonEmpty), `${label}.comparison.peer_extensions is invalid`);
    assert(Array.isArray(comparison.peer_exclude) && comparison.peer_exclude.length > 0 && comparison.peer_exclude.every(nonEmpty), `${label}.comparison.peer_exclude is invalid`);
    assert(nonEmpty(comparison.loc_policy), `${label}.comparison.loc_policy is missing`);

    assert(Array.isArray(app.features) && app.features.length > 0, `${label}.features is missing`);
    for (const [featureIndex, feature] of app.features.entries()) {
      safeId(feature.id, `${label}.features[${featureIndex}].id`);
      assert(Array.isArray(feature.facts) && feature.facts.length > 0 && feature.facts.every(nonEmpty), `${label}.features[${featureIndex}].facts is invalid`);
    }
    assert(Array.isArray(app.metrics) && app.metrics.length > 0 && app.metrics.every(nonEmpty), `${label}.metrics is invalid`);
    const program = app.recorded_program;
    assert(program && typeof program === "object" && safeId(program.id, `${label}.recorded_program.id`), `${label}.recorded_program is missing`);
    assert(program.entry === app.entry && program.same_inputs === true && nonEmpty(program.source_and_workflow_contract), `${label}.recorded_program contract is incomplete`);
    const workflows = app.workflows;
    assert(workflows && typeof workflows === "object" && !Array.isArray(workflows), `${label}.workflows is missing`);
    for (const metric of ["first_run", "edit_to_see", "error_to_fix", "test_loop"]) {
      const workflow = workflows[metric];
      assert(workflow && DIMENSION_IDS.includes(workflow.dimension), `${label}.workflows.${metric}.dimension is invalid`);
      safeId(workflow.profile, `${label}.workflows.${metric}.profile`);
      assert(!profileIds || profileIds.has(workflow.profile), `${label}.workflows.${metric}.profile is unknown`);
      assert(manifest.profiles[workflow.profile]?.commands?.[workflow.command], `${label}.workflows.${metric} command is not declared by its profile`);
      assert(nonEmpty(workflow.command) && nonEmpty(workflow.cwd) && nonEmpty(workflow.marker), `${label}.workflows.${metric} is incomplete`);
    }
    if (workflows.graph) {
      const graph = workflows.graph;
      safeId(graph.profile, `${label}.workflows.graph.profile`);
      assert(profileIds.has(graph.profile), `${label}.workflows.graph.profile is unknown`);
      assert(manifest.profiles[graph.profile]?.commands?.[graph.command], `${label}.workflows.graph command is not declared by its profile`);
      assert(nonEmpty(graph.cwd) && nonEmpty(graph.marker), `${label}.workflows.graph is incomplete`);
    }

    const loop = app.edit_loop;
    assert(loop && typeof loop === "object" && nonEmpty(loop.file), `${label}.edit_loop is missing`);
    for (const section of ["edit", "restore", "failure", "repair"]) {
      const item = loop[section];
      assert(item && typeof item === "object" && nonEmpty(item.needle) && typeof item.replacement === "string", `${label}.edit_loop.${section} is invalid`);
    }
    assert(nonEmpty(loop.edit.visible_marker) && nonEmpty(loop.failure.diagnostic), `${label}.edit_loop markers are missing`);
    assert(Array.isArray(loop.last_good_facts) && loop.last_good_facts.length > 0 && loop.last_good_facts.every(nonEmpty), `${label}.edit_loop.last_good_facts is invalid`);

    const receipt = app.receipt;
    assert(receipt && typeof receipt === "object" && nonEmpty(receipt.path) && nonEmpty(receipt.schema), `${label}.receipt is missing`);
    assert(Array.isArray(receipt.required_fields) && receipt.required_fields.length > 0 && receipt.required_fields.every(nonEmpty), `${label}.receipt.required_fields is invalid`);
  }
}

function validateManifest(manifest) {
  assert(manifest && typeof manifest === "object" && !Array.isArray(manifest), "manifest must be an object");
  assert(manifest.schema === MANIFEST_SCHEMA && manifest.schema_version === 1, `manifest schema must be ${MANIFEST_SCHEMA}`);
  assert(Array.isArray(manifest.cards) && manifest.cards.includes("#2481") && manifest.cards.includes("#2482"), "manifest must cover cards #2481 and #2482");
  assert(manifest.source && nonEmpty(manifest.source.id) && nonEmpty(manifest.source.authority) && nonEmpty(manifest.source.command_environment), "manifest source is incomplete");
  const policy = manifest.policy;
  assert(policy && typeof policy === "object", "manifest policy is missing");
  assert(Array.isArray(policy.dimensions) && stableJson(policy.dimensions) === stableJson(DIMENSION_IDS), "manifest dimensions are not in canonical order");
  assert(policy.sample_policy && typeof policy.sample_policy === "object", "manifest sample_policy is missing");
  for (const dimension of DIMENSION_IDS) assert(Number.isInteger(policy.sample_policy[dimension]) && policy.sample_policy[dimension] >= 1, `sample count for ${dimension} is invalid`);
  assert(Number.isInteger(policy.max_samples) && policy.max_samples >= 1 && policy.max_samples <= 10, "manifest max_samples is invalid");
  assert(policy.neutral_matching?.rule && Array.isArray(policy.neutral_matching.required_task_fields), "manifest neutral matching contract is missing");
  assert(policy.no_score_without_evidence === true, "manifest must forbid scores without evidence");
  assert(stableJson(policy.outcomes) === stableJson(["measured", "unmeasured", "failed", "pending"]), "manifest outcomes are invalid");
  assert(policy.comparison && Number.isFinite(policy.comparison.parity_ratio_low) && Number.isFinite(policy.comparison.parity_ratio_high), "manifest comparator policy is missing");

  assert(manifest.default_workflows && typeof manifest.default_workflows === "object", "manifest default_workflows is missing");
  for (const dimension of DIMENSION_IDS) {
    const workflow = manifest.default_workflows[dimension];
    assert(workflow && Array.isArray(workflow.setup) && workflow.setup.every((value) => typeof value === "string"), `${dimension} default workflow is invalid`);
    assert(nonEmpty(workflow.command), `${dimension} default workflow command is missing`);
    if (workflow.two_passes !== undefined) assert(typeof workflow.two_passes === "boolean", `${dimension} two_passes is invalid`);
  }

  assert(Array.isArray(manifest.tasks) && manifest.tasks.length >= REQUIRED_DOMAIN_IDS.length, "manifest must contain every required benchmark task");
  const tasks = new Map();
  const taskIds = new Set();
  const fixtureIds = new Set();
  for (const [index, task] of manifest.tasks.entries()) {
    validateTask(task, index);
    assert(!tasks.has(task.id), `duplicate task ${task.id}`);
    assert(!taskIds.has(task.task_id), `duplicate task_id ${task.task_id}`);
    assert(!fixtureIds.has(task.fixture_id), `duplicate fixture_id ${task.fixture_id}`);
    tasks.set(task.id, task);
    taskIds.add(task.task_id);
    fixtureIds.add(task.fixture_id);
  }

  assert(manifest.profiles && typeof manifest.profiles === "object" && !Array.isArray(manifest.profiles), "manifest profiles are missing");
  const profileIds = new Set();
  for (const [profileId, profile] of Object.entries(manifest.profiles)) {
    safeId(profileId, `profile ${profileId}`);
    assert(!profileIds.has(profileId), `duplicate profile ${profileId}`);
    profileIds.add(profileId);
    for (const field of ["label", "kind", "executable"]) assert(nonEmpty(profile[field]), `profile ${profileId}.${field} is missing`);
    assert(profile.kind === "jet" || profile.kind === "peer", `profile ${profileId}.kind is invalid`);
    assert(Array.isArray(profile.version_args) && profile.version_args.every((value) => typeof value === "string"), `profile ${profileId}.version_args is invalid`);
    assert(profile.commands && typeof profile.commands === "object", `profile ${profileId}.commands are missing`);
    const commands = new Set();
    for (const [commandId, command] of Object.entries(profile.commands)) {
      commandShape({ id: commandId, ...command }, `profile ${profileId}.commands.${commandId}`);
      commands.add(commandId);
    }
    const support = profile.workflow_support || {};
    for (const [dimension, rule] of Object.entries(support)) {
      assert(DIMENSION_IDS.includes(dimension), `profile ${profileId} has unknown workflow ${dimension}`);
      assert(rule && typeof rule === "object" && typeof rule.supported === "boolean", `profile ${profileId}.${dimension} support is invalid`);
      if (!rule.supported) assert(nonEmpty(rule.reason), `profile ${profileId}.${dimension} unavailable reason is missing`);
    }
    for (const dimension of DIMENSION_IDS) {
      const rule = support[dimension];
      if (rule?.supported === false) continue;
      assert(commands.has(manifest.default_workflows[dimension].command), `profile ${profileId} lacks ${dimension} command`);
    }
    if (!profile.workflow_support?.package || profile.workflow_support.package.supported !== false) assert(nonEmpty(profile.artifact), `profile ${profileId} package artifact is missing`);
    if (profile.artifact !== undefined) assert(nonEmpty(profile.artifact) && !isAbsolute(profile.artifact) && !profile.artifact.split("/").includes(".."), `profile ${profileId}.artifact is unsafe`);
  }
  validateReferenceApps(manifest, profileIds);

  assert(Array.isArray(manifest.domains) && manifest.domains.length >= REQUIRED_DOMAIN_IDS.length, "manifest must contain every required benchmark domain");
  const domains = new Set();
  for (const [index, domain] of manifest.domains.entries()) {
    assert(domain && typeof domain === "object", `domains[${index}] is invalid`);
    assert(Number.isInteger(domain.order) && domain.order === index + 1, `domain order drifted at ${index}`);
    safeId(domain.id, `domains[${index}].id`);
    assert(!domains.has(domain.id), `duplicate domain ${domain.id}`);
    domains.add(domain.id);
    assert(tasks.has(domain.task_id), `${domain.id} references unknown task ${domain.task_id}`);
    assert(nonEmpty(domain.slug) && nonEmpty(domain.title), `${domain.id} title or slug is missing`);
    assert(domain.required === true, `${domain.id} must be required`);
    safeId(domain.benchmark_peer, `${domain.id}.benchmark_peer`);
    assert(profileIds.has(domain.benchmark_peer), `${domain.id}.benchmark_peer is unknown`);
    assert(Array.isArray(domain.comparators) && domain.comparators.length >= 2, `${domain.id}.comparators must contain Jet and a peer`);
    const seen = new Set();
    for (const [participantIndex, participant] of domain.comparators.entries()) {
      assert(participant && typeof participant === "object", `${domain.id}.comparators[${participantIndex}] is invalid`);
      safeId(participant.profile, `${domain.id}.comparators[${participantIndex}].profile`);
      assert(profileIds.has(participant.profile), `${domain.id} references unknown profile ${participant.profile}`);
      assert(!seen.has(participant.profile), `${domain.id} repeats profile ${participant.profile}`);
      seen.add(participant.profile);
      if (participant.target !== undefined) assert(typeof participant.target === "string", `${domain.id} target must be text`);
      if (participantIndex === 0) assert(participant.profile === "jet" && manifest.profiles.jet.kind === "jet", `${domain.id} must put Jet first`);
      else assert(manifest.profiles[participant.profile].kind === "peer", `${domain.id} peer slot is not a peer profile`);
    }
    assert(seen.has(domain.benchmark_peer), `${domain.id} benchmark peer is not in comparators`);
  }
  for (const requiredId of REQUIRED_DOMAIN_IDS) assert(domains.has(requiredId), `manifest is missing required domain ${requiredId}`);

  const fresh = manifest.fresh_agent;
  assert(fresh && fresh.card === "#2482" && fresh.runner === "scripts/agent/fresh-agent-domain-run.mjs", "fresh-agent campaign is not integrated in the canonical manifest");
  assert(fresh.context && stableJson(fresh.context.modes) === stableJson(["capsule", "control"]), "fresh-agent context contract is invalid");
  assert(fresh.adapters && fresh.adapters.transport === "command" && Array.isArray(fresh.adapters.required_families), "fresh-agent adapter contract is invalid");
  assert(fresh.protocol && fresh.protocol.fresh_process === true && fresh.protocol.hidden_context === false, "fresh-agent process contract is invalid");
  assert(Array.isArray(fresh.domains) && fresh.domains.length >= REQUIRED_DOMAIN_IDS.length, "fresh-agent must reference every benchmark domain");
  for (const [index, domain] of fresh.domains.entries()) {
    assert(Number.isInteger(domain.order) && domain.order === index + 1, "fresh-agent domain order is invalid");
    assert(tasks.has(domain.task_id), `fresh-agent references unknown task ${domain.task_id}`);
  }
  const freshIds = new Set(fresh.domains.map((domain) => domain.id));
  for (const requiredId of REQUIRED_DOMAIN_IDS) assert(freshIds.has(requiredId), `fresh-agent is missing required domain ${requiredId}`);
  return true;
}

async function loadManifest(file = DEFAULT_MANIFEST_PATH) {
  const manifest = await readJson(resolve(file), "manifest");
  validateManifest(manifest);
  return { ...manifest, __file: resolve(file) };
}

function commandDisplay(program, args) {
  return [program, ...args].map((value) => /^[A-Za-z0-9_./:@=+,-]+$/u.test(value) ? value : JSON.stringify(value)).join(" ");
}

function hiddenEnvironmentName(name) {
  return /^(?:OMP|JET)_(?:SESSION|TOOLS|SKILLS|RULES|EXTENSIONS|CONTEXT|SYSTEM_PROMPT|MAINTAINER_CONTEXT|AGENT_CONTEXT|HIDDEN_CONTEXT)$/iu.test(name)
    || /(?:SYSTEM_PROMPT|MAINTAINER_CONTEXT|AGENT_CONTEXT|HIDDEN_CONTEXT)/iu.test(name);
}

function envForCommand(scratch) {
  const clean = Object.fromEntries(Object.entries(process.env).filter(([name]) => !hiddenEnvironmentName(name) && !secretEnvironmentName(name)));
  return {
    ...clean,
    CI: "1",
    NO_COLOR: "1",
    TERM: "dumb",
    TZ: "UTC",
    LC_ALL: "C",
    TMPDIR: scratch,
    TMP: scratch,
    TEMP: scratch,
    JET_TEST_SCRATCH: scratch,
    JET_TEST_SCRATCH_DIR: scratch,
    JET_DX_BENCHMARK: "1",
  };
}
function locateExecutable(program) {
  if (program.includes("/") || program.startsWith(".")) {
    const candidate = isAbsolute(program) ? program : resolve(ROOT, program);
    return pathExists(candidate, fsConstants.X_OK) ? candidate : null;
  }
  try {
    const result = spawnSync("which", [program], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    return result.status === 0 ? result.stdout.trim().split("\n")[0] || null : null;
  } catch {
    return null;
  }
}

function resolveProgram(program, cwd) {
  if (!program.includes("/") && !program.startsWith(".")) return locateExecutable(program);
  const candidate = isAbsolute(program)
    ? program
    : program.startsWith("scripts/")
      ? resolve(ROOT, program)
      : resolve(cwd || ROOT, program);
  try {
    return pathExists(candidate, fsConstants.X_OK) ? candidate : null;
  } catch {
    return null;
  }
}

function monoNow() {
  return process.hrtime.bigint();
}

function elapsedMs(start) {
  return Math.round(Number(monoNow() - start) / 100_000) / 10;
}

function boundedText(value) {
  const bytes = Buffer.from(String(value || ""), "utf8");
  if (bytes.length <= MAX_OUTPUT_BYTES) return bytes.toString("utf8");
  return Buffer.concat([Buffer.from("…", "utf8"), bytes.subarray(Math.max(0, bytes.length - MAX_OUTPUT_BYTES + 3))]).toString("utf8");
}

async function rssBytes(pid) {
  if (!pid || process.platform !== "linux") return { value: UNMEASURED, reason: `RSS is not available on ${process.platform}` };
  try {
    const text = await fs.readFile(`/proc/${pid}/status`, "utf8");
    const match = text.match(/^VmHWM:\s+(\d+)\s+kB$/mu) || text.match(/^VmRSS:\s+(\d+)\s+kB$/mu);
    return match ? { value: Number(match[1]) * 1024, reason: "linux /proc status" } : { value: UNMEASURED, reason: "linux /proc has no RSS field" };
  } catch (error) {
    return { value: UNMEASURED, reason: `RSS unavailable: ${error.code || error.message}` };
  }
}

function stopChild(child, signal = "SIGTERM") {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  try {
    if (process.platform !== "win32" && child.pid) process.kill(-child.pid, signal);
    else child.kill(signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

async function runCommand(program, args, options = {}) {
  const startedAt = new Date().toISOString();
  const started = monoNow();
  const timeoutMs = options.timeout_ms || DEFAULT_TIMEOUT_MS;
  const resolved = resolveProgram(program, options.cwd);
  const base = {
    program,
    resolved_program: resolved,
    args: [...args],
    display: commandDisplay(program, args),
    cwd: options.cwd || ROOT,
    started_at: startedAt,
  };
  if (!resolved) {
    return {
      status: "unavailable",
      command: base,
      process: { pid: null, exit_code: null, signal: null, timed_out: false, marker_reached: false, wall_ms: 0, first_output_ms: null, peak_rss_bytes: null, rss_status: "unavailable", rss_reason: "executable or local command was not found", stdout_bytes: 0, stderr_bytes: 0 },
      stdout: "",
      stderr: "",
      error: `executable not found: ${program}`,
      ended_at: new Date().toISOString(),
    };
  }

  let child;
  try {
    child = spawn(resolved, args, { cwd: options.cwd || ROOT, env: options.env || envForCommand(options.cwd || ROOT), detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"] });
  } catch (error) {
    return {
      status: "unavailable",
      command: { ...base, resolved_program: resolved },
      process: { pid: null, exit_code: null, signal: null, timed_out: false, marker_reached: false, wall_ms: elapsedMs(started), first_output_ms: null, peak_rss_bytes: null, rss_status: "unavailable", rss_reason: error.message, stdout_bytes: 0, stderr_bytes: 0 },
      stdout: "",
      stderr: "",
      error: error.message,
      ended_at: new Date().toISOString(),
    };
  }

  let stdout = "";
  let stderr = "";
  let stdoutBytes = 0;
  let stderrBytes = 0;
  let outputLimit = false;
  let firstOutputMs = null;
  let childError = null;
  let markerReached = false;
  let peakRss = UNMEASURED;
  let rssReason = "RSS was not sampled";
  const marker = options.marker || null;
  let markerPattern = null;
  if (marker) {
    try { markerPattern = new RegExp(marker, "u"); } catch { markerPattern = null; }
  }
  const observeMarker = () => {
    if (markerReached || !marker) return;
    const output = `${stdout}\n${stderr}`;
    markerReached = markerPattern ? markerPattern.test(output) : output.includes(marker);
  };
  const onOutput = (target, chunk) => {
    if (firstOutputMs === null) firstOutputMs = elapsedMs(started);
    const bytes = Buffer.byteLength(String(chunk), "utf8");
    if (target === "stdout") {
      stdoutBytes += bytes;
      stdout = boundedText(`${stdout}${chunk}`);
    } else {
      stderrBytes += bytes;
      stderr = boundedText(`${stderr}${chunk}`);
    }
    outputLimit ||= stdoutBytes > MAX_OUTPUT_BYTES || stderrBytes > MAX_OUTPUT_BYTES;
    if (outputLimit) stopChild(child);
    observeMarker();
  };
  child.stdout?.setEncoding("utf8");
  child.stderr?.setEncoding("utf8");
  child.stdout?.on("data", (chunk) => onOutput("stdout", chunk));
  child.stderr?.on("data", (chunk) => onOutput("stderr", chunk));
  child.once("error", (error) => { childError = error; });
  const exited = new Promise((resolveExit) => child.once("close", (code, signal) => resolveExit({ code, signal })));
  const sampleRss = async () => {
    const sample = await rssBytes(child.pid);
    if (sample.value !== UNMEASURED && (peakRss === UNMEASURED || sample.value > peakRss)) peakRss = sample.value;
    rssReason = sample.reason;
  };
  await sampleRss();
  const rssTimer = setInterval(() => { sampleRss().catch(() => {}); }, 25);
  const timeout = new Promise((resolveTimeout) => {
    const timer = setTimeout(() => resolveTimeout({ timed_out: true }), timeoutMs);
    timer.unref?.();
  });
  const markerStop = marker
    ? new Promise((resolveMarker) => {
      const timer = setInterval(() => {
        observeMarker();
        if (markerReached) {
          clearInterval(timer);
          resolveMarker({ marker_reached: true });
        }
      }, 5);
      timer.unref?.();
    })
    : null;
  const outcome = await Promise.race([exited, timeout, ...(markerStop && options.stop_on_marker !== false ? [markerStop] : [])]);
  let processResult;
  if (outcome.marker_reached) {
    stopChild(child);
    await Promise.race([exited, new Promise((resolveExit) => setTimeout(resolveExit, 1_000))]);
    stopChild(child, "SIGKILL");
    processResult = { code: 0, signal: null, timed_out: false };
  } else if (outcome.timed_out) {
    stopChild(child);
    await Promise.race([exited, new Promise((resolveExit) => setTimeout(resolveExit, 1_000))]);
    stopChild(child, "SIGKILL");
    processResult = { code: null, signal: "SIGTERM", timed_out: true };
  } else {
    processResult = { code: outcome.code, signal: outcome.signal, timed_out: false };
  }
  clearInterval(rssTimer);
  await sampleRss();
  const markerSatisfied = !marker || markerReached;
  const endedAt = new Date().toISOString();
  let status = childError ? "unmeasured" : outputLimit ? "failed" : processResult.timed_out ? "failed" : processResult.code === 0 ? "ok" : "failed";
  if (options.expect_failure && marker && markerReached && processResult.code !== 0 && !processResult.timed_out && !outputLimit) status = "ok";
  if (status === "ok" && !markerSatisfied) status = "failed";
  const error = childError
    ? childError.message
    : outputLimit
      ? `command output exceeded ${MAX_OUTPUT_BYTES} byte capture limit`
      : processResult.timed_out
        ? `command timed out after ${timeoutMs}ms`
        : status === "failed" && !markerSatisfied
          ? `expected marker was not observed: ${marker}`
          : status === "failed"
            ? `command exited ${processResult.code ?? processResult.signal}`
            : null;
  return {
    status,
    command: { ...base, resolved_program: resolved, ended_at: endedAt },
    process: { pid: child.pid || null, exit_code: processResult.code, signal: processResult.signal || null, timed_out: processResult.timed_out, output_limit: outputLimit, marker_reached: markerReached, wall_ms: elapsedMs(started), first_output_ms: firstOutputMs, peak_rss_bytes: peakRss, rss_status: peakRss === UNMEASURED ? "unavailable" : "measured", rss_reason: rssReason, stdout_bytes: stdoutBytes, stderr_bytes: stderrBytes, stdout_captured_bytes: Buffer.byteLength(stdout, "utf8"), stderr_captured_bytes: Buffer.byteLength(stderr, "utf8"), stdout_truncated: stdoutBytes > Buffer.byteLength(stdout, "utf8"), stderr_truncated: stderrBytes > Buffer.byteLength(stderr, "utf8") },
    stdout,
    stderr,
    error,
    marker: { expected: marker, observed: markerReached, text: markerReached ? marker : null },
    ended_at: endedAt,
  };
}

function render(value, context) {
  return String(value)
    .replaceAll("$CELL", context.cellRoot)
    .replaceAll("$PROJECT", context.projectRoot)
    .replaceAll("$FIXTURE", context.fixtureRoot)
    .replaceAll("$ROOT", ROOT)
    .replaceAll("$TARGET", context.target || "")
    .replaceAll("$ARTIFACT", context.artifact || "");
}

function commandFor(participant, command, context) {
  const cwdValue = render(command.cwd || "$PROJECT", context);
  const cwd = isAbsolute(cwdValue) ? cwdValue : resolve(context.projectRoot, cwdValue);
  const programValue = render(command.program, context);
  const program = programValue.includes("/") || programValue.startsWith(".")
    ? (programValue.startsWith("scripts/") ? resolve(ROOT, programValue) : resolve(cwd, programValue))
    : programValue;
  const args = command.args.map((value) => render(value, context));
  return { program, args, cwd, timeout_ms: command.resident ? RESIDENT_TIMEOUT_MS : command.timeout_ms, resident: command.resident === true, participant };
}

async function execute(participant, commandId, context, marker = null, options = {}) {
  const command = participant.commands[commandId];
  if (!command) {
    return { status: "unavailable", command: { id: commandId, program: null, args: [], cwd: context.projectRoot }, process: { pid: null, exit_code: null, signal: null, timed_out: false, marker_reached: false, wall_ms: 0, first_output_ms: null, peak_rss_bytes: null, rss_status: "unavailable", rss_reason: "command is not declared" }, stdout: "", stderr: "", error: `command is not declared: ${commandId}`, marker: { expected: marker, observed: false, text: null }, ended_at: new Date().toISOString() };
  }
  const invocation = commandFor(participant, command, context);
  const result = await runCommand(invocation.program, invocation.args, { cwd: invocation.cwd, timeout_ms: invocation.timeout_ms, env: envForCommand(context.cellRoot), marker, stop_on_marker: options.stop_on_marker ?? true, expect_failure: options.expect_failure === true });
  return { ...result, id: commandId, label: command.label || commandId, command: { id: commandId, ...result.command, args: invocation.args, cwd: relativePath(invocation.cwd), display: commandDisplay(invocation.program, invocation.args) } };
}

function commandEvidence(results) {
  return results.map((result) => ({
    id: result.id || result.command?.id || null,
    label: result.label || result.id || null,
    status: result.status,
    command: result.command || null,
    process: result.process || null,
    marker: result.marker || null,
    stdout: result.stdout,
    stderr: result.stderr,
    stdout_sha256: hashBytes(Buffer.from(result.stdout || "", "utf8")),
    stderr_sha256: hashBytes(Buffer.from(result.stderr || "", "utf8")),
    error: result.error || null,
    ended_at: result.ended_at || null,
  }));
}

function sampleState(result) {
  if (!result) return "unmeasured";
  if (result.status === "ok") return "measured";
  return result.status === "unavailable" ? "unmeasured" : "failed";
}

function sample(id, path, state, value, results, marker, reason = null, extra = {}) {
  const startedAt = results[0]?.command?.started_at || new Date().toISOString();
  const endedAt = results.at(-1)?.ended_at || new Date().toISOString();
  const output = results.map((result) => `${result.stdout || ""}\n${result.stderr || ""}`).join("\n");
  return {
    sample_id: id,
    path,
    status: state,
    value_ms: state === "measured" && finiteMs(value) ? value : UNMEASURED,
    timestamps: { started_at: startedAt, ended_at: endedAt },
    commands: commandEvidence(results),
    process: results.map((result) => result.process).filter(Boolean),
    marker: { expected: marker || null, observed: Boolean(marker && results.some((result) => result.marker?.observed)), text: marker && output.includes(marker) ? marker : null },
    evidence_sha256: hashBytes(Buffer.from(output, "utf8")),
    reason: state === "measured" ? null : reason || results.find((result) => result.error)?.error || `sample ${state}`,
    ...extra,
  };
}

function median(values) {
  if (values.length === 0) return UNMEASURED;
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function percentile(values, fraction) {
  if (values.length === 0) return UNMEASURED;
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * fraction) - 1))];
}

function summarizeNumeric(samples, unit = "ms", selector = (item) => item.value_ms) {
  const values = samples.filter((item) => item.status === "measured").map(selector).filter((value) => typeof value === "number" && Number.isFinite(value));
  const state = samples.some((item) => item.status === "failed") ? "failed" : samples.length === 0 || samples.some((item) => item.status === "unmeasured") || values.length === 0 ? "unmeasured" : "measured";
  return { status: state, unit, sample_count: samples.length, measured_sample_count: values.length, failed_sample_count: samples.filter((item) => item.status === "failed").length, unmeasured_sample_count: samples.filter((item) => item.status === "unmeasured").length, values: [...values].sort((left, right) => left - right), p50: state === "measured" ? median(values) : UNMEASURED, p95: state === "measured" ? percentile(values, 0.95) : UNMEASURED, samples };
}

function summarizeError(samples) {
  const state = samples.some((item) => item.status === "failed") ? "failed" : samples.length === 0 || samples.some((item) => item.status === "unmeasured") ? "unmeasured" : "measured";
  const numeric = (field) => summarizeNumeric(samples, "ms", (item) => item[field]);
  return { status: state, diagnosis: numeric("diagnosis_ms"), repair: numeric("repair_ms"), total: numeric("total_ms"), samples };
}

function emptySummary(unit, reason, count = 1) {
  const samples = Array.from({ length: count }, (_, index) => sample(`${unit}-${String(index + 1).padStart(2, "0")}`, unit, "unmeasured", UNMEASURED, [], null, reason));
  return summarizeNumeric(samples, unit, () => UNMEASURED);
}

function emptyError(reason) {
  return summarizeError([sample("error-01", "error", "unmeasured", UNMEASURED, [], null, reason, { diagnosis_ms: UNMEASURED, repair_ms: UNMEASURED, total_ms: UNMEASURED })]);
}

function sourceLoc(text) {
  return String(text).split(/\r?\n/u).filter((line) => /\S/u.test(line) && !/^\s*\/\//u.test(line)).length;
}

function excludedPeerPath(relativePath, comparison) {
  const parts = relativePath.split("/");
  return comparison.peer_exclude.some((excluded) => parts.includes(excluded));
}

async function walkFiles(root, comparison) {
  const files = [];
  const walk = async (directory, prefix = "") => {
    let entries;
    try {
      entries = await fs.readdir(directory, { withFileTypes: true });
    } catch {
      return;
    }
    entries.sort((left, right) => utf8Compare(left.name, right.name));
    for (const entry of entries) {
      const relativeName = prefix ? `${prefix}/${entry.name}` : entry.name;
      const target = join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!excludedPeerPath(relativeName, comparison)) await walk(target, relativeName);
      } else if (entry.isFile() && comparison.peer_extensions.includes(`.${entry.name.split(".").at(-1)}`)) {
        if (!excludedPeerPath(relativeName, comparison)) files.push({ relative: relativeName, absolute: target });
      }
    }
  };
  await walk(root);
  return files;
}

async function sourceInventory(root, files, comparison = null) {
  const candidates = files
    ? files.map((file) => ({ relative: file, absolute: resolve(root, file) }))
    : await walkFiles(root, comparison);
  const descriptors = [];
  let loc = 0;
  for (const candidate of candidates) {
    try {
      const text = await fs.readFile(candidate.absolute, "utf8");
      const bytes = Buffer.byteLength(text, "utf8");
      descriptors.push({ path: candidate.relative, bytes, sha256: hashBytes(Buffer.from(text, "utf8")), loc: sourceLoc(text) });
      loc += sourceLoc(text);
    } catch (error) {
      return { status: "unmeasured", reason: `source inventory unavailable: ${candidate.relative}: ${error.message}`, files: descriptors, loc: null, sha256: null };
    }
  }
  return { status: "measured", files: descriptors, loc, sha256: hashJson(descriptors) };
}

function referenceMetric(metric, status, value, reason = null, extra = {}) {
  return { metric, status, value: status === "measured" ? value : null, unit: metric === "lines_of_code" ? "lines" : "ms", reason: status === "measured" ? null : reason, ...extra };
}

function unavailableReferenceMetrics(metrics, reason) {
  return Object.fromEntries(metrics.map((metric) => [metric, referenceMetric(metric, "unmeasured", null, reason)]));
}

async function fixturePath(root, file) {
  const target = resolve(root, file);
  assert(target === resolve(root) || target.startsWith(`${resolve(root)}/`), `fixture path escaped project: ${file}`);
  return target;
}

async function writeFixture(root, fixture) {
  for (const [file, contents] of Object.entries(fixture.files)) {
    const target = await fixturePath(root, file);
    await fs.mkdir(dirname(target), { recursive: true });
    await fs.writeFile(target, contents, "utf8");
  }
}

async function replaceFixture(root, edit) {
  const target = await fixturePath(root, edit.file);
  let current;
  try { current = await fs.readFile(target, "utf8"); } catch (error) { return { ok: false, reason: `fixture file is unavailable: ${error.message}` }; }
  const first = current.indexOf(edit.needle);
  if (first < 0) return { ok: false, reason: `fixture needle not found: ${edit.file}` };
  if (current.indexOf(edit.needle, first + edit.needle.length) >= 0) return { ok: false, reason: `fixture needle is not unique: ${edit.file}` };
  await fs.writeFile(target, current.slice(0, first) + edit.replacement + current.slice(first + edit.needle.length), "utf8");
  return { ok: true };
}

function workflowFor(manifest, participant, dimension) {
  const base = manifest.default_workflows[dimension];
  const override = participant.workflow_overrides?.[dimension] || {};
  const support = participant.workflow_support?.[dimension];
  return { ...base, ...override, ...(support ? { supported: support.supported, reason: support.reason } : {}) };
}

function fixtureFor(participant, task) {
  const fixture = participant.fixture || task.fixtures?.[participant.id] || task.fixtures?.[participant.profile_id] || task.fixture_by_profile?.[participant.profile_id];
  if (!fixture || typeof fixture !== "object" || !fixture.files || typeof fixture.files !== "object") return null;
  return fixture;
}

function noFixtureReason(participant, task) {
  return `${participant.id} has no native fixture adapter for ${task.id}; generic task.fixture fallback is disabled`;
}

function participantFor(manifest, domain, descriptor) {
  const profile = manifest.profiles[descriptor.profile];
  const support = { ...(profile.workflow_support || {}), ...(descriptor.workflow_support || {}) };
  const target = descriptor.target ?? profile.target_by_domain?.[domain.id] ?? "";
  return {
    ...profile,
    ...descriptor,
    id: descriptor.id || descriptor.profile,
    profile_id: descriptor.profile,
    target,
    workflow_support: support,
    commands: profile.commands,
    artifact: descriptor.artifact || profile.artifact || null,
    fixture: descriptor.fixture || profile.fixture_by_domain?.[domain.id] || profile.fixture || null,
    label: descriptor.label || profile.label,
    kind: descriptor.kind || profile.kind,
  };
}

async function setupParticipant(participant, task, context, workflow, results) {
  await fs.mkdir(context.projectRoot, { recursive: true });
  const fixture = fixtureFor(participant, task);
  if (!fixture) return { ok: false, state: "unmeasured", reason: noFixtureReason(participant, task), fixture: null };
  for (const commandId of workflow.setup || []) {
    const result = await execute(participant, commandId, context);
    results.push(result);
    if (result.status !== "ok") return { ok: false, state: sampleState(result), reason: result.error || `setup command ${commandId} did not complete`, fixture };
  }
  try {
    await writeFixture(context.projectRoot, fixture);
    return { ok: true, state: "measured", reason: null, fixture };
  } catch (error) {
    return { ok: false, state: "failed", reason: `fixture setup failed: ${error.message}`, fixture };
  }
}

function setupSamples(count, path, state, reason, results, marker = null) {
  return Array.from({ length: count }, (_, index) => sample(`${path}-${String(index + 1).padStart(2, "0")}`, path, state, UNMEASURED, results, marker, reason));
}

async function measureCold(participant, task, runRoot, count, options) {
  const workflow = workflowFor(options.manifest, participant, "cold");
  if (workflow.supported === false) return emptySummary("ms", workflow.reason, count);
  if (options.dryRun) return emptySummary("ms", "dry-run requested", count);
  const samples = [];
  for (let index = 0; index < count; index += 1) {
    const id = `cold-${String(index + 1).padStart(2, "0")}`;
    const cellRoot = await fs.mkdtemp(join(runRoot, `${participant.id}-cold-`));
    const context = { cellRoot, projectRoot: join(cellRoot, "project"), fixtureRoot: join(cellRoot, "project"), displayCell: relativePath(cellRoot), target: participant.target, artifact: participant.artifact };
    const started = monoNow();
    const results = [];
    try {
      const setup = await setupParticipant(participant, task, context, workflow, results);
      if (!setup.ok) {
        samples.push(...setupSamples(1, "cold", setup.state, setup.reason, results, task.markers.cold).map((item) => ({ ...item, sample_id: id })));
        continue;
      }
      const result = await execute(participant, workflow.command, context, task.markers.cold, workflow);
      results.push(result);
      const state = sampleState(result);
      samples.push(sample(id, "cold", state, state === "measured" ? elapsedMs(started) : UNMEASURED, results, task.markers.cold));
    } finally {
      if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
    }
  }
  return summarizeNumeric(samples, "ms");
}

async function measureWarm(participant, task, runRoot, count, options) {
  const workflow = workflowFor(options.manifest, participant, "warm");
  if (workflow.supported === false) return emptySummary("ms", workflow.reason, count);
  if (options.dryRun) return emptySummary("ms", "dry-run requested", count);
  const cellRoot = await fs.mkdtemp(join(runRoot, `${participant.id}-warm-`));
  const context = { cellRoot, projectRoot: join(cellRoot, "project"), fixtureRoot: join(cellRoot, "project"), displayCell: relativePath(cellRoot), target: participant.target, artifact: participant.artifact };
  const results = [];
  const samples = [];
  try {
    const setup = await setupParticipant(participant, task, context, workflow, results);
    if (!setup.ok) return summarizeNumeric(setupSamples(count, "warm", setup.state, setup.reason, results, task.markers.warm), "ms");
    for (let index = 0; index < count; index += 1) {
      const result = await execute(participant, workflow.command, context, task.markers.warm, workflow);
      const state = sampleState(result);
      samples.push(sample(`warm-${String(index + 1).padStart(2, "0")}`, "warm", state, state === "measured" ? result.process.wall_ms : UNMEASURED, [...results, result], task.markers.warm));
      results.push(result);
    }
  } finally {
    if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
  }
  return summarizeNumeric(samples, "ms");
}

async function measureEdit(participant, task, runRoot, count, options) {
  const workflow = workflowFor(options.manifest, participant, "edit");
  if (workflow.supported === false) return emptySummary("ms", workflow.reason, count);
  if (options.dryRun) return emptySummary("ms", "dry-run requested", count);
  const cellRoot = await fs.mkdtemp(join(runRoot, `${participant.id}-edit-`));
  const context = { cellRoot, projectRoot: join(cellRoot, "project"), fixtureRoot: join(cellRoot, "project"), displayCell: relativePath(cellRoot), target: participant.target, artifact: participant.artifact };
  const setupResults = [];
  const samples = [];
  try {
    const setup = await setupParticipant(participant, task, context, workflow, setupResults);
    if (!setup.ok) return summarizeNumeric(setupSamples(count, "edit", setup.state, setup.reason, setupResults, task.markers.edit), "ms");
    const fixture = setup.fixture;
    if (!fixture.edit || !fixture.edit.file || !nonEmptyString(fixture.edit.needle) || !nonEmptyString(fixture.edit.replacement)) {
      return summarizeNumeric(setupSamples(count, "edit", "unmeasured", "native fixture has no deterministic edit operation", setupResults, task.markers.edit), "ms");
    }
    for (let index = 0; index < count; index += 1) {
      const id = `edit-${String(index + 1).padStart(2, "0")}`;
      const applied = await replaceFixture(context.projectRoot, fixture.edit);
      if (!applied.ok) {
        samples.push(sample(id, "edit", "failed", UNMEASURED, setupResults, task.markers.edit, applied.reason));
        continue;
      }
      const result = await execute(participant, workflow.command, context, task.markers.edit, workflow);
      const results = [...setupResults, result];
      const state = sampleState(result);
      const restored = await replaceFixture(context.projectRoot, { ...fixture.edit, needle: fixture.edit.replacement, replacement: fixture.edit.needle });
      const finalState = restored.ok ? state : "failed";
      samples.push(sample(id, "edit", finalState, finalState === "measured" ? result.process.wall_ms : UNMEASURED, results, task.markers.edit, restored.ok ? null : "edited fixture could not be restored"));
    }
  } finally {
    if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
  }
  return summarizeNumeric(samples, "ms");
}

async function measureError(participant, task, runRoot, options) {
  const workflow = workflowFor(options.manifest, participant, "error");
  if (workflow.supported === false) return emptyError(workflow.reason);
  if (options.dryRun) return emptyError("dry-run requested");
  const cellRoot = await fs.mkdtemp(join(runRoot, `${participant.id}-error-`));
  const context = { cellRoot, projectRoot: join(cellRoot, "project"), fixtureRoot: join(cellRoot, "project"), displayCell: relativePath(cellRoot), target: participant.target, artifact: participant.artifact };
  const setupResults = [];
  try {
    const setup = await setupParticipant(participant, task, context, workflow, setupResults);
    if (!setup.ok) return summarizeError([{ ...sample("error-01", "error", setup.state, UNMEASURED, setupResults, task.markers.error, setup.reason), diagnosis_ms: UNMEASURED, repair_ms: UNMEASURED, total_ms: UNMEASURED }]);
    const fixture = setup.fixture;
    if (!fixture.defect || !fixture.fix) return emptyError("native fixture has no deterministic defect/fix pair");
    const defect = await replaceFixture(context.projectRoot, fixture.defect);
    if (!defect.ok) return emptyError(defect.reason);
    const diagnosisStart = monoNow();
    const diagnostic = await execute(participant, workflow.command, context, task.markers.error, { ...workflow, expect_failure: true });
    const diagnosisMs = elapsedMs(diagnosisStart);
    const fix = await replaceFixture(context.projectRoot, fixture.fix);
    if (!fix.ok) return summarizeError([{ ...sample("error-01", "error", "failed", UNMEASURED, [...setupResults, diagnostic], task.markers.error, fix.reason), diagnosis_ms: UNMEASURED, repair_ms: UNMEASURED, total_ms: UNMEASURED }]);
    const repairStart = monoNow();
    const repaired = await execute(participant, workflow.command, context, task.markers.repair, workflow);
    const repairMs = elapsedMs(repairStart);
    const diagnosticState = sampleState(diagnostic);
    const repairedState = sampleState(repaired);
    const state = diagnosticState === "measured" && repairedState === "measured" ? "measured" : diagnosticState === "unmeasured" || repairedState === "unmeasured" ? "unmeasured" : "failed";
    const item = sample("error-01", "error", state, state === "measured" ? diagnosisMs + repairMs : UNMEASURED, [...setupResults, diagnostic, repaired], task.markers.repair, state === "measured" ? null : "diagnostic or first-green marker was not observed", { diagnosis_ms: state === "measured" ? diagnosisMs : UNMEASURED, repair_ms: state === "measured" ? repairMs : UNMEASURED, total_ms: state === "measured" ? diagnosisMs + repairMs : UNMEASURED, diagnostic: diagnostic.marker?.observed ? task.markers.error : null });
    return summarizeError([item]);
  } finally {
    if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
  }
}
async function artifactFact(root, artifact) {
  const target = await fixturePath(root, artifact);
  try {
    const info = await fs.stat(target);
    if (info.isFile()) {
      const bytes = await fs.readFile(target);
      return { status: "measured", path: artifact, bytes: bytes.length, sha256: hashBytes(bytes) };
    }
    if (!info.isDirectory()) return { status: "failed", reason: `package artifact is not a file or directory: ${artifact}` };
    const entries = [];
    let total = 0;
    const walk = async (directory, prefix) => {
      const children = (await fs.readdir(directory, { withFileTypes: true })).sort((left, right) => utf8Compare(left.name, right.name));
      for (const child of children) {
        const childPath = join(directory, child.name);
        const childRelative = `${prefix}/${child.name}`;
        if (child.isDirectory()) await walk(childPath, childRelative);
        else if (child.isFile()) {
          const bytes = await fs.readFile(childPath);
          total += bytes.length;
          entries.push({ path: childRelative, bytes: bytes.length, sha256: hashBytes(bytes) });
        }
      }
    };
    await walk(target, "");
    return { status: "measured", path: artifact, bytes: total, sha256: hashJson(entries), entries };
  } catch (error) {
    return { status: "failed", reason: `package artifact was not produced: ${error.message}` };
  }
}

async function measureTestOrPackage(participant, task, runRoot, dimension, count, options) {
  const workflow = workflowFor(options.manifest, participant, dimension);
  if (workflow.supported === false) return { status: "unmeasured", cold: emptySummary("ms", workflow.reason, count), warm: emptySummary("ms", workflow.reason, count) };
  if (options.dryRun) return { status: "unmeasured", cold: emptySummary("ms", "dry-run requested", count), warm: emptySummary("ms", "dry-run requested", count) };
  const cellRoot = await fs.mkdtemp(join(runRoot, `${participant.id}-${dimension}-`));
  const context = { cellRoot, projectRoot: join(cellRoot, "project"), fixtureRoot: join(cellRoot, "project"), displayCell: relativePath(cellRoot), target: participant.target, artifact: participant.artifact };
  const setupResults = [];
  const cold = [];
  const warm = [];
  try {
    const setup = await setupParticipant(participant, task, context, workflow, setupResults);
    if (!setup.ok) {
      const marker = dimension === "test" ? task.markers.test : null;
      return { status: setup.state === "failed" ? "failed" : "unmeasured", cold: summarizeNumeric(setupSamples(count, "cold", setup.state, setup.reason, setupResults, marker), "ms"), warm: summarizeNumeric(setupSamples(count, "warm", setup.state, setup.reason, setupResults, marker), "ms") };
    }
    const marker = dimension === "test" ? task.markers.test : null;
    for (let index = 0; index < count; index += 1) {
      const coldResult = await execute(participant, workflow.command, context, marker, workflow);
      let coldState = sampleState(coldResult);
      let coldExtra = {};
      if (dimension === "package" && coldState === "measured") {
        coldExtra.artifact = await artifactFact(context.projectRoot, participant.artifact);
        if (coldExtra.artifact.status !== "measured") coldState = coldExtra.artifact.status === "unmeasured" ? "unmeasured" : "failed";
      }
      cold.push(sample(`${dimension}-cold-${String(index + 1).padStart(2, "0")}`, "cold", coldState, coldState === "measured" ? coldResult.process.wall_ms : UNMEASURED, [...setupResults, coldResult], marker, coldState === "measured" ? null : coldResult.error, coldExtra));
      const warmResult = await execute(participant, workflow.command, context, marker, workflow);
      let warmState = sampleState(warmResult);
      let warmExtra = {};
      if (dimension === "package" && warmState === "measured") {
        warmExtra.artifact = await artifactFact(context.projectRoot, participant.artifact);
        if (warmExtra.artifact.status !== "measured") warmState = warmExtra.artifact.status === "unmeasured" ? "unmeasured" : "failed";
      }
      warm.push(sample(`${dimension}-warm-${String(index + 1).padStart(2, "0")}`, "warm", warmState, warmState === "measured" ? warmResult.process.wall_ms : UNMEASURED, [...setupResults, warmResult], marker, warmState === "measured" ? null : warmResult.error, warmExtra));
    }
  } finally {
    if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
  }
  const coldSummary = summarizeNumeric(cold, "ms");
  const warmSummary = summarizeNumeric(warm, "ms");
  return { status: coldSummary.status === "failed" || warmSummary.status === "failed" ? "failed" : coldSummary.status === "unmeasured" || warmSummary.status === "unmeasured" ? "unmeasured" : "measured", cold: coldSummary, warm: warmSummary };
}

async function measureDebug(participant, task, runRoot, options) {
  const workflow = workflowFor(options.manifest, participant, "debug");
  if (workflow.supported === false) return emptySummary("ms", workflow.reason);
  if (options.dryRun) return emptySummary("ms", "dry-run requested");
  const cellRoot = await fs.mkdtemp(join(runRoot, `${participant.id}-debug-`));
  const context = { cellRoot, projectRoot: join(cellRoot, "project"), fixtureRoot: join(cellRoot, "project"), displayCell: relativePath(cellRoot), target: participant.target, artifact: participant.artifact };
  const results = [];
  try {
    const setup = await setupParticipant(participant, task, context, workflow, results);
    if (!setup.ok) return summarizeNumeric(setupSamples(1, "debug", setup.state, setup.reason, results, task.markers.debug), "ms");
    const result = await execute(participant, workflow.command, context, task.markers.debug, workflow);
    results.push(result);
    const state = sampleState(result);
    return summarizeNumeric([sample("debug-01", "warm", state, state === "measured" ? result.process.wall_ms : UNMEASURED, results, task.markers.debug)], "ms");
  } finally {
    if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
  }
}

async function measureVersion(participant, options, availability) {
  if (options.dryRun) return { status: "unmeasured", value: null, reason: "dry-run requested", command: null, result: null };
  if (!availability) return { status: "unmeasured", value: null, reason: `executable not found: ${participant.executable}`, command: null, result: null };
  const result = await runCommand(availability, participant.version_args, { cwd: ROOT, env: envForCommand(options.scratch), timeout_ms: Math.min(DEFAULT_TIMEOUT_MS, 15_000), stop_on_marker: false });
  const text = `${result.stdout}\n${result.stderr}`.trim().replace(/\s+/gu, " ");
  const status = result.status === "ok" && text.length > 0 ? "measured" : result.status === "unavailable" ? "unmeasured" : "failed";
  return { status, value: status === "measured" ? text : null, reason: status === "measured" ? null : result.error || "version command produced no output", command: { ...result.command, args: participant.version_args }, result };
}

function firstFailure(dimensions, version) {
  if (version.status === "failed") return { state: "failed", reason: version.reason };
  if (version.status === "unmeasured") return { state: "unmeasured", reason: version.reason };
  for (const dimension of DIMENSION_IDS) {
    const value = dimensions[dimension];
    if (!value) continue;
    if (value.status === "failed") return { state: "failed", reason: `${dimension} workflow failed` };
    if (value.status === "unmeasured") return { state: "unmeasured", reason: `${dimension} workflow is unmeasured` };
  }
  return null;
}

function dimensionState(value) {
  if (!value) return "unmeasured";
  return value.status || "unmeasured";
}

async function measureRow(manifest, domain, participant, runRoot, options) {
  const startedAt = new Date().toISOString();
  const task = manifest.tasks.find((candidate) => candidate.id === domain.task_id);
  const nativeFixture = fixtureFor(participant, task);
  const availability = resolveProgram(participant.executable, ROOT);
  const version = await measureVersion(participant, options, availability);
  const selected = options.dimensions;
  const dimensions = {};
  const common = { manifest, keep: options.keep, dryRun: options.dryRun };
  for (const dimension of DIMENSION_IDS) {
    if (!selected.has(dimension)) {
      dimensions[dimension] = dimension === "error" ? emptyError("dimension not selected") : dimension === "test" || dimension === "package" ? { status: "unmeasured", cold: emptySummary("ms", "dimension not selected"), warm: emptySummary("ms", "dimension not selected") } : emptySummary("ms", "dimension not selected");
      continue;
    }
    if (dimension === "cold") dimensions.cold = await measureCold(participant, task, runRoot, options.counts.cold, common);
    else if (dimension === "warm") dimensions.warm = await measureWarm(participant, task, runRoot, options.counts.warm, common);
    else if (dimension === "edit") dimensions.edit = await measureEdit(participant, task, runRoot, options.counts.edit, common);
    else if (dimension === "error") dimensions.error = await measureError(participant, task, runRoot, common);
    else if (dimension === "test" || dimension === "package") dimensions[dimension] = await measureTestOrPackage(participant, task, runRoot, dimension, options.counts[dimension], common);
    else if (dimension === "debug") dimensions.debug = await measureDebug(participant, task, runRoot, common);
  }
  const failure = firstFailure(dimensions, version);
  const allStates = DIMENSION_IDS.map((dimension) => dimensionState(dimensions[dimension]));
  const status = failure?.state || (allStates.includes("failed") ? "failed" : allStates.includes("unmeasured") ? "unmeasured" : "measured");
  const receipts = [];
  for (const value of Object.values(dimensions)) {
    const samples = value?.samples || [...(value?.cold?.samples || []), ...(value?.warm?.samples || [])];
    for (const item of samples) receipts.push(...(item.commands || []));
  }
  const evidence = {
    command_sequence: Object.fromEntries(Object.entries(participant.commands).map(([id, command]) => [id, { program: command.program, args: command.args, cwd: command.cwd, timeout_ms: command.timeout_ms }])),
    command_receipts: receipts,
    version: version.command ? { command: version.command, result: commandEvidence([version.result])[0] } : { status: version.status, reason: version.reason },
    dimension_states: Object.fromEntries(DIMENSION_IDS.map((dimension) => [dimension, dimensionState(dimensions[dimension])])),
  };
  return {
    schema: ROW_SCHEMA,
    key: `${domain.id}/${participant.id}`,
    status,
    failure: failure ? { state: failure.state, reason: failure.reason } : null,
    domain: { order: domain.order, id: domain.id, slug: domain.slug, title: domain.title, task_id: task.task_id, fixture_id: task.fixture_id, neutral_task_id: task.neutral.id, scenario: task.scenario || domain.scenario || null },
    participant: { id: participant.id, profile: participant.profile_id, label: participant.label, kind: participant.kind, executable: participant.executable, resolved_path: availability },
    task_identity: { task_id: task.task_id, fixture_id: task.fixture_id, neutral_sha256: hashJson(task.neutral), fixture_sha256: nativeFixture ? hashJson(nativeFixture) : null },
    tool: { version: version.value, status: version.status, reason: version.reason, version_command: version.command, version_elapsed_ms: version.elapsed_ms ?? null },
    dimensions,
    evidence,
    environment: { ...environmentIdentity(envForCommand(runRoot)), command_environment: manifest.source.command_environment, cache_policy: "global package caches retained; project scratch is fresh per sample" },
    timestamps: { started_at: startedAt, ended_at: new Date().toISOString() },
  };
}

function referenceParticipant(manifest, profileId, id, root) {
  const profile = manifest.profiles[profileId];
  return {
    ...profile,
    id,
    profile_id: profileId,
    root,
    commands: profile.commands,
    target: "",
    artifact: profile.artifact || null,
  };
}

function referenceWorkflowReason(metric, workflow) {
  if (!workflow) return "reference workflow is not declared";
  return null;
}

async function referenceWorkflow(participant, workflow, context, marker, options, metricName = null) {
  if (!workflow) return { metric: metricName, result: null, sample: referenceMetric(metricName || "workflow", "unmeasured", null, "reference workflow is not declared") };
  const commandSpec = participant.commands[workflow.command];
  const result = await execute(participant, workflow.command, context, marker, {
    stop_on_marker: commandSpec?.resident === true,
    expect_failure: options.expect_failure === true,
  });
  const state = sampleState(result);
  const detail = /diagnosis_ms=(\d+)\s+repair_ms=(\d+)/u.exec(result.stdout || "");
  const extra = detail ? { diagnosis_ms: Number(detail[1]), repair_ms: Number(detail[2]) } : {};
  return {
    metric: metricName || workflow.metric || null,
    result,
    sample: referenceMetric(
      metricName || workflow.metric || "workflow",
      state,
      state === "measured" ? result.process.wall_ms : null,
      state === "measured" ? null : result.error || `workflow ${state}`,
      { marker: result.marker, command: result.command, ...extra },
    ),
  };
}

async function measureReferenceParticipant(manifest, app, profileId, sourceRoot, runRoot, options) {
  const metrics = app.metrics;
  const participantId = profileId === "jet" ? "jet" : app.comparison.peer_profile;
  const participant = referenceParticipant(manifest, profileId, participantId, sourceRoot);
  const comparison = app.comparison;
  try {
    const info = await fs.stat(sourceRoot);
    if (!info.isDirectory()) throw new Error("source root is not a directory");
  } catch (error) {
    const reason = `source root is unavailable: ${sourceRoot}: ${error.message}`;
    return {
      id: participantId,
      profile: profileId,
      status: "unmeasured",
      reason,
      source: null,
      metrics: unavailableReferenceMetrics(metrics, reason),
      workflows: [],
      graph: { status: "unmeasured", reason, commands: [] },
    };
  }
  const inventory = profileId === "jet"
    ? await sourceInventory(sourceRoot, app.source.files.map((file) => file.replace(`${app.root}/`, "")))
    : await sourceInventory(sourceRoot, null, comparison);
  if (inventory.status !== "measured") {
    return {
      id: participantId,
      profile: profileId,
      status: "unmeasured",
      reason: inventory.reason,
      source: inventory,
      metrics: unavailableReferenceMetrics(metrics, inventory.reason),
      workflows: [],
      graph: { status: "unmeasured", reason: inventory.reason, commands: [] },
    };
  }

  const metricResults = unavailableReferenceMetrics(metrics, options.dryRun ? "dry-run requested" : "workflow evidence not collected");
  metricResults.lines_of_code = referenceMetric("lines_of_code", "measured", inventory.loc, null, { source: inventory.sha256 });
  const workflows = [];
  const graphEvidence = [];
  if (!options.dryRun && profileId === "jet") {
    const cellRoot = await fs.mkdtemp(join(runRoot, `${app.id}-${profileId}-reference-`));
    const projectRoot = join(cellRoot, "project");
    const context = { cellRoot, projectRoot, fixtureRoot: projectRoot, displayCell: relativePath(cellRoot), target: app.target, artifact: null };
    try {
      await fs.cp(sourceRoot, projectRoot, { recursive: true, force: true });
      const appWorkflows = app.workflows || {};
      for (const [metric, workflow] of Object.entries(appWorkflows)) {
        if (metric === "graph") continue;
        if (!metrics.includes(metric)) continue;
        const reason = referenceWorkflowReason(metric, workflow);
        if (reason) {
          metricResults[metric] = referenceMetric(metric, "unmeasured", null, reason);
          workflows.push({ metric, status: "unmeasured", reason, commands: [] });
          continue;
        }
        const observation = await referenceWorkflow(participant, workflow, context, workflow.marker, {}, metric);
        metricResults[metric] = observation.sample;
        workflows.push({ metric, status: observation.sample.status, reason: observation.sample.reason, commands: commandEvidence([observation.result]) });
      }
      const graphWorkflow = appWorkflows.graph;
      if (graphWorkflow) {
        const graph = await referenceWorkflow(participant, graphWorkflow, context, graphWorkflow.marker, {});
        graphEvidence.push(graph.result);
        graphEvidence.push(graph.sample);
      }
    } catch (error) {
      for (const metric of metrics) {
        if (metricResults[metric]?.status === "unmeasured" && metricResults[metric].reason === "workflow evidence not collected") {
          metricResults[metric] = referenceMetric(metric, "failed", null, `reference workflow setup failed: ${error.message}`);
        }
      }
    } finally {
      if (!options.keep) await fs.rm(cellRoot, { recursive: true, force: true });
    }
  } else if (!options.dryRun && profileId !== "jet") {
    const reason = "peer workflow adapter is not run from source inventory; no scaffold, install, or download is attempted";
    for (const metric of metrics) {
      if (metric !== "lines_of_code") metricResults[metric] = referenceMetric(metric, "unmeasured", null, reason);
    }
  }

  const states = Object.values(metricResults).map((metric) => metric.status);
  const status = states.includes("failed") ? "failed" : states.includes("unmeasured") ? "unmeasured" : "measured";
  return {
    id: participantId,
    profile: profileId,
    status,
    reason: status === "measured" ? null : Object.values(metricResults).find((metric) => metric.status !== "measured")?.reason || "reference evidence is incomplete",
    source: inventory,
    metrics: metricResults,
    workflows,
    graph: {
      status: graphEvidence[1]?.status || "unmeasured",
      reason: graphEvidence[1]?.reason || (options.dryRun ? "dry-run requested" : "graph workflow is unavailable"),
      commands: graphEvidence[0] ? commandEvidence([graphEvidence[0]]) : [],
    },
  };
}

function referenceComparison(app, jet, peer) {
  return app.metrics.map((metric) => {
    const left = jet?.metrics?.[metric];
    const right = peer?.metrics?.[metric];
    if (left?.status !== "measured" || right?.status !== "measured") {
      return { metric, status: "unmeasured", jet: left?.value ?? null, peer: right?.value ?? null, ratio: null, score: null, reason: "same recorded program needs measured evidence from both participants" };
    }
    const ratio = right.value === 0 ? left.value === 0 ? 1 : Infinity : left.value / right.value;
    const policy = app.comparison;
    const status = ratio < (policy.parity_ratio_low || 0.8) ? "win" : ratio > (policy.parity_ratio_high || 1.25) ? "loss" : "parity";
    return { metric, status, jet: left.value, peer: right.value, ratio, score: status, reason: null };
  });
}

async function measureReferenceApps(manifest, runRoot, options) {
  const results = [];
  for (const app of manifest.reference_apps || []) {
    const jetRoot = resolve(ROOT, app.root);
    let jet;
    try {
      jet = await measureReferenceParticipant(manifest, app, "jet", jetRoot, runRoot, options);
    } catch (error) {
      jet = { id: "jet", profile: "jet", status: "failed", reason: error.message, source: null, metrics: unavailableReferenceMetrics(app.metrics, error.message), workflows: [], graph: { status: "failed", reason: error.message, commands: [] } };
    }
    const peerEnv = app.comparison.peer_source_env || "JET_DX_REFERENCE_PEER_ROOT";
    const peerOverride = process.env[peerEnv];
    let peerRoot = peerOverride ? resolve(peerOverride) : null;
    let peer;
    if (!peerRoot) {
      const reason = `peer source is unavailable; set ${peerEnv} to an existing checkout; no scaffold/download attempted`;
      peer = { id: app.comparison.peer_profile, profile: app.comparison.peer_profile, status: "unmeasured", reason, source: null, metrics: unavailableReferenceMetrics(app.metrics, reason), workflows: [], graph: { status: "unmeasured", reason, commands: [] } };
    } else {
      peer = await measureReferenceParticipant(manifest, app, app.comparison.peer_profile, peerRoot, runRoot, options);
    }
    results.push({
      schema: "jet.dx.reference-app-result.v1",
      app_id: app.id,
      card: app.card,
      recorded_program: app.recorded_program?.id || app.entry,
      participants: { jet, peer },
      comparisons: referenceComparison(app, jet, peer),
      receipt: app.receipt,
      timestamps: { ended_at: new Date().toISOString() },
    });
  }
  return results;
}

async function writeReferenceReceipts(manifest, results) {
  for (const result of results) {
    const app = manifest.reference_apps?.find((candidate) => candidate.id === result.app_id);
    if (!app?.receipt?.path) continue;
    const jet = result.participants.jet;
    const peer = result.participants.peer;
    const receipt = {
      schema: app.receipt.schema,
      app_id: result.app_id,
      card: result.card,
      recorded_program: result.recorded_program,
      source_digest: jet.source?.sha256 || null,
      loc: jet.metrics?.lines_of_code?.value ?? null,
      peer_loc: peer.metrics?.lines_of_code?.value ?? null,
      source_inventory: { jet: jet.source, peer: peer.source },
      first_run: jet.metrics?.first_run || null,
      edit_to_see: jet.metrics?.edit_to_see || null,
      error_to_fix: jet.metrics?.error_to_fix || null,
      test_loop: jet.metrics?.test_loop || null,
      graph: jet.graph,
      participants: result.participants,
      comparisons: result.comparisons,
      evidence_policy: "No score is emitted unless both participants provide measured evidence for the same recorded program.",
      generated_at: new Date().toISOString(),
    };
    await writeAtomic(resolve(ROOT, app.receipt.path), receipt);
  }
}

function plannedRow(domain, participant, task) {
  return {
    schema: ROW_SCHEMA,
    key: `${domain.id}/${participant.id}`,
    status: "pending",
    failure: null,
    domain: { order: domain.order, id: domain.id, slug: domain.slug, title: domain.title, task_id: task.task_id, fixture_id: task.fixture_id, neutral_task_id: task.neutral.id },
    participant: { id: participant.id, profile: participant.profile_id, label: participant.label, kind: participant.kind, executable: participant.executable, resolved_path: null },
    task_identity: { task_id: task.task_id, fixture_id: task.fixture_id, neutral_sha256: hashJson(task.neutral), fixture_sha256: hashJson(task.fixture) },
    tool: { version: null, status: "pending", reason: "row has not been attempted" },
    dimensions: Object.fromEntries(DIMENSION_IDS.map((dimension) => [dimension, null])),
    evidence: { command_sequence: {}, version: null, dimension_states: Object.fromEntries(DIMENSION_IDS.map((dimension) => [dimension, "pending"])) },
    environment: null,
    timestamps: { started_at: null, ended_at: null },
  };
}

function sortedRows(rows) {
  return [...rows].sort((left, right) => utf8Compare(left.key, right.key));
}

function comparisonValue(row, leaf) {
  if (!row || row.status === "pending") return { status: "unmeasured", value: UNMEASURED };
  const parts = leaf.split(".");
  let value = row.dimensions[parts[0]];
  if (parts.length > 1) value = value?.[parts[1]];
  if (parts.length > 2) value = value?.[parts[2]];
  if (!value) return { status: "unmeasured", value: UNMEASURED };
  if (value.status === "measured" && typeof value.p50 === "number" && Number.isFinite(value.p50)) return { status: "measured", value: value.p50 };
  return { status: value.status === "failed" ? "failed" : "unmeasured", value: UNMEASURED };
}

function compareLeaf(jet, peer, leaf, policy) {
  const left = comparisonValue(jet, leaf);
  const right = comparisonValue(peer, leaf);
  if (left.status === "failed" || right.status === "failed") return { dimension: leaf, status: "failed", jet: left.value, peer: right.value, ratio: null, score: null, reason: "a required workflow failed" };
  if (left.status !== "measured" || right.status !== "measured") return { dimension: leaf, status: "unmeasured", jet: left.value, peer: right.value, ratio: null, score: null, reason: "both participants need measured command and observation evidence" };
  const ratio = right.value === 0 ? left.value === 0 ? 1 : Infinity : left.value / right.value;
  const status = ratio < policy.comparison.parity_ratio_low ? "win" : ratio > policy.comparison.parity_ratio_high ? "loss" : "parity";
  return { dimension: leaf, status, jet: left.value, peer: right.value, ratio, score: status, reason: null };
}

function compareRows(jet, peer, policy, domain) {
  const dimensions = COMPARISON_LEAVES.map((leaf) => compareLeaf(jet, peer, leaf, policy));
  const status = dimensions.some((item) => item.status === "failed") ? "failed" : dimensions.some((item) => item.status === "unmeasured") ? "unmeasured" : dimensions.some((item) => item.status === "loss") ? "loss" : dimensions.every((item) => item.status === "parity") ? "parity" : "win";
  return { key: `${domain.id}/${peer.participant.id}`, domain_id: domain.id, peer_id: peer.participant.id, status, score: ["win", "parity", "loss"].includes(status) ? status : null, dimensions, rule: "every leaf is compared independently; missing or failed evidence never receives a score" };
}

function comparisonsFor(manifest, rows) {
  const byKey = new Map(rows.map((row) => [row.key, row]));
  const comparisons = [];
  for (const domain of manifest.domains) {
    const jet = byKey.get(`${domain.id}/jet`);
    if (!jet) continue;
    for (const descriptor of domain.comparators.slice(1)) {
      const peer = byKey.get(`${domain.id}/${descriptor.profile}`);
      if (peer) comparisons.push(compareRows(jet, peer, manifest.policy, domain));
    }
  }
  return comparisons.sort((left, right) => utf8Compare(left.key, right.key));
}

function reportStatus(rows, expected) {
  if (rows.length < expected || rows.some((row) => row.status === "pending")) return "incomplete";
  if (rows.some((row) => row.status === "failed")) return "recorded-with-failures";
  if (rows.some((row) => row.status === "unmeasured")) return "recorded-with-unmeasured";
  return "recorded";
}

function summaryFor(rows, expected) {
  const summary = { expected, recorded: rows.filter((row) => row.status !== "pending").length, pending: rows.filter((row) => row.status === "pending").length, measured: 0, unmeasured: 0, failed: 0 };
  for (const row of rows) if (Object.hasOwn(summary, row.status)) summary[row.status] += 1;
  return summary;
}

function makeReport({ manifest, identity, run, rows, attempts, scratch, reference_results = [], status = null }) {
  const expectedRows = manifest.domains.reduce((total, domain) => total + domain.comparators.length, 0);
  const sorted = sortedRows(rows);
  return {
    schema: SCHEMA,
    schema_version: SCHEMA_VERSION,
    cards: manifest.cards,
    reference_apps: manifest.reference_apps || [],
    reference_results,
    status: status || reportStatus(sorted, expectedRows),
    identity,
    manifest: { path: relativePath(manifest.__file), id: manifest.source.id, sha256: identity.manifest_sha256 },
    run,
    policy: manifest.policy,
    dimensions: DIMENSION_IDS,
    domains: manifest.domains.map((domain) => ({ order: domain.order, id: domain.id, slug: domain.slug, title: domain.title, task_id: domain.task_id, benchmark_peer: domain.benchmark_peer })),
    scratch: { root: scratch.root, run_dir: scratch.run_dir, disk_only: true },
    expected_rows: expectedRows,
    rows: sorted,
    attempts: [...attempts].sort((left, right) => utf8Compare(left.attempt_id, right.attempt_id)),
    comparisons: comparisonsFor(manifest, sorted),
    summary: summaryFor(sorted, expectedRows),
  };
}

async function writeAtomic(file, value) {
  await fs.mkdir(dirname(file), { recursive: true, mode: 0o700 });
  const temporary = `${file}.${process.pid}.partial`;
  await fs.writeFile(temporary, `${JSON.stringify(canonical(value), null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
  await fs.rename(temporary, file);
}

async function exists(file) {
  return fs.access(file).then(() => true).catch(() => false);
}

function validateStoredRow(row, label = "row") {
  assert(row && typeof row === "object" && row.schema === ROW_SCHEMA && nonEmpty(row.key), `${label} identity is invalid`);
  assert(ROW_STATES.includes(row.status), `${label} status is invalid`);
  assert(row.dimensions && typeof row.dimensions === "object", `${label} dimensions are missing`);
  if (row.status === "pending") return;
  for (const dimension of DIMENSION_IDS) assert(row.dimensions[dimension] && typeof row.dimensions[dimension] === "object", `${label}.${dimension} evidence is missing`);
  assert(row.tool && ["measured", "unmeasured", "failed"].includes(row.tool.status), `${label} tool status is invalid`);
  if (row.status !== "measured") assert(row.failure && nonEmpty(row.failure.reason), `${label} non-measured row lacks failure reason`);
}

export function validateReport(report) {
  assert(report && report.schema === SCHEMA && report.schema_version === SCHEMA_VERSION, "report schema is invalid");
  assert(report.status === "incomplete" || report.status === "recorded" || report.status === "recorded-with-failures" || report.status === "recorded-with-unmeasured", "report status is invalid");
  assert(report.identity && /^[0-9a-f]{64}$/u.test(report.identity.sha256 || ""), "report identity is missing");
  assert(Number.isInteger(report.expected_rows) && report.expected_rows > 0, "report expected_rows is invalid");
  assert(Array.isArray(report.rows) && Array.isArray(report.attempts) && Array.isArray(report.comparisons) && Array.isArray(report.reference_results), "report rows, attempts, comparisons, or reference results are missing");
  for (const [index, result] of report.reference_results.entries()) {
    assert(result && result.schema === "jet.dx.reference-app-result.v1" && nonEmpty(result.app_id), `reference result ${index} identity is invalid`);
    assert(result.participants && result.participants.jet && result.participants.peer, `reference result ${result.app_id} participants are missing`);
    for (const participant of Object.values(result.participants)) {
      assert(SAMPLE_STATES.includes(participant.status), `reference result ${result.app_id} participant status is invalid`);
      assert(participant.metrics && typeof participant.metrics === "object", `reference result ${result.app_id} metrics are missing`);
    }
  }
  const keys = new Set();
  for (const row of report.rows) {
    validateStoredRow(row, `report row ${row.key}`);
    assert(!keys.has(row.key), `duplicate report row ${row.key}`);
    keys.add(row.key);
  }
  assert(stableJson(report.rows) === stableJson(sortedRows(report.rows)), "report rows are not deterministically ordered");
  return report;
}

function identityFor(manifest, options, adaptersFile) {
  const manifestSha = manifestDigest(manifest);
  const identity = { manifest_sha256: manifestSha, manifest_path: relativePath(manifest.__file), adapters: adaptersFile, dimensions: DIMENSION_IDS, domains: manifest.domains.map((domain) => ({ id: domain.id, task_id: domain.task_id, participants: domain.comparators.map((participant) => participant.profile) })), environment: environmentIdentity(envForCommand(options.scratch)) };
  return { ...identity, sha256: hashJson(identity) };
}

function defaultCounts(manifest, override) {
  return Object.fromEntries(DIMENSION_IDS.map((dimension) => [dimension, override || manifest.policy.sample_policy[dimension]]));
}

function attemptId(key, ordinal) {
  return `${key.replaceAll("/", "-")}-a${String(ordinal).padStart(3, "0")}`;
}

async function runCampaign(manifest, options) {
  const scratchRoot = assertSafeScratch(options.scratch, "scratch");
  const output = resolve(options.output);
  const adaptersFile = manifest.fresh_agent?.adapters?.config ? { path: manifest.fresh_agent.adapters.config } : { path: null };
  const identity = identityFor(manifest, options, adaptersFile);
  let existing = null;
  if (await exists(output)) {
    if (!options.resume) fail(`output already exists; pass --resume: ${output}`);
    existing = validateReport(await readJson(output, "existing report"));
    assert(existing.identity.sha256 === identity.sha256, "existing report identity differs from the canonical manifest");
  }
  const runId = existing?.run?.id || options.run_id || `dx-${identity.sha256.slice(0, 16)}`;
  const runDir = existing?.scratch?.run_dir || await fs.mkdtemp(join(scratchRoot, `run-${runId}-`));
  assertSafeScratch(runDir, "run scratch");
  await fs.mkdir(runDir, { recursive: true, mode: 0o700 });
  const rows = existing?.rows ? [...existing.rows] : [];
  const attempts = existing?.attempts ? [...existing.attempts] : [];
  const rowMap = new Map(rows.map((row) => [row.key, row]));
  if (!existing) {
    for (const domain of manifest.domains) {
      const task = manifest.tasks.find((candidate) => candidate.id === domain.task_id);
      for (const descriptor of domain.comparators) rowMap.set(`${domain.id}/${descriptor.profile}`, plannedRow(domain, participantFor(manifest, domain, descriptor), task));
    }
  }
  const referenceResults = existing?.reference_results || await measureReferenceApps(manifest, runDir, options);
  await writeReferenceReceipts(manifest, referenceResults);
  let report = makeReport({ manifest, identity, run: { id: runId, started_at: existing?.run?.started_at || new Date().toISOString(), resumed: Boolean(existing), finished_at: null, elapsed_ms: null }, rows: [...rowMap.values()], attempts, scratch: { root: scratchRoot, run_dir: runDir }, reference_results: referenceResults, status: "incomplete" });
  if (!existing) await writeAtomic(output, report);

  for (const domain of manifest.domains) {
    if (options.domains.size > 0 && !options.domains.has(domain.id)) continue;
    const task = manifest.tasks.find((candidate) => candidate.id === domain.task_id);
    for (const descriptor of domain.comparators) {
      const participant = participantFor(manifest, domain, descriptor);
      const key = `${domain.id}/${participant.id}`;
      const prior = rowMap.get(key);
      if (prior?.status === "measured") continue;
      const ordinal = attempts.filter((attempt) => attempt.key === key).length + 1;
      const id = attemptId(key, ordinal);
      let result;
      try {
        result = await measureRow(manifest, domain, participant, runDir, { ...options, counts: options.counts, dimensions: options.dimensions });
      } catch (error) {
        result = { ...plannedRow(domain, participant, task), status: "failed", failure: { state: "failed", reason: error.message }, tool: { version: null, status: "failed", reason: error.message }, dimensions: Object.fromEntries(DIMENSION_IDS.map((dimension) => [dimension, emptySummary("ms", error.message)])) };
      }
      const attempt = { attempt_id: id, key, ...result };
      attempts.push(attempt);
      rowMap.set(key, result);
      report = makeReport({ manifest, identity, run: { id: runId, started_at: report.run.started_at, resumed: Boolean(existing), finished_at: null, elapsed_ms: null }, rows: [...rowMap.values()], attempts, scratch: { root: scratchRoot, run_dir: runDir }, reference_results: referenceResults, status: "incomplete" });
      await writeAtomic(output, report);
    }
  }
  const finished = report.rows.every((row) => row.status !== "pending");
  report = makeReport({ manifest, identity, run: { id: runId, started_at: report.run.started_at, resumed: Boolean(existing), finished_at: finished ? new Date().toISOString() : null, elapsed_ms: finished ? Math.max(0, Date.now() - Date.parse(report.run.started_at)) : null }, rows: [...rowMap.values()], attempts, scratch: { root: scratchRoot, run_dir: runDir }, reference_results: referenceResults, status: null });
  await writeAtomic(output, report);
  return report;
}

function metricAliases(value) {
  const aliases = { "first-run": ["cold", "warm"], "first_run": ["cold", "warm"], "edit-to-see": ["edit"], "error-to-fix": ["error"], build: ["package"], test: ["test"], debug: ["debug"], cold: ["cold"], warm: ["warm"], edit: ["edit"], package: ["package"] };
  const values = String(value).split(",").flatMap((item) => aliases[item] || fail(`unknown metric ${item}`));
  return [...new Set(values)];
}

function parseArgs(argv) {
  const result = { action: "check", json: false, manifest: DEFAULT_MANIFEST_PATH, output: DEFAULT_OUTPUT, markdown: null, scratch: DEFAULT_SCRATCH, resume: false, replay: null, run_id: null, keep: false, dryRun: false, all: false, domains: new Set(), dimensions: new Set(DIMENSION_IDS), samples: null, counts: null };
  const actions = new Set(["help", "check", "list", "run"]);
  const setAction = (action) => { if (result.action !== "check" && result.action !== action) fail(`modes cannot be combined: ${result.action} and ${action}`); result.action = action; };
  const need = (index, token) => { const value = argv[index + 1]; if (!value || value.startsWith("--")) fail(`${token} requires a value`); return value; };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--help" || token === "-h") { setAction("help"); continue; }
    if (token === "--check") { setAction("check"); continue; }
    if (token === "--list") { setAction("list"); continue; }
    if (token === "--run") { setAction("run"); continue; }
    if (token === "--json") { result.json = true; if (result.action === "check") result.action = "run"; continue; }
    if (token === "--all") { result.all = true; continue; }
    if (token === "--resume") { result.resume = true; continue; }
    if (token === "--keep") { result.keep = true; continue; }
    if (token === "--dry-run") { result.dryRun = true; setAction("run"); continue; }
    if (token === "--manifest" || token === "--output" || token === "--markdown" || token === "--scratch" || token === "--replay" || token === "--run-id" || token === "--samples" || token === "--metric" || token === "--dimension" || token === "--domain" || token === "--archetype") {
      const value = need(index, token); index += 1;
      if (token === "--manifest") result.manifest = resolve(value);
      else if (token === "--output") result.output = resolve(value);
      else if (token === "--markdown") result.markdown = resolve(value);
      else if (token === "--scratch") result.scratch = resolve(value);
      else if (token === "--replay") result.replay = resolve(value);
      else if (token === "--run-id") result.run_id = safeId(value, token);
      else if (token === "--samples") { const count = Number(value); if (!Number.isInteger(count) || count < 1 || count > 10) fail("--samples must be an integer from 1 to 10"); result.samples = count; }
      else if (token === "--metric") for (const dimension of metricAliases(value)) result.dimensions.add(dimension);
      else if (token === "--dimension") for (const dimension of metricAliases(value)) result.dimensions.add(dimension);
      else if (token === "--domain" || token === "--archetype") result.domains.add(value);
      continue;
    }
    if (token.startsWith("--manifest=")) result.manifest = resolve(token.slice(11));
    else if (token.startsWith("--output=")) result.output = resolve(token.slice(9));
    else if (token.startsWith("--markdown=")) result.markdown = resolve(token.slice(11));
    else if (token.startsWith("--scratch=")) result.scratch = resolve(token.slice(10));
    else if (token.startsWith("--replay=")) result.replay = resolve(token.slice(9));
    else if (token.startsWith("--metric=")) for (const dimension of metricAliases(token.slice(9))) result.dimensions.add(dimension);
    else if (token.startsWith("--dimension=")) for (const dimension of metricAliases(token.slice(12))) result.dimensions.add(dimension);
    else if (token.startsWith("--domain=") || token.startsWith("--archetype=")) result.domains.add(token.slice(token.indexOf("=") + 1));
    else fail(`unknown option: ${token}`);
  }
  if (result.action === "help" && argv.some((token) => token !== "--help" && token !== "-h")) fail("--help cannot be combined with another option");
  if (result.resume && result.action !== "run") fail("--resume requires --run");
  if (result.replay && result.action !== "run") result.action = "run";
  if (result.dryRun && result.action !== "run") fail("--dry-run requires --run");
  result.counts = defaultCounts({ policy: { sample_policy: Object.fromEntries(DIMENSION_IDS.map((dimension) => [dimension, 1])) } }, result.samples);
  return result;
}

function usage() {
  return `Usage: node ${SCRIPT_PATH} [--check|--list|--run] [options]

The canonical manifest is tools/agent-eval/dx/manifest.json. It contains all
named E14 domains, matched peer workflows, and the fresh-agent campaign.
The runner never scores a row without command, observation, and artifact evidence.
Reference-app recipes use this same manifest and report; no second harness is allowed.

Options:
  --check                 Validate the manifest without starting tools (default).
  --list                  List every required domain without starting tools.
  --run                   Run missing rows and persist each row after completion.
  --json                  Emit machine-readable report output; implies --run.
  --all                   Select all domains (the default selection).
  --resume                Resume an existing output after identity validation.
  --replay PATH           Validate and print an existing report; starts no tools.
  --manifest PATH         Canonical manifest path.
  --output PATH           Resumable JSON report path.
  --markdown PATH         Write the same matrix as a Markdown report.
  --scratch PATH          Disk-backed scratch root.
  --domain ID             Restrict the run to one domain (repeatable).
  --metric NAME[,NAME]    Select first-run, edit-to-see, error-to-fix, test, debug, or build/package.
  --samples N             Override all sample counts from 1 through 10.
  --keep                  Keep per-row scratch directories.
  --dry-run               Write explicit unmeasured rows without starting tools.
`;
}

function checkPayload(manifest) {
  return { schema: "jet.dx.benchmark.check.v1", status: "ok", manifest: { path: relativePath(manifest.__file), id: manifest.source.id, sha256: manifestDigest(manifest) }, cards: manifest.cards, reference_apps: manifest.reference_apps || [], domains: manifest.domains.map((domain) => ({ order: domain.order, id: domain.id, title: domain.title, peer_count: domain.comparators.length - 1 })), dimensions: DIMENSION_IDS, fresh_agent: { runner: manifest.fresh_agent.runner, domains: manifest.fresh_agent.domains.length }, no_score_without_evidence: manifest.policy.no_score_without_evidence };
}

function human(report) {
  const lines = [`DX benchmark matrix (${report.schema})`, `status=${report.status}`, `rows=${report.summary.recorded}/${report.summary.expected}`, ""];
  if (report.reference_apps?.length) lines.push(`reference_apps=${report.reference_apps.map((app) => app.id).join(",")}`, "");
  if (report.reference_results?.length) {
    lines.push(`reference_results=${report.reference_results.map((result) => `${result.app_id}:${result.participants.jet.status}/${result.participants.peer.status}`).join(",")}`, "");
  }
  for (const domain of report.domains) lines.push(`${domain.id}: ${report.comparisons.filter((comparison) => comparison.domain_id === domain.id).map((comparison) => `${comparison.peer_id}=${comparison.status}`).join(" ") || "pending"}`);
  return `${lines.join("\n")}\n`;
}

function markdown(report) {
  const lines = [
    "# DX benchmark matrix",
    "",
    `- Status: **${report.status}**`,
    `- Manifest: \`${report.manifest.path}\` (\`${report.manifest.sha256}\`)`,
    `- Environment: \`${report.identity.environment?.environment_sha256 || "unavailable"}\``,
    `- Rows: ${report.summary.recorded}/${report.summary.expected}`,
    "",
    "| Domain | Participant | Status | Tool version | Cold p50/p95 (ms) | Warm p50/p95 (ms) |",
    "|---|---|---|---|---:|---:|",
  ];
  for (const row of report.rows) {
    const cold = row.dimensions.cold || {};
    const warm = row.dimensions.warm || {};
    lines.push(`| ${row.domain.id} | ${row.participant.label} | ${row.status} | ${row.tool.version || row.tool.reason || "unavailable"} | ${cold.p50 ?? "—"} / ${cold.p95 ?? "—"} | ${warm.p50 ?? "—"} / ${warm.p95 ?? "—"} |`);
  }
  for (const result of report.reference_results || []) {
    lines.push("", `## Reference app: ${result.app_id}`, "", "| Participant | Status | LOC | First run | Edit to see | Error to fix | Test loop |", "|---|---|---:|---|---|---|---|");
    for (const participant of Object.values(result.participants)) {
      const metric = (name) => participant.metrics?.[name]?.value ?? "—";
      lines.push(`| ${participant.id} | ${participant.status} | ${metric("lines_of_code")} | ${metric("first_run")} | ${metric("edit_to_see")} | ${metric("error_to_fix")} | ${metric("test_loop")} |`);
    }
    lines.push("", "Reference comparisons remain unmeasured until both participants provide the same recorded-program evidence.");
  }
  lines.push("", "Raw command receipts remain in the JSON report; unavailable capabilities retain their exact reason.");
  return `${lines.join("\n")}\n`;
}

export {
  COMPARISON_LEAVES,
  DEFAULT_MANIFEST_PATH,
  DIMENSION_DEFINITIONS,
  DIMENSION_IDS,
  MANIFEST_SCHEMA,
  ROW_SCHEMA,
  SCHEMA,
  validateManifest,
};

export async function main(argv = process.argv.slice(2)) {
  try {
    const options = parseArgs(argv);
    if (options.action === "help") { process.stdout.write(usage()); return 0; }
    const manifest = await loadManifest(options.manifest);
    if (options.action === "list") {
      const payload = { schema: "jet.dx.benchmark.list.v1", domains: manifest.domains.map((domain) => ({ order: domain.order, id: domain.id, title: domain.title, task_id: domain.task_id })) };
      process.stdout.write(options.json ? `${JSON.stringify(canonical(payload), null, 2)}\n` : `${payload.domains.map((domain) => `${String(domain.order).padStart(2, "0")} ${domain.id} ${domain.title}`).join("\n")}\n`);
      return 0;
    }
    if (options.action === "check") {
      const payload = checkPayload(manifest);
      process.stdout.write(options.json ? `${JSON.stringify(canonical(payload), null, 2)}\n` : `DX benchmark matrix: manifest valid\ndomains=${manifest.domains.length}\ndimensions=${DIMENSION_IDS.join(",")}\nfresh_agent_domains=${manifest.fresh_agent.domains.length}\nno_score_without_evidence=true\n`);
      return 0;
    }
    if (options.replay) {
      const report = validateReport(await readJson(options.replay, "replay report"));
      assert(report.identity.manifest_sha256 === manifestDigest(manifest), "replay report was produced by a different manifest");
      if (options.markdown) {
        await fs.mkdir(dirname(options.markdown), { recursive: true, mode: 0o700 });
        await fs.writeFile(options.markdown, markdown(report), { encoding: "utf8", mode: 0o600 });
      }
      process.stdout.write(options.json ? `${JSON.stringify(canonical(report), null, 2)}\n` : human(report));
      return 0;
    }
    const dimensions = options.dimensions.size === DIMENSION_IDS.length && !argv.some((token) => token.startsWith("--metric") || token.startsWith("--dimension")) ? new Set(DIMENSION_IDS) : options.dimensions;
    options.dimensions = dimensions;
    options.counts = defaultCounts(manifest, options.samples);
    for (const dimension of DIMENSION_IDS) if (!options.dimensions.has(dimension)) options.dimensions.add(dimension);
    const report = await runCampaign(manifest, options);
    if (options.markdown) {
      await fs.mkdir(dirname(options.markdown), { recursive: true, mode: 0o700 });
      await fs.writeFile(options.markdown, markdown(report), { encoding: "utf8", mode: 0o600 });
    }
    process.stdout.write(options.json ? `${JSON.stringify(canonical(report), null, 2)}\n` : human(report));
    return report.status === "recorded" ? 0 : 1;
  } catch (error) {
    process.stderr.write(`SETUP: ${error.message}\n`);
    return 78;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] || "").href) process.exitCode = await main();
