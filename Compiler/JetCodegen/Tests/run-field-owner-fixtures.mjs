#!/usr/bin/env node
// Assemble field-owner contracts against the production L4 compiler sources.
// Usage: node Compiler/JetCodegen/Tests/run-field-owner-fixtures.mjs OUTDIR
// Then run OUTDIR/project/src/compiler.jet with the retained checker or jetc0.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-field-owner-fixtures.mjs OUTDIR");
const rungs = join(out, "rungs");
const assembled = spawnSync(process.execPath, [join(repo, "Tools/agent/stage1/ladder.mjs"), "rungs", repo, rungs], { encoding: "utf8" });
if (assembled.status !== 0) throw new Error(assembled.stderr || assembled.stdout);
const unit = readFileSync(join(rungs, "L4/project/src/compiler.jet"), "utf8");
const fixtures = readFileSync(join(repo, "Compiler/JetCodegen/Tests/FieldOwnerFixtures.jet"), "utf8");
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), unit + "\n" + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "field_owner_contract_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, Mem.Rc, Time.Wait] } }\n');
console.log(`${out}/project/src/compiler.jet`);
