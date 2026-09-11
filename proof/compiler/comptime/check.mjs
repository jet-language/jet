#!/usr/bin/env node

/**
 * Structural contract checker for compiler compile-time evaluation (#2938).
 *
 * This stage binds the comptime model, the canonical MIR evaluator seam, the
 * sema/build cache inputs, and the retained source witnesses. It deliberately
 * does not execute authored witnesses: without a requested, identity-bound
 * observation every result remains `unverified` and the stage remains blocked.
 * A source/dependency/configuration identity change is a mismatch, never a
 * cache hit or proof.
 */

import {
  existsSync,
  lstatSync,
  readFileSync,
} from "node:fs";
import {
  dirname,
  isAbsolute,
  relative,
  resolve,
  sep,
} from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  canonical,
  canonicalJson,
  digestBytes,
  digestText,
  identityDigest,
  validateManifest,
} from "../../../scripts/agent/compiler-proof.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const STAGE = "comptime";
const CHECK_SCHEMA = "jet.comptime-contract-check.v1";
const CONTRACT_PATH = "proof/compiler/comptime/contract.json";
const WITNESSES_PATH = "proof/compiler/comptime/witnesses.json";
const OBSERVATIONS_PATH = "proof/compiler/comptime/observations.json";
const OBSERVATION_SCHEMA = "jet.comptime-observations.v1";
const OBSERVATION_CERTIFICATE_SCHEMA = "jet.comptime-observation-certificate.v1";
const EVALUATOR_EVENT_SCHEMA = "jet.comptime-evaluator-event.v1";
const CACHE_EVENT_SCHEMA = "jet.comptime-cache-event.v1";
const OBSERVER_PATH = "proof/compiler/comptime/observe.mjs";
const OBLIGATIONS_PATH = "proof/compiler/obligations.json";
const PROOF_MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const CHECKER_PATH = "proof/compiler/comptime/check.mjs";
const DRIVER_PATH = "scripts/agent/compiler-proof.mjs";
const RUNNER_PATH = "scripts/agent/jet-env";
const COMPILER_PATH = "target/debug/jet";
const FIXTURE_PATH = "proof/compiler/comptime/fixtures/mixed-reader.jet";
const TIMEOUT_MS = 120000;

const OBSERVATION_DIMENSIONS = Object.freeze([
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
const MODES = Object.freeze(["aot", "jet_run", "interpreter", "check"]);
const MAPPING_STATUSES = Object.freeze(["mapped_unproved", "unsupported", "unmapped", "proved"]);
const FAILURE_STATUSES = new Set(["blocked", "cancelled", "invalidated", "mismatch", "unsupported", "unverified"]);
const CHECK_STATUSES = new Set([
  "blocked",
  "matched",
  "mismatch",
  "unsupported",
  "unavailable",
  "timeout",
  "cancelled",
  "invalid_oracle",
  "failed",
]);
const REQUIRED_CACHE_INPUTS = Object.freeze([
  "source_bytes_sha256",
  "imported_module_closure_sha256",
  "checked_body_semantic_hash",
  "compiler_abi_schema",
  "target_dossier",
  "profile",
  "effects",
  "layout",
  "dependency_interfaces",
  "build_facts",
  "evaluator_artifact_identity",
  "configuration",
  "policy",
  "toolchain",
]);
const REQUIRED_CACHE_EVENTS = Object.freeze([
  "source_dependency_changed",
  "imported_fact_changed",
  "compiler_abi_changed",
  "target_profile_effect_changed",
  "evaluator_artifact_changed",
  "configuration_changed",
  "interrupted_before_publish",
  "failed_without_outputs",
]);
const CACHE_EVENT_FRAGMENTS = Object.freeze({
  source_dependency_changed: ["source_bytes_sha256", "imported_module_closure_sha256"],
  imported_fact_changed: ["build_facts", "dependency_interfaces"],
  compiler_abi_changed: ["compiler_abi_schema"],
  target_profile_effect_changed: ["target_dossier", "profile", "effects", "layout"],
  evaluator_artifact_changed: ["evaluator_artifact_identity"],
  configuration_changed: ["configuration", "policy", "toolchain"],
  interrupted_before_publish: ["cancellation", "interrupted"],
  failed_without_outputs: ["failed", "failure", "outputs"],
});
const REQUIRED_BLOCKERS = Object.freeze([
  "unsupported_construct",
  "unknown_registry_route",
  "fuel_exhaustion",
  "purity_violation",
  "resource_failure",
  "interrupted_evaluation",
  "cache_dependency_drift",
  "cache_config_drift",
  "cache_evaluator_drift",
  "cache_imported_fact_drift",
  "known_mismatch_exact_comptime",
]);
const REQUIRED_IDENTITY_FIELDS = Object.freeze([
  "source_sha256",
  "program_sha256",
  "dependency_closure_sha256",
  "configuration_sha256",
  "evaluator_sha256",
  "target",
  "obligations_sha256",
]);
const OBSERVATION_IDENTITY_FIELDS = Object.freeze([
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
const METHOD_SET = new Set(["verified_construction", "direct_verification", "translation_certificate"]);
const OPERATION_EVIDENCE = Object.freeze({
  observation_dimensions: OBSERVATION_DIMENSIONS,
  failure_obligations: Object.freeze(["E0952", "E0953", "E0955", "E0956", "E0957", "E3401"]),
  resource_premises: Object.freeze(["finite fuel", "checked Core route", "foreign resource gate", "completed publication"]),
});

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

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail("invalid_oracle", `${label} must be an object`);
  }
  return value;
}

function array(value, label) {
  if (!Array.isArray(value)) fail("invalid_oracle", `${label} must be an array`);
  return value;
}

function text(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    fail("invalid_oracle", `${label} must be non-empty text`);
  }
  return value;
}

function bool(value, label) {
  if (typeof value !== "boolean") fail("invalid_oracle", `${label} must be boolean`);
  return value;
}

function integer(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    fail("invalid_oracle", `${label} must be a non-negative safe integer`);
  }
  return value;
}

