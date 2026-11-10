// A stand-in for guards-gate.mjs: fails on a FAIL row whose guard is not in the known list.
import fs from "node:fs";
const [out, knownFile] = process.argv.slice(2);
const known = new Set(fs.readFileSync(knownFile, "utf8").split("\n").filter((l) => l !== ""));
const failing = fs.readFileSync(out, "utf8").split("\n").filter((l) => l.startsWith("FAIL ")).map((l) => l.slice(5));
const fresh = failing.filter((name) => !known.has(name));
for (const name of failing) console.log(`guards-gate: ${name} fails${known.has(name) ? " (known)" : ""}`);
process.exit(fresh.length === 0 ? 0 : 1);
