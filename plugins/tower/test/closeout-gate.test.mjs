import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nonTowerChanges, selectCloseout } from '../../../scripts/agent/closeout-gate.mjs';

const HEAD = 'a'.repeat(40);
const milestone = (id, extra = {}) => ({
  id,
  status: 'review-ready',
  progress: { reviewReady: true },
  closeout: { sourceCommit: HEAD },
  ...extra,
});

test('closeout gate ignores only Tower state writes', () => {
  const status = [
    ' M plugins/tower/.tower/tower.json',
    '?? plugins/tower/.tower/.tower.json.tmp-1',
    ' M Source/Main.rs',
    '?? tests/new_case.rs',
  ].join('\n');
  assert.deepEqual(nonTowerChanges(status), [' M Source/Main.rs', '?? tests/new_case.rs']);
});

test('closeout gate selects one review-ready token at the frozen commit', () => {
  const picked = selectCloseout([
    milestone('stale', { closeout: { sourceCommit: 'b'.repeat(40) } }),
    milestone('ready'),
  ], HEAD);
  assert.equal(picked.id, 'ready');
});

test('closeout gate rejects missing, ambiguous, and explicitly stale tokens', () => {
  assert.throws(() => selectCloseout([], HEAD), /no review-ready milestone/);
  assert.throws(() => selectCloseout([milestone('a'), milestone('b')], HEAD), /multiple closeout tokens/);
  assert.throws(
    () => selectCloseout([milestone('stale', { closeout: { sourceCommit: 'b'.repeat(40) } })], HEAD, 'stale'),
    /no valid closeout token/,
  );
});