function deepEqual(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function digest(value) {
  text(value, "digest");
  if (!/^sha256-[0-9a-f]{64}$/u.test(value)) {
    fail("invalid_oracle", `invalid SHA-256 digest ${value}`);
  }
  return value;
}

function projectPath(value, label) {
  text(value, label);
  if (isAbsolute(value) || value.includes("\\") || value.includes("\0")) {
    fail("invalid_oracle", `${label} must be a project-relative path`);
  }
  const target = resolve(ROOT, value);
  const prefix = `${ROOT}${sep}`;
  if (target !== ROOT && !target.startsWith(prefix)) {
    fail("invalid_oracle", `${label} escapes the project root`);
  }
  return target;
}

function regularFile(value, label) {
  const target = projectPath(value, label);
  let stat;
  try {
    stat = lstatSync(target);
  } catch (error) {
    fail("unavailable", `${label} is missing: ${value}`, { path: value, error: error.message });
  }
  if (stat.isSymbolicLink() || !stat.isFile()) {
    fail("invalid_oracle", `${label} is not a regular file: ${value}`);
  }
  return target;
}

function fileDigest(value, label = "file") {
  const target = regularFile(value, label);
  return digestBytes(readFileSync(target));
}

function readText(value, label) {
  return readFileSync(regularFile(value, label), "utf8");
}

function readJson(value, label) {
  const target = regularFile(value, label);
  try {
    return JSON.parse(readFileSync(target, "utf8"));
  } catch (error) {
    fail("invalid_oracle", `invalid JSON in ${label}: ${error.message}`, { path: value });
  }
}

function lineFor(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function withoutWitnessDigest(contract) {
  const projected = JSON.parse(JSON.stringify(contract));
  delete projected.witnesses_sha256;
  return projected;
}

function contractBindingDigest(contract) {
  return digestText(canonicalJson(withoutWitnessDigest(contract)));
}

function configurationIdentity(contract) {
  return digestText(canonicalJson({
    stage: STAGE,
    execution: contract.execution,
    cache: contract.cache,
    policy: contract.policy,
  }));
}

function evaluatorIdentity(contract) {
  return digestText(canonicalJson(contract.evaluator));
}

function validateStringList(values, label) {
  const entries = array(values, label);
  if (entries.length === 0 || entries.some((entry) => typeof entry !== "string" || entry.length === 0)) {
    fail("invalid_oracle", `${label} must contain non-empty strings`);
  }
}

function checkContract(contract) {
  object(contract, "comptime contract");
  if (contract.schema !== "jet.comptime-contract.v1" || contract.schema_version !== 1 || contract.stage !== STAGE) {
    fail("invalid_oracle", "comptime contract schema/version/stage is unsupported");
  }
  text(contract.contract_id, "contract.contract_id");
  if (contract.owner !== "#2938") fail("invalid_oracle", "comptime contract owner must be #2938");

  const policy = object(contract.policy, "contract.policy");
  if (policy.decision_id !== "D-COMPILER-PROOF1" || policy.outcome !== "A" || policy.status !== "ratified") {
    fail("invalid_oracle", "comptime contract does not record ratified D-COMPILER-PROOF1 outcome A");
  }
  text(policy.scope, "contract.policy.scope");
  text(policy.foreign_suffix, "contract.policy.foreign_suffix");

  const claim = object(contract.claim, "contract.claim");
  text(claim.kind, "contract.claim.kind");
  text(claim.statement, "contract.claim.statement");
  for (const field of ["preconditions", "observation_alphabet", "permitted_resource_failures", "trusted_components", "unsupported_cases"]) {
    validateStringList(claim[field], `contract.claim.${field}`);
  }
  if (!deepEqual(claim.observation_alphabet, OBSERVATION_DIMENSIONS)) {
    fail("invalid_oracle", "comptime observation alphabet is incomplete or reordered");
  }
  if (claim.statement.toLowerCase().includes("universal")) {
    fail("invalid_oracle", "comptime claim cannot state a universal proof");
  }

  const evaluator = object(contract.evaluator, "contract.evaluator");
  const model = object(evaluator.model, "contract.evaluator.model");
  text(model.id, "contract.evaluator.model.id");
  validateStringList(model.state, "contract.evaluator.model.state");
  validateStringList(model.transitions, "contract.evaluator.model.transitions");
  validateStringList(model.failure_codes, "contract.evaluator.model.failure_codes");
  const implementation = object(evaluator.implementation, "contract.evaluator.implementation");
  text(implementation.id, "contract.evaluator.implementation.id");
  text(implementation.crate, "contract.evaluator.implementation.crate");
  validateStringList(implementation.entrypoints, "contract.evaluator.implementation.entrypoints");
  text(implementation.canonical_seam, "contract.evaluator.implementation.canonical_seam");
  bool(implementation.no_second_engine, "contract.evaluator.implementation.no_second_engine");
  if (!implementation.no_second_engine) fail("invalid_oracle", "comptime contract permits a second semantic engine");
  const fuel = object(implementation.fuel_budgets, "contract.evaluator.implementation.fuel_budgets");
  for (const mode of ["binding", "dev", "repl"]) integer(fuel[mode], `fuel_budgets.${mode}`);
  validateStringList(evaluator.resource_premises, "contract.evaluator.resource_premises");

  const cache = object(contract.cache, "contract.cache");
  text(cache.key_schema, "contract.cache.key_schema");
  const cacheInputs = array(cache.required_inputs, "contract.cache.required_inputs");
  for (const input of REQUIRED_CACHE_INPUTS) {
    if (!cacheInputs.includes(input)) fail("invalid_oracle", `cache key omits ${input}`);
  }
  const cacheEvents = array(cache.invalidation_events, "contract.cache.invalidation_events");
  for (const event of REQUIRED_CACHE_EVENTS) {
    if (!cacheEvents.includes(event)) fail("invalid_oracle", `cache invalidation omits ${event}`);
  }
  validateStringList(cache.publication_rules, "contract.cache.publication_rules");
  text(cache.lookup_gate, "contract.cache.lookup_gate");
  text(cache.action_key, "contract.cache.action_key");

  const execution = object(contract.execution, "contract.execution");
  if (execution.schema !== "jet.comptime-witness-mapping.v1") fail("invalid_oracle", "comptime execution schema drifted");
  if (execution.runner !== RUNNER_PATH || execution.compiler !== COMPILER_PATH) {
    fail("invalid_oracle", "comptime execution runner/compiler drifted");
  }
  const operations = array(execution.operations, "contract.execution.operations");
  if (!deepEqual(operations, ["run", "check"])) fail("invalid_oracle", "comptime operations must be run/check");
  if (!deepEqual(array(execution.modes, "contract.execution.modes"), MODES)) {
    fail("invalid_oracle", "comptime modes must name AOT, jet run, interpreter, and check");
  }
  if (!deepEqual(array(execution.observation_fields, "contract.execution.observation_fields"), OBSERVATION_DIMENSIONS)) {
    fail("invalid_oracle", "comptime execution observation fields drifted");
  }
  const identityFields = array(execution.identity_fields, "contract.execution.identity_fields");
  for (const field of REQUIRED_IDENTITY_FIELDS) {
    if (!identityFields.includes(field)) fail("invalid_oracle", `comptime identity omits ${field}`);
  }
  integer(execution.timeout_ms, "contract.execution.timeout_ms");
  if (execution.timeout_ms < 1 || execution.timeout_ms > 300000) fail("invalid_oracle", "comptime timeout is outside fail-closed range");
  if (execution.unknown_status !== "unverified") fail("invalid_oracle", "unknown comptime observations must remain unverified");
  text(execution.resource_policy, "contract.execution.resource_policy");
  text(execution.observation_policy, "contract.execution.observation_policy");
  const certificateConfig = object(execution.observation_certificates, "contract.execution.observation_certificates");
  if (certificateConfig.path !== OBSERVATIONS_PATH
      || certificateConfig.schema !== OBSERVATION_SCHEMA
      || certificateConfig.positive_status !== "verified"
      || certificateConfig.universal_proof !== false) {
    fail("invalid_oracle", "comptime observation certificate execution binding drifted");
  }
  text(certificateConfig.producer, "contract.execution.observation_certificates.producer");
  if (!deepEqual(certificateConfig.required_identity_fields, OBSERVATION_IDENTITY_FIELDS)
      || !deepEqual(certificateConfig.required_observations, OBSERVATION_DIMENSIONS)) {
    fail("invalid_oracle", "comptime observation certificate alphabets drifted");
  }

  const methods = array(contract.binding_methods, "contract.binding_methods");
  if (methods.length !== 3) fail("invalid_oracle", "comptime contract requires three binding methods");
  const seenMethods = new Set();
  for (const [index, method] of methods.entries()) {
    object(method, `contract.binding_methods[${index}]`);
    const name = text(method.method, `contract.binding_methods[${index}].method`);
    if (!METHOD_SET.has(name) || seenMethods.has(name)) fail("invalid_oracle", `invalid or duplicate binding method ${name}`);
    seenMethods.add(name);
    text(method.source, `contract.binding_methods[${index}].source`);
    text(method.implementation, `contract.binding_methods[${index}].implementation`);
    text(method.checker, `contract.binding_methods[${index}].checker`);
    text(method.limit, `contract.binding_methods[${index}].limit`);
    const field = { verified_construction: "construction", direct_verification: "candidate_binding", translation_certificate: "certificate" }[name];
    text(method[field], `contract.binding_methods[${index}].${field}`);
  }

  const identity = object(contract.identity, "contract.identity");
  if (identity.schema !== "jet.compiler-proof.v1" || identity.manifest_path !== PROOF_MANIFEST_PATH) {
    fail("invalid_oracle", "comptime contract must reuse the pinned compiler-proof manifest");
  }
  text(identity.claim_id, "contract.identity.claim_id");
  text(identity.identity_digest_source, "contract.identity.identity_digest_source");
  validateStringList(identity.required_fields, "contract.identity.required_fields");

  const sourceBindings = array(contract.source_bindings, "contract.source_bindings");
  if (sourceBindings.length < 10) fail("invalid_oracle", "comptime implementation map is too small");
  const sourceIds = new Set();
  for (const [index, binding] of sourceBindings.entries()) {
    object(binding, `contract.source_bindings[${index}]`);
    const id = text(binding.id, `contract.source_bindings[${index}].id`);
    if (sourceIds.has(id)) fail("invalid_oracle", `duplicate source binding ${id}`);
    sourceIds.add(id);
    text(binding.path, `${id}.path`);
    validateStringList(binding.markers, `${id}.markers`);
    text(binding.role, `${id}.role`);
  }

  const families = array(contract.construct_families, "contract.construct_families");
  if (families.length < 8) fail("invalid_oracle", "comptime construct denominator is too small");
  const familyIds = new Set();
  for (const [index, family] of families.entries()) {
    object(family, `contract.construct_families[${index}]`);
    const id = text(family.id, `contract.construct_families[${index}].id`);
    if (familyIds.has(id)) fail("invalid_oracle", `duplicate construct family ${id}`);
    familyIds.add(id);
    const sourceBinding = text(family.source_binding, `${id}.source_binding`);
    if (!sourceIds.has(sourceBinding)) fail("invalid_oracle", `${id} names unknown source binding ${sourceBinding}`);
    if (!OBSERVATION_DIMENSIONS.includes(text(family.observation, `${id}.observation`))) {
      fail("invalid_oracle", `${id} names an unknown observation dimension`);
    }
    validateStringList(family.failure_obligations, `${id}.failure_obligations`);
    text(family.rule, `${id}.rule`);
  }

  const coverage = object(contract.coverage, "contract.coverage");
  if (coverage.relation_path !== OBLIGATIONS_PATH || coverage.generator !== CHECKER_PATH || coverage.owner !== "#2938") {
    fail("invalid_oracle", "comptime coverage relation/generator/owner drifted");
  }
  text(coverage.source_generator, "contract.coverage.source_generator");
  text(coverage.denominator_rule, "contract.coverage.denominator_rule");
  const mappingStatuses = array(coverage.mapping_statuses, "contract.coverage.mapping_statuses");
  for (const status of MAPPING_STATUSES) {
    if (!mappingStatuses.includes(status)) fail("invalid_oracle", `coverage omits ${status}`);
  }
  validateStringList(coverage.authorities, "contract.coverage.authorities");
  text(coverage.qualification_rule, "contract.coverage.qualification_rule");

  const boundary = object(contract.boundary, "contract.boundary");
  if (boundary.qualification !== "blocked" || boundary.universal_proof !== false) {
    fail("invalid_oracle", "comptime proof boundary must remain blocked and non-universal");
  }
  for (const field of ["proven", "assumed", "tested", "blocked"]) validateStringList(boundary[field], `contract.boundary.${field}`);

  const dependencies = array(contract.dependencies, "contract.dependencies");
  if (dependencies.length < sourceBindings.length) fail("invalid_oracle", "comptime dependency closure omits implementation inputs");
  for (const [index, dependency] of dependencies.entries()) {
    object(dependency, `contract.dependencies[${index}]`);
    text(dependency.path, `contract.dependencies[${index}].path`);
    digest(dependency.sha256);
    text(dependency.role, `contract.dependencies[${index}].role`);
  }
  digest(contract.witnesses_sha256);
  return { contract, sourceIds, familyIds };
}

function checkSourceBindings(contract) {
  const rows = [];
  const byId = new Map();
  for (const binding of contract.source_bindings) {
    const source = readText(binding.path, `source binding ${binding.id}`);
    const markers = binding.markers.map((marker) => {
      const offset = source.indexOf(marker);
      if (offset < 0) fail("mismatch", `source binding ${binding.id} lost marker ${marker}`, { path: binding.path, marker });
      return { marker, line: lineFor(source, offset) };
    });
    const row = {
      id: binding.id,
      path: binding.path,
      role: binding.role,
      sha256: fileDigest(binding.path, `source binding ${binding.id}`),
      markers,
    };
    rows.push(row);
    byId.set(binding.id, row);
  }
  return { rows: rows.sort((left, right) => left.id.localeCompare(right.id)), byId };
}

function checkDependencies(contract) {
  const seen = new Set();
  const rows = [];
  for (const dependency of contract.dependencies) {
    if (seen.has(dependency.path)) fail("invalid_oracle", `duplicate comptime dependency ${dependency.path}`);
    seen.add(dependency.path);
    const actual = fileDigest(dependency.path, `comptime dependency ${dependency.path}`);
    if (actual !== dependency.sha256) {
      fail("mismatch", `comptime dependency changed: ${dependency.path}`, {
        path: dependency.path,
        expected: dependency.sha256,
        actual,
      });
    }
    rows.push({ path: dependency.path, sha256: actual, role: dependency.role });
  }
  for (const binding of contract.source_bindings) {
    if (!seen.has(binding.path)) fail("invalid_oracle", `source binding ${binding.id} is outside dependency closure`);
  }
  rows.sort((left, right) => left.path.localeCompare(right.path));
  return {
    rows,
    sha256: digestText(canonicalJson(rows)),
  };
}

function checkProofIdentity(contractIdentity) {
  const manifest = readJson(PROOF_MANIFEST_PATH, "proof manifest");
  let summary;
  try {
    summary = validateManifest(manifest);
  } catch (error) {
    fail("invalid_oracle", `proof manifest validation failed: ${error.message}`);
  }
  const claim = array(manifest.claims, "proof manifest claims").find((entry) => entry?.id === contractIdentity.claim_id);
  if (!claim) fail("invalid_oracle", `proof claim ${contractIdentity.claim_id} is missing`);
  let expectedIdentity;
  try {
    expectedIdentity = identityDigest(claim);
  } catch (error) {
    fail("invalid_oracle", `pinned compiler-proof identity could not be computed: ${error.message}`);
  }
  if (claim.identity_sha256 !== expectedIdentity) {
    fail("invalid_oracle", "proof claim identity digest does not match compiler-proof identityDigest", {
      recorded: claim.identity_sha256,
      observed: expectedIdentity,
    });
  }
  return {
    schema: "jet.compiler-proof.v1",
    manifest_id: manifest.manifest_id,
    claim_id: claim.id,
    identity_sha256: claim.identity_sha256,
    toolchain_pin_sha256: summary.pin,
    candidate: canonical(claim.candidate),
    correspondence: canonical(claim.correspondence),
    assumptions: canonical(claim.assumptions),
  };
}

function checkObligationRelation(contract, dependencyInfo) {
  const relation = readJson(contract.coverage.relation_path, "obligation relation");
  if (relation.schema !== "jet.compiler-obligations.v1" || relation.schema_version !== 1) {
    fail("invalid_oracle", "obligation relation schema/version drifted");
  }
  if (relation.generated_by !== "proof/compiler/semantics/generate-obligations.mjs") {
    fail("invalid_oracle", "obligation relation was not generated by the canonical generator");
  }
  const rows = array(relation.rows, "obligation relation.rows");
  if (!rows.some((row) => row?.id === "boundary:compile-time" && row?.owner === "#2938" && row?.family === "comptime")) {
    fail("invalid_oracle", "canonical compile-time boundary row is missing from obligations");
  }
  const authorities = array(relation.authorities, "obligation relation.authorities");
  for (const authority of authorities) {
    object(authority, "obligation relation authority");
    const path = text(authority.path, "obligation relation authority.path");
    const recorded = digest(authority.sha256);
    const actual = fileDigest(path, `obligation authority ${path}`);
    if (actual !== recorded) fail("mismatch", `obligation authority changed: ${path}`, { path, expected: recorded, actual });
    if (!dependencyInfo.rows.some((entry) => entry.path === path)) {
      fail("invalid_oracle", `obligation authority is outside dependency closure: ${path}`);
    }
  }
  const declaredAuthorityPaths = contract.coverage.authorities.slice().sort();
  const relationAuthorityPaths = authorities.map((authority) => authority.path).sort();
  if (!deepEqual(declaredAuthorityPaths, relationAuthorityPaths)) {
    fail("invalid_oracle", "comptime coverage authorities do not match obligation authorities");
  }
  return {
    schema: relation.schema,
    row_count: rows.length,
    sha256: fileDigest(contract.coverage.relation_path, "obligation relation"),
    boundary_row: "boundary:compile-time",
    authorities: authorities.map((authority) => ({ path: authority.path, sha256: authority.sha256 })),
  };
}

function parseRegistryRows() {
  const path = "crates/jet-foundation/src/Syntax/core_calls.rs";
  const source = readText(path, "Core-call registry");
  const expression = /CoreCallRecord::new\s*\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"([^"]+)"/gu;
  const seen = new Map();
  const rows = [];
  let match;
  while ((match = expression.exec(source)) !== null) {
    const [, module, member, symbol] = match;
    const line = lineFor(source, match.index);
    const key = `${module}.${member}`;
    const count = (seen.get(key) ?? 0) + 1;
    seen.set(key, count);
    const id = count === 1 ? `operation:core:${key}` : `operation:core:${key}@${line}`;
    const end = source.indexOf("\n", match.index);
    const rowText = source.slice(match.index, end < 0 ? source.length : end);
    const routeHint = rowText.includes("with_pure_route")
      ? "pure"
      : rowText.includes("with_interpreter_route")
        ? "interpreter"
        : "registry-default";
    rows.push({
      id,
      module,
      member,
      symbol,
      source: path,
      source_line: line,
      route_hint: routeHint,
      disposition: routeHint === "registry-default" ? "unmapped" : "mapped_unproved",
    });
  }
  if (rows.length === 0) fail("invalid_oracle", "Core-call registry has no constructor rows");
  return rows;
}

