import path from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

const DEMONSTRATOR_FILE = fileURLToPath(import.meta.url);
const REPO_ROOT = path.resolve(path.dirname(DEMONSTRATOR_FILE), "../../..");
const DEMONSTRATOR_PATH = path.relative(REPO_ROOT, DEMONSTRATOR_FILE).split(path.sep).join("/");
const REPORT_SERIES_PATH = "docs/audits/jet-foundations-and-trust-2026-09-04/";

const EVENT_NAMES = Object.freeze(["read", "write", "publish"]);
const INITIAL_EVENT_STATE = Object.freeze({ revision: 1, value: "before-write" });
const WRITTEN_EVENT_VALUE = "after-write";

function copyFact(fact) {
  return fact === null ? null : { revision: fact.revision, value: fact.value };
}

function copyState(state) {
  return { revision: state.revision, value: state.value };
}

function permutations(values) {
  if (values.length === 0) return [[]];
  const result = [];
  for (let index = 0; index < values.length; index += 1) {
    const head = values[index];
    const rest = values.slice(0, index).concat(values.slice(index + 1));
    for (const tail of permutations(rest)) result.push([head, ...tail]);
  }
  return result;
}

function runEventOrder(order) {
  const state = copyState(INITIAL_EVENT_STATE);
  const readIndex = order.indexOf("read");
  const publishIndex = order.indexOf("publish");
  const legal = readIndex < publishIndex;
  let readFact = null;
  let publication = null;
  const trace = [];

  for (const event of order) {
    if (event === "read") {
      readFact = copyState(state);
      trace.push({
        event,
        observedFact: copyFact(readFact),
        stateAfter: copyState(state),
      });
      continue;
    }

    if (event === "write") {
      const stateBefore = copyState(state);
      state.revision += 1;
      state.value = WRITTEN_EVENT_VALUE;
      trace.push({
        event,
        stateBefore,
        stateAfter: copyState(state),
      });
      continue;
    }

    const stateAtPublish = copyState(state);
    const attachedFact = copyFact(readFact);
    const staleRevision = legal && attachedFact !== null &&
      attachedFact.revision !== stateAtPublish.revision;
    publication = {
      valid: legal,
      invalidReason: legal ? null : "publish-before-read",
      attachedFact,
      stateAtPublish,
      staleRevision,
    };
    trace.push({
      event,
      ...publication,
      stateAfter: copyState(state),
    });
  }

  return {
    order: [...order],
    legal,
    invalidReason: legal ? null : "publish-before-read",
    finalState: copyState(state),
    publication,
    staleRevisionFactAttached: publication?.staleRevision ?? false,
    trace,
  };
}

const allEventResults = permutations(EVENT_NAMES).map(runEventOrder);
const legalEventResults = allEventResults.filter((result) => result.legal);
const invalidEventResults = allEventResults.filter((result) => !result.legal);
const staleEventResults = legalEventResults.filter(
  (result) => result.staleRevisionFactAttached,
);

