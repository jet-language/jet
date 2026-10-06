#!/usr/bin/env node
// Assemble production gate inspection/projection with each executable fixture.
// Usage: node Compiler/JetSema/Tests/run-gate-fixtures.mjs OUTDIR
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-gate-fixtures.mjs OUTDIR");
const manifest = readFileSync(join(repo, "Compiler/Bootstrap/sources.list"), "utf8");
const sources = manifest.split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith("#") && line !== "Compiler/Bootstrap/Host/Entry.jet");
// The canonical assemble.mjs package-import rule; tests replace only the entry.
const packageImport = /^use (?:jet_foundation|jet_lexer|jet_parser|jet_optimizer|jet_sema|jet_codegen|jet_eval|jet_driver|jet_backend|jet_cli|compiler_bootstrap)\.(?:\[[^\]]*\]|[A-Za-z_][A-Za-z0-9_]*[ \t]*$)/gm;
const coreImport = /^use core\.[^\[\n]*$/gm;
const seenCore = new Set();
const blank = text => text.replace(/[^\n]/g, " ");
const production = sources.map(path => readFileSync(join(repo, path), "utf8").replace(packageImport, blank).replace(coreImport, line => {
    const key = line.trimEnd();
    if (seenCore.has(key)) return blank(line);
    seenCore.add(key);
    return line;
})).join("\n");
const fixtures = [
    ["unsafe-obligations", "Compiler/JetSema/Tests/UnsafeObligations.jet"],
    ["gate-ledger", "Compiler/JetSema/Tests/GateLedger.jet"],
    ["gate-projection", "Compiler/JetCli/Tests/GateProjection.jet"],
];
for (const [name, path] of fixtures) {
    const project = join(out, name);
    mkdirSync(join(project, "src"), { recursive: true });
    writeFileSync(join(project, "src/compiler.jet"), production + "\n" + readFileSync(join(repo, path), "utf8"));
    writeFileSync(join(project, "package.jet"), `name: "${name}"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n`);
    console.log(join(project, "src/compiler.jet"));
}
