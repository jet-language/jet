#!/usr/bin/env node

/**
 * Checking-boundary checker for #2937.
 *
 * This module is deliberately a consumer of the production compiler seam. It
 * does not parse Jet, infer a second type system, or manufacture fact and
 * derivation records. It validates retained declarations and compares them
 * with `jet inspect compiler check` output. Every finite observation remains
 * proof-blocking.
 */
import {
  accessSync,
  constants as fsConstants,
  existsSync,
  lstatSync,
  readFileSync,
} from "node:fs";
import { dirname, isAbsolute, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  canonical,
  canonicalJson,
  digestBytes,
  digestText,
  identityDigest,
  validateManifest,
} from "../../../scripts/agent/compiler-proof.mjs";
import { buildLedger } from "../../../scripts/agent/check-core-surface-ledger.mjs";
import { buildObligations } from "../semantics/generate-obligations.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const CONTRACT_PATH = "proof/compiler/checking/contract.json";
const WITNESSES_PATH = "proof/compiler/checking/witnesses.json";
const OBLIGATIONS_PATH = "proof/compiler/obligations.json";
const PROOF_MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const RUNNER_PATH = "scripts/agent/jet-env";
const CHECKER_PATH = "proof/compiler/checking/check.mjs";
const CHECK_SCHEMA = "jet.checking-contract-check.v1";
const TIMEOUT_MS = 120000;
const COMPILER_SCHEMA = "inspect.compiler.check";
const COMPILER_SCHEMA_VERSION = 1;
const COMPILER_API_VERSION = 1;
const SEMANTIC_INDEX_SCHEMA_VERSION = 18;
const CHECKING_BOUNDARIES = Object.freeze([
  "boundary:names",
  "boundary:types",
  "boundary:ownership",
  "boundary:effects",
]);
const CHECKING_CERTIFICATES = Object.freeze([
  "checked_compiler_value",
  "exact_source_digest",
  "definition_and_reference_anchors",
  "typed_callable_signatures",
  "ownership_view_provenance",
  "effect_call_paths_and_spans",
  "state_graph_presence",
  "canonical_fact_kinds",
  "checked_derivation_records",
  "registered_rejection",
  "forged_record_rejection",
  "construction_identity",
]);
const STATUS_SET = new Set([
  "matched",
  "mismatch",
  "empty",
  "unsupported",
  "unavailable",
  "timeout",
  "cancelled",
  "invalid_oracle",
  "contaminated",
  "unknown",
  "unverified",
]);
const DERIVATION_METHODS = new Set([
  "static_derivation",
  "formal_proof",
  "recorded_execution",
  "sampled_agreement",
  "external_assumption",
]);
const DERIVATION_DISPOSITIONS = new Set([
  "current",
  "stale",
  "expired",
  "redacted",
  "unavailable",
  "unsupported",
  "budget_exhausted",
  "unknown",
]);
const DIAGNOSTIC_CODE = /^E[0-9]{4}$/u;
const DIGEST = /^sha256-[0-9a-f]{64}$/u;

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
  if (typeof value !== "string" || value.length === 0 || /[\u0000-\u001f\u007f]/u.test(value)) {
    fail("invalid_oracle", `${label} must be non-empty text`);
  }
  return value;
}

function bool(value, label) {
  if (typeof value !== "boolean") fail("invalid_oracle", `${label} must be boolean`);
  return value;
}

function integer(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) fail("invalid_oracle", `${label} must be a non-negative safe integer`);
  return value;
}

function digest(value, label) {
  text(value, label);
  if (!DIGEST.test(value)) fail("invalid_oracle", `${label} must be a lowercase SHA-256 digest`);
  return value;
}

function projectPath(value, label) {
  text(value, label);
  if (isAbsolute(value) || value.includes("\\") || value.includes("\0")) {
    fail("invalid_oracle", `${label} must be a project-relative path`);
  }
  const target = resolve(ROOT, value);
  const prefix = `${ROOT}${sep}`;
  if (target !== ROOT && !target.startsWith(prefix)) fail("invalid_oracle", `${label} escapes the project root`);
  return target;
}

function regularFile(path, label) {
  const target = projectPath(path, label);
  let stat;
  try {
    stat = lstatSync(target);
  } catch (error) {
    fail("unavailable", `${label} is missing: ${path}`, { path, error: error.message });
  }
  if (stat.isSymbolicLink() || !stat.isFile()) fail("invalid_oracle", `${label} is not a regular file: ${path}`);
  return target;
}

function readJson(path, label) {
  const target = projectPath(path, label);
  if (!existsSync(target)) fail("unavailable", `missing ${label}: ${path}`, { path });
  try {
    return JSON.parse(readFileSync(target, "utf8"));
  } catch (error) {
    fail("invalid_oracle", `invalid JSON in ${label}: ${error.message}`, { path });
  }
}

function digestFile(path) {
  return digestBytes(readFileSync(path));
}

function digestRef(ref, label) {
  object(ref, label);
  const path = text(ref.path, `${label}.path`);
  const target = regularFile(path, label);
  const expected = digest(ref.sha256, `${label}.sha256`);
  const actual = digestFile(target);
  if (actual !== expected) fail("mismatch", `${label} digest changed`, { path, expected, actual });
  return { path, sha256: actual };
}

