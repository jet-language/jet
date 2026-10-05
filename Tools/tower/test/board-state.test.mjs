import { test } from 'node:test';
import assert from 'node:assert/strict';
import { boardEpochs, cardMatches, sortCards, workflowRank, ownerVerifyQueue, openAcceptanceBallot, acceptanceReadiness, cardDeletePlan, cardDeletePayload } from '../app/ui/board-state.js';

const card = (num, lane, phase = lane, extra = {}) => ({
  num, title: `Card ${num}`, phase, priority: 'P1', lane: { lane, label: lane }, ...extra,
});

test('workflow order is verify, building, ready, plan, blocked, closed', () => {
  const cards = [
    card(5, 'done'),
    card(4, 'blocked', 'planning'),
    card(3, 'plan', 'planning'),
    card(2, 'implement', 'ready'),
    card(1, 'verify', 'verify', { workOrder: 99 }),
    card(6, 'building', 'building', { workOrder: 1 }),
  ];
  assert.deepEqual(sortCards(cards).map(c => c.num), [1, 6, 2, 3, 4, 5]);
  assert.deepEqual(cards.map(workflowRank), [4, 3, 2, 1, 0, 0]);
});

test('closed cards are opt-in and filters compose', () => {
  const done = card(9, 'done', 'done', { title: 'Closed parser', priority: 'P0' });
  assert.equal(cardMatches(done), false);
  assert.equal(cardMatches(done, { showClosed: true, text: 'parser', priority: 'P0' }), true);
  assert.equal(cardMatches(done, { showClosed: true, workflow: '0' }), false);

  const ready = card(10, 'implement', 'ready', { title: 'Ready parser', priority: 'P0' });
  assert.equal(cardMatches(ready, { workflow: '1', priority: 'P0', text: '#10' }), true);

  const building = card(12, 'building', 'building');
  assert.equal(cardMatches(building, { workflow: '0' }), true);

  const frozen = card(11, 'frozen', 'frozen');
  assert.equal(workflowRank(frozen), 5);
  assert.equal(cardMatches(frozen, { workflow: '3' }), false);
});

test('kind filter narrows the board to one card kind and composes with others', () => {
  const feature = card(30, 'implement', 'ready', { kind: 'feature', priority: 'P0' });
  const bug = card(31, 'implement', 'ready', { kind: 'bug', priority: 'P0' });
  assert.equal(cardMatches(feature, { kind: 'feature' }), true);
  assert.equal(cardMatches(bug, { kind: 'feature' }), false);
  assert.equal(cardMatches(bug, { kind: 'all' }), true);
  assert.equal(cardMatches(feature, { kind: 'feature', priority: 'P1' }), false);
  assert.equal(cardMatches(feature, { kind: 'feature', workflow: '1', priority: 'P0' }), true);
});

test('explicit sorts keep stable work-order, priority, and number ties', () => {
  const cards = [
    card(3, 'plan', 'planning', { workOrder: 2, priority: 'P2' }),
    card(2, 'verify', 'verify', { workOrder: 1, priority: 'P1' }),
    card(1, 'blocked', 'planning', { workOrder: 1, priority: 'P0' }),
  ];
  assert.deepEqual(sortCards(cards, { col: 'workOrder' }).map(c => c.num), [1, 2, 3]);
  assert.deepEqual(sortCards([
    card(4, 'verify', 'verify', { workOrder: 2 }),
    card(5, 'building', 'building', { workOrder: 1 }),
  ], { col: 'workOrder' }).map(c => c.num), [5, 4], 'an explicit sort overrides workflow order');
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
  const milestones = [{ id: 'm1', epochId: 'e2', title: 'Shipped', status: 'met', progress: { total: 1, done: 1, reviewReady: true, met: true } }];
  assert.deepEqual(boardEpochs(radar, epochs, cards, milestones, false), radar);
  assert.deepEqual(boardEpochs(radar, epochs, cards, milestones, true).map(e => e.id), ['e1', 'e2']);
  assert.deepEqual(boardEpochs(radar, epochs, cards, milestones, true)[1], {
    id: 'e2', name: 'Finished', goal: 'shipped', active: 0, done: 1,
    milestoneTotal: 1, milestonesMet: 1, pct: 100, burndown: [],
    milestones: [{
      id: 'm1', epochId: 'e2', title: 'Shipped', status: 'met',
      progress: { total: 1, done: 1, reviewReady: true, met: true },
      total: 1, done: 1, reviewReady: true, met: true, stalledDays: null,
    }],
  });
});

