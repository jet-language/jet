import { test } from 'node:test';
import assert from 'node:assert/strict';
import { boardEpochs, cardMatches, sortCards, sortCriteria, parseSort, CARD_SORTS, CRITERIA_SORTS, closestCards, workflowRank, ownerVerifyQueue, openAcceptanceBallot, acceptanceReadiness, cardDeletePlan, cardDeletePayload, epochLabel, epochNumber, sortEpochs, refLinks } from '../App/UI/board-state.js';

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

test('done cards never show on the board and filters compose', () => {
  const done = card(9, 'done', 'done', { title: 'Closed parser', priority: 'P0' });
  assert.equal(cardMatches(done), false);
  assert.equal(cardMatches(done, { text: 'parser', priority: 'P0' }), false);
  assert.equal(cardMatches(done, { text: '#9' }), false);

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

test('a finished epoch holding only done cards gets no section', () => {
  const radar = [{ id: 'e1', name: 'Active' }];
  const epochs = [{ id: 'e1', name: 'Active' }, { id: 'e2', name: 'Finished', goal: 'shipped' }];
  const cards = [card(20, 'done', 'done', { epoch: 'e2', track: 'epoch' })];
  const milestones = [{ id: 'm1', epochId: 'e2', title: 'Shipped', status: 'met', progress: { total: 1, done: 1, reviewReady: true, met: true } }];
  assert.deepEqual(boardEpochs(radar, epochs, cards, milestones), radar);
});

test('card sorts: each key starts in its own direction, and :asc or :desc overrides it', () => {
  assert.deepEqual(CARD_SORTS.map(s => s.key), ['workflow', 'workOrder', 'priority', 'open', 'updated', 'created', 'num', 'title', 'milestone', 'lane']);
  for (const s of CARD_SORTS) assert.deepEqual(parseSort(s.key, CARD_SORTS), { col: s.key, dir: s.dir });
  assert.equal(parseSort('updated', CARD_SORTS).dir, 'desc');
  assert.deepEqual(parseSort('updated:asc', CARD_SORTS), { col: 'updated', dir: 'asc' });
  assert.equal(parseSort('size', CARD_SORTS), null);
  assert.equal(parseSort('num:sideways', CARD_SORTS), null);
  assert.equal(parseSort(undefined, CARD_SORTS), null);
});

test('sort by open criteria counts workable open rows only, fewest first', () => {
  const a = card(1, 'building', 'building', { criteria: [{ n: 1, status: 'open' }, { n: 2, status: 'open' }, { n: 3, status: 'met' }] });
  const b = card(2, 'building', 'building', { criteria: [{ n: 1, status: 'open' }, { n: 2, status: 'open', blockedBy: '#9' }] });
  const c = card(3, 'building', 'building', { crit: { open: 0, blocked: 0, settled: 1, total: 1 } });
  assert.deepEqual(sortCards([a, b, c], parseSort('open', CARD_SORTS)).map(x => x.num), [3, 2, 1]);
  assert.deepEqual(sortCards([a, b, c], parseSort('open:desc', CARD_SORTS)).map(x => x.num), [1, 2, 3]);
});

test('closest keeps nearest-to-done order by default and takes any card sort', () => {
  const epochs = [{ id: 'e1', order: 1 }];
  const a = card(1, 'building', 'building', { epoch: 'e1', priority: 'P2', updated: '2026-01-02', crit: { open: 2, blocked: 0, settled: 0, total: 2 } });
  const b = card(2, 'building', 'building', { epoch: 'e1', priority: 'P0', updated: '2026-01-01', crit: { open: 1, blocked: 0, settled: 0, total: 1 } });
  const done = card(3, 'done', 'done', { epoch: 'e1', crit: { open: 0, blocked: 0, settled: 1, total: 1 } });
  const nums = (opts) => closestCards([a, b, done], epochs, opts).map(r => r.card.num);
  assert.deepEqual(nums(), [2, 1]);
  assert.deepEqual(nums({ sort: parseSort('open:desc', CARD_SORTS) }), [1, 2]);
  assert.deepEqual(nums({ sort: parseSort('updated', CARD_SORTS) }), [1, 2]);
  assert.deepEqual(nums({ sort: parseSort('priority', CARD_SORTS), limit: 1 }), [2]);
  assert.equal(closestCards([a], epochs, { sort: parseSort('num', CARD_SORTS) })[0].crit.open, 2, 'rows keep their criteria counts');
});

test('criteria sort by number, status (open, blocked, met, verified) or text', () => {
  const items = [
    { n: 1, status: 'verified', text: 'b' },
    { n: 2, status: 'open', blockedBy: '#9', text: 'a' },
    { n: 3, status: 'met', text: 'c' },
    { n: 4, status: 'open', text: 'D' },
  ];
  assert.deepEqual(CRITERIA_SORTS.map(s => s.key), ['n', 'status', 'text']);
  assert.deepEqual(sortCriteria(items).map(i => i.n), [1, 2, 3, 4]);
  assert.deepEqual(sortCriteria(items, parseSort('n:desc', CRITERIA_SORTS)).map(i => i.n), [4, 3, 2, 1]);
  assert.deepEqual(sortCriteria(items, parseSort('status', CRITERIA_SORTS)).map(i => i.n), [4, 2, 3, 1]);
  assert.deepEqual(sortCriteria(items, parseSort('text', CRITERIA_SORTS)).map(i => i.n), [2, 1, 3, 4]);
  assert.deepEqual(items.map(i => i.n), [1, 2, 3, 4], 'sorting leaves the card\'s rows as they are');
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
  assert.equal(cardMatches(done, { workflow: 'frozen' }), false);
  assert.equal(cardMatches(frozen, { workflow: 'frozen', text: 'disks', priority: 'P2', kind: 'feature' }), true);
  assert.equal(cardMatches(frozen, { workflow: 'frozen', priority: 'P0' }), false);
  for (const workflow of ['0', '1', '2', '3']) assert.equal(cardMatches(frozen, { workflow }), false);
});

test('an epoch the active radar omits still gets a section for its frozen cards, in epoch order', () => {
  const radar = [{ id: 'e9', name: 'jetos' }];
  const epochs = [{ id: 'e2', name: 'Arrived' }, { id: 'e9', name: 'jetos' }, { id: 'e3', name: 'Empty' }];
  const cards = [
    card(40, 'frozen', 'frozen', { epoch: 'e2', track: 'epoch' }),
    card(41, 'frozen', 'frozen', { epoch: 'e3', track: 'sidequest' }),
  ];
  const shown = boardEpochs(radar, epochs, cards, []);
  assert.deepEqual(shown.map(e => e.id), ['e2', 'e9']);
  assert.equal(shown[0].active, 0);
  assert.equal(shown[0].done, 0);
  assert.equal(boardEpochs(radar, epochs, [cards[1]], []), radar, 'sidequest frozen cards stay in the sidequest section');
});

test('an epoch is labeled E plus its number, from num or else from the id', () => {
  assert.equal(epochLabel({ id: 'e18', name: 'Declarations as types' }), 'E18 Declarations as types');
  assert.equal(epochLabel({ id: 'e18', name: 'Declarations as types' }, { name: false }), 'E18');
  assert.equal(epochLabel({ id: 'x', num: 4, name: 'Metaprogramming' }), 'E4 Metaprogramming');
  assert.equal(epochLabel({ id: 'e7', name: 'e7' }), 'E7', 'a name equal to the id is not repeated');
  assert.equal(epochLabel({ id: 'later', name: 'Someday' }), 'later Someday', 'an id with no number shows as itself');
  assert.equal(epochNumber({ id: 'E21' }), 21);
  assert.equal(epochLabel({ id: 'G1', name: 'Metaprogramming' }), 'G1 Metaprogramming');
  assert.equal(epochLabel({ id: 'G9', name: 'JetOS' }, { name: false }), 'G9');
  assert.equal(epochNumber({ id: 'G9' }), 9);
  assert.equal(epochNumber({ id: 'side' }), null);
});

test('epochs sort by order, then the unordered ones by number, then by id', () => {
  const epochs = [{ id: 'e25' }, { id: 'e3', order: 9 }, { id: 'zeta' }, { id: 'e18' }, { id: 'e2', order: 1 }, { id: 'alpha' }];
  assert.deepEqual(sortEpochs(epochs).map(e => e.id), ['e2', 'e3', 'e18', 'e25', 'alpha', 'zeta']);
  assert.deepEqual(epochs.map(e => e.id)[0], 'e25', 'sorting copies; the input keeps its order');
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

test('refLinks turns #N and #N cM into card buttons and, with a commit URL, SHAs into commit links', () => {
  assert.equal(refLinks('see #12 and #3 c4.'),
    'see <button class="linkbtn" data-card-num="12">#12</button> and <button class="linkbtn" data-card-num="3" data-crit="4">#3 c4</button>.');
  // Without a commit URL a SHA stays text; with one it links, `{sha}` replaced.
  assert.equal(refLinks('landed 4f9c2ab1'), 'landed 4f9c2ab1');
  assert.equal(refLinks('landed 4f9c2ab1', { commitUrl: 'https://git.example/c/{sha}?a=1&b=2' }),
    'landed <a class="sha" href="https://git.example/c/4f9c2ab1?a=1&amp;b=2" target="_blank" rel="noopener">4f9c2ab1</a>');
  // Code, existing links and buttons, tags, entities, words, paths and
  // all-digit or all-letter runs are left alone.
  const untouched = '<code>#5</code> <a href="/x#7">#7</a> <button>#8</button> &#39; a#9 /p/#10 1234567 deadbeef src/4f9c2ab1 x.4f9c2ab1';
  assert.equal(refLinks(untouched, { commitUrl: 'u/{sha}' }), untouched);
  assert.equal(refLinks('<p>#6</p>'), '<p><button class="linkbtn" data-card-num="6">#6</button></p>');
});
