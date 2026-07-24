import { test } from 'node:test';
import assert from 'node:assert/strict';
import { boardEpochs, cardMatches, sortCards, workflowRank, ownerVerifyQueue, openAcceptanceBallot } from '../app/ui/board-state.js';

const card = (num, lane, phase = lane, extra = {}) => ({
  num, title: `Card ${num}`, phase, priority: 'P1', lane: { lane, label: lane }, ...extra,
});

test('workflow order is verify, ready, build/plan, blocked, closed', () => {
  const cards = [
    card(5, 'done'),
    card(4, 'blocked', 'planning'),
    card(3, 'plan', 'planning'),
    card(2, 'implement', 'ready'),
    card(1, 'verify'),
    card(6, 'building'),
  ];
  assert.deepEqual(sortCards(cards).map(c => c.num), [1, 2, 3, 6, 4, 5]);
  assert.deepEqual(cards.map(workflowRank), [4, 3, 2, 1, 0, 2]);
});

test('closed cards are opt-in and filters compose', () => {
  const done = card(9, 'done', 'done', { title: 'Closed parser', priority: 'P0' });
  assert.equal(cardMatches(done), false);
  assert.equal(cardMatches(done, { showClosed: true, text: 'parser', priority: 'P0' }), true);
  assert.equal(cardMatches(done, { showClosed: true, workflow: '0' }), false);

  const ready = card(10, 'implement', 'ready', { title: 'Ready parser', priority: 'P0' });
  assert.equal(cardMatches(ready, { workflow: '1', priority: 'P0', text: '#10' }), true);

  const frozen = card(11, 'frozen', 'frozen');
  assert.equal(workflowRank(frozen), 5);
  assert.equal(cardMatches(frozen, { workflow: '3' }), false);
});

test('explicit sorts keep stable work-order, priority, and number ties', () => {
  const cards = [
    card(3, 'plan', 'planning', { workOrder: 2, priority: 'P2' }),
    card(2, 'verify', 'verify', { workOrder: 1, priority: 'P1' }),
    card(1, 'blocked', 'planning', { workOrder: 1, priority: 'P0' }),
  ];
  assert.deepEqual(sortCards(cards, { col: 'workOrder' }).map(c => c.num), [1, 2, 3]);
  assert.deepEqual(sortCards(cards, { col: 'priority', dir: 'desc' }).map(c => c.num), [3, 2, 1]);
  assert.deepEqual(sortCards([
    card(4, 'plan', 'planning', { priority: 'Urgent' }),
    card(5, 'plan', 'planning', { priority: 'Later' }),
  ], { col: 'priority' }, ['Later', 'Urgent']).map(c => c.num), [5, 4]);
});

test('show closed adds finished epochs that the active radar omits', () => {
  const radar = [{ id: 'e1', name: 'Active' }];
  const epochs = [{ id: 'e1', name: 'Active' }, { id: 'e2', name: 'Finished', goal: 'shipped' }];
  const cards = [card(20, 'done', 'done', { epoch: 'e2', track: 'epoch' })];
  assert.deepEqual(boardEpochs(radar, epochs, cards, [], false), radar);
  assert.deepEqual(boardEpochs(radar, epochs, cards, [], true).map(e => e.id), ['e1', 'e2']);
  assert.deepEqual(boardEpochs(radar, epochs, cards, [], true)[1], {
    id: 'e2', name: 'Finished', goal: 'shipped', active: 0, done: 1, pct: 100, burndown: [], milestones: [],
  });
});

test('ownerVerifyQueue: ONLY needsAcceptance verify cards — bare verify is agent work', () => {
  const bare = card(710, 'verify', 'verify', { needsAcceptance: false });
  const visual = card(360, 'verify', 'verify', {
    needsAcceptance: true,
    decisions: [{ id: 'D-ACCEPT-360', status: 'open' }],
  });
  const building = card(1, 'building', 'building', { needsAcceptance: true });
  const q = ownerVerifyQueue([bare, visual, building]);
  assert.equal(q.length, 1);
  assert.equal(q[0].card.num, 360);
  assert.equal(openAcceptanceBallot(q[0].card)?.id, 'D-ACCEPT-360');
  assert.equal(ownerVerifyQueue([bare]).length, 0);
});
