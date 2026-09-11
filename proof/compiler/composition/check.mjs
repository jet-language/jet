#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  readFileSync,
} from "node:fs";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  array,
  canonicalJson,
  checkStageRecord,
  composeRefinement,
  CompositionError,
  digest,
  digestBytes,
  digestJson,
  equal,
  fail,
  object,
  sameDigest,
  text,
} from "./theorem.mjs";

import { replay as replayClaim } from "../../../scripts/agent/compiler-proof.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const CONTRACT_PATH = "proof/compiler/composition/contract.json";
const WITNESSES_PATH = "proof/compiler/composition/witnesses.json";
const CANDIDATE_SCHEMA = "jet.candidate.v1";
const CANDIDATE_PROOF_SCHEMA = "jet.compiler-proof-composition.v1";
const PROOF_MANIFEST_PATH = "proof/compiler/toolchain/manifest.json";
const RESULT_SCHEMA = "jet.compiler-proof-composition-check.v1";
const CHECK_TIMEOUT_MS = 120000;
const COMMIT_RE = /^[0-9a-f]{40}$/u;
const REQUIRED_DEPENDENCIES = Object.freeze([
  "source",
  "generator",
  "compiler",
  "configuration",
  "generated_artifact",
  "target_tool",
  "proof_model",
  "proof_checker",
  "proof_artifact",
  "consumed_test_oracle",
  "behavior",
  "performance",
  "platform",
  "binary",
  "documentation",
  "owner_acceptance",
  "release_claim",
]);

const STAGE_ORDER = Object.freeze([
  "semantics",
  "source",
  "checking",
  "comptime",
  "lowering",
  "optimization",
  "adapters",
  "prelude",
  "runtime",
]);
const STAGE_CHECKERS = Object.freeze({
  semantics: { path: "proof/compiler/semantics/check.mjs", schema: "jet.semantic-contract-check.v1" },
  source: { path: "proof/compiler/source/check.mjs", schema: "jet.source-contract-check.v1" },
  checking: { path: "proof/compiler/checking/check.mjs", schema: "jet.checking-contract-check.v1" },
  comptime: { path: "proof/compiler/comptime/check.mjs", schema: "jet.comptime-contract-check.v1" },
  lowering: { path: "proof/compiler/lowering/check.mjs", schema: "jet.lowering-contract-check.v1" },
  optimization: { path: "proof/compiler/optimization/check.mjs", schema: "jet.optimization-contract-check.v1" },
  prelude: { path: "proof/compiler/prelude/check.mjs", schema: "jet.prelude-contract-check.v1" },
  runtime: { path: "proof/compiler/runtime/check.mjs", schema: "jet.runtime-contract-check.v1" },
  adapters: { path: "proof/compiler/adapters/check.mjs", schema: "jet.adapters-contract-check.v1" },
});

function readJson(path, label) {
  const absolute = resolveProjectPath(path, label);
  try {
    return JSON.parse(readFileSync(absolute, "utf8"));
  } catch (error) {
    fail("unavailable", `${label} could not be read: ${error.message}`, label);
  }
}

function resolveProjectPath(value, label) {
  text(value, label);
  if (isAbsolute(value)) fail("invalid-schema", `${label} must be relative to the repository`, label);
  if (value.includes("\0")) fail("invalid-schema", `${label} contains NUL`, label);
  const absolute = resolve(ROOT, value);
  const rootPrefix = `${ROOT}${sep}`;
  if (absolute !== ROOT && !absolute.startsWith(rootPrefix)) fail("invalid-schema", `${label} escapes the repository`, label);
  return absolute;
}

function relativeProjectPath(value, label) {
  const absolute = resolveProjectPath(value, label);
  const normalized = relative(ROOT, absolute).split(sep).join("/");
  if (!normalized || normalized.startsWith("../") || normalized === "..") fail("invalid-schema", `${label} is not a repository path`, label);
  return { absolute, path: normalized };
}

function fileDigest(path, label) {
  const { absolute, path: normalized } = relativeProjectPath(path, label);
  if (!existsSync(absolute) || !lstatSync(absolute).isFile()) fail("unavailable", `${label} is not a regular file: ${normalized}`, label);
  return { path: normalized, sha256: digestBytes(readFileSync(absolute)) };
}