function checkMixedCase(entry, contract, dependencyInfo, relationInfo, sourceInfo, identityInfo) {
  object(entry, "mixed comptime/runtime witness");
  if (entry.id !== "mixed-reader-mutation-fallible" || entry.kind !== "mixed-comptime-runtime") {
    fail("invalid_oracle", "unexpected mixed witness identity");
  }
  if (entry.fixture !== FIXTURE_PATH) fail("invalid_oracle", "mixed witness fixture path drifted");
  const sourceSha = fileDigest(entry.fixture, "mixed witness fixture");
  if (entry.source_sha256 !== sourceSha) {
    fail("mismatch", "mixed witness fixture changed", { expected: entry.source_sha256, actual: sourceSha });
  }
  if (!dependencyInfo.rows.some((row) => row.path === entry.fixture && row.sha256 === sourceSha)) {
    fail("invalid_oracle", "mixed witness fixture is outside dependency closure");
  }
  const applicableModes = array(entry.applicable_modes, "mixed witness applicable_modes");
  if (!deepEqual(applicableModes, ["aot", "jet_run", "interpreter"])) {
    fail("invalid_oracle", "mixed witness must cover all runtime modes and exclude check mode");
  }
  const allowed = object(entry.allowed_observations, "mixed witness allowed_observations");
  for (const dimension of OBSERVATION_DIMENSIONS) {
    if (!Object.prototype.hasOwnProperty.call(allowed, dimension)) {
      fail("invalid_oracle", `mixed witness omits ${dimension}`);
    }
  }
  const expectedAllowed = {
    result: { comptime: "7|9|1", runtime: "7|9|1" },
    typed_failure: [],
    mutation: [
      { stage: "comptime", binding: "ct", operation: "Reader.read_u8", consumed: 2, remaining: 1 },
      { stage: "runtime", binding: "rt", operation: "Reader.read_u8", consumed: 2, remaining: 1 },
    ],
    input_consumption: [
      { stage: "comptime", input: [7, 9, 11], consumed: [7, 9], remaining: [11] },
      { stage: "runtime", input: [7, 9, 11], consumed: [7, 9], remaining: [11] },
    ],
    ordered_effects: ["stdout:7|9|1", "stdout:7|9|1"],
    cleanup: ["ct-reader", "rt-reader"],
    resource_premises: { fuel: "finite", input: "fixed-bytes", allocation: "successful" },
    schedule: "source-order",
    allowed_schedules: ["source-order"],
  };
  if (!deepEqual(allowed, expectedAllowed)) fail("mismatch", "mixed witness allowed observations changed");

  const observations = object(entry.observations, "mixed witness observations");
  for (const mode of MODES) {
    const observation = object(observations[mode], `mixed witness observation ${mode}`);
    bool(observation.universal_proof, `mixed witness observation ${mode}.universal_proof`);
    if (observation.universal_proof) fail("invalid_oracle", `mixed witness observation ${mode} claims universal proof`);
    if (applicableModes.includes(mode)) {
      if (observation.status !== "unverified") fail("invalid_oracle", `authored mixed observation ${mode} is not unverified`);
      if (!deepEqual(observation.allowed_observations, allowed)) {
        fail("mismatch", `mixed witness observation ${mode} allows a different observation set`);
      }
      const identity = object(observation.identity, `mixed witness observation ${mode}.identity`);
      if (identity.source_sha256 !== sourceSha
          || identity.program_sha256 !== sourceSha
          || identity.dependency_closure_sha256 !== dependencyInfo.sha256
          || identity.configuration_sha256 !== configurationIdentity(contract)
          || identity.evaluator_sha256 !== evaluatorIdentity(contract)
          || identity.obligations_sha256 !== relationInfo.sha256
          || identity.compiler_identity !== identityInfo.identity_sha256
          || identity.checker_sha256 !== fileDigest(CHECKER_PATH, "comptime checker")) {
        fail("mismatch", `mixed witness observation ${mode} identity is stale`);
      }
      if (identity.target !== mode) fail("mismatch", `mixed witness observation ${mode} target is stale`);
    } else {
      if (observation.status !== "not_applicable") fail("invalid_oracle", "check mode must be explicitly not_applicable");
      text(observation.reason, `mixed witness observation ${mode}.reason`);
    }
  }
  return {
    id: entry.id,
    fixture: entry.fixture,
    source_sha256: sourceSha,
    modes: applicableModes,
    statuses: Object.fromEntries(MODES.map((mode) => [mode, observations[mode].status])),
    allowed_observations: canonical(allowed),
    universal_proof: false,
  };
}
function checkObservationCertificates(contract, mixed, dependencyInfo, relationInfo, identityInfo) {
  if (!existsSync(resolve(ROOT, OBSERVATIONS_PATH))) {
    return {
      path: OBSERVATIONS_PATH,
      schema: OBSERVATION_SCHEMA,
      status: "unverified",
      records: [],
      verified_keys: [],
      reason: "no future identity-bound comptime observation carrier was supplied",
      universal_proof: false,
    };
  }
  const payload = readJson(OBSERVATIONS_PATH, "comptime observations");
  if (payload.schema !== OBSERVATION_SCHEMA || payload.schema_version !== 1 || payload.stage !== STAGE) {
    fail("invalid_oracle", "comptime observation carrier schema/version/stage is invalid");
  }
  if (payload.producer !== OBSERVER_PATH || payload.runner !== RUNNER_PATH || payload.compiler !== COMPILER_PATH) {
    fail("invalid_oracle", "comptime observation carrier producer/runner/compiler binding drifted");
  }
  if (payload.fixture !== mixed.fixture || payload.fixture_sha256 !== mixed.source_sha256) {
    fail("mismatch", "comptime observation carrier fixture identity is stale");
  }
  if (!deepEqual(payload.identity_fields, OBSERVATION_IDENTITY_FIELDS)
      || !deepEqual(payload.observation_fields, OBSERVATION_DIMENSIONS)) {
    fail("invalid_oracle", "comptime observation carrier alphabets drifted");
  }
  if (!["verified", "unverified", "unavailable", "failed"].includes(payload.status)) {
    fail("invalid_oracle", `comptime observation carrier has unsupported status ${String(payload.status)}`);
  }
  bool(payload.universal_proof, "comptime observation carrier universal_proof");
  if (payload.universal_proof) fail("invalid_oracle", "comptime observations cannot claim universal proof");
  const records = array(payload.records, "comptime observation carrier records");
  const seen = new Set();
  const verified = [];
  for (const record of records) {
    object(record, "comptime observation certificate record");
    if (record.operation_id !== mixed.id || !mixed.modes.includes(record.mode)) {
      fail("mismatch", "comptime observation certificate names an unknown witness or mode");
    }
    const key = `${record.operation_id}@${record.mode}`;
    if (seen.has(key)) fail("invalid_oracle", `duplicate comptime observation certificate ${key}`);
    seen.add(key);
    if (!deepEqual(record.identity_fields, OBSERVATION_IDENTITY_FIELDS)
        || !deepEqual(record.dimensions, OBSERVATION_DIMENSIONS)) {
      fail("invalid_oracle", `${key} observation certificate alphabets drifted`);
    }
    const expectedIdentity = {
      source_sha256: mixed.source_sha256,
      program_sha256: mixed.source_sha256,
      dependency_closure_sha256: dependencyInfo.sha256,
      configuration_sha256: configurationIdentity(contract),
      evaluator_sha256: evaluatorIdentity(contract),
      compiler_identity: identityInfo.identity_sha256,
      checker_sha256: fileDigest(CHECKER_PATH, "comptime checker"),
      target: record.mode,
      obligations_sha256: relationInfo.sha256,
    };
    const suppliedIdentity = object(record.identity, `${key}.identity`);
    for (const field of OBSERVATION_IDENTITY_FIELDS) {
      if (suppliedIdentity[field] !== expectedIdentity[field]) {
        fail("mismatch", `${key} observation identity does not match current source-bound identity`);
      }
    }
    const certificate = object(record.certificate, `${key}.certificate`);
    if (certificate.schema !== OBSERVATION_CERTIFICATE_SCHEMA
        || certificate.status !== "verified"
        || certificate.universal_proof !== false
        || certificate.producer !== OBSERVER_PATH
        || certificate.checker !== CHECKER_PATH
        || certificate.runner !== RUNNER_PATH) {
      fail("invalid_oracle", `${key} certificate is not an approved positive comptime certificate`);
    }
    text(certificate.execution_id, `${key}.certificate.execution_id`);
    const observations = object(record.observations, `${key}.observations`);
    for (const dimension of OBSERVATION_DIMENSIONS) {
      if (!Object.hasOwn(observations, dimension) || observations[dimension] === null || observations[dimension] === undefined) {
        fail("invalid_oracle", `${key} observations omit ${dimension}`);
      }
    }
    const evaluatorEvents = array(record.evaluator_events, `${key}.evaluator_events`);
    if (evaluatorEvents.length === 0) fail("unverified", `${key} has no canonical evaluator value/input receipt`);
    for (const [index, event] of evaluatorEvents.entries()) {
      object(event, `${key}.evaluator_events[${index}]`);
      if (event.schema !== EVALUATOR_EVENT_SCHEMA
          || event.mode !== record.mode
          || event.operation_id !== record.operation_id
          || typeof event.entry !== "string"
          || !Object.hasOwn(event, "result")) {
        fail("invalid_oracle", `${key} evaluator receipt identity or value is invalid`);
      }
      const inputs = array(event.inputs, `${key}.evaluator_events[${index}].inputs`);
      for (const [inputIndex, input] of inputs.entries()) {
        object(input, `${key}.evaluator_events[${index}].inputs[${inputIndex}]`);
        text(input.path, `${key}.evaluator_events[${index}].inputs[${inputIndex}].path`);
        text(input.hash, `${key}.evaluator_events[${index}].inputs[${inputIndex}].hash`);
      }
      integer(event.fuel_budget, `${key}.evaluator_events[${index}].fuel_budget`);
      if (event.fuel_budget < 1) fail("invalid_oracle", `${key} evaluator receipt has no positive fuel budget`);
    }
    const cacheEvents = array(record.cache_events, `${key}.cache_events`);
    if (cacheEvents.length === 0) fail("unverified", `${key} has no canonical cache lookup/publication receipt`);
    for (const [index, event] of cacheEvents.entries()) {
      object(event, `${key}.cache_events[${index}]`);
      if (event.schema !== CACHE_EVENT_SCHEMA
          || event.mode !== record.mode
          || event.operation_id !== record.operation_id
          || typeof event.action !== "string"
          || typeof event.key !== "string"
          || !["lookup", "restore", "publish", "terminal", "interrupted"].includes(event.phase)
          || !["hit", "miss"].includes(event.status)
          || typeof event.reason !== "string"
          || !Object.hasOwn(event, "outcome")
          || (event.outcome !== null && typeof event.outcome !== "string")) {
        fail("invalid_oracle", `${key} cache receipt identity or status is invalid`);
      }
    }
    if (!deepEqual(observations, mixed.allowed_observations)) {
      fail("mismatch", `${key} observation dimensions differ from the retained allowed observations`);
    }
    verified.push({
      key,
      operation_id: record.operation_id,
      mode: record.mode,
      identity: canonical(suppliedIdentity),
      identity_fields: [...OBSERVATION_IDENTITY_FIELDS],
      dimensions: [...OBSERVATION_DIMENSIONS],
      observations: canonical(observations),
      evaluator_events: canonical(evaluatorEvents),
      cache_events: canonical(cacheEvents),
      certificate: canonical(certificate),
    });
  }
  if (payload.status === "verified" && verified.length !== mixed.modes.length) {
    fail("mismatch", "verified comptime observation carrier does not cover every applicable mode");
  }
  return {
    path: OBSERVATIONS_PATH,
    schema: payload.schema,
    schema_version: payload.schema_version,
    status: payload.status,
    records: verified,
    verified_keys: verified.map((record) => record.key).sort(),
    cache_invalidation: payload.cache_invalidation ?? null,
    reason: payload.status === "verified"
      ? "future runner supplied complete identity-bound positive comptime observations"
      : "future runner supplied no complete positive observation set",
    universal_proof: false,
  };
}

