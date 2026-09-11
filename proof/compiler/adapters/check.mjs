#!/usr/bin/env node

/**
 * Source-bound checker for compiler adapter correspondence (#2941).
 *
 * This stage indexes the real Core dispatcher, canonical MIR artifact facts,
 * target bridge source, capability applicability relation, and named foreign
 * suffixes. It never executes Jet, a backend, a browser, a VM, or a fixture;
 * optional formal discharge reuses the pinned compiler-proof Lean replay only
 * after this checker derives and binds a canonical adapter semantic goal.
 * Without complete supported certificates, source correspondence remains blocked.
 */

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalJson,
  digestBytes,
  digestText,
  identityDigest,
  replay,
  validateManifest,
} from "../../../scripts/agent/compiler-proof.mjs";
import {
  ROOT,
  array,
  bool,
  fileDigest,
  integer,
  object,
  parseDispatcher,
  projectPath,
  readJson,
  readText,
  text,
  treeDigest,
} from "../runtime/common.mjs";
import { buildObligations } from "../semantics/generate-obligations.mjs";

const STAGE = "adapters";
const CHECK_SCHEMA = "jet.adapters-contract-check.v1";
const CONTRACT_PATH = "proof/compiler/adapters/contract.json";
const WITNESSES_PATH = "proof/compiler/adapters/witnesses.json";
const OBLIGATIONS_PATH = "proof/compiler/obligations.json";
const CORE_PATH = "crates/jet-codegen/src/Prelude/Core.jet";
const CAPABILITY_MANIFEST_PATH = ".jet/hardening-manifest.json";
const PROOF_MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const RUNNER_PATH = "scripts/agent/jet-env";
const OBSERVATION_CERTIFICATE_PATH = "proof/compiler/adapters/observations.json";
const OBSERVATION_CERTIFICATE_SCHEMA = "jet.adapters-observations.v1";
const OBSERVATION_PRODUCER_PATH = "proof/compiler/adapters/observe.mjs";
const FORMAL_DISCHARGE_PATH = "proof/compiler/adapters/discharge.json";
const FORMAL_DISCHARGE_SCHEMA = "jet.adapters-formal-discharge.v1";
const AUTHORITY_KEY = "compiler-proof.adapters";
const ADAPTER_MODEL_ROOT = "proof/compiler/adapters/models";
const ADAPTER_MODEL_MARKER = "JET_ADAPTER_MODEL_V1:";
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
const FORMAL_REQUIRED_FIELDS = Object.freeze([
  "operation_id",
  "mode",
  "method",
  "replay_ref",
  "identity_fields",
  "identity",
  "universal_proof",
]);
const MAPPING_STATUSES = Object.freeze(["mapped_unproved", "unsupported", "unmapped", "proved"]);
const DISPOSITION_SET = new Set(["mapped_unproved", "unsupported", "unmapped", "proved"]);
const ROUTE_MODES = Object.freeze(["aot", "jet_run", "interpreter", "web", "comptime"]);
const TARGET_IDS = Object.freeze(["aot", "jet_run", "interpreter", "web", "comptime"]);
const TARGET_OBLIGATION_IDS = Object.freeze({
  aot: "adapter:rust-aot",
  jet_run: "adapter:cranelift",
  interpreter: "adapter:interpreter",
  web: "adapter:web",
});
const REQUIRED_NEGATIVE_CATEGORIES = Object.freeze([
  "wrong-abi-width",
  "wrong-layout",
  "wrong-event-order",
  "callback-lifetime",
  "failure-mapping",
  "source-identity",
  "target-identity",
  "tool-configuration",
  "forged-approval",
  "wrong-operation",
  "wrong-model",
  "wrong-target",
  "unrelated-valid-proof",
  "incomplete-coverage",
]);
const REQUIRED_FOREIGN_IDS = Object.freeze([
  "foreign.rustc-llvm",
  "foreign.cranelift",
  "foreign.assembler-linker",
  "foreign.browser-vm",
  "foreign.library",
  "foreign.physical-environment",
]);
const SOURCE_ROOTS = Object.freeze([
  "crates/jet-codegen/src/Prelude",
  "crates/jet-foundation/src",
  "crates/jet-jit/src",
  "crates/jet-comptime/src",
  "crates/jet-rt/src",
  "Source",
]);

class CheckError extends Error {
  constructor(code, message, details = undefined) {
    super(message);
    this.name = "CheckError";
    this.code = code;
    this.details = details;
  }
}

function fail(code, message, details = undefined) {
  throw new CheckError(code, message, details);
}

function deepEqual(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function sorted(values) {
  return [...values].sort();
}

function unique(values) {
  return [...new Set(values)];
}

function requireArray(value, label, { nonempty = false } = {}) {
  if (!Array.isArray(value) || (nonempty && value.length === 0)) {
    fail("invalid_oracle", `${label} must be ${nonempty ? "a non-empty " : "an "}array`);
  }
  return value;
}

function requireObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail("invalid_oracle", `${label} must be an object`);
  }
  return value;
}

function requireText(value, label) {
  if (typeof value !== "string" || value.length === 0) fail("invalid_oracle", `${label} must be non-empty text`);
  return value;
}

function digestPath(path) {
  const project = projectPath(path, "digest path");
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail("unavailable", `digest path is missing: ${project}`);
  const stat = statSync(absolute);
  return stat.isDirectory() ? treeDigest(project) : fileDigest(project);
}

const CHECKER_PATH = "proof/compiler/adapters/check.mjs";
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

function digestValue(value) {
  return digestText(canonicalJson(value));
}
function hardeningDigest(value) {
  return digestText(canonicalJson(value)).replace(/^sha256-/u, "sha256:");
}

function digestRecords(paths) {
  return unique(paths).sort().map((path) => ({ path, sha256: digestPath(path) }));
}

function adapterIdentity({ operation, sourcePaths, artifact, target, toolConfiguration }) {
  const sources = digestRecords(sourcePaths);
  const modelRecords = digestRecords([CORE_PATH, OBLIGATIONS_PATH]);
  const compilerRecords = digestRecords(["scripts/agent/compiler-proof.mjs", PROOF_MANIFEST_PATH]);
  const model_sha256 = digestValue(modelRecords);
  const source_sha256 = digestValue(sources);
  const shared_operation_sha256 = digestValue(operation);
  const adapter_artifact_sha256 = digestValue(artifact);
  const target_sha256 = digestValue(target);
  const tool_configuration_sha256 = digestValue(toolConfiguration);
  const program_sha256 = digestValue({
    model_sha256,
    source_sha256,
    shared_operation_sha256,
    adapter_artifact_sha256,
    target_sha256,
    tool_configuration_sha256,
    compiler: compilerRecords,
  });
  return {
    program_sha256,
    model_sha256,
    source_sha256,
    shared_operation_sha256,
    adapter_artifact_sha256,
    target_sha256,
    tool_configuration_sha256,
    checker_sha256: digestPath(CHECKER_PATH),
    obligations_sha256: digestPath(OBLIGATIONS_PATH),
    model_records: modelRecords,
    source_records: sources,
    compiler: { runner: RUNNER_PATH, compiler: COMPILER_PATH, proof_manifest: PROOF_MANIFEST_PATH, records: compilerRecords },
  };
}
function assertPath(path, label, { file = false } = {}) {
  const project = projectPath(path, label);
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail("unavailable", `${label} is missing: ${project}`, { path: project });
  if (file && !statSync(absolute).isFile()) fail("invalid_oracle", `${label} must be a file: ${project}`);
  return project;
}

function assertSourceMarkers(path, markers, label) {
  const project = assertPath(path, label, { file: true });
  const source = readFileSync(resolve(ROOT, project), "utf8");
  const expected = requireArray(markers, `${label}.markers`, { nonempty: true });
  const missing = expected.filter((marker) => typeof marker !== "string" || marker.length === 0 || !source.includes(marker));
  if (missing.length > 0) {
    fail("mismatch", `${label} is missing required source marker(s)`, { path: project, missing });
  }
  return {
    path: project,
    sha256: digestBytes(Buffer.from(source, "utf8")),
    markers: [...expected],
  };
}

function lineForOffset(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function walkFiles(path) {
  const project = projectPath(path, "source root");
  const absolute = resolve(ROOT, project);
  if (!existsSync(absolute)) fail("unavailable", `source root is missing: ${project}`);
  if (statSync(absolute).isFile()) return [project];
  const result = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === ".git" || entry.name === "target" || entry.name === "node_modules") continue;
      const child = join(directory, entry.name);
      if (entry.isDirectory()) visit(child);
      else if (entry.isFile()) result.push(relative(ROOT, child).replaceAll("\\", "/"));
    }
  };
  visit(absolute);
  return result.sort();
}

const SOURCE_FILES = new Map();
const SOURCE_TEXT = new Map();
function sourceFiles() {
  if (SOURCE_FILES.size > 0) return [...SOURCE_FILES.values()].flat();
  for (const root of SOURCE_ROOTS) SOURCE_FILES.set(root, walkFiles(root));
  return [...SOURCE_FILES.values()].flat();
}
function sourceText(path) {
  const project = projectPath(path, "source file");
  if (!SOURCE_TEXT.has(project)) SOURCE_TEXT.set(project, readFileSync(resolve(ROOT, project), "utf8"));
  return SOURCE_TEXT.get(project);
}

function routeSymbol(route) {
  return requireText(route, "dispatcher route").split("::").at(-1);
}

function definitionMatches(source, symbol) {
  const escaped = symbol.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const patterns = [
    { kind: "rust_fn", regex: new RegExp(`(?:^|\\n)\\s*(?:pub(?:\\([^)]*\\))?\\s+)?(?:(?:unsafe|async)\\s+)*(?:extern\\s+\"[^\"]+\"\\s+)?fn\\s+${escaped}(?:\\s*<[^>]*>)?\\s*\\(`, "gu") },
    { kind: "javascript_function", regex: new RegExp(`(?:^|\\n)\\s*(?:async\\s+)?function\\s+${escaped}\\s*\\(`, "gu") },
    { kind: "javascript_binding", regex: new RegExp(`(?:^|\\n)\\s*(?:const|let|var)\\s+${escaped}\\s*=`, "gu") },
  ];
  const matches = [];
  for (const { kind, regex } of patterns) {
    for (const match of source.matchAll(regex)) {
      matches.push({ kind, line: lineForOffset(source, match.index + (match[0].startsWith("\n") ? 1 : 0)) });
    }
  }
  return matches;
}

function bindRoute(route) {
  const symbol = routeSymbol(route);
  const definitions = [];
  for (const path of sourceFiles()) {
    const matches = definitionMatches(sourceText(path), symbol);
    for (const match of matches) definitions.push({ path, symbol, ...match });
  }
  return { route, symbol, definitions };
}