function binary64Hex(value) {
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setFloat64(0, value, false);
  return Array.from(new Uint8Array(bytes), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

function describeNumber(value) {
  return {
    value,
    text: String(value),
    binary64Hex: binary64Hex(value),
    isNegativeZero: Object.is(value, -0),
    isFinite: Number.isFinite(value),
  };
}

function runReassociationWitness() {
  const first = 1e16;
  const second = -1e16;
  const third = 1;
  const firstPair = first + second;
  const left = firstPair + third;
  const secondPair = second + third;
  const right = first + secondPair;

  return {
    inputs: {
      operands: {
        first: describeNumber(first),
        second: describeNumber(second),
        third: describeNumber(third),
      },
      groupings: [
        "(first + second) + third",
        "first + (second + third)",
      ],
      representation: "JavaScript Number arithmetic represented as IEEE-754 binary64",
    },
    outputs: {
      left: describeNumber(left),
      right: describeNumber(right),
      reassociationChangesValue: left !== right,
    },
    counterexampleTrace: [
      {
        expression: "first + second",
        value: describeNumber(firstPair),
      },
      {
        expression: "(first + second) + third",
        value: describeNumber(left),
      },
      {
        expression: "second + third",
        value: describeNumber(secondPair),
      },
      {
        expression: "first + (second + third)",
        value: describeNumber(right),
      },
    ],
  };
}

function copyEntity(entity) {
  return entity === null ? null : { label: entity.label, writes: entity.writes };
}

function snapshotSlots(slots) {
  return slots.map((slot, index) => ({
    slot: index,
    generation: slot.generation,
    entity: copyEntity(slot.entity),
  }));
}

function runEntityReuse(checkGeneration) {
  const slots = [{ generation: 0, entity: null }];
  const trace = [];
  let step = 0;

  function record(operation, details = {}) {
    trace.push({
      step: (step += 1),
      operation,
      ...details,
      slots: snapshotSlots(slots),
    });
  }

  function allocate(label) {
    const slotIndex = slots.findIndex((slot) => slot.entity === null);
    if (slotIndex < 0) throw new Error("model has no free slot");
    const slot = slots[slotIndex];
    slot.generation += 1;
    slot.entity = { label, writes: 0 };
    const handle = { slot: slotIndex, generation: slot.generation };
    record("allocate", { label, handle: { ...handle } });
    return handle;
  }

  function release(handle) {
    const slot = slots[handle.slot];
    if (
      slot === undefined ||
      slot.entity === null ||
      slot.generation !== handle.generation
    ) {
      throw new Error("model release received an invalid handle");
    }
    slot.entity = null;
    record("release", { handle: { ...handle } });
  }

  function resolve(handle) {
    const slot = slots[handle.slot];
    if (slot === undefined || slot.entity === null) {
      return { slot: null, reason: "empty-or-missing-slot" };
    }
    if (checkGeneration && slot.generation !== handle.generation) {
      return { slot: null, reason: "generation-mismatch" };
    }
    return {
      slot,
      reason: checkGeneration ? "slot-and-generation-match" : "slot-only-match",
    };
  }

  const originalHandle = allocate("original");
  const retainedHandle = { ...originalHandle };
  release(retainedHandle);
  const replacementHandle = allocate("replacement");
  const resolution = resolve(retainedHandle);
  const resolvedEntityBeforeWrite = resolution.slot
    ? copyEntity(resolution.slot.entity)
    : null;

  let accepted = false;
  let rejectionReason = null;
  if (resolution.slot === null) {
    rejectionReason = resolution.reason;
  } else {
    resolution.slot.entity.label = "stale-handle-write";
    resolution.slot.entity.writes += 1;
    accepted = true;
  }

  record("stale-handle-write", {
    handle: { ...retainedHandle },
    generationCheck: checkGeneration,
    accepted,
    rejectionReason,
    resolvedEntityBeforeWrite,
  });

  return {
    inputs: {
      slotCount: slots.length,
      initialSlots: [{ slot: 0, generation: 0, entity: null }],
      operationOrder: [
        "allocate original",
        "retain original handle",
        "release original",
        "allocate replacement in reused slot",
        "use retained original handle",
      ],
      generationCheck: checkGeneration,
    },
    outputs: {
      originalHandle,
      retainedHandle,
      replacementHandle,
      slotWasReused:
        replacementHandle.slot === retainedHandle.slot &&
        replacementHandle.generation !== retainedHandle.generation,
      staleHandleWrite: {
        accepted,
        rejectionReason,
        resolvedEntityBeforeWrite,
      },
      finalSlots: snapshotSlots(slots),
    },
    trace,
  };
}

const entityReuseWithoutCheck = runEntityReuse(false);
const entityReuseWithCheck = runEntityReuse(true);
const entityCounterexampleTraces = [entityReuseWithoutCheck, entityReuseWithCheck]
  .filter((run) => run.outputs.staleHandleWrite.accepted && run.outputs.slotWasReused)
  .map((run) => ({
    generationCheck: run.inputs.generationCheck,
    trace: run.trace,
  }));

function runFixedStepSchedule(
  frameRate,
  totalTimeSeconds,
  fixedStepSeconds,
  velocityUnitsPerSecond,
  maxStepsPerFrame,
) {
  const frameCount = frameRate;
  const frameDelta = totalTimeSeconds / frameCount;
  let accumulator = 0;
  let simulationTime = 0;
  let position = 0;
  let fixedSteps = 0;
  const frameTrace = [];

  for (let frame = 1; frame <= frameCount; frame += 1) {
    const accumulatorBefore = accumulator;
    accumulator += frameDelta;
    let stepsThisFrame = 0;

    while (
      accumulator >= fixedStepSeconds &&
      stepsThisFrame < maxStepsPerFrame
    ) {
      accumulator -= fixedStepSeconds;
      simulationTime += fixedStepSeconds;
      position += velocityUnitsPerSecond * fixedStepSeconds;
      fixedSteps += 1;
      stepsThisFrame += 1;
    }

    frameTrace.push({
      frame,
      frameDelta,
      accumulatorBefore,
      stepsThisFrame,
      accumulatorAfter: accumulator,
      fixedSteps,
      maxStepsReached: stepsThisFrame === maxStepsPerFrame,
      wholeStepPendingAfterCap: accumulator >= fixedStepSeconds,
    });
  }

  const wallTime = frameTrace.reduce((sum, frame) => sum + frame.frameDelta, 0);
  const countedFixedSteps = frameTrace.reduce(
    (sum, frame) => sum + frame.stepsThisFrame,
    0,
  );
  const maxStepBlockedFrames = frameTrace
    .filter((frame) => frame.wholeStepPendingAfterCap)
    .map((frame) => frame.frame);
  return {
    inputs: {
      frameRate,
      frameCount,
      frameDelta,
      totalTimeSeconds,
      fixedStepSeconds,
      velocityUnitsPerSecond,
      maxStepsPerFrame,
    },
    outputs: {
      framesProcessed: frameTrace.length,
      fixedStepEvents: fixedSteps,
      countedFixedStepEvents: countedFixedSteps,
      wallTime,
      wallTimeError: wallTime - totalTimeSeconds,
      simulatedTime: simulationTime,
      position,
      accumulatorRemainder: accumulator,
      accumulatorRemainderText: String(accumulator),
      maxStepBlockedFrames,
      wholeStepPendingAfterFinalFrame: accumulator >= fixedStepSeconds,
    },
    trace: frameTrace,
  };
}

const accumulatorTotalTimeSeconds = 1;
const accumulatorFixedStepSeconds = 1 / 60;
const accumulatorInputs = {
  totalTimeSeconds: accumulatorTotalTimeSeconds,
  fixedStepSeconds: accumulatorFixedStepSeconds,
  velocityUnitsPerSecond: 1,
  frameRates: [30, 60, 144],
  maxStepsPerFrame: Math.ceil(
    accumulatorTotalTimeSeconds / accumulatorFixedStepSeconds,
  ),
  representation: "JavaScript Number arithmetic represented as IEEE-754 binary64",
};
const accumulatorRuns = accumulatorInputs.frameRates.map((frameRate) =>
  runFixedStepSchedule(
    frameRate,
    accumulatorInputs.totalTimeSeconds,
    accumulatorInputs.fixedStepSeconds,
    accumulatorInputs.velocityUnitsPerSecond,
    accumulatorInputs.maxStepsPerFrame,
  ),
);

const accumulatorBaseline = accumulatorRuns[0].outputs;
const accumulatorComparisons = accumulatorRuns.map((run) => ({
  frameRate: run.inputs.frameRate,
  sameFixedStepCount:
    run.outputs.fixedStepEvents === accumulatorBaseline.fixedStepEvents,
  sameSimulatedTime: Object.is(
    run.outputs.simulatedTime,
    accumulatorBaseline.simulatedTime,
  ),
  samePosition: Object.is(run.outputs.position, accumulatorBaseline.position),
  sameAccumulatorRemainder: Object.is(
    run.outputs.accumulatorRemainder,
    accumulatorBaseline.accumulatorRemainder,
  ),
}));
const accumulatorCounterexampleTraces = accumulatorRuns
  .map((run, index) => ({ run, comparison: accumulatorComparisons[index] }))
  .filter(
    ({ comparison }) =>
      !comparison.sameFixedStepCount || !comparison.sameSimulatedTime || !comparison.samePosition,
  )
  .map(({ run, comparison }) => ({
    frameRate: run.inputs.frameRate,
    comparison,
    trace: run.trace,
  }));
function runExactTickSchedule(
  frameRate,
  totalTimeSeconds,
  ticksPerSecond,
  frameIncrementTicks,
  fixedStepTicks,
  maxStepsPerFrame,
) {
  const frameCount = frameRate;
  let accumulatorTicks = 0;
  let simulatedTicks = 0;
  let fixedSteps = 0;
  const frameTrace = [];

  for (let frame = 1; frame <= frameCount; frame += 1) {
    const accumulatorBeforeTicks = accumulatorTicks;
    accumulatorTicks += frameIncrementTicks;
    let stepsThisFrame = 0;

    while (
      accumulatorTicks >= fixedStepTicks &&
      stepsThisFrame < maxStepsPerFrame
    ) {
      accumulatorTicks -= fixedStepTicks;
      simulatedTicks += fixedStepTicks;
      fixedSteps += 1;
      stepsThisFrame += 1;
    }

    frameTrace.push({
      frame,
      frameIncrementTicks,
      accumulatorBeforeTicks,
      stepsThisFrame,
      accumulatorAfterTicks: accumulatorTicks,
      fixedSteps,
      maxStepsReached: stepsThisFrame === maxStepsPerFrame,
      wholeStepPendingAfterCap: accumulatorTicks >= fixedStepTicks,
    });
  }

  const wallTicks = frameTrace.reduce(
    (sum, frame) => sum + frame.frameIncrementTicks,
    0,
  );
  const countedFixedSteps = frameTrace.reduce(
    (sum, frame) => sum + frame.stepsThisFrame,
    0,
  );
  const maxStepBlockedFrames = frameTrace
    .filter((frame) => frame.wholeStepPendingAfterCap)
    .map((frame) => frame.frame);
  const totalTimeTicks = totalTimeSeconds * ticksPerSecond;
  return {
    inputs: {
      frameRate,
      frameCount,
      totalTimeSeconds,
      ticksPerSecond,
      frameIncrementTicks,
      fixedStepTicks,
      maxStepsPerFrame,
    },
    outputs: {
      framesProcessed: frameTrace.length,
      fixedStepEvents: fixedSteps,
      countedFixedStepEvents: countedFixedSteps,
      wallTicks,
      wallSeconds: wallTicks / ticksPerSecond,
      wallTickError: wallTicks - totalTimeTicks,
      simulatedTicks,
      simulatedTimeSeconds: simulatedTicks / ticksPerSecond,
      accumulatorRemainderTicks: accumulatorTicks,
      accumulatorRemainderSeconds: accumulatorTicks / ticksPerSecond,
      maxStepBlockedFrames,
      wholeStepPendingAfterFinalFrame: accumulatorTicks >= fixedStepTicks,
    },
    trace: frameTrace,
  };
}

const exactTicksPerSecond = 720;
const exactFixedStepTicks = 12;
const exactFrameRates = [...accumulatorInputs.frameRates];
const exactTickInputs = {
  totalTimeSeconds: accumulatorTotalTimeSeconds,
  ticksPerSecond: exactTicksPerSecond,
  fixedStepTicks: exactFixedStepTicks,
  frameRates: exactFrameRates,
  frameIncrementTicksByFrameRate: exactFrameRates.map((frameRate) => ({
    frameRate,
    frameIncrementTicks: exactTicksPerSecond / frameRate,
  })),
  maxStepsPerFrame: Math.ceil(
    (accumulatorTotalTimeSeconds * exactTicksPerSecond) /
      exactFixedStepTicks,
  ),
  representation: "Exact integer clock ticks; 720 ticks per second",
};
const exactTickRuns = exactTickInputs.frameIncrementTicksByFrameRate.map(
  ({ frameRate, frameIncrementTicks }) =>
    runExactTickSchedule(
      frameRate,
      exactTickInputs.totalTimeSeconds,
      exactTickInputs.ticksPerSecond,
      frameIncrementTicks,
      exactTickInputs.fixedStepTicks,
      exactTickInputs.maxStepsPerFrame,
    ),
);
const exactTickBaseline = exactTickRuns[0].outputs;
const exactTickComparisons = exactTickRuns.map((run) => ({
  frameRate: run.inputs.frameRate,
  sameFixedStepCount:
    run.outputs.fixedStepEvents === exactTickBaseline.fixedStepEvents,
  sameSimulatedTicks:
    run.outputs.simulatedTicks === exactTickBaseline.simulatedTicks,
  sameAccumulatorRemainder:
    run.outputs.accumulatorRemainderTicks ===
    exactTickBaseline.accumulatorRemainderTicks,
  sameWallTicks: run.outputs.wallTicks === exactTickBaseline.wallTicks,
}));
const exactTickCounterexampleTraces = exactTickRuns
  .map((run, index) => ({ run, comparison: exactTickComparisons[index] }))
  .filter(
    ({ comparison }) =>
      !comparison.sameFixedStepCount ||
      !comparison.sameSimulatedTicks ||
      !comparison.sameAccumulatorRemainder ||
      !comparison.sameWallTicks,
  )
  .map(({ run, comparison }) => ({
    frameRate: run.inputs.frameRate,
    comparison,
    trace: run.trace,
  }));
const clockRepresentationComparisons = accumulatorRuns.map(
  (secondsRun, index) => {
    const exactRun = exactTickRuns[index];
    return {
      frameRate: secondsRun.inputs.frameRate,
      secondsFixedStepEvents: secondsRun.outputs.fixedStepEvents,
      exactTickFixedStepEvents: exactRun.outputs.fixedStepEvents,
      sameFixedStepEventCount:
        secondsRun.outputs.fixedStepEvents === exactRun.outputs.fixedStepEvents,
      secondsAccumulatorRemainder: secondsRun.outputs.accumulatorRemainder,
      exactTickAccumulatorRemainderTicks:
        exactRun.outputs.accumulatorRemainderTicks,
      exactTickAccumulatorRemainderSeconds:
        exactRun.outputs.accumulatorRemainderSeconds,
      secondsWallTimeError: secondsRun.outputs.wallTimeError,
      exactTickWallTickError: exactRun.outputs.wallTickError,
    };
  },
);

const output = {
  format: "research-demonstrator-v1",
  execution: {
    command: `scripts/agent/jet-env full node ${DEMONSTRATOR_PATH}`,
    reportSeries: REPORT_SERIES_PATH,
    workerExecuted: false,
    note: "Worker wrote this file but did not execute it; Main must run the command above.",
  },
  scope: {
    independence: "Each case uses only native JavaScript and its own in-memory state.",
    relationshipToJet:
      "These are modeled protocol and JavaScript arithmetic observations, not current Jet runtime proof.",
  },
  assumptionsNeedingAuthorConfirmation: [
    {
      case: "stale-revision fact attachment",
      question:
        "This model treats only orders with read before publish as legal; publish-before-read orders are invalid and excluded from stale counterexamples. AstraSynthesis confirmed this model.",
    },
    {
      case: "fixed-step accumulator",
      question:
        "This model uses one nominal second, binary64 frame seconds plus an exact 720-ticks-per-second clock, fixed dt 1/60 second (12 ticks), enough max steps, and no dropped remainder. AstraSynthesis confirmed this model.",
    },
  ],
  cases: {
    staleRevisionFactAttachment: {
      status: "modeled protocol experiment; not Jet runtime proof",
      inputs: {
        initialState: copyState(INITIAL_EVENT_STATE),
        events: [...EVENT_NAMES],
        write: {
          revisionIncrement: 1,
          replacementValue: WRITTEN_EVENT_VALUE,
        },
        legalOrderRule: "read must precede publish",
        legalEventOrders: legalEventResults.map((result) => result.order),
        publishRule:
          "publish attaches most recent read fact and compares its revision with state revision at publish",
      },
      outputs: {
        allOrders: allEventResults,
        allOrderCount: allEventResults.length,
        legalOrderCount: legalEventResults.length,
        legalOrders: legalEventResults.map((result) => result.order),
        invalidOrders: invalidEventResults.map((result) => ({
          order: result.order,
          reason: result.invalidReason,
          trace: result.trace,
        })),
        staleRevisionOrderCount: staleEventResults.length,
        staleRevisionOrders: staleEventResults.map((result) => result.order),
      },
      counterexampleTraces: staleEventResults.map((result) => ({
        order: result.order,
        publication: result.publication,
        trace: result.trace,
      })),
      limits: [
        "Sequential in-memory model only; no threads, queues, transactions, persistence, or memory-order guarantees.",
        "Event names and revision policy are demonstrator inputs, not Jet semantics.",
      ],
    },

    ieee754Reassociation: {
      status: "computed JavaScript arithmetic experiment; not Jet runtime proof",
      ...runReassociationWitness(),
      limits: [
        "Covers one finite binary64 witness only; it does not establish behavior for decimal, fixed-point, arbitrary-precision, or Jet numeric types.",
        "No compiler optimization or reassociation policy is tested.",
      ],
    },

    entitySlotReuse: {
      status: "modeled handle-safety experiment; not Jet runtime proof",
      model: {
        representation: "One reusable slot with monotonically increasing integer generation.",
        initialConditions: "Slot 0 starts empty at generation 0.",
        guardBehavior: {
          withoutGenerationCheck: "Resolve retained handle by slot index only.",
          withGenerationCheck: "Require slot index and generation to match.",
        },
      },
      outputs: {
        withoutGenerationCheck: entityReuseWithoutCheck.outputs,
        withGenerationCheck: entityReuseWithCheck.outputs,
      },
      counterexampleTraces: entityCounterexampleTraces,
      limits: [
        "Single-threaded one-slot model only; no pointer representation, concurrent destruction, ABA timing, allocator behavior, or memory reclamation is modeled.",
        "The generation guard is an explicit model choice, not a claim about Jet handles.",
      ],
    },

    fixedStepAccumulator: {
      status: "modeled clock experiment; not Jet runtime proof",
      inputs: {
        secondsRepresentation: accumulatorInputs,
        exactTickRepresentation: exactTickInputs,
        timePolicy:
          "Process accumulated time in fixed steps, carry remainder forward, and use a max-step bound sized for this one-second experiment without dropping time.",
      },
      outputs: {
        secondsRepresentation: {
          schedules: accumulatorRuns.map((run) => ({
            inputs: run.inputs,
            outputs: run.outputs,
          })),
          comparisons: accumulatorComparisons,
          allSchedulesMatchFixedStepOutcome: accumulatorComparisons.every(
            (comparison) =>
              comparison.sameFixedStepCount &&
              comparison.sameSimulatedTime &&
              comparison.samePosition,
          ),
        },
        exactTickRepresentation: {
          schedules: exactTickRuns.map((run) => ({
            inputs: run.inputs,
            outputs: run.outputs,
          })),
          comparisons: exactTickComparisons,
          allSchedulesMatchFixedStepOutcome: exactTickComparisons.every(
            (comparison) =>
              comparison.sameFixedStepCount &&
              comparison.sameSimulatedTicks &&
              comparison.sameAccumulatorRemainder &&
              comparison.sameWallTicks,
          ),
        },
        crossRepresentationComparisons: clockRepresentationComparisons,
      },
      counterexampleTraces: [
        ...accumulatorCounterexampleTraces.map((entry) => ({
          representation: "binary64-seconds",
          ...entry,
        })),
        ...exactTickCounterexampleTraces.map((entry) => ({
          representation: "exact-integer-ticks",
          ...entry,
        })),
      ],
      traces: {
        seconds: accumulatorRuns.map((run) => ({
          frameRate: run.inputs.frameRate,
          frames: run.trace,
        })),
        exactTicks: exactTickRuns.map((run) => ({
          frameRate: run.inputs.frameRate,
          frames: run.trace,
        })),
      },
      limits: [
        "Schedules use nominal equal duration, not a real monotonic clock, jitter, stalls, interpolation, or clock drift.",
        "Binary64 seconds report floating-point wall-time and accumulator residuals; exact ticks report integer wall-tick and accumulator residuals.",
        "Max-step bound is sized for this one-second model; no accumulated time is dropped, and production loop policy may differ.",
        "Numeric clock observations are demonstrator results, not Jet clock semantics.",
      ],
    },
  },
};

const FRONTIER_STUDY_SCHEMA = "jet.frontier-study.v1";
const FRONTIER_ARTIFACT_SCHEMA = "jet.frontier-study-artifact.v1";
const FRONTIER_EVIDENCE_SCHEMA = "jet.frontier-evidence-class-validation.v1";
const FRONTIER_STUDY_ROOT = path.join(path.dirname(DEMONSTRATOR_FILE), "fixtures");
const FRONTIER_ENV_RUNNER = "scripts/agent/jet-env";
const FRONTIER_DEFAULT_TIMEOUT_MS = 30_000;
const FRONTIER_HUMAN_ABSENCE_REASON = "human transfer evidence is unavailable: agent/model output cannot establish whether a reader can explain, predict, modify, or derive; supply a consented observation record";
const FRONTIER_MAX_BUFFER = 4 * 1024 * 1024;
const FRONTIER_DISPOSITIONS = Object.freeze([
  "reject",
  "supported-with-scope",
  "unresolved",
]);
const FRONTIER_EVIDENCE_CLASSES = Object.freeze([
  "model",
  "runtime",
  "formal",
  "simulated-learner",
  "human",
]);
const FRONTIER_FALSIFIERS = Object.freeze({
  H1: "An explanation needs a second semantic interpreter; a valid derivation licenses a wrong transform; a source edit leaves a current-looking old fact; or the derivation cannot represent a required observation. A latency regression is a product problem even if the theorem succeeds.",
  H2: "A stale capability is accepted; ordinary safe reuse becomes inexpressible; callbacks erase the relation; or the implementation must maintain a separate lifecycle law for each subsystem.",
  H3: "The optimized program changes failures or results; the “faster” path wins only after changing input or output rules; an expert rejection uses a different semantic implementation; or dispatch costs erase the benefit. The current campaign runs no Jet benchmark, so these remain obligations under existing performance cards.",
  H4: "Gains disappear on transfer, are explained by extra time, or require a visual feature unavailable in ordinary editors. The source-based RLI5 assessment in this bundle does not answer that empirical question. [#2926](index.md#card-2926) owns it.",
});
const FRONTIER_PRIOR_WORK = Object.freeze({
  H1: Object.freeze([
    { name: "proof-carrying compilation", source: "https://doi.org/10.1145/367168.367176", comparison: "proof attached to compilation claims; not a complete Jet implementation correspondence" },
    { name: "executable semantics", source: "https://doi.org/10.1145/203477.203481", comparison: "executable meaning and observation rules; not a shared Jet fact consumer" },
    { name: "egglog", source: "https://github.com/egraphs-good/egglog", comparison: "relational equality saturation; not a source-bound Jet evidence record" },
  ]),
  H2: Object.freeze([
    { name: "typestate", source: "https://doi.org/10.1145/360204.360217", comparison: "state-indexed legal operations; not revision, generation, lifetime, and clock joins together" },
    { name: "generation handles", source: "https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2023/p1456r1.html", comparison: "stale slot protection; not a general Jet lifecycle contract" },
    { name: "revisioned incremental systems", source: "https://doi.org/10.1145/2509136.2509514", comparison: "dependency freshness; not resource generation or callback lifetime" },
  ]),
  H3: Object.freeze([
    { name: "Exo", source: "https://github.com/exo-lang/exo", comparison: "scheduling and tensor transformation control; not this observation-preservation route" },
    { name: "MEMOIR", source: "https://doi.org/10.1145/3563323", comparison: "memory transformation reasoning; not Jet's whole-job conditional cost contract" },
    { name: "Marmoset", source: "https://doi.org/10.1145/3520304.3529027", comparison: "compiler validation workloads; not this paired legality/profitability witness" },
    { name: "Indexed Streams", source: "https://doi.org/10.1145/3563323.3563338", comparison: "indexed stream transformations; not this defined failure and input-consumption relation" },
  ]),
  H4: Object.freeze([
    { name: "prediction-versus-production research", source: "https://doi.org/10.1145/3025453.3025636", comparison: "prediction can expose a learner model; not evidence of Jet transfer here" },
    { name: "SMoL misconceptions", source: "https://cs.brown.edu/~sk/Publications/Papers/Published/ghk-sme-plt-tutor/", comparison: "misconception-led programming instruction; not this source-bound reason record" },
    { name: "Python Tutor", source: "https://pythontutor.com/", comparison: "stepwise values and frames; not a claim of human effectiveness from this scripted route" },
  ]),
});
const FRONTIER_STUDIES = Object.freeze([
  Object.freeze({
    id: "H1-one-semantic-contract",
    key: "H1",
    title: "One semantic contract across routes",
    claim: "A shared derivation over canonical facts can drive checking, explanation, and optimization without an independent semantic rule in each consumer.",
    falsifier: FRONTIER_FALSIFIERS.H1,
    owners: Object.freeze(["#2945", "#2937", "#2939"]),
    proof_families: Object.freeze(["checking", "core", "lowering"]),
    fixture: "frontier2954-semantic-contract",
    contract: "frontier2954-semantic-contract.json",
    baseline: "frontier2954-semantic-contract-baseline.mjs",
    intervention: "frontier2954-semantic-contract-intervention.mjs",
  }),
  Object.freeze({
    id: "H2-temporal-identity",
    key: "H2",
    title: "Temporal identity and lifecycle reuse",
    claim: "Revision, generation, lifetime, and clock relations can reject stale work without erasing distinct temporal meanings or adding mandatory beginner annotations.",
    falsifier: FRONTIER_FALSIFIERS.H2,
    owners: Object.freeze(["#2949", "#2940", "#2937"]),
    proof_families: Object.freeze(["checking", "core", "runtime", "adapter"]),
    fixture: "frontier2954-temporal-identity",
    contract: "frontier2954-temporal-identity.json",
    baseline: "frontier2954-temporal-identity-baseline.mjs",
    intervention: "frontier2954-temporal-identity-intervention.mjs",
  }),
  Object.freeze({
    id: "H3-conditional-optimization",
    key: "H3",
    title: "Conditional optimization with preserved observations",
    claim: "Checked legality and measured whole-job cost can select an optimization while preserving defined results, failures, mutation, input consumption, and event order.",
    falsifier: FRONTIER_FALSIFIERS.H3,
    owners: Object.freeze(["#2951", "#2939", "#2950"]),
    proof_families: Object.freeze(["core", "lowering", "runtime"]),
    fixture: "frontier2954-conditional-optimization",
    contract: "frontier2954-conditional-optimization.json",
    baseline: "frontier2954-conditional-optimization-baseline.mjs",
    intervention: "frontier2954-conditional-optimization-intervention.mjs",
  }),
  Object.freeze({
    id: "H4-evidence-backed-explanation",
    key: "H4",
    title: "Evidence-backed explanations and unfamiliar transfer",
    claim: "A reason record linked to source and oracle identities can support prediction, controlled change, and an unfamiliar transfer task without claiming a human learning effect.",
    falsifier: FRONTIER_FALSIFIERS.H4,
    owners: Object.freeze(["#2945", "#2947", "#2926"]),
    proof_families: Object.freeze(["core", "checking"]),
    fixture: "frontier2954-explanation-transfer",
    contract: null,
    baseline: "frontier2954-explanation-transfer-baseline.mjs",
    intervention: "frontier2954-explanation-transfer-intervention.mjs",
  }),
]);

function frontierCanonical(value) {
  if (Array.isArray(value)) return value.map(frontierCanonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, frontierCanonical(value[key])]),
    );
  }
  return value;
}

