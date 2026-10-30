import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { COMPILER_ALIASES, compareSuites, jetpackKeys, linkCompilerAliases, mergeTreeConflicts, nextReleaseName, parseArgs, parseChainResult, parseDispositions, seedAssetName } from './release-compiler.mjs';

const row = (id, tier, status, suite = 'ui') => ({ suite, case: id, tier, status, detail: '' });

test('the next release name follows the highest a0_NNN', () => {
    assert.equal(nextReleaseName(['a0_005', 'a0_020', 'round-d', 'current', '.a0_021.assembling-7']), 'a0_021');
    assert.equal(nextReleaseName([]), 'a0_001');
});

test('the seed asset names the release and the host it runs on', () => {
    assert.equal(seedAssetName('a0_022', 'linux', 'x64'), 'jet-a0_022-linux-x64.tar.gz');
});

test('the release ships jet-cc, jet-cxx and jet-c++ as hard links of jet', () => {
    const dir = mkdtempSync(join(tmpdir(), 'release-aliases-'));
    try {
        writeFileSync(join(dir, 'jet'), 'binary');
        linkCompilerAliases(dir);
        assert.deepEqual(COMPILER_ALIASES, ['jet-cc', 'jet-cxx', 'jet-c++']);
        const jet = statSync(join(dir, 'jet'));
        for (const alias of COMPILER_ALIASES) assert.equal(statSync(join(dir, alias)).ino, jet.ino, alias);
        assert.equal(jet.nlink, 4);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});

test('options collect every problem at once', () => {
    const options = parseArgs(['--shards', 'x', '--bogus']);
    assert.deepEqual(options.errors, ['--shards needs a whole number, not x', 'unknown option --bogus', 'COMMIT is required']);
    assert.equal(parseArgs(['abc', '--min-gains', '0']).minGains, 0);
});

test('a lost row is one the release passes and the candidate fails on both runs', () => {
    const release = [row('a.jet', 'o0', 'PASS'), row('b.jet', 'o0', 'PASS'), row('c.jet', 'o0', 'FAIL'), row('d.jet', 'o1', 'UNEXPECTED-PASS')];
    const candidate = [row('a.jet', 'o0', 'PASS'), row('b.jet', 'o0', 'FAIL'), row('c.jet', 'o0', 'PASS'), row('e.jet', 'o0', 'PASS')];
    const result = compareSuites(release, candidate);
    assert.equal(result.rows, 4);
    assert.deepEqual(result.lost.map(r => [r.case, r.now]), [['b.jet', 'FAIL'], ['d.jet', 'MISSING']]);
    assert.deepEqual(result.gained.map(r => [r.case, r.was]), [['c.jet', 'FAIL']]);
    assert.deepEqual(result.added.map(r => r.case), ['e.jet']);
    const retried = compareSuites(release, candidate, [row('b.jet', 'o0', 'PASS')]);
    assert.deepEqual(retried.lost.map(r => r.case), ['d.jet']);
    assert.deepEqual(retried.flaky.map(r => r.case), ['b.jet']);
});

test('a disposition names a card and matches a tier or every tier', () => {
    const parsed = parseDispositions('# comment\nui\tb.jet\t*\t#6001\tthe fixture tests a removed form\ncases\tx\to0\t6002\tno hash\n');
    assert.equal(parsed.rows.length, 1);
    assert.equal(parsed.errors.length, 1);
    const result = compareSuites([row('b.jet', 'o2', 'PASS')], [row('b.jet', 'o2', 'FAIL')], [], parsed.rows);
    assert.equal(result.lost[0].disposition.card, '#6001');
});

test('Jetpack census keys drop line numbers and stop at the per-file summary', () => {
    const text = 'Jetpack/A.jet:10:2: error[E1]: bad\nJetpack/A.jet:99:1: error[E1]: bad\nJetpack/B.jet:3:4: error[E2]: worse\nerrors per file:\nJetpack/C.jet:1:1: x: y\n';
    assert.deepEqual(jetpackKeys(text), ['Jetpack/A.jet\terror[E1]', 'Jetpack/B.jet\terror[E2]']);
});

test('chain RESULT rows and merge-tree conflicts parse', () => {
    const rows = parseChainResult('stage\tstatus\trc\tnote\ns2\tBUILT\t0\tstored\nverdict\tFIXED_POINT\t0\tlevel=full\n');
    assert.equal(rows.get('verdict').status, 'FIXED_POINT');
    assert.deepEqual(mergeTreeConflicts('9dca0ae\nDocs/a.md\n\nAuto-merging Docs/a.md\nCONFLICT (add/add): Docs/a.md\n'), ['Docs/a.md']);
});