function checkContract(contract, witnesses) {
  requireObject(contract, "adapter contract");
  if (contract.schema !== "jet.adapters-contract.v1" || contract.schema_version !== 1) {
    fail("invalid_oracle", "adapter contract schema/version is invalid");
  }
  if (contract.stage !== STAGE || contract.contract_id !== "jet-adapters-contract-v1") {
    fail("invalid_oracle", "adapter contract stage or id is invalid");
  }
  const policy = requireObject(contract.policy, "adapter contract policy");
  for (const [field, expected] of Object.entries({ decision_id: "D-COMPILER-PROOF1", tier_decision: "D-TIER-ONEIR1", outcome: "A", status: "ratified" })) {
    if (policy[field] !== expected) fail("invalid_oracle", `adapter contract policy.${field} drifted`);
  }
  requireText(policy.scope, "adapter contract policy.scope");
  requireText(policy.foreign_suffix, "adapter contract policy.foreign_suffix");

  const claim = requireObject(contract.claim, "adapter contract claim");
  for (const field of ["kind", "statement"]) requireText(claim[field], `adapter contract claim.${field}`);
  for (const field of ["preconditions", "unsupported_cases"]) {
    const values = requireArray(claim[field], `adapter contract claim.${field}`, { nonempty: true });
    if (values.some((value) => typeof value !== "string" || value.length === 0)) fail("invalid_oracle", `adapter contract claim.${field} contains invalid text`);
  }
  const observations = requireArray(claim.observation_alphabet, "adapter contract claim.observation_alphabet", { nonempty: true });
  if (!deepEqual(sorted(observations), sorted(OBSERVATION_FIELDS))) fail("invalid_oracle", "adapter observation alphabet is incomplete or reordered");
  const dimensions = requireArray(claim.adapter_dimensions, "adapter contract claim.adapter_dimensions", { nonempty: true });
  if (!deepEqual(sorted(dimensions), sorted(ADAPTER_DIMENSIONS))) fail("invalid_oracle", "adapter dimension alphabet is incomplete or reordered");

  const execution = requireObject(contract.execution, "adapter contract execution");
  const certificateConfig = requireObject(execution.observation_certificates, "adapter execution.observation_certificates");
  if (certificateConfig.path !== OBSERVATION_CERTIFICATE_PATH || certificateConfig.schema !== OBSERVATION_CERTIFICATE_SCHEMA || certificateConfig.producer !== OBSERVATION_PRODUCER_PATH || certificateConfig.positive_status !== "verified" || certificateConfig.universal_proof !== false) {
    fail("invalid_oracle", "adapter observation certificate execution binding drifted");
  }
  if (!deepEqual(execution.operations, ["check", "run"]) || !deepEqual(execution.tiers, ["aot", "jet_run", "interpreter", "web", "comptime"])) {
    fail("invalid_oracle", "adapter execution operations or tiers drifted");
  }
  if (!Number.isSafeInteger(execution.timeout_ms) || execution.timeout_ms < 1) fail("invalid_oracle", "adapter execution timeout is invalid");
  requireText(execution.resource_policy, "adapter execution.resource_policy");
  requireText(certificateConfig.producer, "adapter observation certificate producer");
  if (!deepEqual(certificateConfig.required_identity_fields, IDENTITY_FIELDS) || !deepEqual(certificateConfig.required_observations, OBSERVATION_FIELDS)) {
    fail("invalid_oracle", "adapter observation certificate alphabets drifted");
  }
  if (!deepEqual(certificateConfig.certificate_fields, ["schema", "status", "producer", "execution_id", "checker", "runner", "observation_sha256", "identity_sha256"])) {
    fail("invalid_oracle", "adapter observation certificate fields drifted");
  }
  const formalConfig = requireObject(execution.formal_discharge, "adapter execution.formal_discharge");
  if (formalConfig.path !== FORMAL_DISCHARGE_PATH || formalConfig.schema !== FORMAL_DISCHARGE_SCHEMA || formalConfig.universal_proof !== false || formalConfig.status_when_absent !== "unavailable") {
    fail("invalid_oracle", "adapter formal discharge binding drifted");
  }
  if (!deepEqual(formalConfig.methods, ["direct_verification", "translation_certificate"])) {
    fail("invalid_oracle", "adapter formal discharge methods drifted");
  }
  if (!deepEqual(formalConfig.required_fields, FORMAL_REQUIRED_FIELDS)) {
    fail("invalid_oracle", "adapter formal discharge required fields drifted");
  }

  const coverage = requireObject(contract.coverage, "adapter contract coverage");
  if (coverage.relation_path !== OBLIGATIONS_PATH || coverage.owner !== "#2941") fail("invalid_oracle", "adapter coverage relation or owner drifted");
  if (!deepEqual(coverage.mapping_statuses, MAPPING_STATUSES) || coverage.universal_proof !== false) fail("invalid_oracle", "adapter coverage policy drifted");
  requireText(coverage.owner_rule, "adapter coverage.owner_rule");
  requireText(coverage.qualification_rule, "adapter coverage.qualification_rule");

  if (identity.schema !== "jet.compiler-proof.v1"
      || identity.manifest_path !== PROOF_MANIFEST_PATH
      || identity.authority_key !== AUTHORITY_KEY
      || identity.claim_id !== "compiler-proof.identity-direct") {
    fail("invalid_oracle", "adapter compiler-proof identity binding drifted");
  }
  const requiredIdentity = requireArray(identity.required_fields, "adapter identity.required_fields", { nonempty: true });
  for (const field of ["claim.identity_sha256", "candidate.record_identity.target_inputs_sha256", "candidate.record_identity.tool_version", "candidate.record_identity.engine", "candidate.commit", "candidate.compiler.source", "candidate.compiler.artifact", "candidate.configuration", "candidate.target", "checker.toolchain_pin_sha256", "proof.artifact", "raw_replay"]) {
    if (!requiredIdentity.includes(field)) fail("invalid_oracle", `adapter identity omits ${field}`);
  }

  const methods = requireArray(contract.binding_methods, "adapter contract binding_methods", { nonempty: true });
  const methodNames = new Set(methods.map((method) => requireObject(method, "adapter binding method").method));
  for (const method of ["verified_construction", "direct_verification", "translation_certificate"]) {
    if (!methodNames.has(method)) fail("invalid_oracle", `adapter contract omits ${method}`);
  }

  const premises = requireArray(contract.memory_premises, "adapter contract memory_premises", { nonempty: true });
  const premiseIds = new Set();
  for (const premise of premises) {
    requireObject(premise, "adapter memory premise");
    const id = requireText(premise.id, "adapter memory premise.id");
    if (premiseIds.has(id)) fail("invalid_oracle", `duplicate adapter memory premise ${id}`);
    premiseIds.add(id);
    assertPath(premise.source, `adapter memory premise ${id}.source`, { file: true });
    requireText(premise.statement, `adapter memory premise ${id}.statement`);
  }
  for (const id of ["memory-safe-call", "memory-checked-allocation", "memory-receiver-lifetime"]) {
    if (!premiseIds.has(id)) fail("invalid_oracle", `adapter contract omits memory premise ${id}`);
  }

  const dependencies = requireArray(contract.dependencies, "adapter contract dependencies", { nonempty: true });
  const dependencyRecords = [];
  const dependencyPaths = new Set();
  for (const dependency of dependencies) {
    requireObject(dependency, "adapter dependency");
    const path = assertPath(dependency.path, "adapter dependency.path");
    requireText(dependency.kind, `adapter dependency ${path}.kind`);
    dependencyPaths.add(path);
    dependencyRecords.push({ path, kind: dependency.kind, sha256: digestPath(path) });
  }
  for (const path of [CHECKER_PATH, "scripts/agent/compiler-proof.mjs"]) {
    if (!dependencyPaths.has(path)) fail("invalid_oracle", `adapter contract dependencies omit ${path}`);
  }
  if (contract.witnesses_sha256 !== digestPath(WITNESSES_PATH)) fail("invalid_oracle", "adapter contract witness digest is stale", { expected: digestPath(WITNESSES_PATH), actual: contract.witnesses_sha256 });

  const assumptions = requireArray(contract.foreign_assumptions, "adapter contract foreign_assumptions", { nonempty: true });
  const assumptionIds = new Set();
  for (const assumption of assumptions) {
    requireObject(assumption, "adapter foreign assumption");
    const id = requireText(assumption.id, "adapter foreign assumption.id");
    if (assumptionIds.has(id)) fail("invalid_oracle", `duplicate adapter foreign assumption ${id}`);
    assumptionIds.add(id);
    requireText(assumption.kind, `${id}.kind`);
    requireText(assumption.edge, `${id}.edge`);
    requireText(assumption.contract, `${id}.contract`);
  }
  for (const id of REQUIRED_FOREIGN_IDS) if (!assumptionIds.has(id)) fail("invalid_oracle", `adapter contract omits foreign assumption ${id}`);

  const boundary = requireObject(contract.boundary, "adapter contract boundary");
  for (const field of ["proven", "assumed", "tested", "blocked"]) requireArray(boundary[field], `adapter contract boundary.${field}`, { nonempty: true });
  if (boundary.qualification !== "blocked" || boundary.universal_proof !== false) fail("invalid_oracle", "adapter contract boundary must remain blocked and non-universal");
  return { dependencyRecords, assumptions, identity };
}