function frontierCanonicalJson(value) {
  return JSON.stringify(frontierCanonical(value));
}

function frontierDigestBytes(value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value);
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function frontierDigest(value) {
  return frontierDigestBytes(Buffer.from(frontierCanonicalJson(value), "utf8"));
}

function frontierRelative(absolute) {
  return path.relative(REPO_ROOT, absolute).split(path.sep).join("/") || ".";
}

function frontierReadJson(relativePath) {
  const absolute = path.resolve(REPO_ROOT, relativePath);
  try {
    const bytes = readFileSync(absolute);
    return {
      status: "available",
      path: relativePath,
      digest: frontierDigestBytes(bytes),
      value: JSON.parse(bytes.toString("utf8")),
    };
  } catch (error) {
    return {
      status: "unavailable",
      path: relativePath,
      digest: null,
      value: null,
      reason: `cannot read ${relativePath}: ${error.message}`,
    };
  }
}

function frontierFileDigest(absolute) {
  try {
    if (!statSync(absolute).isFile()) return null;
    return frontierDigestBytes(readFileSync(absolute));
  } catch {
    return null;
  }
}
function frontierFileByteSize(absolute) {
  try {
    const info = statSync(absolute);
    return info.isFile() ? info.size : null;
  } catch {
    return null;
  }
}

function frontierFixtureFiles(absoluteRoot, current = absoluteRoot, result = []) {
  let entries;
  try {
    entries = readdirSync(current, { withFileTypes: true }).sort((left, right) =>
      left.name.localeCompare(right.name));
  } catch {
    return result;
  }
  for (const entry of entries) {
    const absolute = path.join(current, entry.name);
    if (entry.isDirectory()) frontierFixtureFiles(absoluteRoot, absolute, result);
    else if (entry.isFile()) {
      result.push({
        path: path.relative(absoluteRoot, absolute).split(path.sep).join("/"),
        digest: frontierFileDigest(absolute),
        bytes: frontierFileByteSize(absolute),
      });
    }
  }
  return result;
}


function frontierFixtureIdentity(study) {
  const absoluteRoot = path.join(FRONTIER_STUDY_ROOT, study.fixture);
  const files = frontierFixtureFiles(absoluteRoot);
  return {
    root: frontierRelative(absoluteRoot),
    files,
    digest: frontierDigest(files),
  };
}

function frontierTaskSourceIdentity(task) {
  if (typeof task?.source?.path !== "string") {
    return { path: null, digest: null };
  }
  const candidates = [
    path.join("examples/learn", task.source.path),
    path.join("examples/learn/feedback", task.source.path),
  ];
  const sourcePath = candidates.find((candidate) =>
    existsSync(path.resolve(REPO_ROOT, candidate))) ?? candidates[0];
  return {
    path: sourcePath,
    digest: frontierFileDigest(path.resolve(REPO_ROOT, sourcePath)),
  };
}

