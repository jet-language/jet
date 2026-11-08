import { readFileSync } from "node:fs";

const seen = new Set();
for (const line of readFileSync("w13.txt", "utf8").split("\n")) {
  if (line !== "") seen.add(line);
}
console.log(`unique=${seen.size}`);
