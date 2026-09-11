import { existsSync, readFileSync, writeFileSync } from "node:fs";

import { canonicalJson, digestText } from "../../../scripts/agent/compiler-proof.mjs";

export const OBSERVATION_FIELDS = Object.freeze([
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

export const NULLABLE_OBSERVATION_FIELDS = Object.freeze(["typed_failure", "mutation"]);

function operationSet(operationIds) {
  if (operationIds === undefined || operationIds === null) return null;
  return operationIds instanceof Set ? operationIds : new Set(operationIds);
}

function missingFields(observations, nullableFields) {
  const nullable = new Set(nullableFields);
  return OBSERVATION_FIELDS.filter((field) => !Object.hasOwn(observations, field)
    || observations[field] === undefined
    || (observations[field] === null && !nullable.has(field)));
}

/**
 * Read the bounded producer journal shared by adapter, lowering, optimization,
 * and comptime observation workflows. The journal is JSONL so a producer can
 * append one typed record per operation without inventing an aggregate status.
 */
export function readComparisonJournal(path, { mode, operationIds, nullableFields = NULLABLE_OBSERVATION_FIELDS } = {}) {
  if (!existsSync(path)) return { records: new Map(), journal_count: 0, errors: [] };
  let lines;
  try {
    lines = readFileSync(path, "utf8").split(/\r?\n/u).filter(Boolean);
  } catch (error) {
    return { records: new Map(), journal_count: 0, errors: [`observation journal could not be read: ${error.message}`] };
  }
  const known = operationSet(operationIds);
  const records = new Map();
  const errors = [];
  for (const line of lines) {
    let payload;
    try {
      payload = JSON.parse(line);
    } catch (error) {
      errors.push(`observation journal emitted invalid JSON: ${error.message}`);
      continue;
    }
    const operationId = payload?.operation_id;
    if (!payload || typeof payload !== "object" || Array.isArray(payload)
      || (mode !== undefined && payload.mode !== mode)
      || typeof operationId !== "string"
      || (known && !known.has(operationId))) {
      errors.push("observation journal identity is invalid");
      continue;
    }
    if (records.has(operationId)) {
      errors.push(`observation journal duplicates ${operationId}`);
      continue;
    }
    const observations = payload.observations;
    if (!observations || typeof observations !== "object" || Array.isArray(observations)) {
      errors.push(`observation journal record ${operationId} has no observations object`);
      continue;
    }
    const missing = missingFields(observations, nullableFields);
    if (missing.length > 0) {
      errors.push(`observation journal record ${operationId} omits fields: ${missing.join(", ")}`);
      continue;
    }
    records.set(operationId, {
      operation_id: operationId,
      mode: payload.mode,
      observations,
      record_sha256: digestText(canonicalJson(payload)),
    });
  }
  return { records, journal_count: lines.length, errors };
}

function observationStrings(value, label) {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    throw new Error(`${label} must be an array of strings for ordered observation comparison`);
  }
  return value;
}

/** Build the canonical jet test-compare corpus for two typed observations. */
export function buildComparisonCorpus({ operationId, referenceMode, candidateMode, reference, candidate, source = `operation:${operationId}` }) {
  const sample = {
    case_id: `${operationId}:${referenceMode}->${candidateMode}`,
    input_id: operationId,
    reference: canonicalJson(reference),
    candidate: canonicalJson(candidate),
    reference_effects: observationStrings(reference.ordered_effects, `${operationId}@${referenceMode}.ordered_effects`),
    candidate_effects: observationStrings(candidate.ordered_effects, `${operationId}@${candidateMode}.ordered_effects`),
    reference_cleanup: observationStrings(reference.cleanup, `${operationId}@${referenceMode}.cleanup`),
    candidate_cleanup: observationStrings(candidate.cleanup, `${operationId}@${candidateMode}.cleanup`),
  };
  if (typeof reference.typed_failure === "string" && typeof candidate.typed_failure === "string") {
    sample.reference_failure = reference.typed_failure;
    sample.candidate_failure = candidate.typed_failure;
  }
  if (typeof reference.mutation === "string" && typeof candidate.mutation === "string") {
    sample.reference_mutation = reference.mutation;
    sample.candidate_mutation = candidate.mutation;
  }
  return {
    schema_version: 1,
    relation: "ordered_effects",
    source,
    tool: "jet",
    target: `${referenceMode}->${candidateMode}`,
    cases: [sample],
    universal_proof: false,
  };
}

export function writeComparisonCorpus(path, input) {
  const corpus = buildComparisonCorpus(input);
  writeFileSync(path, `${canonicalJson(corpus)}\n`, "utf8");
  return corpus;
}
