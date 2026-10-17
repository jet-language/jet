#!/usr/bin/env node
// Inventory classification, not an emission seed or a native layout witness.
// The MIR-backed lower_native_source_type predicate makes final ownership.
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runtimeAbiAtom } from './runtime-abi-symbols.mjs';

export function classifyNativeInventory(inventory, routes, source) {
  const types = new Map();
  for (const match of routes.matchAll(/LowerRuntimeHandle\{jet_name: "([^"]+)", runtime: "([^"]+)"/g)) {
    const list = types.get(match[2]) ?? [];
    list.push(match[1]);
    types.set(match[2], list);
  }
  const schemas = new Set([...routes.matchAll(/LowerRecordSchema\{jet_name: "([^"]+)"/g)].map((match) => match[1]));
  const names = [...types.keys()].sort((a, b) => runtimeAbiAtom(b).length - runtimeAbiAtom(a).length || a.localeCompare(b));
  const rows = new Map();
  const unclassified = [];
  const inventoryLines = inventory.trimEnd().split('\n');
  const columns = inventoryLines[0].split('\t');
  const symbolColumn = columns.indexOf('symbol'), layerColumn = columns.indexOf('layer');
  if (symbolColumn < 0 || layerColumn < 0) throw new Error('native inventory must contain symbol and layer columns');
  for (const line of inventoryLines.slice(1)) {
    const fields = line.split('\t');
    if (fields[layerColumn] !== 'cabi-glue') continue;
    const symbol = fields[symbolColumn];
    if (!/^jet_rt_(?:handle|record)_/.test(symbol)) continue;
    const family = /^jet_rt_(?:handle_(?:clone|drop|eq|display|show|debug|field)|record_(?:new|get|tag))_(.*)$/.exec(symbol);
    const runtime = family ? names.find((name) => family[1] === runtimeAbiAtom(name) || family[1].startsWith(`${runtimeAbiAtom(name)}_`)) : null;
    if (!runtime) { unclassified.push(symbol); continue; }
    let row = rows.get(runtime);
    if (!row) {
      const declarations = [];
      for (const key of types.get(runtime)) {
        const match = /^<corelib>\/[^:]+::(Core\/[^:]+\.jet)::([A-Za-z_]\w*)(?:<.*>)?$/.exec(key);
        if (!match) continue;
        const text = source(match[1]);
        if (text !== null && new RegExp(`\\b(?:struct|enum)\\s+${match[2]}\\b`).test(text.replace(/\/\/[^\n]*/g, ''))) declarations.push({ key, path: match[1], name: match[2] });
      }
      row = { runtime, declarations, schema: schemas.has(runtime), symbols: [] };
      rows.set(runtime, row);
    }
    row.symbols.push(symbol);
  }
  const classified = [...rows.values()].map((row) => ({ ...row,
    status: row.declarations.length === 0 ? 'rust-only' : row.schema ? 'source-record-candidate' : 'source-opaque-candidate',
    nativeLayout: 'unexercised: requires current MIR ownership predicate',
    symbols: [...new Set(row.symbols)].sort(),
  })).sort((a, b) => b.symbols.length - a.symbols.length || a.runtime.localeCompare(b.runtime));
  return { types: classified, unclassified: unclassified.sort(), counts: {
    types: classified.length,
    symbols: classified.reduce((count, row) => count + row.symbols.length, 0),
    rustOnlyTypes: classified.filter((row) => row.status === 'rust-only').length,
    sourceRecordCandidates: classified.filter((row) => row.status === 'source-record-candidate').length,
    sourceOpaqueCandidates: classified.filter((row) => row.status === 'source-opaque-candidate').length,
    unclassifiedSymbols: unclassified.length,
  } };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [inventory, routes, output] = process.argv.slice(2);
  if (!inventory || !routes || !output) throw new Error('usage: classify-native-route-glue.mjs <inventory.tsv> <RuntimeRoutes.jet> <report.json>');
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  const cache = new Map();
  const report = classifyNativeInventory(readFileSync(inventory, 'utf8'), readFileSync(routes, 'utf8'), (path) => {
    if (!cache.has(path)) cache.set(path, existsSync(`${root}/${path}`) ? readFileSync(`${root}/${path}`, 'utf8') : null);
    return cache.get(path);
  });
  writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report.counts));
}
