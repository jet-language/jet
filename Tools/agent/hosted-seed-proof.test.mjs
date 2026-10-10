import test from 'node:test';
import assert from 'node:assert/strict';
import {hostedSeedCensus, hostedLowerSweepCensus, hostedOptimizationPassCensus, hostedEmissionCensus} from './hosted-seed-proof.mjs';

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

test('optimizer census retains incidental probes without rejecting a clean main pipeline', () => {
    const proof = hostedOptimizationPassCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 1 functions failed to lower',
        'JET_DEBUG_LOWER_ALL optimization: operation=mir.optimize pass=probe violation=ProbeFailure',
        'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=probe violations=1 output=discarded',
        'JET_DEBUG_LOWER_ALL summary: 0 of 28532 functions failed to lower',
        'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=cfg-simplification violations=0 output=accepted',
        'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=effect-ordering violations=0 output=accepted',
    ].join('\n'));
    assert.equal(proof.clean, true);
    assert.equal(proof.summaries.length, 3);
    assert.equal(proof.violations.length, 1);
    assert.equal(proof.main_summaries.length, 2);
    assert.deepEqual(proof.main_violations, []);
});

test('optimizer rollback never hides multiple rejected main passes or their full identities', () => {
    const first = 'Validation(InvalidReferenceIndex { function: MirFunctionId(13129819222604582527), value: MirValueId(12024045181009992773), span: Span { start: 21839604, end: 21839608 } })';
    const second = 'Validation(MovedPlace { function: MirFunctionId(22), place: MirPlaceId(33), span: Span { start: 44, end: 55 } })';
    const proof = hostedOptimizationPassCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 28532 functions failed to lower',
        `JET_DEBUG_LOWER_ALL optimization: operation=mir.optimize pass=cfg-simplification violation=${first}`,
        'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=cfg-simplification violations=1 output=discarded',
        `JET_DEBUG_LOWER_ALL optimization: operation=mir.optimize pass=borrow-insertion violation=${second}`,
        'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=borrow-insertion violations=1 output=discarded',
        'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=effect-ordering violations=0 output=accepted',
    ].join('\n'));
    assert.equal(proof.clean, false);
    assert.equal(proof.main_summaries.length, 3);
    assert.deepEqual(proof.main_violations.map(row => row.violation), [first, second]);
});

test('optimizer gate requires main boundary evidence and rejects inconsistent accepted output', () => {
    const main = 'JET_DEBUG_LOWER_ALL summary: 0 of 10 functions failed to lower\n';
    const accepted = 'JET_DEBUG_LOWER_ALL optimization summary: operation=mir.optimize pass=cfg-simplification violations=0 output=accepted\n';
    for (const log of ['', accepted, main, main + accepted.replace('violations=0', 'violations=1'),
        main + 'JET_DEBUG_LOWER_ALL optimization: operation=mir.optimize pass=cfg-simplification violation=InvalidReference\n' + accepted]) {
        assert.equal(hostedOptimizationPassCensus(log).clean, false);
    }
});

test('emitter census uses its actual item count and separates comptime probes', () => {
    const proof = hostedEmissionCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 1 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission: kind=function id=1 name="probe" span=0..1 message="probe failure"',
        'JET_DEBUG_LOWER_ALL emission failures 1 of 1',
        'JET_DEBUG_LOWER_ALL summary: 0 of 28533 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission failures 0 of 32007',
    ].join('\n'));
    assert.equal(proof.clean, true);
    assert.equal(proof.final_summary.total, 32007);
    assert.equal(proof.failures.length, 1);
    assert.deepEqual(proof.main_failures, []);
});

test('emitter census preserves every failure and lossless u64 identities', () => {
    const proof = hostedEmissionCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 28533 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission: kind=impl id=10065691745087956836 name="JetCLINativeExecutor" span=21291822..21291987 message="trait \\"Err\\" differs"',
        'JET_DEBUG_LOWER_ALL emission: kind=function id=18446744073709551615 name="next" span=10..20 message="second\\nline"',
        'JET_DEBUG_LOWER_ALL emission failures 2 of 32007',
    ].join('\n'));
    assert.equal(proof.clean, false);
    assert.equal(proof.main_failures.length, 2);
    assert.equal(proof.main_failures[0].id, '10065691745087956836');
    assert.equal(proof.main_failures[1].id, '18446744073709551615');
    assert.equal(proof.main_failures[0].message, 'trait "Err" differs');
    assert.equal(proof.main_failures[1].message, 'second\nline');
});

