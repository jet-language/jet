#!/usr/bin/env node

/**
 * Pinned compiler-proof replay maintenance tool.
 *
 * The compiler-proof claim entry point remains --replay. The semantics stage
 * is routed to its canonical checker so this tool does not duplicate semantic
 * contract logic. The claim verifier consumes a row (or a row in the checked-
 * in manifest), verifies every recorded identity against current inputs, and
 * asks the pinned Lean binary to check the proof source. A producer result flag
 * is never used as proof.
 */

import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DEFAULT_MANIFEST = "proof/compiler/toolchain/manifest.json";
const SCHEMA = "jet.compiler-proof.v1";
const DIGEST = /^sha256-[0-9a-f]{64}$/;
const NIX_SRI = /^sha256-[A-Za-z0-9+/]{43}=$/;
const NIX_STORE = /^\/nix\/store\/[a-z0-9]{32}-lean4-[0-9]+\.[0-9]+\.[0-9]+$/;
const COMMIT = /^[0-9a-f]{40}$/;
const METHODS = new Set([
  "verified_construction",
  "direct_verification",
  "translation_certificate",
]);
const RECORDED_RESULTS = new Set([
  "proved",
  "unknown",
  "unavailable",
  "timeout",
  "unsupported",
]);
const ASSUMPTION_KINDS = [
  "model_validity",
  "preservation",
  "implementation_correspondence",
  "compiler_construction",
];
const CHECKER_ENV = Object.freeze({
  locale: "C",
  lang: "C",
  timezone: "UTC",
  home: "",
  path_policy: "pinned-store-only",
});
const EXIT = Object.freeze({
  OK: 0,
  REJECTED: 1,
  USAGE: 2,
  UNAVAILABLE: 3,
  TIMEOUT: 4,
});
const STAGE_DEFINITIONS = Object.freeze({
  semantics: Object.freeze({
    checker: "proof/compiler/semantics/check.mjs",
    schema: "jet.semantic-contract-check.v1",
    label: "semantic",
  }),
  source: Object.freeze({
    checker: "proof/compiler/source/check.mjs",
    schema: "jet.source-contract-check.v1",
    label: "source",
  }),
  checking: Object.freeze({
    checker: "proof/compiler/checking/check.mjs",
    schema: "jet.checking-contract-check.v1",
    label: "checking",
  }),
  lowering: Object.freeze({
    checker: "proof/compiler/lowering/check.mjs",
    schema: "jet.lowering-contract-check.v1",
    label: "lowering",
  }),
  comptime: Object.freeze({
    checker: "proof/compiler/comptime/check.mjs",
    schema: "jet.comptime-contract-check.v1",
    label: "comptime",
  }),
  adapters: Object.freeze({
    checker: "proof/compiler/adapters/check.mjs",
    schema: "jet.adapters-contract-check.v1",
    label: "adapters",
  }),
  optimization: Object.freeze({
    checker: "proof/compiler/optimization/check.mjs",
    schema: "jet.optimization-contract-check.v1",
    label: "optimization",
  }),
  prelude: Object.freeze({
    checker: "proof/compiler/prelude/check.mjs",
    schema: "jet.prelude-contract-check.v1",
    label: "prelude",
  }),
  runtime: Object.freeze({
    checker: "proof/compiler/runtime/check.mjs",
    schema: "jet.runtime-contract-check.v1",
    label: "runtime",
  }),
});
const STAGE_SUCCESS_STATUSES = new Set(["matched", "valid"]);
const STAGE_STATUS_EXIT = Object.freeze({
  unavailable: EXIT.UNAVAILABLE,
  timeout: EXIT.TIMEOUT,
  unknown: EXIT.REJECTED,
  unverified: EXIT.REJECTED,
  blocked: EXIT.REJECTED,
  mismatch: EXIT.REJECTED,
  invalid_oracle: EXIT.REJECTED,
  rejected: EXIT.REJECTED,
  unsupported: EXIT.REJECTED,
  failed: EXIT.REJECTED,
  cancelled: EXIT.REJECTED,
  empty: EXIT.REJECTED,
  contaminated: EXIT.REJECTED,
});

class ProofError extends Error {
  constructor(code, message, component = null, details = undefined) {
    super(message);
    this.name = "ProofError";
    this.code = code;
    this.component = component;
    this.details = details;
  }
}
function proofScratchRoot() {
  const configured = process.env.JET_TEST_SCRATCH_DIR
    ?? process.env.JET_TEST_SCRATCH
    ?? join(homedir(), ".cache", "jet-test-scratch");
  const root = resolve(configured);
  if (root === "/tmp" || root.startsWith("/tmp/")) {
    throw new ProofError("unavailable", "compiler-proof scratch must not use /tmp", "proof.scratch");
  }
  mkdirSync(root, { recursive: true, mode: 0o700 });
  const canonicalRoot = realpathSync(root);
  if (canonicalRoot === "/tmp" || canonicalRoot.startsWith("/tmp/")) {
    throw new ProofError("unavailable", "compiler-proof scratch resolves under /tmp", "proof.scratch");
  }
  return canonicalRoot;
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    const sorted = {};
    for (const key of Object.keys(value).sort()) sorted[key] = canonical(value[key]);
    return sorted;
  }
  return value;
}

function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

function digestBytes(bytes) {
  return `sha256-${createHash("sha256").update(bytes).digest("hex")}`;
}

function digestText(text) {
  return digestBytes(Buffer.from(text, "utf8"));
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new ProofError("invalid-schema", `${label} must be an object`, label);
  }
  return value;
}

function text(value, label) {
  if (typeof value !== "string" || value.length === 0 || /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new ProofError("invalid-schema", `${label} must be non-empty text`, label);
  }
  return value;
}

function digest(value, label) {
  text(value, label);
  if (!DIGEST.test(value)) {
    throw new ProofError("invalid-schema", `${label} must be a lowercase SHA-256 digest`, label);
  }
  return value;
}

function relativeProjectPath(value, label) {
  text(value, label);
  if (isAbsolute(value) || value.includes("\\") || value.includes("\0")) {
    throw new ProofError("invalid-schema", `${label} must be a project-relative path`, label);
  }
  const absolute = resolve(ROOT, value);
  const rootPrefix = `${ROOT}${sep}`;
  if (absolute !== ROOT && !absolute.startsWith(rootPrefix)) {
    throw new ProofError("invalid-schema", `${label} escapes the project root`, label);
  }
  return value;
}

function absoluteProjectPath(value, label) {
  relativeProjectPath(value, label);
  return resolve(ROOT, value);
}

function readJson(path, label) {
  let bytes;
  try {
    bytes = readFileSync(path);
  } catch (error) {
    throw new ProofError("missing-artifact", `cannot read ${label}: ${error.message}`, label);
  }
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    throw new ProofError("invalid-proof", `${label} is not valid JSON: ${error.message}`, label);
  }
}

function regularFile(ref, label) {
  const path = absoluteProjectPath(ref.path, `${label}.path`);
  let stat;
  try {
    stat = lstatSync(path);
  } catch (error) {
    throw new ProofError("missing-artifact", `${label} is missing: ${ref.path}`, label);
  }
  if (stat.isSymbolicLink() || !stat.isFile()) {
    throw new ProofError("invalid-artifact", `${label} is not a regular file: ${ref.path}`, label);
  }
  return path;
}

function verifyFileRef(ref, label) {
  object(ref, label);
  const path = regularFile(ref, label);
  digest(ref.sha256, `${label}.sha256`);
  const actual = digestBytes(readFileSync(path));
  if (actual !== ref.sha256) {
    throw new ProofError(
      "identity-mismatch",
      `${label} digest changed (recorded ${ref.sha256}, observed ${actual})`,
      `${label}.sha256`,
      { recorded: ref.sha256, observed: actual, path: ref.path },
    );
  }
  return { path, digest: actual };
}