function promoteMixedCase(mixed, observationInfo) {
  const byMode = new Map(observationInfo.records.map((record) => [record.mode, record]));
  const complete = mixed.modes.every((mode) => byMode.has(mode));
  return {
    ...mixed,
    statuses: Object.fromEntries(MODES.map((mode) => [mode, byMode.has(mode) ? "verified" : mixed.statuses[mode]])),
    positive_observations: complete ? observationInfo.records : [],
    observation_status: observationInfo.status,
    qualification: complete ? "observed-but-stage-blocked" : "unverified",
  };
}

function checkBlockingObligations(witnesses) {
  const entries = array(witnesses.blocking_obligations, "blocking obligations");
  const seen = new Set();
  for (const entry of entries) {
    object(entry, "blocking obligation");
    const id = text(entry.id, "blocking obligation.id");
    if (seen.has(id)) fail("invalid_oracle", `duplicate blocking obligation ${id}`);
    seen.add(id);
    if (!REQUIRED_BLOCKERS.includes(id)) fail("invalid_oracle", `unknown blocking obligation ${id}`);
    const status = text(entry.status, `${id}.status`);
    if (!FAILURE_STATUSES.has(status)) fail("invalid_oracle", `${id} has an unclassifiable blocking status ${status}`);
    const disposition = text(entry.disposition, `${id}.disposition`);
    if (!["unsupported", "unmapped", "mapped_unproved"].includes(disposition)) {
      fail("invalid_oracle", `${id} has an invalid disposition ${disposition}`);
    }
    text(entry.source, `${id}.source`);
    text(entry.obligation, `${id}.obligation`);
    bool(entry.universal_proof, `${id}.universal_proof`);
    if (entry.universal_proof) fail("invalid_oracle", `${id} cannot claim universal proof`);
    if (entry.diagnostic !== undefined) validateStringList(entry.diagnostic, `${id}.diagnostic`);
  }
  for (const id of REQUIRED_BLOCKERS) if (!seen.has(id)) fail("invalid_oracle", `required blocker ${id} is missing`);
  const mismatch = entries.find((entry) => entry.id === "known_mismatch_exact_comptime");
  if (mismatch.source !== "docs/audits/domain-foundations-2026-09-02/defects.json") {
    fail("invalid_oracle", "known comptime mismatch must retain the recorded defect ledger");
  }
  const defects = readText(mismatch.source, "known comptime defect ledger");
  if (!defects.includes("DEF-13") || !defects.includes("exact-comptime")) {
    fail("mismatch", "known comptime mismatch ledger no longer names DEF-13");
  }
  return entries.map((entry) => ({
    id: entry.id,
    status: entry.status,
    disposition: entry.disposition,
    source: entry.source,
    obligation: entry.obligation,
    diagnostic: entry.diagnostic ?? [],
    universal_proof: false,
  })).sort((left, right) => left.id.localeCompare(right.id));
}