function verifyIdentity(value, label, { requirePath = false } = {}) {
  object(value, label);
  text(value.id, `${label}.id`);
  digest(value.sha256, `${label}.sha256`);
  text(value.status, `${label}.status`);
  if (["stale", "unknown", "unavailable", "failed", "timeout", "invalid", "mismatch"].includes(value.status)) {
    fail("blocked", `${label} has non-current status ${value.status}`, label);
  }
  if (requirePath || value.path !== undefined) {
    const current = fileDigest(value.path, `${label}.path`);
    if (!sameDigest(current.sha256, value.sha256)) fail("identity-mismatch", `${label} digest is stale`, label);
  }
  return value;
}

function withoutContentDigest(candidate) {
  const copy = JSON.parse(JSON.stringify(candidate));
  delete copy.content_digest;
  if (copy.candidate_identity) delete copy.candidate_identity.record_sha256;
  return copy;
}
function withoutRecordDigest(candidate) {
  const copy = JSON.parse(JSON.stringify(candidate));
  delete copy.content_digest;
  copy.compiler_proof = null;
  if (copy.candidate_identity) delete copy.candidate_identity.record_sha256;
  return copy;
}


const CANDIDATE_IDENTITY_FIELDS = Object.freeze([
  "id",
  "commit",
  "source_content_sha256",
  "record_sha256",
  "compiler_sha256",
  "configuration_sha256",
  "target_tool_sha256",
  "dependency_sha256",
]);

function validateCandidate(candidate) {
  object(candidate, "candidate");
  if (candidate.schema !== CANDIDATE_SCHEMA || candidate.schema_version !== 1) fail("invalid-schema", `candidate schema must be ${CANDIDATE_SCHEMA}`);
  digest(candidate.content_digest, "candidate.content_digest");
  if (!sameDigest(candidate.content_digest, digestJson(withoutContentDigest(candidate)))) fail("identity-mismatch", "candidate content_digest is stale");
  const identity = object(candidate.candidate_identity, "candidate.candidate_identity");
  if (!equal(Object.keys(identity).sort(), [...CANDIDATE_IDENTITY_FIELDS].sort())) fail("invalid-schema", "candidate.candidate_identity fields are not canonical");
  text(identity.id, "candidate.candidate_identity.id");
  if (!COMMIT_RE.test(identity.commit ?? "")) fail("invalid-schema", "candidate.candidate_identity.commit must be a full commit SHA");
  digest(identity.source_content_sha256, "candidate.candidate_identity.source_content_sha256");
  digest(identity.record_sha256, "candidate.candidate_identity.record_sha256");
  digest(identity.compiler_sha256, "candidate.candidate_identity.compiler_sha256");
  digest(identity.configuration_sha256, "candidate.candidate_identity.configuration_sha256");
  digest(identity.target_tool_sha256, "candidate.candidate_identity.target_tool_sha256");
  digest(identity.dependency_sha256, "candidate.candidate_identity.dependency_sha256");
  if (!sameDigest(identity.record_sha256, digestJson(withoutRecordDigest(candidate)))) fail("identity-mismatch", "candidate record identity does not match canonical base record");
  const dependencies = object(candidate.dependencies, "candidate.dependencies");
  if (!equal(candidate.required_dependencies, REQUIRED_DEPENDENCIES)) fail("invalid-schema", "candidate.required_dependencies is not canonical");
  if (!equal(Object.keys(dependencies).sort(), [...REQUIRED_DEPENDENCIES].sort())) fail("invalid-schema", "candidate.dependencies fields are not canonical");
  for (const key of REQUIRED_DEPENDENCIES) {
    if (!Object.hasOwn(dependencies, key)) fail("invalid-schema", `candidate.dependencies is missing ${key}`);
    verifyIdentity(dependencies[key], `candidate.dependencies.${key}`, { requirePath: ["source", "generator", "compiler", "configuration", "generated_artifact", "target_tool", "proof_model", "proof_checker", "proof_artifact", "binary", "documentation"].includes(key) });
  }
  if (!sameDigest(identity.dependency_sha256, digestJson(dependencies))) fail("identity-mismatch", "candidate dependency identity digest is stale");
  if (!sameDigest(dependencies.source.sha256, identity.source_content_sha256)) fail("identity-mismatch", "candidate source identity does not match candidate_identity.source_content_sha256");
  if (!sameDigest(dependencies.compiler.sha256, identity.compiler_sha256)) fail("identity-mismatch", "candidate compiler identity does not match candidate_identity.compiler_sha256");
  if (!sameDigest(dependencies.configuration.sha256, identity.configuration_sha256)) fail("identity-mismatch", "candidate configuration identity does not match candidate_identity.configuration_sha256");
  if (!sameDigest(dependencies.target_tool.sha256, identity.target_tool_sha256)) fail("identity-mismatch", "candidate target identity does not match candidate_identity.target_tool_sha256");
  if (!candidate.prerelease || candidate.prerelease.status !== "prerelease") fail("blocked", "candidate must remain explicitly prerelease");
  if (candidate.prerelease.owner_decision !== "#2927") fail("invalid-schema", "candidate prerelease owner decision changed");
  if (candidate.prerelease.current_version_migration !== "not-required") fail("invalid-schema", "candidate adds current-version migration machinery");
  array(candidate.claims, "candidate.claims");
  array(candidate.historical_receipts, "candidate.historical_receipts");
  for (const receipt of candidate.historical_receipts) {
    object(receipt, "candidate.historical_receipts[]");
    if (receipt.historical !== true || receipt.immutable !== true) fail("invalid-schema", "historical receipt lost immutable historical disposition");
  }
  return { identity, dependencies };
}