function digestCanonical(value) {
  return digestText(canonicalJson(value));
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function withoutBindingDigests(witnesses) {
  const projected = clone(witnesses);
  if (projected.binding && typeof projected.binding === "object") {
    delete projected.binding.contract_sha256;
    delete projected.binding.manifest_sha256;
  }
  return projected;
}

function checkContract(contract) {
  object(contract, "checking contract");
  if (contract.schema !== "jet.checking-contract.v1" || contract.schema_version !== 1) {
    fail("invalid_oracle", "checking contract schema/version is unsupported");
  }
  text(contract.contract_id, "contract.contract_id");
  const policy = object(contract.policy, "contract.policy");
  if (policy.decision_id !== "D-COMPILER-CHECKING-PROOF1" || policy.outcome !== "A" || policy.status !== "ratified") {
    fail("invalid_oracle", "checking contract does not record ratified checking outcome A");
  }
  text(policy.scope, "contract.policy.scope");
  text(policy.foreign_suffix, "contract.policy.foreign_suffix");

  const claim = object(contract.claim, "contract.claim");
  text(claim.kind, "contract.claim.kind");
  text(claim.statement, "contract.claim.statement");
  for (const field of ["preconditions", "observation_alphabet", "unsupported_cases"]) {
    const values = array(claim[field], `contract.claim.${field}`);
    if (values.length === 0 || values.some((value) => typeof value !== "string" || value.length === 0)) {
      fail("invalid_oracle", `contract.claim.${field} must contain named entries`);
    }
  }
  const requiredObservations = [
    "canonical_fact_registry",
    "checked_derivations",
    "checked_names",
    "checked_types",
    "construction_identity",
    "diagnostics",
    "effect_facts_and_provenance",
    "ownership_and_views",
    "registered_diagnostics",
    "source_spans",
    "state_transitions",
  ];
  for (const observation of requiredObservations) {
    if (!claim.observation_alphabet.includes(observation)) {
      fail("invalid_oracle", `checking contract omits observation ${observation}`);
    }
  }

  const execution = object(contract.execution, "contract.execution");
  if (execution.schema !== "jet.compiler-inspection.v1" || execution.operation !== "check") {
    fail("invalid_oracle", "checking contract does not bind the compiler check operation");
  }
  if (execution.runner !== RUNNER_PATH || execution.compiler !== "target/debug/jet") {
    fail("invalid_oracle", "checking contract compiler runner changed");
  }
  integer(execution.timeout_ms, "contract.execution.timeout_ms");
  text(execution.command, "contract.execution.command");
  text(execution.unknown_status, "contract.execution.unknown_status");
  text(execution.crash_policy, "contract.execution.crash_policy");

  const methods = array(contract.binding_methods, "contract.binding_methods");
  const methodNames = new Set();
  for (const [index, entry] of methods.entries()) {
    object(entry, `contract.binding_methods[${index}]`);
    const method = text(entry.method, `contract.binding_methods[${index}].method`);
    if (methodNames.has(method)) fail("invalid_oracle", `duplicate checking binding method ${method}`);
    methodNames.add(method);
    if (!new Set(["verified_construction", "direct_verification", "translation_certificate"]).has(method)) {
      fail("invalid_oracle", `unknown checking binding method ${method}`);
    }
    for (const field of ["source", "implementation", "checker", "limit"]) text(entry[field], `contract.binding_methods[${index}].${field}`);
  }
  if (methodNames.size !== 3) fail("invalid_oracle", "checking contract must retain all three binding methods");

  const identity = object(contract.identity, "contract.identity");
  if (identity.schema !== "jet.compiler-proof.v1" || identity.claim_id !== "compiler-proof.identity-direct") {
    fail("invalid_oracle", "checking contract identity does not select the pinned compiler-proof claim");
  }
  text(identity.manifest_path, "contract.identity.manifest_path");
  if (identity.authority_key !== "compiler-proof.checking") {
    fail("invalid_oracle", "checking contract identity does not select the approved checking authority");
  }
  text(identity.authority_key, "contract.identity.authority_key");
  text(identity.identity_digest_source, "contract.identity.identity_digest_source");
  text(identity.driver_path, "contract.identity.driver_path");
  const identityFields = array(identity.required_fields, "contract.identity.required_fields");
  if (identityFields.length < 10) fail("invalid_oracle", "checking contract identity fields are incomplete");

  const coverage = object(contract.coverage, "contract.coverage");
  if (coverage.relation_path !== OBLIGATIONS_PATH || coverage.owner !== "#2937") {
    fail("invalid_oracle", "checking contract coverage relation or owner changed");
  }
  text(coverage.generator, "contract.coverage.generator");
  text(coverage.qualification_rule, "contract.coverage.qualification_rule");
  const covered = array(coverage.obligations, "contract.coverage.obligations");
  if (covered.length !== CHECKING_BOUNDARIES.length) fail("invalid_oracle", "checking coverage must name four canonical boundaries");
  const coverageIds = covered.map((entry, index) => {
    object(entry, `contract.coverage.obligations[${index}]`);
    for (const field of ["id", "rule", "subject"]) text(entry[field], `contract.coverage.obligations[${index}].${field}`);
    return entry.id;
  });
  if (canonicalJson(coverageIds.slice().sort()) !== canonicalJson(CHECKING_BOUNDARIES.slice().sort())) {
    fail("invalid_oracle", "checking coverage does not name the canonical boundary rows");
  }

  const facts = object(contract.facts, "contract.facts");
  for (const field of ["source", "registry", "enum", "derivation_record", "derivation_method", "derivation_disposition", "canonical_rule"]) {
    text(facts[field], `contract.facts.${field}`);
  }
  const core = object(contract.core, "contract.core");
  for (const field of ["ledger", "source_of_truth", "checker", "authority_rule"]) text(core[field], `contract.core.${field}`);
  const certificates = array(contract.certificates, "contract.certificates");
  for (const certificate of CHECKING_CERTIFICATES) {
    if (!certificates.includes(certificate)) fail("invalid_oracle", `checking contract omits certificate ${certificate}`);
  }
  const boundary = object(contract.boundary, "contract.boundary");
  if (boundary.qualification !== "blocked") fail("invalid_oracle", "checking contract boundary must remain blocked");
  for (const field of ["proven", "assumed", "tested", "blocked"]) {
    const values = array(boundary[field], `contract.boundary.${field}`);
    if (values.length === 0 || values.some((value) => typeof value !== "string" || value.length === 0)) {
      fail("invalid_oracle", `contract.boundary.${field} must contain named entries`);
    }
  }
  return contract;
}

function checkProofIdentity(contractIdentity) {
  const manifest = readJson(PROOF_MANIFEST_PATH, "proof manifest");
  let summary;
  try {
    summary = validateManifest(manifest);
  } catch (error) {
    fail("invalid_oracle", `proof manifest validation failed: ${error.message}`);
  }
  if (manifest.schema !== "jet.compiler-proof.v1") fail("invalid_oracle", "proof manifest schema changed");
  const claim = array(manifest.claims, "proof manifest claims").find((entry) => entry?.id === contractIdentity.claim_id);
  if (!claim) fail("invalid_oracle", `proof claim ${contractIdentity.claim_id} is missing`);
  const expectedIdentity = identityDigest(claim);
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
  if (!new Set(["verified_construction", "direct_verification", "translation_certificate"]).has(text(correspondence.method, "proof claim correspondence.method"))) {
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
    candidate: canonical(candidate),
    correspondence: canonical(correspondence),
    assumptions: canonical(assumptions),
    checker: canonical(checker),
    toolchain_pin_sha256: summary.pin,
  };
}

function checkLedger(contract) {
  const ledger = buildLedger();
  if (ledger.schemaVersion !== 2) {
    fail("invalid_oracle", "Core surface ledger schema is not 2");
  }
  const files = array(ledger.sourceFiles, "Core surface ledger sourceFiles");
  if (files.length === 0) fail("invalid_oracle", "Core surface ledger has no source files");
  for (const [index, entry] of files.entries()) {
    object(entry, `Core surface ledger sourceFiles[${index}]`);
    text(entry.path, `Core surface ledger sourceFiles[${index}].path`);
    digest(entry.sha256, `Core surface ledger sourceFiles[${index}].sha256`);
    integer(entry.lineCount, `Core surface ledger sourceFiles[${index}].lineCount`);
  }
  return {
    path: contract.core.ledger,
    schema_version: ledger.schemaVersion,
    source_of_truth: contract.core.source_of_truth,
    source_file_count: files.length,
  };
}

function checkCanonicalFacts(contract, witnesses) {
  const sourcePath = regularFile(contract.facts.source, "canonical Facts source");
  const source = readFileSync(sourcePath, "utf8");
  const enumMatch = source.match(/pub\s+enum\s+FactKind\s*\{([\s\S]*?)\n\}/u);
  if (!enumMatch) fail("invalid_oracle", "canonical Facts source does not expose FactKind");
  const canonicalKinds = [...enumMatch[1].matchAll(/^\s*([A-Z][A-Za-z0-9_]*)\s*,/gmu)].map((match) => match[1]);
  const controls = object(witnesses.fact_controls, "checking witness fact_controls");
  const allowedKinds = array(controls.allowed_kinds, "checking witness fact_controls.allowed_kinds");
  if (canonicalJson(canonicalKinds.slice().sort()) !== canonicalJson(allowedKinds.slice().sort())) {
    fail("invalid_oracle", "witness fact kinds do not match canonical FactKind");
  }
  const allowedValidity = array(controls.allowed_validity, "checking witness fact_controls.allowed_validity");
  const allowedMethods = array(controls.allowed_derivation_methods, "checking witness fact_controls.allowed_derivation_methods");
  const allowedDispositions = array(controls.allowed_dispositions, "checking witness fact_controls.allowed_dispositions");
  for (const value of allowedMethods) {
    if (!DERIVATION_METHODS.has(value) || !source.includes(`\"${value}\"`)) fail("invalid_oracle", `derivation method is not owned by Facts.rs: ${value}`);
  }
  for (const value of [...new Set([...allowedValidity, ...allowedDispositions])]) {
    if (!DERIVATION_DISPOSITIONS.has(value) || !source.includes(`\"${value}\"`)) fail("invalid_oracle", `derivation disposition is not owned by Facts.rs: ${value}`);
  }
  return { source: contract.facts.source, enum: contract.facts.enum, kinds: canonicalKinds };
}

function checkBinding(contract, witnesses) {
  object(witnesses.binding, "checking witness binding");
  digest(witnesses.binding.contract_sha256, "checking witness binding.contract_sha256");
  digest(witnesses.binding.manifest_sha256, "checking witness binding.manifest_sha256");
  const contractPath = regularFile(CONTRACT_PATH, "checking contract");
  const contractDigest = digestFile(contractPath);
  if (witnesses.binding.contract_sha256 !== contractDigest) {
    fail("mismatch", "checking witness contract binding changed", { expected: witnesses.binding.contract_sha256, actual: contractDigest });
  }
  const projectedDigest = digestCanonical(withoutBindingDigests(witnesses));
  if (witnesses.binding.manifest_sha256 !== projectedDigest) {
    fail("mismatch", "checking witness manifest binding changed", { expected: witnesses.binding.manifest_sha256, actual: projectedDigest });
  }
  const checker = digestRef(witnesses.binding.checker, "checking witness binding.checker");
  if (checker.path !== CHECKER_PATH) fail("invalid_oracle", "checking witness binds a different checker");
  const driver = digestRef(witnesses.binding.driver, "checking witness binding.driver");
  if (driver.path !== contract.identity.driver_path) fail("invalid_oracle", "checking witness binds a different driver");
  const obligationRef = digestRef(witnesses.binding.obligations, "checking witness binding.obligations");
  if (obligationRef.path !== OBLIGATIONS_PATH) fail("invalid_oracle", "checking witness binds a different obligation relation");
  const authorities = array(witnesses.binding.authorities, "checking witness binding.authorities");
  if (authorities.length === 0) fail("invalid_oracle", "checking witness has no authority bindings");
  for (const [index, ref] of authorities.entries()) digestRef(ref, `checking witness binding.authorities[${index}]`);
  const implementation = array(witnesses.binding.implementation, "checking witness binding.implementation");
  if (implementation.length === 0) fail("invalid_oracle", "checking witness has no implementation bindings");
  for (const [index, ref] of implementation.entries()) digestRef(ref, `checking witness binding.implementation[${index}]`);
  return {
    checker,
    driver,
    obligations: obligationRef,
    authority_count: authorities.length,
    implementation_file_count: implementation.length,
    obligations_sha256: obligationRef.sha256,
  };
}

function checkObligations(contract) {
  const recorded = readJson(OBLIGATIONS_PATH, "obligation relation");
  let expected;
  try {
    expected = buildObligations();
  } catch (error) {
    fail("unavailable", `canonical obligation generator unavailable: ${error.message}`);
  }
  if (canonicalJson(recorded) !== canonicalJson(expected)) {
    fail("invalid_oracle", "obligation relation is stale; regenerate it from current authorities", {
      command: "scripts/agent/jet-env node proof/compiler/semantics/generate-obligations.mjs",
      recorded_rows: recorded.row_count,
      expected_rows: expected.row_count,
    });
  }
  const rows = array(recorded.rows, "obligation relation rows");
  if (recorded.schema !== "jet.compiler-obligations.v1" || recorded.schema_version !== 1 || recorded.row_count !== rows.length) {
    fail("invalid_oracle", "obligation relation schema/version/count is unsupported");
  }
  const selected = [];
  for (const id of CHECKING_BOUNDARIES) {
    const row = rows.find((candidate) => candidate.id === id);
    if (!row) fail("invalid_oracle", `missing checking obligation ${id}`);
    if (row.family !== "checking" || row.owner !== "#2937" || row.obligation_state !== "uncovered") {
      fail("invalid_oracle", `checking obligation ${id} is not an explicit #2937 uncovered row`);
    }
    selected.push({
      id: row.id,
      family: row.family,
      kind: row.kind,
      subject: row.subject,
      rule: row.rule,
      owner: row.owner,
      source: row.source,
      obligation_state: row.obligation_state,
      disposition: "mapped_unproved",
      qualification: "blocked",
    });
  }
  return {
    relation_sha256: digestFile(projectPath(OBLIGATIONS_PATH, "obligation relation")),
    relation_row_count: rows.length,
    denominator: selected.length,
    rows: selected,
    dispositions: selected.reduce((result, row) => {
      result[row.id] = row.disposition;
      return result;
    }, {}),
    qualification: "blocked",
  };
}

function byteSlice(source, start, end) {
  return Buffer.from(source, "utf8").subarray(start, end).toString("utf8");
}

function checkSpan(span, source, label, required = false) {
  if (span === null || span === undefined) {
    if (required) fail("mismatch", `${label} is missing`);
    return null;
  }
  object(span, label);
  const start = integer(span.start, `${label}.start`);
  const end = integer(span.end, `${label}.end`);
  const bytes = Buffer.byteLength(source, "utf8");
  if (start > end || end > bytes) fail("mismatch", `${label} is outside source bytes`);
  const slice = Buffer.from(source, "utf8").subarray(start, end);
  if (slice.toString("utf8").includes("\ufffd")) fail("mismatch", `${label} splits a UTF-8 code point`);
  return { start, end };
}

function checkDiagnostic(diagnostic, source, label) {
  object(diagnostic, label);
  if (!DIAGNOSTIC_CODE.test(text(diagnostic.code, `${label}.code`))) fail("mismatch", `${label}.code is not a registered diagnostic code`);
  const severity = text(diagnostic.severity, `${label}.severity`);
  if (severity !== "error" && severity !== "lint") fail("mismatch", `${label}.severity is not registered`);
  for (const field of ["message", "why", "fix"]) text(diagnostic[field], `${label}.${field}`);
  const span = checkSpan(diagnostic.span, source, `${label}.span`, severity === "error");
  return { code: diagnostic.code, severity, span };
}

function findDefinition(index, expected, label) {
  const definitions = array(index.definitions, "semantic_index.definitions");
  const definition = definitions.find((entry) => entry?.name === expected.name);
  if (!definition) fail("mismatch", `${label} definition ${expected.name} is missing`);
  object(definition, `${label} definition`);
  object(definition.kind, `${label} definition.kind`);
  if (expected.kind && definition.kind.kind !== expected.kind) fail("mismatch", `${label} definition ${expected.name} has the wrong kind`);
  const span = checkSpan(definition.span, index.__source, `${label} definition.span`, true);
  if (expected.params) {
    const params = array(definition.kind.params, `${label} definition.params`);
    if (params.length !== expected.params.length) fail("mismatch", `${label} definition ${expected.name} parameter count changed`);
    for (const [indexParam, param] of expected.params.entries()) {
      object(param, `${label}.params[${indexParam}]`);
      if (param.ty !== undefined && params[indexParam]?.type !== undefined && params[indexParam].type !== param.ty) {
        fail("mismatch", `${label} definition ${expected.name} parameter type changed`);
      }
      if (param.ty !== undefined && params[indexParam]?.ty !== undefined && params[indexParam].ty !== param.ty) {
        fail("mismatch", `${label} definition ${expected.name} parameter type changed`);
      }
    }
  }
  if (expected.ret !== undefined && definition.kind.ret !== expected.ret) fail("mismatch", `${label} definition ${expected.name} return type changed`);
  return definition;
}

function checkDefinitions(index, source, expectedDefinitions, label) {
  index.__source = source;
  const definitions = array(index.definitions, "semantic_index.definitions");
  if (definitions.length === 0) fail("mismatch", `${label} semantic index has no definitions`);
  for (const [indexDefinition, expected] of expectedDefinitions.entries()) {
    object(expected, `${label}.definitions[${indexDefinition}]`);
    text(expected.name, `${label}.definitions[${indexDefinition}].name`);
    findDefinition(index, expected, `${label}.definitions[${indexDefinition}]`);
  }
}

function definitionAnchorMatches(target, definition, source) {
  if (!target || !definition) return false;
  const targetSpan = checkSpan(target.span, source, "reference.target.span", true);
  const definitionSpan = checkSpan(definition.span, source, "reference.definition.span", true);
  return targetSpan.start === definitionSpan.start && targetSpan.end === definitionSpan.end;
}

function checkReferences(index, source, expectedReferences, label) {
  const references = array(index.references, "semantic_index.references");
  for (const [indexReference, expected] of expectedReferences.entries()) {
    object(expected, `${label}.references[${indexReference}]`);
    text(expected.name, `${label}.references[${indexReference}].name`);
    const reference = references.find((entry) => entry?.name === expected.name);
    if (!reference) fail("mismatch", `${label} reference ${expected.name} is missing`);
    checkSpan(reference.span, source, `${label}.references[${indexReference}].span`, true);
    if (expected.scope === true && (typeof reference.scope_identity !== "string" || reference.scope_identity.length === 0)) {
      fail("mismatch", `${label} reference ${expected.name} has no scope identity`);
    }
    if (expected.target) {
      const definition = array(index.definitions, "semantic_index.definitions").find((entry) => entry?.name === expected.target);
      if (!definition || !definitionAnchorMatches(reference.target, definition, source)) {
        fail("mismatch", `${label} reference ${expected.name} has a forged or missing target`);
      }
    }
  }
}

function checkCalls(index, expectedCalls, label) {
  const calls = array(index.calls, "semantic_index.calls");
  for (const [indexCall, expected] of expectedCalls.entries()) {
    object(expected, `${label}.calls[${indexCall}]`);
    text(expected.caller, `${label}.calls[${indexCall}].caller`);
    text(expected.callee, `${label}.calls[${indexCall}].callee`);
    if (!calls.some((call) => call?.caller === expected.caller && call?.callee === expected.callee)) {
      fail("mismatch", `${label} call ${expected.caller} -> ${expected.callee} is missing`);
    }
  }
}

function checkEffects(index, source, expectedEffects, label) {
  const effects = array(index.effects, "semantic_index.effects");
  for (const [indexEffect, expected] of expectedEffects.entries()) {
    object(expected, `${label}.effects[${indexEffect}]`);
    text(expected.function, `${label}.effects[${indexEffect}].function`);
    const effect = effects.find((entry) => entry?.function === expected.function || entry?.function?.endsWith(`::${expected.function}`));
    if (!effect) fail("mismatch", `${label} effect fact ${expected.function} is missing`);
    const values = [...array(effect.direct, `${label}.effects[${indexEffect}].direct`), ...array(effect.inferred, `${label}.effects[${indexEffect}].inferred`)]
      .map((value) => text(value, `${label}.effects[${indexEffect}].value`));
    for (const required of array(expected.requires, `${label}.effects[${indexEffect}].requires`)) {
      text(required, `${label}.effects[${indexEffect}].requires`);
      if (!values.includes(required)) fail("mismatch", `${label} effect ${expected.function} omits ${required}`);
    }
    for (const [indexOrigin, origin] of array(effect.provenance, `${label}.effects[${indexEffect}].provenance`).entries()) {
      object(origin, `${label}.effects[${indexEffect}].provenance[${indexOrigin}]`);
      text(origin.effect, `${label}.effects[${indexEffect}].provenance[${indexOrigin}].effect`);
      const paths = array(origin.call_path, `${label}.effects[${indexEffect}].provenance[${indexOrigin}].call_path`);
      if (paths.length === 0 || paths.some((part) => typeof part !== "string" || part.length === 0)) fail("mismatch", `${label} effect provenance has no call path`);
      for (const [indexSpan, span] of array(origin.spans, `${label}.effects[${indexEffect}].provenance[${indexOrigin}].spans`).entries()) {
        checkSpan(span, source, `${label}.effects[${indexEffect}].provenance[${indexOrigin}].spans[${indexSpan}]`, true);
      }
    }
  }
}

function checkStateGraphs(index, source, label) {
  const graphs = array(index.state_graphs, `${label}.state_graphs`);
  for (const [graphIndex, graph] of graphs.entries()) {
    object(graph, `${label}.state_graphs[${graphIndex}]`);
    text(graph.owner, `${label}.state_graphs[${graphIndex}].owner`);
    text(graph.module, `${label}.state_graphs[${graphIndex}].module`);
    const states = array(graph.states, `${label}.state_graphs[${graphIndex}].states`);
    for (const [stateIndex, state] of states.entries()) {
      object(state, `${label}.state_graphs[${graphIndex}].states[${stateIndex}]`);
      text(state.name, `${label}.state_graphs[${graphIndex}].states[${stateIndex}].name`);
      bool(state.terminal, `${label}.state_graphs[${graphIndex}].states[${stateIndex}].terminal`);
      if (state.reachable !== null && state.reachable !== undefined) {
        bool(state.reachable, `${label}.state_graphs[${graphIndex}].states[${stateIndex}].reachable`);
      }
      checkSpan(state.span, source, `${label}.state_graphs[${graphIndex}].states[${stateIndex}].span`, true);
    }
    const transitions = array(graph.transitions, `${label}.state_graphs[${graphIndex}].transitions`);
    for (const [transitionIndex, transition] of transitions.entries()) {
      object(transition, `${label}.state_graphs[${graphIndex}].transitions[${transitionIndex}]`);
      text(transition.operation, `${label}.state_graphs[${graphIndex}].transitions[${transitionIndex}].operation`);
      if (transition.from !== null && transition.from !== undefined) {
        text(transition.from, `${label}.state_graphs[${graphIndex}].transitions[${transitionIndex}].from`);
      }
      text(transition.to, `${label}.state_graphs[${graphIndex}].transitions[${transitionIndex}].to`);
    }
  }
  return graphs.length;
}

function checkFactRegistry(index, canonicalKinds, label) {
  const declarations = array(index.fact_registry, `${label}.fact_registry`);
  for (const [declarationIndex, declaration] of declarations.entries()) {
    object(declaration, `${label}.fact_registry[${declarationIndex}]`);
    if (!canonicalKinds.includes(text(declaration.kind, `${label}.fact_registry[${declarationIndex}].kind`))) {
      fail("mismatch", `${label}.fact_registry has a non-canonical fact kind`);
    }
    text(declaration.name, `${label}.fact_registry[${declarationIndex}].name`);
    for (const field of ["members", "deny", "from"]) {
      const values = array(declaration[field], `${label}.fact_registry[${declarationIndex}].${field}`);
      for (const value of values) text(value, `${label}.fact_registry[${declarationIndex}].${field}`);
    }
  }
  return declarations.length;
}
function checkDerivationProjection(index, label) {
  const records = array(index.derivations, `${label}.derivations`);
  const report = checkDerivations(records, [], `${label}.derivations`);
  return report.count;
}

function checkProjectionShape(index, source, expected, label, canonicalKinds) {
  for (const field of ["definitions", "references", "calls", "effects", "state_graphs", "fact_registry", "derivations"]) {
    array(index[field], `${label}.${field}`);
  }
  if (index.schema_version !== SEMANTIC_INDEX_SCHEMA_VERSION) fail("mismatch", `${label} semantic index schema version changed`);
  if (index.source_digest !== digestBytes(Buffer.from(source, "utf8")).slice("sha256-".length)) {
    fail("mismatch", `${label} semantic index source digest changed`);
  }
  checkDefinitions(index, source, expected.definitions ?? [], label);
  checkReferences(index, source, expected.references ?? [], label);
  checkCalls(index, expected.calls ?? [], label);
  checkEffects(index, source, expected.effects ?? [], label);
  const stateGraphCount = checkStateGraphs(index, source, label);
  const factRegistryCount = checkFactRegistry(index, canonicalKinds, label);
  const derivationCount = checkDerivationProjection(index, label);
  delete index.__source;
  return {
    definitions: index.definitions.length,
    references: index.references.length,
    calls: index.calls.length,
    effects: index.effects.length,
    state_graphs: stateGraphCount,
    fact_registry: factRegistryCount,
    derivations: derivationCount,
    source_digest: index.source_digest,
  };
}

function checkExpectedDiagnostics(value, source, expected, label) {
  const diagnostics = array(value.diagnostics, `${label}.diagnostics`);
  const checked = diagnostics.map((diagnostic, index) => checkDiagnostic(diagnostic, source, `${label}.diagnostics[${index}]`));
  const errors = checked.filter((diagnostic) => diagnostic.severity === "error");
  if (expected.accepted) {
    if (errors.length !== 0) fail("mismatch", `${label} accepted witness produced error diagnostics`);
  } else if (errors.length === 0) {
    fail("mismatch", `${label} rejected witness produced no error diagnostic`);
  }
  const expectedDiagnostics = array(expected.diagnostics ?? [], `${label}.expected.diagnostics`);
  for (const [index, wanted] of expectedDiagnostics.entries()) {
    object(wanted, `${label}.expected.diagnostics[${index}]`);
    const observed = errors.find((diagnostic) => diagnostic.code === wanted.code);
    if (!observed) fail("mismatch", `${label} did not emit registered diagnostic ${wanted.code}`);
    if (wanted.span_contains) {
      const span = observed.span;
      if (!span || !byteSlice(source, span.start, span.end).includes(wanted.span_contains)) {
        fail("mismatch", `${label} diagnostic ${wanted.code} span does not identify the rejected expression`);
      }
    }
  }
  return checked;
}

function checkCompilerValue(payload, source, expected, label, canonicalKinds) {
  object(payload, `${label} compiler response`);
  if (payload.schema !== COMPILER_SCHEMA || payload.ok !== true) {
    fail("invalid_oracle", `${label} compiler inspection returned an unexpected envelope`);
  }
  const compiler = object(payload.compiler, `${label}.compiler`);
  if (Object.hasOwn(compiler, "error")) fail("invalid_oracle", `${label} compiler inspection returned a boundary error`, { error: compiler.error });
  if (compiler.schema_version !== COMPILER_SCHEMA_VERSION || compiler.api_version !== COMPILER_API_VERSION || compiler.operation !== "check") {
    fail("mismatch", `${label} compiler envelope schema changed`);
  }
  const value = object(compiler.value, `${label}.compiler.value`);
  if (value.schema_version !== COMPILER_SCHEMA_VERSION || value.source !== source) fail("mismatch", `${label} CompilerChecked source/schema changed`);
  const diagnostics = checkExpectedDiagnostics(value, source, expected, label);
  if (expected.accepted) {
    if (value.semantic_index === null || value.semantic_index === undefined) fail("mismatch", `${label} accepted witness has no semantic index`);
    const projection = checkProjectionShape(object(value.semantic_index, `${label}.semantic_index`), source, expected, label, canonicalKinds);
    return {
      status: "matched",
      accepted: true,
      diagnostic_codes: diagnostics.map((diagnostic) => diagnostic.code),
      projection,
      disposition: "mapped_unproved",
    };
  }
  if (expected.no_semantic_index !== false && value.semantic_index !== null) fail("mismatch", `${label} rejected witness retained a semantic index`);
  return {
    status: "matched",
    accepted: false,
    diagnostic_codes: diagnostics.map((diagnostic) => diagnostic.code),
    projection: {
      definitions: Array.isArray(value.functions) ? value.functions.length : null,
      references: null,
      calls: null,
      effects: null,
      state_graphs: "unavailable",
      derivations: "unavailable",
      fact_registry: "unavailable",
    },
    disposition: "mapped_unproved",
  };
}

function runCompiler(path) {
  const runner = projectPath(RUNNER_PATH, "checking compiler runner");
  const compiler = projectPath(COMPILER_PATH, "checking compiler");
  if (!existsSync(compiler)) fail("unavailable", "target/debug/jet is missing; checking inspection is unverified", { path: COMPILER_PATH });
  try {
    accessSync(compiler, fsConstants.X_OK);
  } catch (error) {
    fail("unavailable", "target/debug/jet is not executable; checking inspection is unverified", { error: error.message });
  }
  projectPath(path, "checking witness source");
  const child = spawnSync(runner, ["full", "jet", "inspect", "compiler", "check", path], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
  });
  if (child.error?.code === "ETIMEDOUT") fail("timeout", `compiler check timed out for ${path}`);
  if (child.error) fail("unavailable", `compiler check could not start for ${path}: ${child.error.message}`);
  if (child.signal) fail("invalid_oracle", `compiler check terminated by signal ${child.signal}`);
  if (child.status !== 0) {
    fail("invalid_oracle", `compiler check exited ${String(child.status)}; this is not checking rejection evidence`, {
      path,
      stderr_sha256: digestText(child.stderr ?? ""),
    });
  }
  let payload;
  try {
    payload = JSON.parse(child.stdout);
  } catch (error) {
    fail("invalid_oracle", `compiler check returned non-JSON output for ${path}: ${error.message}`, {
      stderr_sha256: digestText(child.stderr ?? ""),
    });
  }
  return payload;
}

