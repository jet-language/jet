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
process.stdout.write(`${JSON.stringify({
  schema: "jet.frontier-study-observation.v1",
  study_id: "H4-evidence-backed-explanation",
  task_id: task.id,
  capability_id: task.capability_id,
  variant: "baseline",
  evidence_class: "simulated-learner",
  explanation: {
    claim: "The example works because the source says so.",
    evidence_refs: [],
    premises: [],
  },
  interaction: {
    prediction: { output: primaryOutput, reason: "copied example" },
    reveal: { output: primaryOutput, evidence_class: "declared-oracle" },
    controlled_change: { status: "unmeasured", reason: "baseline does not derive the changed condition" },
    transfer: {
      prediction: primaryOutput,
      observed_output: transferOutput,
      passed: primaryOutput === transferOutput,
    },
  },
})}\n`);
