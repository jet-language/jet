#!/usr/bin/env node
// Provenance/dispatch consistency only: this does not execute native programs.
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const lower = join(root, "Compiler/JetBackend/Source/Lower");
const ledger = readFileSync(join(root, "Compiler/JetRuntime/Tests/lists-native.tsv"), "utf8").trim().split("\n");
const failures = [];
const seen = new Set();
const source = new Map();
function read(path) {
    if (!source.has(path)) source.set(path, readFileSync(path, "utf8"));
    return source.get(path);
}
function jetFunction(site) {
    const [file, name] = site.split(":");
    const text = read(join(lower, file));
    const start = text.indexOf(`\nfn ${name}(`);
    if (start < 0 && !text.startsWith(`fn ${name}(`)) throw Error(`missing function ${site}`);
    const begin = start < 0 ? 0 : start + 1;
    const next = text.indexOf("\nfn ", begin);
    return text.slice(begin, next < 0 ? text.length : next);
}
const entry = read(join(lower, "Lower.jet"));
const runtimeCalls = readdirSync(lower).filter(file => file.endsWith(".jet")).flatMap(file =>
    [...read(join(lower, file)).matchAll(/(?:lower_call_runtime|lower_noreturn_call)\(\s*&m,\s*&f,\s*"([^"]+)"/gu)].map(match => ({ file, symbol: match[1] })));
const runtimeRoutes = read(join(lower, "RuntimeRoutes.jet"));
console.log("symbol\tdirect_body_site\tremaining_literal_runtime_calls\troute_row");
for (const line of ledger.slice(1)) {
    const [symbol, dispatch, body, reference, semantics, limits, ...extra] = line.split("\t");
    if (extra.length || !symbol || !dispatch || !body || !reference || !semantics || !limits) {
        failures.push(`malformed row: ${line}`);
        continue;
    }
    if (seen.has(symbol)) failures.push(`duplicate symbol: ${symbol}`);
    seen.add(symbol);
    try {
        const route = jetFunction(dispatch);
        const implementation = jetFunction(body);
        if (!route.includes(`"${symbol}"`)) failures.push(`${symbol}: dispatch does not select symbol`);
        if (!implementation.includes("lower_emit(") && !implementation.includes("lower_call_helper(") && !implementation.includes("lower_exact_arithmetic(") && !implementation.includes("lower_each_begin(")) failures.push(`${symbol}: body has no LIR implementation`);
        if (implementation.includes(`lower_call_runtime(&m, &f, "${symbol}"`)) failures.push(`${symbol}: body still calls Rust provider`);
        const [, dispatchName] = dispatch.split(":");
        const listEntry = jetFunction("Lists.jet:lower_list_method");
        const closureEntry = jetFunction("Combinators.jet:lower_closure_method");
        const linked = entry.includes(`${dispatchName}(p, c,`) ||
            (entry.includes("lower_list_method(p, c,") && listEntry.includes(`${dispatchName}(`)) ||
            (entry.includes("lower_closure_method(p, c,") && closureEntry.includes(`${dispatchName}(`));
        if (!linked) failures.push(`${symbol}: dispatcher is not linked to MIR entry`);
        const [rustFile, rustName] = reference.split(":");
        if (!read(join(root, "crates/jet-codegen/src/Prelude", rustFile)).includes(`fn ${rustName}`)) failures.push(`${symbol}: missing reference ${reference}`);
        const [, bodyName] = body.split(":");
        if (bodyName.startsWith("lower_helper_") && !entry.includes(`${bodyName}(p, &m, glue)`)) failures.push(`${symbol}: helper is not registered`);
        const calls = runtimeCalls.filter(call => call.symbol === symbol);
        const hasRoute = runtimeRoutes.includes(`symbol: "${symbol}"`);
        console.log(`${symbol}\t${body}\t${calls.length}\t${hasRoute ? "present" : "absent (already direct)"}`);
        if (calls.length) failures.push(`${symbol}: remaining runtime calls in ${calls.map(call => call.file).join(", ")}`);
        if (hasRoute) failures.push(`${symbol}: obsolete runtime route remains`);
    } catch (error) { failures.push(`${symbol}: ${error.message}`); }
}
for (const failure of failures) console.error(failure);
console.log(`lists-native source consistency: ${seen.size} rows, ${failures.length} failures (native behavior not measured)`);
if (failures.length) process.exitCode = 1;
