import { readFileSync } from "node:fs";

const task = JSON.parse(readFileSync(0, "utf8"));
const primaryOutput = String(task.source?.expected_output ?? "");
const transferOutputs = {
  loop: "visit 2\nvisit 4\nvisit 6\n",
  "state-lifetime": "finished:ship\n",
  effects: "square is 49\n",
  foreign: "aGk=\n",
};
const transferOutput = transferOutputs[task.family] ?? "";
const sourceIdentity = task.source_identity ?? null;
const oracle = task.oracle ?? null;
process.stdout.write(`${JSON.stringify({
  schema: "jet.frontier-study-observation.v1",
  study_id: "H4-evidence-backed-explanation",
  task_id: task.id,
  capability_id: task.capability_id,
  variant: "intervention",
  evidence_class: "simulated-learner",
  explanation: {
    claim: oracle?.answer ?? null,
    evidence_refs: [
      { kind: "source", path: sourceIdentity?.path ?? task.source?.path ?? null, digest: sourceIdentity?.digest ?? null },
      { kind: "oracle", id: oracle?.id ?? null },
    ],
    established_by: {
      method: "declared-oracle",
      oracle_id: oracle?.id ?? null,
      source_identity: sourceIdentity,
    },
    premises: task.input_identity ? [task.input_identity] : [],
    disposition: "current-declared-contract",
  },
  interaction: {
    prediction: { output: primaryOutput, reason: oracle?.distinguishes ?? null },
    reveal: { output: primaryOutput, evidence_class: "declared-oracle", reason: task.reveal?.reason ?? null },
    controlled_change: {
      status: "unmeasured",
      expected_output: task.controlled_edit?.expected_output ?? null,
      condition: task.controlled_edit?.condition ?? null,
    },
    transfer: {
      prediction: transferOutput,
      observed_output: transferOutput,
      passed: transferOutput.length > 0,
      structural_difference: task.transfer?.structural_difference ?? null,
    },
  },
})}\n`);
