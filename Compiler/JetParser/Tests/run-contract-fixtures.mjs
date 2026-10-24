#!/usr/bin/env node
// Parser contract fixtures with the production Foundation, Lexer and Parser packages.
// Usage: node Compiler/JetParser/Tests/run-contract-fixtures.mjs OUTDIR [FIXTURE.jet]
// Then compile OUTDIR/project/src/compiler.jet with a Jet compiler.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compilerFixtureUnit } from "../../../Tools/agent/compiler-packages.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-contract-fixtures.mjs OUTDIR [FIXTURE.jet]");
// The marker declarations are the raw text of jet_foundation's MARKER_SOURCE.
const markerFile = readFileSync(join(repo, "Compiler/JetFoundation/Source/Policy/PreludeMarkers.jet"), "utf8");
const source = markerFile.match(/^MARKER_SOURCE :: comptime \{ ```([\s\S]*?)``` \}$/m)?.[1];
if (source === undefined) throw new Error("PreludeMarkers.jet has no MARKER_SOURCE raw text");
const literal = (value) => JSON.stringify(value).replaceAll("{", "{{").replaceAll("}", "}}");
const rows = source.split("\n").filter((line) => line.startsWith("marker ")).map((line) => {
  const name = line.match(/^marker (\w+)\(/)?.[1];
  if (!name) throw new Error(`unrecognized marker declaration: ${line}`);
  const retired = line.match(/\$retired:\s*("(?:[^"\\]|\\.)*")/);
  if (line.includes("$retired") && !retired) throw new Error(`unrecognized retirement: ${line}`);
  const replacement = retired ? JSON.parse(retired[1]).replaceAll("{{", "{").replaceAll("}}", "}") : "";
  return `${literal(name)}: ${literal(replacement)}`;
});
const unit = compilerFixtureUnit(repo, ["Compiler/JetParser"]);
const fixturePath = process.argv[3] ? resolve(repo, process.argv[3]) : join(repo, "Compiler/JetParser/Tests/ContractFixtures.fixture/ContractFixtures.jet");
const fixtures = readFileSync(fixturePath, "utf8");
const runtimeSource = readFileSync(join(repo, "tests/fixtures/fmt_import_c_bindings.jet"), "utf8");
const constants = `\nPC_MARKER_SOURCE :: comptime { ${literal(source)} }\nPC_MARKER_STATUS :: comptime { [String:String]{${rows.join(",\n")}} }\nPC_FMT_RUNTIME_SOURCE :: comptime { ${literal(runtimeSource)} }\n`;
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), unit + constants + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "parser_contract_fixtures"\nversion: "0.1.0"\nauthority: { allow: [IO, Mem.Alloc] }\n');
console.log(`${out}/project/src/compiler.jet (${rows.length} marker statuses)`);
