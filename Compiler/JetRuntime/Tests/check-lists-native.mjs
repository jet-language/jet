#!/usr/bin/env node
// Provenance/dispatch consistency only: this does not execute native programs.
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runtimeExportSignatures } from "../../../Tools/agent/runtime-route-abi.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
// Rust provenance checks are retired; independent Jet dispatch checks stay live.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
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
    const runtime = file.startsWith("runtime/");
    const path = runtime ? join(root, "Compiler/JetRuntime/Source", file.slice(8)) : join(lower, file);
    const text = read(path);
    const match = new RegExp(`(?:^|\\n)(?:#Export\\(c\\) )?fn ${name}\\(`, "u").exec(text);
    if (!match) throw Error(`missing function ${site}`);
    const begin = match.index + (match[0].startsWith("\n") ? 1 : 0);
    const next = /\n(?:#Export\(c\) )?fn /u.exec(text.slice(begin + 1));
    return text.slice(begin, next ? begin + 1 + next.index : text.length);
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
        const [, bodyName] = body.split(":");
        if (bodyName.startsWith("lower_helper_") && !entry.includes(`${bodyName}(p, &m, glue)`)) failures.push(`${symbol}: helper is not registered`);
        const calls = runtimeCalls.filter(call => call.symbol === symbol);
        const hasRoute = runtimeRoutes.includes(`symbol: "${symbol}"`);
        console.log(`${symbol}\t${body}\t${calls.length}\t${hasRoute ? "present" : "absent (already direct)"}`);
        if (calls.length) failures.push(`${symbol}: remaining runtime calls in ${calls.map(call => call.file).join(", ")}`);
        if (hasRoute) failures.push(`${symbol}: obsolete runtime route remains`);
    } catch (error) { failures.push(`${symbol}: ${error.message}`); }
}
const residue = read(join(root, "Compiler/JetRuntime/Tests/collections-residue.tsv")).trim().split("\n").slice(1);
const residueSeen = new Set();
const nativeExports = new Set(runtimeExportSignatures(root).keys());
let nativePaths = 0, nativeProviders = 0, liveProviders = 0;
for (const line of residue) {
    const [symbol, status, dispatch, body, reference, remaining, routeState, semantics, limits, ...extra] = line.split("\t");
    if (!symbol || !status || !dispatch || !body || !reference || !remaining || !routeState || !semantics || !limits || extra.length || residueSeen.has(symbol)) {
        failures.push(`invalid or duplicate residue row: ${symbol}`);
        continue;
    }
    residueSeen.add(symbol);
    try {
        jetFunction(dispatch);
        const code = jetFunction(body);
        const hasRoute = runtimeRoutes.includes(`export: "${symbol}"`);
        if (hasRoute !== (routeState === "present")) failures.push(`${symbol}: route metadata state changed; reconcile the ledger`);
        if (status === "native-path") {
            nativePaths++;
            if (!/lower_emit|lower_call|lower_slice_window/u.test(code)) failures.push(`${symbol}: direct lowering body is missing`);
            const calls = runtimeCalls.filter(call => call.symbol === symbol);
            if (calls.length) failures.push(`${symbol}: remaining literal provider calls in ${calls.map(call => call.file).join(", ")}`);
        } else if (status === "native-export") {
            nativeProviders++;
            if (!nativeExports.has(symbol) || !code.includes(`#Export(c) fn ${symbol}(`)) failures.push(`${symbol}: native provider declaration/ownership missing`);
        } else if (status === "provider-live") {
            liveProviders++;
            if (nativeExports.has(symbol)) failures.push(`${symbol}: provider has acquired a native export; reconcile its call path`);
            if (!/lower_runtime_symbol|LIRDrop\.Runtime|lower_route_result/u.test(code)) failures.push(`${symbol}: missing dynamic provider/drop path`);
        } else failures.push(`${symbol}: unknown residue status ${status}`);
        console.log(`${symbol}\t${status}\t${body}\t${remaining}\troute ${routeState}`);
    } catch (error) { failures.push(`${symbol}: ${error.message}`); }
}
if (residueSeen.size !== 21) failures.push(`expected 21 residue rows, found ${residueSeen.size}`);
console.log(`collection residue consistency: ${residueSeen.size} rows, ${nativePaths} native paths, ${nativeProviders} native providers, ${liveProviders} live providers (dynamic/drop calls included; native behavior not measured)`);
for (const failure of failures) console.error(failure);
console.log(`lists-native source consistency: ${seen.size} rows, ${failures.length} failures (native behavior not measured)`);
if (failures.length) process.exitCode = 1;