function verifyTextDigest(value, recorded, label) {
  digest(recorded, `${label}.sha256`);
  const actual = digestText(value);
  if (actual !== recorded) {
    throw new ProofError(
      "identity-mismatch",
      `${label} changed (recorded ${recorded}, observed ${actual})`,
      `${label}.sha256`,
      { recorded, observed: actual },
    );
  }
}

function toolchainWithoutPin(toolchain) {
  const { pin_sha256: _ignored, ...withoutPin } = toolchain;
  return withoutPin;
}

function toolchainPin(toolchain) {
  return digestText(canonicalJson(toolchainWithoutPin(toolchain)));
}

function validateToolchain(toolchain) {
  object(toolchain, "toolchain");
  if (toolchain.checker !== "lean") {
    throw new ProofError("invalid-schema", "toolchain.checker must be lean", "toolchain.checker");
  }
  text(toolchain.package, "toolchain.package");
  text(toolchain.version, "toolchain.version");
  if (!/^4\.[0-9]+\.[0-9]+$/u.test(toolchain.version)) {
    throw new ProofError("invalid-schema", "toolchain.version must be a full Lean 4 version", "toolchain.version");
  }
  if (!NIX_STORE.test(toolchain.store_path)) {
    throw new ProofError(
      "invalid-schema",
      "toolchain.store_path must pin the Lean 4 Nix store output",
      "toolchain.store_path",
    );
  }
  if (!toolchain.store_path.endsWith(`-lean4-${toolchain.version}`)) {
    throw new ProofError(
      "identity-mismatch",
      `toolchain store output does not contain the pinned version ${toolchain.version}`,
      "toolchain.store_path",
    );
  }
  if (toolchain.executable !== "bin/lean") {
    throw new ProofError("invalid-schema", "toolchain.executable must be bin/lean", "toolchain.executable");
  }
  if (canonicalJson(toolchain.version_args) !== canonicalJson(["--version"])) {
    throw new ProofError("invalid-schema", "toolchain.version_args must be [--version]", "toolchain.version_args");
  }
  if (typeof toolchain.version_output !== "string" || toolchain.version_output.includes("\0")) {
    throw new ProofError("invalid-schema", "toolchain.version_output must be text", "toolchain.version_output");
  }
  if (toolchain.version_output !== `Lean (version ${toolchain.version}, commit v${toolchain.version}, Release)\n`) {
    throw new ProofError(
      "invalid-schema",
      "toolchain.version_output is not the pinned Lean version output",
      "toolchain.version_output",
    );
  }
  verifyTextDigest(toolchain.version_output, toolchain.version_output_sha256, "toolchain.version_output");
  if (toolchain.version_stderr !== "") {
    throw new ProofError("invalid-schema", "toolchain.version_stderr must be empty", "toolchain.version_stderr");
  }
  verifyTextDigest(toolchain.version_stderr, toolchain.version_stderr_sha256, "toolchain.version_stderr");
  const nixpkgs = object(toolchain.nixpkgs, "toolchain.nixpkgs");
  text(nixpkgs.input, "toolchain.nixpkgs.input");
  if (nixpkgs.input !== "proofNixpkgs") {
    throw new ProofError("invalid-schema", "toolchain.nixpkgs.input must be proofNixpkgs", "toolchain.nixpkgs.input");
  }
  if (typeof nixpkgs.rev !== "string" || !/^[0-9a-f]{40}$/u.test(nixpkgs.rev)) {
    throw new ProofError("invalid-schema", "toolchain.nixpkgs.rev must be a commit", "toolchain.nixpkgs.rev");
  }
  text(nixpkgs.nar_hash, "toolchain.nixpkgs.nar_hash");
  if (!NIX_SRI.test(nixpkgs.nar_hash)) {
    throw new ProofError("invalid-schema", "toolchain.nixpkgs.nar_hash must be a Nix SRI SHA-256 digest", "toolchain.nixpkgs.nar_hash");
  }
  digest(toolchain.pin_sha256, "toolchain.pin_sha256");
  const expectedPin = toolchainPin(toolchain);
  if (toolchain.pin_sha256 !== expectedPin) {
    throw new ProofError(
      "identity-mismatch",
      `toolchain pin changed (recorded ${toolchain.pin_sha256}, observed ${expectedPin})`,
      "toolchain.pin_sha256",
      { recorded: toolchain.pin_sha256, observed: expectedPin },
    );
  }
  return expectedPin;
}

function validateRecordIdentity(recordIdentity, label) {
  object(recordIdentity, label);
  digest(recordIdentity.target_inputs_sha256, `${label}.target_inputs_sha256`);
  text(recordIdentity.tool_version, `${label}.tool_version`);
  text(recordIdentity.engine, `${label}.engine`);
}

function validateAssumptions(assumptions) {
  if (!Array.isArray(assumptions)) {
    throw new ProofError("invalid-schema", "assumptions must be an array", "assumptions");
  }
  const seen = new Set();
  for (const [index, assumption] of assumptions.entries()) {
    object(assumption, `assumptions[${index}]`);
    const kind = text(assumption.kind, `assumptions[${index}].kind`);
    if (!ASSUMPTION_KINDS.includes(kind)) {
      throw new ProofError("invalid-schema", `unknown assumption kind ${kind}`, `assumptions[${index}].kind`);
    }
    if (seen.has(kind)) {
      throw new ProofError("invalid-schema", `duplicate assumption kind ${kind}`, `assumptions[${index}].kind`);
    }
    seen.add(kind);
    text(assumption.id, `assumptions[${index}].id`);
    text(assumption.statement, `assumptions[${index}].statement`);
    if (assumption.status !== "assumed") {
      throw new ProofError(
        "invalid-schema",
        `assumption ${kind} must be explicitly marked assumed`,
        `assumptions[${index}].status`,
      );
    }
  }
  for (const kind of ASSUMPTION_KINDS) {
    if (!seen.has(kind)) {
      throw new ProofError("invalid-schema", `missing ${kind} assumption`, "assumptions");
    }
  }
}

