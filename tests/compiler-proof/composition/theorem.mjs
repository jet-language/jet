import { createHash } from "node:crypto";

const DIGEST = /^sha256-[0-9a-f]{64}$/u;

export const STAGES = Object.freeze([
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

export const OBSERVATION_ALPHABET = Object.freeze([
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

export class CompositionError extends Error {
  constructor(code, message, details = undefined) {
    super(message);
    this.name = "CompositionError";
    this.code = code;
    this.details = details;
  }
}

export function fail(code, message, details = undefined) {
  throw new CompositionError(code, message, details);
}

export function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (isObject(value)) {
    const result = {};
    for (const key of Object.keys(value).sort()) result[key] = canonical(value[key]);
    return result;
  }
  return value;
}

export function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

export function digestBytes(value) {
  return `sha256-${createHash("sha256").update(value).digest("hex")}`;
}

export function digestJson(value) {
  return digestBytes(Buffer.from(canonicalJson(value), "utf8"));
}

export function sameDigest(left, right) {
  return typeof left === "string" && typeof right === "string" && left === right;
}

export function text(value, label) {
  if (typeof value !== "string" || value.length === 0 || /[\u0000-\u001f\u007f]/u.test(value)) {
    fail("invalid-schema", `${label} must be non-empty text`, label);
  }
  return value;
}

export function digest(value, label) {
  const result = text(value, label);
  if (!DIGEST.test(result)) fail("invalid-schema", `${label} must be a SHA-256 digest`, label);
  return result;
}

export function object(value, label) {
  if (!isObject(value)) fail("invalid-schema", `${label} must be an object`, label);
  return value;
}

export function array(value, label, { nonempty = false } = {}) {
  if (!Array.isArray(value) || (nonempty && value.length === 0)) {
    fail("invalid-schema", `${label} must be${nonempty ? " a non-empty" : " an"} array`, label);
  }
  return value;
}

export function equal(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function candidateIdentityProjection(candidateIdentity) {
  object(candidateIdentity, "candidate.candidate_identity");
  return {
    id: text(candidateIdentity.id, "candidate.candidate_identity.id"),
    commit: text(candidateIdentity.commit, "candidate.candidate_identity.commit"),
    source_content_sha256: text(candidateIdentity.source_content_sha256, "candidate.candidate_identity.source_content_sha256"),
    record_sha256: text(candidateIdentity.record_sha256, "candidate.candidate_identity.record_sha256"),
    compiler_sha256: text(candidateIdentity.compiler_sha256, "candidate.candidate_identity.compiler_sha256"),
    configuration_sha256: text(candidateIdentity.configuration_sha256, "candidate.candidate_identity.configuration_sha256"),
    target_tool_sha256: text(candidateIdentity.target_tool_sha256, "candidate.candidate_identity.target_tool_sha256"),
    dependency_sha256: text(candidateIdentity.dependency_sha256, "candidate.candidate_identity.dependency_sha256"),
  };
}

export function candidateIdentityDigest(candidateIdentity) {
  return digest(text(candidateIdentity.record_sha256, "candidate.candidate_identity.record_sha256"), "candidate.candidate_identity.record_sha256");
}
function requiredIdentityFields(contract) {
  return contract.identity_fields;
}

function checkIdentityObject(identity, candidate, dependencies, contract, label) {
  object(identity, label);
  for (const field of requiredIdentityFields(contract)) {
    const value = identity[field];
    object(value, `${label}.${field}`);
    const dependencyKey = {
      source: "source",
      model: "proof_model",
      proof: "proof_artifact",
      checker: "proof_checker",
      compiler_artifact: "generated_artifact",
      candidate_binary: "binary",
      target: "target_tool",
      configuration: "configuration",
    }[field];
    if (field === "candidate") {
      if (!equal(candidateIdentityProjection(value), candidateIdentityProjection(candidate))) {
        fail("identity-mismatch", `${label}.candidate does not match the candidate record`, label);
      }
      continue;
    }
    const expected = dependencies[dependencyKey];
    object(expected, `candidate.dependencies.${dependencyKey}`);
    if (!equal(value, expected)) fail("identity-mismatch", `${label}.${field} does not match candidate.dependencies.${dependencyKey}`, label);
  }
  return identity;
}

function coverageComplete(coverage, contract) {
  object(coverage, "stage.coverage");
  if (coverage.complete === true) return true;
  if (contract.proof_requirements.coverage_statuses.includes(coverage.status)) return true;
  if (contract.proof_requirements.coverage_statuses.includes(coverage.qualification)) return true;
  return false;
}

export function checkProofObject(record, stage, contract, dependencies) {
  const proof = object(record.proof_object, `${stage}.proof_object`);
  text(proof.schema, `${stage}.proof_object.schema`);
  if (!contract.proof_requirements.proof_object_statuses.includes(proof.status)) {
    fail("non-proof", `${stage} proof object is not verified`, stage);
  }
  if (proof.universal_proof === true) fail("invalid-oracle", `${stage} proof object claims universal proof`, stage);
  const payloads = ["artifact", "evidence", "replay_ref", "replay", "checker"].filter((field) => proof[field] !== undefined);
  if (payloads.length === 0) fail("non-proof", `${stage} proof object has no verification payload`, stage);
  let artifact = dependencies.proof_artifact;
  if (proof.artifact !== undefined) {
    const declared = object(proof.artifact, `${stage}.proof_object.artifact`);
    text(declared.path, `${stage}.proof_object.artifact.path`);
    digest(declared.sha256, `${stage}.proof_object.artifact.sha256`);
    artifact = { path: declared.path, sha256: declared.sha256 };
  }
  if (proof.checker !== undefined) {
    if (typeof proof.checker === "string" && proof.checker !== dependencies.proof_checker.path) fail("identity-mismatch", `${stage} proof checker does not match candidate proof_checker`, stage);
    if (isObject(proof.checker)) text(proof.checker.id, `${stage}.proof_object.checker.id`);
  }
  if (proof.replay !== undefined) {
    const replay = object(proof.replay, `${stage}.proof_object.replay`);
    if (replay.status !== undefined && !contract.proof_requirements.replay_statuses.includes(replay.status)) fail("non-proof", `${stage} proof object replay is not verified`, stage);
  }
  if (proof.replay_ref !== undefined) object(proof.replay_ref, `${stage}.proof_object.replay_ref`);
  if (proof.evidence !== undefined && !isObject(proof.evidence) && !Array.isArray(proof.evidence)) fail("invalid-schema", `${stage} proof evidence must be an object or array`, stage);
  return {
    status: proof.status,
    artifact: { path: artifact.path, sha256: artifact.sha256 },
    checker: proof.checker ?? null,
    replay: proof.replay ?? null,
    identity_sha256: proof.identity_sha256 ?? null,
  };
}

export function checkCandidateBinding(record, candidate, label) {
  const identity = object(record.identity, `${label}.identity`);
  if (!equal(candidateIdentityProjection(identity.candidate), candidateIdentityProjection(candidate))) {
    fail("identity-mismatch", `${label} is bound to a different candidate`, label);
  }
  const expected = candidateIdentityDigest(candidate);
  if (record.candidate_identity_sha256 && !sameDigest(record.candidate_identity_sha256, expected)) {
    fail("identity-mismatch", `${label}.candidate_identity_sha256 is stale`, label);
  }
  return expected;
}

export function checkStageRecord(record, stage, contract, candidate, dependencies) {
  object(record, `${stage} stage evidence`);
  if (record.stage !== stage) fail("mismatch", `${stage} stage evidence names a different stage`, stage);
  const result = object(record.result, `${stage}.result`);
  if (result.stage !== undefined && result.stage !== stage) fail("mismatch", `${stage}.result names a different stage`, stage);
  if (typeof result.schema !== "string" || !result.schema.endsWith("-contract-check.v1")) {
    fail("invalid-oracle", `${stage}.result is not a stage contract result`, stage);
  }
  if (!contract.proof_requirements.stage_statuses.includes(result.status)) {
    fail("blocked", `${stage} result is not conditionally qualified`, { stage, status: result.status });
  }
  const boundary = object(result.proof_boundary, `${stage}.result.proof_boundary`);
  if (!["conditional", "qualified", "proved"].includes(boundary.qualification)) {
    fail("blocked", `${stage} proof boundary is not conditionally qualified`, stage);
  }
  if (boundary.universal_proof !== false) fail("invalid-oracle", `${stage} result claims universal proof`, stage);
  if (!coverageComplete(record.coverage, contract)) fail("blocked", `${stage} obligation coverage is incomplete`, stage);
  checkIdentityObject(record.identity, candidate, dependencies, contract, `${stage}.identity`);
  const proof = checkProofObject(record, stage, contract, dependencies);
  const candidateDigest = checkCandidateBinding(record, candidate, stage);
  if (record.proof_object.artifact !== undefined && !sameDigest(record.proof_object.artifact.sha256, proof.artifact.sha256)) fail("identity-mismatch", `${stage} proof artifact identity changed`, stage);
  return {
    stage,
    schema: result.schema,
    status: result.status,
    candidate_identity_sha256: candidateDigest,
    proof,
    coverage: {
      status: record.coverage.status ?? record.coverage.qualification,
      denominator: record.coverage.denominator ?? record.coverage.row_count ?? record.coverage.global_row_count ?? null,
    },
    identity: record.identity,
    relation: record.relation,
  };
}

function modeMap(value, label) {
  const rows = array(value.modes, `${label}.modes`, { nonempty: true });
  const result = new Map();
  for (const row of rows) {
    object(row, `${label}.modes[]`);
    const mode = text(row.mode, `${label}.modes[].mode`);
    if (result.has(mode)) fail("invalid-oracle", `${label} repeats mode ${mode}`);
    result.set(mode, row);
  }
  return result;
}

function observedStatus(status, label) {
  if (!["observed", "verified", "matched", "accepted", "rejected"].includes(status)) {
    fail("blocked", `${label} is not observed`, label);
  }
}

export function checkSupportedAcceptance(value, candidate, source, stageResults, contract) {
  const acceptance = object(value, "compiler_proof.supported_program_acceptance");
  if (!["observed", "verified", "matched"].includes(acceptance.status)) fail("blocked", "supported_program_acceptance.status is not a positive observation");
  const modes = modeMap(acceptance, "supported_program_acceptance");
  const requiredModes = contract.required_execution_modes.filter((entry) => entry !== "check");
  if (modes.size !== requiredModes.length || !equal([...modes.keys()].sort(), [...requiredModes].sort())) fail("blocked", "supported-program acceptance has missing or unknown execution modes");
  for (const mode of requiredModes) {
    const row = modes.get(mode);
    if (!row) fail("blocked", `supported-program acceptance is missing execution mode ${mode}`);
    if (!["accepted", "observed", "verified", "matched"].includes(row.status) || row.accepted !== true) {
      fail("blocked", `supported-program acceptance mode ${mode} did not accept`, mode);
    }
    object(row.observations, `supported_program_acceptance.${mode}.observations`);
    if (row.observations.result === undefined) fail("blocked", `supported-program acceptance mode ${mode} has no result observation`, mode);
    if (row.exit_code !== 0) fail("blocked", `supported-program acceptance mode ${mode} exited unsuccessfully`, mode);
    if (!sameDigest(row.source_sha256, source.sha256)) fail("identity-mismatch", `supported-program acceptance mode ${mode} source changed`, mode);
    if (!sameDigest(row.candidate_identity_sha256, candidateIdentityDigest(candidate))) fail("identity-mismatch", `supported-program acceptance mode ${mode} candidate changed`, mode);
  }
  object(acceptance.evidence_ref, "supported_program_acceptance.evidence_ref");
  const refStage = text(acceptance.evidence_ref.stage, "supported_program_acceptance.evidence_ref.stage");
  const refCase = text(acceptance.evidence_ref.case_id, "supported_program_acceptance.evidence_ref.case_id");
  const record = stageResults.get(refStage);
  if (!record || !JSON.stringify(record.result).includes(refCase)) fail("mismatch", `supported-program acceptance witness ${refCase} is not in ${refStage}`);
  return { status: acceptance.status, modes: [...modes.keys()].sort(), evidence_ref: acceptance.evidence_ref };
}

function checkDiagnostic(diagnostic) {
  object(diagnostic, "correct_rejection.diagnostic");
  if (!/^E[0-9]{4}$/u.test(diagnostic.code ?? "")) fail("invalid-oracle", "correct rejection diagnostic is not a registered Jet code");
  object(diagnostic.span, "correct_rejection.diagnostic.span");
  if (!Number.isInteger(diagnostic.span.start) || !Number.isInteger(diagnostic.span.end) || diagnostic.span.start < 0 || diagnostic.span.end < diagnostic.span.start) fail("invalid-oracle", "correct rejection diagnostic span is invalid");
  text(diagnostic.reason, "correct_rejection.diagnostic.reason");
  return diagnostic;
}

export function checkCorrectRejection(value, candidate, source, stageResults) {
  const rejection = object(value, "compiler_proof.correct_rejection");
  observedStatus(rejection.status, "correct_rejection.status");
  if (rejection.accepted !== false) fail("blocked", "correct rejection is marked accepted");
  const modes = modeMap(rejection, "correct_rejection");
  if (modes.size !== 1 || !modes.has("check")) fail("blocked", "correct rejection must discharge the canonical check mode");
  const row = modes.get("check");
  if (!["rejected", "observed", "verified", "matched"].includes(row.status) || row.accepted !== false) fail("blocked", "correct rejection has no rejected check observation");
  const diagnostic = checkDiagnostic(row.diagnostic);
  object(row.observations, "correct_rejection.check.observations");
  if (row.observations.result === undefined && row.result === undefined) fail("blocked", "correct rejection check has no result observation");
  if (row.exit_code === 0 && row.result?.status !== "rejected" && row.observations?.status !== "rejected") fail("blocked", "correct rejection check did not reject");
  if (!sameDigest(rejection.source_sha256, source.sha256)) fail("identity-mismatch", "correct rejection source identity changed");
  if (!sameDigest(rejection.candidate_identity_sha256, candidateIdentityDigest(candidate))) fail("identity-mismatch", "correct rejection candidate identity changed");
  object(rejection.evidence_ref, "correct_rejection.evidence_ref");
  const record = stageResults.get(text(rejection.evidence_ref.stage, "correct_rejection.evidence_ref.stage"));
  const caseId = text(rejection.evidence_ref.case_id, "correct_rejection.evidence_ref.case_id");
  if (!record || !JSON.stringify(record.result).includes(caseId) || !JSON.stringify(record.result).includes(diagnostic.code)) fail("invalid-oracle", `correct rejection witness ${caseId} does not carry ${diagnostic.code}`);
  return { status: rejection.status, accepted: false, modes: [{ mode: "check", status: row.status, accepted: false, diagnostic, observations: row.observations, result: row.result, exit_code: row.exit_code }], source_sha256: rejection.source_sha256, candidate_identity_sha256: rejection.candidate_identity_sha256, evidence_ref: rejection.evidence_ref };
}

export function checkRelation(relation, contract, stageRecords) {
  object(relation, "compiler_proof.relation");
  if (relation.schema !== "jet.compiler-refinement.v1" || relation.schema_version !== 1) fail("invalid-schema", "composition relation schema is unsupported");
  if (relation.relation_id !== contract.theorem.relation) fail("mismatch", "composition relation identity changed");
  if (!equal(relation.stage_order, contract.stage_order)) fail("mismatch", "composition stage order is incomplete or reordered");
  if (!equal(relation.representations, contract.representations)) fail("mismatch", "composition intermediate representations changed");
  if (!equal(relation.edges, contract.edges)) fail("mismatch", "composition refinement edges changed");
  if (!equal(relation.observation_alphabet, contract.observation_alphabet)) fail("mismatch", "composition observation alphabet changed");
  if (!equal(relation.preservation_dimensions, contract.preservation_dimensions)) fail("mismatch", "composition preservation dimensions changed");
  const rows = array(relation.stages, "compiler_proof.relation.stages", { nonempty: true });
  if (!equal(rows.map((row) => row.stage), contract.stage_order)) fail("blocked", "composition relation omits or duplicates a required stage");
  for (const row of rows) {
    object(row, `compiler_proof.relation.stages.${row.stage}`);
    const record = stageRecords.get(row.stage);
    if (!record) fail("blocked", `composition relation has no checked record for ${row.stage}`);
    const edge = contract.edges.find((entry) => entry.stage === row.stage);
    if (!edge || row.from !== edge.from || row.to !== edge.to) fail("mismatch", `composition relation edge changed for ${row.stage}`);
    if (!sameDigest(row.candidate_identity_sha256, record.candidate_identity_sha256)) fail("identity-mismatch", `composition relation candidate identity changed for ${row.stage}`);
    if (!sameDigest(row.proof_artifact_sha256, record.proof.artifact.sha256)) fail("identity-mismatch", `composition relation proof identity changed for ${row.stage}`);
  }
  return {
    schema: relation.schema,
    schema_version: relation.schema_version,
    relation_id: relation.relation_id,
    stage_order: [...relation.stage_order],
    representations: [...relation.representations],
    observation_alphabet: [...relation.observation_alphabet],
    preservation_dimensions: [...relation.preservation_dimensions],
  };
}

export function checkPreservation(value, contract, stageRecords, candidate, source) {
  const preservation = object(value, "compiler_proof.preservation");
  observedStatus(preservation.status, "preservation.status");
  if (!["observed", "verified", "matched"].includes(preservation.status)) fail("blocked", "preservation.status is not a positive observation");
  if (preservation.universal_proof !== false) fail("invalid-oracle", "preservation cannot claim a universal physical-machine theorem");
  if (!sameDigest(preservation.source_sha256, source.sha256)) fail("identity-mismatch", "preservation source identity changed");
  if (!sameDigest(preservation.candidate_identity_sha256, candidateIdentityDigest(candidate))) fail("identity-mismatch", "preservation candidate identity changed");
  object(preservation.evidence_ref, "compiler_proof.preservation.evidence_ref");
  const preservationStage = text(preservation.evidence_ref.stage, "compiler_proof.preservation.evidence_ref.stage");
  const preservationCase = text(preservation.evidence_ref.case_id, "compiler_proof.preservation.evidence_ref.case_id");
  const preservationRecord = stageRecords.get(preservationStage);
  if (!preservationRecord || !JSON.stringify(preservationRecord.result).includes(preservationCase)) fail("mismatch", `preservation witness ${preservationCase} is not in ${preservationStage}`);
  if (!equal(preservation.observation_alphabet, contract.observation_alphabet)) fail("mismatch", "preservation observation alphabet changed");
  if (!equal(preservation.preservation_dimensions, contract.preservation_dimensions)) fail("mismatch", "preservation dimensions changed");
  const rows = array(preservation.stages, "compiler_proof.preservation.stages", { nonempty: true });
  if (!equal(rows.map((row) => row.stage), contract.stage_order)) fail("blocked", "preservation omits a required stage");
  for (const row of rows) {
    object(row, `compiler_proof.preservation.${row.stage}`);
    if (!["qualified", "verified", "proved", "matched"].includes(row.status)) fail("blocked", `preservation stage ${row.stage} is not checked`);
    if (!sameDigest(row.proof_artifact_sha256, stageRecords.get(row.stage)?.proof.artifact.sha256)) fail("identity-mismatch", `preservation proof identity changed for ${row.stage}`);
  }
  const premises = array(preservation.premises, "compiler_proof.preservation.premises", { nonempty: true });
  if (!Array.isArray(contract.premises) || premises.length !== contract.premises.length) fail("blocked", "preservation premises are incomplete or duplicated");
  for (const expected of contract.premises) {
    const actual = premises.find((row) => row.id === expected.id);
    if (!actual || actual.status !== expected.status || actual.statement !== expected.statement) {
      fail("blocked", `preservation premise ${expected.id} is missing or changed`);
    }
  }
  return { status: preservation.status, universal_proof: false, source_sha256: preservation.source_sha256, candidate_identity_sha256: preservation.candidate_identity_sha256, evidence_ref: preservation.evidence_ref, observation_alphabet: [...preservation.observation_alphabet], preservation_dimensions: [...preservation.preservation_dimensions], stages: rows.map((row) => row.stage), premises: premises.map((row) => row.id).sort() };
}

export function checkAssumptions(value, contract) {
  const assumptions = array(value, "compiler_proof.foreign_assumptions", { nonempty: true });
  if (assumptions.length !== contract.foreign_assumptions.length || !equal(assumptions.map((entry) => entry.id).sort(), contract.foreign_assumptions.map((entry) => entry.id).sort())) fail("blocked", "foreign assumptions are incomplete or contain unknown assumptions");
  for (const expected of contract.foreign_assumptions) {
    const actual = assumptions.find((entry) => entry.id === expected.id);
    if (!actual || actual.status !== "assumed" || actual.universal_proof !== false || actual.statement !== expected.statement || actual.boundary !== expected.boundary) fail("blocked", `foreign assumption ${expected.id} is missing, changed, or promoted to proof`);
  }
  return assumptions.map((entry) => ({ id: entry.id, status: entry.status, statement: entry.statement, boundary: entry.boundary, universal_proof: entry.universal_proof }));
}

export function composeRefinement({ contract, candidate, relation, stageRecords, supportedProgramAcceptance, correctRejection, preservation, foreignAssumptions, identity, compositionProof }) {
  object(contract, "composition contract");
  object(candidate, "candidate");
  object(identity, "compiler_proof.candidate_identity");
  object(compositionProof, "compiler_proof.proof_object");
  const expectedCandidate = candidateIdentityProjection(candidate.candidate_identity);
  if (!equal(candidateIdentityProjection(identity), expectedCandidate)) fail("identity-mismatch", "compiler_proof.candidate_identity differs from candidate.candidate_identity");
  if (stageRecords.size !== contract.stage_order.length) fail("blocked", "candidate proof does not cover every required stage");
  for (const stage of contract.stage_order) if (!stageRecords.has(stage)) fail("blocked", `candidate proof is missing stage ${stage}`);
  const relationSummary = checkRelation(relation, contract, stageRecords);
  const acceptance = checkSupportedAcceptance(supportedProgramAcceptance, candidate, candidate.dependencies.source, stageRecords, contract);
  const rejection = checkCorrectRejection(correctRejection, candidate, candidate.dependencies.source, stageRecords);
  const preservationSummary = checkPreservation(preservation, contract, stageRecords, candidate, candidate.dependencies.source);
  const assumptions = checkAssumptions(foreignAssumptions, contract);
  return {
    schema: "jet.compiler-proof-composition-check.v1",
    schema_version: 1,
    stage: "composition",
    status: "qualified",
    theorem: { id: contract.theorem.id, relation: contract.theorem.relation, claim: contract.theorem.conditional_claim, universal_proof: false },
    candidate_identity: { ...identity, candidate_identity_sha256: candidateIdentityDigest(candidate.candidate_identity) },
    proof_object: compositionProof,
    relation: relationSummary,
    stages: contract.stage_order.map((stage) => {
      const record = stageRecords.get(stage);
      return { stage, schema: record.schema, status: record.status, candidate_identity_sha256: record.candidate_identity_sha256, proof: record.proof, coverage: record.coverage };
    }),
    supported_program_acceptance: acceptance,
    correct_rejection: rejection,
    preservation: preservationSummary,
    foreign_assumptions: assumptions,
    proof_boundary: {
      qualification: "conditional",
      universal_proof: false,
      source_model_validity: "separate premise",
      implementation_correspondence: "checked per stage proof object",
      compiler_construction_trust: "separate premise",
      physical_machine_theorem: false,
      reason: "all listed Jet-controlled stages discharged under one candidate-bound refinement relation and an approved composition semantic goal replayed by the pinned kernel; named foreign assumptions remain outside the theorem",
    },
  };
}
