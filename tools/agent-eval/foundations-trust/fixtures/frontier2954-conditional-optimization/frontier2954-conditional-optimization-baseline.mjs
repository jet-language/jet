import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { runScalar } from "./frontier2954-conditional-optimization-shared.mjs";

const fixtureDir = path.dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(readFileSync(path.join(fixtureDir, "frontier2954-conditional-optimization.json"), "utf8"));
process.stdout.write(`${JSON.stringify(runScalar(contract))}\n`);