function validateContract(contract, witnesses) {
  object(contract, "composition contract");
  if (contract.schema !== "jet.compiler-composition.v1" || contract.schema_version !== 1) fail("invalid-schema", "composition contract schema/version is unsupported");
  if (contract.stage !== "composition" || contract.identity?.authority_key !== "compiler-proof.composition") fail("invalid-oracle", "composition contract authority selector drifted");
  if (!equal(contract.composition_authority, { authority_key: "compiler-proof.composition", obligation_id: "compiler-proof.composition-candidate" })) fail("invalid-oracle", "composition contract obligation selector drifted");
  if (contract.proof_requirements?.requires_composition_replay !== true) fail("invalid-oracle", "composition contract does not require semantic composition replay");
  if (contract.policy?.decision_id !== "D-COMPILER-PROOF1" || contract.policy?.outcome !== "A" || contract.policy?.status !== "ratified") fail("invalid-oracle", "composition contract is not ratified D-COMPILER-PROOF1=A");
  if (contract.policy?.release_decision !== "D-HARDENING-GATE1") fail("invalid-oracle", "composition contract dropped D-HARDENING-GATE1");
  if (!equal(contract.stage_order, STAGE_ORDER)) fail("invalid-oracle", "composition stage order is not the ratified canonical order");
  if (!equal(contract.identity_fields, ["source", "model", "proof", "checker", "compiler_artifact", "candidate_binary", "target", "configuration", "candidate"])) fail("invalid-oracle", "composition identity denominator drifted");
  object(witnesses, "composition witnesses");
  if (witnesses.schema !== "jet.compiler-composition-witnesses.v1" || witnesses.schema_version !== 1) fail("invalid-oracle", "composition witness schema/version is unsupported");
  array(witnesses.controls, "composition witnesses.controls", { nonempty: true });
  return contract;
}

function runStageChecker(stage) {
  const descriptor = STAGE_CHECKERS[stage];
  const checker = resolveProjectPath(descriptor.path, `${stage} checker`);
  const checkerArgs = [checker, "--json", ...(["runtime", "prelude"].includes(stage) ? ["--replay"] : [])];
  const child = spawnSync(process.execPath, checkerArgs, {
    cwd: ROOT,
    encoding: "utf8",
    timeout: CHECK_TIMEOUT_MS,
    maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
  });
  if (child.error?.code === "ETIMEDOUT") fail("timeout", `${stage} checker timed out`, stage);
  if (child.error) fail("unavailable", `${stage} checker could not start: ${child.error.message}`, stage);
  let result;
  try {
    result = JSON.parse(child.stdout);
  } catch (error) {
    fail("invalid-oracle", `${stage} checker returned non-JSON output: ${error.message}`, stage);
  }
  object(result, `${stage} checker result`);
  if (result.schema !== descriptor.schema) fail("invalid-oracle", `${stage} checker returned schema ${String(result.schema)}`, stage);
  if (child.signal) fail("invalid-oracle", `${stage} checker terminated by ${child.signal}`, stage);
  return result;
}

