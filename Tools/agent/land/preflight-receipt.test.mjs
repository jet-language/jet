import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { gate, receiptPath, refusal, writeReceipt } from './preflight-receipt.mjs';

const SOURCE = 'jet@0.0.1#aaaa';
const SEED = 'jet@0.0.1#bbbb';

test('a receipt admits only the source identity and seed its census ran', () => {
    const dir = mkdtempSync(join(tmpdir(), 'preflight-'));
    try {
        assert.match(gate(SOURCE, SEED, dir).reason, /^no preflight receipt for source identity jet@0\.0\.1#aaaa$/);
        writeReceipt({ sourceIdentity: SOURCE, seedIdentity: SEED, commit: '0123456789abcdef', files: ['Compiler/JetCli/run.jet'] }, dir);
        const admitted = gate(SOURCE, SEED, dir);
        assert.equal(admitted.admitted, true);
        assert.match(admitted.reason, /census of 0123456789 \+ 1 worktree file\(s\) by seed jet@0\.0\.1#bbbb/);
        const other = gate(SOURCE, 'jet@0.0.1#cccc', dir);
        assert.equal(other.admitted, false);
        assert.match(other.reason, /from a census by seed jet@0\.0\.1#bbbb, not jet@0\.0\.1#cccc/);
        assert.equal(gate('jet@0.0.1#dddd', SEED, dir).admitted, false);
        writeFileSync(receiptPath(SOURCE, dir), '{');
        assert.match(gate(SOURCE, SEED, dir).reason, /is not JSON/);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});

test('a refusal names the census-snap.sh command for the seed', () => {
    const text = refusal('chain.sh', 'no preflight receipt for source identity x', '/seeds/a0_029', '0123456789abcdef');
    assert.deepEqual(text.split('\n'), [
        'chain.sh: refused: no preflight receipt for source identity x.',
        'Run the collect-all preflight in the dev worktree at 0123456789 (name each uncommitted file the launch tree carries after it), then launch again:',
        '  CENSUS_JET_DIR=/seeds/a0_029 Tools/agent/land/census-snap.sh',
    ]);
});
