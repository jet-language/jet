#!/usr/bin/env node
// Execute only a caller linked to the fresh composed object, not a mock provider.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
const [binary] = process.argv.slice(2);
if (!binary || process.argv.length !== 3) throw Error('usage: run-io-witness.mjs FRESH_COMPOSED_IOWITNESS');
const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
function run(mode) {
    const result = spawnSync(resolve(binary), mode ? [mode] : [], { encoding: 'utf8', timeout: 30000 });
    assert.equal(result.error, undefined);
    assert.equal(result.signal, null);
    return result;
}
const normal = run();
assert.equal(normal.status, 0);
assert.equal(normal.stderr, '');
assert.equal(normal.stdout, readFileSync(resolve(here, 'IOWitness.stdout'), 'utf8'));
// Reuse canonical registry footer goldens, while pinning this family's header
// and source location. No host Rust type rendering is involved in the oracle.
const panicFooter = readFileSync(resolve(repo, 'Examples/features/expected/collections/step_by_zero.err.out'), 'utf8').split('\n').slice(2).join('\n');
const numericFooter = readFileSync(resolve(repo, 'Examples/features/expected/collections/list_bounds.err.out'), 'utf8').split('\n').slice(2).join('\n');
const recordStop = 'Stop [E3001]: `panic: record variant index out of range`\n  --> <core.prelude>:0\n' + panicFooter;
const numericStop = 'Stop [E3010]: `native Int argument exceeds host range`\n  --> <core.prelude>:0\n' + numericFooter;
for (const mode of ['bad-op-low', 'bad-op-high', 'bad-limit-low', 'bad-limit-high', 'wrong-limit', 'wrong-context', 'limit-as-context', 'code-overflow', 'code-underflow']) {
    const stopped = run(mode);
    assert.equal(stopped.status, 70, mode);
    assert.equal(stopped.stdout, '', mode);
    assert.equal(stopped.stderr, mode.startsWith('code-') ? numericStop : recordStop, mode);
}
console.log('IO37 real-object witness: normal + 9 exact fatal modes OK');
