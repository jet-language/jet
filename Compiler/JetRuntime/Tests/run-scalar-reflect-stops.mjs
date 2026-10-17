#!/usr/bin/env node
// Run only against the real composed Jet object linked client. Rust hook's
// host/source/backtrace prefix is not Jet semantics (jet_rt_unreachable rule).
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
const [output, binary] = process.argv.slice(2);
if (!output || !binary || process.argv.length !== 4) {
    throw Error('usage: run-scalar-reflect-stops.mjs OUT SCALAR_REFLECT_WITNESS');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const rows = [];
for (const [mode, index] of [['bounds-equal', '0'], ['bounds-negative', '18446744073709551615']]) {
    const result = spawnSync(resolve(binary), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
    writeFileSync(join(out, `${mode}.stdout`), result.stdout ?? Buffer.alloc(0));
    writeFileSync(join(out, `${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal !== null || result.status !== 101) {
        throw Error(`${mode}: expected fatal exit 101, got ${result.status}/${result.signal}/${result.error?.message ?? ''}`);
    }
    if (!result.stdout.equals(Buffer.from('flushed'))) throw Error(`${mode}: pending runtime stdout was not flushed`);
    const golden = Buffer.from(`index out of bounds: the len is 0 but the index is ${index}\n`);
    if (!result.stderr.equals(golden)) throw Error(`${mode}: stderr differs from exact foreign-panic body`);
    rows.push(`${mode}\tpass\texit=101\tstdout-flushed=true\tstderr-byte-identical=true`);
}
writeFileSync(join(out, 'scalar-reflect-stops.tsv'), rows.join('\n') + '\n');
console.log('ScalarReflect bounds: exit 101, exact unsigned-index body and stdout flushed');