// Owner 2026-10-04: frozen cards live in their epoch section, shown by
// default; the Frozen filter narrows the board to them alone.
test('frozen cards show by default and the Frozen filter shows only them', () => {
  const frozen = card(30, 'frozen', 'frozen', { title: 'jetos disks', priority: 'P2', kind: 'feature' });
  const ready = card(31, 'implement', 'ready', { title: 'jetos boot' });
  const done = card(32, 'done', 'done');
  assert.equal(cardMatches(frozen), true);
  assert.equal(cardMatches(frozen, { workflow: 'frozen' }), true);
  assert.equal(cardMatches(ready, { workflow: 'frozen' }), false);
  assert.equal(cardMatches(done, { workflow: 'frozen', showClosed: true }), false);
  assert.equal(cardMatches(frozen, { workflow: 'frozen', text: 'disks', priority: 'P2', kind: 'feature' }), true);
  assert.equal(cardMatches(frozen, { workflow: 'frozen', priority: 'P0' }), false);
  for (const workflow of ['0', '1', '2', '3']) assert.equal(cardMatches(frozen, { workflow }), false);
});

test('an epoch the active radar omits still gets a section for its frozen cards', () => {
  const radar = [{ id: 'e9', name: 'jetos' }];
  const epochs = [{ id: 'e2', name: 'Arrived' }, { id: 'e9', name: 'jetos' }, { id: 'e3', name: 'Empty' }];
  const cards = [
    card(40, 'frozen', 'frozen', { epoch: 'e2', track: 'epoch' }),
    card(41, 'frozen', 'frozen', { epoch: 'e3', track: 'sidequest' }),
  ];
  const shown = boardEpochs(radar, epochs, cards, [], false);
  assert.deepEqual(shown.map(e => e.id), ['e9', 'e2']);
  assert.equal(shown[1].active, 0);
  assert.equal(shown[1].done, 0);
  assert.equal(boardEpochs(radar, epochs, [cards[1]], [], false), radar, 'sidequest frozen cards stay in the sidequest section');
});

test('card delete payload lists every ballot and needs a tick per ratified one', () => {
  const c = { id: 'c7', num: 7, decisions: [
    { id: 'D-OPEN', title: 'open one', status: 'open' },
    { id: 'D-RAT', title: 'ratified one', status: 'ratified', outcome: 'B' },
  ] };
  assert.deepEqual(cardDeletePlan(c), {
    ballots: [
      { id: 'D-OPEN', title: 'open one', status: 'open', outcome: null, ratified: false },
      { id: 'D-RAT', title: 'ratified one', status: 'ratified', outcome: 'B', ratified: true },
    ],
    ratified: ['D-RAT'],
  });
  assert.equal(cardDeletePayload(c), null);
  assert.equal(cardDeletePayload(c, ['D-OPEN']), null);
  assert.deepEqual(cardDeletePayload(c, ['D-RAT']),
    { id: 'c7', by: 'owner', decisions: ['D-OPEN', 'D-RAT'], deleteRatified: ['D-RAT'] });
  assert.deepEqual(cardDeletePayload({ id: 'c8', decisions: [] }),
    { id: 'c8', by: 'owner', decisions: [], deleteRatified: [] });
});

const capture = [{ kind: 'image', path: 'docs/proposals/visual-acceptance/media/360/screen.png', alt: 'Settings screen', caption: 'after' }];

test('ownerVerifyQueue: ONLY needsAcceptance verify cards — bare verify is agent work', () => {
  const bare = card(710, 'verify', 'verify', { needsAcceptance: false });
  const visual = card(360, 'verify', 'verify', {
    needsAcceptance: true,
    decisions: [{ id: 'D-ACCEPT-360', status: 'open', visualMedia: capture }],
  });
  const building = card(1, 'building', 'building', { needsAcceptance: true });
  const q = ownerVerifyQueue([bare, visual, building]);
  assert.equal(q.length, 1);
  assert.equal(q[0].card.num, 360);
  assert.equal(openAcceptanceBallot(q[0].card)?.id, 'D-ACCEPT-360');
  assert.equal(ownerVerifyQueue([bare]).length, 0);
});