function frontierLoadGauntletRelation() {
  const matrix = frontierReadJson("gauntlet/matrix.json");
  const manifest = frontierReadJson("gauntlet/measurement-manifest.json");
  const policy = matrix.value?.strict_performance_policy;
  const areas = Array.isArray(policy?.critical_areas) ? policy.critical_areas : [];
  const cells = Array.isArray(matrix.value?.cells) ? matrix.value.cells : [];
  const entryNames = Array.isArray(manifest.value?.corpus?.entry_names)
    ? manifest.value.corpus.entry_names
    : [];
  const entries = entryNames.map((name) => {
    const entry = frontierReadJson(`gauntlet/entries/${name}/entry.json`);
    if (entry.status !== "available") return {
      name,
      status: "unavailable",
      path: entry.path,
      digest: null,
      reason: entry.reason,
    };
    return {
      name,
      status: "available",
      path: entry.path,
      digest: entry.digest,
      identity: {
        name: entry.value?.name ?? name,
        cells: Array.isArray(entry.value?.cells) ? [...entry.value.cells] : [],
        tier: entry.value?.tier ?? null,
        mode: entry.value?.mode ?? null,
        languages: Array.isArray(entry.value?.languages) ? [...entry.value.languages] : [],
      },
    };
  });
  const entryByCell = (cellId) => entries
    .filter((entry) => entry.status === "available" && entry.identity.cells.includes(cellId))
    .map((entry) => ({
      workload_id: entry.identity.name,
      entry_path: entry.path,
      entry_digest: entry.digest,
      tier: entry.identity.tier,
      mode: entry.identity.mode,
    }));
  const missionJoins = areas.map((area) => {
    const areaCells = Array.isArray(area.cells) ? area.cells : [];
    const joins = areaCells.map((cellId) => {
      const cell = cells.find((candidate) => candidate?.id === cellId) ?? null;
      const workloads = entryByCell(cellId);
      return {
        cell_id: cellId,
        cell: cell
          ? {
            domain: cell.domain ?? null,
            kind: cell.kind ?? null,
            description: cell.description ?? null,
          }
          : null,
        status: workloads.length > 0 ? "joined" : "unavailable",
        workloads,
        reason: workloads.length > 0
          ? null
          : "canonical Gauntlet has no entry identity for this matrix cell; no substitute workload is permitted",
      };
    });
    const status = joins.length === 0
      ? "unavailable"
      : joins.every((join) => join.status === "joined") ? "joined" : "partial";
    return {
      area_id: area.id,
      required: area.required === true,
      activation: area.activation ?? null,
      status,
      joins,
      reason: joins.length === 0
        ? "canonical policy has no workload cells; first-party battery activation is not present"
        : null,
    };
  });
  const status = matrix.status !== "available" || manifest.status !== "available"
    ? "unavailable"
    : missionJoins.length > 0 ? "available" : "unavailable";
  return {
    status,
    reason: status === "available"
      ? null
      : matrix.reason ?? manifest.reason ?? "canonical Gauntlet relation is unavailable",
    matrix: {
      path: matrix.path,
      digest: matrix.digest,
      schema: matrix.value?.version ? "gauntlet-matrix-v1" : null,
    },
    measurement_manifest: {
      path: manifest.path,
      digest: manifest.digest,
      schema: manifest.value?.report_contract?.id ?? null,
    },
    entries,
    mission_joins: missionJoins,
    relation: "gauntlet/matrix.json.strict_performance_policy.critical_areas joined to gauntlet/entries/*/entry.json.cells",
  };
}

function frontierLoadLearningRelation() {
  const curriculum = frontierReadJson("examples/learn/curriculum.json");
  const value = curriculum.value;
  const valid = curriculum.status === "available"
    && value?.schema === "jet-learning-curriculum-v1"
    && value?.relation === "jet-learning-census-v1"
    && value?.relation_source === "scripts/agent/learning-census.mjs"
    && Array.isArray(value.tasks);
  if (!valid) {
    return {
      status: "unavailable",
      reason: curriculum.reason ?? "canonical learning relation is malformed or unavailable",
      path: curriculum.path,
      digest: curriculum.digest,
      task_joins: [],
      relation: "examples/learn/curriculum.json.tasks projected from scripts/agent/learning-census.mjs",
    };
  }
  const taskJoins = value.tasks.map((task) => ({
    task_id: task.id ?? null,
    capability_id: task.capability_id ?? null,
    family: task.family ?? null,
    source_identity: frontierTaskSourceIdentity(task),
    input_identity: task.input_identity ?? null,
    oracle_id: task.oracle?.id ?? null,
    status: task.id && task.capability_id && task.oracle?.id ? "joined" : "unavailable",
    reason: task.id && task.capability_id && task.oracle?.id
      ? null
      : "canonical learning task lacks an identity-bearing capability or oracle",
  }));
  return {
    status: taskJoins.every((task) => task.status === "joined") ? "available" : "unavailable",
    reason: taskJoins.every((task) => task.status === "joined")
      ? null
      : "one or more canonical learning tasks cannot be joined without inventing an identity",
    path: curriculum.path,
    digest: curriculum.digest,
    schema: value.schema,
    relation: "examples/learn/curriculum.json.tasks projected from scripts/agent/learning-census.mjs",
    revision_rule: value.revision_rule,
    task_joins: taskJoins,
    tasks: value.tasks,
  };
}

function frontierLoadProofRelation() {
  const obligations = frontierReadJson("proof/compiler/obligations.json");
  const value = obligations.value;
  const valid = obligations.status === "available"
    && value?.schema === "jet.compiler-obligations.v1"
    && Array.isArray(value.rows);
  if (!valid) {
    return {
      status: "unavailable",
      reason: obligations.reason ?? "canonical proof obligation relation is malformed or unavailable",
      path: obligations.path,
      digest: obligations.digest,
      row_count: null,
      rows: [],
      relation: "proof/compiler/obligations.json.rows",
    };
  }
  const rows = value.rows.map((row) => ({
    id: row.id ?? null,
    family: row.family ?? null,
    rule: row.rule ?? null,
    owner: row.owner ?? null,
    state: row.obligation_state ?? null,
  }));
  return {
    status: "available",
    reason: null,
    path: obligations.path,
    digest: obligations.digest,
    schema: value.schema,
    row_count: rows.length,
    rows,
    relation: "proof/compiler/obligations.json.rows",
  };
}

function frontierLoadCanonicalRelations() {
  return {
    gauntlet: frontierLoadGauntletRelation(),
    learning: frontierLoadLearningRelation(),
    proof: frontierLoadProofRelation(),
  };
}

function frontierProofJoin(relation, families) {
  if (relation.status !== "available") {
    return {
      status: "unavailable",
      relation: relation.relation,
      path: relation.path,
      digest: relation.digest,
      families: [...families],
      obligation_ids: [],
      reason: relation.reason,
    };
  }
  const rows = relation.rows.filter((row) => families.includes(row.family));
  return {
    status: rows.length > 0 ? "joined" : "unavailable",
    relation: relation.relation,
    path: relation.path,
    digest: relation.digest,
    families: [...families],
    obligation_ids: rows.map((row) => row.id).filter(Boolean),
    owners: [...new Set(rows.map((row) => row.owner).filter(Boolean))].sort(),
    reason: rows.length > 0
      ? null
      : "canonical proof relation has no obligation in the requested family; no formal claim may be promoted",
  };
}

function frontierContractIdentity(study) {
  if (!study.contract) {
    return {
      status: "canonical-learning-join",
      relation: "examples/learn/curriculum.json",
      path: "examples/learn/curriculum.json",
      digest: null,
    };
  }
  const contract = frontierReadJson(
    path.join("tools/agent-eval/foundations-trust/fixtures", study.fixture, study.contract),
  );
  return {
    status: contract.status,
    relation: "frontier study job contract",
    path: contract.path,
    digest: contract.digest,
    job_id: contract.value?.job_id ?? null,
    contract_id: contract.value?.contract_id ?? null,
    reason: contract.reason ?? null,
  };
}

function frontierFixturePaths(study) {
  const root = path.join(FRONTIER_STUDY_ROOT, study.fixture);
  return {
    root,
    baseline: path.join(root, study.baseline),
    intervention: path.join(root, study.intervention),
  };
}

function frontierTierPlan(study, options, executionStatus = "not-requested") {
  return [
    {
      tier: "model",
      evidence_class: "model",
      status: "available",
      contract_id: study.id,
      relation: "retained native-JavaScript model witnesses in this demonstrator",
      reason: null,
    },
    {
      tier: "runtime",
      evidence_class: "runtime",
      status: options.execute ? executionStatus : "not-requested",
      contract_id: study.id,
      relation: "paired fixture process output",
      reason: options.execute ? null : "runtime study execution was not requested",
    },
    {
      tier: "formal",
      evidence_class: "formal",
      status: "unavailable",
      contract_id: study.id,
      relation: "canonical proof obligation join only",
      reason: "no formal checker result is inferred from the study fixture; proof relation remains a separate evidence class",
    },
  ];
}
function frontierModelEvidence(study) {
  const witnesses = {
    H1: ["staleRevisionFactAttachment"],
    H2: ["entitySlotReuse", "fixedStepAccumulator"],
    H3: ["ieee754Reassociation"],
    H4: [],
  }[study.key] ?? [];
  const observations = Object.fromEntries(witnesses
    .filter((name) => output.cases?.[name])
    .map((name) => [name, output.cases[name]]));
  return {
    evidence_class: "model",
    status: Object.keys(observations).length > 0 ? "available" : "unavailable",
    source: DEMONSTRATOR_PATH,
    source_digest: frontierFileDigest(DEMONSTRATOR_FILE),
    witnesses,
    observations,
    cost: {
      status: "unmeasured",
      wall_ms: null,
      reason: "retained model output is not a runtime performance measurement",
    },
    environment: frontierRunEnvironment([process.execPath, DEMONSTRATOR_PATH]),
    repair_actionability: {
      status: "not-applicable",
      actionable: false,
      reasons: [],
      context: "model witnesses do not execute the paired study fixture",
    },
  };
}

function frontierRunEnvironment(command) {
  return {
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    cwd: frontierRelative(REPO_ROOT),
    locale: "C",
    timezone: "UTC",
    command: [...command],
  };
}

function frontierRepairActionability(parsed, rawStderr) {
  const reasons = [];
  const collect = (value) => {
    if (!value || typeof value !== "object") return;
    for (const key of ["failure_reason", "rejection_reason", "typed_failure", "reason"]) {
      const item = value[key];
      if (item !== null && item !== undefined) reasons.push(
        typeof item === "string" ? item : frontierCanonical(item),
      );
    }
    for (const nested of Object.values(value)) if (nested && typeof nested === "object") collect(nested);
  };
  collect(parsed);
  if (rawStderr) reasons.push(rawStderr.trim());
  return reasons.length > 0
    ? {
      status: "observed",
      actionable: true,
      reasons: reasons.slice(0, 16),
      context: "derived from retained failure or rejection output; no repair was applied by the driver",
    }
    : {
      status: "not-applicable",
      actionable: false,
      reasons: [],
      context: "the observed run exposed no failure or rejection requiring a repair",
    };
}

function frontierRunFixture({
  study,
  variant,
  fixture,
  input,
  fixtureIdentity,
  options,
  canonicalRefs,
}) {
  const relativeFixture = frontierRelative(fixture);
  const runner = options.runner || process.env.JET_FRONTIER_STUDY_RUNNER || FRONTIER_ENV_RUNNER;
  const directNode = runner === "node" || runner === process.execPath || runner === "direct-node";
  const command = directNode
    ? [process.execPath, relativeFixture]
    : [runner, "full", "node", relativeFixture];
  const inputText = frontierCanonicalJson(input);
  const started = process.hrtime.bigint();
  let child;
  try {
    child = spawnSync(command[0], command.slice(1), {
      cwd: REPO_ROOT,
      input: `${inputText}\n`,
      encoding: "utf8",
      env: {
        ...process.env,
        LC_ALL: "C",
        LANG: "C",
        TZ: "UTC",
        NO_COLOR: "1",
        CLICOLOR: "0",
      },
      timeout: options.timeoutMs,
      maxBuffer: FRONTIER_MAX_BUFFER,
    });
  } catch (error) {
    child = { error };
  }
  const elapsedMs = Number(process.hrtime.bigint() - started) / 1_000_000;
  const stdout = typeof child?.stdout === "string" ? child.stdout : String(child?.stdout ?? "");
  const stderr = typeof child?.stderr === "string" ? child.stderr : String(child?.stderr ?? "");
  const timedOut = child?.error?.code === "ETIMEDOUT";
  let parsed = null;
  let parseError = null;
  if (stdout.trim()) {
    try {
      parsed = JSON.parse(stdout.trim());
    } catch (error) {
      parseError = error.message;
    }
  }
  const observed = !child?.error && !child?.signal && child?.status === 0 && parsed !== null;
  const status = timedOut
    ? "timeout"
    : child?.error
      ? "unavailable"
      : child?.signal
        ? "failed"
        : child?.status !== 0
          ? "failed"
          : parseError
            ? "failed"
            : "observed";
  const raw = {
    variant,
    fixture: relativeFixture,
    fixture_identity: fixtureIdentity.digest,
    input: input,
    input_identity: frontierDigest(input),
    command,
    stdout,
    stderr,
    exit_code: Number.isInteger(child?.status) ? child.status : null,
    signal: child?.signal ?? null,
    timed_out: timedOut,
    error: child?.error ? String(child.error.message ?? child.error) : null,
  };
  const rawArtifactId = frontierDigest(raw);
  return {
    status,
    evidence_class: "runtime",
    variant,
    fixture: relativeFixture,
    input_identity: frontierDigest(input),
    observed_digest: parsed === null ? null : frontierDigest(parsed),
    raw_artifact_id: rawArtifactId,
    parsed,
    parse_error: parseError,
    raw,
    cost: {
      wall_ms: Number.isFinite(elapsedMs) ? elapsedMs : null,
      stdout_bytes: Buffer.byteLength(stdout),
      stderr_bytes: Buffer.byteLength(stderr),
      input_bytes: Buffer.byteLength(inputText),
      peak_rss_kb: null,
      peak_rss_status: "unavailable",
      peak_rss_reason: "the child runner does not expose a portable per-process peak RSS value",
    },
    environment: frontierRunEnvironment(command),
    repair_actionability: frontierRepairActionability(parsed, stderr),
    context: {
      fixture_files: fixtureIdentity.files,
      canonical_relations: canonicalRefs,
      source_bytes: fixtureIdentity.files.reduce(
        (total, file) => total + (Number.isFinite(file.bytes) ? file.bytes : 0),
        0,
      ),
      context_identity: frontierDigest({
        fixture: fixtureIdentity.digest,
        input: frontierDigest(input),
        canonical_relations: canonicalRefs,
      }),
    },
  };
}