function checkWitnesses(witnesses, contract) {
  requireObject(witnesses, "adapter witnesses");
  if (witnesses.schema !== "jet.adapters-witnesses.v1" || witnesses.schema_version !== 1 || witnesses.stage !== STAGE) {
    fail("invalid_oracle", "adapter witness schema/version/stage is invalid");
  }
  if (!deepEqual(witnesses.observation_fields, OBSERVATION_FIELDS)) fail("invalid_oracle", "adapter witness observation fields drifted");
  if (!deepEqual(witnesses.obligation_dimensions, ADAPTER_DIMENSIONS)) fail("invalid_oracle", "adapter witness dimensions drifted");
  const identityFields = requireArray(witnesses.identity_fields, "adapter witness identity_fields", { nonempty: true });
  if (!deepEqual(sorted(identityFields), sorted(IDENTITY_FIELDS))) fail("invalid_oracle", "adapter witness identity fields drifted");
  for (const field of IDENTITY_FIELDS) {
    if (!identityFields.includes(field)) fail("invalid_oracle", `adapter witnesses omit identity field ${field}`);
  }
  const identity = requireObject(witnesses.identity, "adapter witness identity");
  if (identity.authority_key !== AUTHORITY_KEY) fail("invalid_oracle", "adapter witness authority binding drifted");
  for (const key of ["model", "source", "compiler", "checker", "configuration", "target", "obligations"]) {
    const record = requireObject(identity[key], `adapter witness identity.${key}`);
    requireText(record.meaning, `adapter witness identity.${key}.meaning`);
  }
  const runner = requireObject(witnesses.runner, "adapter witness runner");
  if (runner.launcher !== RUNNER_PATH || runner.compiler !== COMPILER_PATH || runner.scratch.includes("/tmp")) fail("invalid_oracle", "adapter witness runner identity or scratch policy drifted");
  if (!deepEqual(runner.supported_operations, ["check", "run"]) || !deepEqual(runner.tiers, ["aot", "jet_run", "interpreter", "web", "comptime"])) fail("invalid_oracle", "adapter witness runner operations or tiers drifted");
  if (!Number.isSafeInteger(runner.timeout_ms) || runner.timeout_ms !== 120000) fail("invalid_oracle", "adapter witness timeout drifted");
  requireText(runner.invocation, "adapter witness runner.invocation");
  requireText(runner.unknown_policy, "adapter witness runner.unknown_policy");

  const observationCertificates = requireObject(witnesses.observation_certificates, "adapter observation_certificates");
  if (observationCertificates.path !== OBSERVATION_CERTIFICATE_PATH || observationCertificates.schema !== OBSERVATION_CERTIFICATE_SCHEMA || observationCertificates.producer !== OBSERVATION_PRODUCER_PATH || observationCertificates.universal_proof !== false) {
    fail("invalid_oracle", "adapter observation certificate carrier binding drifted");
  }
  if (observationCertificates.status !== "unverified") fail("invalid_oracle", "adapter observation certificate witness status must remain unverified");
  const formalDischarge = requireObject(witnesses.formal_discharge, "adapter formal_discharge");
  if (formalDischarge.path !== FORMAL_DISCHARGE_PATH || formalDischarge.schema !== FORMAL_DISCHARGE_SCHEMA || formalDischarge.status_when_absent !== "unavailable" || formalDischarge.universal_proof !== false || !deepEqual(formalDischarge.methods, ["direct_verification", "translation_certificate"])) {
    fail("invalid_oracle", "adapter formal discharge carrier binding drifted");
  }
  if (!deepEqual(formalDischarge.required_fields, FORMAL_REQUIRED_FIELDS)) {
    fail("invalid_oracle", "adapter formal discharge required fields drifted");
  }
  const coverage = requireObject(witnesses.coverage, "adapter witness coverage");
  if (coverage.relation !== OBLIGATIONS_PATH || coverage.owner !== "#2941") fail("invalid_oracle", "adapter witness coverage relation or owner drifted");
  if (!deepEqual(coverage.mapping_statuses, MAPPING_STATUSES)) fail("invalid_oracle", "adapter witness mapping statuses drifted");
  requireText(coverage.qualification_rule, "adapter witness qualification_rule");

  const relation = requireObject(witnesses.capability_relation, "adapter witness capability_relation");
  if (relation.manifest_path !== CAPABILITY_MANIFEST_PATH || relation.field !== "capability_relation" || relation.schema !== "jet.capability.relation.v1" || relation.key_format !== "<capability_id>@<mode>") {
    fail("invalid_oracle", "adapter witness capability relation binding drifted");
  }
  for (const field of ["source_snapshot_field", "unknown_policy"]) requireText(relation[field], `adapter witness capability_relation.${field}`);
  const relationFields = requireArray(relation.required_fields, "adapter witness capability_relation.required_fields", { nonempty: true });
  for (const field of ["capability_id", "family", "mode", "contract", "route", "observable", "oracle", "evidence", "identity", "owner", "disposition", "source_identity", "candidate_identity", "owner_link", "status", "counted"]) {
    if (!relationFields.includes(field)) fail("invalid_oracle", `adapter witness capability relation omits ${field}`);
  }

  const roots = requireArray(witnesses.route_binding.implementation_roots, "adapter witness implementation roots", { nonempty: true });
  for (const root of roots) assertPath(root, "adapter implementation root");
  if (witnesses.route_binding.dispatcher !== CORE_PATH) fail("invalid_oracle", "adapter dispatcher path drifted");
  requireText(witnesses.route_binding.parser, "adapter route parser");
  const targetBindings = requireArray(witnesses.target_bindings, "adapter target_bindings", { nonempty: true });
  const targets = new Map();
  for (const binding of targetBindings) {
    requireObject(binding, "adapter target binding");
    const id = requireText(binding.id, "adapter target binding.id");
    if (targets.has(id)) fail("invalid_oracle", `duplicate adapter target binding ${id}`);
    targets.set(id, binding);
    requireText(binding.artifact_target, `${id}.artifact_target`);
    requireText(binding.target_label, `${id}.target_label`);
    requireText(binding.kind, `${id}.kind`);
    const markers = requireObject(binding.source_markers, `${id}.source_markers`);
    for (const [path, values] of Object.entries(markers)) assertSourceMarkers(path, values, `${id}.source_markers.${path}`);
    const tools = requireArray(binding.tool_configuration, `${id}.tool_configuration`, { nonempty: true });
    for (const tool of tools) {
      requireObject(tool, `${id}.tool_configuration entry`);
      requireText(tool.kind, `${id}.tool_configuration.kind`);
      requireText(tool.assumption, `${id}.tool_configuration.assumption`);
      for (const path of requireArray(tool.paths, `${id}.tool_configuration.paths`, { nonempty: true })) assertPath(path, `${id}.tool_configuration path`);
    }
  }
  for (const id of TARGET_IDS) if (!targets.has(id)) fail("invalid_oracle", `adapter witnesses omit target binding ${id}`);

  const fixturePaths = requireArray(witnesses.fixtures, "adapter witness fixtures", { nonempty: true });
  const fixtures = fixturePaths.map((path) => ({ path: assertPath(path, "adapter fixture", { file: true }), sha256: digestPath(path) }));
  if (!deepEqual(fixturePaths, fixtures.map((fixture) => fixture.path))) fail("invalid_oracle", "adapter fixture paths are not canonical");
  const assumptions = requireArray(witnesses.foreign_assumptions, "adapter witness foreign_assumptions", { nonempty: true });
  const assumptionIds = new Set(assumptions.map((assumption) => requireText(assumption.id, "adapter witness foreign assumption.id")));
  for (const id of REQUIRED_FOREIGN_IDS) if (!assumptionIds.has(id)) fail("invalid_oracle", `adapter witnesses omit foreign assumption ${id}`);
  return { runner, coverage, relation, targetBindings, targets, fixtures, identityFields, identity, assumptions, observationCertificates, formalDischarge };
}

function checkSameObservationFixture(path) {
  const fixture = readJson(path, "adapter same-observation fixture");
  if (fixture.schema !== "jet.adapters-fixtures.v1" || fixture.schema_version !== 1) fail("invalid_oracle", "same-observation fixture schema is invalid");
  if (fixture.fixture_policy?.universal_proof !== false) fail("invalid_oracle", "same-observation fixture cannot claim universal proof");
  if (!deepEqual(fixture.observation_fields, OBSERVATION_FIELDS) || !deepEqual(sorted(fixture.dimensions), sorted(ADAPTER_DIMENSIONS))) fail("invalid_oracle", "same-observation fixture alphabets drifted");
  const cases = requireArray(fixture.cases, "same-observation fixture cases", { nonempty: true });
  const ids = new Set();
  const covered = new Set();
  for (const entry of cases) {
    requireObject(entry, "same-observation fixture entry");
    const id = requireText(entry.id, "same-observation fixture entry.id");
    if (ids.has(id)) fail("invalid_oracle", `duplicate same-observation fixture ${id}`);
    ids.add(id);
    requireText(entry.route_difference, `${id}.route_difference`);
    if (entry.status !== "unobserved") fail("invalid_oracle", `${id} must remain unobserved until the proof workflow executes it`);
    for (const source of requireArray(entry.source_refs, `${id}.source_refs`, { nonempty: true })) assertPath(source, `${id}.source_ref`, { file: true });
    for (const dimension of requireArray(entry.dimensions, `${id}.dimensions`, { nonempty: true })) {
      if (!ADAPTER_DIMENSIONS.includes(dimension)) fail("invalid_oracle", `${id} names unknown adapter dimension ${dimension}`);
      covered.add(dimension);
    }
    const observation = requireObject(entry.observation, `${id}.observation`);
    if (!deepEqual(sorted(Object.keys(observation)), sorted(OBSERVATION_FIELDS))) fail("invalid_oracle", `${id}.observation is incomplete`);
    for (const field of OBSERVATION_FIELDS) requireText(observation[field], `${id}.observation.${field}`);
  }
  const missing = ADAPTER_DIMENSIONS.filter((dimension) => !covered.has(dimension));
  if (missing.length > 0) fail("invalid_oracle", "same-observation fixtures omit adapter dimensions", { missing });
  return { schema: fixture.schema, count: cases.length, ids: [...ids].sort(), sha256: digestPath(path), status: "unobserved", universal_proof: false };
}

function checkNegativeFixture(path, obligations) {
  const fixture = readJson(path, "adapter negative-control fixture");
  if (fixture.schema !== "jet.adapters-negative-controls.v1" || fixture.schema_version !== 1 || fixture.universal_proof !== false) fail("invalid_oracle", "negative-control fixture schema/policy is invalid");
  const controls = requireArray(fixture.controls, "adapter negative controls", { nonempty: true });
  const ids = new Set();
  const categories = new Set();
  const obligationIds = new Set(obligations.rows.filter((row) => row.owner === "#2941").map((row) => row.id));
  for (const control of controls) {
    requireObject(control, "adapter negative control");
    const id = requireText(control.id, "adapter negative control.id");
    if (ids.has(id)) fail("invalid_oracle", `duplicate adapter negative control ${id}`);
    ids.add(id);
    const category = requireText(control.category, `${id}.category`);
    categories.add(category);
    for (const obligation of requireArray(control.obligations, `${id}.obligations`, { nonempty: true })) {
      if (!obligationIds.has(obligation) && !["boundary:ffi", "boundary:layout"].includes(obligation)) fail("invalid_oracle", `${id} references unknown obligation ${obligation}`);
    }
    for (const source of requireArray(control.source_refs, `${id}.source_refs`, { nonempty: true })) assertPath(source, `${id}.source_ref`, { file: true });
    requireObject(control.mutation, `${id}.mutation`);
    requireText(control.mutation.property, `${id}.mutation.property`);
    requireText(control.mutation.operation, `${id}.mutation.operation`);
    requireArray(control.mutation.must_change, `${id}.mutation.must_change`, { nonempty: true });
    const expected = requireObject(control.expected, `${id}.expected`);
    if (expected.status !== "mismatch" || expected.universal_proof !== false) fail("invalid_oracle", `${id} must expect mismatch without universal proof`);
    requireText(expected.reason, `${id}.expected.reason`);
  }
  const missing = REQUIRED_NEGATIVE_CATEGORIES.filter((category) => !categories.has(category));
  if (missing.length > 0) fail("invalid_oracle", "negative controls omit required adapter categories", { missing });
  return { schema: fixture.schema, count: controls.length, ids: [...ids].sort(), categories: [...categories].sort(), sha256: digestPath(path), status: "unobserved", universal_proof: false };
}