function checkCacheTransitions(witnesses, contract) {
  const entries = array(witnesses.cache_transitions, "cache transitions");
  const seen = new Set();
  for (const entry of entries) {
    object(entry, "cache transition");
    const id = text(entry.id, "cache transition.id");
    if (seen.has(id)) fail("invalid_oracle", `duplicate cache transition ${id}`);
    seen.add(id);
    if (!REQUIRED_CACHE_EVENTS.includes(id)) fail("invalid_oracle", `unknown cache transition ${id}`);
    text(entry.changed_identity, `${id}.changed_identity`);
    if (!CACHE_EVENT_FRAGMENTS[id].some((fragment) => entry.changed_identity.includes(fragment))) {
      fail("invalid_oracle", `${id} does not name its changed cache identity`);
    }
    if (entry.expected_lookup !== "miss") fail("invalid_oracle", `${id} must force a cache miss`);
    if (!["invalidated", "none"].includes(entry.expected_publication)) {
      fail("invalid_oracle", `${id} has an invalid publication transition`);
    }
    if (entry.status !== "unverified") fail("invalid_oracle", `${id} must remain unverified before replay`);
    bool(entry.universal_proof, `${id}.universal_proof`);
    if (entry.universal_proof) fail("invalid_oracle", `${id} cannot claim universal proof`);
  }
  for (const id of REQUIRED_CACHE_EVENTS) if (!seen.has(id)) fail("invalid_oracle", `required cache transition ${id} is missing`);
  const requiredFields = contract.cache.required_inputs.slice().sort();
  return entries.map((entry) => ({
    id: entry.id,
    changed_identity: entry.changed_identity,
    expected_lookup: entry.expected_lookup,
    expected_publication: entry.expected_publication,
    status: entry.status,
    key_fields: requiredFields,
  })).sort((left, right) => left.id.localeCompare(right.id));
}

