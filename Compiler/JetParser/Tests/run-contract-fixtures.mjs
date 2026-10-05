#!/usr/bin/env node
// Assemble parser contract fixtures with the same L2 sources as selfcheck.
// Usage: node Compiler/JetParser/Tests/run-contract-fixtures.mjs OUTDIR
// Then compile OUTDIR/project/src/compiler.jet with a retained Jet compiler.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-contract-fixtures.mjs OUTDIR");
const rungs = join(out, "rungs");
const assembled = spawnSync(process.execPath, [join(repo, "Tools/agent/stage1/ladder.mjs"), "rungs", repo, rungs], { encoding: "utf8" });
if (assembled.status !== 0) throw new Error(assembled.stderr || assembled.stdout);
const source = readFileSync(join(repo, "crates/jet-codegen/src/Prelude/Markers.jet"), "utf8");
const literal = (value) => JSON.stringify(value).replaceAll("{", "{{").replaceAll("}", "}}");
const rows = source.split("\n").filter((line) => line.startsWith("marker ")).map((line) => {
  const name = line.match(/^marker (\w+)\(/)?.[1];
  if (!name) throw new Error(`unrecognized marker declaration: ${line}`);
  const retired = line.match(/\$retired:\s*("(?:[^"\\]|\\.)*")/);
  if (line.includes("$retired") && !retired) throw new Error(`unrecognized retirement: ${line}`);
  const replacement = retired ? JSON.parse(retired[1]).replaceAll("{{", "{").replaceAll("}}", "}") : "";
  return `${literal(name)}: ${literal(replacement)}`;
});
const unit = readFileSync(join(rungs, "L2/project/src/compiler.jet"), "utf8");
const fixtures = readFileSync(join(repo, "Compiler/JetParser/Tests/ContractFixtures.jet"), "utf8");
const constants = `\nPC_MARKER_SOURCE :: prep { ${literal(source)} }\nPC_MARKER_STATUS :: prep { [String:String]{${rows.join(",\n")}} }\n`;
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), unit + constants + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "parser_contract_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n');
console.log(`${out}/project/src/compiler.jet (${rows.length} marker statuses)`);