function checkCapabilityRelation() {
  const manifest = readJson(CAPABILITY_MANIFEST_PATH, "canonical capability manifest");
  if (manifest.schema !== "jet.hardening.surface.v1" || manifest.schema_version !== 1) fail("invalid_oracle", "canonical capability manifest schema is invalid");
  const relation = manifest.capability_relation;
  if (!relation || typeof relation !== "object" || Array.isArray(relation)) fail("unavailable", "canonical capability relation is unavailable", { manifest_path: CAPABILITY_MANIFEST_PATH, field: "capability_relation" });
  if (relation.schema !== "jet.capability.relation.v1" || relation.schema_version !== 1) fail("invalid_oracle", "canonical capability relation schema/version is invalid");
  const relationDigestInput = { ...relation };
  delete relationDigestInput.content_digest;
  if (relation.content_digest !== hardeningDigest(relationDigestInput)) fail("mismatch", "capability relation content digest is stale");
  requireArray(relation.identities, "canonical capability relation identities", { nonempty: true });
  const sourceSnapshotHash = text(manifest.source_snapshot?.hash, "canonical capability source_snapshot.hash");
  if (relation.source_snapshot_hash !== sourceSnapshotHash) fail("mismatch", "capability relation source snapshot is stale", { expected: sourceSnapshotHash, actual: relation.source_snapshot_hash });
  const rows = requireArray(relation.rows, "canonical capability relation rows", { nonempty: true });
  const rowIds = new Set();
  for (const row of rows) {
    requireObject(row, "canonical capability relation row");
    const rowId = requireText(row.row_id, "capability relation row_id");
    if (rowIds.has(rowId)) fail("invalid_oracle", `duplicate capability relation row ${rowId}`);
    rowIds.add(rowId);
    requireText(row.owner_link, `capability relation ${rowId}.owner_link`);
    requireText(row.status, `capability relation ${rowId}.status`);
    if (row.status !== row.disposition || row.counted !== true) fail("invalid_oracle", `capability relation ${rowId} status/counting is not canonical`);
    for (const field of ["capability_id", "family", "mode", "disposition"]) requireText(row[field], `capability relation ${rowId}.${field}`);
    for (const field of ["contract", "route", "observable", "oracle", "evidence", "identity", "owner", "source_identity", "candidate_identity"]) requireObject(row[field], `capability relation ${rowId}.${field}`);
    if (!["planned", "implemented-unqualified", "passed", "failed", "stale", "unavailable", "unsupported", "owner-ratified-not-applicable"].includes(row.disposition)) fail("invalid_oracle", `capability relation ${rowId} has unsupported disposition ${row.disposition}`);
    requireText(row.owner.card, `capability relation ${rowId}.owner.card`);
    if (row.row_id !== `${row.capability_id}@${row.mode}`) fail("invalid_oracle", `capability relation row key is not canonical: ${rowId}`);
  }
  const denominator = requireObject(relation.denominator, "canonical capability relation denominator");
  if (denominator.row_count !== rows.length || denominator.counted_rows !== rows.filter((row) => row.counted === true).length) {
    fail("mismatch", "capability relation denominator counts are stale", { row_count: rows.length, counted_rows: rows.filter((row) => row.counted === true).length });
  }
  return {
    manifest_schema: manifest.schema,
    manifest_sha256: digestPath(CAPABILITY_MANIFEST_PATH),
    source_snapshot_hash: sourceSnapshotHash,
    schema: relation.schema,
    schema_version: relation.schema_version,
    relation_sha256: digestText(canonicalJson(relation)),
    denominator: relation.denominator ?? null,
    rows,
    row_ids: [...rowIds].sort(),
  };
}

function capabilityMatches(relation, candidates, mode) {
  const values = new Set(candidates.filter((candidate) => typeof candidate === "string" && candidate.length > 0));
  return relation.rows.filter((row) => row.mode === mode && (values.has(row.capability_id) || values.has(row.row_id.split("@")[0])));
}

function capabilityRecord(relation, candidates, mode) {
  const matches = capabilityMatches(relation, candidates, mode);
  if (matches.length === 0) {
    return {
      mode,
      status: "unavailable",
      applicable: true,
      required: true,
      bound: false,
      reason: "missing_relation_row",
      rows: [],
    };
  }
  if (matches.length > 1) {
    return {
      mode,
      status: "mismatch",
      applicable: true,
      required: true,
      bound: false,
      rows: matches.map((row) => row.row_id),
    };
  }
  const row = matches[0];
  if (row.disposition === "owner-ratified-not-applicable") {
    return {
      mode,
      status: "owner-ratified-not-applicable",
      applicable: false,
      required: false,
      bound: true,
      row_id: row.row_id,
      disposition: row.disposition,
      identity: row.identity,
    };
  }
  const qualified = ["implemented-unqualified", "passed"].includes(row.disposition);
  return {
    mode,
    status: qualified ? "applicable-unqualified" : "required-unqualified",
    applicable: true,
    required: true,
    bound: true,
    row_id: row.row_id,
    disposition: row.disposition,
    identity: row.identity,
  };
}
function operationCapabilityRecord(relation, candidates, dispatcher, mode) {
  const directTarget = mode === "aot" ? "aot" : mode === "jet_run" ? "jit" : null;
  if (directTarget && (dispatcher.dispatcher.options ?? []).some((option) => option.kind === `without_direct_${directTarget}`)) {
    return { mode, status: "dispatcher-not-applicable", applicable: false, required: false, bound: true, reason: `dispatcher_without_direct_${directTarget}` };
  }
  return capabilityRecord(relation, candidates, mode);
}

function checkRuntimeDependency(stage, contractPath, witnessesPath, contractSchema) {
  const contract = readJson(contractPath, `${stage} dependency contract`);
  const witnesses = readJson(witnessesPath, `${stage} dependency witnesses`);
  if (contract.schema !== contractSchema || contract.schema_version !== 1 || contract.stage !== stage) fail("invalid_oracle", `${stage} dependency contract schema/stage is invalid`);
  if (witnesses.schema !== "jet.core-runtime-witnesses.v1" || witnesses.schema_version !== 1 || witnesses.stage !== stage) fail("invalid_oracle", `${stage} dependency witness schema/stage is invalid`);
  if (witnesses.coverage?.owner !== "#2940") fail("invalid_oracle", `${stage} dependency witnesses are not owned by #2940`);
  if (contract.boundary?.qualification !== "blocked") fail("invalid_oracle", `${stage} dependency qualification is not blocked`);
  const fields = requireArray(witnesses.observation_fields, `${stage} dependency observation_fields`, { nonempty: true });
  for (const field of OBSERVATION_FIELDS) if (!fields.includes(field)) fail("invalid_oracle", `${stage} dependency omits observation field ${field}`);
  return {
    stage,
    contract_schema: contract.schema,
    contract_sha256: digestPath(contractPath),
    witnesses_schema: witnesses.schema,
    witnesses_sha256: digestPath(witnessesPath),
    relation: witnesses.coverage.relation,
    owner: witnesses.coverage.owner,
    observation_fields: fields,
    qualification: "blocked",
    universal_proof: false,
  };
}

function checkProofIdentity(contract) {
  const manifest = readJson(contract.identity.manifest_path, "pinned compiler-proof manifest");
  try {
    validateManifest(manifest);
  } catch (error) {
    fail("invalid_oracle", "pinned compiler-proof manifest failed validation", { error: String(error) });
  }
  const claim = manifest.claims?.find((candidate) => candidate.id === contract.identity.claim_id);
  if (!claim) fail("unavailable", `pinned compiler-proof claim is missing: ${contract.identity.claim_id}`);
  return {
    schema: manifest.schema,
    schema_version: manifest.schema_version,
    manifest_path: contract.identity.manifest_path,
    toolchain_pin_sha256: claim.checker?.toolchain_pin_sha256,
    claim_id: claim.id,
    identity_sha256: claim.identity_sha256 ?? identityDigest(claim),
    result: claim.result,
    qualification: "validated pinned identity only; this stage does not replay the proof claim",
  };
}

function checkTargetBindings(targets) {
  const result = {};
  for (const binding of targets) {
    const sourceBindings = [];
    for (const [path, markers] of Object.entries(binding.source_markers)) sourceBindings.push(assertSourceMarkers(path, markers, `${binding.id}.source_markers.${path}`));
    const configurations = [];
    for (const tool of binding.tool_configuration) {
      configurations.push({
        kind: tool.kind,
        assumption: tool.assumption,
        paths: tool.paths.map((path) => ({ path: assertPath(path, `${binding.id}.${tool.kind} configuration path`, { file: true }), sha256: digestPath(path) })),
      });
    }
    result[binding.id] = {
      artifact_target: binding.artifact_target,
      target_label: binding.target_label,
      kind: binding.kind,
      source_bindings: sourceBindings,
      tool_configuration: configurations,
      status: "source-bound-unproved",
      universal_proof: false,
    };
  }
  return result;
}

function checkObligationRelation(obligations) {
  if (obligations.schema !== "jet.compiler-obligations.v1" || obligations.schema_version !== 1) fail("invalid_oracle", "compiler obligation relation schema is invalid");
  let generated;
  try {
    generated = buildObligations();
  } catch (error) {
    fail("invalid_oracle", "current compiler obligation relation could not be regenerated", { error: String(error) });
  }
  if (!deepEqual(generated, obligations)) fail("mismatch", "compiler obligation relation is stale relative to its authoritative sources");
  const rows = requireArray(obligations.rows, "compiler obligation rows", { nonempty: true });
  const ownerRows = rows.filter((row) => row.owner === "#2941");
  if (ownerRows.length === 0) fail("unavailable", "compiler obligation relation has no #2941 rows");
  const ids = new Set();
  for (const row of ownerRows) {
    if (ids.has(row.id)) fail("invalid_oracle", `duplicate #2941 obligation ${row.id}`);
    ids.add(row.id);
  }
  return { schema: obligations.schema, schema_version: obligations.schema_version, relation_sha256: digestPath(OBLIGATIONS_PATH), rows: ownerRows, row_ids: [...ids].sort() };
}

function targetBindingFor(targets, targetId) {
  const binding = targets.get(targetId);
  if (!binding) fail("invalid_oracle", `missing target binding ${targetId}`);
  return binding;
}

function operationCandidates(row, dispatcher) {
  const route = dispatcher.dispatcher?.route_symbol ?? "";
  const moduleMember = `${dispatcher.dispatcher.module}.${dispatcher.dispatcher.member}`;
  return unique([row.id, row.subject, moduleMember, `module:${moduleMember}`, route, routeSymbol(route)]);
}

function targetCandidates(row, binding) {
  const slug = binding.target_label;
  const target = binding.artifact_target;
  return unique([row.id, row.subject, slug, target, `adapter:${slug}`, `adapter:${target}`, `compiler.adapter:${slug}`, `compiler.adapter:${target}`]);
}

function boundaryCandidates(row) {
  const suffix = row.id.startsWith("boundary:") ? row.id.slice("boundary:".length) : row.subject ?? row.id;
  return unique([row.id, row.subject, suffix, `boundary:${suffix}`, `adapter.boundary.${suffix}`]);
}

function capabilityModesForOperation(dispatcher) {
  const modes = ["aot", "jet_run", "interpreter", "web"];
  const options = dispatcher.dispatcher.options ?? [];
  if (options.some((option) => option.kind === "pure_route" || option.kind === "interpreter_route")) modes.push("comptime");
  return modes;
}

