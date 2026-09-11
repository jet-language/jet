import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { runTemporalJob } from "./frontier2954-temporal-identity-shared.mjs";

const fixtureDir = path.dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(readFileSync(path.join(fixtureDir, "frontier2954-temporal-identity.json"), "utf8"));
process.stdout.write(`${JSON.stringify(runTemporalJob(contract, true))}\n`);