function checkWitnesses(witnesses, contract, dependencyInfo, relationInfo, sourceInfo, identityInfo) {
  object(witnesses, "comptime witnesses");
  if (witnesses.schema !== "jet.comptime-witnesses.v1" || witnesses.schema_version !== 1 || witnesses.stage !== STAGE) {
    fail("invalid_oracle", "comptime witness schema/version/stage is unsupported");
  }
  if (witnesses.authored_not_executed !== true) fail("invalid_oracle", "comptime witnesses must be explicitly authored and unexecuted");
  if (witnesses.relation !== "compile_time_evaluation_and_cache") fail("invalid_oracle", "comptime witness relation drifted");
  const certificateBinding = object(witnesses.observation_certificates, "comptime witness observation_certificates");
  if (certificateBinding.path !== OBSERVATIONS_PATH
      || certificateBinding.schema !== OBSERVATION_SCHEMA
      || certificateBinding.producer !== OBSERVER_PATH
      || certificateBinding.universal_proof !== false
      || !["unverified", "verified", "unavailable"].includes(certificateBinding.status)) {
    fail("invalid_oracle", "comptime witness observation certificate binding drifted");
  }
  if (!deepEqual(certificateBinding.required_identity_fields, OBSERVATION_IDENTITY_FIELDS)
      || !deepEqual(certificateBinding.required_observations, OBSERVATION_DIMENSIONS)) {
    fail("invalid_oracle", "comptime witness observation alphabets drifted");
  }
  const certificateStatus = text(certificateBinding.status, "comptime witness observation_certificates.status");
  if (certificateStatus === "verified") fail("invalid_oracle", "authored comptime witness cannot claim verified observations");
  const binding = object(witnesses.binding, "comptime witness binding");
  if (binding.checker !== CHECKER_PATH || binding.driver !== DRIVER_PATH) fail("invalid_oracle", "comptime witness checker/driver binding drifted");
  if (binding.contract_sha256 !== contractBindingDigest(contract)) fail("mismatch", "comptime witness contract binding is stale");
  if (binding.toolchain_pin_sha256 !== identityInfo.toolchain_pin_sha256) fail("mismatch", "comptime witness proof toolchain binding is stale");
  if (binding.dependency_closure_sha256 !== dependencyInfo.sha256) fail("mismatch", "comptime witness dependency closure is stale");
  digest(binding.contract_sha256);
  digest(binding.toolchain_pin_sha256);
  digest(binding.dependency_closure_sha256);
  const expectedSourceDigests = sourceInfo.rows
    .map(({ path, sha256 }) => ({ path, sha256 }))
    .sort((left, right) => left.path.localeCompare(right.path));
  const sourceDigests = array(binding.source_digests, "comptime witness source_digests");
  if (!deepEqual(sourceDigests, expectedSourceDigests)) fail("mismatch", "comptime witness source digest map is stale");

  const identity = object(witnesses.identity, "comptime witness identity");
  if (identity.schema !== "jet.compiler-proof.v1" || identity.claim_id !== identityInfo.claim_id || identity.identity_sha256 !== identityInfo.identity_sha256) {
    fail("mismatch", "comptime witness compiler-proof identity is stale");
  }
  const witnessCoverage = object(witnesses.coverage, "comptime witness coverage");
  if (witnessCoverage.relation_path !== contract.coverage.relation_path || witnessCoverage.generator !== contract.coverage.generator) {
    fail("invalid_oracle", "comptime witness coverage binding drifted");
  }
  if (witnessCoverage.denominator_rule !== contract.coverage.denominator_rule) fail("mismatch", "comptime witness denominator rule is stale");

  const cases = array(witnesses.cases, "comptime witness cases");
  if (cases.length !== 1) fail("invalid_oracle", "comptime witness corpus must retain exactly one mixed case");
  const authoredMixed = checkMixedCase(cases[0], contract, dependencyInfo, relationInfo, sourceInfo, identityInfo);
  const observationCertificates = checkObservationCertificates(contract, authoredMixed, dependencyInfo, relationInfo, identityInfo);
  const mixed = promoteMixedCase(authoredMixed, observationCertificates);
  const blockers = checkBlockingObligations(witnesses);
  const cacheTransitions = checkCacheTransitions(witnesses, contract);
  return {
    count: cases.length,
    mixed,
    blockers,
    cache_transitions: cacheTransitions,
    observation_certificates: observationCertificates,
    categories: ["mixed_mutation", "fallible_operation", "mode_parity", "negative_boundary", "cache_invalidation"],
  };
}

