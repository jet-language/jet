#!/usr/bin/env node
// Shared source/acceptance corpus: assemble the Jet checker witness, or check
// those exact inputs with the Rust seed CLI. Neither mode invents a mock rule.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [mode, outArg, rustCli] = process.argv.slice(2);
if (!outArg || !["assemble", "rust"].includes(mode) || (mode === "rust" && !rustCli)) throw Error("usage: run-atomic-field-fixtures.mjs assemble OUT | rust OUT RUST_JET");
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const cases = JSON.parse(readFileSync(join(repo, "Compiler/JetSema/Tests/atomic-field-parity.json"), "utf8"));
if (mode === "assemble") {
    const result = spawnSync(process.execPath, [join(repo, "Compiler/JetSema/Tests/run-gate-fixtures.mjs"), out], { encoding: "utf8" });
    if (result.status !== 0) throw Error(result.stderr || result.stdout);
    // Reuse the canonical whole-production fixture assembler, replacing only
    // its fixture tail; no shadow checker or duplicate constructor is compiled.
    const assembled = readFileSync(join(out, "unsafe-obligations/src/compiler.jet"), "utf8");
    const oldFixture = readFileSync(join(repo, "Compiler/JetSema/Tests/UnsafeObligations.jet"), "utf8");
    if (!assembled.endsWith(oldFixture)) throw Error("production fixture boundary changed");
    const production = assembled.slice(0, -oldFixture.length);
    const quote = text => JSON.stringify(text).replaceAll("{", "{{").replaceAll("}", "}}");
    const calls = cases.map(row => `    atomic_field_test_source(${quote(row.name)}, ${quote(row.source)}, ${row.accepted}, ${row.atomic})`).join("\n");
    const fixture = readFileSync(join(repo, "Compiler/JetSema/Tests/AtomicFieldFixtures.jet"), "utf8");
    const project = join(out, "atomic-fields");
    mkdirSync(join(project, "src"), { recursive: true });
    writeFileSync(join(project, "src/compiler.jet"), production + fixture + `\nfn run() {\n${calls}\n    print("atomic-field parity: ${cases.length} cases")\n}\n`);
    writeFileSync(join(project, "package.jet"), 'name: "atomic_fields"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, Mem.Rc, Time.Wait] } }\n');
    console.log(join(project, "src/compiler.jet"));
} else {
    const receipts = [];
    for (const row of cases) {
        const path = join(out, `${row.name}.jet`);
        writeFileSync(path, row.source);
        const result = spawnSync(resolve(rustCli), ["check", path], { cwd: repo, encoding: "utf8", maxBuffer: 32 << 20 });
        const accepted = result.status === 0;
        const passed = !result.error && result.signal === null && result.status !== null && accepted === row.accepted;
        receipts.push({ name: row.name, expectedAccepted: row.accepted, accepted, passed, toolExit: result.status, signal: result.signal, stdout: result.stdout, stderr: result.stderr, error: result.error?.message });
        console.log(`${row.name}: exit=${result.status} accepted=${accepted} expected=${row.accepted} ${passed ? "PASS" : "FAIL"}`);
    }
    writeFileSync(join(out, "rust-receipt.json"), JSON.stringify(receipts, null, 2) + "\n");
    if (receipts.some(row => !row.passed)) process.exitCode = 1;
}
