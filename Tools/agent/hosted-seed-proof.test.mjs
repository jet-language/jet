import test from 'node:test';
import assert from 'node:assert/strict';
import {hostedSeedCensus, hostedLowerSweepCensus} from './hosted-seed-proof.mjs';

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

test('hosted MIR gate uses the final actual-count summary, not a comptime probe', () => {
    const proof = hostedLowerSweepCensus('JET_DEBUG_LOWER_ALL summary: 1 of 1 functions failed to lower\nJET_DEBUG_LOWER_ALL summary: 0 of 28411 functions failed to lower\n');
    assert.equal(proof.failed, 0);
    assert.equal(proof.total, 28411);
    assert.deepEqual(proof.summaries, [{failed: 1, total: 1}, {failed: 0, total: 28411}]);
});

test('hosted MIR gate rejects missing, empty and failed final sweeps', () => {
    for (const log of ['', 'warning: unused import\n', 'JET_DEBUG_LOWER_ALL summary: 0 of 0 functions failed to lower\n',
        'JET_DEBUG_LOWER_ALL summary: 0 of 1 functions failed to lower\nJET_DEBUG_LOWER_ALL summary: 18 of 28409 functions failed to lower\n']) {
        assert.throws(() => hostedLowerSweepCensus(log), /hosted MIR sweep is not clean/);
    }
});
