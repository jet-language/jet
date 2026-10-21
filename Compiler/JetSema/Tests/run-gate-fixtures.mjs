#!/usr/bin/env node
// Production sources with each executable fixture. The tuple registry fixture
// needs the Foundation package; gate fixtures need the whole compiler (every
// package the CLI depends on, without the CLI's own entry).
// Usage: node Compiler/JetSema/Tests/run-gate-fixtures.mjs OUTDIR
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compilerFixtureUnit, compilerPackages } from "../../../Tools/agent/compiler-packages.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-gate-fixtures.mjs OUTDIR");
// A fixture is part of the unit's namespace: its compiler package imports blank as production's do.
const packageImport = new RegExp(`^use (?:${[...compilerPackages(repo).keys()].join("|")})\\.(?:\\[[^\\]]*\\]|[A-Za-z_][A-Za-z0-9_]*[ \\t]*$)`, "gm");
const blank = text => text.replace(/[^\n]/g, " ");
const production = compilerFixtureUnit(repo);
const foundation = compilerFixtureUnit(repo, ["Compiler/JetFoundation"]);
const fixtures = [
    ["unsafe-obligations", "Compiler/JetSema/Tests/UnsafeObligations.fixture/UnsafeObligations.jet"],
    ["gate-ledger", "Compiler/JetSema/Tests/GateLedger.fixture/GateLedger.jet"],
    ["gate-projection", "Compiler/JetCli/Tests/GateProjection.fixture/GateProjection.jet"],
    ["export-boundary", "Compiler/JetSema/Tests/ExportBoundary.fixture/ExportBoundary.jet"],
    ["view-field-provenance", "Compiler/JetSema/Tests/ViewFieldProvenance.fixture/ViewFieldProvenance.jet"],
    ["named-tuple-identity", "Compiler/JetSema/Tests/NamedTupleIdentityFixtures.jet"],
    ["recovered-frozen", "Compiler/JetSema/Tests/FrozenFixtures.fixture/FrozenFixtures.jet"],
    ["recovered-policy-guest", "Compiler/JetSema/Tests/BurnDownFixtures.fixture/BurnDownFixtures.jet"],
];
for (const [name, path] of fixtures) {
    const project = join(out, name);
    mkdirSync(join(project, "src"), { recursive: true });
    const fixture = readFileSync(join(repo, path), "utf8").replace(packageImport, blank);
    const entry = name === "named-tuple-identity" ? "\nfn run() { named_tuple_identity_fixture() }\n" : "";
    const implementation = name === "named-tuple-identity" ? foundation : production;
    writeFileSync(join(project, "src/compiler.jet"), implementation + "\n" + fixture + entry);
    writeFileSync(join(project, "package.jet"), `name: "${name}"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, Mem.Rc, Time.Wait] } }\n`);
    console.log(join(project, "src/compiler.jet"));
}
