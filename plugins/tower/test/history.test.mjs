// Card #461 — split live/history store with a walk-back buffer.
// D-TWR-ARCHIVE1=B MODIFIED: nothing retires immediately; a buffer window
// (config.retireAfterDays, default 3) lets the owner walk back a fresh
// ratification. retire() is the single chokepoint inside store.mutate().
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, copyFileSync, existsSync, statSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, normalize, project, findInHistory, restoreFromHistory, TowerError } from '../app/store.mjs';
import { writeJSON, historyFile, today, now } from '../app/paths.mjs';
import * as db from '../app/store.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-hist-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const ballot = (extra = {}) => ({
  gist: 'a plain sentence', lesson: 'Concept, mechanics, terms, stakes, and a tiny example.', story: 'Dana hits this while shipping X.', inWild: 'real code here', rec: 'A',
  options: [{ key: 'A', name: 'Option A', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'Option B', detail: 'B is brief.', code: 'b()' }],
  recommendation: { why: 'A wins here.', whyNot: [{ key: 'B', reason: 'B loses the needed behavior.' }], tradeoff: 'A adds one visible step.' },
  hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
  ...extra,
});

const OLD = '2020-01-01'; // always > any retireAfterDays in the past

// ---- 1. buffer respected ----------------------------------------------------

test('a fresh ratification stays live through the buffer window', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  // another write runs the retire chokepoint again — must be a no-op here
  st.mutate((s, cfg) => db.updateCard(s, '#1', { body: 'notes' }, cfg));
  const s = st.load();
  assert.ok(s.decisions.find(d => d.id === 'D-1'), 'ratified today must still be live');
  assert.equal(existsSync(historyFile(st.dataDir)), false, 'nothing should have moved to history yet');
});

test('a ratified decision on a still-active (non-done) card stays live no matter how old', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  st.mutate((s) => { s.decisions.find(d => d.id === 'D-1').ratifiedAt = OLD; }); // backdate, still triggers retire
  const s = st.load();
  assert.ok(s.decisions.find(d => d.id === 'D-1'), 'card view must stay whole while the card is still active');
});

// ---- 2. retire after buffer --------------------------------------------------

test('ratified decision retires once its card is done and aged past the buffer', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  // backdate both the card and the decision, then any write re-runs retire()
  st.mutate((s) => { s.cards[0].updated = OLD; s.decisions[0].ratifiedAt = OLD; });
  st.mutate((s, cfg) => db.setDigestCursor(s));
  const s = st.load();
  assert.equal(s.cards.length, 0, 'done+aged card must retire');
  assert.equal(s.decisions.length, 0, 'its decision retires with it');
  const h = st.loadHistory();
  assert.ok(h.cards.find(c => c.id === '#1' || c.num === 1));
  assert.ok(h.decisions.find(d => d.id === 'D-1'));
});

// ---- 3. card retires with its decisions + questions --------------------------

test('card retires together with ALL its decisions and questions', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  st.mutate((s) => db.addQuestion(s, { cardId: '#1', text: 'why?', by: 'owner' }));
  st.mutate((s) => db.answerQuestion(s, s.questions[0].id, 'because', 'agent-1'));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  st.mutate((s) => { s.cards[0].updated = OLD; s.decisions[0].ratifiedAt = OLD; });
  st.mutate((s) => db.setDigestCursor(s));

  const s = st.load();
  assert.equal(s.cards.length, 0);
  assert.equal(s.decisions.length, 0);
  assert.equal(s.questions.length, 0, 'questions retire with their card too');

  const h = st.loadHistory();
  const archivedCard = h.cards[0];
  assert.equal(archivedCard.num, 1);
  assert.equal(archivedCard.questions.length, 1, 'the question rides along embedded on the archived card');
  assert.equal(archivedCard.questions[0].answer, 'because');
  assert.ok(archivedCard.retiredAt);
  assert.ok(h.decisions[0].retiredAt);
});

// ---- 4. restore round-trip ----------------------------------------------------

