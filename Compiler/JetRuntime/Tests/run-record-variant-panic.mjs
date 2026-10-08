#!/usr/bin/env node
// Execute real C clients linked to the Jet object; the direct jet_panic call
// is the process-mode golden oracle, not a replacement runtime provider.
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
const [output, tree, event] = process.argv.slice(2);
if (!output || !tree || !event || process.argv.length !== 5) {
    throw Error('usage: run-record-variant-panic.mjs OUT DATA_TREE_WITNESS DATA_EVENT_WITNESS');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const rows = [];
for (const [name, binary] of [['tree', tree], ['event', event]]) {
    const results = new Map();
    for (const mode of ['panic-reference', 'wrong-variant']) {
        const result = spawnSync(resolve(binary), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
        writeFileSync(join(out, `${name}.${mode}.stdout`), result.stdout ?? Buffer.alloc(0));
        writeFileSync(join(out, `${name}.${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
        writeFileSync(join(out, `${name}.${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
        if (result.error || result.signal !== null || result.status !== 70 || result.stdout.length !== 0) {
            throw Error(`${name}/${mode}: expected exit 70, no signal/error/stdout; got ${result.status}/${result.signal}/${result.error?.message ?? ''}`);
        }
        results.set(mode, result.stderr);
    }
    const golden = results.get('panic-reference');
    if (!golden.includes(Buffer.from('record variant index out of range')) || !golden.includes(Buffer.from('<core.prelude>:0'))) {
        throw Error(`${name}: jet_panic reference lost message/location`);
    }
    if (!results.get('wrong-variant').equals(golden)) throw Error(`${name}: wrong-variant stderr differs from jet_panic golden`);
    rows.push(`${name}\tpass\texit=70\tstderr-byte-identical=true`);
}
writeFileSync(join(out, 'record-variant-results.tsv'), rows.join('\n') + '\n');
console.log('DataTree/DataEvent wrong-variant: exit 70 and stderr byte-identical to jet_panic reference');
