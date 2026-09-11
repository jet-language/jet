#!/usr/bin/env node

/**
 * Machine checker for the #2925 semantic contract experiment.
 *
 * It consumes one semantic contract, one generated obligation relation, and
 * one retained witness corpus. The finite model/implementation comparison is
 * reported through the canonical TestingComparison record shape. It never
 * turns finite agreement, an assumed premise, or an unavailable proof-tool
 * replay into universal proof.
 */

import { createHash } from "node:crypto";
import { accessSync, constants as fsConstants, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

import {
  canonical,
  canonicalJson,
  evaluate as evaluateModel,
  toObservation as modelObservation,
  MAX_FUEL,
  MAX_STEPS,
  MODEL_ID,
  OBSERVATION_RELATION,
} from "./model.mjs";
import {
  execute as executeImplementation,
  toObservation as implementationObservation,
} from "./implementation.mjs";
import { buildObligations, OBLIGATION_AUTHORITIES } from "./generate-obligations.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const CONTRACT_PATH = "proof/compiler/semantics/contract.json";
const WITNESSES_PATH = "proof/compiler/semantics/witnesses.json";
const OBLIGATIONS_PATH = "proof/compiler/obligations.json";
const PROOF_MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const PROOF_TOOL_PATH = "scripts/agent/compiler-proof.mjs";
const CHECK_SCHEMA = "jet.semantic-contract-check.v1";
const COMPARISON_SCHEMA_VERSION = 1;
const COMPARISON_STATUSES = Object.freeze([
  "matched",
  "mismatch",
  "empty",
  "unsupported",
  "unavailable",
  "timeout",
  "cancelled",
  "invalid_oracle",
  "contaminated",
]);
const STATUS_SET = new Set(COMPARISON_STATUSES);
const METHOD_SET = new Set(["verified_construction", "direct_verification", "translation_certificate"]);
const NO_DUPLICATE_OWNERS = Object.freeze(["#2898", "#2335", "#2919"]);
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
const JET_TIERS = Object.freeze(["aot", "jet_run", "interpreter", "check"]);
const MAPPING_STATUSES = Object.freeze(["observed", "unobserved", "not_applicable"]);
const COVERAGE_STATUSES = Object.freeze(["mapped_unproved", "unsupported", "unmapped", "proved"]);
const JET_EXECUTION_STATUSES = Object.freeze(["observed", "unverified", "unavailable", "failed"]);
const JET_RUNNER_PATH = "scripts/agent/jet-env";
const JET_COMPILER_PATH = "target/debug/jet";
const JET_SCRATCH_ENV = "JET_TEST_SCRATCH_DIR";
const JET_IDENTITY_FIELDS = Object.freeze(["program_sha256", "source_sha256"]);
const JET_EXECUTION_TIMEOUT_MS = 120000;

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

function absolute(path) {
  return resolve(ROOT, path);
}

function readJson(path, label) {
  const target = absolute(path);
  if (!existsSync(target)) fail("unavailable", `missing ${label}: ${path}`, { path });
  try {
    return JSON.parse(readFileSync(target, "utf8"));
  } catch (error) {
    fail("invalid_oracle", `invalid JSON in ${label}: ${error.message}`, { path });
  }
}

function text(value, label) {
  if (typeof value !== "string" || value.length === 0) fail("invalid_oracle", `${label} must be a non-empty string`);
  return value;
}

function array(value, label) {
  if (!Array.isArray(value)) fail("invalid_oracle", `${label} must be an array`);
  return value;
}

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail("invalid_oracle", `${label} must be an object`);
  return value;
}