test('restore round-trip: whole card back from archive, decisions+questions ride along, clock resets', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  st.mutate((s) => db.addQuestion(s, { cardId: '#1', text: 'why?', by: 'owner' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  st.mutate((s) => { s.cards[0].updated = OLD; s.decisions[0].ratifiedAt = OLD; });
  st.mutate((s) => db.setDigestCursor(s));
  assert.equal(st.load().cards.length, 0, 'precondition: card is archived');

  const { result } = st.restoreArchived('#1', 'owner');
  assert.equal(result.kind, 'card');
  assert.equal(result.num, 1);

  const s = st.load();
  assert.equal(s.cards.length, 1);
  assert.equal(s.cards[0].updated.slice(0, 10), today(), 'clock resets to today');
  assert.equal(s.decisions.length, 1);
  assert.equal(s.decisions[0].id, 'D-1');
  assert.equal(s.decisions[0].ratifiedAt, today(), 'decision clock resets too — otherwise it would standalone-retire right back out');
  assert.equal(s.questions.length, 1);
  assert.equal(st.loadHistory().cards.length, 0, 'gone from history once restored');
  assert.equal(st.loadHistory().decisions.length, 0);
  assert.match(s.cards[0].log[0].text, /Restored from archive/);

  // it does not immediately re-retire on the next write (clock was reset)
  st.mutate((s2) => db.setDigestCursor(s2));
  assert.equal(st.load().cards.length, 1, 'restored card must not instantly re-archive');
});

test('restore round-trip: a single decision, its card still live', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  const cardId = st.load().cards[0].id;
  // simulate this one decision having been archived on its own (the standalone
  // path, (a) above) by moving it to history ourselves, then restoring it.
  st.mutate((s) => { s.decisions = s.decisions.filter(d => d.id !== 'D-1'); });
  writeJSON(historyFile(st.dataDir), { version: 1, decisions: [{ id: 'D-1', cardId, title: 't', status: 'ratified', outcome: 'A', ratifiedAt: OLD }], cards: [], events: [] });

  const { result } = st.restoreArchived('D-1', 'agent');
  assert.equal(result.kind, 'decision');
  const s = st.load();
  assert.ok(s.decisions.find(d => d.id === 'D-1'));
  assert.equal(s.decisions.find(d => d.id === 'D-1').ratifiedAt, today());
  assert.equal(s.cards[0].updatedBy, 'agent');
});

test('restoreFromHistory refuses a decision whose card is archived too', () => {
  const s = empty('T');
  const h = { version: 1, cards: [], decisions: [{ id: 'D-1', cardId: 'c-missing', status: 'ratified', outcome: 'A' }], events: [] };
  assert.throws(() => restoreFromHistory(s, h, 'D-1', 'owner'), (e) => e instanceof TowerError && e.code === 'E_NOT_FOUND');
});

// ---- 5. merged read fall-through ----------------------------------------------

test('findInHistory resolves an archived card by id or #num', () => {
  const h = { cards: [{ id: 'c-abc', num: 7, title: 'Old' }], decisions: [], events: [] };
  assert.equal(findInHistory(h, 'c-abc').title, 'Old');
  assert.equal(findInHistory(h, '#7').title, 'Old');
  assert.equal(findInHistory(h, '7').title, 'Old');
  assert.equal(findInHistory(h, 'nope'), null);
});

test('a retired card is findable via loadHistory once gone from live cards', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  st.mutate((s) => { s.cards[0].updated = OLD; });
  st.mutate((s) => db.setDigestCursor(s));
  assert.equal(db.findCard(st.load(), '#1'), undefined, 'gone from live findCard');
  const arch = findInHistory(st.loadHistory(), '#1');
  assert.ok(arch, 'reachable through history');
  assert.equal(arch.num, 1);
});

// ---- 6. events overflow -------------------------------------------------------

test('events beyond the newest 500 archive to history.events', () => {
  const st = fresh();
  for (let i = 0; i < 520; i++) st.mutate((s, cfg) => db.addCard(s, { title: 'c' + i }, cfg));
  const s = st.load();
  assert.equal(s.events.length, 500, 'live events cap at 500');
  const h = st.loadHistory();
  assert.equal(h.events.length, 20, 'the 20-event overflow archived, one per write past the cap');
});