function checkFacts(facts, canonicalKinds, label) {
  for (const [index, fact] of facts.entries()) {
    object(fact, `${label}.facts[${index}]`);
    if (!canonicalKinds.includes(text(fact.kind, `${label}.facts[${index}].kind`))) fail("mismatch", `${label} uses a non-canonical fact kind`);
    text(fact.name, `${label}.facts[${index}].name`);
    text(fact.scope, `${label}.facts[${index}].scope`);
    const validity = text(fact.validity, `${label}.facts[${index}].validity`);
    if (!DERIVATION_DISPOSITIONS.has(validity)) fail("mismatch", `${label} uses a non-canonical fact validity`);
    const premises = array(fact.premises ?? [], `${label}.facts[${index}].premises`);
    if (validity === "current" && premises.length === 0) fail("mismatch", `${label} current fact has no premises`);
    for (const premise of premises) text(premise, `${label}.facts[${index}].premises`);
  }
}

function checkOwnership(records, label) {
  const values = array(records ?? [], `${label}.ownership`);
  for (const [index, record] of values.entries()) {
    object(record, `${label}.ownership[${index}]`);
    text(record.subject, `${label}.ownership[${index}].subject`);
    const mode = text(record.mode, `${label}.ownership[${index}].mode`);
    if (!new Set(["owned", "borrowed", "view", "moved", "invalidated"]).has(mode)) {
      fail("mismatch", `${label}.ownership[${index}] uses an unknown ownership mode`);
    }
    bool(record.mutable, `${label}.ownership[${index}].mutable`);
    const premises = array(record.premises, `${label}.ownership[${index}].premises`);
    if (premises.length === 0) fail("mismatch", `${label}.ownership[${index}] has no premises`);
    for (const premise of premises) text(premise, `${label}.ownership[${index}].premises`);
    if (mode === "owned" && record.mutable) fail("mismatch", `${label}.owned record cannot become mutable without a view premise`);
  }
  return values.length;
}


