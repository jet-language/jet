import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: browser_automation_preflight INPUT_FILE");
}

const availableCapabilities = new Set(["profile", "timeout"]);
const profiles = new Set(["bidi-2025.5", "bidi-2024.11"]);

function hasCapability(name) {
  return availableCapabilities.has(name);
}


function parseTimeout(value) {
  const raw = value.trim();
  if (!/^[+-]?[0-9]+$/.test(raw)) return null;
  const milliseconds = Number(raw);
  return Number.isSafeInteger(milliseconds) ? milliseconds : null;
}

for (const [index, line] of readFileSync(process.argv[2], "utf8")
  .split(/\r?\n/)
  .entries()) {
  if (index === 0 || !line) continue;
  const fields = line.split("\t");
  if (fields.length !== 2) {
    throw new Error(`malformed browser row ${index + 1}`);
  }
  const [operation, value] = fields;
  let accepted;
  if (operation === "profile") {
    accepted = hasCapability("profile") && profiles.has(value);
  } else if (operation === "timeout") {
    const milliseconds = parseTimeout(value);
    accepted =
      hasCapability("timeout") &&
      milliseconds !== null &&
      milliseconds >= 1 &&
      milliseconds <= 600000;
  } else if (operation === "connect") {
    accepted = false;
  } else {
    throw new Error(`unknown browser operation ${operation}`);
  }
  console.log(`${operation}|${value}|${accepted ? "accepted" : "rejected"}`);
}
