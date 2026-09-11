function executeRecord(contract) {
  const mapped = [];
  const events = [];
  let total = 0;
  for (let index = 0; index < contract.records.length; index += 1) {
    const value = contract.records[index];
    events.push({ kind: "map", index, value });
    if (value === contract.failure_value) {
      events.push({ kind: "failure", index, value });
      return {
        status: "failed",
        result: null,
        typed_failure: { code: "CallbackFailure", value, index },
        mutation: { total_before_failure: total, mapped_values: [...mapped] },
        input_consumption: { records: index + 1 },
        event_order: events,
      };
    }
    const mappedValue = value * contract.factor;
    mapped.push(mappedValue);
    total += mappedValue;
    events.push({ kind: "reduce", index, value: mappedValue });
  }
  return {
    status: "ok",
    result: { total, mapped_values: mapped },
    typed_failure: null,
    mutation: { total, mapped_values: mapped },
    input_consumption: { records: contract.records.length },
    event_order: events,
  };
}

function reassociationWitness() {
  const first = 1e16;
  const second = -1e16;
  const third = 1;
  const left = (first + second) + third;
  const right = first + (second + third);
  return {
    left,
    right,
    changes_defined_observation: left !== right,
  };
}

export function runScalar(contract) {
  return {
    schema: "jet.frontier-study-observation.v1",
    study_id: contract.study_id,
    job_id: contract.job_id,
    contract_id: contract.contract_id,
    variant: "baseline",
    plan: {
      name: "scalar-map-then-reduce",
      legality: "baseline",
      profitability: "not-selected",
    },
    observations: executeRecord(contract),
    controls: {
      invalid_reassociation: {
        accepted: true,
        legality: "not-checked",
        witness: reassociationWitness(),
      },
    },
  };
}

export function runCheckedFusion(contract) {
  const observations = executeRecord(contract);
  const witness = reassociationWitness();
  return {
    schema: "jet.frontier-study-observation.v1",
    study_id: contract.study_id,
    job_id: contract.job_id,
    contract_id: contract.contract_id,
    variant: "intervention",
    plan: {
      name: "checked-fused-map-reduce",
      legality: "checked",
      profitability: "measured-by-driver",
      fallback: "scalar-map-then-reduce",
    },
    observations,
    controls: {
      invalid_reassociation: {
        accepted: false,
        legality: "rejected",
        witness,
        rejection_reason: contract.invalid_control.reason,
      },
    },
  };
}
