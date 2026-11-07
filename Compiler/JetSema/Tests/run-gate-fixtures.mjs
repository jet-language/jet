#!/usr/bin/env node
// Production sources with each executable fixture. The tuple registry fixture
// needs the Foundation package; gate fixtures need the whole compiler (every
// package the CLI depends on, without the CLI's own entry), with the files its
// comptime inputs name copied beside the joined unit.
// Usage: node Compiler/JetSema/Tests/run-gate-fixtures.mjs OUTDIR
import { execFileSync } from "node:child_process";
import { copyFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { toolJet } from "../../../Tools/agent/roots.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-gate-fixtures.mjs OUTDIR");
// Tools/agent/CompilerPackages.jet, run by roots.mjs toolJet.
const compilerPackages = (...args) => execFileSync(toolJet(), [join(repo, "Tools/agent/CompilerPackages.jet"), "--root", repo, ...args], { maxBuffer: 1 << 30 }).toString("utf8");
const rows = text => text.split("\n").filter(Boolean).map(row => row.split("\t"));
// A fixture is part of the unit's namespace: its compiler package imports blank as production's do.
const packageImport = new RegExp(`^use (?:${rows(compilerPackages("packages")).map(([name]) => name).join("|")})\\.(?:\\[[^\\]]*\\]|[A-Za-z_][A-Za-z0-9_]*[ \\t]*$)`, "gm");
const blank = text => text.replace(/[^\n]/g, " ");
const production = compilerPackages("unit");
const productionInputs = rows(compilerPackages("unit-inputs"));
const foundation = compilerPackages("unit", "Compiler/JetFoundation");
const foundationInputs = rows(compilerPackages("unit-inputs", "Compiler/JetFoundation"));
const fixtures = [
    ["unsafe-obligations", "Compiler/JetSema/Tests/UnsafeObligations.fixture/UnsafeObligations.jet"],
    ["gate-ledger", "Compiler/JetSema/Tests/GateLedger.fixture/GateLedger.jet"],
    ["gate-projection", "Compiler/JetCli/Tests/GateProjection.fixture/GateProjection.jet"],
    ["view-field-provenance", "Compiler/JetSema/Tests/ViewFieldProvenance.fixture/ViewFieldProvenance.jet"],
    ["named-tuple-identity", "Compiler/JetSema/Tests/NamedTupleIdentityFixtures.jet"],
    ["recovered-frozen", "Compiler/JetSema/Tests/FrozenFixtures.fixture/FrozenFixtures.jet"],
    ["recovered-policy-guest", "Compiler/JetSema/Tests/BurnDownFixtures.fixture/BurnDownFixtures.jet"],
];
const unblocked = compilerPackages("unblock", ...fixtures.map(([, path]) => path)).split("\0");
for (const [index, [name]] of fixtures.entries()) {
    const project = join(out, name);
    mkdirSync(join(project, "src"), { recursive: true });
    const fixture = unblocked[index].replace(packageImport, blank);
    const entry = name === "named-tuple-identity" ? "\nrun: fn() { named_tuple_identity_fixture() }\n" : "";
    const implementation = name === "named-tuple-identity" ? foundation : production;
    writeFileSync(join(project, "src/compiler.jet"), implementation + "\n" + fixture + entry);
    for (const [literal, file] of name === "named-tuple-identity" ? foundationInputs : productionInputs) {
        mkdirSync(dirname(join(project, "src", literal)), { recursive: true });
        copyFileSync(file, join(project, "src", literal));
    }
    writeFileSync(join(project, "package.jet"), `name: "${name}"\nversion: "0.1.0"\nauthority: { allow: [IO, Mem.Alloc, Mem.Rc, Time.Wait] }\n`);
    console.log(join(project, "src/compiler.jet"));
}