function generatedObligations(contract, relation, sourceInfo, registryRows, blockers) {
  const rows = [];
  const ids = new Set();
  const add = (row) => {
    if (ids.has(row.id)) fail("invalid_oracle", `generated comptime obligation id is duplicated: ${row.id}`);
    ids.add(row.id);
    rows.push({ ...row, universal_proof: false });
  };

  for (const row of relation.rows) {
    if (row.family === "comptime" || row.owner === "#2938") {
      add({
        id: row.id,
        family: "comptime",
        kind: row.kind ?? "boundary",
        subject: row.subject ?? "compile-time evaluation",
        rule: row.rule ?? "comptime.evaluation-and-cache",
        owner: "#2938",
        source: row.source,
        source_line: row.source_line ?? null,
        implementation_bound: true,
        disposition: "mapped_unproved",
        status: "unverified",
      });
    }
  }
  for (const source of sourceInfo.rows) {
    add({
      id: `binding:${source.id}`,
      family: "implementation-correspondence",
      kind: "source-binding",
      subject: source.role,
      rule: "comptime.implementation-correspondence",
      owner: "#2938",
      source: source.path,
      source_line: source.markers[0]?.line ?? null,
      registry_symbol: source.id,
      implementation_bound: true,
      disposition: "mapped_unproved",
      status: "unverified",
    });
  }
  for (const family of contract.construct_families) {
    const source = sourceInfo.byId.get(family.source_binding);
    add({
      id: `construct:${family.id}`,
      family: "comptime-construct",
      kind: "construct",
      subject: family.id,
      rule: family.rule,
      owner: "#2938",
      source: source.path,
      source_line: source.markers[0]?.line ?? null,
      registry_symbol: family.source_binding,
      observation: family.observation,
      failure_obligations: family.failure_obligations,
      resource_premises: [...OPERATION_EVIDENCE.resource_premises],
      implementation_bound: true,
      disposition: "mapped_unproved",
      status: "unverified",
    });
  }
  for (const row of registryRows) {
    add({
      id: row.id,
      family: "comptime-core-operation",
      kind: "operation",
      subject: `${row.module}.${row.member}`,
      rule: "comptime.core-call-registry",
      owner: "#2938",
      source: row.source,
      source_line: row.source_line,
      registry_symbol: row.symbol,
      route_hint: row.route_hint,
      ...OPERATION_EVIDENCE,
      implementation_bound: true,
      disposition: row.disposition,
      status: "unverified",
    });
  }
  for (const blocker of blockers) {
    add({
      id: `blocking:${blocker.id}`,
      family: "blocking",
      kind: "negative",
      subject: blocker.id,
      rule: "comptime.blocking-obligation",
      owner: "#2938",
      source: blocker.source,
      source_line: null,
      registry_symbol: blocker.obligation,
      implementation_bound: true,
      disposition: blocker.disposition,
      status: blocker.status,
      diagnostic: blocker.diagnostic,
    });
  }
  rows.sort((left, right) => left.id.localeCompare(right.id));
  const statusCounts = Object.fromEntries(MAPPING_STATUSES.map((status) => [status, 0]));
  for (const row of rows) statusCounts[row.disposition] += 1;
  if (statusCounts.proved !== 0) fail("invalid_oracle", "comptime generated obligations contain a proved row");
  return {
    relation: contract.coverage.relation_path,
    generator: contract.coverage.generator,
    denominator: rows.length,
    complete_denominator: true,
    source_binding_count: sourceInfo.rows.length,
    core_operation_count: registryRows.length,
    rows,
    status_counts: statusCounts,
    mapped_unproved: rows.filter((row) => row.disposition === "mapped_unproved").map((row) => row.id),
    unsupported: rows.filter((row) => row.disposition === "unsupported").map((row) => row.id),
    unmapped: rows.filter((row) => row.disposition === "unmapped").map((row) => row.id),
    proved: [],
    qualification: "blocked",
    universal_proof: false,
  };
}

