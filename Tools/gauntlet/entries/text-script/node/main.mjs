import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "notes.txt";
const lines = readFileSync(input, "utf8")
  .split("\n")
  .filter((line) => line.trim().length > 0)
  .map((line) => line.trim())
  .sort();
process.stdout.write(`lines ${lines.length}\n${lines.join("\n")}\n`);