function sourceOperationBinding(row, dispatcher) {
  const route = dispatcher.dispatcher.route_symbol;
  const implementation = bindRoute(route);
  const options = dispatcher.dispatcher.options ?? [];
  const jitSymbols = options.filter((option) => option.kind === "jit_symbol").map((option) => option.value);
  const jitBindings = jitSymbols.map((symbol) => bindRoute(symbol));
  return {
    operation_id: row.id,
    subject: row.subject,
    source: row.source,
    source_line: row.source_line,
    route_symbol: route,
    route_definition: implementation,
    jit_symbols: jitBindings,
    options,
    implementation_bound: implementation.definitions.length > 0 && jitBindings.every((entry) => entry.definitions.length > 0),
    dimensions: [...ADAPTER_DIMENSIONS],
    status: implementation.definitions.length > 0 && jitBindings.every((entry) => entry.definitions.length > 0) ? "source-bound-unproved" : "route-unavailable",
    universal_proof: false,
  };
}

function sourceBoundaryBinding(row) {
  const refs = row.id === "boundary:ffi"
    ? ["crates/jet-jit/src/Ffi.rs", "crates/jet-codegen/src/Codegen/MIREval.rs", "crates/jet-codegen/src/Codegen/MIRWeb.rs"]
    : ["crates/jet-foundation/src/MIR.rs", "crates/jet-codegen/src/Codegen/MIRRust.rs", "crates/jet-jit/src/Ffi.rs"];
  return {
    id: row.id,
    source_refs: refs.map((path) => ({ path, sha256: digestPath(path) })),
    dimensions: [...ADAPTER_DIMENSIONS],
    status: "source-bound-unproved",
    universal_proof: false,
  };
}
function bindingSourcePaths(binding) {
  return (binding.source_bindings ?? []).map((entry) => entry.path);
}

function operationIdentity(operationRow, dispatcher, implementation, capability, targetBinding) {
  const routeDefinitions = [
    ...(implementation.route_definition?.definitions ?? []),
    ...implementation.jit_symbols.flatMap((entry) => entry.definitions ?? []),
  ];
  const sourcePaths = [
    operationRow.source,
    ...routeDefinitions.map((entry) => entry.path),
    ...bindingSourcePaths(targetBinding),
  ];
  return adapterIdentity({
    operation: {
      id: operationRow.id,
      subject: operationRow.subject,
      source: operationRow.source,
      source_line: operationRow.source_line,
      dispatcher: dispatcher.dispatcher,
    },
    artifact: {
      route_symbol: implementation.route_symbol,
      route_definition: implementation.route_definition,
      jit_symbols: implementation.jit_symbols,
      capability,
    },
    target: targetBinding,
    toolConfiguration: targetBinding.tool_configuration,
    sourcePaths,
  });
}

function targetIdentity(row, targetId, binding, capability) {
  return adapterIdentity({
    operation: {
      id: row.id,
      subject: row.subject,
      source: row.source,
      source_line: row.source_line,
    },
    artifact: {
      artifact_target: binding.artifact_target,
      target_label: binding.target_label,
      capability,
    },
    target: { id: targetId, ...binding },
    toolConfiguration: binding.tool_configuration,
    sourcePaths: bindingSourcePaths(binding),
  });
}

function boundaryIdentities(row, implementation, targets, capabilities) {
  const sourcePaths = implementation.source_refs.map((entry) => entry.path);
  return Object.fromEntries(ROUTE_MODES.map((mode) => {
    const target = targets.get(mode);
    const capability = capabilities.find((entry) => entry.mode === mode);
    return [mode, adapterIdentity({
      operation: { id: row.id, subject: row.subject, source: row.source },
      artifact: { boundary: row.id, source_refs: implementation.source_refs, capability },
      target: target ? { id: mode, ...target } : { id: mode },
      toolConfiguration: target?.tool_configuration ?? [],
      sourcePaths: [...sourcePaths, ...bindingSourcePaths(target ?? {})],
    })];
  }));
}

function annotateIdentity(implementation, identity) {
  return { ...implementation, identity_fields: [...IDENTITY_FIELDS], identity };
}


function buildAdapterRows(obligationInfo, dispatcherRows, relation, targets) {
  const dispatcherById = new Map(dispatcherRows.filter((row) => row.owner === "#2941").map((row) => [row.id, row]));
  const targetBindingById = new Map(Object.entries(targets));
  const entries = [];
  for (const row of obligationInfo.rows.filter((candidate) => candidate.owner === "#2941")) {
    if (row.family === "adapter" && row.kind === "execution_boundary") {
      const targetId = Object.entries(TARGET_OBLIGATION_IDS).find(([, id]) => id === row.id)?.[0];
      if (!targetId) fail("mismatch", `adapter target obligation has no target binding: ${row.id}`);
      const binding = targetBindingFor(targetBindingById, targetId);
      const capability = capabilityRecord(relation, targetCandidates(row, binding), targetId === "aot" ? "aot" : targetId);
      const route = annotateIdentity({
        operation_id: row.id,
        subject: row.subject,
        source: row.source,
        source_line: row.source_line,
        artifact_target: binding.artifact_target,
        target: targetId,
        capability,
        dimensions: [...ADAPTER_DIMENSIONS],
        status: capability.applicable && capability.bound ? "source-bound-unproved" : `target-applicability-${capability.status}`,
        universal_proof: false,
      }, targetIdentity(row, targetId, binding, capability));
      const disposition = capability.status === "mismatch" ? "unsupported" : !capability.bound ? "unmapped" : "mapped_unproved";
      const reason = capability.status === "mismatch"
        ? "capability relation has duplicate conflicting target rows"
        : capability.bound
          ? "target source and capability row are bound; observations remain unexecuted"
          : `target capability ${capability.status}`;
      entries.push({ ...row, disposition, disposition_reason: reason, implementation: route });
      continue;
    }
    if (row.family === "core" && row.kind === "adapter") {
      const dispatcher = dispatcherById.get(row.id);
      if (!dispatcher) fail("mismatch", `adapter obligation is missing its dispatcher row: ${row.id}`);
      const implementation = sourceOperationBinding(row, dispatcher);
      const modes = capabilityModesForOperation(dispatcher);
      const capabilities = modes.map((mode) => operationCapabilityRecord(relation, operationCandidates(row, dispatcher), dispatcher, mode));
      const routeMissing = !implementation.implementation_bound;
      const hasUnavailable = capabilities.some((entry) => entry.required && !entry.bound);
      const hasMismatch = capabilities.some((entry) => entry.status === "mismatch");
      const identities = Object.fromEntries(modes.map((mode, index) => {
        const target = targetBindingFor(targetBindingById, mode);
        return [mode, operationIdentity(row, dispatcher, implementation, capabilities[index], target)];
      }));
      const disposition = hasMismatch || routeMissing ? "unsupported" : hasUnavailable ? "unmapped" : "mapped_unproved";
      entries.push({ ...row, disposition, disposition_reason: routeMissing ? "dispatcher route or declared JIT symbol has no real source definition" : hasMismatch ? "capability relation has duplicate conflicting rows" : hasUnavailable ? "a required mode has no capability relation row; denominator is retained" : "dispatcher, route definitions, capability rows, and dimensions are source-bound; observations remain unexecuted", implementation: annotateIdentity({ ...implementation, capabilities }, identities) });
      continue;
    }
    if (row.family === "foreign" && row.kind === "boundary") {
      const implementation = sourceBoundaryBinding(row);
      const capabilities = ROUTE_MODES.map((mode) => capabilityRecord(relation, boundaryCandidates(row), mode));
      const hasMismatch = capabilities.some((entry) => entry.status === "mismatch");
      const hasUnavailable = capabilities.some((entry) => entry.required && !entry.bound);
      const disposition = hasMismatch ? "unsupported" : hasUnavailable ? "unmapped" : "mapped_unproved";
      entries.push({ ...row, disposition, disposition_reason: hasMismatch ? "capability relation has duplicate conflicting boundary rows" : hasUnavailable ? "a required boundary mode has no capability relation row; denominator is retained" : "boundary source and capability rows are source-bound; observations remain unexecuted", implementation: annotateIdentity({ ...implementation, capabilities }, boundaryIdentities(row, implementation, targetBindingById, capabilities)) });
      continue;
    }
    fail("mismatch", `unrecognized #2941 obligation shape: ${row.id}`);
  }
  const counts = Object.fromEntries(MAPPING_STATUSES.map((status) => [status, 0]));
  for (const entry of entries) {
    if (!DISPOSITION_SET.has(entry.disposition)) fail("invalid_oracle", `${entry.id} has invalid adapter disposition ${entry.disposition}`);
    counts[entry.disposition] += 1;
  }
  return { entries, counts };
}

function observationIdentity(entry, mode) {
  const identity = entry.implementation?.identity;
  if (!identity || typeof identity !== "object") fail("mismatch", `${entry.id} has no identity for observation mode ${mode}`);
  const value = identity[mode] && typeof identity[mode] === "object" ? identity[mode] : identity;
  requireObject(value, `${entry.id}.${mode}.identity`);
  return value;
}

function checkObservationCertificates(adapterRows) {
  const path = OBSERVATION_CERTIFICATE_PATH;
  if (!existsSync(resolve(ROOT, path))) {
    return {
      path,
      schema: OBSERVATION_CERTIFICATE_SCHEMA,
      status: "unverified",
      records: [],
      verified_keys: [],
      reason: "no identity-bound observation certificate was supplied by the compiler-proof workflow",
      universal_proof: false,
    };
  }
  const payload = readJson(path, "adapter observation certificates");
  if (payload.schema !== OBSERVATION_CERTIFICATE_SCHEMA || payload.schema_version !== 1 || payload.stage !== STAGE) {
    fail("invalid_oracle", "adapter observation certificate schema/stage is invalid");
  }
  if (payload.universal_proof !== false || payload.producer !== OBSERVATION_PRODUCER_PATH) fail("invalid_oracle", "adapter observation certificates cannot claim universal proof or use an unknown producer");
  if (!["verified", "matched", "unverified", "unavailable", "failed", "mismatch"].includes(payload.status)) {
    fail("invalid_oracle", `adapter observation carrier has unsupported status ${String(payload.status)}`);
  }
  const records = requireArray(payload.records, "adapter observation certificate records");
  if (payload.status === "verified" && records.length === 0) fail("mismatch", "verified adapter observation carrier has no positive records");
  const entriesById = new Map(adapterRows.entries.map((entry) => [entry.id, entry]));
  const seen = new Set();
  const verified = [];
  for (const record of records) {
    requireObject(record, "adapter observation certificate record");
    const id = requireText(record.operation_id, "adapter observation operation_id");
    const mode = requireText(record.mode, `${id}.mode`);
    const key = `${id}@${mode}`;
    if (seen.has(key)) fail("invalid_oracle", `duplicate adapter observation certificate ${key}`);
    if (!ROUTE_MODES.includes(mode)) fail("invalid_oracle", `${id} certificate names unknown mode ${mode}`);
    const entry = entriesById.get(id);
    if (!entry) fail("mismatch", `adapter observation certificate names unknown obligation ${id}`);
    const expectedIdentity = observationIdentity(entry, mode);
    const certificate = requireObject(record.certificate, `${key}.certificate`);
    if (certificate.schema !== "jet.adapter-observation-certificate.v1" || certificate.status !== "verified" || certificate.universal_proof !== false) {
      fail("invalid_oracle", `${key} certificate is not a supported positive certificate`);
    }
    if (certificate.producer !== OBSERVATION_PRODUCER_PATH) fail("invalid_oracle", `${key} certificate producer binding drifted`);
    requireText(certificate.execution_id, `${key}.certificate.execution_id`);
    if (certificate.checker !== CHECKER_PATH || certificate.runner !== RUNNER_PATH) fail("invalid_oracle", `${key} certificate checker/runner binding drifted`);
    const identityFields = requireArray(record.identity_fields, `${key}.identity_fields`, { nonempty: true });
    if (!deepEqual(sorted(identityFields), sorted(IDENTITY_FIELDS))) fail("invalid_oracle", `${key} certificate identity fields drifted`);
    const suppliedIdentity = requireObject(record.identity, `${key}.identity`);
    for (const field of IDENTITY_FIELDS) {
      if (!Object.hasOwn(suppliedIdentity, field) || requireText(suppliedIdentity[field], `${key}.identity.${field}`) !== requireText(expectedIdentity[field], `${key}.expected.${field}`)) {
        fail("mismatch", `${key} observation identity does not match the current source-bound identity`);
      }
    }
    const observations = requireObject(record.observations, `${key}.observations`);
    for (const field of OBSERVATION_FIELDS) {
      if (!Object.hasOwn(observations, field) || observations[field] === null || observations[field] === undefined) {
        fail("invalid_oracle", `${key} observations omit ${field}`);
      }
    }
    if (certificate.observation_sha256 !== digestValue(observations) || certificate.identity_sha256 !== digestValue(suppliedIdentity)) {
      fail("mismatch", `${key} certificate does not bind its observation and identity payload`);
    }
    const dimensions = requireArray(record.dimensions, `${key}.dimensions`, { nonempty: true });
    if (!deepEqual(sorted(dimensions), sorted(ADAPTER_DIMENSIONS))) fail("invalid_oracle", `${key} certificate dimensions drifted`);
    verified.push({
      key,
      operation_id: id,
      mode,
      identity: suppliedIdentity,
      identity_fields: [...IDENTITY_FIELDS],
      observations,
      certificate,
    });
  }
  return {
    path,
    schema: payload.schema,
    schema_version: payload.schema_version,
    status: verified.length > 0 ? "verified" : payload.status,
    records: verified,
    verified_keys: verified.map((record) => record.key).sort(),
    reason: verified.length > 0
      ? "identity-bound positive observations were supplied and checked without execution by this consumer"
      : "observation producer supplied no complete positive records; finite adapter evidence remains unavailable",
    universal_proof: false,
  };
}