function frontierDeterminism(runs) {
  if (!Array.isArray(runs) || runs.length < 2) {
    return {
      status: "unmeasured",
      deterministic: null,
      observations: runs?.length ?? 0,
      reason: "repeat count below two; determinism is not inferred from one run",
    };
  }
  const digests = runs.map((run) => run.observed_digest).filter(Boolean);
  const deterministic = digests.length === runs.length && digests.every((digest) => digest === digests[0]);
  return {
    status: deterministic ? "observed" : "failed",
    deterministic,
    observations: runs.length,
    digests,
    reason: deterministic ? null : "repeated retained observations differ or one run did not produce JSON",
  };
}

function frontierPair({ study, input, fixtureIdentity, fixturePaths, options, canonicalRefs }) {
  const repeat = Math.max(1, options.repeat);
  const baselineRuns = [];
  const interventionRuns = [];
  for (let index = 0; index < repeat; index += 1) {
    baselineRuns.push(frontierRunFixture({
      study,
      variant: "baseline",
      fixture: fixturePaths.baseline,
      input,
      fixtureIdentity,
      options,
      canonicalRefs,
    }));
    interventionRuns.push(frontierRunFixture({
      study,
      variant: "intervention",
      fixture: fixturePaths.intervention,
      input,
      fixtureIdentity,
      options,
      canonicalRefs,
    }));
  }
  return {
    input,
    input_identity: frontierDigest(input),
    same_job: {
      input_identity_equal: true,
      fixture_identity_equal: true,
      baseline_variant: "baseline",
      intervention_variant: "intervention",
    },
    baseline: {
      runs: baselineRuns,
      determinism: frontierDeterminism(baselineRuns),
    },
    intervention: {
      runs: interventionRuns,
      determinism: frontierDeterminism(interventionRuns),
    },
  };
}

function frontierFirstObserved(pair, variant) {
  const runs = pair?.[variant]?.runs;
  return Array.isArray(runs) && runs.length > 0 && runs[0].status === "observed"
    ? runs[0]
    : null;
}

function frontierRunAvailability(pairs) {
  if (!Array.isArray(pairs) || pairs.length === 0) return "not-requested";
  const runs = pairs.flatMap((pair) => [
    ...(pair.baseline?.runs ?? []),
    ...(pair.intervention?.runs ?? []),
  ]);
  if (runs.length === 0) return "unavailable";
  if (runs.every((run) => run.status === "observed")) return "observed";
  if (runs.some((run) => run.status === "timeout")) return "timeout";
  return "partial";
}
function frontierEqual(left, right) {
  return frontierCanonicalJson(left) === frontierCanonicalJson(right);
}

function frontierStudyCanonicalRefs(relations, study) {
  return {
    gauntlet_matrix: relations.gauntlet.matrix.digest,
    gauntlet_manifest: relations.gauntlet.measurement_manifest.digest,
    learning_curriculum: relations.learning.digest,
    proof_obligations: relations.proof.digest,
    proof_families: [...study.proof_families],
  };
}

function frontierStudyContract(study) {
  if (!study.contract) return { status: "canonical-learning-relation", value: null };
  const contractPath = path.join(
    "tools/agent-eval/foundations-trust",
    "fixtures",
    study.fixture,
    study.contract,
  );
  const valid = contract.status === "available"
    && contract.value?.schema === "jet.frontier-study-job.v1"
    && contract.value?.study_id === study.id
    && typeof contract.value?.job_id === "string"
    && typeof contract.value?.contract_id === "string";
  return {
    status: valid ? "available" : "unavailable",
    value: valid ? contract.value : null,
    path: contract.path,
    digest: contract.digest,
    reason: valid
      ? null
      : contract.reason ?? "study job contract is malformed or names a different study",
  };
}

function frontierExpectedEvidenceFailure(study, reason, details = {}) {
  return [{
    id: `${study.id}-observed-contract-failure`,
    study_id: study.id,
    owner: study.owners[0],
    owners: [...study.owners],
    reason,
    ...details,
  }];
}

function frontierEvaluateH1(study, pairs, contract) {
  if (!pairs.length) {
    return {
      disposition: "unresolved",
      reason: "no baseline/intervention runtime pair was retained",
      checks: {},
      defects: [],
    };
  }
  const pair = pairs[0];
  const baseline = frontierFirstObserved(pair, "baseline");
  const intervention = frontierFirstObserved(pair, "intervention");
  if (!baseline || !intervention) {
    return {
      disposition: "unresolved",
      reason: "baseline or intervention did not produce an observed JSON result",
      checks: {
        baseline_status: pair.baseline?.runs?.[0]?.status ?? "missing",
        intervention_status: pair.intervention?.runs?.[0]?.status ?? "missing",
      },
      defects: [],
    };
  }
  const expected = contract?.expected_observations ?? null;
  const sameContract = baseline.parsed?.contract_id === intervention.parsed?.contract_id
    && baseline.parsed?.contract_id === contract?.contract_id;
  const sameJobIdentity = baseline.parsed?.study_id === contract?.study_id
    && intervention.parsed?.study_id === contract?.study_id
    && baseline.parsed?.job_id === contract?.job_id
    && intervention.parsed?.job_id === contract?.job_id;
  const routeVariants = baseline.parsed?.variant === "baseline"
    && intervention.parsed?.variant === "intervention";
  const sameObservations = frontierEqual(
    baseline.parsed?.observations,
    intervention.parsed?.observations,
  );
  const oracleAgreement = expected !== null
    && frontierEqual(baseline.parsed?.observations, expected)
    && frontierEqual(intervention.parsed?.observations, expected);
  const supported = sameContract
    && sameJobIdentity
    && routeVariants
    && sameObservations
    && oracleAgreement;
  return {
    disposition: supported ? "supported-with-scope" : "reject",
    reason: supported
      ? "the retained job uses one contract identity and both routes preserve the declared observations"
      : "the paired routes diverge in contract identity or a declared observation",
    checks: {
      same_contract_identity: sameContract,
      same_job_identity: sameJobIdentity,
      baseline_intervention_variants: routeVariants,
      same_observations: sameObservations,
      declared_oracle_agreement: oracleAgreement,
      observation_contract: contract?.observation_contract ?? [],
    },
    defects: supported ? [] : frontierExpectedEvidenceFailure(
      study,
      "one semantic operation did not preserve its declared result, failure, mutation, input consumption, or event order",
    ),
    scope: "This is one retained JavaScript job and does not establish implementation-bound Jet semantics or universal consumer reuse.",
  };
}

function frontierEvaluateH2(study, pairs, contract) {
  if (!pairs.length) {
    return {
      disposition: "unresolved",
      reason: "no baseline/intervention runtime pair was retained",
      checks: {},
      defects: [],
    };
  }
  const pair = pairs[0];
  const baseline = frontierFirstObserved(pair, "baseline");
  const intervention = frontierFirstObserved(pair, "intervention");
  if (!baseline || !intervention) {
    return {
      disposition: "unresolved",
      reason: "baseline or intervention did not produce an observed temporal result",
      checks: {},
      defects: [],
    };
  }
  const sameJobIdentity = baseline.parsed?.study_id === contract?.study_id
    && intervention.parsed?.study_id === contract?.study_id
    && baseline.parsed?.job_id === contract?.job_id
    && intervention.parsed?.job_id === contract?.job_id;
  const routeVariants = baseline.parsed?.variant === "baseline"
    && intervention.parsed?.variant === "intervention";
  const baselineObserved = baseline.parsed?.temporal_observations;
  const interventionObserved = intervention.parsed?.temporal_observations;
  const expectedBaseline = contract?.expected?.baseline ?? {};
  const expectedIntervention = contract?.expected?.intervention ?? {};
  const checks = {
    same_job_identity: sameJobIdentity,
    baseline_intervention_variants: routeVariants,
    revision_distinguished: Boolean(
      baselineObserved?.revision_identity
      && interventionObserved?.revision_identity,
    ),
    generation_distinguished: Boolean(
      baselineObserved?.resource_generation
      && interventionObserved?.resource_generation,
    ),
    lifetime_distinguished: Boolean(
      baselineObserved?.resource_lifetime
      && interventionObserved?.resource_lifetime,
    ),
    clock_distinguished: baselineObserved?.clock_representation?.representation
      !== interventionObserved?.clock_representation?.representation,
    baseline_revision_control: baselineObserved?.revision_identity?.accepted
      === expectedBaseline.revision_stale_accepted,
    intervention_revision_guard: interventionObserved?.revision_identity?.accepted
      === expectedIntervention.revision_stale_accepted,
    baseline_generation_control: baselineObserved?.resource_generation?.accepted
      === expectedBaseline.generation_stale_accepted,
    intervention_generation_guard: interventionObserved?.resource_generation?.accepted
      === expectedIntervention.generation_stale_accepted,
    baseline_lifetime_control: baselineObserved?.resource_lifetime?.accepted
      === expectedBaseline.closed_callback_accepted,
    intervention_lifetime_guard: interventionObserved?.resource_lifetime?.accepted
      === expectedIntervention.closed_callback_accepted,
    baseline_clock_control: baselineObserved?.clock_representation?.schedules_match_baseline
      === expectedBaseline.clock_schedules_match,
    intervention_clock_guard: interventionObserved?.clock_representation?.schedules_match_baseline
      === expectedIntervention.clock_schedules_match,
  };
  const supported = Object.values(checks).every(Boolean);
  return {
    disposition: supported ? "supported-with-scope" : "reject",
    reason: supported
      ? "revision, generation, lifetime, and clock controls retain distinct relations and reject the modeled stale paths"
      : "a temporal guard accepted a modeled stale path or collapsed distinct temporal meanings",
    checks,
    defects: supported ? [] : frontierExpectedEvidenceFailure(
      study,
      "temporal invalidation or distinction did not match the declared control",
    ),
    scope: "This is a sequential fixture model with no concurrent reclamation, scheduler, persistence, or physical clock guarantee.",
  };
}

