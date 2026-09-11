#!/usr/bin/env node

/**
 * Source-boundary checker for the #2936 compiler-proof slice.
 *
 * The checker never implements Jet's grammar. It drives the production
 * `inspect compiler` seam and checks the returned lexer/parser/sema values
 * against retained, byte-identified witnesses. A tool failure is classified
 * separately from a source rejection, and finite witness agreement never
 * becomes an unbounded theorem.
 */

import {
  accessSync,
  constants as fsConstants,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";

import {
  canonical,
  canonicalJson,
  digestBytes,
  digestText,
  identityDigest,
  validateManifest,
} from "../../../scripts/agent/compiler-proof.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const CONTRACT_PATH = "proof/compiler/source/contract.json";
const WITNESSES_PATH = "proof/compiler/source/witnesses.json";
const OBLIGATIONS_PATH = "proof/compiler/obligations.json";
const PROOF_MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const RUNNER_PATH = "scripts/agent/jet-env";
const COMPILER_PATH = "target/debug/jet";
const CHECKER_PATH = "proof/compiler/source/check.mjs";
const CHECK_SCHEMA = "jet.source-contract-check.v1";
const TIMEOUT_MS = 120000;
const SOURCE_OPERATIONS = Object.freeze(["lex", "parse", "check"]);
const SOURCE_BOUNDARIES = Object.freeze([
  "boundary:source-bytes",
  "boundary:lexing",
  "boundary:parsing",
]);
const SOURCE_CERTIFICATES = Object.freeze([
  "exact_source_bytes",
  "token_spans_and_utf8_boundaries",
  "complete_input_consumption",
  "syntax_node_origins",
  "precedence_token_order",
  "nested_scope_facts",
  "qualified_import_graph",
  "registered_rejection",
  "mutation_identity",
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
]);
const IMPLEMENTATION_ROOTS = Object.freeze([
  "crates/jet-lexer/src",
  "crates/jet-parser/src",
  "Source/Compiler.rs",
  "Source/CmdCompile.rs",
  "crates/jet-driver/src/Loader.rs",
]);
const AUTHORITY_PATHS = Object.freeze([
  "crates/jet-foundation/src/Syntax.rs",
  "docs/spec/syntax-decisions.md",
  "docs/spec/architecture.md",
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

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function digestCanonical(value) {
  return digestText(canonicalJson(value));
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

function regularFile(path, label) {
  const target = projectPath(path, label);
  let stat;
  try {
    stat = lstatSync(target);
  } catch (error) {
    fail("unavailable", `${label} is missing: ${path}`, { path, error: error.message });
  }
  if (stat.isSymbolicLink() || !stat.isFile()) {
    fail("invalid_oracle", `${label} is not a regular file: ${path}`);
  }
  return target;
}

function digestFile(path) {
  return digestBytes(readFileSync(path));
}

function digestRef(ref, label) {
  object(ref, label);
  const path = text(ref.path, `${label}.path`);
  const target = regularFile(path, label);
  const expected = text(ref.sha256, `${label}.sha256`);
  if (!/^sha256-[0-9a-f]{64}$/u.test(expected)) {
    fail("invalid_oracle", `${label}.sha256 is not a lowercase SHA-256 digest`);
  }
  const actual = digestFile(target);
  if (actual !== expected) {
    fail("mismatch", `${label} digest changed`, { path, expected, actual });
  }
  return { path, target, sha256: actual };
}

function collectFiles(rootRef) {
  const root = projectPath(rootRef, "implementation root");
  const paths = [];
  function visit(current) {
    let stat;
    try {
      stat = lstatSync(current);
    } catch (error) {
      fail("unavailable", `implementation path is missing: ${relative(ROOT, current)}`, { error: error.message });
    }
    if (stat.isSymbolicLink()) fail("invalid_oracle", `implementation path is a symlink: ${relative(ROOT, current)}`);
    if (stat.isFile()) {
      paths.push(relative(ROOT, current).split(sep).join("/"));
      return;
    }
    if (!stat.isDirectory()) fail("invalid_oracle", `implementation path is not a file or directory: ${relative(ROOT, current)}`);
    for (const entry of readdirSync(current, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
      visit(join(current, entry.name));
    }
  }
  visit(root);
  return paths.sort();
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
  object(contract, "source contract");
  if (contract.schema !== "jet.source-contract.v1" || contract.schema_version !== 1) {
    fail("invalid_oracle", "source contract schema/version is unsupported");
  }
  text(contract.contract_id, "contract.contract_id");
  const policy = object(contract.policy, "contract.policy");
  if (policy.decision_id !== "D-COMPILER-PROOF1" || policy.outcome !== "A" || policy.status !== "ratified") {
    fail("invalid_oracle", "source contract does not record ratified D-COMPILER-PROOF1 outcome A");
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
  const observations = [...new Set(claim.observation_alphabet)].sort();
  const expectedObservations = [
    "construction_identity",
    "diagnostics",
    "input_consumption",
    "lexical_tokens",
    "source_bytes",
    "source_graph",
    "source_origins",
    "syntax_nodes",
  ].sort();
  if (!deepEqual(observations, expectedObservations) || claim.observation_alphabet.length !== expectedObservations.length) {
    fail("invalid_oracle", "source observation alphabet is incomplete");
  }

  const execution = object(contract.execution, "contract.execution");
  if (execution.schema !== "jet.compiler-inspection.v1"
      || execution.runner !== RUNNER_PATH
      || execution.compiler !== COMPILER_PATH
      || execution.unknown_status !== "unverified"
      || execution.crash_policy !== "tool failures are unavailable or invalid_oracle, never source rejection") {
    fail("invalid_oracle", "source compiler execution policy is missing or drifted");
  }
  text(execution.command, "contract.execution.command");
  if (!deepEqual(array(execution.operations, "contract.execution.operations"), SOURCE_OPERATIONS)) {
    fail("invalid_oracle", "source compiler operation list drifted");
  }
  if (!Number.isSafeInteger(execution.timeout_ms) || execution.timeout_ms !== TIMEOUT_MS) {
    fail("invalid_oracle", "source compiler timeout drifted");
  }
  text(execution.scratch, "contract.execution.scratch");
  if (execution.scratch.includes("/tmp")) fail("invalid_oracle", "source proof scratch path uses forbidden /tmp");

  const methods = array(contract.binding_methods, "contract.binding_methods");
  const requiredMethods = new Set(["verified_construction", "direct_verification", "translation_certificate"]);
  if (methods.length !== requiredMethods.size) fail("invalid_oracle", "source contract binding methods are incomplete");
  for (const [index, method] of methods.entries()) {
    object(method, `contract.binding_methods[${index}]`);
    const name = text(method.method, `contract.binding_methods[${index}].method`);
    if (!requiredMethods.delete(name)) fail("invalid_oracle", `invalid or duplicate binding method ${name}`);
    for (const field of ["source", "implementation", "checker", "limit"]) text(method[field], `contract.binding_methods[${index}].${field}`);
    const special = {
      verified_construction: "construction",
      direct_verification: "candidate_binding",
      translation_certificate: "certificate",
    }[name];
    text(method[special], `contract.binding_methods[${index}].${special}`);
  }
  if (requiredMethods.size > 0) fail("invalid_oracle", "source contract is missing a binding method");

  const identity = object(contract.identity, "contract.identity");
  if (identity.schema !== "jet.compiler-proof.v1" || identity.manifest_path !== PROOF_MANIFEST_PATH) {
    fail("invalid_oracle", "source contract must reuse the pinned compiler-proof manifest");
  }
  text(identity.claim_id, "contract.identity.claim_id");
  text(identity.identity_digest_source, "contract.identity.identity_digest_source");
  if (identity.driver_path !== "scripts/agent/compiler-proof.mjs") {
    fail("invalid_oracle", "contract identity driver path drifted");
  }
  const requiredIdentity = array(identity.required_fields, "contract.identity.required_fields");
  if (requiredIdentity.length < 10 || requiredIdentity.some((field) => typeof field !== "string" || field.length === 0)) {
    fail("invalid_oracle", "source contract identity field list is incomplete");
  }

  const coverage = object(contract.coverage, "contract.coverage");
  if (coverage.relation_path !== OBLIGATIONS_PATH || coverage.generator !== "proof/compiler/semantics/generate-obligations.mjs" || coverage.owner !== "#2936") {
    fail("invalid_oracle", "source coverage relation or owner drifted");
  }
  text(coverage.qualification_rule, "contract.coverage.qualification_rule");
  const obligations = array(coverage.obligations, "contract.coverage.obligations");
  if (obligations.length !== SOURCE_BOUNDARIES.length) fail("invalid_oracle", "source coverage boundary list is incomplete");
  const ids = new Set(SOURCE_BOUNDARIES);
  for (const [index, row] of obligations.entries()) {
    object(row, `contract.coverage.obligations[${index}]`);
    if (!ids.delete(text(row.id, `contract.coverage.obligations[${index}].id`))) fail("invalid_oracle", "source coverage boundary is duplicate or unknown");
    text(row.rule, `contract.coverage.obligations[${index}].rule`);
    text(row.subject, `contract.coverage.obligations[${index}].subject`);
  }
  if (ids.size > 0) fail("invalid_oracle", "source coverage boundary is missing");

  const certificates = array(contract.certificates, "contract.certificates");
  if (!deepEqual([...new Set(certificates)].sort(), [...SOURCE_CERTIFICATES].sort())) {
    fail("invalid_oracle", "source certificate list is incomplete or duplicated");
  }
  const boundary = object(contract.boundary, "contract.boundary");
  const classes = ["proven", "assumed", "tested", "blocked"];
  const seenBoundary = new Set();
  for (const label of classes) {
    const values = array(boundary[label], `contract.boundary.${label}`);
    if (values.length === 0 || values.some((value) => typeof value !== "string" || value.length === 0)) {
      fail("invalid_oracle", `contract.boundary.${label} must contain named entries`);
    }
    for (const value of values) {
      if (seenBoundary.has(value)) fail("invalid_oracle", `source boundary entry is duplicated: ${value}`);
      seenBoundary.add(value);
    }
  }
  if (boundary.qualification !== "blocked") fail("invalid_oracle", "source contract cannot qualify as an unbounded proof");
  return { claim, execution, identity, coverage, boundary };
}

function checkBinding(contract, witnesses) {
  const binding = object(witnesses.binding, "source witness binding");
  const contractDigest = text(binding.contract_sha256, "source witness binding.contract_sha256");
  const expectedContractDigest = digestCanonical(contract);
  if (contractDigest !== expectedContractDigest) {
    fail("mismatch", "source contract construction input changed", { expected: contractDigest, observed: expectedContractDigest });
  }
  const manifestDigest = text(binding.manifest_sha256, "source witness binding.manifest_sha256");
  const expectedManifestDigest = digestCanonical(withoutBindingDigests(witnesses));
  if (manifestDigest !== expectedManifestDigest) {
    fail("mismatch", "source witness construction input changed", { expected: manifestDigest, observed: expectedManifestDigest });
  }

  const checker = digestRef(binding.checker, "source witness checker");
  if (checker.path !== CHECKER_PATH) fail("invalid_oracle", "source witness checker path drifted");
  const driver = digestRef(binding.driver, "source witness compiler-proof driver");
  if (driver.path !== contract.identity.driver_path) fail("invalid_oracle", "source witness driver path disagrees with contract");

  const authorities = array(binding.authorities, "source witness authorities");
  const authorityPaths = new Set(AUTHORITY_PATHS);
  for (const [index, ref] of authorities.entries()) {
    const checked = digestRef(ref, `source witness authority ${index}`);
    if (!authorityPaths.delete(checked.path)) fail("invalid_oracle", `source witness authority is duplicate or unknown: ${checked.path}`);
  }
  if (authorityPaths.size > 0) fail("invalid_oracle", "source witness authority set is incomplete");

  const implementation = array(binding.implementation, "source witness implementation");
  const expectedPaths = IMPLEMENTATION_ROOTS.flatMap(collectFiles).sort();
  const actualPaths = implementation.map((ref, index) => {
    const checked = digestRef(ref, `source witness implementation ${index}`);
    return checked.path;
  }).sort();
  if (!deepEqual(actualPaths, expectedPaths)) {
    fail("mismatch", "source implementation file set changed", { expected: expectedPaths, observed: actualPaths });
  }
  if (new Set(actualPaths).size !== actualPaths.length) fail("invalid_oracle", "source witness implementation contains duplicate files");

  const obligations = digestRef(binding.obligations, "source witness obligation relation");
  if (obligations.path !== OBLIGATIONS_PATH) fail("invalid_oracle", "source witness obligation relation path drifted");
  return {
    checker,
    driver,
    authorities: authorities.length,
    implementation: implementation.length,
    obligations,
  };
}

function checkProofIdentity(identity) {
  const manifest = readJson(PROOF_MANIFEST_PATH, "proof manifest");
  let summary;
  try {
    summary = validateManifest(manifest);
  } catch (error) {
    fail("invalid_oracle", `pinned compiler-proof manifest validation failed: ${error.message}`);
  }
  const claims = array(manifest.claims, "proof manifest claims");
  const claim = claims.find((entry) => entry?.id === identity.claim_id);
  if (!claim) fail("invalid_oracle", `pinned compiler-proof claim is missing: ${identity.claim_id}`);
  let expected;
  try {
    expected = identityDigest(claim);
  } catch (error) {
    fail("invalid_oracle", `pinned compiler-proof identity could not be computed: ${error.message}`);
  }
  if (claim.identity_sha256 !== expected) {
    fail("mismatch", "pinned compiler-proof identity changed", { expected: claim.identity_sha256, observed: expected });
  }
  return {
    schema: "jet.compiler-proof.v1",
    manifest_id: manifest.manifest_id,
    claim_id: claim.id,
    identity_sha256: claim.identity_sha256,
    toolchain_pin_sha256: summary.pin,
  };
}

function checkObligations(contract) {
  const recorded = readJson(OBLIGATIONS_PATH, "source obligation relation");
  if (recorded.schema !== "jet.compiler-obligations.v1" || recorded.schema_version !== 1) {
    fail("invalid_oracle", "source obligation relation schema/version is unsupported");
  }
  const rows = array(recorded.rows, "source obligation relation rows");
  const byId = new Map();
  for (const [index, row] of rows.entries()) {
    object(row, `source obligation relation row ${index}`);
    const id = text(row.id, `source obligation relation row ${index}.id`);
    if (byId.has(id)) fail("invalid_oracle", `duplicate source obligation row ${id}`);
    byId.set(id, row);
  }
  const authorityRows = array(recorded.authorities, "source obligation authorities");
  for (const [index, authority] of authorityRows.entries()) {
    const checked = digestRef(authority, `source obligation authority ${index}`);
    if (checked.sha256 !== authority.sha256) fail("mismatch", `source obligation authority changed: ${checked.path}`);
  }
  const required = contract.coverage.obligations.map((row) => row.id);
  for (const [index, requiredId] of required.entries()) {
    const expected = contract.coverage.obligations[index];
    const row = byId.get(requiredId);
    if (!row) fail("mismatch", `source obligation is missing: ${requiredId}`);
    if (row.owner !== "#2936" || row.rule !== expected.rule || row.subject !== expected.subject || row.obligation_state !== "uncovered") {
      fail("mismatch", `source obligation row drifted: ${requiredId}`, { row });
    }
  }
  return {
    relation: OBLIGATIONS_PATH,
    denominator: required.length,
    rows: required.map((id) => ({ id, status: "mapped_unproved" })),
    status_counts: { mapped_unproved: required.length, unsupported: 0, unmapped: 0, proved: 0 },
    qualification: "blocked",
    recorded_row_count: rows.length,
  };
}

function byteSlice(bytes, start, end) {
  return bytes.subarray(start, end).toString("utf8");
}

function utf8Boundary(bytes, offset) {
  return offset === 0 || offset === bytes.length || (bytes[offset] & 0xc0) !== 0x80;
}

function checkSpan(span, bytes, label) {
  object(span, label);
  const start = integer(span.start, `${label}.start`);
  const end = integer(span.end, `${label}.end`);
  if (start > end || end > bytes.length || !utf8Boundary(bytes, start) || !utf8Boundary(bytes, end)) {
    fail("mismatch", `${label} is outside UTF-8 source boundaries`, { span, byte_length: bytes.length });
  }
  return { start, end };
}

function checkDiagnostic(diagnostic, bytes, label) {
  object(diagnostic, label);
  const code = text(diagnostic.code, `${label}.code`);
  if (!/^E[0-9]{4}$/u.test(code)) fail("invalid_oracle", `${label}.code is not a registered Jet diagnostic code`);
  const severity = text(diagnostic.severity, `${label}.severity`);
  if (severity !== "error" && severity !== "lint") fail("invalid_oracle", `${label}.severity is not error or lint`);
  text(diagnostic.message, `${label}.message`);
  text(diagnostic.why, `${label}.why`);
  text(diagnostic.fix, `${label}.fix`);
  if (diagnostic.span !== null) checkSpan(diagnostic.span, bytes, `${label}.span`);
  return { code, severity, span: diagnostic.span };
}

function checkDiagnostics(value, bytes, label) {
  return array(value, label).map((diagnostic, index) => checkDiagnostic(diagnostic, bytes, `${label}[${index}]`));
}

function checkCompilerEnvelope(payload, operation, sourcePath) {
  object(payload, `${operation} compiler envelope`);
  if (payload.schema !== ENVELOPE_SCHEMA || payload.ok !== true) {
    fail("invalid_oracle", `${operation} compiler inspection returned an unexpected envelope`, { source: sourcePath });
  }
  const compiler = object(payload.compiler, `${operation} compiler envelope.compiler`);
  if (compiler.operation !== operation || compiler.schema_version !== 1 || compiler.api_version !== 1) {
    fail("invalid_oracle", `${operation} compiler inspection version/operation drifted`);
  }
  if (Object.hasOwn(compiler, "error")) fail("invalid_oracle", `${operation} compiler inspection returned a boundary error`, { error: compiler.error });
  if (!Object.hasOwn(compiler, "value")) fail("invalid_oracle", `${operation} compiler inspection omitted value`);
  return compiler.value;
}

function runCompiler(operation, sourcePath) {
  const runner = projectPath(RUNNER_PATH, "source compiler runner");
  const compiler = projectPath(COMPILER_PATH, "source compiler");
  if (!existsSync(compiler)) fail("unavailable", "target/debug/jet is missing; source inspection is unverified", { path: COMPILER_PATH });
  try {
    accessSync(compiler, fsConstants.X_OK);
  } catch (error) {
    fail("unavailable", "target/debug/jet is not executable; source inspection is unverified", { error: error.message });
  }
  const child = spawnSync(runner, ["full", "jet", "inspect", "compiler", operation, sourcePath], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: TIMEOUT_MS,
    maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
  });
  if (child.error?.code === "ETIMEDOUT") fail("timeout", `source compiler inspection timed out for ${operation}`);
  if (child.error) fail("unavailable", `source compiler inspection could not start: ${child.error.message}`);
  if (child.signal) fail("invalid_oracle", `source compiler inspection terminated by signal ${child.signal}`);
  if (child.status !== 0) {
    fail("invalid_oracle", `source compiler inspection exited ${String(child.status)}; this is not source rejection evidence`, {
      operation,
      stderr_sha256: digestText(child.stderr ?? ""),
    });
  }
  let payload;
  try {
    payload = JSON.parse(child.stdout);
  } catch (error) {
    fail("invalid_oracle", `source compiler inspection returned non-JSON output: ${error.message}`, {
      operation,
      stderr_sha256: digestText(child.stderr ?? ""),
    });
  }
  return checkCompilerEnvelope(payload, operation, sourcePath);
}

function checkToken(token, bytes, previousEnd, label) {
  object(token, label);
  const kind = text(token.kind, `${label}.kind`);
  const tokenText = typeof token.text === "string" ? token.text : fail("invalid_oracle", `${label}.text must be text`);
  const span = checkSpan(token.span, bytes, `${label}.span`);
  if (span.start < previousEnd) fail("mismatch", `${label}.span overlaps an earlier token`);
  if (span.end > span.start && byteSlice(bytes, span.start, span.end) !== tokenText) {
    fail("mismatch", `${label}.text does not identify its source bytes`, { kind, span });
  }
  return { kind, text: tokenText, start: span.start, end: span.end };
}

function checkTokenSequence(tokens, expected, label) {
  const sequence = array(expected, label).map((entry, index) => {
    object(entry, `${label}[${index}]`);
    return { kind: text(entry.kind, `${label}[${index}].kind`), text: typeof entry.text === "string" ? entry.text : fail("invalid_oracle", `${label}[${index}].text must be text`) };
  });
  const filtered = tokens.filter((token) => !token.kind.startsWith("comment.") && token.kind !== "terminator" && token.kind !== "eof");
  for (let start = 0; start + sequence.length <= filtered.length; start += 1) {
    if (sequence.every((entry, offset) => filtered[start + offset].kind === entry.kind && filtered[start + offset].text === entry.text)) return;
  }
  fail("mismatch", `${label} is absent from canonical token order`, { expected: sequence });
}

function checkLexValue(value, sourceInfo, expected) {
  const source = sourceInfo.source;
  const bytes = Buffer.from(source, "utf8");
  object(value, "lex value");
  if (value.source !== source) fail("mismatch", "lexer did not preserve exact source bytes");
  const diagnostics = checkDiagnostics(value.diagnostics, bytes, "lex diagnostics");
  if (expected.accepted === true && diagnostics.length > 0) fail("mismatch", "accepted lexical witness produced diagnostics", { diagnostics });
  const rawTokens = array(value.tokens, "lex tokens");
  if (rawTokens.length === 0) fail("mismatch", "lexer returned no tokens");
  const tokens = [];
  let previousEnd = 0;
  for (const [index, token] of rawTokens.entries()) {
    const checked = checkToken(token, bytes, previousEnd, `lex token ${index}`);
    previousEnd = Math.max(previousEnd, checked.end);
    tokens.push(checked);
  }
  const eof = tokens[tokens.length - 1];
  if (eof.kind !== "eof" || eof.start !== bytes.length || eof.end !== bytes.length) {
    fail("mismatch", "lexer did not terminate at the exact source end", { eof, byte_length: bytes.length });
  }
  let cursor = 0;
  for (const token of tokens) {
    if (token.start > cursor && byteSlice(bytes, cursor, token.start).trim() !== "") {
      fail("mismatch", "lexer left non-whitespace source bytes unconsumed", { start: cursor, end: token.start });
    }
    cursor = Math.max(cursor, token.end);
  }
  if (cursor < bytes.length && byteSlice(bytes, cursor, bytes.length).trim() !== "") {
    fail("mismatch", "lexer left a non-whitespace source suffix unconsumed", { start: cursor, end: bytes.length });
  }
  if (expected.token_sequence) checkTokenSequence(tokens, expected.token_sequence, "lex token_sequence");
  if (expected.token_sequences) {
    const sequences = array(expected.token_sequences, "lex token_sequences");
    sequences.forEach((sequence, index) => checkTokenSequence(tokens, sequence, `lex token_sequences[${index}]`));
  }
  return {
    source_sha256: digestBytes(bytes),
    token_count: tokens.length,
    diagnostics: diagnostics.map((diagnostic) => diagnostic.code),
    eof: { start: eof.start, end: eof.end },
  };
}

function checkSyntaxItems(items, source, label, exact = true, allowForeignSpans = false) {
  const bytes = Buffer.from(source, "utf8");
  const values = array(items, label);
  const checked = values.map((item, index) => {
    object(item, `${label}[${index}]`);
    const kind = text(item.kind, `${label}[${index}].kind`);
    const name = item.name === null ? null : text(item.name, `${label}[${index}].name`);
    let span;
    try {
      span = checkSpan(item.span, bytes, `${label}[${index}].span`);
    } catch (error) {
      if (!allowForeignSpans || !(error instanceof CheckError) || error.code !== "mismatch") throw error;
      const rawSpan = object(item.span, `${label}[${index}].span`);
      span = { start: integer(rawSpan.start, `${label}[${index}].span.start`), end: integer(rawSpan.end, `${label}[${index}].span.end`) };
      if (span.start > span.end) fail("mismatch", `${label}[${index}].span is reversed`);
    }
    if (name !== null && span.end <= bytes.length && byteSlice(bytes, span.start, span.end) !== name && !allowForeignSpans) {
      fail("mismatch", `${label}[${index}] name does not identify its origin`, { name, span });
    }
    return { kind, name, span };
  });
  if (exact && checked.length === 0) fail("mismatch", `${label} is empty for an accepted source witness`);
  return checked;
}

function checkExpectedItems(items, expected, label) {
  const declarations = array(expected, label).map((entry, index) => {
    object(entry, `${label}[${index}]`);
    return { kind: text(entry.kind, `${label}[${index}].kind`), name: text(entry.name, `${label}[${index}].name`) };
  });
  if (items.length !== declarations.length || !declarations.every((entry, index) => items[index].kind === entry.kind && items[index].name === entry.name)) {
    fail("mismatch", `${label} does not match canonical declaration order`, { expected: declarations, observed: items });
  }
}

function parseDiagnostics(value, source, expectedAccepted, expectedDiagnostic) {
  const bytes = Buffer.from(source, "utf8");
  const diagnostics = checkDiagnostics(value.diagnostics, bytes, "parse diagnostics");
  if (expectedAccepted) {
    if (diagnostics.length > 0) fail("mismatch", "accepted parse witness produced diagnostics", { diagnostics });
    return diagnostics;
  }
  if (diagnostics.length === 0) fail("mismatch", "rejection witness was accepted without a diagnostic");
  if (expectedDiagnostic && !diagnostics.some((diagnostic) => diagnostic.code === expectedDiagnostic)) {
    fail("mismatch", `rejection witness did not report ${expectedDiagnostic}`, { diagnostics });
  }
  return diagnostics;
}

function checkParseValue(value, sourceInfo, expected) {
  const source = sourceInfo.source;
  object(value, "parse value");
  if (value.source !== source) fail("mismatch", "parser did not preserve exact source bytes");
  const accepted = expected.accepted === true;
  const diagnostics = parseDiagnostics(value, source, accepted, expected.diagnostic);
  const items = checkSyntaxItems(value.items, source, "parse items", accepted);
  if (expected.items) checkExpectedItems(items, expected.items, "parse expected items");
  if (expected.token_sequence || expected.token_sequences) {
    const lexValue = runCompiler("lex", sourceInfo.entryPath);
    checkLexValue(lexValue, sourceInfo, {
      accepted,
      token_sequence: expected.token_sequence,
      token_sequences: expected.token_sequences,
    });
  }
  return {
    source_sha256: digestText(source),
    item_count: items.length,
    diagnostics: diagnostics.map((diagnostic) => ({ code: diagnostic.code, span: diagnostic.span })),
    consumed_suffix: accepted ? "none" : "rejection-boundary",
  };
}

function moduleForPath(moduleName, files) {
  if (typeof moduleName !== "string") return null;
  return files.find((file) => moduleName === file.path || moduleName.endsWith(`/${file.path}`) || moduleName.endsWith(file.path)) ?? null;
}

function checkDefinition(definition, sourceInfo, expected, label) {
  object(definition, label);
  if (definition.name !== expected.name) fail("mismatch", `${label}.name differs`, { expected: expected.name, observed: definition.name });
  const kind = object(definition.kind, `${label}.kind`);
  if (kind.kind !== expected.kind) fail("mismatch", `${label}.kind differs`, { expected: expected.kind, observed: kind.kind });
  const module = text(definition.module, `${label}.module`);
  if (!module.endsWith(expected.module_suffix)) fail("mismatch", `${label}.module is not the expected source origin`, { expected: expected.module_suffix, observed: module });
  const file = moduleForPath(module, sourceInfo.files);
  if (!file) fail("mismatch", `${label}.module has no identified source file`, { module });
  const bytes = Buffer.from(file.source, "utf8");
  const span = checkSpan(definition.span, bytes, `${label}.span`);
  if (byteSlice(bytes, span.start, span.end) !== expected.name) fail("mismatch", `${label}.span does not identify its declaration`, { expected: expected.name, span });
  return { name: definition.name, kind: kind.kind, module, span };
}

function checkReference(reference, sourceInfo, expected, label) {
  object(reference, label);
  if (reference.name !== expected.name) fail("mismatch", `${label}.name differs`, { expected: expected.name, observed: reference.name });
  const module = text(reference.module, `${label}.module`);
  const sourceFile = moduleForPath(module, sourceInfo.files);
  if (!sourceFile) fail("mismatch", `${label}.module has no identified source file`, { module });
  const sourceBytes = Buffer.from(sourceFile.source, "utf8");
  const span = checkSpan(reference.span, sourceBytes, `${label}.span`);
  if (expected.scope_required && typeof reference.scope_identity !== "string") {
    fail("mismatch", `${label} omitted its lexical scope identity`);
  }
  if (expected.target_module_suffix) {
    const target = object(reference.target, `${label}.target`);
    const targetModule = text(target.module, `${label}.target.module`);
    if (!targetModule.endsWith(expected.target_module_suffix)) {
      fail("mismatch", `${label}.target does not preserve qualified source identity`, { expected: expected.target_module_suffix, observed: targetModule });
    }
  }
  return { name: reference.name, module, span, scope_identity: reference.scope_identity ?? null };
}

function checkCheckValue(value, sourceInfo, expected) {
  const source = sourceInfo.source;
  const bytes = Buffer.from(source, "utf8");
  object(value, "check value");
  if (value.source !== source) fail("mismatch", "checker did not preserve exact entry source bytes");
  const diagnostics = checkDiagnostics(value.diagnostics, bytes, "check diagnostics");
  const errors = diagnostics.filter((diagnostic) => diagnostic.severity === "error");
  if (expected.accepted === true && errors.length > 0) fail("mismatch", "accepted checking witness produced errors", { errors });
  if (expected.semantic_index) {
    const semantic = object(value.semantic_index, "check semantic_index");
    if (semantic.source_digest !== digestText(source)) fail("mismatch", "semantic index source digest does not identify entry bytes");
    const syntax = object(value.syntax, "check syntax");
    if (syntax.source !== source) fail("mismatch", "checked syntax tree did not preserve entry source bytes");
    const syntaxItems = checkSyntaxItems(syntax.items, source, "check syntax items", false, true);
    if (expected.syntax_items) checkExpectedItems(syntaxItems, expected.syntax_items, "check expected syntax items");
    const definitions = array(semantic.definitions, "check semantic definitions");
    const usedDefinitions = new Set();
    for (const [index, definition] of expected.definitions.entries()) {
      const matchIndex = definitions.findIndex((candidate, candidateIndex) =>
        !usedDefinitions.has(candidateIndex)
        && candidate?.name === definition.name
        && candidate?.module?.endsWith(definition.module_suffix));
      if (matchIndex < 0) fail("mismatch", `check semantic definitions omit ${definition.name}`);
      usedDefinitions.add(matchIndex);
      checkDefinition(definitions[matchIndex], sourceInfo, definition, `check definition ${index}`);
    }
    const references = array(semantic.references, "check semantic references");
    for (const [index, reference] of (expected.references ?? []).entries()) {
      const match = references.find((candidate) => candidate?.name === reference.name && (!reference.target_module_suffix || candidate?.target?.module?.endsWith(reference.target_module_suffix)));
      if (!match) fail("mismatch", `check semantic references omit ${reference.name}`);
      checkReference(match, sourceInfo, reference, `check reference ${index}`);
    }
    return {
      source_sha256: semantic.source_digest,
      diagnostics: diagnostics.map((diagnostic) => diagnostic.code),
      definition_count: definitions.length,
      reference_count: references.length,
    };
  }
  return { source_sha256: digestText(source), diagnostics: diagnostics.map((diagnostic) => diagnostic.code) };
}

function checkSourceFiles(entry) {
  object(entry.source, `${entry.id}.source`);
  const source = entry.source;
  const entryPath = text(source.entry, `${entry.id}.source.entry`);
  const refs = array(source.files, `${entry.id}.source.files`);
  if (refs.length === 0) fail("invalid_oracle", `${entry.id}.source.files is empty`);
  const files = refs.map((ref, index) => {
    const checked = digestRef(ref, `${entry.id}.source.files[${index}]`);
    return {
      path: checked.path,
      sha256: checked.sha256,
      source: readFileSync(checked.target, "utf8"),
      target: checked.target,
    };
  });
  if (new Set(files.map((file) => file.path)).size !== files.length) fail("invalid_oracle", `${entry.id}.source.files contains duplicate paths`);
  const entryFile = files.find((file) => file.path === entryPath);
  if (!entryFile) fail("invalid_oracle", `${entry.id}.source.entry is not listed in source.files`);
  const graphProjection = files
    .map((file) => ({ path: file.path, sha256: file.sha256 }))
    .sort((left, right) => left.path.localeCompare(right.path));
  if (source.graph_sha256 !== digestCanonical(graphProjection)) {
    fail("mismatch", `${entry.id}.source graph identity changed`, { expected: source.graph_sha256, observed: digestCanonical(graphProjection) });
  }
  if (source.entry_sha256 !== entryFile.sha256) fail("mismatch", `${entry.id}.source entry identity changed`);
  return {
    entryPath: entryFile.target,
    entryName: entryPath,
    source: entryFile.source,
    sha256: entryFile.sha256,
    files,
    graph_sha256: source.graph_sha256,
  };
}

function checkCaseMetadata(entry, index, ids) {
  object(entry, `source witness ${index}`);
  const id = text(entry.id, `source witness ${index}.id`);
  if (ids.has(id)) fail("invalid_oracle", `duplicate source witness ${id}`);
  ids.add(id);
  const category = text(entry.category, `${id}.category`);
  const operation = text(entry.operation, `${id}.operation`);
  if (!SOURCE_OPERATIONS.includes(operation)) fail("invalid_oracle", `${id}.operation is unsupported`);
  const obligations = array(entry.obligations, `${id}.obligations`);
  if (obligations.length === 0 || obligations.some((value) => !SOURCE_BOUNDARIES.includes(value))) fail("invalid_oracle", `${id}.obligations are not source boundaries`);
  const expected = object(entry.expect, `${id}.expect`);
  bool(expected.accepted, `${id}.expect.accepted`);
  if (expected.diagnostic !== undefined) text(expected.diagnostic, `${id}.expect.diagnostic`);
  if (expected.items !== undefined) array(expected.items, `${id}.expect.items`);
  if (expected.syntax_items !== undefined) array(expected.syntax_items, `${id}.expect.syntax_items`);
  if (expected.token_sequence !== undefined) array(expected.token_sequence, `${id}.expect.token_sequence`);
  if (expected.token_sequences !== undefined) {
    array(expected.token_sequences, `${id}.expect.token_sequences`).forEach((sequence, index) => {
      array(sequence, `${id}.expect.token_sequences[${index}]`);
    });
  }
  const sourceInfo = checkSourceFiles(entry);
  return { entry, id, category, operation, obligations, expected, sourceInfo };
}

function runCase(caseInfo, executeCompiler) {
  const { id, operation, expected, sourceInfo } = caseInfo;
  if (!executeCompiler) {
    return { id, status: "unverified", operation, source_sha256: sourceInfo.sha256, graph_sha256: sourceInfo.graph_sha256 };
  }
  const lexValue = runCompiler("lex", sourceInfo.entryPath);
  const lexReport = checkLexValue(lexValue, sourceInfo, {
    accepted: expected.accepted,
    token_sequence: expected.token_sequence,
    token_sequences: expected.token_sequences,
  });
  const value = runCompiler(operation, sourceInfo.entryPath);
  let report;
  if (operation === "lex") report = lexReport;
  else if (operation === "parse") report = checkParseValue(value, sourceInfo, expected);
  else report = checkCheckValue(value, sourceInfo, expected);
  return {
    id,
    status: "observed",
    operation,
    source_sha256: sourceInfo.sha256,
    graph_sha256: sourceInfo.graph_sha256,
    report,
    certificates: [...new Set([
      "exact_source_bytes",
      "token_spans_and_utf8_boundaries",
      "complete_input_consumption",
      ...(expected.token_sequence || expected.token_sequences ? ["precedence_token_order"] : []),
      ...(operation === "parse" || operation === "check" ? ["syntax_node_origins"] : []),
      ...(expected.semantic_index && expected.references?.some((reference) => reference.scope_required) ? ["nested_scope_facts"] : []),
      ...(sourceInfo.files.length > 1 ? ["qualified_import_graph"] : []),
      ...(expected.accepted === false ? ["registered_rejection"] : []),
    ])].sort(),
  };
}

function checkMutation(control, casesById, executeCompiler) {
  object(control, "source mutation control");
  const id = text(control.id, "source mutation control.id");
  const baseId = text(control.base_case, `${id}.base_case`);
  const base = casesById.get(baseId);
  if (!base) fail("invalid_oracle", `${id} names unknown base witness ${baseId}`);
  const mutation = object(control.mutation, `${id}.mutation`);
  if (mutation.kind !== "append") fail("unsupported", `${id} uses unsupported mutation kind`);
  const append = text(mutation.bytes, `${id}.mutation.bytes`);
  const baseBytes = Buffer.from(base.sourceInfo.source, "utf8");
  const mutatedBytes = Buffer.concat([baseBytes, Buffer.from(append, "utf8")]);
  const mutatedSourceSha = digestBytes(mutatedBytes);
  if (mutatedSourceSha !== text(control.mutated_source_sha256, `${id}.mutated_source_sha256`)) {
    fail("mismatch", `${id} mutation bytes changed`);
  }
  const expected = object(control.expect, `${id}.expect`);
  bool(expected.accepted, `${id}.expect.accepted`);
  if (expected.accepted !== false) fail("invalid_oracle", `${id} mutation must be a negative witness`);
  const expectedDiagnostic = text(expected.diagnostic, `${id}.expect.diagnostic`);
  if (!executeCompiler) {
    return { id, base_case: baseId, status: "unverified", mutated_source_sha256: mutatedSourceSha };
  }
  const scratch = resolve(process.env.JET_TEST_SCRATCH_DIR ?? join(homedir(), ".cache", "jet-source-proof"));
  const cacheRoot = resolve(homedir(), ".cache");
  if (scratch === "/tmp" || scratch.startsWith("/tmp/") || (scratch !== cacheRoot && !scratch.startsWith(`${cacheRoot}${sep}`))) {
    fail("unavailable", "source mutation scratch path is outside $HOME/.cache");
  }
  mkdirSync(scratch, { recursive: true });
  const tempRoot = mkdtempSync(join(scratch, "mutation-"));
  const target = join(tempRoot, "mutation.jet");
  writeFileSync(target, mutatedBytes);
  try {
    const value = runCompiler("parse", target);
    const payload = object(value, "mutation parse value");
    if (payload.source !== mutatedBytes.toString("utf8")) fail("mismatch", `${id} parser did not preserve mutated bytes`);
    const diagnostics = checkDiagnostics(payload.diagnostics, mutatedBytes, `${id} diagnostics`);
    if (!diagnostics.some((diagnostic) => diagnostic.code === expectedDiagnostic)) {
      fail("mismatch", `${id} did not reject the mutated suffix with ${expectedDiagnostic}`, { diagnostics });
    }
    return {
      id,
      base_case: baseId,
      status: "observed",
      mutated_source_sha256: mutatedSourceSha,
      diagnostic: expectedDiagnostic,
      certificates: ["mutation_identity", "registered_rejection", "complete_input_consumption"],
    };
  } finally {
    rmSync(tempRoot, { recursive: true, force: true });
  }
}

function checkCoverage(cases, controls, contract) {
  const references = new Map(SOURCE_BOUNDARIES.map((id) => []));
  for (const entry of cases) {
    for (const obligation of entry.obligations) {
      const list = references.get(obligation);
      if (!list.includes(entry.id)) list.push(entry.id);
    }
  }
  for (const id of SOURCE_BOUNDARIES) {
    if (references.get(id).length === 0) fail("invalid_oracle", `source boundary ${id} has no retained witness`);
  }
  return {
    relation: contract.coverage.relation_path,
    denominator: SOURCE_BOUNDARIES.length,
    complete_denominator: true,
    rows: SOURCE_BOUNDARIES.map((id) => ({ id, witness: references.get(id).sort(), status: "mapped_unproved" })),
    status_counts: { mapped_unproved: SOURCE_BOUNDARIES.length, unsupported: 0, unmapped: 0, proved: 0 },
    mapped_unproved: [...SOURCE_BOUNDARIES],
    unsupported: [],
    unmapped: [],
    proved: [],
    controls: controls.map((control) => control.id),
    qualification: "blocked",
  };
}

function checkWitnesses(witnesses, contract, executeCompiler) {
  object(witnesses, "source witnesses");
  if (witnesses.schema !== "jet.source-witnesses.v1" || witnesses.schema_version !== 1 || witnesses.relation !== "source_processing") {
    fail("invalid_oracle", "source witness schema/version/relation is unsupported");
  }
  if (!deepEqual(array(witnesses.operations, "source witnesses.operations"), SOURCE_OPERATIONS)) fail("invalid_oracle", "source witness operation list drifted");
  const cases = array(witnesses.cases, "source witness cases");
  if (cases.length === 0) fail("empty", "source witness corpus is empty");
  const ids = new Set();
  const normalized = cases.map((entry, index) => checkCaseMetadata(entry, index, ids));
  const casesById = new Map(normalized.map((entry) => [entry.id, entry]));
  const reports = normalized.map((entry) => runCase(entry, executeCompiler));
  const controls = array(witnesses.mutations, "source witness mutations").map((control) => checkMutation(control, casesById, executeCompiler));
  const categories = [...new Set(normalized.map((entry) => entry.category))].sort();
  for (const category of ["bytes", "lexing", "parsing", "scope", "graph", "rejection"]) {
    if (!categories.includes(category)) fail("invalid_oracle", `source witness corpus lacks ${category} coverage`);
  }
  return { normalized, reports, controls, categories };
}

function runCheck({ executeCompiler = true } = {}) {
  const contract = readJson(CONTRACT_PATH, "source contract");
  const checkedContract = checkContract(contract);
  const witnesses = readJson(WITNESSES_PATH, "source witness corpus");
  const binding = checkBinding(contract, witnesses);
  const identity = checkProofIdentity(checkedContract.identity);
  const obligations = checkObligations(checkedContract);
  const witnessResult = checkWitnesses(witnesses, contract, executeCompiler);
  const coverage = checkCoverage(witnessResult.normalized, witnessResult.controls, checkedContract);
  const status = executeCompiler ? "matched" : "matched";
  return {
    schema: CHECK_SCHEMA,
    status,
    contract: {
      id: contract.contract_id,
      schema: contract.schema,
      schema_version: contract.schema_version,
      claim: checkedContract.claim.kind,
      certificates: SOURCE_CERTIFICATES,
    },
    identity,
    binding: {
      checker: binding.checker,
      driver: binding.driver,
      authority_count: binding.authorities,
      implementation_file_count: binding.implementation,
      obligations_sha256: binding.obligations.sha256,
    },
    obligations,
    witnesses: {
      count: witnessResult.reports.length,
      categories: witnessResult.categories,
      mutation_count: witnessResult.controls.length,
      reports: witnessResult.reports,
      mutations: witnessResult.controls,
    },
    compiler_execution: {
      status: executeCompiler ? "observed" : "unverified",
      requested: executeCompiler,
      runner: RUNNER_PATH,
      compiler: COMPILER_PATH,
      timeout_ms: TIMEOUT_MS,
    },
    comparison: {
      schema_version: 1,
      status,
      relation: "source_processing",
      samples: witnessResult.reports,
      discarded_cases: 0,
      first_difference: null,
      reduced_counterexample: null,
      contamination: null,
      reason: executeCompiler ? "retained source witnesses matched canonical compiler observations" : "structural source witness checks passed; compiler observation was skipped",
      universal_proof: false,
    },
    coverage,
    proof_boundary: {
      ...canonical(checkedContract.boundary),
      qualification: "blocked",
      universal_proof: false,
      compiler_execution_status: executeCompiler ? "observed" : "unverified",
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
  process.stdout.write(`source contract: ${result.status}\n`);
  if (result.identity) process.stdout.write(`candidate: ${result.identity.claim_id} ${result.identity.identity_sha256}\n`);
  if (result.witnesses) process.stdout.write(`witnesses: ${result.witnesses.count}; mutations=${result.witnesses.mutation_count}\n`);
  if (result.coverage) process.stdout.write(`coverage: ${result.coverage.denominator} source boundaries; qualification=${result.coverage.qualification}\n`);
  if (result.compiler_execution) process.stdout.write(`compiler: ${result.compiler_execution.status}\n`);
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}
export async function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const skipCompiler = argv.includes("--skip-compiler");
  const allowed = new Set(["--json", "--skip-compiler"]);
  if (argv.some((arg) => !allowed.has(arg))) {
    const result = errorPayload(new CheckError("invalid_oracle", "usage: source/check.mjs [--skip-compiler] [--json]"));
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else printHuman(result);
    return 64;
  }
  try {
    const result = runCheck({ executeCompiler: !skipCompiler });
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
  checkObligations,
  checkWitnesses,
  runCheck,
};