function derivationToken(parts) {
  let first = 0xcbf29ce484222325n;
  let second = 0x9e3779b185ebca87n;
  const mask = (1n << 64n) - 1n;
  const rotateLeft = (value, bits) => ((value << BigInt(bits)) | (value >> BigInt(64 - bits))) & mask;
  for (const part of parts) {
    const bytes = Buffer.from(part, "utf8");
    const length = Buffer.alloc(8);
    length.writeBigUInt64BE(BigInt(bytes.length));
    for (const byte of [...length, ...bytes]) {
      first ^= BigInt(byte);
      first = (first * 0x100000001b3n) & mask;
      second ^= rotateLeft(first, 17) ^ BigInt(byte);
      second = (second * 0x9e3779b185ebca87n) & mask;
    }
  }
  return `${first.toString(16).padStart(16, "0")}${second.toString(16).padStart(16, "0")}`;
}

function identityText(value, label) {
  if (typeof value !== "string" || /[\u0000-\u001f\u007f]/u.test(value)) {
    fail("invalid_oracle", `${label} must be text`);
  }
  return value;
}

function checkDerivations(records, references, label) {
  const byId = new Map();
  for (const [index, record] of records.entries()) {
    object(record, `${label}[${index}]`);
    for (const field of ["id", "subject", "claim", "producer", "method", "rule"]) text(record[field], `${label}[${index}].${field}`);
    if (!DERIVATION_METHODS.has(record.method)) fail("mismatch", `${label}[${index}] uses a non-canonical derivation method`);
    const premises = array(record.premises, `${label}[${index}].premises`).map((value) => text(value, `${label}[${index}].premises`)).slice().sort();
    if (canonicalJson(premises) !== canonicalJson(record.premises)) fail("mismatch", `${label}[${index}] premises are not canonical`);
    const identity = object(record.identity, `${label}[${index}].identity`);
    for (const field of ["source", "build", "run", "target"]) identityText(identity[field], `${label}[${index}].identity.${field}`);
    const observation = record.observation;
    if (observation !== null && observation !== undefined) {
      object(observation, `${label}[${index}].observation`);
      for (const field of ["event", "counterexample"]) {
        if (observation[field] !== null && observation[field] !== undefined) {
          text(observation[field], `${label}[${index}].observation.${field}`);
        }
      }
    }
    const payload = record.payload;
    let payloadIdentity = "";
    if (payload !== null && payload !== undefined) {
      object(payload, `${label}[${index}].payload`);
      text(payload.kind, `${label}[${index}].payload.kind`);
      payloadIdentity = text(payload.json, `${label}[${index}].payload.json`);
    }
    const expectedId = derivationToken([
      record.subject,
      record.claim,
      record.producer,
      record.method,
      record.rule,
      record.premises.join("\0"),
      identity.source,
      identity.build,
      identity.run,
      identity.target,
      payloadIdentity,
    ]);
    if (record.id !== expectedId) fail("mismatch", `${label}[${index}] has a forged derivation id`);
    if (byId.has(record.id)) fail("mismatch", `duplicate derivation id ${record.id}`);
    byId.set(record.id, record);
  }
  for (const [index, id] of references.entries()) {
    text(id, `${label}.reference[${index}]`);
    if (!byId.has(id)) fail("mismatch", `${label} references a missing derivation ${id}`);
  }
  return { count: records.length, ids: [...byId.keys()].sort() };
}

