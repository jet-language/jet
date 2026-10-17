#!/usr/bin/env node
// Source/ownership reconciliation, not native execution or all-type coverage.
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runtimeExportSignatures } from '../../../Tools/agent/runtime-route-abi.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
// Rust provenance checks are retired; independent Jet ownership checks stay live.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
const source = new Map(), failures = [];
function read(path) {
  if (!source.has(path)) source.set(path, readFileSync(join(root, path), 'utf8'));
  return source.get(path);
}
function rows(path, columns) {
  const lines = read(path).trim().split('\n').slice(1).map(line => line.split('\t'));
  const seen = new Set();
  for (const row of lines) {
    if (row.length !== columns || row.some(value => !value)) throw Error(`malformed ${path}: ${row.join('\t')}`);
    if (seen.has(row[0])) throw Error(`duplicate ${path}: ${row[0]}`);
    seen.add(row[0]);
  }
  return lines;
}
const functions = new Map();
function collect(path) {
  const text = read(path), matches = [...text.matchAll(/^(?:#Export\(c\)\s+)?fn (\w+)\(/gm)];
  for (let i = 0; i < matches.length; i++) functions.set(`${path}:${matches[i][1]}`, text.slice(matches[i].index, matches[i + 1]?.index ?? text.length));
}
const lower = 'Compiler/JetBackend/Source/Lower/';
const lowerPaths = readdirSync(join(root, lower)).filter(file => file.endsWith('.jet')).map(file => lower + file);
for (const path of lowerPaths) collect(path);
const runtimePaths = read('Compiler/JetRuntime/sources.list').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#'));
for (const path of runtimePaths) collect(path);
const exported = new Set(runtimeExportSignatures(root).keys());
const literalCalls = lowerPaths.flatMap(path => [...read(path).matchAll(/(?:lower_call_runtime|lower_noreturn_call)\(\s*&m,\s*&f,\s*"([^"]+)"/g)].map(match => [path, match[1]]));
const reachable = new Set([...functions.keys()].filter(site => site.startsWith(lower + 'Lower.jet:')));
let changed = true;
while (changed) {
  changed = false;
  for (const [site] of functions) {
    if (!site.startsWith(lower) || reachable.has(site)) continue;
    const name = site.slice(site.indexOf(':') + 1);
    if ([...reachable].some(caller => functions.get(caller).slice(functions.get(caller).indexOf('{') + 1).includes(`${name}(`))) {
      reachable.add(site); changed = true;
    }
  }
}
const census = rows('Compiler/JetRuntime/Tests/collections-census.tsv', 7);
const groups = {};
for (const [symbol, status, dispatch, body, nativeExport, reference] of census) {
  groups[symbol.split('_')[1]] = (groups[symbol.split('_')[1]] ?? 0) + 1;
  const route = functions.get(dispatch), implementation = functions.get(body);
  if (!route || !route.includes(`"${symbol}"`)) failures.push(`${symbol}: missing checked dispatcher ${dispatch}`);
  if (!reachable.has(dispatch)) failures.push(`${symbol}: dispatcher not reachable from Lower entry`);
  if (!implementation) failures.push(`${symbol}: missing Jet body ${body}`);
  if (literalCalls.some(([, called]) => called === symbol)) failures.push(`${symbol}: literal Rust provider call remains`);
  if (status === 'native-provider') {
    if (!exported.has(nativeExport) || !implementation?.startsWith(`#Export(c) fn ${nativeExport}(`)) failures.push(`${symbol}: missing exported Jet provider ${nativeExport}`);
    const marshal = read(lower + 'LazyIter.jet');
    if (!marshal.includes(`"${nativeExport}"`) && !(marshal.includes('"jet_rt_native_{runtime_abi_atom(tail)}"') && nativeExport === 'jet_rt_native_' + symbol.slice(4))) failures.push(`${symbol}: no native provider marshalling`);
  } else if (status !== 'native-LIR' || nativeExport !== '-' || !reachable.has(body)) failures.push(`${symbol}: invalid direct-LIR body/status`);
}
const expected = { iter: 32, list: 48, loop: 1, map: 28, option: 1, slice: 3, view: 6 };
if (census.length !== 119 || Object.keys(groups).length !== 7 || Object.entries(expected).some(([key, count]) => groups[key] !== count)) failures.push(`census coverage drift: ${JSON.stringify(groups)}`);
const hands = rows('Compiler/JetRuntime/Tests/collections-hand-abi.tsv', 5);
for (const [symbol, status, nativeExport, provider, rustPath] of hands) {
  const implementation = functions.get(provider);
  if (!exported.has(nativeExport) || !implementation?.startsWith(`#Export(c) fn ${nativeExport}(`)) failures.push(`${symbol}: missing native provider ${provider}`);
  if (status === 'removed') {
    const dispatch = read(lower + 'LazyIter.jet');
    if (!dispatch.includes(`"${symbol}"`) || !dispatch.includes('"jet_rt_native_{runtime_abi_atom(tail)}"')) failures.push(`${symbol}: missing native callback/count dispatcher`);
  } else if (status !== 'seed-only') failures.push(`${symbol}: invalid handwritten ABI disposition`);
}
if (hands.length !== 19 || hands.filter(row => row[1] === 'removed').length !== 14 || hands.filter(row => row[1] === 'seed-only').length !== 5) failures.push('handwritten ABI coverage drift');
const residue = rows('Compiler/JetRuntime/Tests/collections-residue.tsv', 9);
for (const symbol of ['jet_range_value_contains', 'JetRegex_count']) {
  const row = residue.find(row => row[0] === symbol);
  if (!row || row[1] !== 'provider-live' || !row[8].startsWith('NAMED MISSING PREREQUISITE:')) failures.push(`${symbol}: lost G1-carrier prerequisite`);
}
for (const failure of failures) console.error(failure);
console.log(`collections census source consistency: ${census.length} rust-fn rows (${census.filter(row => row[1] === 'native-LIR').length} direct, ${census.filter(row => row[1] === 'native-provider').length} Jet providers), ${hands.length} handwritten ABI rows, 2 named G1 prerequisites, ${failures.length} failures (native behavior not measured)`);
if (failures.length) process.exitCode = 1;
