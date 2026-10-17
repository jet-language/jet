#!/usr/bin/env node
// Real Dyn C client linked to composed Jet objects, including c39d89b6b's
// shared index helper. These files are canonical expected bytes, not receipts.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const [output, binary] = process.argv.slice(2);
if (!output || !binary || process.argv.length !== 4) throw Error('usage: run-data-sequence-stops.mjs OUT DATA_TREE_WITNESS');
const out = resolve(output), tests = dirname(fileURLToPath(import.meta.url));
mkdirSync(out, { recursive: true });
const cases = [
    ['list-oob', 'DataSequenceIndexZero.stderr'], ['entries-oob', 'DataSequenceIndexZero.stderr'],
    ['list-negative', 'DataSequenceIndexNegative.stderr'], ['entries-negative', 'DataSequenceIndexNegative.stderr'],
    ['list-capacity-negative', 'DataSequenceCapacity.stderr'], ['list-capacity-overflow', 'DataSequenceCapacity.stderr'],
    ['entries-capacity-negative', 'DataSequenceCapacity.stderr'], ['entries-capacity-overflow', 'DataSequenceCapacity.stderr'],
];
const rows = [];
for (const [mode, golden] of cases) {
    const result = spawnSync(resolve(binary), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
    writeFileSync(join(out, `${mode}.stdout`), result.stdout ?? Buffer.alloc(0));
    writeFileSync(join(out, `${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal !== null || result.status !== 101 || result.stdout.length !== 0 || !result.stderr.equals(readFileSync(join(tests, golden)))) {
        throw Error(`${mode}: expected exit 101, empty stdout and exact ${golden}; got ${result.status}/${result.signal}/${result.error?.message ?? ''}`);
    }
    rows.push(`${mode}\tpass\texit=101\tstderr-byte-identical=true`);
}
writeFileSync(join(out, 'data-sequence-results.tsv'), rows.join('\n') + '\n');
console.log('8 Dyn List/Entries std Vec bounds/capacity process goldens passed');