function validateProofArtifact(record, dependencies, stage) {
  const proof = object(record.proof_object, `${stage}.proof_object`);
  if (proof.artifact === undefined) {
    if (proof.replay_ref === undefined) fail("non-proof", `${stage} proof object has no artifact or replay reference`, stage);
    return;
  }
  object(proof.artifact, `${stage}.proof_object.artifact`);
  if (proof.artifact.path !== dependencies.proof_artifact.path || !sameDigest(proof.artifact.sha256, dependencies.proof_artifact.sha256)) fail("identity-mismatch", `${stage} proof artifact does not match candidate proof_artifact`, stage);
  if (proof.checker !== undefined && proof.checker !== dependencies.proof_checker.path) fail("identity-mismatch", `${stage} proof checker does not match candidate proof_checker`, stage);
  const artifact = fileDigest(proof.artifact.path, `${stage}.proof_object.artifact.path`);
  if (!sameDigest(artifact.sha256, proof.artifact.sha256)) fail("identity-mismatch", `${stage} proof artifact bytes changed`, stage);
}

function validateReplayReference(record, actualResult, dependencies, stage) {
  const proof = object(record.proof_object, `${stage}.proof_object`);
  const reference = object(proof.replay_ref, `${stage}.proof_object.replay_ref`);
  const replayPath = text(reference.path, `${stage}.proof_object.replay_ref.path`);
  const replayClaimId = text(reference.claim_id, `${stage}.proof_object.replay_ref.claim_id`);
  let records;
  if (actualResult.formal_discharge !== undefined) {
    const discharge = object(actualResult.formal_discharge, `${stage}.result.formal_discharge`);
    if (discharge.status !== "proved") fail("non-proof", `${stage} formal discharge is not proved`, stage);
    records = array(discharge.records, `${stage}.result.formal_discharge.records`, { nonempty: true });
  } else {
    const claims = array(actualResult.identity?.stage_claims?.items, `${stage}.result.identity.stage_claims.items`, { nonempty: true });
    records = claims.filter((claim) => claim.verified === true || claim.status === "verified").map((claim) => {
      const proof = object(claim.proof_object ?? claim.proof, `${stage}.stage_claims.proof`);
      const semanticBinding = object(proof.semantic_binding, `${stage}.stage_claims.proof.semantic_binding`);
      return {
        replay_ref: proof.replay_ref,
        canonical_goal: { proposition: proof.proposition, formal_goal: proof.formal_goal, source: proof.implementation },
        authority: semanticBinding.authority,
        replay: claim.proof_replay ?? proof.replay,
      };
    });
    if (records.length === 0) fail("non-proof", `${stage} has no verified stage claims with replay records`, stage);
  }
  let referenceMatched = false;
  for (const dischargeRecord of records) {
    const replayRef = object(dischargeRecord.replay_ref, `${stage}.formal_discharge.replay_ref`);
    const path = text(replayRef.path, `${stage}.formal_discharge.replay_ref.path`);
    const claimId = text(replayRef.claim_id, `${stage}.formal_discharge.replay_ref.claim_id`);
    const goal = object(dischargeRecord.canonical_goal, `${stage}.formal_discharge.canonical_goal`);
    const authorityRecord = object(dischargeRecord.authority, `${stage}.formal_discharge.authority`);
    const authority = {
      authority_key: text(authorityRecord.authority_key, `${stage}.formal_discharge.authority.authority_key`),
      obligation_id: text(authorityRecord.obligation_id, `${stage}.formal_discharge.authority.obligation_id`),
    };
    let replayResult;
    try {
      replayResult = replayClaim(path, claimId, authority);
    } catch (error) {
      fail(error?.code ?? "non-proof", `${stage} compiler-proof replay failed: ${error?.message ?? String(error)}`, stage);
    }
    if (path === replayPath && claimId === replayClaimId) referenceMatched = true;
    object(replayResult, `${stage}.compiler-proof.replay`);
    if (replayResult.schema !== "jet.compiler-proof.v1" || replayResult.status !== "proved" || replayResult.claim_id !== claimId) fail("non-proof", `${stage} compiler-proof replay did not return the canonical proved result`, stage);
    digest(replayResult.identity_sha256, `${stage}.compiler-proof.replay.identity_sha256`);
    const rawReplay = object(replayResult.replay, `${stage}.compiler-proof.replay.replay`);
    if (rawReplay.exit_code !== 0 || rawReplay.kernel_replay?.exit_code !== 0) fail("non-proof", `${stage} compiler-proof replay did not pass the semantic-goal kernel check`, stage);
    const components = object(replayResult.components, `${stage}.compiler-proof.replay.components`);
    if (components.claim?.proposition !== goal.proposition || components.proof?.formal_goal !== goal.formal_goal) fail("identity-mismatch", `${stage} replay claim is not bound to the canonical stage goal`, stage);
    if (components.implementation?.path !== goal.source?.path || !sameDigest(components.implementation?.sha256, goal.source?.sha256)) fail("identity-mismatch", `${stage} replay implementation is not bound to the canonical stage source`, stage);
    const candidate = object(components.candidate, `${stage}.compiler-proof.replay.components.candidate`);
    if (!sameDigest(candidate.compiler?.source?.sha256, dependencies.source.sha256)
      || !sameDigest(candidate.compiler?.artifact?.sha256, dependencies.generated_artifact.sha256)
      || !sameDigest(candidate.configuration?.sha256, dependencies.configuration.sha256)) {
      fail("identity-mismatch", `${stage} replay candidate does not match canonical candidate dependencies`, stage);
    }
    const artifact = object(components.proof?.artifact, `${stage}.compiler-proof.replay.components.proof.artifact`);
    if (artifact.path !== dependencies.proof_artifact.path || !sameDigest(artifact.sha256, dependencies.proof_artifact.sha256)) fail("identity-mismatch", `${stage} replay proof artifact does not match candidate proof_artifact`, stage);
    if (dischargeRecord.replay !== undefined && !equal(dischargeRecord.replay, replayResult)) fail("identity-mismatch", `${stage} recorded replay differs from the independently recomputed replay`, stage);
  }
  if (!referenceMatched) fail("mismatch", `${stage} proof replay reference is not an actual formal discharge record`, stage);
}

