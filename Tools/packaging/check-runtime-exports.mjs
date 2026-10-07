#!/usr/bin/env node
// A Jet runtime layer and its Rust-export deletion are one pack cutover.
// Archive ordering must never silently choose one of two strong definitions.
import { spawnSync } from "node:child_process";
const [jetObject, rustArchive] = process.argv.slice(2);
if (!jetObject || !rustArchive) throw new Error("usage: check-runtime-exports.mjs <jet_runtime_jet.o> <libjet_runtime.rlib>");
function definitions(path) {
  const result = spawnSync("nm", ["-g", "--defined-only", "--format=posix", path], { encoding: "utf8", maxBuffer: 1 << 28 });
  if (result.error || result.status !== 0) throw new Error(`nm ${path} failed: ${result.error ?? result.stderr}`);
  const names = new Set();
  for (const line of result.stdout.split("\n")) {
    const row = /^(?:.*:\s+)?(\S+)\s+([A-Z])\s/.exec(line);
    if (row && !["W", "V", "U"].includes(row[2])) names.add(row[1]);
  }
  return names;
}
const jet = definitions(jetObject), rust = definitions(rustArchive);
const duplicates = [...jet].filter((name) => rust.has(name)).sort();
if (duplicates.length) {
  console.error(`prebuild-runtime: replaced runtime exports still defined in Rust:\n${duplicates.map((name) => `  ${name}`).join("\n")}\nDelete these Rust definitions in the same cutover as their Jet runtime layer; duplicate definitions are not permitted.`);
  process.exit(1);
}