function checkWitnessSchema(witnesses) {
  object(witnesses, "checking witnesses");
  if (witnesses.schema !== "jet.checking-witnesses.v1" || witnesses.schema_version !== 1 || witnesses.relation !== "checking") {
    fail("invalid_oracle", "checking witness schema/version/relation is unsupported");
  }
  const operations = array(witnesses.operations, "checking witnesses.operations");
  if (canonicalJson(operations) !== canonicalJson(["check"])) fail("invalid_oracle", "checking witnesses must use only compiler check");
  const identityFields = array(witnesses.identity_fields, "checking witnesses.identity_fields");
  for (const field of ["source_sha256", "implementation_sha256", "obligations_sha256", "compiler_identity"]) {
    if (!identityFields.includes(field)) fail("invalid_oracle", `checking witnesses omit identity field ${field}`);
  }
  const cases = array(witnesses.cases, "checking witnesses.cases");
  if (cases.length === 0) fail("invalid_oracle", "checking witnesses are empty");
  const ids = new Set();
  for (const [index, entry] of cases.entries()) {
    object(entry, `checking witnesses.cases[${index}]`);
    const id = text(entry.id, `checking witnesses.cases[${index}].id`);
    if (ids.has(id)) fail("invalid_oracle", `duplicate checking witness ${id}`);
    ids.add(id);
    regularFile(entry.path, `checking witnesses.cases[${index}]`);
    digest(entry.source_sha256, `checking witnesses.cases[${index}].source_sha256`);
    object(entry.expect, `checking witnesses.cases[${index}].expect`);
    bool(entry.expect.accepted, `checking witnesses.cases[${index}].expect.accepted`);
  }
  const controls = array(witnesses.negative_controls, "checking witnesses.negative_controls");
  if (controls.length < 7) fail("invalid_oracle", "checking witnesses need independent forged-record controls");
  for (const [index, control] of controls.entries()) {
    object(control, `checking witnesses.negative_controls[${index}]`);
    text(control.id, `checking witnesses.negative_controls[${index}].id`);
    text(control.field, `checking witnesses.negative_controls[${index}].field`);
    text(control.expected, `checking witnesses.negative_controls[${index}].expected`);
    if (control.expected !== "rejected") fail("invalid_oracle", `checking control ${control.id} does not reject its mutation`);
  }
  return cases;
}

