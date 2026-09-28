import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { CONTRACT_ID, executeContract } from "./frontier2954-semantic-contract-shared.mjs";

const FIXTURE_DIR = path.dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(readFileSync(path.join(FIXTURE_DIR, "frontier2954-semantic-contract.json"), "utf8"));
const observations = executeContract(contract);
process.stdout.write(`${JSON.stringify({
  schema: "jet.frontier-study-observation.v1",
  study_id: contract.study_id,
  job_id: contract.job_id,
  contract_id: CONTRACT_ID,
  variant: "intervention",
  route: "shared-contract",
  observations,
})}\n`);