function frontierEvaluateH3(study, pairs) {
  if (!pairs.length) {
    return {
      disposition: "unresolved",
      reason: "no baseline/intervention runtime pair was retained",
      checks: {},
      defects: [],
    };
  }
  const pair = pairs[0];
  const baseline = frontierFirstObserved(pair, "baseline");
  const intervention = frontierFirstObserved(pair, "intervention");
  if (!baseline || !intervention) {
    return {
      disposition: "unresolved",
      reason: "baseline or intervention did not produce an observed optimization result",
      checks: {},
      defects: [],
    };
  }
  const sameJobIdentity = baseline.parsed?.study_id === study.id
    && intervention.parsed?.study_id === study.id
    && baseline.parsed?.job_id === intervention.parsed?.job_id;
  const routeVariants = baseline.parsed?.variant === "baseline"
    && intervention.parsed?.variant === "intervention";
  const checks = {
    same_job_identity: sameJobIdentity,
    baseline_intervention_variants: routeVariants,
    same_defined_observations: frontierEqual(
      baseline.parsed?.observations,
      intervention.parsed?.observations,
    ),
    checked_intervention_plan: intervention.parsed?.plan?.legality === "checked",
    invalid_reassociation_rejected:
      intervention.parsed?.controls?.invalid_reassociation?.accepted === false,
    invalid_reassociation_changes_observation:
      intervention.parsed?.controls?.invalid_reassociation?.witness?.changes_defined_observation === true,
    whole_job_cost_recorded: Number.isFinite(baseline.cost?.wall_ms)
      && Number.isFinite(intervention.cost?.wall_ms),
  };
  const legal = checks.same_job_identity
    && checks.baseline_intervention_variants
    && checks.same_defined_observations
    && checks.checked_intervention_plan
    && checks.invalid_reassociation_rejected
    && checks.invalid_reassociation_changes_observation;
  return {
    disposition: legal ? "supported-with-scope" : "reject",
    reason: legal
      ? "the checked candidate preserves the defined observation and rejects an invalid arithmetic control"
      : "the candidate changed a defined observation or failed to reject an invalid transformation",
    checks,
    costs: {
      baseline_wall_ms: baseline.cost?.wall_ms ?? null,
      intervention_wall_ms: intervention.cost?.wall_ms ?? null,
      comparison: "recorded for this same job; no speed winner is inferred from one run",
    },
    defects: legal ? [] : frontierExpectedEvidenceFailure(
      study,
      "conditional optimization changed a defined observation or accepted the invalid control",
    ),
    scope: "Legality is supported only for this job; profitability, warm/cold cost, and every applicable mode remain unresolved until the owning performance campaign runs.",
  };
}

function frontierEvaluateHumanTransfer(study, humanEvidence, learning) {
  if (humanEvidence?.status !== "available") {
    return {
      disposition: "unresolved",
      reason: humanEvidence?.reason ?? FRONTIER_HUMAN_ABSENCE_REASON,
      checks: [],
      defects: [],
    };
  }
  const canonicalTasks = Array.isArray(learning?.tasks) ? learning.tasks : [];
  const checks = canonicalTasks.map((task) => {
    const source = frontierTaskSourceIdentity(task);
    const records = humanEvidence.records.filter((record) => record.task_id === task.id);
    const current = records.filter((record) => record.source_revision === source.digest);
    const stale = records.filter((record) =>
      record.source_revision && record.source_revision !== source.digest);
    const unrevisioned = records.filter((record) => !record.source_revision);
    return {
      task_id: task.id,
      records: current.length,
      stale_records: stale.length,
      unrevisioned_records: unrevisioned.length,
      source_revision_current: current.length > 0
        && records.length === current.length,
      transfer_observed: current.some((record) => record.transfer === true),
      all_required_observations: current.length > 0 && current.every((record) =>
        record.completion === true
        && record.prediction_correct === true
        && record.transfer === true
        && record.recovery === true),
    };
  });
  const complete = checks.length > 0
    && checks.every((check) =>
      check.records > 0
      && check.stale_records === 0
      && check.unrevisioned_records === 0
      && check.source_revision_current
      && check.all_required_observations);
  const observedFailure = checks.some((check) =>
    check.records > 0
    && (check.stale_records > 0 || !check.all_required_observations));
  return {
    disposition: complete
      ? "supported-with-scope"
      : observedFailure ? "reject" : "unresolved",
    reason: complete
      ? "consented human records cover every canonical transfer task at the current source revision"
      : observedFailure
        ? "a consented human record is stale or fails a required transfer observation"
        : "consented human records do not cover every canonical transfer task",
    checks,
    defects: complete || !observedFailure ? [] : frontierExpectedEvidenceFailure(
      study,
      "consented human transfer record did not satisfy the current source-bound observation contract",
      { owner: "#2926" },
    ),
    scope: "Human evidence is limited to the supplied consented records and current source revisions; it does not generalize beyond their participants or tasks.",
  };
}

function frontierEvaluateH4(study, pairs, learning, humanEvidence = null) {
  if (!pairs.length) {
    return {
      disposition: "unresolved",
      reason: learning.status === "available"
        ? "no simulated learner pair was retained"
        : learning.reason,
      checks: {},
      defects: [],
      human_transfer: frontierEvaluateHumanTransfer(study, humanEvidence, learning),
    };
  }
  const canonicalTasks = Array.isArray(learning?.tasks) ? learning.tasks : [];
  const checks = [];
  for (const pair of pairs) {
    const task = canonicalTasks.find((candidate) => candidate.id === pair.task_id);
    const intervention = frontierFirstObserved(pair, "intervention");
    const baseline = frontierFirstObserved(pair, "baseline");
    const sourceIdentity = frontierTaskSourceIdentity(task);
    const refs = intervention?.parsed?.explanation?.evidence_refs ?? [];
    const sourceRef = refs.find((ref) => ref.kind === "source");
    const oracleRef = refs.find((ref) => ref.kind === "oracle");
    const sameJobIdentity = baseline?.parsed?.study_id === study.id
      && intervention?.parsed?.study_id === study.id
      && baseline?.parsed?.task_id === pair.task_id
      && intervention?.parsed?.task_id === pair.task_id;
    const routeVariants = baseline?.parsed?.variant === "baseline"
      && intervention?.parsed?.variant === "intervention";
    checks.push({
      same_job_identity: sameJobIdentity,
      baseline_intervention_variants: routeVariants,
      task_id: pair.task_id,
      canonical_task_joined: Boolean(task),
      baseline_observed: Boolean(baseline),
      intervention_observed: Boolean(intervention),
      intervention_reason_linked: Boolean(
        sourceRef
        && sourceRef.path === sourceIdentity.path
        && sourceRef.digest === sourceIdentity.digest
        && oracleRef?.id === task?.oracle?.id,
      ),
      intervention_transfer_observed: intervention?.parsed?.interaction?.transfer?.passed === true,
      baseline_has_no_reason_links: (baseline?.parsed?.explanation?.evidence_refs ?? []).length === 0,
    });
  }
  const supported = checks.length === canonicalTasks.length
    && checks.length > 0
    && checks.every((check) =>
      check.same_job_identity
      && check.baseline_intervention_variants
      && check.canonical_task_joined
      && check.baseline_observed
      && check.intervention_observed
      && check.intervention_reason_linked
      && check.intervention_transfer_observed
      && check.baseline_has_no_reason_links);
  return {
    disposition: supported ? "supported-with-scope" : "reject",
    reason: supported
      ? "the simulated learner route linked source/oracle identities and solved structurally different transfer jobs"
      : "the explanation route invented a reason, lost a canonical identity, or failed transfer",
    checks,
    defects: supported ? [] : frontierExpectedEvidenceFailure(
      study,
      "evidence-backed explanation or unfamiliar transfer failed its canonical identity check",
    ),
    human_transfer: frontierEvaluateHumanTransfer(study, humanEvidence, learning),
    scope: "This route measures a deterministic simulated learner only. It does not claim human comprehension, uptake, accessibility, or learning effectiveness.",
  };
}

function frontierEvaluateStudy(study, pairs, relations, contract, humanEvidence = null) {
  if (study.key === "H1") return frontierEvaluateH1(study, pairs, contract);
  if (study.key === "H2") return frontierEvaluateH2(study, pairs, contract);
  if (study.key === "H3") return frontierEvaluateH3(study, pairs, contract);
  return frontierEvaluateH4(study, pairs, relations.learning, humanEvidence);
}
function frontierHumanEvidence(pathValue, learning) {
  const requiredFields = [
    "participant",
    "task_id",
    "completion",
    "prediction_correct",
    "transfer",
    "recovery",
    "latency_ms",
  ];
  if (!pathValue) {
    return {
      status: "unavailable",
      evidence_class: "human",
      reason: FRONTIER_HUMAN_ABSENCE_REASON,
      required_fields: requiredFields,
      records: [],
    };
  }
  const relativePath = path.isAbsolute(pathValue) ? frontierRelative(pathValue) : pathValue;
  const loaded = frontierReadJson(relativePath);
  if (loaded.status !== "available") {
    return {
      status: "unavailable",
      evidence_class: "human",
      reason: loaded.reason,
      required_fields: requiredFields,
      records: [],
    };
  }
  const parsed = loaded.value;
  const records = Array.isArray(parsed) ? parsed : parsed?.records;
  const consent = parsed && !Array.isArray(parsed) ? parsed.consent : null;
  const consented = consent?.obtained === true || consent?.consented === true;
  if (!consented) {
    return {
      status: "unavailable",
      evidence_class: "human",
      reason: "human observation record does not carry explicit consent.obtained=true or consent.consented=true",
      required_fields: requiredFields,
      records: [],
      path: loaded.path,
      digest: loaded.digest,
    };
  }
  if (!Array.isArray(records)) {
    return {
      status: "unavailable",
      evidence_class: "human",
      reason: "human observation record must be an array or an object with records",
      required_fields: requiredFields,
      records: [],
      path: loaded.path,
      digest: loaded.digest,
    };
  }
  const canonicalTasks = new Set(learning.task_joins.map((task) => task.task_id));
  const errors = [];
  const clean = [];
  records.forEach((record, index) => {
    if (!record || typeof record !== "object" || Array.isArray(record)) {
      errors.push(`${index}.record`);
      return;
    }
    for (const field of requiredFields) if (!(field in record)) errors.push(`${index}.${field}`);
    if (record.participant !== "newcomer" && record.participant !== "experienced") {
      errors.push(`${index}.participant`);
    }
    if (!canonicalTasks.has(record.task_id)) errors.push(`${index}.task_id`);
    for (const field of ["completion", "prediction_correct", "transfer", "recovery"]) {
      if (typeof record[field] !== "boolean") errors.push(`${index}.${field}`);
    }
    if (typeof record.latency_ms !== "number" || record.latency_ms < 0) {
      errors.push(`${index}.latency_ms`);
    }
    if (record.source_revision !== undefined && typeof record.source_revision !== "string") {
      errors.push(`${index}.source_revision`);
    }
    if (errors.length === 0 || !errors.some((error) => error.startsWith(`${index}.`))) {
      clean.push({
        participant: record.participant,
        task_id: record.task_id,
        completion: record.completion,
        prediction_correct: record.prediction_correct,
        transfer: record.transfer,
        recovery: record.recovery,
        latency_ms: record.latency_ms,
        source_revision: record.source_revision ?? null,
      });
    }
  });
  if (errors.length > 0) {
    return {
      status: "unavailable",
      evidence_class: "human",
      reason: `human observation record is invalid: ${errors.slice(0, 16).join(", ")}`,
      required_fields: requiredFields,
      records: [],
      path: loaded.path,
      digest: loaded.digest,
      consent: { obtained: true },
    };
  }
  return {
    status: "available",
    evidence_class: "human",
    reason: null,
    required_fields: requiredFields,
    records: clean,
    path: loaded.path,
    digest: loaded.digest,
    consent: { obtained: true },
  };
}

