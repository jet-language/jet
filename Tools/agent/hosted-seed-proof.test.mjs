import test from 'node:test';
import assert from 'node:assert/strict';
import {hostedSeedCensus} from './hosted-seed-proof.mjs';

test('hosted seed accepts only a real successful build without error diagnostics', () => {
    const proof = hostedSeedCensus(0, 'warning: unused import\nFinished release profile\n');
    assert.equal(proof.buildRc, 0);
    assert.deepEqual(proof.errors, []);
    assert.equal(proof.keepGoingTsvRequired, false);
    assert.match(proof.boundary, /Rust-hosted/);
    assert.throws(() => hostedSeedCensus(101, ''), /build rc=101/);
});

test('a nonempty seed error, ICE or panic log fails even with exit zero', () => {
    for (const line of ['Error [E0121]: consumed value', 'error[E0308]: mismatched types', 'error: linking failed', 'internal compiler error: missing target', 'ICE: invalid MIR', "thread 'main' panicked at Codegen.rs:455", 'panicked at a failed assertion', 'panic: invalid compiler state']) {
        assert.throws(() => hostedSeedCensus(0, `warning: metadata\n${line}\n`), /error\/ICE\/panic rows=1/);
    }
});
