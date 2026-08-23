import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: browser_automation_preflight INPUT_FILE");
}

const profiles = new Set(["bidi-2025.5", "bidi-2024.11"]);
for (const [index, line] of readFileSync(process.argv[2], "utf8")
  .split(/\r?\n/)
  .entries()) {
  if (index === 0 || !line) continue;
  const [operation, value] = line.split("\t");
  const accepted = operation === "profile"
    ? profiles.has(value)
    : operation === "timeout" && value === "500";
  if (operation !== "profile" && operation !== "timeout") {
    if (operation !== "connect") {
      throw new Error(`unknown browser operation ${operation}`);
    }
  }
  console.log(`${operation}|${value}|${accepted ? "accepted" : "rejected"}`);
}
