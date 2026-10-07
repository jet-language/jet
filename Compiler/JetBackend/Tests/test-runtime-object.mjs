#!/usr/bin/env node
// Assemble the current ABI fixtures with the pre-refactor program-object
// writer as an independent byte oracle. Run `jet run unit.jet` afterward.
// usage: node test-runtime-object.mjs <outdir> <pre-refactor-commit>
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, baseline] = process.argv.slice(2);
if (!output || !baseline) throw new Error("usage: test-runtime-object.mjs <outdir> <pre-refactor-commit>");
const out = resolve(output);
execFileSync(process.execPath, [join(repo, "Compiler/JetBackend/Tests/run-lower-fixtures.mjs"), out, "--abi"], { stdio: "inherit" });
const old = execFileSync("git", ["show", `${baseline}:Compiler/JetBackend/Source/Image/Object.jet`], { cwd: repo, encoding: "utf8" });
const marker = "pub fn x64_relocatable_object(";
const start = old.indexOf(marker);
if (start < 0 || old.includes("fn x64_object_functions(")) throw new Error("baseline must contain the pre-refactor program-object writer");
const oracle = old.slice(start).replace(marker, "fn runtime_reference_object(");
const unit = readFileSync(join(out, "unit.jet"), "utf8");
const entry = "fn run() { abi_fixtures() }";
if (!unit.includes(entry)) throw new Error("ABI fixture entry changed; update the object regression adapter");
const witness = `fn run() {
    abi_fixtures()
    // Both imported-runtime/data relocations and a plain return function.
    loop plain in [Bool]{false, true} {
        entry :: LIRFunctionID{value: 0}
        program := abi_alignment_module(true)
        if plain {
            program.data = [LIRData]{}
            program.functions[0].values = [LIRValue]{}
            program.functions[0].blocks[0].insts = [LIRInst]{}
        }
        current :: x64_relocatable_object(program, entry)
        before :: runtime_reference_object(program, entry)
        assert(current.errors.len() == 0)
        assert(before.errors.len() == 0)
        assert(current.bytes == before.bytes)
        files.write_bytes("program-current-{plain}.o", current.bytes) ?? panic("cannot write current program object")
        files.write_bytes("program-before-{plain}.o", before.bytes) ?? panic("cannot write reference program object")
    }
}
`;
writeFileSync(join(out, "unit.jet"), `${unit.replace(entry, witness)}\n${oracle}\n`);
console.log(join(out, "unit.jet"));
