// The input has no quoted fields, so a line split is a full CSV parse of it.
import { readFileSync } from "node:fs";

const lines = readFileSync("w05.csv", "utf8").split("\n");
let count = 0;
let qty = 0;
let value = 0;
for (let i = 1; i < lines.length; i++) {
  if (lines[i] === "") continue;
  const fields = lines[i].split(",");
  const q = Number.parseInt(fields[3], 10);
  count += 1;
  qty += q;
  value += q * Number.parseInt(fields[4], 10);
}
console.log(`rows=${count} qty=${qty} value=${value}`);
