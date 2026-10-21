#!/usr/bin/env node
// Field-owner contracts against the production compiler sources through codegen
// (Foundation, Lexer, Parser, Sema, Optimizer and Codegen packages).
// Usage: node Compiler/JetCodegen/Tests/run-field-owner-fixtures.mjs OUTDIR
// Then run OUTDIR/project/src/compiler.jet with a Jet compiler.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compilerFixtureUnit } from "../../../Tools/agent/compiler-packages.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-field-owner-fixtures.mjs OUTDIR");
const unit = compilerFixtureUnit(repo, ["Compiler/JetCodegen", "Compiler/JetOptimizer"]);
const fixtures = readFileSync(join(repo, "Compiler/JetCodegen/Tests/FieldOwnerFixtures.fixture/FieldOwnerFixtures.jet"), "utf8");
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), unit + "\n" + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "field_owner_contract_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, Mem.Rc, Time.Wait] } }\n');
console.log(`${out}/project/src/compiler.jet`);