function frontierEvidenceClassValidation(studies, humanEvidence) {
  const issues = [];
  const seenClasses = new Set();
  const inspectRun = (run, location) => {
    if (!run) return;
    if (!FRONTIER_EVIDENCE_CLASSES.includes(run.evidence_class)) {
      issues.push(`${location}.evidence_class is not a recognized evidence class`);
    } else {
      seenClasses.add(run.evidence_class);
    }
    if (run.evidence_class !== "runtime") {
      issues.push(`${location}.evidence_class must be runtime for an executed child process`);
    }
    if (run.parsed?.evidence_class) {
      const parsedClass = run.parsed.evidence_class;
      if (!FRONTIER_EVIDENCE_CLASSES.includes(parsedClass)) {
        issues.push(`${location}.parsed.evidence_class is not a recognized evidence class`);
      } else {
        seenClasses.add(parsedClass);
      }
      if (parsedClass === "human" || parsedClass === "model" || parsedClass === "formal") {
        issues.push(`${location}.parsed.evidence_class promotes non-runtime evidence`);
      }
    }
    if (run.evidence_class === "human" && run.consent?.obtained !== true) {
      issues.push(`${location}.human evidence lacks explicit consent`);
    }
  };
  for (const study of studies) {
    for (const pair of study.pairs ?? []) {
      for (const run of pair.baseline?.runs ?? []) {
        inspectRun(run, `${study.id}.baseline`);
      }
      for (const run of pair.intervention?.runs ?? []) {
        inspectRun(run, `${study.id}.intervention`);
      }
    }
  }
  for (const study of studies) {
    if (study.model_evidence?.status === "available") seenClasses.add("model");
  }
  if (humanEvidence?.status === "available") {
    seenClasses.add("human");
    if (humanEvidence.evidence_class !== "human" || humanEvidence.consent?.obtained !== true) {
      issues.push("human evidence is available without the human evidence class and explicit consent");
    }
  }
  return {
    schema: FRONTIER_EVIDENCE_SCHEMA,
    status: issues.length === 0 ? "passed" : "failed",
    classes: Object.fromEntries(FRONTIER_EVIDENCE_CLASSES.map((name) => [
      name,
      {
        observed: seenClasses.has(name),
        scope: name === "human"
          ? "consented participant observations only"
          : name === "simulated-learner"
            ? "scripted interaction output; not a participant result"
            : name === "formal"
              ? "checked proof artifacts only"
              : name === "model"
                ? "bounded executable model only"
                : "retained child-process stdout/stderr and exit status",
      },
    ])),
    issues,
  };
}
function frontierNormalizeOptions(options = {}) {
  const allStudies = new Set(FRONTIER_STUDIES.map((study) => study.id));
  const studySet = options.studySet instanceof Set
    ? new Set(options.studySet)
    : options.studySet
      ? new Set(options.studySet)
      : allStudies;
  return {
    execute: options.execute === true,
    replay: options.replay ?? null,
    artifact: options.artifact ?? null,
    humanRecord: options.humanRecord ?? null,
    study: options.study ?? "all",
    studySet,
    repeat: Number.isInteger(options.repeat) && options.repeat > 0 ? options.repeat : 1,
    timeoutMs: Number.isInteger(options.timeoutMs) && options.timeoutMs > 0
      ? options.timeoutMs
      : FRONTIER_DEFAULT_TIMEOUT_MS,
    runner: options.runner ?? null,
  };
}


function frontierStudyRecord(study, relations, options) {
  const fixtureIdentity = frontierFixtureIdentity(study);
  const fixturePaths = frontierFixturePaths(study);
  const contract = frontierStudyContract(study);
  const proof = frontierProofJoin(relations.proof, study.proof_families);
  const canonicalRefs = frontierStudyCanonicalRefs(relations, study);
  const workload = {
    relation: "gauntlet/matrix.json.strict_performance_policy.critical_areas",
    joins: relations.gauntlet.mission_joins,
    no_substitution: true,
  };
  const base = {
    id: study.id,
    hypothesis: study.key,
    title: study.title,
    claim: study.claim,
    falsifier: study.falsifier,
    prior_work: FRONTIER_PRIOR_WORK[study.key],
    owners: [...study.owners],
    same_job: {
      required: true,
      baseline_fixture: frontierRelative(fixturePaths.baseline),
      intervention_fixture: frontierRelative(fixturePaths.intervention),
      input_identity: contract.digest ?? relations.learning.digest,
    },
    fixtures: fixtureIdentity,
    contract: study.contract
      ? contract
      : {
        status: relations.learning.status,
        relation: relations.learning.relation,
        path: relations.learning.path,
        digest: relations.learning.digest,
        reason: relations.learning.reason ?? null,
      },
    workload_basis: workload,
    whole_job_comparison: {
      required: true,
      mission_areas: relations.gauntlet.mission_joins,
      omission_policy: "no omissions; unavailable joins remain explicit",
      substitution_policy: "no substitute workload or easier input",
      aggregation: "per-area and per-join only; no averaging away a loss",
      evidence_promotion: "forbidden across model, runtime, formal, simulated-learner, and human classes",
    },
    proof_relation: proof,
    canonical_refs: canonicalRefs,
    model_evidence: frontierModelEvidence(study),
    execution: {
      status: "not-requested",
      requested: false,
      repeat: options.repeat,
      pairs: [],
    },
    tiers: frontierTierPlan(study, options),
    disposition: {
      value: "unresolved",
      reason: "study execution was not requested; no runtime result is inferred from the model or plan",
    },
    defects: [],
    unknowns: [],
    human_evidence: {
      status: "not-applicable",
      evidence_class: "human",
      reason: "this hypothesis does not measure human transfer",
      records: [],
    },
  };
  return { base, fixtureIdentity, fixturePaths, contract, canonicalRefs };
}

function frontierBuildStudy(study, relations, options = {}) {
  options = frontierNormalizeOptions(options);
  const record = frontierStudyRecord(study, relations, options);
  const pairs = [];
  const selected = options.execute && options.studySet.has(study.id);
  if (selected) {
    if (study.key === "H4") {
      if (relations.learning.status === "available") {
        for (const task of relations.learning.tasks) {
          const input = {
            ...task,
            source_identity: frontierTaskSourceIdentity(task),
          };
          const pair = frontierPair({
            study,
            input,
            fixtureIdentity: record.fixtureIdentity,
            fixturePaths: record.fixturePaths,
            options,
            canonicalRefs: record.canonicalRefs,
          });
          pair.task_id = task.id;
          pairs.push(pair);
        }
      }
    } else if (record.contract.status === "available") {
      pairs.push(frontierPair({
        study,
        input: record.contract.value,
        fixtureIdentity: record.fixtureIdentity,
        fixturePaths: record.fixturePaths,
        options,
        canonicalRefs: record.canonicalRefs,
      }));
    }
  }
  const executionStatus = selected
    ? pairs.length > 0 ? frontierRunAvailability(pairs) : "unavailable"
    : "not-requested";
  const humanEvidence = study.key === "H4"
    ? frontierHumanEvidence(options.humanRecord, relations.learning)
    : {
      status: "not-applicable",
      evidence_class: "human",
      reason: "this hypothesis does not measure human transfer",
      records: [],
    };
  const evaluation = frontierEvaluateStudy(
    study,
    pairs,
    relations,
    record.contract.value,
    humanEvidence,
  );
  const evidenceValidation = frontierEvidenceClassValidation(
    [{ ...record.base, id: study.id, pairs }],
    humanEvidence,
  );
  const failedRuns = pairs.flatMap((pair) => [
    ...(pair.baseline?.runs ?? []),
    ...(pair.intervention?.runs ?? []),
  ]).filter((run) => run.status !== "observed");
  const executionFailures = failedRuns.map((run) => ({
    id: `${study.id}-${run.variant}-execution`,
    study_id: study.id,
    owner: study.owners[0],
    owners: [...study.owners],
    variant: run.variant,
    status: run.status,
    reason: run.raw?.error
      ?? run.parse_error
      ?? run.raw?.stderr
      ?? "runtime fixture did not produce an observed result",
    repair_actionability: run.repair_actionability,
  }));
  const executionUnknowns = selected && pairs.length === 0
    ? [{
      id: `${study.id}-execution-unavailable`,
      study_id: study.id,
      owner: study.owners[0],
      owners: [...study.owners],
      reason: study.key === "H4"
        ? relations.learning.reason ?? "canonical learning relation is unavailable"
        : record.contract.reason ?? "study contract is unavailable",
    }]
    : [];
  const result = {
    ...record.base,
    execution: {
      status: executionStatus,
      requested: selected,
      repeat: options.repeat,
      pairs,
      failure_count: executionFailures.length,
      unknown_count: executionUnknowns.length,
    },
    tiers: frontierTierPlan(study, options, executionStatus),
    disposition: {
      value: evaluation.disposition,
      reason: evaluation.reason,
      checks: evaluation.checks,
      scope: evaluation.scope ?? null,
      costs: evaluation.costs ?? null,
    },
    defects: [
      ...(evaluation.defects ?? []),
      ...(evaluation.human_transfer?.defects ?? []),
      ...executionFailures,
    ],
    unknowns: executionUnknowns,
    evidence_class_validation: evidenceValidation,
    human_evidence: humanEvidence,
  };
  if (evaluation.human_transfer) result.human_transfer = evaluation.human_transfer;
  return result;
}

function frontierArtifactForStudies(studies, relations) {
  const artifact = {
    schema: FRONTIER_ARTIFACT_SCHEMA,
    schema_version: 1,
    driver: {
      path: DEMONSTRATOR_PATH,
      digest: frontierFileDigest(DEMONSTRATOR_FILE),
    },
    canonical_relations: {
      gauntlet_matrix: relations.gauntlet.matrix.digest,
      gauntlet_manifest: relations.gauntlet.measurement_manifest.digest,
      learning_curriculum: relations.learning.digest,
      proof_obligations: relations.proof.digest,
    },
    studies: studies.map((study) => ({
      study_id: study.id,
      fixture_identity: study.fixtures.digest,
      contract_identity: study.contract.digest ?? study.canonical_refs.learning_curriculum,
      pairs: study.execution.pairs,
    })),
    content_digest: null,
  };
  artifact.content_digest = frontierDigest(artifact);
  return artifact;
}

function frontierBuildEnvelope(options = {}) {
  options = frontierNormalizeOptions(options);
  const relations = frontierLoadCanonicalRelations();
  const studies = FRONTIER_STUDIES.map((study) =>
    frontierBuildStudy(study, relations, options));
  const executedStudies = studies.filter((study) => study.execution.requested);
  const artifact = options.execute
    ? frontierArtifactForStudies(executedStudies, relations)
    : null;
  const executionStatuses = executedStudies.map((study) => study.execution.status);
  const executionStatus = !options.execute
    ? "not-requested"
    : executionStatuses.length === 0
      ? "unavailable"
      : executionStatuses.every((status) => status === "observed")
        ? "observed"
        : "partial";
  const evidenceIssues = studies.flatMap((study) =>
    study.evidence_class_validation?.issues ?? []);
  return {
    schema: FRONTIER_STUDY_SCHEMA,
    status: studies.every((study) => study.disposition.value === "supported-with-scope")
      ? "supported-with-scope"
      : studies.some((study) => study.disposition.value === "reject")
        ? "reject"
        : "unresolved",
    execution: {
      requested: options.execute,
      status: executionStatus,
      runner: options.runner || process.env.JET_FRONTIER_STUDY_RUNNER || FRONTIER_ENV_RUNNER,
      repeat: options.repeat,
      timeout_ms: options.timeoutMs,
      study_ids: [...options.studySet],
    },
    falsifiers: Object.fromEntries(FRONTIER_STUDIES.map((study) => [
      study.key,
      {
        source: "docs/audits/jet-foundations-and-trust-2026-09-04/02-frontier.md",
        text: study.falsifier,
      },
    ])),
    canonical_relations: relations,
    mission_coverage: {
      relation: relations.gauntlet.relation,
      joins: relations.gauntlet.mission_joins,
      no_second_denominator: true,
    },
    evidence_classes: {
      schema: FRONTIER_EVIDENCE_SCHEMA,
      validation_status: evidenceIssues.length === 0 ? "passed" : "failed",
      model: {
        scope: "existing bounded JavaScript witnesses only",
        promoted_to_runtime: false,
      },
      runtime: {
        scope: "fixture child-process observations only",
        promoted_to_formal: false,
      },
      formal: {
        scope: "checked proof artifacts only; this driver retains the canonical relation without claiming a checker result",
        promoted_from_runtime: false,
      },
      simulated_learner: {
        scope: "scripted learner interaction only",
        promoted_to_human: false,
      },
      human: {
        scope: "consented participant records only",
        available: studies.some((study) => study.human_evidence.status === "available"),
      },
      issues: evidenceIssues,
    },
    studies,
    artifact,
    limits: [
      "A model witness is not Jet runtime evidence.",
      "A runtime fixture result is not formal proof or a universal semantic claim.",
      "A simulated learner is not a human participant.",
      "A joined Gauntlet workload identity does not make an unavailable area measurable.",
      "No performance winner or novelty claim is inferred without the named same-job and prior-work evidence.",
    ],
  };
}
function frontierReplayRun(stored, issues, location) {
  if (!stored || typeof stored !== "object") {
    issues.push(`${location} is not an execution record`);
    return null;
  }
  const raw = stored.raw;
  if (!raw || typeof raw.stdout !== "string") {
    issues.push(`${location}.raw.stdout is missing`);
    return null;
  }
  const input = raw.input;
  const inputIdentity = input === undefined ? null : frontierDigest(input);
  if (raw.input_identity !== inputIdentity || stored.input_identity !== inputIdentity) {
    issues.push(`${location} input identity does not match retained input`);
  }
  if (stored.raw_artifact_id !== frontierDigest(raw)) {
    issues.push(`${location} raw artifact identity does not match retained raw bytes`);
  }
  let parsed = null;
  let parseError = null;
  if (raw.stdout.trim()) {
    try {
      parsed = JSON.parse(raw.stdout.trim());
    } catch (error) {
      parseError = error.message;
      issues.push(`${location} retained stdout is not JSON: ${parseError}`);
    }
  } else {
    issues.push(`${location} retained stdout is empty`);
  }
  const observedDigest = parsed === null ? null : frontierDigest(parsed);
  if (stored.observed_digest !== observedDigest) {
    issues.push(`${location} observed digest does not match retained stdout`);
  }
  const status = raw.timed_out
    ? "timeout"
    : raw.error
      ? "unavailable"
      : raw.signal
        ? "failed"
        : raw.exit_code !== 0
          ? "failed"
          : parseError
            ? "failed"
            : parsed === null
              ? "failed"
              : "observed";
  return {
    ...stored,
    status,
    parsed,
    parse_error: parseError,
    repair_actionability: frontierRepairActionability(parsed, raw.stderr),
  };
}