function stripLeanComments(source) {
  let result = source.replace(/\/\*[\s\S]*?\*\//gu, "");
  result = result.replace(/--[^\n\r]*/gu, "");
  return result;
}

function quotedNeedle(source, value, label) {
  if (!source.includes(JSON.stringify(value))) {
    throw new ProofError(
      "invalid-proof",
      `${label} is not bound in the checked Lean artifact`,
      `proof.artifact.${label}`,
    );
  }
}

function identifier(value, label) {
  text(value, label);
  if (!/^[A-Za-z_][A-Za-z0-9_']*$/u.test(value)) {
    throw new ProofError("invalid-schema", `${label} must be a Lean identifier`, label);
  }
  return value;
}

function validateProofTrust(proofSource) {
  const active = stripLeanComments(proofSource);
  const forbidden = /\b(?:axiom|sorry|admit|by_contra!?|unsafe|opaque|implemented_by|extern|foreign|run_tac|run_cmd|elab|macro|syntax|command|quote|unquote)\b|^\s*#\s*(?:exit|eval|reduce|check|print|guard|synth)\b/gmu;
  const match = forbidden.exec(active);
  if (match) {
    throw new ProofError(
      "non-proof",
      `proof artifact contains an untrusted command or admitted obligation: ${match[0].trim()}`,
      "proof.artifact",
    );
  }
  const contextMutation = /^\s*(?:namespace|section|end|variable|include|open|notation|infix|prefix|postfix|scoped|attribute|local|set_option)\b/gmu;
  const contextMatch = contextMutation.exec(active);
  if (contextMatch) {
    throw new ProofError(
      "non-proof",
      `proof artifact mutates the trusted elaboration context: ${contextMatch[0].trim()}`,
      "proof.artifact",
    );
  }
  if (/\bimport\b/u.test(active)) {
    throw new ProofError(
      "non-proof",
      "proof artifact imports undeclared external assumptions",
      "proof.artifact",
    );
  }
}

function validateCorrespondence(claim, proofSource) {
  const correspondence = object(claim.correspondence, "correspondence");
  if (!METHODS.has(correspondence.method)) {
    throw new ProofError(
      "invalid-binding",
      `unsupported correspondence method ${String(correspondence.method)}; digest-only claims are not proof`,
      "correspondence.method",
    );
  }
  text(correspondence.route, "correspondence.route");
  identifier(correspondence.checked_declaration, "correspondence.checked_declaration");
  const active = stripLeanComments(proofSource);
  quotedNeedle(active, claim.id, "claim id");
  quotedNeedle(active, claim.proposition, "proposition");
  quotedNeedle(active, claim.proof.formal_goal, "formal goal");
  quotedNeedle(active, claim.model.path, "model path");
  quotedNeedle(active, claim.model.sha256, "model digest");
  quotedNeedle(active, claim.implementation.path, "implementation path");
  quotedNeedle(active, claim.implementation.sha256, "implementation digest");
  quotedNeedle(active, correspondence.method, "correspondence method");
  const declaration = correspondence.checked_declaration.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  const declarationPattern = new RegExp(`\\btheorem\\s+${declaration}\\b`, "u");
  if (!declarationPattern.test(active)) {
    throw new ProofError(
      "invalid-binding",
      `Lean artifact does not declare checked correspondence ${correspondence.checked_declaration}`,
      "correspondence.checked_declaration",
    );
  }
  const bindingPattern = new RegExp(
    `${declaration}[\\s\\S]{0,512}(?:jetImplementation|jetModel)`,
    "u",
  );
  if (!bindingPattern.test(active)) {
    throw new ProofError(
      "invalid-binding",
      "checked correspondence declaration is detached from implementation and model",
      "correspondence.checked_declaration",
    );
  }
  const methodDeclaration = new RegExp(
    `(?:def|theorem)\\s+jetCompilerProofMethod\\b[\\s\\S]{0,256}${correspondence.method}`,
    "u",
  );
  if (!methodDeclaration.test(active)) {
    throw new ProofError(
      "invalid-binding",
      "Lean artifact does not expose the selected correspondence route",
      "correspondence.method",
    );
  }
}

function identityProjection(claim) {
  return {
    claim: {
      id: claim.id,
      proposition: claim.proposition,
      proposition_sha256: claim.proposition_sha256,
    },
    model: claim.model,
    source: claim.source,
    implementation: claim.implementation,
    correspondence: claim.correspondence,
    candidate: claim.candidate,
    checker: claim.checker,
    assumptions: claim.assumptions,
    proof: { artifact: claim.proof.artifact },
  };
}

function identityDigest(claim) {
  return digestText(canonicalJson(identityProjection(claim)));
}

function validateCandidate(candidate, source) {
  object(candidate, "candidate");
  validateRecordIdentity(candidate.record_identity, "candidate.record_identity");
  text(candidate.commit, "candidate.commit");
  const compiler = object(candidate.compiler, "candidate.compiler");
  verifyFileRef(compiler.source, "candidate.compiler.source");
  verifyFileRef(compiler.artifact, "candidate.compiler.artifact");
  verifyFileRef(candidate.configuration, "candidate.configuration");
  const target = object(candidate.target, "candidate.target");
  text(target.triple, "candidate.target.triple");
  text(target.arch, "candidate.target.arch");
  text(target.os, "candidate.target.os");
  text(target.backend, "candidate.target.backend");
  text(target.profile, "candidate.target.profile");
  const sourceDigest = digestBytes(readFileSync(source));
  if (candidate.record_identity.target_inputs_sha256 !== sourceDigest) {
    throw new ProofError(
      "identity-mismatch",
      `candidate target input closure changed (recorded ${candidate.record_identity.target_inputs_sha256}, observed ${sourceDigest})`,
      "candidate.record_identity.target_inputs_sha256",
      { recorded: candidate.record_identity.target_inputs_sha256, observed: sourceDigest },
    );
  }
}

function validateRawReplay(rawReplay, { allowNonProof = false } = {}) {
  object(rawReplay, "raw_replay");
  verifyFileRef(rawReplay.stdout, "raw_replay.stdout");
  verifyFileRef(rawReplay.stderr, "raw_replay.stderr");
  if (!Number.isInteger(rawReplay.exit_code) || rawReplay.exit_code < 0) {
    throw new ProofError("invalid-schema", "raw_replay.exit_code must be a non-negative integer", "raw_replay.exit_code");
  }
  if (rawReplay.exit_code !== 0 && !allowNonProof) {

    throw new ProofError("non-proof", "recorded checker exit is not successful proof evidence", "raw_replay.exit_code");
  }
}
function validateApprovedAuthority(manifest, authority, claim) {
  const requested = object(authority, "replay authority");
  const authorityKey = text(requested.authority_key, "replay authority.authority_key");
  const obligationId = text(requested.obligation_id, "replay authority.obligation_id");
  if (claim.authority !== undefined) {
    const claimAuthority = object(claim.authority, "claim.authority");
    const claimAuthorityKey = text(claimAuthority.authority_key, "claim.authority.authority_key");
    const claimObligationId = text(claimAuthority.obligation_id, "claim.authority.obligation_id");
    if (claimAuthorityKey !== authorityKey || claimObligationId !== obligationId) {
      throw new ProofError("identity-mismatch", "claim authority does not select the requested approved obligation", "claim.authority");
    }
  }
  const registry = object(manifest.approved_authorities, "manifest.approved_authorities");
  const approved = object(registry[authorityKey], `manifest.approved_authorities.${authorityKey}`);
  if (approved.schema !== "jet.compiler-proof-authority.v1") {
    throw new ProofError("invalid-schema", `approved authority ${authorityKey} has an unsupported schema`, "manifest.approved_authorities");
  }
  if (approved.stage !== undefined) text(approved.stage, `manifest.approved_authorities.${authorityKey}.stage`);
  const obligationsRef = {
    path: text(approved.obligations_path, `manifest.approved_authorities.${authorityKey}.obligations_path`),
    sha256: digest(approved.obligations_sha256, `manifest.approved_authorities.${authorityKey}.obligations_sha256`),
  };
  verifyFileRef(obligationsRef, `manifest.approved_authorities.${authorityKey}.obligations`);
  let contractRef = null;
  if (approved.contract_path !== undefined || approved.contract_sha256 !== undefined) {
    contractRef = {
      path: text(approved.contract_path, `manifest.approved_authorities.${authorityKey}.contract_path`),
      sha256: digest(approved.contract_sha256, `manifest.approved_authorities.${authorityKey}.contract_sha256`),
    };
    verifyFileRef(contractRef, `manifest.approved_authorities.${authorityKey}.contract`);
    const contract = readJson(absoluteProjectPath(contractRef.path, `${authorityKey}.contract`), `${authorityKey} contract`);
    if (approved.stage !== undefined && contract.stage !== approved.stage) {
      throw new ProofError("identity-mismatch", `approved authority ${authorityKey} names a different stage contract`, "manifest.approved_authorities");
    }
    if (contract.identity?.authority_key !== authorityKey) {
      throw new ProofError("identity-mismatch", `approved authority ${authorityKey} does not match the stage contract authority selector`, "manifest.approved_authorities");
    }
  } else if (authorityKey !== "compiler-proof.identity" || approved.manifest_path !== DEFAULT_MANIFEST) {
    throw new ProofError("invalid-schema", `approved authority ${authorityKey} has no canonical stage contract`, "manifest.approved_authorities");
  }
  const goals = object(approved.goals, `manifest.approved_authorities.${authorityKey}.goals`);
  const goal = goals[obligationId];
  if (!goal || typeof goal !== "object" || Array.isArray(goal)) {
    throw new ProofError("unavailable", `approved authority ${authorityKey} has no semantic goal for ${obligationId}`, "replay authority.obligation_id");
  }
  if (goal.claim_id !== undefined && goal.claim_id !== claim.id) {
    throw new ProofError("identity-mismatch", `approved authority ${authorityKey} goal is bound to another claim`, "replay authority.obligation_id");
  }
  const model = object(goal.model, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.model`);
  const modelRef = {
    path: text(model.path, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.model.path`),
    sha256: digest(model.sha256, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.model.sha256`),
  };
  verifyFileRef(modelRef, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.model`);
  const proposition = text(goal.proposition, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.proposition`);
  const formalGoal = text(goal.formal_goal, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.formal_goal`);
  const implementation = object(goal.implementation, `manifest.approved_authorities.${authorityKey}.goals.${obligationId}.implementation`);
  text(implementation.id, `${authorityKey}.${obligationId}.implementation.id`);
  text(implementation.representation, `${authorityKey}.${obligationId}.implementation.representation`);
  const implementationRef = {
    ...implementation,
    path: text(implementation.path, `${authorityKey}.${obligationId}.implementation.path`),
    sha256: digest(implementation.sha256, `${authorityKey}.${obligationId}.implementation.sha256`),
  };
  verifyFileRef(implementationRef, `${authorityKey}.${obligationId}.implementation`);
  const correspondence = object(goal.correspondence, `${authorityKey}.${obligationId}.correspondence`);
  text(correspondence.method, `${authorityKey}.${obligationId}.correspondence.method`);
  text(correspondence.route, `${authorityKey}.${obligationId}.correspondence.route`);
  const goalDeclaration = identifier(goal.goal_declaration, `${authorityKey}.${obligationId}.goal_declaration`);
  identifier(correspondence.checked_declaration, `${authorityKey}.${obligationId}.correspondence.checked_declaration`);
  if (claim.proposition !== proposition || claim.proof.formal_goal !== formalGoal) {
    throw new ProofError("identity-mismatch", "claim semantic goal differs from the approved obligation goal", "replay authority");
  }
  if (claim.model.path !== modelRef.path || claim.model.sha256 !== modelRef.sha256) {
    throw new ProofError("identity-mismatch", "claim model differs from the approved obligation model", "replay authority.model");
  }
  if (canonicalJson(claim.implementation) !== canonicalJson(implementationRef)) {
    throw new ProofError("identity-mismatch", "claim implementation differs from the approved obligation implementation", "replay authority.implementation");
  }
  if (canonicalJson(claim.correspondence) !== canonicalJson(correspondence)) {
    throw new ProofError("identity-mismatch", "claim correspondence differs from the approved obligation correspondence", "replay authority.correspondence");
  }
  return {
    authority_key: authorityKey,
    obligation_id: obligationId,
    contract: contractRef,
    obligations: obligationsRef,
    model: modelRef,
    proposition,
    formal_goal: formalGoal,
    goal_declaration: goalDeclaration,
    implementation: implementationRef,
    correspondence,
    ...(goal.candidate === undefined ? {} : { candidate: goal.candidate }),
  };
}

function validateClaim(claim, toolchainPinValue, { checkFiles = true, allowNonProof = false } = {}) {
  object(claim, "claim");
  text(claim.id, "claim.id");
  text(claim.proposition, "claim.proposition");
  verifyTextDigest(claim.proposition, claim.proposition_sha256, "claim.proposition");
  object(claim.model, "model");
  text(claim.model.id, "model.id");
  relativeProjectPath(claim.model.path, "model.path");
  digest(claim.model.sha256, "model.sha256");
  object(claim.source, "source");
  relativeProjectPath(claim.source.path, "source.path");
  digest(claim.source.sha256, "source.sha256");
  object(claim.implementation, "implementation");
  text(claim.implementation.id, "implementation.id");
  text(claim.implementation.representation, "implementation.representation");
  relativeProjectPath(claim.implementation.path, "implementation.path");
  digest(claim.implementation.sha256, "implementation.sha256");
  const correspondence = object(claim.correspondence, "correspondence");
  text(correspondence.route, "correspondence.route");
  identifier(correspondence.checked_declaration, "correspondence.checked_declaration");
  object(claim.checker, "checker");
  if (claim.checker.protocol !== "lean-source-v1") {
    throw new ProofError("invalid-schema", "checker.protocol must be lean-source-v1", "checker.protocol");
  }
  if (claim.checker.toolchain_pin_sha256 !== toolchainPinValue) {
    throw new ProofError(
      "identity-mismatch",
      `claim names a different checker toolchain (recorded ${claim.checker.toolchain_pin_sha256}, manifest ${toolchainPinValue})`,
      "checker.toolchain_pin_sha256",
      { recorded: claim.checker.toolchain_pin_sha256, observed: toolchainPinValue },
    );
  }
  digest(claim.checker.toolchain_pin_sha256, "checker.toolchain_pin_sha256");
  if (canonicalJson(claim.checker.invocation) !== canonicalJson(["lean", "<proof-artifact>"])) {
    throw new ProofError(
      "invalid-schema",
      "checker.invocation must use the pinned Lean proof-artifact protocol",
      "checker.invocation",
    );
  }
  validateAssumptions(claim.assumptions);
  object(claim.proof, "proof");
  if (claim.proof.artifact?.kind !== "lean-source") {
    throw new ProofError("invalid-schema", "proof.artifact.kind must be lean-source", "proof.artifact.kind");
  }
  const proofPath = relativeProjectPath(claim.proof.artifact.path, "proof.artifact.path");
  digest(claim.proof.artifact.sha256, "proof.artifact.sha256");
  text(claim.proof.formal_goal, "proof.formal_goal");
  if (!Number.isInteger(claim.proof.budget_ms) || claim.proof.budget_ms <= 0 || claim.proof.budget_ms > 300_000) {
    throw new ProofError("non-proof", "proof checker budget is missing or outside the fail-closed range", "proof.budget_ms");
  }
  object(claim.candidate, "candidate");
  if (checkFiles) {
    const sourcePath = regularFile(claim.source, "source");
    verifyFileRef(claim.source, "source");
    verifyFileRef(claim.model, "model");
    verifyFileRef(claim.implementation, "implementation");
    const proof = verifyFileRef(claim.proof.artifact, "proof.artifact");
    if (proofPath !== claim.proof.artifact.path) {
      throw new ProofError("invalid-schema", "proof artifact path normalization changed", "proof.artifact.path");
    }
    validateCandidate(claim.candidate, sourcePath);
    const proofSource = readFileSync(proof.path, "utf8");
    validateProofTrust(proofSource);
    validateCorrespondence(claim, proofSource);
    validateRawReplay(claim.raw_replay, { allowNonProof });
  }
  if (typeof claim.result !== "string" || !RECORDED_RESULTS.has(claim.result)) {
    throw new ProofError("invalid-schema", `unknown claim result ${String(claim.result)}`, "result");
  }
  if (!allowNonProof && claim.result !== "proved") {
    throw new ProofError(
      "non-proof",
      `claim result ${String(claim.result)} is not a checked proof`,
      "result",
    );
  }
  digest(claim.identity_sha256, "claim.identity_sha256");
  const expectedIdentity = identityDigest(claim);
  if (claim.identity_sha256 !== expectedIdentity) {
    throw new ProofError(
      "identity-mismatch",
      `claim identity changed (recorded ${claim.identity_sha256}, observed ${expectedIdentity})`,
      "claim.identity_sha256",
      { recorded: claim.identity_sha256, observed: expectedIdentity },
    );
  }
  return { identity_sha256: expectedIdentity };
}

function validateManifest(manifest) {
  object(manifest, "manifest");
  if (manifest.schema !== SCHEMA) {
    throw new ProofError("invalid-schema", `manifest schema must be ${SCHEMA}`, "manifest.schema");
  }
  if (manifest.schema_version !== 1) {
    throw new ProofError("invalid-schema", "manifest.schema_version must be 1", "manifest.schema_version");
  }
  text(manifest.manifest_id, "manifest.manifest_id");
  if (canonicalJson(manifest.binding_methods) !== canonicalJson([...METHODS])) {
    throw new ProofError(
      "invalid-schema",
      "manifest.binding_methods must enumerate the three ratified correspondence methods",
      "manifest.binding_methods",
    );
  }
  const pin = validateToolchain(manifest.toolchain);
  if (!Array.isArray(manifest.claims) || manifest.claims.length === 0) {
    throw new ProofError("invalid-schema", "manifest.claims must contain at least one claim", "manifest.claims");
  }
  const ids = new Set();
  const summaries = [];
  for (const claim of manifest.claims) {
    if (ids.has(claim?.id)) throw new ProofError("invalid-schema", `duplicate claim ${claim.id}`, "manifest.claims");
    ids.add(claim?.id);
    const result = validateClaim(claim, pin, { allowNonProof: true });
    summaries.push({ id: claim.id, result: claim.result, identity_sha256: result.identity_sha256 });
  }
  return { pin, summaries };
}

function loadManifest(path = DEFAULT_MANIFEST) {
  const absolute = path === DEFAULT_MANIFEST ? join(ROOT, DEFAULT_MANIFEST) : absoluteProjectPath(path, "manifest path");
  return { manifest: readJson(absolute, "claim manifest"), path: absolute };
}

function loadClaim(path, claimId) {
  const { manifest, path: manifestPath } = loadManifest(path);
  const summary = validateManifest(manifest);
  let claim;
  if (claimId) claim = manifest.claims.find((entry) => entry.id === claimId);
  else if (manifest.claims.length === 1) claim = manifest.claims[0];
  if (!claim) {
    throw new ProofError("usage", `claim ${claimId ?? "<unspecified>"} is not present in ${manifestPath}`, "claim");
  }
  return { manifest, manifestPath, claim, summary };
}

function gitHead() {
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

function hostTarget() {
  const arch = process.arch === "x64" ? "x86_64" : process.arch;
  const os = process.platform;
  return { arch, os };
}

function validateRuntimeCandidate(candidate) {
  if (COMMIT.test(candidate.commit)) {
    const observed = gitHead();
    if (!observed || observed !== candidate.commit) {
      throw new ProofError(
        "identity-mismatch",
        `compiler source commit changed (recorded ${candidate.commit}, observed ${observed ?? "unavailable"})`,
        "candidate.commit",
        { recorded: candidate.commit, observed },
      );
    }
  }
  const observedTarget = hostTarget();
  if (candidate.target.arch !== observedTarget.arch || candidate.target.os !== observedTarget.os) {
    throw new ProofError(
      "identity-mismatch",
      `compiler target changed (recorded ${candidate.target.arch}/${candidate.target.os}, observed ${observedTarget.arch}/${observedTarget.os})`,
      "candidate.target",
      { recorded: candidate.target, observed: observedTarget },
    );
  }
}

function checkerExecutable(toolchain) {
  const path = join(toolchain.store_path, toolchain.executable);
  if (!existsSync(path)) {
    throw new ProofError(
      "unavailable",
      `pinned checker executable is unavailable: ${path}`,
      "toolchain.executable",
      { path, store_path: toolchain.store_path },
    );
  }
  let resolved;
  try {
    resolved = realpathSync(path);
  } catch (error) {
    throw new ProofError("unavailable", `cannot resolve pinned checker executable: ${error.message}`, "toolchain.executable");
  }
  if (!resolved.startsWith(`${toolchain.store_path}${sep}`)) {
    throw new ProofError(
      "identity-mismatch",
      `checker executable resolves outside the pinned store output: ${resolved}`,
      "toolchain.executable",
    );
  }
  let stat;
  try {
    stat = lstatSync(resolved);
  } catch (error) {
    throw new ProofError("unavailable", `cannot inspect checker executable: ${error.message}`, "toolchain.executable");
  }
  if (!stat.isFile()) {
    throw new ProofError("unavailable", `checker executable is not a file: ${resolved}`, "toolchain.executable");
  }
  return { path, resolved, bytes: readFileSync(resolved) };
}

function checkPinnedToolchain(toolchain) {
  validateToolchain(toolchain);
  const executable = checkerExecutable(toolchain);
  const env = {
    PATH: dirname(executable.resolved),
    HOME: CHECKER_ENV.home,
    LC_ALL: CHECKER_ENV.locale,
    LANG: CHECKER_ENV.lang,
    TZ: CHECKER_ENV.timezone,
  };
  const result = spawnSync(executable.resolved, toolchain.version_args, {
    cwd: ROOT,
    env,
    encoding: "buffer",
    timeout: 10_000,
    maxBuffer: 1 << 20,
  });
  if (result.error) {
    if (result.error.code === "ETIMEDOUT") {
      throw new ProofError("timeout", "pinned checker version probe timed out", "toolchain.version_args");
    }
    throw new ProofError("unavailable", `pinned checker version probe failed: ${result.error.message}`, "toolchain.executable");
  }
  if (result.status !== 0) {
    throw new ProofError(
      "unavailable",
      `pinned checker version probe exited ${String(result.status)}`,
      "toolchain.version_args",
      { exit_code: result.status, stderr: result.stderr.toString("utf8") },
    );
  }
  const stdout = result.stdout.toString("utf8");
  const stderr = result.stderr.toString("utf8");
  if (stdout !== toolchain.version_output) {
    throw new ProofError(
      "identity-mismatch",
      `checker version output changed (recorded ${JSON.stringify(toolchain.version_output)}, observed ${JSON.stringify(stdout)})`,
      "toolchain.version_output",
      { recorded: toolchain.version_output, observed: stdout },
    );
  }
  if (stderr !== toolchain.version_stderr) {
    throw new ProofError(
      "identity-mismatch",
      "checker version stderr changed",
      "toolchain.version_stderr",
      { recorded: toolchain.version_stderr, observed: stderr },
    );
  }
  return {
    checker: toolchain.checker,
    package: toolchain.package,
    version: toolchain.version,
    nixpkgs: toolchain.nixpkgs,
    store_path: toolchain.store_path,
    executable: toolchain.executable,
    executable_sha256: digestBytes(executable.bytes),
    version_output_sha256: digestBytes(result.stdout),
    version_stderr_sha256: digestBytes(result.stderr),
    environment: CHECKER_ENV,
  };
}

function runProof(claim, toolchain, checkerObservation, authority) {
  const proofPath = absoluteProjectPath(claim.proof.artifact.path, "proof.artifact.path");
  const modelPath = absoluteProjectPath(authority.model.path, "approved authority model.path");
  const timeout = Number.isInteger(claim.proof.budget_ms) ? claim.proof.budget_ms : 30_000;
  if (timeout <= 0 || timeout > 300_000) {
    throw new ProofError("invalid-schema", "proof.budget_ms must be between 1 and 300000", "proof.budget_ms");
  }
  const scratchRoot = proofScratchRoot();
  const env = {
    PATH: dirname(join(toolchain.store_path, toolchain.executable)),
    HOME: CHECKER_ENV.home,
    LC_ALL: CHECKER_ENV.locale,
    LANG: CHECKER_ENV.lang,
    TZ: CHECKER_ENV.timezone,
    TMPDIR: scratchRoot,
    TMP: scratchRoot,
    TEMP: scratchRoot,
    JET_TEST_SCRATCH: scratchRoot,
    JET_TEST_SCRATCH_DIR: scratchRoot,
  };
  const temporaryDirectory = mkdtempSync(join(scratchRoot, "jet-compiler-proof-"));
  const bridgePath = join(temporaryDirectory, "semantic-goal-bridge.lean");
  const modelSource = readFileSync(modelPath, "utf8");
  validateProofTrust(modelSource);
  const goalDeclaration = authority.goal_declaration;
  const frozenGoal = `def ${goalDeclaration} : Prop := ${authority.formal_goal}`;
  const bridgeSource = [
    modelSource,
    "",
    frozenGoal,
    "",
    proofSourceForBridge(proofPath),
    "",
    "theorem jetCompilerProofKernelGoal",
    `  : ${goalDeclaration} := by`,
    `  exact ${authority.correspondence.checked_declaration}`,
    "",
  ].join("\n");
  writeFileSync(bridgePath, bridgeSource, "utf8");
  try {
    const kernel = spawnSync(
      checkerObservation.executable_path,
      [bridgePath],
      { cwd: ROOT, env, encoding: "buffer", timeout, maxBuffer: 4 << 20 },
    );
    if (kernel.error) {
      if (kernel.error.code === "ETIMEDOUT") {
        throw new ProofError("timeout", `semantic-goal kernel check exhausted proof budget (${timeout} ms)`, "proof.semantic_goal");
      }
      throw new ProofError("unavailable", `semantic-goal kernel check failed to start: ${kernel.error.message}`, "proof.semantic_goal");
    }
    const raw = {
      command: [toolchain.checker, bridgePath],
      stdout: kernel.stdout.toString("utf8"),
      stderr: kernel.stderr.toString("utf8"),
      stdout_sha256: digestBytes(kernel.stdout),
      stderr_sha256: digestBytes(kernel.stderr),
      exit_code: kernel.status,
      signal: kernel.signal,
      checker: checkerObservation,
    };
    const kernelReplay = {
      ...raw,
      authority_key: authority.authority_key,
      obligation_id: authority.obligation_id,
      formal_goal: authority.formal_goal,
      formal_goal_sha256: digestText(authority.formal_goal),
    };
    if (kernel.signal) throw new ProofError("non-proof", `semantic-goal kernel check terminated by ${kernel.signal}`, "proof.semantic_goal", kernelReplay);
    if (kernel.status !== 0) throw new ProofError("non-proof", `semantic-goal kernel check rejected the checked declaration (exit ${String(kernel.status)})`, "proof.semantic_goal", kernelReplay);
    return { ...raw, kernel_replay: kernelReplay };
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

function proofSourceForBridge(proofPath) {
  const source = readFileSync(proofPath, "utf8");
  if (!source.trim()) throw new ProofError("non-proof", "proof artifact is empty", "proof.artifact");
  return source;
}

function assertReplayClaim(claim) {
  if (claim.result !== "proved") {
    throw new ProofError(
      "non-proof",
      `claim result ${String(claim.result)} is not a checked proof`,
      "result",
    );
  }
  if (claim.raw_replay.exit_code !== 0) {
    throw new ProofError(
      "non-proof",
      `recorded checker exit is not successful proof evidence`,
      "raw_replay.exit_code",
    );
  }
}

function replay(path = DEFAULT_MANIFEST, claimId = undefined, authority = undefined) {
  if (path !== DEFAULT_MANIFEST) throw new ProofError("invalid-schema", "compiler-proof replay only accepts the fixed canonical toolchain manifest", "manifest");
  const loaded = loadClaim(DEFAULT_MANIFEST, claimId);
  const { manifest, claim } = loaded;
  const requestedAuthority = authority ?? object(claim.authority, "claim.authority");
  const staticResult = validateClaim(claim, loaded.summary.pin, { allowNonProof: true });
  const approvedAuthority = validateApprovedAuthority(manifest, requestedAuthority, claim);
  validateRuntimeCandidate(claim.candidate);
  const checker = checkPinnedToolchain(manifest.toolchain);
  assertReplayClaim(claim);
  const checkerObservation = {
    ...checker,
    executable_path: checker.store_path ? join(checker.store_path, checker.executable) : undefined,
  };
  const raw = runProof(claim, manifest.toolchain, checkerObservation, approvedAuthority);
  return {
    schema: SCHEMA,
    status: "proved",
    claim_id: claim.id,
    identity_sha256: staticResult.identity_sha256,
    authority: approvedAuthority,
    components: identityProjection(claim),
    checker: checkerObservation,
    replay: raw,
  };
}

function identity(path = DEFAULT_MANIFEST, claimId = undefined) {
  const loaded = loadClaim(path, claimId);
  const result = validateClaim(loaded.claim, loaded.summary.pin, { allowNonProof: true });
  return {
    schema: SCHEMA,
    status: "valid",
    claim_id: loaded.claim.id,
    identity_sha256: result.identity_sha256,
    components: identityProjection(loaded.claim),
  };
}

function mutateAt(claim, component) {
  const mutated = clone(claim);
  switch (component) {
    case "claim.proposition":
      mutated.proposition += " (changed)";
      break;
    case "model":
      mutated.model.sha256 = "sha256-0000000000000000000000000000000000000000000000000000000000000000";
      break;
    case "source":
      mutated.source.sha256 = "sha256-1111111111111111111111111111111111111111111111111111111111111111";
      break;
    case "implementation":
      mutated.implementation.sha256 = "sha256-2222222222222222222222222222222222222222222222222222222222222222";
      break;
    case "correspondence":
      mutated.correspondence.method = "digest_only";
      break;
    case "candidate.record_identity":
      mutated.candidate.record_identity.engine = "substituted-engine";
      break;
    case "candidate.compiler.source":
      mutated.candidate.compiler.source.sha256 = "sha256-3333333333333333333333333333333333333333333333333333333333333333";
      break;
    case "candidate.compiler.artifact":
      mutated.candidate.compiler.artifact.sha256 = "sha256-4444444444444444444444444444444444444444444444444444444444444444";
      break;
    case "candidate.configuration":
      mutated.candidate.configuration.sha256 = "sha256-5555555555555555555555555555555555555555555555555555555555555555";
      break;
    case "checker":
      mutated.checker.toolchain_pin_sha256 = "sha256-6666666666666666666666666666666666666666666666666666666666666666";
      break;
    case "assumptions":
      mutated.assumptions[0].statement += " (changed)";
      break;
    case "proof.goal":
      mutated.proof.formal_goal += " (changed)";
      break;
    case "proof.artifact":
      mutated.proof.artifact.sha256 = "sha256-7777777777777777777777777777777777777777777777777777777777777777";
      break;
    case "missing-proof":
      mutated.proof.artifact.path = "proof/compiler/toolchain/fixtures/valid/missing-proof.lean";
      break;
    case "admitted-proof":
      mutated.proof.artifact.path = "proof/compiler/toolchain/fixtures/invalid/admitted.lean";
      mutated.proof.artifact.sha256 = "sha256-541a68bf7abe88b1cf9c3560354c0f7e5cdec03feb6014bfdfdfce7da7e3ff5e";
      break;
    case "detached-proof":
      mutated.proof.artifact.path = "proof/compiler/toolchain/fixtures/invalid/detached.lean";
      mutated.proof.artifact.sha256 = "sha256-e41f3cfa2c5b43698918f7c1e05dc7b9513b4a704f5c726d6f1159341ce46179";
      break;
    case "timeout-budget":
      mutated.proof.budget_ms = 0;
      break;
    case "result":
      mutated.result = "unknown";
      break;
    default:
      throw new ProofError("usage", `unknown hostile fixture component ${component}`, component);
  }
  return mutated;
}

function hostileFixtures(path = DEFAULT_MANIFEST, claimId = undefined) {
  const loaded = loadClaim(path, claimId);
  const components = [
    "claim.proposition",
    "model",
    "source",
    "implementation",
    "correspondence",
    "candidate.record_identity",
    "candidate.compiler.source",
    "candidate.compiler.artifact",
    "candidate.configuration",
    "checker",
    "assumptions",
    "proof.artifact",
    "proof.goal",
    "missing-proof",
    "admitted-proof",
    "detached-proof",
    "timeout-budget",
    "result",
  ];
  const rows = [];
  for (const component of components) {
    const mutated = mutateAt(loaded.claim, component);
    try {
      validateClaim(mutated, loaded.summary.pin);
      rows.push({ component, status: "accepted" });
    } catch (error) {
      if (!(error instanceof ProofError)) throw error;
      rows.push({ component, status: "rejected", code: error.code, error_component: error.component });
    }
  }
  return {
    schema: "jet.compiler-proof-hostile-fixtures.v1",
    schema_version: 1,
    status: "complete",
    claim_id: loaded.claim.id,
    fixtures: rows,
  };
}

function checkToolchain(path = DEFAULT_MANIFEST) {
  const loaded = loadManifest(path);
  const summary = validateManifest(loaded.manifest);
  const checker = checkPinnedToolchain(loaded.manifest.toolchain);
  return {
    schema: SCHEMA,
    status: "valid",
    manifest_id: loaded.manifest.manifest_id,
    toolchain_pin_sha256: summary.pin,
    checker,
  };
}

function usage() {
  return [
    "       node scripts/agent/compiler-proof.mjs --stage <semantics|source|checking|comptime|lowering|optimization|adapters|prelude|runtime> [--stage ...]",
    "       node scripts/agent/compiler-proof.mjs --compose <candidate.json>",
    "",
    "Options:",
    "  --claim <id>                    select a claim when a manifest has more than one",
    "  --json                          emit stable canonical JSON",
    "  --help                          show this help",
  ].join("\n");
}
function runComposition(path) {
  absoluteProjectPath(path, "candidate path");
  const checkerPath = absoluteProjectPath("proof/compiler/composition/check.mjs", "composition checker");
  const child = spawnSync(process.execPath, [checkerPath, "--json", path], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: 15 * 60 * 1000,
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
  });
  if (child.error?.code === "ETIMEDOUT") {
    return {
      result: {
        schema: "jet.compiler-proof-composition-check.v1",
        schema_version: 1,
        stage: "composition",
        status: "timeout",
        error: { code: "timeout", message: "composition checker timed out" },
        proof_boundary: { qualification: "blocked", universal_proof: false },
      },
      exitCode: EXIT.TIMEOUT,
    };
  }
  if (child.error) {
    return {
      result: {
        schema: "jet.compiler-proof-composition-check.v1",
        schema_version: 1,
        stage: "composition",
        status: "unavailable",
        error: { code: "unavailable", message: `composition checker could not start: ${child.error.message}` },
        proof_boundary: { qualification: "blocked", universal_proof: false },
      },
      exitCode: EXIT.UNAVAILABLE,
    };
  }
  let result;
  try {
    result = JSON.parse(child.stdout);
  } catch (error) {
    result = {
      schema: "jet.compiler-proof-composition-check.v1",
      schema_version: 1,
      stage: "composition",
      status: "invalid_oracle",
      error: {
        code: "invalid_oracle",
        message: `composition checker returned non-JSON output: ${error.message}`,
        details: { stderr_sha256: digestText(child.stderr ?? "") },
      },
      proof_boundary: { qualification: "blocked", universal_proof: false },
    };
  }
  if (!result || result.schema !== "jet.compiler-proof-composition-check.v1" || result.stage !== "composition") {
    result = {
      schema: "jet.compiler-proof-composition-check.v1",
      schema_version: 1,
      stage: "composition",
      status: "invalid_oracle",
      error: { code: "invalid_oracle", message: "composition checker returned an unexpected schema" },
      proof_boundary: { qualification: "blocked", universal_proof: false },
    };
  }
  const status = result.status === "qualified" && child.status === 0 ? EXIT.OK : (
    result.status === "timeout" ? EXIT.TIMEOUT
      : result.status === "unavailable" ? EXIT.UNAVAILABLE
        : EXIT.REJECTED
  );
  return { result, exitCode: status };
}

function parseArgs(argv) {
  let mode = "check";
  let path = DEFAULT_MANIFEST;
  let claimId;
  const stages = [];
  let json = false;
  let composePath = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--json") {
      json = true;
    } else if (arg === "--help" || arg === "-h") {
      return { help: true };
    } else if (arg === "--compose") {
      if (mode !== "check") throw new ProofError("usage", "choose one compiler-proof mode", "mode");
      if (index + 1 >= argv.length || argv[index + 1].startsWith("--")) {
        throw new ProofError("usage", "--compose requires a candidate path", "compose");
      }
      mode = "compose";
      composePath = true;
      path = argv[++index];
    } else if (arg === "--stage") {
      if (index + 1 >= argv.length || argv[index + 1].startsWith("--")) {
        throw new ProofError("usage", "--stage requires a stage name", "stage");
      }
      const stage = argv[++index];
      if (!Object.hasOwn(STAGE_DEFINITIONS, stage)) {
        throw new ProofError("usage", `unknown compiler-proof stage ${stage}`, "stage");
      }
      stages.push(stage);
    } else if (["--check", "--check-toolchain", "--identity", "--replay", "--hostile-fixtures"].includes(arg)) {
      if (mode !== "check") throw new ProofError("usage", "choose one compiler-proof mode", "mode");
      mode = arg.slice(2);
      if (index + 1 < argv.length && !argv[index + 1].startsWith("--")) path = argv[++index];
    } else if (arg === "--claim") {
      if (index + 1 >= argv.length) throw new ProofError("usage", "--claim requires an id", "--claim");
      claimId = argv[++index];
    } else {
      throw new ProofError("usage", `unknown argument ${arg}`, "arguments");
    }
  }
  if (mode === "compose" && (!composePath || stages.length || claimId !== undefined)) {
    throw new ProofError("usage", "composition cannot be combined with stage routing or claim selectors", "compose");
  }
  if (stages.length && (mode !== "check" || path !== DEFAULT_MANIFEST || claimId !== undefined)) {
    throw new ProofError("usage", "stage routing cannot be combined with a claim-proof mode, path, or claim selector", "stage");
  }
  return { mode, path, claimId, stages, json };
}
function stageError(stage, status, message, details = undefined) {
  const descriptor = STAGE_DEFINITIONS[stage];
  return {
    schema: descriptor.schema,
    schema_version: 1,
    status,
    stage,
    error: {
      code: status,
      message,
      ...(details === undefined ? {} : { details }),
    },
  };
}

function stageStatus(result) {
  if (!result || typeof result !== "object") return "invalid_oracle";
  if (typeof result.status !== "string" || result.status.length === 0) return "invalid_oracle";
  const nested = [
    result.compiler_execution?.status,
    result.jet_execution?.status,
    result.proof_replay?.status,
  ].filter((value) => typeof value === "string");
  const failures = new Set([
    "timeout",
    "unavailable",
    "cancelled",
    "failed",
    "invalid_oracle",
    "mismatch",
    "rejected",
    "unknown",
    "unverified",
    "blocked",
    "unsupported",
    "empty",
    "contaminated",
  ]);
  const priority = [
    "timeout",
    "unavailable",
    "cancelled",
    "invalid_oracle",
    "failed",
    "mismatch",
    "rejected",
    "empty",
    "contaminated",
    "unknown",
    "unverified",
    "blocked",
    "unsupported",
  ];
  const candidates = [result.status, ...nested].filter((status) => failures.has(status));
  if (candidates.length === 0) return result.status;
  return priority.find((status) => candidates.includes(status)) ?? candidates[0];
}

function stageExitCode(status) {
  if (STAGE_SUCCESS_STATUSES.has(status) || status === "observed") return EXIT.OK;
  return STAGE_STATUS_EXIT[status] ?? EXIT.REJECTED;
}

function runStage(stage) {
  const descriptor = STAGE_DEFINITIONS[stage];
  let checkerPath;
  try {
    checkerPath = absoluteProjectPath(descriptor.checker, `${stage} checker`);
  } catch (error) {
    const status = error instanceof ProofError && error.code === "timeout" ? "timeout" : "unavailable";
    const result = stageError(stage, status, error.message);
    return { stage, result, exitCode: stageExitCode(status) };
  }
  const checkerArgs = ["--json", ...(["runtime", "prelude"].includes(stage) ? ["--replay"] : [])];
  const child = spawnSync(process.execPath, [checkerPath, ...checkerArgs], {
    cwd: ROOT,
    encoding: "utf8",
    timeout: 120000,
    maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
  });
  if (child.error?.code === "ETIMEDOUT") {
    const result = stageError(stage, "timeout", `${stage} contract checker timed out`);
    return { stage, result, exitCode: EXIT.TIMEOUT };
  }
  if (child.error) {
    const result = stageError(stage, "unavailable", `${stage} contract checker could not start: ${child.error.message}`);
    return { stage, result, exitCode: EXIT.UNAVAILABLE };
  }
  if (child.signal) {
    const result = stageError(stage, "invalid_oracle", `${stage} contract checker terminated by signal ${child.signal}`);
    return { stage, result, exitCode: EXIT.REJECTED };
  }
  let result;
  try {
    result = JSON.parse(child.stdout);
  } catch (error) {
    result = stageError(stage, "invalid_oracle", `${stage} contract checker returned non-JSON output: ${error.message}`, {
      stderr_sha256: digestText(child.stderr ?? ""),
    });
    return { stage, result, exitCode: EXIT.REJECTED };
  }
  if (!result || typeof result !== "object" || Array.isArray(result) || result.schema !== descriptor.schema) {
    const actualSchema = result && typeof result === "object" ? result.schema : null;
    result = stageError(stage, "invalid_oracle", `${stage} contract checker returned an unexpected schema`, {
      expected: descriptor.schema,
      actual: actualSchema,
    });
    return { stage, result, exitCode: EXIT.REJECTED };
  }
  const status = stageStatus(result);
  const exitCode = stageExitCode(status);
  if (child.status !== 0 && exitCode === EXIT.OK) {
    result = stageError(stage, "invalid_oracle", `${stage} contract checker exited ${String(child.status)} with a successful status`, {
      reported_status: result.status,
    });
    return { stage, result, exitCode: EXIT.REJECTED };
  }
  return {
    stage,
    result,
    exitCode: child.status !== 0 && exitCode === EXIT.OK ? child.status : exitCode,
  };
}

function aggregateStageStatus(routed) {
  const rank = new Map([
    ["timeout", 100],
    ["unavailable", 90],
    ["cancelled", 80],
    ["invalid_oracle", 70],
    ["failed", 65],
    ["mismatch", 60],
    ["rejected", 60],
    ["empty", 55],
    ["contaminated", 55],
    ["unknown", 50],
    ["unverified", 45],
    ["blocked", 40],
    ["unsupported", 35],
    ["matched", 0],
    ["valid", 0],
    ["observed", 0],
  ]);
  return routed
    .map(({ result }) => stageStatus(result))
    .sort((left, right) => (rank.get(right) ?? 60) - (rank.get(left) ?? 60))[0] ?? "unknown";
}

function runStages(stages) {
  const routed = stages.map((stage) => runStage(stage));
  if (routed.length === 1) return routed[0];
  const status = aggregateStageStatus(routed);
  const exitCode = routed.reduce((current, entry) => Math.max(current, entry.exitCode), EXIT.OK);
  return {
    stage: "aggregate",
    result: {
      schema: "jet.compiler-proof-aggregate.v1",
      schema_version: 1,
      status,
      stages: routed.map(({ stage, result, exitCode: stageExit }) => ({
        stage,
        schema: result.schema,
        status: stageStatus(result),
        exit_code: stageExit,
        result,
      })),
      proof_boundary: {
        qualification: "blocked",
        universal_proof: false,
        reason: "stage aggregation preserves finite checker outcomes and does not establish semantic proof",
      },
    },
    exitCode,
  };
}

function printStage(stage, result) {
  if (stage === "semantics") {
    printSemanticsStage(result);
    return;
  }
  if (stage === "source") {
    printSourceStage(result);
    return;
  }
  process.stdout.write(`${stage} contract: ${result.status}\n`);
  if (result.compiler_execution) {
    process.stdout.write(`compiler: status=${result.compiler_execution.status}; requested=${String(result.compiler_execution.requested)}\n`);
  }
  if (result.jet_execution) {
    process.stdout.write(`jet-mappings: status=${result.jet_execution.status}; requested=${String(result.jet_execution.requested)}\n`);
  }
  if (result.proof_boundary) {
    process.stdout.write(`qualification: ${result.proof_boundary.qualification ?? "blocked"}\n`);
  }
  if (result.error) process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
}

function printRoutedStages(routed) {
  if (routed.stage === "aggregate") {
    process.stdout.write(`compiler-proof stages: ${routed.result.status}\n`);
    for (const entry of routed.result.stages) printStage(entry.stage, entry.result);
    return;
  }
  printStage(routed.stage, routed.result);
}


function output(value, json) {
  if (json) {
    process.stdout.write(`${canonicalJson(value)}\n`);
    return;
  }
  if (value.status === "proved" || value.status === "valid" || value.status === "qualified") {
    if (value.claim_id) process.stdout.write(`claim: ${value.claim_id}\n`);
    if (value.identity_sha256) process.stdout.write(`identity: ${value.identity_sha256}\n`);
    if (value.toolchain_pin_sha256) process.stdout.write(`toolchain: ${value.toolchain_pin_sha256}\n`);
    if (value.fixtures) {
      for (const fixture of value.fixtures) {
        process.stdout.write(`fixture ${fixture.component}: ${fixture.status} (${fixture.code})\n`);
      }
    }
    return;
  }
  process.stdout.write(`compiler-proof: ${value.status}\n`);
  if (value.error) process.stderr.write(`${value.error.code}: ${value.error.message}\n`);
}

function errorResult(error) {
  const proofError = error instanceof ProofError
    ? error
    : new ProofError("internal", error?.message ?? String(error), null);
  return {
    schema: SCHEMA,
    status: proofError.code === "unavailable"
      ? "unavailable"
      : proofError.code === "timeout"
        ? "timeout"
        : "rejected",
    error: {
      code: proofError.code,
      message: proofError.message,
      ...(proofError.component ? { component: proofError.component } : {}),
      ...(proofError.details === undefined ? {} : { details: proofError.details }),
    },
  };
}

export function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs(argv);
    if (options.help) {
      process.stdout.write(`${usage()}\n`);
      return EXIT.OK;
    }
    if (options.stages.length > 0) {
      const routed = runStages(options.stages);
      if (options.json) process.stdout.write(`${canonicalJson(routed.result)}\n`);
      else printRoutedStages(routed);
      return routed.exitCode;
    }
    let result;
    let exitCode = EXIT.OK;
    if (options.mode === "compose") {
      const composed = runComposition(options.path);
      result = composed.result;
      exitCode = composed.exitCode;
    } else if (options.mode === "check") {
      const loaded = loadManifest(options.path);
      const summary = validateManifest(loaded.manifest);
      result = {
        schema: SCHEMA,
        status: "valid",
        manifest_id: loaded.manifest.manifest_id,
        toolchain_pin_sha256: summary.pin,
        claims: summary.summaries,
      };
    } else if (options.mode === "check-toolchain") {
      result = checkToolchain(options.path);
    } else if (options.mode === "identity") {
      result = identity(options.path, options.claimId);
    } else if (options.mode === "replay") {
      const loaded = loadClaim(DEFAULT_MANIFEST, options.claimId);
      const authority = object(loaded.claim.authority, "claim.authority");
      result = replay(options.path, options.claimId, authority);
    } else if (options.mode === "hostile-fixtures") {
      result = hostileFixtures(options.path, options.claimId);
    } else {
      throw new ProofError("usage", `unknown mode ${options.mode}`, "mode");
    }
    output(result, options.json);
    return exitCode;
  } catch (error) {
    const result = errorResult(error);
    output(result, options?.json ?? false);
    if (result.status === "unavailable") return EXIT.UNAVAILABLE;
    if (result.status === "timeout") return EXIT.TIMEOUT;
    if (result.error.code === "usage") return EXIT.USAGE;
    return EXIT.REJECTED;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = main();
}

export {
  SCHEMA,
  canonical,
  canonicalJson,
  digestBytes,
  digestText,
  identityDigest,
  toolchainPin,
  replay,
  validateClaim,
  validateManifest,
  checkPinnedToolchain,
};
