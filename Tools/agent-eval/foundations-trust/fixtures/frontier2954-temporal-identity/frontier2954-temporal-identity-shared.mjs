function revisionObservation(guard) {
  const initial = { revision: 1, value: "before-write" };
  const current = { ...initial, revision: 2, value: "after-write" };
  const attached = { ...initial };
  const stale = attached.revision !== current.revision;
  const accepted = guard ? !stale : true;
  return {
    attached_revision: attached.revision,
    current_revision: current.revision,
    stale,
    accepted,
    failure_reason: accepted ? null : "revision-mismatch",
    observation: accepted ? "published-attached-value" : "publication-rejected",
  };
}

function generationObservation(guard) {
  const original = { slot: 0, generation: 1, label: "original", writes: 0 };
  const replacement = { slot: 0, generation: 2, label: "replacement", writes: 0 };
  const stale = original.slot === replacement.slot && original.generation !== replacement.generation;
  const accepted = guard ? !stale : true;
  return {
    retained_handle: { slot: original.slot, generation: original.generation },
    replacement_handle: { slot: replacement.slot, generation: replacement.generation },
    slot_reused: true,
    stale,
    accepted,
    failure_reason: accepted ? null : "generation-mismatch",
    resolved_label: accepted ? "stale-handle-write" : null,
  };
}

function lifetimeObservation(guard) {
  const resource = { generation: 1, open: true, callback_writes: 0 };
  const deferred = { generation: resource.generation };
  resource.open = false;
  const lifetimeViolation = !resource.open;
  const accepted = guard ? !lifetimeViolation : true;
  if (accepted) resource.callback_writes += 1;
  return {
    resource_generation: resource.generation,
    deferred_generation: deferred.generation,
    closed_before_callback: true,
    lifetime_violation: lifetimeViolation,
    accepted,
    failure_reason: accepted ? null : "resource-closed",
    callback_writes: resource.callback_writes,
  };
}

function fixedStepSeconds(frameRate, totalSeconds, fixedStepSeconds) {
  let accumulator = 0;
  let fixedSteps = 0;
  const frameDelta = totalSeconds / frameRate;
  for (let frame = 0; frame < frameRate; frame += 1) {
    accumulator += frameDelta;
    while (accumulator >= fixedStepSeconds) {
      accumulator -= fixedStepSeconds;
      fixedSteps += 1;
    }
  }
  return { frame_rate: frameRate, fixed_steps: fixedSteps, remainder: accumulator };
}

function fixedStepTicks(frameRate, totalSeconds, ticksPerSecond, fixedStepTicks) {
  const totalTicks = totalSeconds * ticksPerSecond;
  const frameIncrement = totalTicks / frameRate;
  let accumulator = 0;
  let fixedSteps = 0;
  for (let frame = 0; frame < frameRate; frame += 1) {
    accumulator += frameIncrement;
    while (accumulator >= fixedStepTicks) {
      accumulator -= fixedStepTicks;
      fixedSteps += 1;
    }
  }
  return { frame_rate: frameRate, fixed_steps: fixedSteps, remainder_ticks: accumulator };
}

export function runTemporalJob(contract, guard) {
  const clock = contract.clock;
  const seconds = clock.frame_rates.map((frameRate) => fixedStepSeconds(
    frameRate,
    clock.total_seconds,
    clock.fixed_step_ticks / clock.ticks_per_second,
  ));
  const exact_ticks = clock.frame_rates.map((frameRate) => fixedStepTicks(
    frameRate,
    clock.total_seconds,
    clock.ticks_per_second,
    clock.fixed_step_ticks,
  ));
  const schedules = guard ? exact_ticks : seconds;
  return {
    schema: "jet.frontier-study-observation.v1",
    study_id: contract.study_id,
    job_id: contract.job_id,
    contract_id: contract.contract_id,
    variant: guard ? "intervention" : "baseline",
    temporal_observations: {
      revision_identity: revisionObservation(guard),
      resource_generation: generationObservation(guard),
      resource_lifetime: lifetimeObservation(guard),
      clock_representation: {
        representation: guard ? "exact-integer-ticks" : "binary64-seconds",
        schedules,
        fixed_step_counts_match: schedules.every((schedule) => schedule.fixed_steps === 60),
        schedules_match_baseline: schedules.every((schedule) => schedule.fixed_steps === schedules[0].fixed_steps),
      },
    },
  };
}
