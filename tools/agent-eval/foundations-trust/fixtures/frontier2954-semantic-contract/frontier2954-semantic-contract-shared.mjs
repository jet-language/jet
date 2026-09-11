export const CONTRACT_ID = "frontier2954-semantic-contract-v1";

function operationObservation(operation, result, typedFailure, mutation, inputItems, eventOrder) {
  return {
    id: operation.id,
    result,
    typed_failure: typedFailure,
    mutation,
    input_consumption: { items: inputItems },
    event_order: eventOrder,
  };
}

export function executeOperation(operation) {
  if (operation.kind === "bounds-check") {
    const valid = operation.value >= 0 && operation.value < operation.bound;
    return operationObservation(
      operation,
      valid
        ? { status: "ok", value: operation.value }
        : { status: "failed", value: null },
      valid ? null : { code: "Bounds", index: operation.value, bound: operation.bound },
      null,
      1,
      [operation.id],
    );
  }

  if (operation.kind === "mutation") {
    const after = operation.initial + operation.delta;
    return operationObservation(
      operation,
      { status: "ok", value: after },
      null,
      { before: operation.initial, after },
      1,
      [operation.id],
    );
  }

  if (operation.kind === "fallible-divide") {
    const valid = operation.denominator !== 0;
    return operationObservation(
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

  if (operation.kind === "pure-map") {
    const values = operation.values.map((value, index) => value * operation.factor);
    return operationObservation(
      operation,
      { status: "ok", value: values },
      null,
      null,
      operation.values.length,
      operation.values.map((_, index) => `${operation.id}:${index}`),
    );
  }

  throw new Error(`unknown semantic operation ${operation.kind}`);
}

export function executeContract(contract) {
  if (contract.contract_id && contract.contract_id !== CONTRACT_ID) {
    throw new Error(`unexpected semantic contract ${contract.contract_id}`);
  }
  return contract.operations.map(executeOperation);
}