function validateCompositionProofObject(proof, candidateInfo, contract, evidence) {
  const proofObject = object(proof.proof_object, "candidate.compiler_proof.proof_object");
  if (proofObject.schema !== "jet.compiler-proof-object.v1") {
    fail("invalid-schema", "candidate compiler proof composition proof object schema is invalid");
  }
  if (!contract.proof_requirements.proof_object_statuses.includes(proofObject.status)) {
    fail("non-proof", "candidate compiler proof composition proof object is not verified");
  }
  object(proofObject.artifact, "candidate.compiler_proof.proof_object.artifact");
  if (proofObject.artifact.path !== candidateInfo.dependencies.proof_artifact.path
    || !sameDigest(proofObject.artifact.sha256, candidateInfo.dependencies.proof_artifact.sha256)) {
    fail("identity-mismatch", "candidate compiler proof composition artifact does not match candidate proof artifact");
  }
  const artifact = fileDigest(proofObject.artifact.path, "candidate.compiler_proof.proof_object.artifact.path");
  if (!sameDigest(artifact.sha256, proofObject.artifact.sha256)) {
    fail("identity-mismatch", "candidate compiler proof composition artifact is stale");
  }
  const replayRef = object(proofObject.replay_ref, "candidate.compiler_proof.proof_object.replay_ref");
  const replayPath = text(replayRef.path, "candidate.compiler_proof.proof_object.replay_ref.path");
  if (replayPath !== PROOF_MANIFEST_PATH) {
    fail("mismatch", "candidate compiler proof composition replay is not pinned to the canonical manifest");
  }
  const claimId = text(replayRef.claim_id, "candidate.compiler_proof.proof_object.replay_ref.claim_id");
  const authorityRecord = object(proofObject.authority, "candidate.compiler_proof.proof_object.authority");
  const authority = {
    authority_key: text(authorityRecord.authority_key, "candidate.compiler_proof.proof_object.authority.authority_key"),
    obligation_id: text(authorityRecord.obligation_id, "candidate.compiler_proof.proof_object.authority.obligation_id"),
  };
  if (authority.authority_key !== "compiler-proof.composition"
    || authority.obligation_id !== "compiler-proof.composition-candidate") {
    fail("mismatch", "candidate compiler proof composition authority is not the canonical selector");
  }
  let replayResult;
  try {
    replayResult = replayClaim(replayPath, claimId, authority);
  } catch (error) {
    fail(error?.code ?? "non-proof", `composition compiler-proof replay failed: ${error?.message ?? String(error)}`);
  }
  object(replayResult, "candidate.compiler_proof.proof_object.replay");
  if (replayResult.schema !== "jet.compiler-proof.v1"
    || replayResult.status !== "proved"
    || replayResult.claim_id !== claimId) {
    fail("non-proof", "composition compiler-proof replay did not return the canonical proved result");
  }
  digest(replayResult.identity_sha256, "candidate.compiler_proof.proof_object.replay.identity_sha256");
  const replayAuthority = object(replayResult.authority, "candidate.compiler_proof.proof_object.replay.authority");
  if (replayAuthority.authority_key !== authority.authority_key
    || replayAuthority.obligation_id !== authority.obligation_id) {
    fail("identity-mismatch", "composition compiler-proof replay authority changed");
  }
  if (!Object.hasOwn(replayAuthority, "candidate")) {
    fail("invalid-oracle", "approved composition semantic goal has no candidate binding");
  }
  const rawReplay = object(replayResult.replay, "candidate.compiler_proof.proof_object.replay.replay");
  if (rawReplay.exit_code !== 0 || rawReplay.kernel_replay?.exit_code !== 0) {
    fail("non-proof", "composition compiler-proof replay did not pass the semantic-goal kernel check");
  }
  const components = object(replayResult.components, "candidate.compiler_proof.proof_object.replay.components");
  if (typeof components.claim?.proposition !== "string"
    || typeof components.proof?.formal_goal !== "string") {
    fail("non-proof", "composition compiler-proof replay returned no semantic goal");
  }
  const replayCandidate = object(components.candidate, "candidate.compiler_proof.proof_object.replay.components.candidate");
  if (replayCandidate.commit !== candidateInfo.identity.commit
    || !sameDigest(replayCandidate.compiler?.source?.sha256, candidateInfo.dependencies.compiler.sha256)
    || !sameDigest(replayCandidate.compiler?.artifact?.sha256, candidateInfo.dependencies.generated_artifact.sha256)
    || !sameDigest(replayCandidate.configuration?.sha256, candidateInfo.dependencies.configuration.sha256)) {
    fail("identity-mismatch", "composition compiler-proof replay candidate does not match the checked candidate");
  }
  if (!equal(replayAuthority.candidate, replayCandidate)) {
    fail("identity-mismatch", "composition compiler-proof replay candidate differs from its approved semantic goal");
  }
  const relation = object(proof.relation, "candidate.compiler_proof.relation");
  const expectedBinding = {
    candidate_identity: proof.candidate_identity,
    relation,
    stages: evidence.map((entry) => ({
      stage: entry.stage,
      edge: {
        from: entry.relation.from,
        to: entry.relation.to,
      },
      model: entry.identity.model,
      proof: entry.identity.proof,
      replay_ref: entry.proof_object.replay_ref,
    })),
  };
  if (!equal(replayCandidate.composition_binding, expectedBinding)) {
    fail("identity-mismatch", "composition compiler-proof replay is not bound to the checked candidate, relation, and nine stage proof/model references");
  }
  return {
    schema: proofObject.schema,
    status: proofObject.status,
    artifact: { path: proofObject.artifact.path, sha256: proofObject.artifact.sha256 },
    replay_ref: { path: replayPath, claim_id: claimId },
    authority,
    replay: replayResult,
  };
}