function frontierReplayArtifact(artifactPath) {
  const loaded = frontierReadJson(frontierRelative(path.resolve(REPO_ROOT, artifactPath)));
  const issues = [];
  if (loaded.status !== "available") {
    return {
      schema: FRONTIER_STUDY_SCHEMA,
      status: "failed",
      independent_reader: true,
      artifact_path: artifactPath,
      issues: [loaded.reason],
      studies: [],
    };
  }
  const artifact = loaded.value;
  if (artifact?.schema !== FRONTIER_ARTIFACT_SCHEMA) {
    issues.push(`artifact schema must be ${FRONTIER_ARTIFACT_SCHEMA}`);
  }
  const contentCopy = artifact && typeof artifact === "object" ? { ...artifact, content_digest: null } : null;
  if (artifact?.content_digest !== frontierDigest(contentCopy)) {
    issues.push("artifact content digest does not match retained artifact");
  }
  const currentDriver = {
    path: DEMONSTRATOR_PATH,
    digest: frontierFileDigest(DEMONSTRATOR_FILE),
  };
  if (!frontierEqual(artifact?.driver, currentDriver)) {
    issues.push("artifact driver identity does not match the current demonstrator");
  }
  const currentRelations = frontierLoadCanonicalRelations();
  const currentRelationDigests = {
    gauntlet_matrix: currentRelations.gauntlet.matrix.digest,
    gauntlet_manifest: currentRelations.gauntlet.measurement_manifest.digest,
    learning_curriculum: currentRelations.learning.digest,
    proof_obligations: currentRelations.proof.digest,
  };
  if (!frontierEqual(artifact?.canonical_relations, currentRelationDigests)) {
    issues.push("artifact canonical relation digests do not match current canonical inputs");
  }
  const replayStudies = [];
  for (const storedStudy of artifact?.studies ?? []) {
    const study = FRONTIER_STUDIES.find((candidate) => candidate.id === storedStudy.study_id);
    if (!study) {
      issues.push(`artifact contains unknown study ${storedStudy.study_id}`);
      continue;
    }
    const studyIssueCount = issues.length;
    const fixtureIdentity = frontierFixtureIdentity(study);
    if (storedStudy.fixture_identity !== fixtureIdentity.digest) {
      issues.push(`${study.id} fixture identity does not match current fixture files`);
    }
    const currentContractIdentity = study.contract
      ? frontierStudyContract(study).digest
      : currentRelations.learning.digest;
    if (storedStudy.contract_identity !== currentContractIdentity) {
      issues.push(`${study.id} contract identity does not match current canonical input`);
    }
    const pairs = [];
    for (const storedPair of storedStudy.pairs ?? []) {
      const pairIssues = [];
      const baselineRuns = (storedPair.baseline?.runs ?? []).map((run, index) =>
        frontierReplayRun(run, pairIssues, `${study.id}.baseline[${index}]`))
        .filter(Boolean);
      const interventionRuns = (storedPair.intervention?.runs ?? []).map((run, index) =>
        frontierReplayRun(run, pairIssues, `${study.id}.intervention[${index}]`))
        .filter(Boolean);
      const inputIdentity = frontierDigest(storedPair.input);
      if (storedPair.input_identity !== inputIdentity) {
        pairIssues.push(`${study.id} pair input identity does not match retained input`);
      }
      pairs.push({
        input: storedPair.input,
        input_identity: inputIdentity,
        task_id: storedPair.task_id,
        baseline: {
          runs: baselineRuns,
          determinism: frontierDeterminism(baselineRuns),
        },
        intervention: {
          runs: interventionRuns,
          determinism: frontierDeterminism(interventionRuns),
        },
      });
      issues.push(...pairIssues);
    }
    const contract = frontierStudyContract(study);
    const evaluation = frontierEvaluateStudy(
      study,
      pairs,
      currentRelations,
      contract.value,
    );
    const humanEvidence = study.key === "H4"
      ? {
        status: "unavailable",
        evidence_class: "human",
        reason: "replay does not synthesize consented human evidence",
        records: [],
      }
      : {
        status: "not-applicable",
        evidence_class: "human",
        reason: "this hypothesis does not measure human transfer",
        records: [],
      };
    const evidenceValidation = frontierEvidenceClassValidation(
      [{ id: study.id, pairs }],
      humanEvidence,
    );
    replayStudies.push({
      study_id: study.id,
      status: issues.length === studyIssueCount && evaluation.disposition !== "reject"
        ? "replayed"
        : "failed",
      disposition: evaluation.disposition,
      checks: evaluation.checks,
      reason: evaluation.reason,
      pairs,
      evidence_class_validation: evidenceValidation,
      issues: issues.slice(studyIssueCount),
    });
  }
  const failedStudy = replayStudies.some((study) =>
    study.status === "failed" || study.disposition === "reject");
  return {
    schema: FRONTIER_STUDY_SCHEMA,
    status: issues.length > 0 || failedStudy ? "failed" : "replayed",
    independent_reader: true,
    execution: {
      status: "replayed",
      child_processes_started: false,
      source: artifactPath,
    },
    artifact_path: artifactPath,
    artifact_identity: {
      digest: loaded.digest,
      content_digest: artifact?.content_digest ?? null,
    },
    studies: replayStudies,
    issues,
  };
}

function frontierUsage() {
  return [
    "Usage: node tools/agent-eval/foundations-trust/research-demonstrators.mjs [options]",
    "",
    "Without --execute, emit the study plan and canonical joins.",
    "  --execute, --run             run selected baseline/intervention fixtures",
    "  --study <all|H1|H2|H3|H4>    select one study or all (default: all)",
    "  --repeat <n>                  repeat each same-job route (default: 1)",
    "  --timeout-ms <n>              child timeout in milliseconds",
    "  --runner <path|node>          runner (default: scripts/agent/jet-env)",
    "  --artifact <path>             retain raw execution artifact",
    "  --human-record <path>         consented human transfer observations for H4",
    "  --replay <artifact>            independently replay retained raw output",
    "  --help                         show this help",
  ].join("\n");
}

function frontierParseArgs(argv) {
  const options = {
    execute: false,
    replay: null,
    artifact: null,
    humanRecord: null,
    study: "all",
    studySet: new Set(FRONTIER_STUDIES.map((study) => study.id)),
    repeat: 1,
    timeoutMs: FRONTIER_DEFAULT_TIMEOUT_MS,
    runner: null,
    help: false,
  };
  const studyIds = new Map();
  for (const study of FRONTIER_STUDIES) {
    studyIds.set(study.id.toLowerCase(), study.id);
    studyIds.set(study.key.toLowerCase(), study.id);
  }
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      options.help = true;
    } else if (arg === "--execute" || arg === "--run") {
      options.execute = true;
    } else if (arg === "--replay") {
      options.replay = argv[++index];
      if (!options.replay) throw new Error("--replay requires an artifact path");
    } else if (arg === "--artifact" || arg === "--out") {
      options.artifact = argv[++index];
      if (!options.artifact) throw new Error(`${arg} requires a path`);
    } else if (arg === "--human-record") {
      options.humanRecord = argv[++index];
      if (!options.humanRecord) throw new Error("--human-record requires a path");
    } else if (arg === "--study" || arg === "--studies") {
      options.study = argv[++index];
      if (!options.study) throw new Error(`${arg} requires a study`);
      if (options.study.toLowerCase() === "all") {
        options.studySet = new Set(FRONTIER_STUDIES.map((study) => study.id));
      } else {
        const selected = options.study.split(",").map((item) => {
          const id = studyIds.get(item.trim().toLowerCase());
          if (!id) throw new Error(`unknown frontier study: ${item}`);
          return id;
        });
        options.studySet = new Set(selected);
      }
    } else if (arg === "--repeat") {
      options.repeat = Number.parseInt(argv[++index], 10);
      if (!Number.isInteger(options.repeat) || options.repeat < 1) {
        throw new Error("--repeat must be a positive integer");
      }
    } else if (arg === "--timeout-ms") {
      options.timeoutMs = Number.parseInt(argv[++index], 10);
      if (!Number.isInteger(options.timeoutMs) || options.timeoutMs < 1) {
        throw new Error("--timeout-ms must be a positive integer");
      }
    } else if (arg === "--runner") {
      options.runner = argv[++index];
      if (!options.runner) throw new Error("--runner requires a path");
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }
  return options;
}

function frontierWriteArtifact(artifactPath, artifact) {
  const absolute = path.resolve(REPO_ROOT, artifactPath);
  mkdirSync(path.dirname(absolute), { recursive: true });
  writeFileSync(absolute, `${JSON.stringify(artifact, null, 2)}\n`);
  return frontierRelative(absolute);
}


function frontierMain(argv = process.argv.slice(2)) {
  let options;
  try {
    options = frontierParseArgs(argv);
    if (options.help) {
      process.stdout.write(`${frontierUsage()}\n`);
      return 0;
    }
    if (options.replay) {
      const replay = frontierReplayArtifact(options.replay);
      process.stdout.write(`${JSON.stringify(replay, null, 2)}\n`);
      return replay.status === "replayed" ? 0 : 1;
    }
    const frontier = frontierBuildEnvelope(options);
    if (options.artifact && frontier.artifact) {
      const artifactPath = frontierWriteArtifact(options.artifact, frontier.artifact);
      frontier.artifact_path = artifactPath;
    }
    process.stdout.write(JSON.stringify({ ...output, frontier_studies: frontier }, null, 2) + "\n");
    return 0;
  } catch (error) {
    process.stderr.write(`${error.stack ?? error}\n`);
    return 2;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  process.exitCode = frontierMain();
}

export {
  FRONTIER_FALSIFIERS,
  FRONTIER_STUDIES,
  frontierBuildEnvelope,
  frontierBuildStudy,
  frontierLoadCanonicalRelations,
  frontierMain,
  frontierParseArgs,
  frontierReplayArtifact,
};
