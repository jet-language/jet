#!/usr/bin/env node
// Real native-composed client only; shared fixtures intentionally reused.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const [output, binary] = process.argv.slice(2);
if (!output || !binary || process.argv.length !== 4) throw Error('usage: run-ui-slice3-witness.mjs OUT UI_SLICE3_WITNESS');
const here = dirname(fileURLToPath(import.meta.url)), out = resolve(output);
mkdirSync(out, { recursive: true });
const rows = [];
for (const [mode, status, golden] of [
    ['', 0, 'UiCarriersSlice3Witness.stdout'],
    ['sort-tag', 70, 'DataPlotStops-ctor-range.stderr'],
    ['negative-capacity', 101, 'UiCarriersSlice2Stops-capacity.stderr'],
    ['oversize-capacity', 101, 'UiCarriersSlice2Stops-capacity.stderr'],
    ['index-zero', 101, 'UiCarriersSlice2Stops-index-zero.stderr'],
    ['index-negative', 101, 'UiCarriersSlice2Stops-index-negative.stderr'],
]) {
    const name = mode || 'ownership';
    const result = spawnSync(resolve(binary), mode ? [mode] : [], { timeout: 30000, maxBuffer: 1 << 20 });
    writeFileSync(join(out, `${name}.stdout`), result.stdout ?? Buffer.alloc(0));
    writeFileSync(join(out, `${name}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `${name}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal !== null || result.status !== status) throw Error(`${name}: expected exit ${status}, got ${result.status}/${result.signal}/${result.error?.message ?? ''}`);
    const expected = readFileSync(join(here, golden));
    if (!result.stdout.equals(mode ? Buffer.alloc(0) : expected) || !result.stderr.equals(mode ? expected : Buffer.alloc(0))) throw Error(`${name}: byte-exact stdout/stderr mismatch`);
    rows.push(`${name}\tpass\texit=${status}\tstdout-byte-identical=true\tstderr-byte-identical=true`);
}
writeFileSync(join(out, 'ui-slice3.tsv'), rows.join('\n') + '\n');
console.log('UI slice3: ownership, range stop and four shared-sequence stops byte-identical');