function validateStageInput(entry, actualResult, stage, candidateInfo, contract) {
  object(entry, `compiler_proof.stage_evidence.${stage}`);
  if (entry.stage !== stage) fail("mismatch", `${stage} evidence names another stage`, stage);
  object(entry.result, `${stage}.result`);
  digest(entry.result_sha256, `${stage}.result_sha256`);
  if (!sameDigest(entry.result_sha256, digestJson(actualResult))) fail("identity-mismatch", `${stage} result evidence is stale or from another checker run`, stage);
  if (!equal(entry.result, actualResult)) fail("identity-mismatch", `${stage} result evidence differs from the current checker result`, stage);
  if (!equal(entry.coverage, actualResult.coverage)) fail("mismatch", `${stage} obligation coverage differs from the current checker result`, stage);
  validateProofArtifact(entry, candidateInfo.dependencies, stage);
  validateReplayReference(entry, actualResult, candidateInfo.dependencies, stage);
  const edge = contract.edges.find((candidate) => candidate.stage === stage);
  object(entry.relation, `${stage}.relation`);
  if (entry.relation.stage !== stage || entry.relation.from !== edge.from || entry.relation.to !== edge.to) fail("mismatch", `${stage} relation edge is incompatible`, stage);
  return checkStageRecord(entry, stage, contract, candidateInfo.identity, candidateInfo.dependencies);
}
function validateCompilerProof(candidate, candidateInfo, contract) {
  const proof = object(candidate.compiler_proof, "candidate.compiler_proof");
  if (proof.schema !== CANDIDATE_PROOF_SCHEMA || proof.schema_version !== 1) fail("invalid-schema", `candidate.compiler_proof schema must be ${CANDIDATE_PROOF_SCHEMA}`);
  object(proof.candidate_identity, "candidate.compiler_proof.candidate_identity");
  if (!equal(proof.candidate_identity, candidateInfo.identity)) fail("identity-mismatch", "compiler proof candidate identity changed");
  const evidence = array(proof.stage_evidence, "candidate.compiler_proof.stage_evidence", { nonempty: true });
  if (!equal(evidence.map((entry) => entry?.stage), contract.stage_order)) fail("blocked", "candidate compiler proof stage_evidence is not in canonical relation order");
  const byStage = new Map();
  for (const entry of evidence) {
    text(entry?.stage, "candidate.compiler_proof.stage_evidence[].stage");
    if (byStage.has(entry.stage)) fail("invalid-schema", `duplicate compiler proof stage ${entry.stage}`);
    byStage.set(entry.stage, entry);
  }
  for (const stage of contract.stage_order) if (!byStage.has(stage)) fail("blocked", `candidate compiler proof is missing stage ${stage}`);
  for (const stage of byStage.keys()) if (!contract.stage_order.includes(stage)) fail("invalid-schema", `candidate compiler proof names unknown stage ${stage}`);
  const records = new Map();
  for (const stage of contract.stage_order) {
    const entry = byStage.get(stage);
    const actual = runStageChecker(stage);
    records.set(stage, validateStageInput(entry, actual, stage, candidateInfo, contract));
  }
  const compositionProof = validateCompositionProofObject(proof, candidateInfo, contract, evidence);
  const result = composeRefinement({
    contract,
    candidate,
    relation: proof.relation,
    stageRecords: records,
    supportedProgramAcceptance: proof.supported_program_acceptance,
    correctRejection: proof.correct_rejection,
    preservation: proof.preservation,
    foreignAssumptions: proof.foreign_assumptions,
    identity: proof.candidate_identity,
    compositionProof,
  });
  if (proof.status !== "qualified") fail("blocked", `candidate compiler_proof status is ${String(proof.status)}, not qualified`);
  return result;
}