// ---- 7. real-board-copy migration test ----------------------------------------

const REAL_BOARD = '/home/nate/Projects/Github/jet/.tower/tower.json';

test('migration on a copy of the real board: retire pass shrinks tower.json, integrity holds', { skip: !existsSync(REAL_BOARD) }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-realcopy-'));
  copyFileSync(REAL_BOARD, join(dir, 'tower.json'));
  const preSize = statSync(join(dir, 'tower.json')).size;
  const preRaw = JSON.parse(readFileSync(join(dir, 'tower.json'), 'utf8'));

  const st = openStore(dir);
  // Predict what SHOULD retire under the default 3-day buffer, mirroring
  // retire()'s own (b)-then-(a) rule directly off the pre-migration snapshot
  // (today is whatever the running clock says) — no magic numbers, derived.
  const isOld = (d) => {
    if (!d) return false;
    const t = Date.parse(`${d}T00:00:00Z`);
    return !Number.isNaN(t) && (Date.now() - t) > 3 * 86_400_000;
  };
  const expectRetiredCardIds = new Set(preRaw.cards.filter(c => c.phase === 'done' && isOld(c.updated)).map(c => c.id));
  const remainingCards = preRaw.cards.filter(c => !expectRetiredCardIds.has(c.id));
  const liveCardById = new Map(remainingCards.map(c => [c.id, c]));
  const expectRetiredDecisionIds = new Set(preRaw.decisions.filter(d => {
    if (expectRetiredCardIds.has(d.cardId)) return true; // swept in with its card
    if (d.status !== 'ratified' || !isOld(d.ratifiedAt)) return false;
    const c = liveCardById.get(d.cardId);
    return !c || c.phase === 'done';
  }).map(d => d.id));

  // One harmless write trips the retire chokepoint (this IS the migration).
  st.mutate((s) => db.setDigestCursor(s));

  const postSize = statSync(join(dir, 'tower.json')).size;
  const liveState = st.load();
  const h = st.loadHistory();

  // Isolate the retire pass's own effect from one-time schema-migration
  // growth (e.g. #462 adding a new `refs: []` default to every card the
  // first time normalize() sees it) — reserialize the pre-migration state
  // through normalize() alone (no retire) and compare against THAT, not the
  // raw on-disk preSize, so a legitimate new default field never falsely
  // fails this test.
  const migratedOnlySize = Buffer.byteLength(JSON.stringify(normalize(JSON.parse(JSON.stringify(preRaw))), null, 2) + '\n');
  // The real board may currently have nothing old enough to retire (e.g.
  // every done card is fresher than the buffer window right now) — in that
  // case retiring is correctly a no-op and sizes match; only demand a
  // strict shrink when something was actually predicted to retire.
  if (expectRetiredCardIds.size || expectRetiredDecisionIds.size)
    assert.ok(postSize < migratedOnlySize, `retire pass must shrink from the migrated-but-unretired size (${migratedOnlySize} -> ${postSize})`);
  else
    assert.equal(postSize, migratedOnlySize, 'nothing predicted to retire — size should only reflect the schema migration');
  assert.equal(liveState.cards.length, preRaw.cards.length - expectRetiredCardIds.size, 'live card count matches the predicted retire set');
  assert.equal(h.cards.length, expectRetiredCardIds.size, 'history card count matches the predicted retire set');
  for (const id of expectRetiredCardIds) {
    assert.equal(liveState.cards.find(c => c.id === id), undefined, `${id} must be gone from live`);
    assert.ok(h.cards.find(c => c.id === id), `${id} must be reachable via loadHistory`);
  }
  for (const id of expectRetiredDecisionIds) {
    assert.equal(liveState.decisions.find(d => d.id === id), undefined, `decision ${id} must be gone from live`);
    assert.ok(h.decisions.find(d => d.id === id), `decision ${id} must be reachable via loadHistory`);
  }
  // ratifications from within the buffer window (fresh) must stay live
  for (const d of preRaw.decisions) {
    if (d.status === 'ratified' && !isOld(d.ratifiedAt) && !expectRetiredCardIds.has(d.cardId)) {
      assert.ok(liveState.decisions.find(x => x.id === d.id), `fresh ratification ${d.id} must stay live`);
    }
  }
  // live board still fully computable
  const proj = project(liveState);
  assert.ok(Array.isArray(proj.cards));
  for (const c of liveState.cards) {
    assert.doesNotThrow(() => db.laneOf(c, liveState.decisions, liveState.cards));
  }
});