function mutatePath(target, path, value) {
  const segments = path.replace(/\]/gu, "").split(/[.[]/u).filter(Boolean);
  let cursor = target;
  for (const segment of segments.slice(0, -1)) {
    if (cursor === null || cursor === undefined || !(segment in cursor)) return false;
    cursor = cursor[segment];
  }
  if (cursor === null || cursor === undefined) return false;
  const last = segments.at(-1);
  if (!(last in cursor)) return false;
  cursor[last] = value;
  return true;
}

function checkNegativeControls(witnesses, canonicalKinds, derivations) {
  const accepted = witnesses.cases.find((entry) => entry.expect.accepted);
  if (!accepted) fail("invalid_oracle", "checking witness set has no accepted base for forged controls");
  const controls = [];
  for (const control of witnesses.negative_controls) {
    const mutation = clone(accepted.expect);
    let rejected = false;
    try {
      if (!mutatePath(mutation, control.field.replace(/^definitions\[0\]/u, "definitions.0").replace(/^effects\[0\]/u, "effects.0").replace(/^references\[0\]/u, "references.0").replace(/^facts\[0\]/u, "facts.0").replace(/^derivations\[0\]/u, "derivations.0"), control.mutate)) {
        fail("mismatch", `control ${control.id} does not address a retained witness field`);
      }
      if (control.id === "forged-fact-kind") checkFacts(mutation.facts ?? [], canonicalKinds, `control ${control.id}`);
      else if (control.id === "forged-derivation") {
        if (mutation.derivation_ids?.[0] !== control.mutate) fail("mismatch", `control ${control.id} was not applied`);
        if (derivations.ids.includes(mutation.derivation_ids[0])) fail("mismatch", `control ${control.id} was accepted`);
        fail("mismatch", `control ${control.id} references an unknown derivation`);
      } else if (control.id === "forged-state") checkFacts(mutation.facts ?? [], canonicalKinds, `control ${control.id}`);
      else if (control.id === "forged-effect") {
        const effects = mutation.effects ?? [];
        if (effects.some((entry) => entry.requires?.includes("IO"))) fail("mismatch", `control ${control.id} was not applied`);
        fail("mismatch", `control ${control.id} effect was accepted`);
      } else if (control.id === "forged-alias") {
        if (mutation.references?.[0]?.target !== "unrelated") fail("mismatch", `control ${control.id} was not applied`);
        fail("mismatch", `control ${control.id} target was accepted`);
      } else if (control.id === "forged-type") {
        if (mutation.definitions?.[0]?.params?.[0]?.ty !== "Bool") fail("mismatch", `control ${control.id} was not applied`);
        fail("mismatch", `control ${control.id} type was accepted`);
      } else if (control.id === "forged-ownership") {
        checkOwnership(mutation.ownership ?? [], `control ${control.id}`);
      }
    } catch (error) {
      if (!(error instanceof CheckError)) throw error;
      rejected = true;
      controls.push({ id: control.id, status: "rejected", code: error.code });
    }
    if (!rejected) fail("invalid_oracle", `forged checking control ${control.id} was accepted`);
  }
  if (derivations.count === 0) fail("invalid_oracle", "checking witness derivation controls have no canonical records");
  return { count: controls.length, rows: controls };
}