function runCheck() {
  const contract = readJson(CONTRACT_PATH, "comptime contract");
  const checkedContract = checkContract(contract);
  const identity = checkProofIdentity(contract.identity);
  const sourceInfo = checkSourceBindings(contract);
  const dependencyInfo = checkDependencies(contract);
  const relationInfo = checkObligationRelation(contract, dependencyInfo);
  const relation = readJson(contract.coverage.relation_path, "obligation relation");
  const witnesses = readJson(WITNESSES_PATH, "comptime witnesses");
  const witnessSha = fileDigest(WITNESSES_PATH, "comptime witnesses");
  if (contract.witnesses_sha256 !== witnessSha) {
    fail("mismatch", "comptime witness file is stale", { expected: contract.witnesses_sha256, actual: witnessSha });
  }
  const witnessInfo = checkWitnesses(witnesses, contract, dependencyInfo, relationInfo, sourceInfo, identity);
  const registryRows = parseRegistryRows();
  const obligations = generatedObligations(contract, relation, sourceInfo, registryRows, witnessInfo.blockers);
  const configurationSha = configurationIdentity(contract);
  const evaluatorSha = evaluatorIdentity(contract);
  const checkerSha = fileDigest(CHECKER_PATH, "comptime checker");
  return {
    schema: CHECK_SCHEMA,
    schema_version: 1,
    stage: STAGE,
    status: "blocked",
    contract: {
      id: contract.contract_id,
      schema: contract.schema,
      schema_version: contract.schema_version,
      stage: STAGE,
      claim: contract.claim.kind,
      owner: contract.owner,
      evaluator: contract.evaluator.implementation.id,
      cache_key_schema: contract.cache.key_schema,
    },
    identity: {
      ...identity,
      checker_sha256: checkerSha,
      configuration_sha256: configurationSha,
      evaluator_sha256: evaluatorSha,
    },
    binding: {
      checker: CHECKER_PATH,
      checker_sha256: checkerSha,
      driver: DRIVER_PATH,
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      contract_sha256: contractBindingDigest(contract),
      toolchain_pin_sha256: identity.toolchain_pin_sha256,
      dependency_closure_sha256: dependencyInfo.sha256,
      source_bindings: sourceInfo.rows,
      dependency_count: dependencyInfo.rows.length,
      obligations_sha256: relationInfo.sha256,
    },
    dependencies: {
      status: "matched",
      closure_sha256: dependencyInfo.sha256,
      entries: dependencyInfo.rows,
      exact_identity: true,
    },
    evaluator: {
      model: canonical(contract.evaluator.model),
      implementation: canonical(contract.evaluator.implementation),
      model_sha256: digestText(canonicalJson(contract.evaluator.model)),
      implementation_sha256: digestText(canonicalJson(contract.evaluator.implementation)),
      evaluator_sha256: evaluatorSha,
      canonical_seam: contract.evaluator.implementation.canonical_seam,
      second_engine: false,
    },
    witnesses: {
      authored_not_executed: true,
      count: witnessInfo.count,
      categories: witnessInfo.categories,
      mixed: witnessInfo.mixed,
      blockers: witnessInfo.blockers,
      cache_transitions: witnessInfo.cache_transitions,
      observation_certificates: witnessInfo.observation_certificates,
    },
    compiler_execution: {
      status: witnessInfo.observation_certificates.status,
      requested: witnessInfo.observation_certificates.status !== "unverified",
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      timeout_ms: TIMEOUT_MS,
      observation_carrier: OBSERVATIONS_PATH,
      reason: witnessInfo.observation_certificates.reason,
    },
    comparison: {
      schema_version: 1,
      status: witnessInfo.mixed.qualification === "observed-but-stage-blocked" ? "matched" : "unverified",
      relation: "compile_time_evaluation_and_cache",
      samples: [witnessInfo.mixed],
      discarded_cases: 0,
      first_difference: null,
      reduced_counterexample: null,
      contamination: null,
      reason: witnessInfo.mixed.qualification === "observed-but-stage-blocked"
        ? "the approved observer matched the retained mixed witness; denominator blockers still prevent stage qualification"
        : "source/dependency bindings are current, but no complete comptime mode observation certificate was supplied",
      universal_proof: false,
    },
    cache: {
      key_schema: contract.cache.key_schema,
      required_inputs: contract.cache.required_inputs,
      invalidation_events: witnessInfo.cache_transitions,
      lookup_gate: contract.cache.lookup_gate,
      action_key: contract.cache.action_key,
      publication_requires_completion: true,
      cancelled_publication: "rejected",
      failed_action_outputs: "empty",
    },
    coverage: obligations,
    obligation_relation: relationInfo,
    proof_boundary: {
      ...canonical(checkedContract.contract.boundary),
      qualification: "blocked",
      universal_proof: false,
      compiler_execution_status: witnessInfo.observation_certificates.status,
      source_witnesses_executed: false,
    },
  };
}

function errorPayload(error) {
  const code = error instanceof CheckError ? error.code : "invalid_oracle";
  const status = CHECK_STATUSES.has(code) ? code : "invalid_oracle";
  return {
    schema: CHECK_SCHEMA,
    schema_version: 1,
    stage: STAGE,
    status,
    error: {
      code,
      message: error?.message ?? String(error),
      ...(error?.details === undefined ? {} : { details: error.details }),
    },
    proof_boundary: { qualification: "blocked", universal_proof: false },
  };
}

function printHuman(result) {
  process.stdout.write(`comptime contract: ${result.status}\n`);
  if (result.identity) process.stdout.write(`candidate: ${result.identity.claim_id} ${result.identity.identity_sha256}\n`);
  if (result.coverage) process.stdout.write(`coverage: ${result.coverage.denominator} generated obligations; qualification=${result.coverage.qualification}\n`);
  if (result.compiler_execution) process.stdout.write(`compiler: ${result.compiler_execution.status}\n`);
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}

export async function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const allowed = new Set(["--json"]);
  if (argv.some((arg) => !allowed.has(arg))) {
    const result = errorPayload(new CheckError("invalid_oracle", "usage: comptime/check.mjs [--json]"));
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 64;
  }
  try {
    const result = runCheck();
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 0;
  } catch (error) {
    const result = errorPayload(error);
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    if (result.status === "unavailable") return 3;
    if (result.status === "timeout") return 4;
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await main();
}

export {
  CHECK_SCHEMA,
  checkContract,
  checkSourceBindings,
  checkDependencies,
  generatedObligations,
  runCheck,
};