// ---- 8. undo/restore duplicate-tolerance --------------------------------------

test('undo of a retiring write does not corrupt history: no duplicate archive entries', () => {
  const st = fresh();
  // A card + ratified decision that are ALREADY stale (as if real wall-clock
  // time had passed) but no write has touched the board yet, so retire()
  // hasn't had a chokepoint to run at. Written directly (bypassing mutate)
  // so this is genuinely "stale and still live", not "just retired".
  const staleLiveState = normalize({
    ...empty('Test'),
    cards: [{ id: 'c-1', num: 1, title: 'A', body: '', kind: 'task', track: 'epoch', epoch: null, milestoneId: null,
      phase: 'done', priority: 'P2', plan: null, blockedBy: [], assignee: null, log: [],
      criteria: [], needsAcceptance: false, created: now(), updated: OLD }],
    decisions: [{ id: 'D-1', cardId: 'c-1', group: 'other', title: 't', gist: '', explainer: '', story: '', inWild: '',
      detail: '', options: [], comparisons: [], rec: null, draft: false, status: 'ratified', outcome: 'A', created: now(), ratifiedAt: OLD }],
  });
  writeJSON(join(st.dataDir, 'tower.json'), staleLiveState);
  assert.equal(st.load().cards.length, 1, 'precondition: stale card sits live, retire has not run on it yet');

  // a write finally trips the retire chokepoint
  st.mutate((s) => db.setDigestCursor(s));
  assert.equal(st.load().cards.length, 0, 'retired by the write above');
  assert.equal(st.loadHistory().cards.length, 1);

  // undo: tower.json reverts to the pre-retire (stale) snapshot; history.json
  // is untouched — it is never rolled back.
  st.restore(staleLiveState, { expectRev: st.load().meta.rev });
  assert.equal(st.load().cards.length, 1, 'undo brings the stale card back live');
  assert.equal(st.loadHistory().cards.length, 1, 'history unaffected by undo (append-only)');

  // Next write re-runs retire(): the reintroduced card/decision are still
  // stale (undo didn't reset their clock) and self-heal back out of live
  // WITHOUT duplicating the already-archived entries.
  st.mutate((s3) => db.setDigestCursor(s3));
  const h = st.loadHistory();
  assert.equal(st.load().cards.length, 0, 'self-healed back out of live');
  assert.equal(h.cards.filter(c => c.id === 'c-1').length, 1, 'no duplicate card in history');
  assert.equal(h.decisions.filter(d => d.id === 'D-1').length, 1, 'no duplicate decision in history');
});

test('archived cards still drive milestone completion', () => {
  const st = fresh();
  st.mutate((s) => db.addEpoch(s, { id: 'e1', name: 'One' }));
  const milestone = st.mutate((s) => db.addMilestone(s, { epochId: 'e1', title: 'MVP' })).result;
  st.mutate((s, cfg) => db.addCard(s, {
    title: 'Archived done work', epoch: 'e1', milestoneId: milestone.id, phase: 'done',
  }, cfg));
  st.mutate((s, cfg) => db.addCard(s, {
    title: 'Live open work', epoch: 'e1', milestoneId: milestone.id,
  }, cfg));
  st.mutate((s) => { s.cards.find(c => c.num === 1).updated = OLD; });
  st.mutate((s) => db.setDigestCursor(s));
  assert.equal(st.loadHistory().cards.length, 1, 'done card retired');
  assert.equal(st.load().milestones[0].status, 'open', 'live work keeps milestone open');

  st.mutate((s, cfg) => db.updateCard(s, '#2', { milestoneId: null, by: 'agent-1' }, cfg));
  assert.equal(st.load().milestones[0].status, 'met', 'archived done work completes milestone');
  assert.deepEqual(st.project().milestones[0].progress, { total: 1, done: 1, met: true });
});