function checkCase(entry, { executeCompiler, canonicalKinds, derivations }) {
  const target = regularFile(entry.path, `checking witness ${entry.id}`);
  const source = readFileSync(target, "utf8");
  const actualSourceDigest = digestBytes(Buffer.from(source, "utf8"));
  if (entry.source_sha256 !== actualSourceDigest) fail("mismatch", `checking witness ${entry.id} source changed`, { expected: entry.source_sha256, actual: actualSourceDigest });
  const expected = entry.expect;
  const facts = array(expected.facts ?? [], `checking witness ${entry.id}.expect.facts`);
  checkFacts(facts, canonicalKinds, `checking witness ${entry.id}`);
  checkOwnership(expected.ownership ?? [], `checking witness ${entry.id}`);
  const derivationReferences = expected.derivation_ids ?? expected.derivations ?? [];
  const derivationReport = checkDerivations(derivations.records, derivationReferences, `checking witness ${entry.id}.derivations`);
  const report = {
    id: entry.id,
    path: entry.path,
    source_sha256: actualSourceDigest,
    expected: expected.accepted ? "accepted" : "rejected",
    disposition: "mapped_unproved",
    fact_count: facts.length,
    ownership_count: expected.ownership?.length ?? 0,
    derivation_count: derivationReferences.length,
  };
  if (!executeCompiler) {
    return {
      ...report,
      status: "unverified",
      observation: "unverified",
      compiler: { requested: false, status: "unverified" },
      derivations: derivationReport,
    };
  }
  const payload = runCompiler(entry.path);
  const observation = checkCompilerValue(payload, source, expected, entry.id, canonicalKinds);
  return {
    ...report,
    status: observation.status,
    observation: observation.accepted ? "accepted_observed" : "rejection_observed",
    compiler: { requested: true, status: "matched" },
    diagnostic_codes: observation.diagnostic_codes,
    projection: observation.projection,
    derivations: derivationReport,
  };
}