function attachObservedRows(adapterRows, observation) {
  const certificates = new Map(observation.records.map((record) => [record.key, record]));
  const entries = adapterRows.entries.map((entry) => {
    if (entry.disposition !== "mapped_unproved") return entry;
    const capabilities = entry.implementation?.capabilities ?? (entry.implementation?.capability ? [entry.implementation.capability] : []);
    const required = capabilities.filter((capability) => capability.required && capability.applicable && capability.bound);
    if (required.length === 0 || required.some((capability) => !["implemented-unqualified", "passed"].includes(capability.disposition)) || required.some((capability) => !certificates.has(`${entry.id}@${capability.mode}`))) return entry;
    const observations = Object.fromEntries(required.map((capability) => {
      const record = certificates.get(`${entry.id}@${capability.mode}`);
      return [capability.mode, { identity: record.identity, observations: record.observations, certificate: record.certificate }];
    }));
    return {
      ...entry,
      observation_status: "matched",
      observation_reason: "all required adapter modes have identity-bound positive observations and supported certificates; this is bounded evidence, not semantic proof",
      implementation: { ...entry.implementation, observations },
    };
  });
  const counts = Object.fromEntries(MAPPING_STATUSES.map((status) => [status, 0]));
  for (const entry of entries) counts[entry.disposition] += 1;
  return { entries, counts };
}

function capabilityForMode(entry, mode) {
  const capabilities = entry.implementation?.capabilities ?? (entry.implementation?.capability ? [entry.implementation.capability] : []);
  return capabilities.find((capability) => capability.mode === mode) ?? null;
}

function formalRequiredKeys(adapterRows) {
  const keys = [];
  for (const entry of adapterRows.entries) {
    if (entry.disposition !== "mapped_unproved") continue;
    const capabilities = entry.implementation?.capabilities ?? (entry.implementation?.capability ? [entry.implementation.capability] : []);
    for (const capability of capabilities) {
      if (capability.required && capability.applicable && capability.bound && ["implemented-unqualified", "passed"].includes(capability.disposition)) {
        keys.push(`${entry.id}@${capability.mode}`);
      }
    }
  }
  return keys.sort();
}

function formalClaim(record, key) {
  const reference = requireObject(record.replay_ref, `${key}.replay_ref`);
  if (reference.path !== PROOF_MANIFEST_PATH) {
    fail("mismatch", `${key} replay reference is not the pinned compiler-proof manifest`, { expected: PROOF_MANIFEST_PATH, actual: reference.path });
  }
  const claimId = requireText(reference.claim_id, `${key}.replay_ref.claim_id`);
  let manifest;
  try {
    manifest = readJson(reference.path, `${key} replay manifest`);
    validateManifest(manifest);
  } catch (error) {
    const code = ["unavailable", "timeout"].includes(error?.code) ? error.code : "mismatch";
    fail(code, `${key} replay manifest is not available for semantic proof`, { claim_id: claimId, error: String(error) });
  }
  const claim = manifest.claims?.find((candidate) => candidate.id === claimId);
  if (!claim) fail("unavailable", `${key} replay claim is missing from the pinned compiler-proof manifest`, { claim_id: claimId });
  return { reference: { path: reference.path, claim_id: claimId }, manifest, claim };
}

function canonicalAdapterRoute(entry) {
  const implementation = entry.implementation ?? {};
  if (typeof implementation.route_symbol === "string" && implementation.route_symbol.length > 0) return implementation.route_symbol;
  if (typeof implementation.artifact_target === "string" && implementation.artifact_target.length > 0) return implementation.artifact_target;
  return entry.id;
}

function canonicalAdapterDeclaration(entry, mode) {
  const implementation = entry.implementation ?? {};
  if (typeof implementation.route_symbol === "string" && implementation.route_symbol.length > 0) return `${implementation.route_symbol}@${mode}`;
  if (typeof implementation.artifact_target === "string" && implementation.artifact_target.length > 0) return `${implementation.artifact_target}@${mode}`;
  const source = implementation.source_refs?.[0]?.path ?? entry.source;
  return `${source}:${entry.source_line ?? "unknown"}@${mode}`;
}
function canonicalModelDeclaration(modelSource, entry, mode, expectedModelId) {
  const lines = modelSource.split(/\r?\n/u)
    .filter((line) => /^\s*(?:--|\/\/)\s*JET_ADAPTER_MODEL_V1:/u.test(line))
    .map((line) => line.slice(line.indexOf(ADAPTER_MODEL_MARKER)));
  if (lines.length !== 1) {
    fail("unavailable", `${entry.id}@${mode} canonical adapter model must expose exactly one checked semantic marker`, { model_marker: ADAPTER_MODEL_MARKER, count: lines.length });
  }
  let marker;
  try {
    marker = JSON.parse(lines[0].slice(ADAPTER_MODEL_MARKER.length));
  } catch (error) {
    fail("invalid_oracle", `${entry.id}@${mode} canonical adapter model marker is invalid JSON`, { error: String(error) });
  }
  if (marker.schema !== "jet.adapter-model.v1" || marker.schema_version !== 1 || marker.id !== expectedModelId || marker.operation_id !== entry.id || marker.mode !== mode || marker.route !== canonicalAdapterRoute(entry) || marker.checked_declaration !== canonicalAdapterDeclaration(entry, mode) || marker.universal_proof !== false) {
    fail("mismatch", `${entry.id}@${mode} canonical adapter model marker is not bound to the checked route`, {
      expected: {
        id: expectedModelId,
        operation_id: entry.id,
        mode,
        route: canonicalAdapterRoute(entry),
        checked_declaration: canonicalAdapterDeclaration(entry, mode),
        universal_proof: false,
      },
      actual: marker,
    });
  }
  if (!deepEqual(sorted(requireArray(marker.dimensions, `${entry.id}@${mode}.canonical_model_marker.dimensions`, { nonempty: true })), sorted(ADAPTER_DIMENSIONS))
      || !deepEqual(sorted(requireArray(marker.observation_fields, `${entry.id}@${mode}.canonical_model_marker.observation_fields`, { nonempty: true })), sorted(OBSERVATION_FIELDS))) {
    fail("mismatch", `${entry.id}@${mode} canonical adapter model marker omits checked dimensions or observations`);
  }
  requireText(marker.proposition, `${entry.id}@${mode}.canonical_model_marker.proposition`);
  requireText(marker.formal_goal, `${entry.id}@${mode}.canonical_model_marker.formal_goal`);
  return marker;
}


function canonicalAdapterGoal(entry, mode, claim, identity) {
  const expectedModelId = `adapter-model:${entry.id}@${mode}`;
  const model = requireObject(claim.model, `${entry.id}@${mode}.claim.model`);
  if (model.id !== expectedModelId) {
    fail("unavailable", `${entry.id}@${mode} has no approved canonical adapter semantic model`, { expected_model_id: expectedModelId, actual_model_id: model.id });
  }
  const modelPath = assertPath(model.path, `${entry.id}@${mode}.claim.model.path`, { file: true });
  if (!modelPath.startsWith(`${ADAPTER_MODEL_ROOT}/`) || !modelPath.endsWith(".lean")) {
    fail("unavailable", `${entry.id}@${mode} model is outside the approved adapter semantic-model root`, { model_path: modelPath, model_root: ADAPTER_MODEL_ROOT });
  }
  const modelSha256 = digestPath(modelPath);
  if (model.sha256 !== modelSha256) {
    fail("mismatch", `${entry.id}@${mode} semantic model digest is stale`, { expected: modelSha256, actual: model.sha256 });
  }
  const modelSource = readFileSync(resolve(ROOT, modelPath), "utf8");
  const modelDeclaration = canonicalModelDeclaration(modelSource, entry, mode, expectedModelId);
  const semanticModel = {
    schema: "jet.adapter-model.v1",
    schema_version: 1,
    id: expectedModelId,
    operation_id: entry.id,
    subject: entry.subject,
    mode,
    route: canonicalAdapterRoute(entry),
    checked_declaration: canonicalAdapterDeclaration(entry, mode),
    proposition: modelDeclaration.proposition,
    formal_goal: modelDeclaration.formal_goal,
    source: { path: entry.source, sha256: digestPath(entry.source) },
    target: {
      id: mode,
      artifact_target: entry.implementation?.artifact_target ?? null,
      target_label: entry.implementation?.target_label ?? null,
    },
    capability: capabilityForMode(entry, mode),
    dimensions: [...ADAPTER_DIMENSIONS],
    observation_fields: [...OBSERVATION_FIELDS],
    identity_fields: [...IDENTITY_FIELDS],
    identity,
    model: { id: model.id, path: modelPath, sha256: modelSha256 },
    universal_proof: false,
  };
  const goal = {
    schema: "jet.adapter-goal.v1",
    schema_version: 1,
    id: expectedModelId,
    operation_id: entry.id,
    mode,
    status: "available",
    model: semanticModel,
    correspondence: {
      route: canonicalAdapterRoute(entry),
      checked_declaration: canonicalAdapterDeclaration(entry, mode),
    },
    dimensions: [...ADAPTER_DIMENSIONS],
    universal_proof: false,
  };
  const identityOnly = /\b(?:identity|hash|digest|sha256|program_sha256|source_sha256|target_sha256|checker_sha256|obligations_sha256)\b/iu;
  const semanticTerms = /\b(?:preserv\w*|correspond\w*|result|failure|mutation|cleanup|schedule|dispatch|control|layout|representation|target)\b/iu;
  for (const [label, actual, expected] of [["proposition", claim.proposition, modelDeclaration.proposition], ["formal_goal", claim.proof?.formal_goal, modelDeclaration.formal_goal]]) {
    requireText(actual, `${entry.id}@${mode}.claim.${label}`);
    if (actual !== expected || identityOnly.test(actual) || !actual.includes(entry.id) || !actual.includes(mode) || !semanticTerms.test(actual)) {
      fail("mismatch", `${entry.id}@${mode} compiler-proof claim is identity-only, detached, or inconsistent with the canonical adapter model`, { field: label, expected, actual });
    }
  }
  return {
    ...goal,
    proposition: claim.proposition,
    formal_goal: claim.proof.formal_goal,
    source: { path: claim.implementation.path, sha256: claim.implementation.sha256 },
  };
}

