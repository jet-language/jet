import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const FIXTURE_DIR = path.dirname(fileURLToPath(import.meta.url));
const contractPath = path.join(FIXTURE_DIR, "frontier2954-semantic-contract.json");
const contract = JSON.parse(readFileSync(contractPath, "utf8"));

function observation(operation, result, typedFailure, mutation, items, events) {
  return {
    id: operation.id,
    result,
    typed_failure: typedFailure,
    mutation,
    input_consumption: { items },
    event_order: events,
  };
}

function run(operation) {
  switch (operation.kind) {
    case "bounds-check": {
      const valid = operation.value >= 0 && operation.value < operation.bound;
      return observation(
        operation,
        valid ? { status: "ok", value: operation.value } : { status: "failed", value: null },
        valid ? null : { code: "Bounds", index: operation.value, bound: operation.bound },
        null,
        1,
        [operation.id],
      );
    }
    case "mutation": {
      const after = operation.initial + operation.delta;
      return observation(
        operation,
        { status: "ok", value: after },
        null,
        { before: operation.initial, after },
        1,
        [operation.id],
      );
    }
    case "fallible-divide": {
      const valid = operation.denominator !== 0;
      return observation(
        operation,
        valid
          ? { status: "ok", value: operation.numerator / operation.denominator }
          : { status: "failed", value: null },
        valid ? null : { code: "DivideByZero" },
        null,
        1,
        [operation.id],
      );
    }
    case "pure-map": {
      const values = [];
      const events = [];
      for (let index = 0; index < operation.values.length; index += 1) {
        values.push(operation.values[index] * operation.factor);
        events.push(`${operation.id}:${index}`);
      }
      return observation(
        operation,
        { status: "ok", value: values },
        null,
        null,
        values.length,
        events,
      );
    }
    default:
      throw new Error(`unknown semantic operation ${operation.kind}`);
  }
}

const observations = contract.operations.map(run);
process.stdout.write(`${JSON.stringify({
  schema: "jet.frontier-study-observation.v1",
  study_id: contract.study_id,
  job_id: contract.job_id,
  contract_id: contract.contract_id,
  variant: "baseline",
  observations,
})}\n`);