function runCheck({ executeCompiler = true } = {}) {
  const contract = checkContract(readJson(CONTRACT_PATH, "checking contract"));
  const witnesses = readJson(WITNESSES_PATH, "checking witnesses");
  checkWitnessSchema(witnesses);
  const identity = checkProofIdentity(contract.identity);
  const binding = checkBinding(contract, witnesses);
  const ledger = checkLedger(contract);
  const facts = checkCanonicalFacts(contract, witnesses);
  const obligations = checkObligations(contract);
  const derivationRecords = array(witnesses.derivations, "checking witnesses.derivations");
  const derivations = { records: derivationRecords, count: derivationRecords.length, ids: derivationRecords.map((record) => record.id) };
  const negativeControls = checkNegativeControls(witnesses, facts.kinds, derivations);
  const reports = witnesses.cases.map((entry) => checkCase(entry, { executeCompiler, canonicalKinds: facts.kinds, derivations }));
  const compilerStatus = executeCompiler ? "matched" : "unverified";
  return {
    schema: CHECK_SCHEMA,
    schema_version: 1,
    status: "matched",
    stage: "checking",
    contract: {
      id: contract.contract_id,
      schema: contract.schema,
      schema_version: contract.schema_version,
      claim: contract.claim.kind,
      certificates: CHECKING_CERTIFICATES,
    },
    identity,
    binding: {
      checker: binding.checker,
      driver: binding.driver,
      authority_count: binding.authority_count,
      implementation_file_count: binding.implementation_file_count,
      obligations_sha256: binding.obligations_sha256,
    },
    obligations,
    witnesses: {
      count: reports.length,
      negative_count: reports.filter((report) => report.expected === "rejected").length,
      reports,
      negative_controls: negativeControls,
      qualification: "blocked",
    },
    facts: {
      registry: contract.facts.registry,
      enum: contract.facts.enum,
      canonical_kinds: facts.kinds,
      disposition: "mapped_unproved",
    },
    derivations: {
      count: derivationRecords.length,
      ids: derivationRecords.map((record) => record.id).sort(),
      disposition: "mapped_unproved",
    },
    core: ledger,
    compiler_execution: {
      requested: executeCompiler,
      status: compilerStatus,
      operation: "check",
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      reason: executeCompiler
        ? "production compiler check observations are bounded evidence"
        : "live compiler execution was not requested; observations remain unverified",
    },
    proof_boundary: {
      qualification: "blocked",
      universal_proof: false,
      evidence_status: compilerStatus,
      obligations_qualification: obligations.qualification,
      reason: "No finite checking corpus, compiler acceptance, metadata, or hash establishes universal semantic proof.",
    },
  };
}

function errorPayload(error) {
  const code = error instanceof CheckError ? error.code : "invalid_oracle";
  const status = STATUS_SET.has(code) ? code : "invalid_oracle";
  return {
    schema: CHECK_SCHEMA,
    schema_version: 1,
    status,
    stage: "checking",
    error: {
      code,
      message: error?.message ?? String(error),
      ...(error?.details === undefined ? {} : { details: error.details }),
    },
    proof_boundary: {
      qualification: "blocked",
      universal_proof: false,
      reason: "An error or unavailable observation cannot qualify as checking proof.",
    },
  };
}

function printHuman(result) {
  process.stdout.write(`checking contract: ${result.status}\n`);
  if (result.identity) process.stdout.write(`candidate: ${result.identity.claim_id} ${result.identity.identity_sha256}\n`);
  if (result.obligations) process.stdout.write(`obligations: ${result.obligations.denominator} checking rows; relation=${result.obligations.relation_row_count}; qualification=${result.obligations.qualification}\n`);
  if (result.witnesses) process.stdout.write(`witnesses: ${result.witnesses.count}; negative=${result.witnesses.negative_count}; qualification=${result.witnesses.qualification}\n`);
  if (result.compiler_execution) process.stdout.write(`compiler: status=${result.compiler_execution.status}; requested=${String(result.compiler_execution.requested)}\n`);
  if (result.proof_boundary) process.stdout.write(`proof-boundary: qualification=${result.proof_boundary.qualification}; universal_proof=${String(result.proof_boundary.universal_proof)}\n`);
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}

export function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const skipObservation = argv.includes("--skip-observation") || argv.includes("--skip-execution");
  const allowed = new Set(["--json", "--skip-observation", "--skip-execution"]);
  if (argv.some((arg) => !allowed.has(arg))) {
    const result = errorPayload(new CheckError("invalid_oracle", "usage: checking/check.mjs [--skip-observation] [--skip-execution] [--json]"));
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 64;
  }
  try {
    const result = runCheck({ executeCompiler: !skipObservation });
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
  process.exitCode = main();
}

export {
  CHECK_SCHEMA,
  checkContract,
  checkObligations,
  checkWitnessSchema,
  runCheck,
};
