import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "app.log";
const counts = new Map([["DEBUG", 0], ["INFO", 0], ["WARN", 0], ["ERROR", 0]]);
const errors = new Map();
let firstText = "";
let lastText = "";
let firstMs = 0;
let lastMs = 0;
for (const rawLine of readFileSync(input, "utf8").split("\n")) {
  const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
  if (!line) continue;
  const fields = line.split(" ");
  if (fields.length < 4) continue;
  const timestampText = fields[0];
  const timestamp = Date.parse(timestampText);
  if (!Number.isFinite(timestamp)) continue;
  const level = fields[1];
  const component = fields[2];
  if (!firstText) {
    firstText = timestampText;
    firstMs = timestamp;
  }
  lastText = timestampText;
  lastMs = timestamp;
  counts.set(level, (counts.get(level) ?? 0) + 1);
  if (level === "ERROR") errors.set(component, (errors.get(component) ?? 0) + 1);
}
const topErrors = [...errors.entries()]
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .slice(0, 3);
for (const level of ["DEBUG", "INFO", "WARN", "ERROR"]) console.log(`${level} ${counts.get(level) ?? 0}`);
console.log("top-error-components:");
for (const [component, count] of topErrors) console.log(`${count} ${component}`);
console.log(`span ${firstText} .. ${lastText} (${Math.floor((lastMs - firstMs) / 1000)}s)`);
