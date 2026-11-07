#!/usr/bin/env node
// Parser contract fixtures with the production Foundation, Lexer and Parser packages.
// Usage: node Compiler/JetParser/Tests/run-contract-fixtures.mjs OUTDIR [FIXTURE.jet]
// Then compile OUTDIR/project/src/compiler.jet with a Jet compiler.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { seedDir } from "../../../Tools/agent/roots.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
// The fixture unit of ENTRY... (Tools/agent/CompilerPackages.jet unit, run by the seed).
const compilerFixtureUnit = (...entries) => execFileSync(join(seedDir(), "jet"), [join(repo, "Tools/agent/CompilerPackages.jet"), "--root", repo, "unit", ...entries], { maxBuffer: 1 << 30 }).toString("utf8");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-contract-fixtures.mjs OUTDIR [FIXTURE.jet]");
// The marker structs are the raw text of jet_foundation's MARKER_SOURCE.
const markerFile = readFileSync(join(repo, "Compiler/JetFoundation/Source/Policy/PreludeMarkers.jet"), "utf8");
const source = markerFile.match(/^MARKER_SOURCE: comptime \{ ```([\s\S]*?)``` \}$/m)?.[1];
if (source === undefined) throw new Error("PreludeMarkers.jet has no MARKER_SOURCE raw text");
const literal = (value) => JSON.stringify(value).replaceAll("{", "{{").replaceAll("}", "}}");
const rows = source.split("\n").filter((line) => /^\w+: struct\b/.test(line)).map((line) => {
  const name = line.match(/^(\w+): struct \{$/)?.[1];
  if (!name) throw new Error(`unrecognized marker struct: ${line}`);
  return literal(name);
});
const unit = compilerFixtureUnit("Compiler/JetParser");
const fixturePath = process.argv[3] ? resolve(repo, process.argv[3]) : join(repo, "Compiler/JetParser/Tests/ContractFixtures.fixture/ContractFixtures.jet");
const fixtures = readFileSync(fixturePath, "utf8");
const runtimeSource = readFileSync(join(repo, "tests/fixtures/fmt_import_c_bindings.jet"), "utf8");
const constants = `\nPC_MARKER_SOURCE: comptime { ${literal(source)} }\nPC_MARKER_NAMES: comptime { [String]{${rows.join(",\n")}} }\nPC_FMT_RUNTIME_SOURCE: comptime { ${literal(runtimeSource)} }\n`;
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), unit + constants + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "parser_contract_fixtures"\nversion: "0.1.0"\nauthority: { allow: [IO, Mem.Alloc] }\n');
console.log(`${out}/project/src/compiler.jet (${rows.length} marker structs)`);
