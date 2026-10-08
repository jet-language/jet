#!/usr/bin/env node
// Derived from RtL5c6's atomic-field runner: one source/acceptance corpus,
// production checker assembly, and actual CLI receipts in both tiers.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [mode, manifestArg, outArg, cli] = process.argv.slice(2);
if (!outArg || !["assemble", "rust", "jet"].includes(mode) || (mode !== "assemble" && !cli)) {
    throw Error("usage: run-tier-parity-fixtures.mjs assemble MANIFEST OUT | rust|jet MANIFEST OUT CLI");
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const manifest = JSON.parse(readFileSync(resolve(repo, manifestArg), "utf8"));
const cases = (Array.isArray(manifest) ? manifest : manifest.cases).map(row => {
    if (typeof row.accepted !== "boolean" && !Array.isArray(row.errors)) throw Error(`${row.name}: requires accepted or errors`);
    return {
        ...row,
        source: row.sourcePath ? readFileSync(resolve(repo, row.sourcePath), "utf8") : row.source,
        accepted: row.accepted ?? row.errors.length === 0,
    };
});
if (mode === "assemble") {
    if (!manifest.fixtureFile || !manifest.fixtureFunction || !manifest.fixtureArguments) throw Error("assembly requires the manifest's production fixtureFile, fixtureFunction and fixtureArguments");
    const result = spawnSync(process.execPath, [join(repo, "Compiler/JetSema/Tests/run-gate-fixtures.mjs"), out], { encoding: "utf8" });
    if (result.status !== 0) throw Error(result.stderr || result.stdout);
    const assembled = readFileSync(join(out, "unsafe-obligations/src/compiler.jet"), "utf8");
    const oldFixture = readFileSync(join(repo, "Compiler/JetSema/Tests/UnsafeObligations.jet"), "utf8");
    if (!assembled.endsWith(oldFixture)) throw Error("production fixture boundary changed");
    const production = assembled.slice(0, -oldFixture.length);
    const literal = value => {
        if (typeof value === "string") return JSON.stringify(value).replaceAll("{", "{{").replaceAll("}", "}}");
        if (typeof value === "boolean" || typeof value === "number") return String(value);
        throw Error("fixture arguments must be strings, booleans or numbers");
    };
    const calls = cases.map(row => `    ${manifest.fixtureFunction}(${manifest.fixtureArguments.map(key => literal(row[key])).join(", ")})`).join("\n");
    const fixture = readFileSync(resolve(repo, manifest.fixtureFile), "utf8");
    const project = join(out, "tier-parity");
    mkdirSync(join(project, "src"), { recursive: true });
    writeFileSync(join(project, "src/compiler.jet"), production + fixture + `\nfn run() {\n${calls}\n    print("tier parity: ${cases.length} cases")\n}\n`);
    writeFileSync(join(project, "package.jet"), 'name: "tier_parity"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, Mem.Rc, Time.Wait] } }\n');
    console.log(join(project, "src/compiler.jet"));
} else {
    const receipts = [];
    for (const row of cases) {
        const path = join(out, `${row.name}.jet`);
        writeFileSync(path, row.source);
        const result = spawnSync(resolve(cli), ["check", path], { cwd: repo, encoding: "utf8", maxBuffer: 32 << 20 });
        const accepted = result.status === 0;
        const errors = [...new Set([...`${result.stdout ?? ""}\n${result.stderr ?? ""}`.matchAll(/\bE\d{4}\b/g)].map(match => match[0]))].sort();
        const diagnosticOutput = `${result.stdout ?? ""}\n${result.stderr ?? ""}`.replace(/\u001b\[[0-9;]*m/g, "");
        const errorCount = [...diagnosticOutput.matchAll(/^Error \[E\d{4}\]/gm)].length;
        const expectedErrors = row.errors && [...row.errors].sort();
        const passed = !result.error && result.signal === null && result.status !== null && accepted === row.accepted && (!expectedErrors || JSON.stringify(errors) === JSON.stringify(expectedErrors)) && (row.errorCount === undefined || errorCount === row.errorCount);
        receipts.push({ name: row.name, expectedAccepted: row.accepted, expectedErrors, expectedErrorCount: row.errorCount, accepted, errors, errorCount, passed, toolExit: result.status, signal: result.signal, stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
        console.log(`${row.name}: exit=${result.status} accepted=${accepted} expected=${row.accepted} ${passed ? "PASS" : "FAIL"}`);
    }
    writeFileSync(join(out, `${mode}-receipt.json`), JSON.stringify(receipts, null, 2) + "\n");
    if (receipts.some(row => !row.passed)) process.exitCode = 1;
}