test('emitter gate never accepts missing, empty, malformed or contradictory evidence', () => {
    const main = 'JET_DEBUG_LOWER_ALL summary: 0 of 28533 functions failed to lower\n';
    const zero = 'JET_DEBUG_LOWER_ALL emission failures 0 of 32007\n';
    for (const log of ['', zero, main, main + zero.replace('32007', '0'),
        main + 'JET_DEBUG_LOWER_ALL emission: truncated row\n' + zero,
        main + 'JET_DEBUG_LOWER_ALL emission: kind=function id=1 name="bad" span=0..1 message="failure"\n' + zero,
        main + 'JET_DEBUG_LOWER_ALL emission failures 1 of 32007\n' + zero]) {
        assert.equal(hostedEmissionCensus(log).clean, false);
    }
});

test('Rust Debug-only escapes remain visible rather than inventing decoded diagnostics', () => {
    const proof = hostedEmissionCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 2 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission: kind=function id=2 name="symbol" span=1..2 message="control\\u{1b}"',
        'JET_DEBUG_LOWER_ALL emission failures 1 of 2',
    ].join('\n'));
    assert.equal(proof.clean, false);
    assert.equal(proof.main_failures[0].message, null);
    assert.equal(proof.main_failures[0].message_debug, '"control\\u{1b}"');
});

test('diagnostic partial Rust is preserved but never qualifies successful emission', () => {
    const proof = hostedEmissionCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 28533 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission: kind=impl id=3 name="Owner" span=1..2 message="failed owner"',
        'JET_DEBUG_LOWER_ALL emission failures 1 of 32007',
        'JET_DEBUG_LOWER_ALL emission partial Rust: path="/tmp/partial \\"name\\".rs" failures=1 items=32007',
    ].join('\n'));
    assert.equal(proof.clean, false);
    assert.equal(proof.partial_consistent_with_final, true);
    assert.equal(proof.latest_partial.path, '/tmp/partial "name".rs');
    assert.equal(proof.latest_partial.admitted, false);
    assert.equal(proof.latest_partial.role, 'diagnostic-partial');
});

test('partial announcements cannot override clean, missing or inconsistent emission evidence', () => {
    const main = 'JET_DEBUG_LOWER_ALL summary: 0 of 28533 functions failed to lower\n';
    for (const log of [
        main + 'JET_DEBUG_LOWER_ALL emission failures 0 of 10\nJET_DEBUG_LOWER_ALL emission partial Rust: path="/tmp/partial.rs" failures=1 items=10',
        main + 'JET_DEBUG_LOWER_ALL emission failures 1 of 10\nJET_DEBUG_LOWER_ALL emission partial Rust: path="/tmp/partial.rs" failures=2 items=10',
        main + 'JET_DEBUG_LOWER_ALL emission partial Rust: path="/tmp/partial.rs" failures=1 items=10',
        main + 'JET_DEBUG_LOWER_ALL emission failures 0 of 10\nJET_DEBUG_LOWER_ALL emission partial Rust: truncated',
    ]) {
        const proof = hostedEmissionCensus(log);
        assert.equal(proof.clean, false);
        assert.equal(proof.partial_consistent_with_final, false);
    }
});

test('partial comptime diagnostics do not masquerade as the main program dump', () => {
    const proof = hostedEmissionCensus([
        'JET_DEBUG_LOWER_ALL summary: 0 of 1 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission failures 1 of 1',
        'JET_DEBUG_LOWER_ALL emission partial Rust: path="/tmp/probe.rs" failures=1 items=1',
        'JET_DEBUG_LOWER_ALL summary: 0 of 28533 functions failed to lower',
        'JET_DEBUG_LOWER_ALL emission failures 0 of 32007',
    ].join('\n'));
    assert.equal(proof.clean, true);
    assert.equal(proof.partials.length, 1);
    assert.equal(proof.latest_partial, null);
});