function deepEqual(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function digestText(textValue) {
  return `sha256-${createHash("sha256").update(textValue, "utf8").digest("hex")}`;
}

function assertStatus(status, label) {
  if (!STATUS_SET.has(status)) fail("invalid_oracle", `${label} uses an unclassifiable comparison status ${String(status)}`);
}

function checkContract(contract) {
  object(contract, "contract");
  if (contract.schema !== "jet.semantic-contract.v1" || contract.schema_version !== 1) {
    fail("invalid_oracle", "semantic contract schema/version is unsupported");
  }
  text(contract.contract_id, "contract.contract_id");
  const policy = object(contract.policy, "contract.policy");
  if (policy.decision_id !== "D-COMPILER-PROOF1" || policy.outcome !== "A" || policy.status !== "ratified") {
    fail("invalid_oracle", "semantic contract does not record ratified D-COMPILER-PROOF1 outcome A");
  }
  const claim = object(contract.claim, "contract.claim");
  text(claim.kind, "contract.claim.kind");
  text(claim.statement, "contract.claim.statement");
  for (const field of ["preconditions", "observation_alphabet", "permitted_resource_failures", "trusted_components", "unsupported_cases"]) {
    const values = array(claim[field], `contract.claim.${field}`);
    if (values.length === 0 || values.some((value) => typeof value !== "string" || value.length === 0)) {
      fail("invalid_oracle", `contract.claim.${field} must contain named entries`);
    }
  }
  const expectedObservations = new Set(OBSERVATION_DIMENSIONS);
  const actualObservations = [...new Set(claim.observation_alphabet)].sort();
  const requiredObservations = [...expectedObservations].sort();
  if (claim.observation_alphabet.length !== requiredObservations.length
      || !deepEqual(actualObservations, requiredObservations)) {
    fail("invalid_oracle", "semantic observation alphabet does not cover the complete contract");
  }

  const execution = object(contract.execution, "contract.execution");
  if (execution.schema !== "jet.semantic-witness-mapping.v1"
      || execution.runner !== JET_RUNNER_PATH
      || execution.compiler !== JET_COMPILER_PATH
      || execution.unknown_status !== "unverified"
      || execution.resource_policy !== "unknown resource observations remain unproved") {
    fail("invalid_oracle", "semantic Jet mapping execution policy is missing or drifted");
  }
  const executionOperations = array(execution.operations, "contract.execution.operations");
  if (!deepEqual(executionOperations, ["run", "check"])) fail("invalid_oracle", "semantic Jet mapping operations drifted");
  const executionTiers = array(execution.tiers, "contract.execution.tiers");
  if (!deepEqual(executionTiers, JET_TIERS)) fail("invalid_oracle", "semantic Jet mapping tiers drifted");
  const executionDimensions = array(execution.observation_fields, "contract.execution.observation_fields");
  const executionIdentityFields = array(execution.identity_fields, "contract.execution.identity_fields");
  if (!deepEqual(executionIdentityFields, JET_IDENTITY_FIELDS)) fail("invalid_oracle", "semantic Jet mapping identity fields drifted");
  if (!deepEqual(executionDimensions, OBSERVATION_DIMENSIONS)) fail("invalid_oracle", "semantic Jet mapping observation dimensions drifted");
  if (!Number.isSafeInteger(execution.timeout_ms) || execution.timeout_ms < 1) {
    fail("invalid_oracle", "semantic Jet mapping timeout must be a positive integer");
  }


  const methods = array(contract.binding_methods, "contract.binding_methods");
  if (methods.length !== 3) fail("invalid_oracle", "exactly three implementation-binding methods are required");
  const seenMethods = new Set();
  for (const [index, binding] of methods.entries()) {
    object(binding, `contract.binding_methods[${index}]`);
    const method = text(binding.method, `contract.binding_methods[${index}].method`);
    if (!METHOD_SET.has(method) || seenMethods.has(method)) fail("invalid_oracle", `invalid or duplicate binding method ${method}`);
    seenMethods.add(method);
    text(binding.source, `contract.binding_methods[${index}].source`);
    text(binding.implementation, `contract.binding_methods[${index}].implementation`);
    text(binding.checker, `contract.binding_methods[${index}].checker`);
    text(binding.limit, `contract.binding_methods[${index}].limit`);
    const methodField = {
      verified_construction: "construction",
      direct_verification: "candidate_binding",
      translation_certificate: "certificate",
    }[method];
    text(binding[methodField], `contract.binding_methods[${index}].${methodField}`);
  }
  if (seenMethods.size !== METHOD_SET.size) fail("invalid_oracle", "one of the three binding methods is missing");

  const identity = object(contract.identity, "contract.identity");
  if (identity.schema !== "jet.compiler-proof.v1") fail("invalid_oracle", "semantic contract must reuse jet.compiler-proof.v1");
  if (identity.manifest_path !== PROOF_MANIFEST_PATH) fail("invalid_oracle", "semantic contract names a different proof manifest");
  text(identity.claim_id, "contract.identity.claim_id");
  text(identity.identity_digest_source, "contract.identity.identity_digest_source");
  const identityFields = array(identity.required_fields, "contract.identity.required_fields");
  if (identityFields.length < 10 || identityFields.some((field) => typeof field !== "string" || field.length === 0)) {
    fail("invalid_oracle", "candidate identity field list is incomplete");
  }

  const coverage = object(contract.coverage, "contract.coverage");
  if (coverage.relation_path !== OBLIGATIONS_PATH || coverage.generator !== "proof/compiler/semantics/generate-obligations.mjs") {
    fail("invalid_oracle", "coverage relation or generator path drifted");
  }
  const coverageAuthorities = array(coverage.authorities, "contract.coverage.authorities");
  if (!deepEqual(coverageAuthorities, OBLIGATION_AUTHORITIES)) {
    fail("invalid_oracle", "coverage authority list drifted from the obligation generator");
  }
  if (typeof coverage.owner_rule !== "string" || coverage.owner_rule.length === 0) {
    fail("invalid_oracle", "coverage owner rule is missing");
  }
  const noDuplicateOwners = array(coverage.no_duplicate_owners, "coverage.no_duplicate_owners");
  if (!deepEqual(noDuplicateOwners, NO_DUPLICATE_OWNERS)) {
    fail("invalid_oracle", "coverage no-duplicate owner list drifted from the excluded card set");
  }
  if (noDuplicateOwners.length === 0 || noDuplicateOwners.some((owner) => typeof owner !== "string" || owner.length === 0)) {
    fail("invalid_oracle", "coverage no-duplicate owner list is incomplete");
  }
  const mappingStatuses = array(coverage.mapping_statuses, "contract.coverage.mapping_statuses");
  if (!deepEqual(mappingStatuses, COVERAGE_STATUSES)) {
    fail("invalid_oracle", "coverage mapping status vocabulary drifted");
  }
  text(coverage.qualification_rule, "contract.coverage.qualification_rule");


  const boundary = object(contract.boundary, "contract.boundary");
  const labels = ["proven", "assumed", "tested", "blocked"];
  const allBoundaryEntries = new Set();
  for (const label of labels) {
    const values = array(boundary[label], `contract.boundary.${label}`);
    if (values.length === 0 || values.some((value) => typeof value !== "string" || value.length === 0)) {
      fail("invalid_oracle", `contract.boundary.${label} must contain named entries`);
    }
    for (const value of values) {
      if (allBoundaryEntries.has(value)) fail("invalid_oracle", `boundary entry appears in more than one class: ${value}`);
      allBoundaryEntries.add(value);
    }
  }
  if (boundary.qualification !== "blocked") fail("invalid_oracle", "semantic experiment cannot qualify as an unbounded proof");
  const comparisonAuthority = object(contract.comparison_authority, "contract.comparison_authority");
  if (comparisonAuthority.path !== "crates/jet-foundation/src/TestingComparison.rs" || comparisonAuthority.schema_version !== 1) {
    fail("invalid_oracle", "comparison authority is not canonical TestingComparison");
  }
  const statuses = array(comparisonAuthority.statuses, "contract.comparison_authority.statuses");
  if (!deepEqual(statuses, COMPARISON_STATUSES)) fail("invalid_oracle", "comparison status vocabulary drifted from TestingComparison");
  if (comparisonAuthority.universal_proof !== false) fail("invalid_oracle", "finite comparison cannot claim universal proof");
  return { identity, execution, coverage, boundary, claim };
}

async function checkProofIdentity(contractIdentity) {
  const manifest = readJson(PROOF_MANIFEST_PATH, "proof manifest");
  let proofTool;
  try {
    proofTool = await import(pathToFileURL(absolute(PROOF_TOOL_PATH)).href);
  } catch (error) {
    fail("unavailable", `compiler-proof identity helper is unavailable: ${error.message}`);
  }
  if (typeof proofTool.validateManifest !== "function" || typeof proofTool.identityDigest !== "function") {
    fail("unavailable", "compiler-proof identity helper does not expose validateManifest/identityDigest");
  }
  let summary;
  try {
    summary = proofTool.validateManifest(manifest);
  } catch (error) {
    fail("invalid_oracle", `proof manifest validation failed: ${error.message}`);
  }
  if (manifest.schema !== "jet.compiler-proof.v1") fail("invalid_oracle", "proof manifest schema changed");
  const claims = array(manifest.claims, "proof manifest claims");
  const claim = claims.find((entry) => entry?.id === contractIdentity.claim_id);
  if (!claim) fail("invalid_oracle", `proof claim ${contractIdentity.claim_id} is missing`);
  const expectedIdentity = proofTool.identityDigest(claim);
  if (claim.identity_sha256 !== expectedIdentity) {
    fail("invalid_oracle", "proof claim identity digest does not match compiler-proof identityDigest", {
      recorded: claim.identity_sha256,
      observed: expectedIdentity,
    });
  }
  const candidate = object(claim.candidate, "proof claim candidate");
  const recordIdentity = object(candidate.record_identity, "proof claim candidate.record_identity");
  for (const field of ["target_inputs_sha256", "tool_version", "engine"]) text(recordIdentity[field], `candidate.record_identity.${field}`);
  text(candidate.commit, "candidate.commit");
  object(candidate.compiler, "candidate.compiler");
  object(candidate.compiler.source, "candidate.compiler.source");
  object(candidate.compiler.artifact, "candidate.compiler.artifact");
  object(candidate.configuration, "candidate.configuration");
  object(candidate.target, "candidate.target");
  const correspondence = object(claim.correspondence, "proof claim correspondence");
  if (!METHOD_SET.has(text(correspondence.method, "proof claim correspondence.method"))) {
    fail("invalid_oracle", "proof claim correspondence uses an unknown binding method");
  }
  text(correspondence.route, "proof claim correspondence.route");
  text(correspondence.checked_declaration, "proof claim correspondence.checked_declaration");
  const assumptions = array(claim.assumptions, "proof claim assumptions");
  if (assumptions.length === 0) fail("invalid_oracle", "proof claim has no explicit assumptions");
  for (const [index, assumption] of assumptions.entries()) {
    object(assumption, `proof claim assumption ${index}`);
    text(assumption.kind, `proof claim assumption ${index}.kind`);
    text(assumption.id, `proof claim assumption ${index}.id`);
    text(assumption.statement, `proof claim assumption ${index}.statement`);
    if (assumption.status !== "assumed") fail("invalid_oracle", `proof claim assumption ${index} is not marked assumed`);
  }
  const checker = object(claim.checker, "proof claim checker");
  text(checker.toolchain_pin_sha256, "checker.toolchain_pin_sha256");
  object(claim.proof, "proof claim proof");
  object(claim.proof.artifact, "proof claim proof.artifact");
  object(claim.raw_replay, "proof claim raw_replay");
  return {
    schema: "jet.compiler-proof.v1",
    manifest_id: manifest.manifest_id,
    claim_id: claim.id,
    identity_sha256: claim.identity_sha256,
    model: canonical(claim.model),
    source: canonical(claim.source),
    implementation: canonical(claim.implementation),
    correspondence: canonical(correspondence),
    candidate: canonical(candidate),
    checker: canonical(checker),
    assumptions: canonical(assumptions),
    proof: canonical({ artifact: claim.proof.artifact, formal_goal: claim.proof.formal_goal }),
    raw_replay: canonical(claim.raw_replay),
    toolchain_pin_sha256: summary.pin,
  };
}

function validateObligationRecord(recorded, expected = buildObligations()) {
  if (recorded.schema !== "jet.compiler-obligations.v1" || recorded.schema_version !== 1) {
    fail("invalid_oracle", "obligation relation schema/version is unsupported");
  }
  if (canonicalJson(recorded) !== canonicalJson(expected)) {
    fail("invalid_oracle", "obligation relation is stale; regenerate it from current authorities", {
      command: "scripts/agent/jet-env node proof/compiler/semantics/generate-obligations.mjs",
      recorded_rows: recorded.row_count,
      expected_rows: expected.row_count,
    });
  }
  const rows = array(recorded.rows, "obligation relation rows");
  const owners = new Set(array(recorded.owner_set, "obligation relation owner_set"));
  const excludedOwners = new Set(NO_DUPLICATE_OWNERS);
  if (NO_DUPLICATE_OWNERS.some((owner) => owners.has(owner))) {
    fail("invalid_oracle", "obligation owner_set contains an excluded duplicate owner");
  }
  const ids = new Set();
  const families = Object.create(null);
  for (const [index, row] of rows.entries()) {
    object(row, `obligation relation row ${index}`);
    for (const field of ["id", "family", "kind", "subject", "rule", "owner", "source", "obligation_state"]) text(row[field], `obligation relation row ${index}.${field}`);
    if (ids.has(row.id)) fail("invalid_oracle", `duplicate obligation id ${row.id}`);
    ids.add(row.id);
    if (excludedOwners.has(row.owner)) fail("invalid_oracle", `obligation ${row.id} uses excluded duplicate owner ${row.owner}`);
    if (!owners.has(row.owner)) fail("invalid_oracle", `obligation ${row.id} has no live owner ${row.owner}`);
    if (row.obligation_state !== "uncovered") fail("invalid_oracle", `obligation ${row.id} hides its uncovered state`);
    families[row.family] = (families[row.family] ?? 0) + 1;
  }
  if (recorded.row_count !== rows.length || rows.length === 0) fail("invalid_oracle", "obligation relation row count is not mechanical");
  const rowIds = rows.map((row) => row.id);
  return {
    schema: recorded.schema,
    schema_version: recorded.schema_version,
    row_count: rows.length,
    row_ids: rowIds,
    families: canonical(families),
    generated_from: recorded.authorities,
    all_rows_have_rule_and_owner: true,
    all_rows_explicitly_uncovered: true,
  };
}

function checkObligations() {
  return validateObligationRecord(readJson(OBLIGATIONS_PATH, "obligation relation"));
}

function comparisonIdentity(caseId, program) {
  return {
    case_id: caseId,
    input_id: digestText(canonicalJson(program)),
    seed: null,
    source: WITNESSES_PATH,
    tool: "proof/compiler/semantics/check.mjs",
    target: MODEL_ID,
  };
}

function comparisonSample(caseId, program, reference, candidate, referenceReplay, candidateReplay) {
  return {
    identity: comparisonIdentity(caseId, program),
    reference,
    candidate,
    reference_replay: referenceReplay,
    candidate_replay: candidateReplay,
    declared_equal: null,
  };
}
function checkObservation(observation, label) {
  object(observation, label);
  for (const field of [
    "raw",
    "result",
    "typed_failure",
    "mutation",
    "input_consumption",
    "ordered_effects",
    "cleanup",
    "resource_premises",
    "schedule",
    "allowed_schedules",
  ]) {
    if (!Object.prototype.hasOwnProperty.call(observation, field)) {
      fail("invalid_oracle", `${label}.${field} is missing`);
    }
  }
  if (typeof observation.raw !== "string") fail("invalid_oracle", `${label}.raw must be a string`);
  if (observation.result !== null) {
    const result = object(observation.result, `${label}.result`);
    if (result.kind === "value") {
      if (!Object.prototype.hasOwnProperty.call(result, "value")) fail("invalid_oracle", `${label}.result.value is missing`);
    } else if (result.kind === "failure") {
      text(result.error, `${label}.result.error`);
    } else {
      fail("invalid_oracle", `${label}.result.kind is not value or failure`);
    }
  }
  for (const field of ["typed_failure", "mutation"]) {
    if (observation[field] !== null && typeof observation[field] !== "string") {
      fail("invalid_oracle", `${label}.${field} must be a string or null`);
    }
  }
  for (const field of ["input_consumption", "ordered_effects", "cleanup", "schedule"]) {
    const values = array(observation[field], `${label}.${field}`);
    if (values.some((value) => typeof value !== "string")) {
      fail("invalid_oracle", `${label}.${field} must contain strings`);
    }
  }
  const allowedSchedules = array(observation.allowed_schedules, `${label}.allowed_schedules`);
  if (allowedSchedules.some((schedule) => !Array.isArray(schedule) || schedule.length === 0 || schedule.some((task) => typeof task !== "string" || task.length === 0))) {
    fail("invalid_oracle", `${label}.allowed_schedules must contain non-empty task schedules`);
  }
  if (observation.schedule.length > 0
      && !allowedSchedules.some((schedule) => deepEqual(schedule, observation.schedule))) {
    fail("invalid_oracle", `${label}.schedule is not one of its allowed_schedules`);
  }
  if (observation.resource_premises !== null) object(observation.resource_premises, `${label}.resource_premises`);
}


function baseRecord(samples, status, reason) {
  assertStatus(status, "comparison record");
  return {
    schema_version: COMPARISON_SCHEMA_VERSION,
    status,
    relation: OBSERVATION_RELATION,
    samples,
    discarded_cases: 0,
    first_difference: null,
    reduced_counterexample: null,
    contamination: null,
    reason,
    universal_proof: false,
  };
}

function compareSamples(samples, discardedCases = 0) {
  if (!Array.isArray(samples)) {
    return baseRecord([], "invalid_oracle", "comparison samples must be an array");
  }
  if (!Number.isSafeInteger(discardedCases) || discardedCases < 0) {
    return baseRecord(samples, "invalid_oracle", "discarded case count must be a non-negative integer");
  }
  const record = baseRecord(samples, "empty", null);
  record.discarded_cases = discardedCases;
  if (samples.length === 0) {
    record.reason = discardedCases === 0 ? "comparison corpus is empty" : "all comparison cases were discarded";
    return record;
  }
  for (const [index, sample] of samples.entries()) {
    try {
      object(sample.identity, `comparison sample ${index}.identity`);
      for (const field of ["case_id", "input_id", "source", "tool", "target"]) text(sample.identity[field], `comparison sample ${index}.identity.${field}`);
      if (sample.identity.seed !== null && sample.identity.seed !== undefined
          && (!Number.isSafeInteger(sample.identity.seed) || sample.identity.seed < 0)) {
        fail("invalid_oracle", `comparison sample ${index}.identity.seed must be a non-negative integer or null`);
      }
      if (sample.declared_equal !== null && sample.declared_equal !== undefined && typeof sample.declared_equal !== "boolean") {
        fail("invalid_oracle", `comparison sample ${index}.declared_equal must be a boolean or null`);
      }
      checkObservation(sample.reference, `comparison sample ${index}.reference`);
      checkObservation(sample.candidate, `comparison sample ${index}.candidate`);
      if (sample.reference_replay !== null && sample.reference_replay !== undefined) {
        checkObservation(sample.reference_replay, `comparison sample ${index}.reference_replay`);
      }
      if (sample.candidate_replay !== null && sample.candidate_replay !== undefined) {
        checkObservation(sample.candidate_replay, `comparison sample ${index}.candidate_replay`);
      }
      if (sample.reference_replay && !deepEqual(sample.reference_replay, sample.reference)) {
        record.status = "contaminated";
        record.contamination = `reference observation changed on replay for case ${sample.identity.case_id}`;
        record.reason = record.contamination;
        return record;
      }
      if (sample.candidate_replay && !deepEqual(sample.candidate_replay, sample.candidate)) {
        record.status = "contaminated";
        record.contamination = `candidate observation changed on replay for case ${sample.identity.case_id}`;
        record.reason = record.contamination;
        return record;
      }
      const differences = [
        "result",
        "typed_failure",
        "mutation",
        "input_consumption",
        "ordered_effects",
        "cleanup",
        "resource_premises",
        "schedule",
        "allowed_schedules",
        "raw",
      ].filter((field) => !deepEqual(sample.reference[field], sample.candidate[field]));
      if (differences.length > 0) {
        record.status = "mismatch";
        record.first_difference = index;
        record.reduced_counterexample = {
          ...sample,
          first_differing_observation: differences[0],
        };
        record.reason = `first differing observation is ${differences[0]} in case ${sample.identity.case_id}`;
        return record;
      }
    } catch (error) {
      if (error instanceof CheckError && error.code === "invalid_oracle") {
        record.status = "invalid_oracle";
        record.reason = error.message;
        return record;
      }
      throw error;
    }
  }
  if (discardedCases > 0) {
    record.status = "invalid_oracle";
    record.reason = "comparison discarded cases and cannot claim equality";
  } else {
    record.status = "matched";
    record.reason = `${samples.length} observed case(s) matched under ${OBSERVATION_RELATION}`;
  }
  return record;
}



function expectedObservationCheck(entry, modelResult, implementationResult) {
  const expected = object(entry.expect, `${entry.id}.expect`);
  if (expected.accepted !== undefined && modelResult.accepted !== expected.accepted) fail("invalid_oracle", `${entry.id} model accepted state disagrees with witness expectation`);
  if (expected.accepted !== undefined && implementationResult.accepted !== expected.accepted) fail("invalid_oracle", `${entry.id} implementation accepted state disagrees with witness expectation`);
  if (!modelResult.accepted) {
    if (expected.diagnostic && modelResult.diagnostic.code !== expected.diagnostic) fail("invalid_oracle", `${entry.id} model diagnostic disagrees with witness expectation`);
    if (expected.diagnostic && implementationResult.diagnostic.code !== expected.diagnostic) fail("invalid_oracle", `${entry.id} implementation diagnostic disagrees with witness expectation`);
    return;
  }
  const expectedResult = expected.result;
  if (expectedResult && modelResult.result?.kind !== expectedResult) fail("invalid_oracle", `${entry.id} model result kind disagrees with witness expectation`);
  if (expectedResult && implementationResult.result?.kind !== expectedResult) fail("invalid_oracle", `${entry.id} implementation result kind disagrees with witness expectation`);
  if (expected.value !== undefined && !deepEqual(modelResult.result.value, expected.value)) fail("invalid_oracle", `${entry.id} model result value disagrees with witness expectation`);
  if (expected.value !== undefined && !deepEqual(implementationResult.result.value, expected.value)) fail("invalid_oracle", `${entry.id} implementation result value disagrees with witness expectation`);
  if (expected.error !== undefined && modelResult.result?.error !== expected.error) fail("invalid_oracle", `${entry.id} model failure disagrees with witness expectation`);
  if (expected.error !== undefined && implementationResult.result?.error !== expected.error) fail("invalid_oracle", `${entry.id} implementation failure disagrees with witness expectation`);
  const modelState = object(modelResult.state, `${entry.id}.model.state`);
  const implementationState = object(implementationResult.state, `${entry.id}.implementation.state`);
  const modelObs = modelObservation(modelResult);
  const implementationObs = implementationObservation(implementationResult);
  for (const [field, expectedValue] of [["mutation", true], ["cleanup", true], ["compile_time", true]]) {
    if (expected[field] === expectedValue && modelState[field === "mutation" ? "mutations" : field].length === 0) fail("invalid_oracle", `${entry.id} lacks expected ${field} witness`);
    if (expected[field] === expectedValue && implementationState[field === "mutation" ? "mutations" : field].length === 0) fail("invalid_oracle", `${entry.id} implementation lacks expected ${field} witness`);
  }
  if (expected.consumed_input !== undefined && !deepEqual(modelState.consumed_input, expected.consumed_input)) fail("invalid_oracle", `${entry.id} model input consumption disagrees with witness expectation`);
  if (expected.consumed_input !== undefined && !deepEqual(implementationState.consumed_input, expected.consumed_input)) fail("invalid_oracle", `${entry.id} implementation input consumption disagrees with witness expectation`);
  for (const [label, observation] of [["model", modelObs], ["implementation", implementationObs]]) {
    if (expected.ordered_effects !== undefined && !deepEqual(observation.ordered_effects, expected.ordered_effects)) fail("invalid_oracle", `${entry.id} ${label} effect order disagrees with witness expectation`);
    if (expected.schedule !== undefined && !deepEqual(observation.schedule, expected.schedule)) fail("invalid_oracle", `${entry.id} ${label} schedule disagrees with witness expectation`);
    if (expected.allowed_schedules !== undefined && !deepEqual(observation.allowed_schedules, expected.allowed_schedules)) fail("invalid_oracle", `${entry.id} ${label} allowed schedules disagree with witness expectation`);
    if (expected.resource_premises !== undefined) {
      const premises = object(observation.resource_premises, `${entry.id}.${label}.resource_premises`);
      for (const [field, value] of Object.entries(expected.resource_premises)) {
        if (!deepEqual(premises[field], value)) fail("invalid_oracle", `${entry.id} ${label} resource premise ${field} disagrees with witness expectation`);
      }
    }
  }
  if (expected.resource !== undefined) {
    const expectedResource = object(expected.resource, `${entry.id}.expect.resource`);
    for (const [label, state] of [["model", modelState], ["implementation", implementationState]]) {
      const actual = object(state.resource, `${entry.id}.${label}.state.resource`);
      for (const [field, value] of Object.entries(expectedResource)) {
        if (!deepEqual(actual[field], value)) fail("invalid_oracle", `${entry.id} ${label} resource ${field} disagrees with witness expectation`);
      }
    }
  }
}

function checkWitnessMetadata(witnesses) {
  const fields = array(witnesses.observation_fields, "semantic witness observation_fields");
  if (!deepEqual(fields, OBSERVATION_DIMENSIONS)) {
    fail("invalid_oracle", "semantic witness observation dimensions drifted");
  }
  const identityFields = array(witnesses.identity_fields, "semantic witness identity_fields");
  if (!deepEqual(identityFields, JET_IDENTITY_FIELDS)) fail("invalid_oracle", "semantic witness identity fields drifted");
  const runner = object(witnesses.runner, "semantic witness runner");
  if (runner.launcher !== JET_RUNNER_PATH
      || runner.compiler !== JET_COMPILER_PATH
      || runner.scratch !== "$HOME/.cache/jet-test-scratch"
      || runner.unknown_policy !== "unverified mappings never become comparison evidence; unknown resource observations remain unproved") {
    fail("invalid_oracle", "semantic witness runner policy is missing or drifted");
  }
  text(runner.invocation, "semantic witness runner.invocation");
  if (!deepEqual(array(runner.supported_operations, "semantic witness runner.supported_operations"), ["run", "check"])) {
    fail("invalid_oracle", "semantic witness runner operations drifted");
  }
  if (!deepEqual(array(runner.tiers, "semantic witness runner.tiers"), JET_TIERS)) {
    fail("invalid_oracle", "semantic witness runner tiers drifted");
  }
  if (!Number.isSafeInteger(runner.timeout_ms) || runner.timeout_ms !== JET_EXECUTION_TIMEOUT_MS) {
    fail("invalid_oracle", "semantic witness runner timeout drifted");
  }
  if (runner.scratch.includes("/tmp")) fail("invalid_oracle", "semantic witness runner uses forbidden /tmp scratch");

  const coverage = object(witnesses.coverage, "semantic witness coverage");
  if (coverage.relation !== OBLIGATIONS_PATH) fail("invalid_oracle", "semantic witness coverage relation drifted");
  const coverageStatuses = array(coverage.mapping_statuses, "semantic witness coverage.mapping_statuses");
  if (!deepEqual(coverageStatuses, COVERAGE_STATUSES)) fail("invalid_oracle", "semantic witness coverage statuses drifted");
  text(coverage.qualification_rule, "semantic witness coverage.qualification_rule");

  const controls = array(witnesses.negative_controls, "semantic witness negative_controls");
  if (controls.length === 0) fail("invalid_oracle", "semantic witness corpus has no independent negative controls");
  return { runner, coverage, controls };
}

function checkJetMapping(entry) {
  const id = text(entry.id, "semantic witness.id");
  const jet = object(entry.jet, `${id}.jet`);
  const status = text(jet.status, `${id}.jet.status`);
  if (!JET_EXECUTION_STATUSES.includes(status)) fail("invalid_oracle", `${id}.jet.status is not a recognized execution status`);
  if (jet.source_mode !== "inline") fail("invalid_oracle", `${id}.jet.source_mode must be inline`);
  const entryPath = text(jet.entry, `${id}.jet.entry`);
  if (entryPath !== "main.jet") fail("invalid_oracle", `${id}.jet.entry must be main.jet`);
  const source = text(jet.source, `${id}.jet.source`);
  if (jet.source_sha256 !== digestText(source)) fail("invalid_oracle", `${id}.jet.source_sha256 does not identify its source bytes`);

  if (jet.program_sha256 !== digestText(canonicalJson(entry.program))) {
    fail("invalid_oracle", `${id}.jet.program_sha256 does not identify its semantic program`);
  }
  const runner = object(jet.runner, `${id}.jet.runner`);
  const operation = text(runner.operation, `${id}.jet.runner.operation`);
  if (!["run", "check"].includes(operation)) fail("invalid_oracle", `${id}.jet.runner.operation is unsupported`);
  const expectedTiers = operation === "run" ? ["aot", "jet_run", "interpreter"] : ["check"];
  const tiers = object(runner.tiers, `${id}.jet.runner.tiers`);
  if (!deepEqual(Object.keys(tiers).sort(), [...expectedTiers].sort())) {
    fail("invalid_oracle", `${id}.jet.runner.tiers do not match its operation`);
  }
  const expectedArgs = {
    aot: ["jet", "run", "--release", entryPath],
    jet_run: ["jet", "run", entryPath],
    interpreter: ["jet", "run", "--interpret", entryPath],
    check: ["jet", "check", entryPath],
  };
  const tierDeclarations = {};
  for (const tier of expectedTiers) {
    const declaration = object(tiers[tier], `${id}.jet.runner.tiers.${tier}`);
    if (declaration.applicable !== true) fail("invalid_oracle", `${id}.jet.runner.tiers.${tier} must be applicable`);
    const args = array(declaration.args, `${id}.jet.runner.tiers.${tier}.args`);
    if (args.some((arg) => typeof arg !== "string" || arg.length === 0)
        || !deepEqual(args, expectedArgs[tier])) {
      fail("invalid_oracle", `${id}.jet.runner.tiers.${tier}.args are not canonical`);
    }
    if (!Number.isSafeInteger(declaration.expected_exit)
        || declaration.expected_exit < 0
        || declaration.expected_exit > 255
        || (operation === "check" && declaration.expected_exit !== 1)) {
      fail("invalid_oracle", `${id}.jet.runner.tiers.${tier}.expected_exit is not a canonical exit expectation`);
    }
    tierDeclarations[tier] = {
      applicable: true,
      args: [...args],
      expected_exit: declaration.expected_exit,
    };
  }

  const observation = object(jet.observation, `${id}.jet.observation`);
  if (observation.protocol !== "marker-lines-v1") fail("invalid_oracle", `${id}.jet.observation.protocol is unsupported`);
  text(observation.marker, `${id}.jet.observation.marker`);
  const dimensions = object(observation.dimensions, `${id}.jet.observation.dimensions`);
  if (!deepEqual(Object.keys(dimensions).sort(), [...OBSERVATION_DIMENSIONS].sort())) {
    fail("invalid_oracle", `${id}.jet.observation.dimensions are incomplete`);
  }
  for (const field of OBSERVATION_DIMENSIONS) {
    const declaration = object(dimensions[field], `${id}.jet.observation.dimensions.${field}`);
    const mappingStatus = text(declaration.status, `${id}.jet.observation.dimensions.${field}.status`);
    if (!MAPPING_STATUSES.includes(mappingStatus)) fail("invalid_oracle", `${id}.${field} has an unknown mapping status`);
    if (mappingStatus === "observed") {
      text(declaration.from, `${id}.jet.observation.dimensions.${field}.from`);
      text(declaration.encoding, `${id}.jet.observation.dimensions.${field}.encoding`);
    } else {
      text(declaration.reason, `${id}.jet.observation.dimensions.${field}.reason`);
    }
  }
  return {
    status,
    operation,
    tiers: expectedTiers,
    tier_declarations: tierDeclarations,
    entry: entryPath,
    source,
    source_sha256: jet.source_sha256,
    program_sha256: jet.program_sha256,
    observation,
  };
}

function checkCoverage(obligations, cases) {
  const denominator = new Set(array(obligations.row_ids, "obligation summary.row_ids"));
  if (denominator.size !== obligations.row_count) fail("invalid_oracle", "obligation denominator summary is not unique");
  const references = new Map();
  for (const [index, entry] of cases.entries()) {
    const refs = array(entry.obligations, `semantic witness ${index}.obligations`);
    if (refs.length === 0) fail("invalid_oracle", `${entry.id} has no obligation mapping`);
    const local = new Set();
    for (const reference of refs) {
      text(reference, `${entry.id}.obligations`);
      if (!denominator.has(reference)) fail("invalid_oracle", `${entry.id} maps unknown obligation ${reference}`);
      if (local.has(reference)) fail("invalid_oracle", `${entry.id} maps duplicate obligation ${reference}`);
      local.add(reference);
      const witnessesForRow = references.get(reference) ?? [];
      if (witnessesForRow.includes(entry.id)) fail("invalid_oracle", `${entry.id} maps duplicate obligation ${reference}`);
      witnessesForRow.push(entry.id);
      references.set(reference, witnessesForRow);
    }
  }
  const mappedUnproved = [...denominator].filter((id) => references.has(id)).sort();
  const unmapped = [...denominator].filter((id) => !references.has(id)).sort();
  const rows = [...denominator].sort().map((id) => ({
    id,
    witness: references.get(id) ?? [],
    status: references.has(id) ? "mapped_unproved" : "unmapped",
  }));
  return {
    relation: OBLIGATIONS_PATH,
    denominator: obligations.row_count,
    complete_denominator: true,
    rows,
    status_counts: {
      mapped_unproved: mappedUnproved.length,
      unsupported: 0,
      unmapped: unmapped.length,
      proved: 0,
    },
    mapped_unproved: mappedUnproved,
    unsupported: [],
    unmapped,
    proved: [],
    qualification: "blocked",
  };
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function replaceFirstExpression(value, from, to) {
  if (!value || typeof value !== "object") return false;
  if (!Array.isArray(value) && value.kind === from) {
    value.kind = to;
    return true;
  }
  for (const child of Object.values(value)) {
    if (replaceFirstExpression(child, from, to)) return true;
  }
  return false;
}

function wrongRuleSample(entry, rule) {
  const referenceFirst = evaluateModel(entry.program);
  const referenceSecond = evaluateModel(entry.program);
  if (rule === "numeric.add-as-subtract") {
    const wrongProgram = clone(entry.program);
    if (!replaceFirstExpression(wrongProgram, "add", "sub")) {
      fail("invalid_oracle", `${entry.id} negative control has no add expression to mutate`);
    }
    const candidateFirst = executeImplementation(wrongProgram);
    const candidateSecond = executeImplementation(wrongProgram);
    return comparisonSample(
      `negative:${rule}`,
      entry.program,
      modelObservation(referenceFirst),
      implementationObservation(candidateFirst),
      modelObservation(referenceSecond),
      implementationObservation(candidateSecond),
    );
  }
  if (rule === "cleanup.defer-fifo") {
    const candidateFirst = executeImplementation(entry.program);
    const candidateSecond = executeImplementation(entry.program);
    const candidate = implementationObservation(candidateFirst);
    const candidateReplay = implementationObservation(candidateSecond);
    candidate.cleanup = [...candidate.cleanup].reverse();
    candidateReplay.cleanup = [...candidateReplay.cleanup].reverse();
    return comparisonSample(
      `negative:${rule}`,
      entry.program,
      modelObservation(referenceFirst),
      candidate,
      modelObservation(referenceSecond),
      candidateReplay,
    );
  }
  fail("invalid_oracle", `negative control rule ${rule} has no checker mutation`);
}

function checkNegativeControls(witnesses) {
  const metadata = object(witnesses.metadata, "normalized semantic witness metadata");
  const controls = array(metadata.controls, "semantic witness metadata.controls");
  const cases = new Map(witnesses.cases.map((entry) => [entry.id, entry]));
  const ids = new Set();
  const rows = [];
  for (const [index, control] of controls.entries()) {
    object(control, `semantic negative control ${index}`);
    const id = text(control.id, `semantic negative control ${index}.id`);
    if (ids.has(id)) fail("invalid_oracle", `duplicate semantic negative control ${id}`);
    ids.add(id);
    const caseId = text(control.case_id, `${id}.case_id`);
    const entry = cases.get(caseId);
    if (!entry) fail("invalid_oracle", `${id} names unknown witness ${caseId}`);
    const rule = text(control.rule, `${id}.rule`);
    text(control.mutation, `${id}.mutation`);
    const expectedDifference = text(control.expected_difference, `${id}.expected_difference`);
    if (control.required_status !== "mismatch") {
      fail("invalid_oracle", `${id}.required_status must be mismatch`);
    }
    if (!OBSERVATION_DIMENSIONS.includes(expectedDifference)) {
      fail("invalid_oracle", `${id}.expected_difference is not an observation dimension`);
    }
    const comparison = compareSamples([wrongRuleSample(entry, rule)]);
    if (comparison.status !== "mismatch"
        || comparison.reduced_counterexample?.first_differing_observation !== expectedDifference) {
      fail("invalid_oracle", `${id} did not reject its independent wrong rule`, { comparison });
    }
    rows.push({ id, case_id: caseId, rule, expected_difference: expectedDifference, required_status: control.required_status, status: "rejected", comparison });
  }
  return { count: rows.length, rows };
}
function parseJetMarker(stdout, mapping, caseId) {
  const dimensions = mapping.observation.dimensions;
  const declared = dimensions.result.status === "observed";
  const lines = stdout.split(/\r?\n/u);
  const matches = lines.filter((line) => line.startsWith(mapping.observation.marker));
  if (!declared) return { observed: {}, marker_count: matches.length };
  if (matches.length !== 1) {
    return { error: `${caseId} emitted ${matches.length} ${mapping.observation.marker} marker lines; exactly one is required` };
  }
  const payload = matches[0].slice(mapping.observation.marker.length);
  let value;
  try {
    if (dimensions.result.encoding === "json") value = JSON.parse(payload);
    else if (dimensions.result.encoding === "text") value = payload;
    else return { error: `${caseId} uses unsupported result encoding ${dimensions.result.encoding}` };
  } catch (error) {
    return { error: `${caseId} emitted an invalid ${dimensions.result.encoding} result marker: ${error.message}` };
  }
  const result = value && typeof value === "object" && ["value", "failure"].includes(value.kind)
    ? value
    : { kind: "value", value };
  return { observed: { result }, marker_count: matches.length };
}
function executeJetMappings(witnesses, { execute = true } = {}) {
  const metadata = object(witnesses.metadata, "normalized semantic witness metadata");
  const runner = object(metadata.runner, "normalized semantic witness runner");
  const runnerPath = runner.launcher;
  const compilerPath = runner.compiler;
  const timeoutMs = runner.timeout_ms;
  const cases = witnesses.cases;
  const mappings = witnesses.mappings;
  if (!execute) {
    return {
      status: "unverified",
      requested: false,
      runner: runnerPath,
      reason: "live Jet mapping execution was skipped; mappings remain unverified",
      cases: cases.map((entry) => ({ id: entry.id, status: "unverified", skipped: true })),
    };
  }
  const compiler = absolute(compilerPath);
  let compilerAvailable = existsSync(compiler);
  if (compilerAvailable) {
    try {
      accessSync(compiler, fsConstants.X_OK);
    } catch {
      compilerAvailable = false;
    }
  }
  if (!compilerAvailable) {
    return {
      status: "unavailable",
      requested: true,
      runner: runnerPath,
      reason: "target/debug/jet is missing or not executable; no package fallback is accepted as fresh compiler evidence",
      cases: cases.map((entry) => ({ id: entry.id, status: "unavailable", reason: "fresh local compiler unavailable" })),
    };
  }
  const configuredScratch = process.env[JET_SCRATCH_ENV]
    ?? runner.scratch.replace("$HOME", homedir());
  const scratchRoot = resolve(configuredScratch);
  const cacheRoot = resolve(homedir(), ".cache");
  if (scratchRoot === "/tmp"
      || scratchRoot.startsWith("/tmp/")
      || (scratchRoot !== cacheRoot && !scratchRoot.startsWith(`${cacheRoot}/`))) {
    return {
      status: "unavailable",
      requested: true,
      runner: runnerPath,
      reason: `${JET_SCRATCH_ENV} must resolve under $HOME/.cache and never /tmp`,
      cases: cases.map((entry) => ({ id: entry.id, status: "unavailable", reason: "scratch path is outside $HOME/.cache" })),
    };
  }
  mkdirSync(scratchRoot, { recursive: true });
  const env = {
    ...process.env,
    [JET_SCRATCH_ENV]: scratchRoot,
    LC_ALL: "C",
    LANG: "C",
    NO_COLOR: "1",
    CLICOLOR: "0",
  };
  const results = [];
  let hasFailure = false;
  let hasUnavailable = false;
  for (const [index, entry] of cases.entries()) {
    const mapping = mappings[index];
    const caseRoot = mkdtempSync(join(scratchRoot, `semantic-${entry.id}-`));
    const sourcePath = join(caseRoot, mapping.entry);
    writeFileSync(sourcePath, mapping.source, "utf8");
    const tiers = [];
    try {
      for (const tier of mapping.tiers) {
        const declaration = mapping.tier_declarations[tier];
        const child = spawnSync(absolute(runnerPath), ["full", ...declaration.args], {
          cwd: caseRoot,
          env,
          encoding: "utf8",
          timeout: timeoutMs,
          maxBuffer: 4 * 1024 * 1024,
        });
        const output = `${child.stdout ?? ""}`;
        const errorOutput = `${child.stderr ?? ""}`;
        const marker = mapping.operation === "run"
          ? parseJetMarker(output, mapping, entry.id)
          : { observed: {}, marker_count: 0 };
        const processError = child.error;
        const processFailed = Boolean(processError)
          || child.status !== declaration.expected_exit;
        const tierResult = {
          observations: marker.observed,
          tier,
          command: [runnerPath, "full", ...declaration.args],
          status: processFailed ? "failed" : (marker.error ? "failed" : "observed"),
          exit_code: Number.isInteger(child.status) ? child.status : null,
          signal: child.signal ?? null,
          timed_out: processError?.code === "ETIMEDOUT",
          stdout_sha256: digestText(output),
          stderr_sha256: digestText(errorOutput),
          observed_dimensions: Object.keys(marker.observed ?? {}),
          unknown_dimensions: OBSERVATION_DIMENSIONS.filter((field) => !Object.hasOwn(marker.observed ?? {}, field)),
          ...(marker.error ? { reason: marker.error } : {}),
          ...(processError ? { reason: processError.message, error_code: processError.code ?? null } : {}),
          ...(processFailed && !marker.error && !processError ? { reason: `Jet operation exited with status ${String(child.status)}; expected ${String(declaration.expected_exit)}` } : {}),
        };
        if (tierResult.status === "failed") hasFailure = true;
        if (processError?.code !== undefined && processError.code !== "ETIMEDOUT") hasUnavailable = true;
        tiers.push(tierResult);
      }
    } finally {
      rmSync(caseRoot, { recursive: true, force: true });
    }
    const unknown = [...new Set(tiers.flatMap((tier) => tier.unknown_dimensions))].sort();
    const status = tiers.some((tier) => tier.status === "failed")
      ? "failed"
      : (tiers.every((tier) => tier.status === "observed") && unknown.length === 0 ? "observed" : "unverified");
    results.push({
      id: entry.id,
      status,
      operation: mapping.operation,
      tiers,
      observed_dimensions: [...new Set(tiers.flatMap((tier) => tier.observed_dimensions))].sort(),
      unknown_dimensions: unknown,
    });
  }
  return {
    status: hasFailure ? "failed" : (hasUnavailable ? "unavailable" : "unverified"),
    requested: true,
    runner: runnerPath,
    scratch: scratchRoot,
    compiler,
    qualification: "blocked",
    cases: results,
  };
}


function checkWitnesses() {
  const witnesses = readJson(WITNESSES_PATH, "semantic witness corpus");
  if (witnesses.schema !== "jet.semantic-witnesses.v1" || witnesses.schema_version !== 1 || witnesses.relation !== OBSERVATION_RELATION) {
    fail("invalid_oracle", "semantic witness schema/version/relation is unsupported");
  }
  const metadata = checkWitnessMetadata(witnesses);
  const cases = array(witnesses.cases, "semantic witness cases");
  if (cases.length === 0) fail("invalid_oracle", "semantic witness cases are empty");
  const ids = new Set();
  const samples = [];
  const categories = new Set();
  const mappings = [];
  for (const [index, entry] of cases.entries()) {
    object(entry, `semantic witness ${index}`);
    const id = text(entry.id, `semantic witness ${index}.id`);
    if (ids.has(id)) fail("invalid_oracle", `duplicate semantic witness ${id}`);
    ids.add(id);
    categories.add(text(entry.category, `${id}.category`));
    object(entry.program, `${id}.program`);
    mappings.push(checkJetMapping(entry));
    let modelFirst;
    let implementationFirst;
    try {
      modelFirst = evaluateModel(entry.program);
      implementationFirst = executeImplementation(entry.program);
    } catch (error) {
      fail("invalid_oracle", `witness ${id} raised an unclassifiable evaluator error: ${error.message}`);
    }
    expectedObservationCheck(entry, modelFirst, implementationFirst);
    const modelSecond = evaluateModel(entry.program);
    const implementationSecond = executeImplementation(entry.program);
    samples.push(comparisonSample(
      id,
      entry.program,
      modelObservation(modelFirst),
      implementationObservation(implementationFirst),
      modelObservation(modelSecond),
      implementationObservation(implementationSecond),
    ));
  }
  for (const category of ["result", "mutation", "compile_time", "rejection", "failure"]) {
    if (!categories.has(category)) fail("invalid_oracle", `semantic witness corpus lacks ${category} coverage`);
  }
  const comparison = compareSamples(samples);
  if (comparison.status !== "matched") fail(comparison.status, `semantic witness comparison is ${comparison.status}`, { comparison });
  return {
    comparison,
    categories: [...categories].sort(),
    count: samples.length,
    cases,
    metadata,
    mappings,
  };
}

function nonProofRecord(status, reason) {
  assertStatus(status, "non-proof control");
  return baseRecord([], status, reason);
}
function unknownStatusRecord() {
  try {
    assertStatus("unknown", "hostile comparison status");
  } catch (error) {
    if (error instanceof CheckError && error.code === "invalid_oracle") {
      return nonProofRecord("invalid_oracle", "unknown comparison status is invalid_oracle, not proof");
    }
    throw error;
  }
  fail("invalid_oracle", "unknown comparison status was accepted");
}


function checkFalseClaims(comparison) {
  const wrong = JSON.parse(JSON.stringify(comparison.samples[0]));
  wrong.candidate = { ...wrong.candidate, raw: canonicalJson({ deliberately_wrong: true }), ordered_effects: [] };
  wrong.candidate_replay = JSON.parse(JSON.stringify(wrong.candidate));
  const mismatch = compareSamples([wrong]);
  if (mismatch.status !== "mismatch" || mismatch.universal_proof !== false) fail("invalid_oracle", "deliberately false observation was not rejected");
  const controls = {
    false_claim: mismatch,
    missing_correspondence: nonProofRecord("unsupported", "missing implementation correspondence is not proof"),
    unknown_status: unknownStatusRecord(),
    empty: nonProofRecord("empty", "empty corpus is not proof"),
    unsupported: nonProofRecord("unsupported", "unsupported construct is not proof"),
    unavailable: nonProofRecord("unavailable", "unavailable checker is not proof"),
    timeout: nonProofRecord("timeout", "timed out checker is not proof"),
    cancelled: nonProofRecord("cancelled", "cancelled checker is not proof"),
    invalid_oracle: compareSamples(comparison.samples.slice(0, 1), 1),
    contaminated: (() => {
      const sample = JSON.parse(JSON.stringify(comparison.samples[0]));
      sample.candidate_replay = { ...sample.candidate, raw: canonicalJson({ changed_on_replay: true }) };
      return compareSamples([sample]);
    })(),
  };
  for (const [name, record] of Object.entries(controls)) {
    assertStatus(record.status, `control ${name}`);
    if (record.status === "matched") fail("invalid_oracle", `non-proof control ${name} was incorrectly matched`);
    if (record.universal_proof !== false) fail("invalid_oracle", `non-proof control ${name} claimed universal proof`);
  }
  return controls;
}

function unavailableReplay() {
  return {
    requested: false,
    status: "unavailable",
    compiler_status: null,
    reason: "live pinned checker replay was not requested; this is not downgraded to tested evidence",
  };
}

function replayPinnedProof() {
  const child = spawnSync(process.execPath, [absolute(PROOF_TOOL_PATH), "--replay", "--json"], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: 120000,
    env: { ...process.env, LC_ALL: "C", LANG: "C" },
  });
  if (child.error?.code === "ETIMEDOUT") {
    return { requested: true, status: "timeout", compiler_status: null, reason: "pinned checker replay timed out" };
  }
  if (child.error) {
    return { requested: true, status: "unavailable", compiler_status: null, reason: `pinned checker could not start: ${child.error.message}` };
  }
  let payload;
  try {
    payload = JSON.parse(child.stdout);
  } catch (error) {
    return { requested: true, status: "invalid_oracle", compiler_status: null, reason: `pinned checker returned non-JSON output: ${error.message}`, stderr: child.stderr };
  }
  if (!payload || payload.schema !== "jet.compiler-proof.v1") {
    return { requested: true, status: "invalid_oracle", compiler_status: payload?.status ?? null, reason: "pinned checker returned an unexpected schema" };
  }
  if (payload.status === "proved") {
    return { requested: true, status: "matched", compiler_status: "proved", reason: "pinned checker replayed the retained artifact; semantic qualification remains bounded" };
  }
  if (payload.status === "unavailable") return { requested: true, status: "unavailable", compiler_status: payload.status, reason: payload.error?.message ?? "pinned checker unavailable" };
  if (payload.status === "timeout") return { requested: true, status: "timeout", compiler_status: payload.status, reason: payload.error?.message ?? "pinned checker timed out" };
  return { requested: true, status: "invalid_oracle", compiler_status: payload.status ?? null, reason: payload.error?.message ?? "pinned checker did not establish proof" };
}

async function runCheck({ replay = false, executeJet = true } = {}) {
  const contract = readJson(CONTRACT_PATH, "semantic contract");
  const checked = checkContract(contract);
  const identity = await checkProofIdentity(checked.identity);
  const obligations = checkObligations();
  const witnesses = checkWitnesses();
  const coverage = checkCoverage(obligations, witnesses.cases);
  const controls = checkFalseClaims(witnesses.comparison);
  const negativeControls = checkNegativeControls(witnesses);
  const jetExecution = executeJetMappings(witnesses, { execute: executeJet });
  if (!JET_EXECUTION_STATUSES.includes(jetExecution.status)) {
    fail("invalid_oracle", `Jet mapping execution returned an unknown status ${String(jetExecution.status)}`);
  }
  const proofReplay = replay ? replayPinnedProof() : unavailableReplay();
  assertStatus(proofReplay.status, "proof replay");
  if (replay && proofReplay.status !== "matched") {
    fail(proofReplay.status, `pinned proof replay did not establish a checked result: ${proofReplay.reason}`, { proof_replay: proofReplay });
  }
  return {
    schema: CHECK_SCHEMA,
    status: witnesses.comparison.status,
    model: {
      id: MODEL_ID,
      schema: contract.schema,
      schema_version: contract.schema_version,
      relation: OBSERVATION_RELATION,
      bounds: { max_steps: MAX_STEPS, max_fuel: MAX_FUEL },
      observation_alphabet: contract.claim.observation_alphabet,
    },
    binding_methods: canonical(contract.binding_methods),
    identity,
    obligations,
    coverage,
    witnesses: {
      count: witnesses.count,
      categories: witnesses.categories,
      negative_control_count: negativeControls.count,
      mapping_declarations: witnesses.mappings.map((mapping) => ({
        status: mapping.status,
        operation: mapping.operation,
        tiers: mapping.tiers,
        entry: mapping.entry,
        source_sha256: mapping.source_sha256,
        program_sha256: mapping.program_sha256,
        observation: mapping.observation,
      })),
    },
    comparison: witnesses.comparison,
    controls: {
      ...controls,
      independent_wrong_rules: negativeControls,
    },
    jet_execution: jetExecution,
    proof_boundary: {
      ...canonical(checked.boundary),
      comparison_status: witnesses.comparison.status,
      universal_proof: false,
      coverage_qualification: coverage.qualification,
      jet_mapping_status: jetExecution.status,
      proof_replay: proofReplay,
    },
  };
}

function errorPayload(error) {
  const code = error instanceof CheckError ? error.code : "invalid_oracle";
  const status = STATUS_SET.has(code) ? code : "invalid_oracle";
  return {
    schema: CHECK_SCHEMA,
    status,
    error: {
      code,
      message: error?.message ?? String(error),
      ...(error?.details === undefined ? {} : { details: error.details }),
    },
  };
}
function printHuman(result) {
  process.stdout.write(`semantic contract: ${result.status}\n`);
  if (result.identity) process.stdout.write(`candidate: ${result.identity.claim_id} ${result.identity.identity_sha256}\n`);
  if (result.obligations) process.stdout.write(`obligations: ${result.obligations.row_count} rows; all explicitly uncovered with live owners\n`);
  if (result.coverage) process.stdout.write(`coverage: ${result.coverage.denominator} denominator rows; mapped_unproved=${result.coverage.status_counts.mapped_unproved}; unmapped=${result.coverage.status_counts.unmapped}\n`);
  if (result.comparison) process.stdout.write(`comparison: ${result.comparison.samples.length} samples; universal_proof=${String(result.comparison.universal_proof)}\n`);
  if (result.jet_execution) process.stdout.write(`jet-mappings: status=${result.jet_execution.status}; requested=${String(result.jet_execution.requested)}\n`);
  if (result.proof_boundary) {
    process.stdout.write(`proof-boundary: qualification=${result.proof_boundary.qualification}; proven=${result.proof_boundary.proven.length}; assumed=${result.proof_boundary.assumed.length}; tested=${result.proof_boundary.tested.length}; blocked=${result.proof_boundary.blocked.length}\n`);
  }
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}

export async function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const replay = argv.includes("--replay");
  const skipJet = argv.includes("--skip-jet");
  if (argv.some((arg) => !["--json", "--replay", "--skip-jet"].includes(arg))) {
    const result = errorPayload(new CheckError("invalid_oracle", "usage: check.mjs [--replay] [--skip-jet] [--json]"));
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 64;
  }
  try {
    const result = await runCheck({ replay, executeJet: !skipJet });
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 0;
  } catch (error) {
    const result = errorPayload(error);
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await main();
}

export {
  COMPARISON_STATUSES,
  compareSamples,
  checkContract,
  checkObligations,
  checkCoverage,
  checkNegativeControls,
  validateObligationRecord,
  checkWitnesses,
  runCheck,
};
