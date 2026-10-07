#!/usr/bin/env node
// Current slot ABI, not the obsolete generic collection-name census.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { candidateJetc0, runJetFront } from "../../JetBackend/Tests/jet-front.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, baseline, mode] = process.argv.slice(2);
if (!output || !baseline || (mode && mode !== "--plan") || process.argv.length > 5) throw Error("usage: run-collections-goldens.mjs OUT BASELINE.results.tsv [--plan]");
const out = resolve(output);
const rows = readFileSync(baseline, "utf8").trim().split("\n").map(line => line.split("\t"));
const prior = new Map(rows.map(([stem, verdict]) => [stem, verdict]));
if (prior.size !== rows.length || !rows.some(([, verdict]) => verdict === "pass")) throw Error("empty or duplicate baseline");
const focused = ["filter_map", "fold_association", "iter_adapters", "iter_join", "iter_numeric_bounds", "lazy_iter", "list_surface", "lists", "map_surface", "operation_contract", "zip_family", "runtime_iter_core", "runtime_iter_cancelled"].map(stem => `collections/${stem}`);
const stems = [...new Set([...rows.filter(([, verdict]) => verdict === "pass").map(([stem]) => stem), ...focused])].sort();
for (const stem of stems) for (const path of [join(root, "Examples/features", `${stem}.jet`), join(root, "Examples/features/expected", `${stem}.out`)]) if (!existsSync(path)) throw Error(`missing golden ${path}`);
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
    const required = ["Iter", "IterNumeric"].flatMap(name => [...readFileSync(join(root, `Compiler/JetRuntime/Source/${name}.jet`), "utf8").matchAll(/#Export\(c\) fn (jet_rt_native_iter_\w+)\(/gu)].map(match => match[1]));
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
    const harness = spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--front", "jet", "--case-list", cases], { cwd: root, stdio: "inherit", env: process.env });
    const resultPath = join(out, "results.tsv");
    const results = existsSync(resultPath) ? new Map(readFileSync(resultPath, "utf8").trim().split("\n").map(line => line.split("\t"))) : new Map();
    failures.push(...stems.filter(stem => results.get(stem) !== "pass").map(stem => `${stem}: ${results.get(stem) ?? "missing"}`));
    if (harness.error || harness.status !== 0) failures.push(`native harness: ${harness.error?.message ?? `rc=${harness.status}`}`);
    const raw = join(out, "raw");
    const assembled = spawnSync(process.execPath, [join(root, "Compiler/JetRuntime/Tests/check-runtime.mjs"), raw, "--assemble-only", "--test", "Collections"], { cwd: root, stdio: "inherit" });
    if (assembled.error || assembled.status !== 0) failures.push(`raw assembly: ${assembled.error?.message ?? `rc=${assembled.status}`}`);
    else {
        try {
            const prefix = join(raw, "collections-test");
            const problem = runJetFront({ candidate: candidateJetc0(), file: join(raw, "unit.jet"), root: raw, cwd: root, prefix, receipt: join(raw, "front.receipt"), pack, store: join(raw, "store") });
            if (problem) failures.push(`raw front: ${problem.detail}`);
            else {
                const run = spawnSync(`${prefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
                writeFileSync(join(raw, "run.stdout"), run.stdout ?? "");
                writeFileSync(join(raw, "run.stderr"), run.stderr ?? "");
                writeFileSync(join(raw, "run.status.json"), `${JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null })}\n`);
                if (run.error || run.status !== 0 || run.signal || run.stdout || run.stderr) failures.push(`raw run: ${run.error?.message ?? `rc=${run.status}, signal=${run.signal}, stdout=${JSON.stringify(run.stdout)}, stderr=${JSON.stringify(run.stderr)}`}`);
            }
        } catch (error) { failures.push(`raw witness: ${error.message}`); }
    }
    const newlyPassing = stems.filter(stem => stem.startsWith("collections/") && prior.has(stem) && prior.get(stem) !== "pass" && results.get(stem) === "pass");
    const receipt = { required: stems.length, passed: stems.filter(stem => results.get(stem) === "pass").length, newlyPassingCount: newlyPassing.length, newlyPassing, failures, requiredExports: required, imports, object, pack };
    writeFileSync(join(out, "collections-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
    console.log(JSON.stringify(receipt, null, 2));
    for (const failure of failures) console.error(failure);
    if (failures.length) process.exitCode = 1;
}