function errorResult(error) {
  const code = error instanceof CompositionError ? error.code : "invalid_oracle";
  const normalized = code === "invalid-oracle" ? "invalid_oracle" : code;
  const status = ["usage", "timeout", "unavailable", "invalid_oracle", "identity-mismatch", "mismatch", "non-proof", "blocked", "invalid-schema"].includes(normalized)
    ? normalized
    : "invalid_oracle";
  return {
    schema: RESULT_SCHEMA,
    schema_version: 1,
    stage: "composition",
    status,
    error: {
      code,
      message: error?.message ?? String(error),
      ...(error?.details === undefined ? {} : { details: error.details }),
    },
    proof_boundary: { qualification: "blocked", universal_proof: false },
  };
}

export function runComposition(candidatePath) {
  const contract = readJson(CONTRACT_PATH, "composition contract");
  const witnesses = readJson(WITNESSES_PATH, "composition witnesses");
  validateContract(contract, witnesses);
  const candidate = readJson(candidatePath, "candidate record");
  const candidateInfo = validateCandidate(candidate);
  return validateCompilerProof(candidate, candidateInfo, contract);
}

function usage() {
  return "Usage: node proof/compiler/composition/check.mjs [--json] candidate.json";
}

export async function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  const paths = argv.filter((arg) => arg !== "--json");
  try {
    if (paths.length !== 1 || argv.some((arg) => arg !== "--json" && arg !== paths[0])) throw new CompositionError("usage", usage());
    const result = runComposition(paths[0]);
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stdout.write(`compiler composition: ${result.status}\n`);
    return 0;
  } catch (error) {
    const result = errorResult(error);
    if (json) process.stdout.write(`${canonicalJson(result)}\n`);
    else process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
    return result.status === "timeout" ? 4 : result.status === "unavailable" ? 3 : result.status === "usage" ? 2 : 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = await main();

export {
  CONTRACT_PATH,
  RESULT_SCHEMA,
  STAGE_CHECKERS,
  validateCandidate,
  validateContract,
  validateCompilerProof,
};
