#!/usr/bin/env node
// Current slot ABI, not the obsolete generic collection-name census.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { candidateJetc0, runJetFront } from "../../JetBackend/Tests/jet-front.mjs";
import { collectGoldenEntries } from "../../../Tools/agent/compiler-diff.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
import {readGoldenResults} from "./golden-results.mjs";
const [output, baseline, mode, sharedResults] = process.argv.slice(2);
if (!output || !baseline || (mode && mode !== "--plan" && mode !== "--results") ||
    (mode === "--results" ? !sharedResults || process.argv.length !== 6 : process.argv.length > 5)) {
    throw Error("usage: run-collections-goldens.mjs OUT BASELINE.results.tsv [--plan | --results fresh-results.tsv]");
}
const out = resolve(output);
const rows = readFileSync(baseline, "utf8").trim().split("\n").map(line => line.split("\t"));
const prior = new Map(rows.map(([stem, verdict]) => [stem, verdict]));
if (prior.size !== rows.length || !rows.some(([, verdict]) => verdict === "pass")) throw Error("empty or duplicate baseline");
const focused = ["filter_map", "fold_association", "iter_adapters", "iter_join", "iter_numeric_bounds", "lazy_iter", "list_surface", "lists", "map_surface", "operation_contract", "zip_family", "runtime_iter_core", "runtime_iter_cancelled"].map(stem => `collections/${stem}`);
focused.push("text/runtime_text_core");
const stems = [...new Set([...rows.filter(([, verdict]) => verdict === "pass").map(([stem]) => stem), ...focused])].sort();
const available = new Map(collectGoldenEntries(join(root, "Examples/features")).map(entry => [entry.stem, entry]));
const missing = stems.flatMap(stem => [
    ...(!available.has(stem) ? [`missing golden source for ${stem}`] : []),
    ...(!existsSync(join(root, "Examples/features/expected", `${stem}.out`)) ? [`missing golden output for ${stem}`] : []),
]);
if (missing.length) throw Error(missing.join("\n"));
mkdirSync(out, { recursive: true });
const cases = join(out, "collections.cases");
writeFileSync(cases, `${stems.join("\n")}\n`);
console.log(JSON.stringify({ baselinePassing: rows.filter(([, verdict]) => verdict === "pass").length, focused: focused.length, required: stems.length }));
if (mode !== "--plan") {
    const object = process.env.JET_RUNTIME_JET_OBJECT, pack = process.env.JET_RUNTIME_PACK;
    if (!object || !pack || !existsSync(object) || !existsSync(pack)) throw Error("JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK must name this tree's freshly built runtime");
    const failures = [];
    function symbols(file, args, name) {
        const run = spawnSync("nm", [...args, "--format=posix", file], { encoding: "utf8", maxBuffer: 1 << 28 });
        writeFileSync(join(out, name), `${run.stdout ?? ""}${run.stderr ?? ""}`);
        if (run.error || run.status !== 0) { failures.push(`nm ${file}: ${run.error?.message ?? run.stderr}`); return []; }
        return run.stdout.split("\n").filter(Boolean).map(line => line.trim().split(/\s+/u));
    }
    const defined = symbols(object, ["-g", "--defined-only"], "object-symbols.txt");
    const packed = symbols(pack, ["-g", "--defined-only"], "pack-symbols.txt");
    const required = ["Collections", "Iter", "IterNumeric"].flatMap(name => [...readFileSync(join(root, `Compiler/JetRuntime/Source/${name}.jet`), "utf8").matchAll(/#Export\(c\) fn (\w+)\(/gu)].map(match => match[1]));
    required.push("jet_rt_native_iter_string_split", "jet_rt_native_iter_string_rsplit");
    if (!required.length || new Set(required).size !== required.length) throw Error("empty or duplicate collection exports");
    for (const name of required) for (const [label, records] of [["Jet object", defined], ["pack", packed]]) {
        const count = records.filter(([symbol, kind]) => symbol === name && kind === "T").length;
        if (count !== 1) failures.push(`${label}: ${name} has ${count} strong definitions`);
    }
    const imports = symbols(object, ["-u"], "object-imports.txt").map(([name]) => name);
    const unresolved = symbols(pack, ["-u"], "pack-imports.txt");
    if (unresolved.length) failures.push(`pack has unresolved imports: ${unresolved.map(([name]) => name).join(", ")}`);
    const legacy = imports.filter(name => /^jet_(?:iter|list|map)_/u.test(name));
    if (legacy.length) failures.push(`obsolete generic collection imports: ${legacy.join(", ")}`);
    // A combined L3a/L5 proof compiles the union once, then each layer checks
    // the same actual result stream and executes its own raw ownership witnesses.
    const harness = mode === "--results" ? null : spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--front", "jet", "--case-list", cases], { cwd: root, stdio: "inherit", env: process.env });
    const resultPath = sharedResults ? resolve(sharedResults) : join(out, "results.tsv");
    const results = existsSync(resultPath) ? readGoldenResults(resultPath) : new Map();
    failures.push(...stems.filter(stem => results.get(stem) !== "pass").map(stem => `${stem}: ${results.get(stem) ?? "missing"}`));
    if (harness && (harness.error || harness.status !== 0)) failures.push(`native harness: ${harness.error?.message ?? `rc=${harness.status}`}`);
    const raw = join(out, "raw");
    const assembled = spawnSync(process.execPath, [join(root, "Compiler/JetRuntime/Tests/check-runtime.mjs"), raw, "--assemble-only", "--test", "Collections"], { cwd: root, stdio: "inherit" });
    if (assembled.error || assembled.status !== 0) failures.push(`raw assembly: ${assembled.error?.message ?? `rc=${assembled.status}`}`);
    else {
        try {
            // The assembler's package is an entryless runtime library. Match
            // Text's witness path: compile the test entry outside that boundary.
            const executableRoot = join(out, "raw-native");
            mkdirSync(executableRoot, { recursive: true });
            const program = join(executableRoot, "unit.jet");
            writeFileSync(program, readFileSync(join(raw, "unit.jet"), "utf8") + "\nfn run() { collections_tests() }\n");
            const prefix = join(executableRoot, "collections-test");
            const problem = runJetFront({ candidate: candidateJetc0(), file: program, root: executableRoot, cwd: root, prefix, pack, store: join(raw, "store") });
            if (problem) failures.push(`raw front: ${problem.detail}`);
            else {
                const run = spawnSync(`${prefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
                writeFileSync(join(raw, "run.stdout"), run.stdout ?? "");
                writeFileSync(join(raw, "run.stderr"), run.stderr ?? "");
                writeFileSync(join(raw, "run.status.json"), `${JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null })}\n`);
                if (run.error || run.status !== 0 || run.signal || run.stdout || run.stderr) failures.push(`raw run: ${run.error?.message ?? `rc=${run.status}, signal=${run.signal}, stdout=${JSON.stringify(run.stdout)}, stderr=${JSON.stringify(run.stderr)}`}`);
            }
            // Rust/source numeric stop golden: Examples/features/expected/errors/u8_add_overflow.err.out:1.
            const overflowHeadline = "Stop [E3010]: `This addition overflows the value's type (the result is outside its range)`\n  --> <core.prelude>:0\n";
            // Rust/source bounds stop golden: Examples/features/expected/collections/list_bounds.err.out:1.
            const boundsHeadline = "Stop [E3010]: `the list has 0 items, so position 0 doesn't exist`\n  --> <core.prelude>:0\n";
            for (const [name, entry, headline] of [
                ["bits-extent", "coll_test_bits_extent_stop()", overflowHeadline],
                ["chain-get-bounds", "coll_test_chain_bounds_stop(false)", boundsHeadline],
                ["chain-contains-bounds", "coll_test_chain_bounds_stop(true)", boundsHeadline],
                ["range-zero-step", "coll_test_range_argument_stop(false)", "Stop [E3001]: `panic: range loop stride must not be zero`\n  --> <core.prelude>:0\n"],
                ["range-exhausted", "coll_test_range_argument_stop(true)", "Stop [E3001]: `panic: range loop value requested after exhaustion`\n  --> <core.prelude>:0\n"],
            ]) {
                const directory = join(executableRoot, name);
                mkdirSync(directory, { recursive: true });
                const stopFile = join(directory, "unit.jet");
                const stopPrefix = join(directory, "stop");
                writeFileSync(stopFile, readFileSync(join(raw, "unit.jet"), "utf8") + `\nfn run() { ${entry} }\n`);
                const problem = runJetFront({ candidate: candidateJetc0(), file: stopFile, root: directory, cwd: root, prefix: stopPrefix, pack, store: join(raw, "store") });
                if (problem) failures.push(`${name} compile: ${problem.detail}`);
                else {
                    const stopped = spawnSync(`${stopPrefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
                    writeFileSync(join(directory, "run.stdout"), stopped.stdout ?? "");
                    writeFileSync(join(directory, "run.stderr"), stopped.stderr ?? "");
                    writeFileSync(join(directory, "run.status.json"), `${JSON.stringify({ status: stopped.status, signal: stopped.signal, error: stopped.error?.message ?? null })}\n`);
                    if (stopped.error || stopped.status !== 70 || stopped.signal || stopped.stdout || !stopped.stderr?.startsWith(headline)) failures.push(`${name}: ${stopped.error?.message ?? `rc=${stopped.status}, signal=${stopped.signal}, stdout=${JSON.stringify(stopped.stdout)}, stderr=${JSON.stringify(stopped.stderr)}`}`);
                }
            }
        } catch (error) { failures.push(`raw witness: ${error.message}`); }
    }
    const stopped = join(out, "stopped");
    for (const stage of ["--assemble", "--lower", "--run"]) {
        const result = spawnSync(process.execPath, [join(root, "Compiler/JetRuntime/Tests/run-collections-stop.mjs"), stopped, stage], { cwd: root, stdio: "inherit", env: process.env });
        if (result.error || result.status !== 0) failures.push(`stopped-result ${stage}: ${result.error?.message ?? `rc=${result.status}`}`);
    }
    const newlyPassing = stems.filter(stem => stem.startsWith("collections/") && prior.has(stem) && prior.get(stem) !== "pass" && results.get(stem) === "pass");
    const receipt = { required: stems.length, passed: stems.filter(stem => results.get(stem) === "pass").length, newlyPassingCount: newlyPassing.length, newlyPassing, failures, requiredExports: required, imports, object, pack, results: resultPath };
    writeFileSync(join(out, "collections-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
    console.log(JSON.stringify(receipt, null, 2));
    for (const failure of failures) console.error(failure);
    if (failures.length) process.exitCode = 1;
}