function assertFormalClaimBinding(entry, mode, claim, identity, goal) {
  const allowedPaths = new Set((identity.source_records ?? []).map((record) => record.path));
  if (!allowedPaths.has(claim.source.path)) {
    fail("mismatch", `${entry.id}@${mode} compiler-proof source is outside the checked adapter model`, { path: claim.source.path });
  }
  if (digestPath(claim.source.path) !== claim.source.sha256) {
    fail("mismatch", `${entry.id}@${mode} compiler-proof source identity is stale`, { path: claim.source.path });
  }
  if (!allowedPaths.has(claim.implementation.path)) {
    fail("mismatch", `${entry.id}@${mode} compiler-proof implementation is outside the checked adapter route`, { path: claim.implementation.path });
  }
  if (digestPath(claim.implementation.path) !== claim.implementation.sha256) {
    fail("mismatch", `${entry.id}@${mode} compiler-proof implementation identity is stale`, { path: claim.implementation.path });
  }
  const correspondence = requireObject(claim.correspondence, `${entry.id}@${mode}.claim.correspondence`);
  const expectedRoute = canonicalAdapterRoute(entry);
  const expectedDeclaration = canonicalAdapterDeclaration(entry, mode);
  if (correspondence.route !== expectedRoute || correspondence.checked_declaration !== expectedDeclaration) {
    fail("mismatch", `${entry.id}@${mode} compiler-proof correspondence is not bound to the canonical adapter route`, {
      expected: { route: expectedRoute, checked_declaration: expectedDeclaration },
      actual: { route: correspondence.route, checked_declaration: correspondence.checked_declaration },
    });
  }
  const method = correspondence.method;
  if (!["direct_verification", "translation_certificate"].includes(method)) {
    fail("mismatch", `${entry.id}@${mode} compiler-proof correspondence method is not approved`, { method });
  }
  return {
    operation_id: entry.id,
    mode,
    method,
    route: expectedRoute,
    checked_declaration: expectedDeclaration,
    status: "verified",
    semantic_goal_id: goal.id,
    observation_fields: [...OBSERVATION_FIELDS],
    identity_sha256: digestValue(identity),
  };
}

function checkFormalDischarge(adapterRows, authorityKey) {
  if (authorityKey !== AUTHORITY_KEY) fail("invalid_oracle", "adapter formal replay authority is not the contract-selected authority", { expected: AUTHORITY_KEY, actual: authorityKey });
  const path = FORMAL_DISCHARGE_PATH;
  const requiredKeys = formalRequiredKeys(adapterRows);
  if (!existsSync(resolve(ROOT, path))) {
    return {
      path,
      schema: FORMAL_DISCHARGE_SCHEMA,
      status: "unavailable",
      records: [],
      verified_keys: [],
      required_keys: requiredKeys,
      missing_keys: requiredKeys,
      reason: "no formal discharge carrier exists; canonical adapter semantic goals and pinned replays remain unavailable",
      universal_proof: false,
    };
  }
  const payload = readJson(path, "adapter formal discharge");
  if (payload.schema !== FORMAL_DISCHARGE_SCHEMA || payload.schema_version !== 1 || payload.stage !== STAGE || payload.universal_proof !== false) {
    fail("invalid_oracle", "adapter formal discharge schema/stage/universal policy is invalid");
  }
  const records = requireArray(payload.records, "adapter formal discharge records");
  const entriesById = new Map(adapterRows.entries.map((entry) => [entry.id, entry]));
  const seen = new Set();
  const verified = [];
  for (const record of records) {
    requireObject(record, "adapter formal discharge record");
    const id = requireText(record.operation_id, "formal discharge operation_id");
    const mode = requireText(record.mode, `${id}.mode`);
    if (!ROUTE_MODES.includes(mode)) fail("invalid_oracle", `${id} formal discharge names unknown mode ${mode}`);
    const key = `${id}@${mode}`;
    if (seen.has(key)) fail("invalid_oracle", `duplicate formal discharge ${key}`);
    seen.add(key);
    if (!requiredKeys.includes(key)) fail("mismatch", `${key} formal discharge is not a required applicable adapter row`);
    const entry = entriesById.get(id);
    if (!entry) fail("mismatch", `formal discharge names unknown obligation ${id}`);
    const capability = capabilityForMode(entry, mode);
    if (entry.disposition !== "mapped_unproved" || !capability?.required || !capability.applicable || !capability.bound || !["implemented-unqualified", "passed"].includes(capability.disposition)) {
      fail("blocked", `${key} formal discharge cannot change source applicability or disposition`, { disposition: entry.disposition, capability });
    }
    if (!["direct_verification", "translation_certificate"].includes(record.method)) {
      fail("invalid_oracle", `${key} formal discharge method is not approved`);
    }
    if (record.universal_proof !== false) fail("invalid_oracle", `${key} formal discharge cannot claim universal proof`);
    for (const field of ["status", "approval", "canonical_goal", "correspondence", "replay"]) {
      if (Object.hasOwn(record, field)) fail("invalid_oracle", `${key} contains self-authored ${field}; checked replay output is derived by this checker`);
    }
    const identityFields = requireArray(record.identity_fields, `${key}.identity_fields`, { nonempty: true });
    if (!deepEqual(sorted(identityFields), sorted(IDENTITY_FIELDS))) fail("invalid_oracle", `${key} formal identity fields drifted`);
    const identity = requireObject(record.identity, `${key}.identity`);
    const expectedIdentity = observationIdentity(entry, mode);
    for (const field of IDENTITY_FIELDS) {
      if (requireText(identity[field], `${key}.identity.${field}`) !== requireText(expectedIdentity[field], `${key}.expected.${field}`)) {
        fail("mismatch", `${key} formal identity does not match the current source-bound identity`);
      }
    }
    const { reference, claim } = formalClaim(record, key);
    const canonicalGoal = canonicalAdapterGoal(entry, mode, claim, expectedIdentity);
    const correspondence = assertFormalClaimBinding(entry, mode, claim, expectedIdentity, canonicalGoal);
    if (record.method !== correspondence.method) fail("mismatch", `${key} formal method does not match the replay claim correspondence method`);
    canonicalGoal.correspondence = { ...canonicalGoal.correspondence, method: correspondence.method };
    const replayAuthority = {
      authority_key: authorityKey,
      obligation_id: `${entry.id}@${mode}`,
      proposition: canonicalGoal.proposition,
      formal_goal: canonicalGoal.formal_goal,
      model: canonicalGoal.model.model,
      implementation: canonicalGoal.source,
      correspondence: canonicalGoal.correspondence,
      candidate: claim.candidate,
    };
    let replayResult;
    try {
      replayResult = replay(reference.path, reference.claim_id, replayAuthority);
    } catch (error) {
      const code = ["unavailable", "timeout", "cancelled"].includes(error?.code) ? error.code : "mismatch";
      fail(code, `${key} pinned compiler-proof replay did not establish the adapter semantic claim`, {
        claim_id: reference.claim_id,
        error: String(error),
      });
    }
    if (replayResult?.status !== "proved"
        || replayResult.claim_id !== reference.claim_id
        || replayResult.replay?.exit_code !== 0
        || replayResult.replay?.kernel_replay?.exit_code !== 0) {
      fail("mismatch", `${key} pinned compiler-proof replay is not a successful checked proof`, {
        claim_id: reference.claim_id,
        status: replayResult?.status,
        replay_status: replayResult?.replay?.exit_code,
        kernel_replay_status: replayResult?.replay?.kernel_replay?.exit_code,
      });
    }
    if (replayResult.identity_sha256 !== claim.identity_sha256) {
      fail("mismatch", `${key} replay identity does not match the checked manifest claim`);
    }
    verified.push({
      key,
      operation_id: id,
      mode,
      method: correspondence.method,
      replay_ref: reference,
      replay: replayResult,
      canonical_goal: canonicalGoal,
      correspondence,
      identity,
      identity_fields: [...IDENTITY_FIELDS],
      universal_proof: false,
    });
  }
  const missingKeys = requiredKeys.filter((key) => !seen.has(key));
  const status = missingKeys.length === 0 && requiredKeys.length > 0
    ? "proved"
    : verified.length > 0
      ? "blocked"
      : "unavailable";
  return {
    path,
    schema: payload.schema,
    schema_version: payload.schema_version,
    status,
    records: verified,
    verified_keys: verified.map((record) => record.key).sort(),
    required_keys: requiredKeys,
    missing_keys: missingKeys,
    reason: status === "proved"
      ? "every required applicable adapter row has a canonical semantic model and an actual pinned compiler-proof replay"
      : verified.length > 0
        ? "pinned semantic replays are incomplete; missing adapter rows remain blocked"
        : "no required adapter row has an available canonical semantic model and pinned replay",
    universal_proof: false,
  };
}

function applyFormalDischarge(adapterRows, formal) {
  const records = new Map(formal.records.map((record) => [record.key, record]));
  const entries = adapterRows.entries.map((entry) => {
    if (entry.disposition !== "mapped_unproved") return entry;
    const capabilities = entry.implementation?.capabilities ?? (entry.implementation?.capability ? [entry.implementation.capability] : []);
    const required = capabilities.filter((capability) => capability.required && capability.applicable && capability.bound);
    if (required.length === 0 || required.some((capability) => !["implemented-unqualified", "passed"].includes(capability.disposition)) || required.some((capability) => !records.has(`${entry.id}@${capability.mode}`))) return entry;
    const formalRecords = Object.fromEntries(required.map((capability) => {
      const record = records.get(`${entry.id}@${capability.mode}`);
      return [capability.mode, record];
    }));
    return {
      ...entry,
      disposition: "proved",
      disposition_reason: "all required adapter modes have canonical semantic models and actual pinned compiler-proof replays",
      implementation: { ...entry.implementation, formal_discharge: formalRecords },
    };
  });
  const counts = Object.fromEntries(MAPPING_STATUSES.map((status) => [status, 0]));
  for (const entry of entries) counts[entry.disposition] += 1;
  return { entries, counts };
}