// Owner 2026-10-04: a visual check waiting for a screen capture showed no
// Accept button. Cards the owner cannot act on stay agent work.
test('ownerVerifyQueue: a capture-pending or agent-pending visual card is not an owner duty', () => {
  const noCapture = card(361, 'verify', 'verify', {
    needsAcceptance: true,
    decisions: [{ id: 'D-ACCEPT-361', status: 'open' }],
  });
  const openCriteria = card(362, 'verify', 'verify', {
    needsAcceptance: true,
    criteria: [{ text: 'golden passes', status: 'open' }],
    decisions: [{ id: 'D-ACCEPT-362', status: 'open', visualMedia: capture }],
  });
  const noBallot = card(363, 'verify', 'verify', { needsAcceptance: true, visualMedia: capture });
  assert.equal(ownerVerifyQueue([noCapture, openCriteria, noBallot]).length, 0);
  assert.deepEqual(acceptanceReadiness(noCapture, openAcceptanceBallot(noCapture)), { waitingOnAgent: false, capturePending: true, ready: false });
  assert.deepEqual(acceptanceReadiness(openCriteria, openAcceptanceBallot(openCriteria)), { waitingOnAgent: true, capturePending: false, ready: false });
  assert.equal(acceptanceReadiness(noBallot, null).waitingOnAgent, true);
});

test('milestone filter narrows to one milestone and sorts unassigned last', () => {
  const inMile = card(20, 'building', 'building', { milestoneId: 'e3-ul4' });
  const other = card(21, 'building', 'building', { milestoneId: 'e3-ul7' });
  const none = card(22, 'building', 'building');

  assert.equal(cardMatches(inMile, { milestone: 'e3-ul4' }), true);
  assert.equal(cardMatches(other, { milestone: 'e3-ul4' }), false);
  assert.equal(cardMatches(none, { milestone: 'e3-ul4' }), false);
  assert.equal(cardMatches(none, {}), true);

  const closed = card(23, 'done', 'done', { milestoneId: 'e3-ul4' });
  assert.equal(cardMatches(closed, { milestone: 'e3-ul4' }), false);
  assert.equal(cardMatches(closed, { milestone: 'e3-ul4', showClosed: true }), true);

  // A frozen card still counts toward its milestone's total, so the drill-down
  // must show it — otherwise the milestone points at a card nobody can see.
  const parked = card(24, 'frozen', 'frozen', { milestoneId: 'e3-ul4' });
  assert.equal(cardMatches(parked, { milestone: 'e3-ul4' }), true);

  const sorted = sortCards([none, other, inMile], { col: 'milestone', dir: 'asc' });
  assert.deepEqual(sorted.map(c => c.num), [20, 21, 22]);
});

test('card number search matches exact ids, including a couple at once', () => {
  const a = card(12, 'building');
  const b = card(120, 'building');
  const titled = card(45, 'building', 'building', { title: 'Fix 12' });
  assert.equal(cardMatches(a, { text: '12' }), true);
  assert.equal(cardMatches(b, { text: '12' }), false);
  assert.equal(cardMatches(titled, { text: '12' }), false);
  assert.equal(cardMatches(a, { text: '#12' }), true);
  assert.equal(cardMatches(a, { text: '#12 #45' }), true);
  assert.equal(cardMatches(titled, { text: '12, 45' }), true);
  assert.equal(cardMatches(b, { text: '#12, #45' }), false);
  assert.equal(cardMatches(titled, { text: 'Fix' }), true);
});

test('sort by created uses created timestamps', () => {
  const older = card(1, 'building', 'building', { created: '2026-01-01T00:00:00.000Z' });
  const newer = card(2, 'building', 'building', { created: '2026-08-01T00:00:00.000Z' });
  assert.deepEqual(sortCards([newer, older], { col: 'created', dir: 'asc' }).map(c => c.num), [1, 2]);
  assert.deepEqual(sortCards([older, newer], { col: 'created', dir: 'desc' }).map(c => c.num), [2, 1]);
});