function requiredCapabilityRecords(entry) {
  const capabilities = entry.implementation?.capabilities ?? (entry.implementation?.capability ? [entry.implementation.capability] : []);
  return capabilities.filter((capability) => capability.required);
}

function coverageComplete(entries, field) {
  return entries.length > 0 && entries.every((entry) => {
    const required = requiredCapabilityRecords(entry);
    if (required.length === 0) return true;
    return required.every((capability) => capability.bound && capability.applicable) && (field === "formal" ? entry.disposition === "proved" : entry.observation_status === "matched");
  });
}

function checkDependencies(contract) {
  return contract.dependencies.map((dependency) => ({ path: dependency.path, kind: dependency.kind, sha256: digestPath(dependency.path) }));
}

function stageResult() {
  const contract = readJson(CONTRACT_PATH, "adapter contract");
  const witnesses = readJson(WITNESSES_PATH, "adapter witnesses");
  const contractInfo = checkContract(contract, witnesses);
  const witnessInfo = checkWitnesses(witnesses, contract);
  const obligations = readJson(OBLIGATIONS_PATH, "compiler obligations");
  const obligationInfo = checkObligationRelation(obligations);
  const capability = checkCapabilityRelation();
  const prelude = checkRuntimeDependency("prelude", "proof/compiler/prelude/contract.json", "proof/compiler/prelude/witnesses.json", "jet.prelude-contract.v1");
  const runtime = checkRuntimeDependency("runtime", "proof/compiler/runtime/contract.json", "proof/compiler/runtime/witnesses.json", "jet.runtime-contract.v1");
  const identity = checkProofIdentity(contract);
  const targetBindingRecords = checkTargetBindings(witnessInfo.targetBindings);
  const dispatcherRows = parseDispatcher(readText(CORE_PATH, "Core dispatcher source")).filter((row) => row.owner === "#2941");
  if (dispatcherRows.length === 0) fail("unavailable", "Core dispatcher has no #2941 adapter rows");
  const operationIds = new Set(dispatcherRows.map((row) => row.id));
  const expectedOperationIds = new Set(obligationInfo.rows.filter((row) => row.family === "core" && row.kind === "adapter").map((row) => row.id));
  if (!deepEqual(sorted(operationIds), sorted(expectedOperationIds))) fail("mismatch", "Core adapter dispatcher denominator disagrees with #2941 obligations", { dispatcher: sorted(operationIds), obligations: sorted(expectedOperationIds) });
  const fixtureSummary = {
    same_observation: checkSameObservationFixture(witnessInfo.fixtures[0].path),
    negative_controls: checkNegativeFixture(witnessInfo.fixtures[1].path, obligations),
  };
  const sourceAdapterRows = buildAdapterRows(obligationInfo, dispatcherRows, capability, targetBindingRecords);
  const observationCertificates = checkObservationCertificates(sourceAdapterRows);
  const observedAdapterRows = attachObservedRows(sourceAdapterRows, observationCertificates);
  const formalDischarge = checkFormalDischarge(observedAdapterRows, contractInfo.identity.authority_key);
  const adapterRows = applyFormalDischarge(observedAdapterRows, formalDischarge);
  const observationComplete = coverageComplete(adapterRows.entries, "observation");
  const formalComplete = coverageComplete(adapterRows.entries, "formal");
  const dependencyRecords = checkDependencies(contract);
  const boundary = {
    proven: contract.boundary.proven,
    assumed: contract.boundary.assumed,
    tested: contract.boundary.tested,
    blocked: contract.boundary.blocked,
    qualification: formalComplete ? "qualified" : "blocked",
    universal_proof: false,
    reason: formalComplete
      ? "all required adapter modes have canonical semantic models and actual pinned compiler-proof replays; this remains finite and non-universal"
      : observationComplete
        ? "bounded identity-bound adapter observations matched; formal semantic qualification still requires complete canonical semantic models and pinned proof replays"
        : "source correspondence is identity-bound but complete positive adapter observations and formal semantic replays are not available",
  };
  const routeEntries = adapterRows.entries.filter((entry) => entry.implementation?.route_symbol || entry.implementation?.artifact_target || entry.id.startsWith("boundary:"));
  const selectedCapabilityIds = new Set();
  for (const entry of adapterRows.entries) {
    for (const capabilityRecordValue of [entry.implementation?.capability, ...(entry.implementation?.capabilities ?? [])]) {
      if (capabilityRecordValue?.row_id) selectedCapabilityIds.add(capabilityRecordValue.row_id);
      for (const rowId of capabilityRecordValue?.rows ?? []) selectedCapabilityIds.add(rowId);
    }
  }
  return {
    schema: CHECK_SCHEMA,
    schema_version: 1,
    status: formalComplete ? "valid" : observationComplete ? "matched" : "blocked",
    contract: {
      id: contract.contract_id,
      schema: contract.schema,
      schema_version: contract.schema_version,
      claim: contract.claim.kind,
      certificates: contract.certificates,
      witnesses_sha256: contract.witnesses_sha256,
    },
    identity,
    binding: {
      checker: "proof/compiler/adapters/check.mjs",
      driver: "scripts/agent/compiler-proof.mjs",
      dependency_count: dependencyRecords.length,
      dependencies: dependencyRecords,
      obligation_dimensions: contract.obligation_dimensions,
      adapter_dimensions: ADAPTER_DIMENSIONS,
      memory_premises: contract.memory_premises.map((premise) => ({ ...premise, sha256: digestPath(premise.source) })),
      inventory_join: contract.inventory_join,
      source_roots: SOURCE_ROOTS,
      identity: witnessInfo.identity,
      identity_fields: witnessInfo.identityFields,
      observation_certificates: witnessInfo.observationCertificates,
      formal_discharge: witnessInfo.formalDischarge,
    },
    obligations: {
      relation_path: OBLIGATIONS_PATH,
      relation_sha256: obligationInfo.relation_sha256,
      row_ids: obligationInfo.row_ids,
      rows: adapterRows.entries,
      disposition_counts: adapterRows.counts,
      qualification: formalComplete ? "qualified" : "blocked",
      universal_proof: false,
      complete: adapterRows.entries.length === obligationInfo.rows.filter((row) => row.owner === "#2941").length && adapterRows.entries.every((entry) => entry.disposition !== "unmapped"),
      observed_complete: observationComplete,
      formal_complete: formalComplete,
    },
    adapters: {
      operation_count: dispatcherRows.length,
      operations: dispatcherRows.map((row) => ({ id: row.id, subject: row.subject, source: row.source, source_line: row.source_line, route_symbol: row.dispatcher.route_symbol, options: row.dispatcher.options })),
      targets: targetBindingRecords,
      routes: routeEntries,
      same_operation_body: "all targets consume canonical MIR/Core operation identities; adapter differences are target-specific marshalling/control-transfer only",
      semantic_reencoding: false,
      status: formalComplete ? "verified" : observationComplete ? "observed" : "source-bound-unproved",
      universal_proof: false,
    },
    capability_relation: {
      manifest_path: CAPABILITY_MANIFEST_PATH,
      schema: capability.schema,
      source_snapshot_hash: capability.source_snapshot_hash,
      manifest_sha256: capability.manifest_sha256,
      relation_sha256: capability.relation_sha256,
      denominator: capability.denominator,
      row_count: capability.rows.length,
      selected_rows: capability.rows.filter((row) => selectedCapabilityIds.has(row.row_id)).map((row) => ({ row_id: row.row_id, capability_id: row.capability_id, mode: row.mode, disposition: row.disposition, identity: row.identity })),
      unknown_policy: "missing/unavailable/stale relation rows remain in the denominator and block qualification",
    },
    dependencies: { prelude, runtime },
    fixtures: fixtureSummary,
    formal_discharge: formalDischarge,
    observation_certificates: observationCertificates,
    assumptions: {
      count: contract.foreign_assumptions.length,
      items: contract.foreign_assumptions.map((assumption) => ({ ...assumption, status: "assumed", universal_proof: false })),
    },
    compiler_execution: {
      status: "unverified",
      requested: false,
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      timeout_ms: contract.execution.timeout_ms,
      invocation: "source checker only; optional formal discharge invokes the pinned Lean replay through compiler-proof, never the Jet compiler/backend/browser/VM",
    },
    jet_execution: {
      status: "unverified",
      requested: false,
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      tiers: ["aot", "jet_run", "interpreter", "web", "comptime"],
      invocation: "retained adapter fixtures are not executed by this stage",
    },
    proof_replay: {
      status: formalDischarge.status,
      requested: formalDischarge.records.length > 0,
      qualification: formalComplete ? "qualified" : "blocked",
      reason: formalDischarge.reason,
      verified_keys: formalDischarge.verified_keys,
      missing_keys: formalDischarge.missing_keys,
      records: formalDischarge.records.map((record) => ({
        key: record.key,
        operation_id: record.operation_id,
        mode: record.mode,
        method: record.method,
        replay_ref: record.replay_ref,
        claim_id: record.replay?.claim_id,
        identity_sha256: record.replay?.identity_sha256,
        replay: record.replay?.replay,
      })),
    },
    proof_boundary: boundary,
  };
}

function printHuman(result) {
  process.stdout.write(`${result.stage ?? STAGE}: ${result.status}\n`);
  if (result.obligations) process.stdout.write(`obligations: ${result.obligations.denominator}; qualification=${result.obligations.qualification}\n`);
  if (result.adapters) process.stdout.write(`adapters: operations=${result.adapters.operation_count}; status=${result.adapters.status}\n`);
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}

export async function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const allowed = new Set(["--json"]);
  if (argv.some((arg) => !allowed.has(arg))) {
    const result = errorPayload(new CheckError("invalid_oracle", "usage: adapters/check.mjs [--json]"));
    if (json) process.stdout.write(`${canonicalJson(result)}\n`); else printHuman(result);
    return 64;
  }
  try {
    const result = stageResult();
    if (json) process.stdout.write(`${canonicalJson(result)}\n`); else printHuman(result);
    return 0;
  } catch (error) {
    const result = errorPayload(error);
    if (json) process.stdout.write(`${canonicalJson(result)}\n`); else printHuman(result);
    if (result.status === "unavailable") return 3;
    return 1;
  }
}

function errorPayload(error) {
  const allowed = new Set(["blocked", "mismatch", "unavailable", "invalid_oracle", "timeout", "cancelled", "failed"]);
  const status = allowed.has(error?.code) ? error.code : "invalid_oracle";
  return {
    schema: CHECK_SCHEMA,
    schema_version: 1,
    status,
    stage: STAGE,
    error: {
      code: status,
      message: error?.message ?? String(error),
      ...(error?.details === undefined ? {} : { details: error.details }),
    },
    proof_boundary: { qualification: "blocked", universal_proof: false },
  };
}

if (process.argv[1] && import.meta.url === new URL(process.argv[1], "file:").href) process.exitCode = await main();
