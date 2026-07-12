// Card #458 — D-TWRGUARD1=C remaining enforcement gates. Agent-hard,
// owner-soft: `by !== 'owner'` is refused; `by === 'owner'` bypasses
// (bypass event-logged). Criteria/checklist model + done-gate are #463's;
// this file covers the rest of the guard family built on top.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-test-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const ballot = (extra = {}) => ({
  gist: 'a plain sentence', lesson: 'Concept, mechanics, terms, stakes, and a tiny example.', story: 'Dana hits this while shipping X.', inWild: 'real code here', rec: 'A',
  options: [{ key: 'A', name: 'Option A', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'Option B', detail: 'B is brief.', code: 'b()' }],
  recommendation: { why: 'A best serves this decision.', whyNot: [{ key: 'B', reason: 'B loses the needed guarantee.' }], tradeoff: 'A adds one explicit step, which keeps behavior visible.' },
  hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
  ...extra,
});

// ---- 1. ballot-ready validation on decision add ----------------------------

test('addDecision refuses an incomplete ballot with E_BALLOT naming the gaps', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'Pick one' })),
    (e) => e instanceof TowerError && e.code === 'E_BALLOT'
      && /gist/.test(e.message) && /lesson/.test(e.message) && /story/.test(e.message) && /inWild/.test(e.message)
      && /options/.test(e.message) && /rec/.test(e.message) && /recommendation/.test(e.message) && /hybrid/.test(e.message));
});

test('addDecision refuses options missing a code field', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'Pick one',
      ...ballot(), options: [{ key: 'A', name: 'a', detail: 'A.' }, { key: 'B', name: 'b', detail: 'B.', code: 'b()' }] })),
    (e) => e.code === 'E_BALLOT' && /options\[\]\.code/.test(e.message));
});

test('addDecision accepts a full ballot', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const { result } = st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'Pick one', ...ballot() }));
  assert.equal(result.draft, false);
});

test('addDecision rejects dense plain-language prose', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const dense = Array.from({ length: 33 }, (_, i) => `word${i}`).join(' ') + '.';
  assert.throws(
    () => st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'Pick one', ...ballot({ lesson: dense }) })),
    (e) => e.code === 'E_BALLOT' && /plain language/.test(e.message) && /33 words/.test(e.message));
});

test('recommendation must explain every losing option', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'Pick one', ...ballot({ recommendation: { why: 'A wins.', whyNot: [], tradeoff: 'A costs one step.' } }) })),
    (e) => e.code === 'E_BALLOT' && /recommendation\.whyNot\[B\]/.test(e.message));
});

test('hybrid pass must harvest every option into the recommendation', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'Pick one', ...ballot({ hybrid: { result: 'B', synthesis: 'B combines ideas.', harvest: [] } }) })),
    (e) => e.code === 'E_BALLOT' && /hybrid\.result \(must match rec\)/.test(e.message) && /hybrid\.harvest\[A\]/.test(e.message));
});

test('--draft exempts validation and stays out of the decide lane + counts', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const { result } = st.mutate((s) => db.addDecision(s, { cardId: '#1', title: 'WIP', draft: true }));
  assert.equal(result.draft, true);
  const proj = db.project(st.load());
  const card = proj.cards.find(c => c.num === 1);
  assert.notEqual(card.lane.lane, 'decide', 'draft decision must not put the card in decide');
  assert.equal(proj.counts.decide, 0, 'draft decisions excluded from counts.decide');
});

test('decision update --ready validates then clears draft', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-R1', title: 'WIP', draft: true }));
  assert.throws(
    () => st.mutate((s) => db.updateDecision(s, 'D-R1', { ready: true }, 'agent')),
    (e) => e.code === 'E_BALLOT');
  st.mutate((s) => db.updateDecision(s, 'D-R1', { ...ballot(), ready: true }, 'agent'));
  const s2 = st.load();
  assert.equal(s2.decisions[0].draft, false);
  const proj = db.project(s2);
  assert.equal(proj.cards[0].lane.lane, 'decide', 'now-ready decision blocks like any other');
});

test('acceptance ballots (mintAcceptance) are exempt from the narrative ballot standard', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  // no criteria — done-gate mints D-ACCEPT-1 straight away; must not throw E_BALLOT
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  assert.equal(result.phase, 'verify');
  assert.ok(st.load().decisions.find(d => d.id === 'D-ACCEPT-1'));
});

// ---- 2. owner-only ratify ----------------------------------------------------

test('ratify refuses a non-owner without --quote (E_OWNER_ONLY)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  assert.throws(
    () => st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'some-agent')),
    (e) => e.code === 'E_OWNER_ONLY');
});

test('ratify allows a non-owner with --quote, and logs the quote', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'some-agent', "go with A"));
  const s2 = st.load();
  assert.equal(s2.decisions[0].status, 'ratified');
  const ev = s2.events.find(e => e.action === 'decision.ratify');
  assert.match(ev.note, /quoting owner: "go with A"/);
});

test('ratify by owner needs no quote', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  assert.equal(st.load().decisions[0].status, 'ratified');
});

// ---- 3. frozen guard ----------------------------------------------------------

test('agent update refused on a frozen card (E_OWNER_LANE)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'frozen' }, cfg));
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { body: 'prep notes', by: 'agent-1' }, cfg)),
    (e) => e.code === 'E_OWNER_LANE');
});

test('agent claim refused on a frozen card (E_OWNER_LANE)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'frozen' }, cfg));
  assert.throws(() => st.mutate((s) => db.claimCard(s, '#1', 'agent-1')), (e) => e.code === 'E_OWNER_LANE');
});

test('owner update/claim on a frozen card is fine', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'frozen' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { body: 'prep', by: 'owner' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'owner'));
  assert.equal(st.load().cards[0].assignee, 'owner');
});

// #516: no greenlight/activate gate — a fresh card lands straight in an
// agent lane (planning), so an agent may change its phase immediately, no
// owner step first.
test('a fresh card lands in planning; an agent can change its phase with no owner step first', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.equal(st.load().cards[0].phase, 'planning');
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'building', by: 'agent-1' }, cfg));
  assert.equal(result.phase, 'building');
});

test('a fresh card also closes freely for an agent (done-gate, not an owner lane, governs done)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  assert.equal(result.phase, 'done');
});

// ---- 4. delete refuses when a ratified decision is attached -----------------

test('deleteCard refuses when a ratified decision is attached (E_HAS_RATIFIED), even for owner', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  assert.throws(
    () => st.mutate((s) => db.deleteCard(s, '#1', { by: 'owner' })),
    (e) => e.code === 'E_HAS_RATIFIED');
});

test('deleteCard proceeds when decisions are only open (unratified)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.deleteCard(s, '#1', { by: 'owner' }));
  assert.equal(st.load().cards.length, 0);
});

// ---- 5. ratify outcome must match an option key -----------------------------

test('ratify refuses an outcome that is not one of the option keys (E_INVALID)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  assert.throws(
    () => st.mutate((s) => db.ratify(s, 'D-1', 'Z', null, 'owner')),
    (e) => e.code === 'E_INVALID');
});

test('ratify accepts an outcome matching an option key', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'B', null, 'owner'));
  assert.equal(st.load().decisions[0].outcome, 'B');
});

// ---- 6. blockedBy accepts decision ids --------------------------------------

test('updateCard blockedBy accepts a decision id', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'B' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#2', { blockedBy: ['D-1'] }, cfg));
  assert.deepEqual(st.load().cards[1].blockedBy, ['D-1']);
});

test('laneOf treats an unratified decision id in blockedBy as blocking; ratified is not', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'B', phase: 'ready', blockedBy: ['D-1'] }, cfg));
  let s = st.load();
  const lane = db.laneOf(db.findCard(s, '#2'), s.decisions, s.cards);
  assert.equal(lane.lane, 'blocked');
  assert.match(lane.label, /D-1/);
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  s = st.load();
  assert.equal(db.laneOf(db.findCard(s, '#2'), s.decisions, s.cards).lane, 'implement');
});

test('updateCard blockedBy rejects an unknown id', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { blockedBy: ['nope'] }, cfg)),
    (e) => e.code === 'E_NOT_FOUND');
});

// ---- 7. release of a building card requires --handoff -----------------------

test('releaseCard on a building card by an agent refuses without --handoff (E_HANDOFF)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  assert.throws(
    () => st.mutate((s) => db.releaseCard(s, '#1', 'agent-1')),
    (e) => e.code === 'E_HANDOFF');
});

test('releaseCard on a building card logs [handoff] and clears assignee', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  st.mutate((s) => db.releaseCard(s, '#1', 'agent-1', 'built the parser half, sema left'));
  const s2 = st.load();
  assert.equal(s2.cards[0].assignee, null);
  assert.match(s2.cards[0].log[0].text, /^\[handoff\] built the parser half, sema left$/);
});

test('releaseCard on a non-building card needs no handoff', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg)); // planning
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  st.mutate((s) => db.releaseCard(s, '#1', 'agent-1'));
  assert.equal(st.load().cards[0].assignee, null);
});

test('owner release of a building card needs no handoff', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'owner'));
  st.mutate((s) => db.releaseCard(s, '#1', 'owner'));
  assert.equal(st.load().cards[0].assignee, null);
});

// ---- 8. tower verdict --------------------------------------------------------

test('mintVerdict is owner-only (E_OWNER_ONLY), no quote escape', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s) => db.mintVerdict(s, '#1', 'looks good', null, 'some-agent')),
    (e) => e.code === 'E_OWNER_ONLY');
});

test('mintVerdict mints an already-ratified decision and logs it on the card', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const { result } = st.mutate((s) => db.mintVerdict(s, '#1', 'ship it', 'Ship review', 'owner'));
  assert.equal(result.status, 'ratified');
  assert.equal(result.comment, 'ship it');
  assert.match(result.id, /^D-VERDICT-1-\d+$/);
  const s2 = st.load();
  const d = s2.decisions.find(x => x.id === result.id);
  assert.ok(d, 'verdict decision persisted');
  assert.equal(d.status, 'ratified');
  assert.match(s2.cards[0].log[0].text, /Verdict recorded/);
});

test('mintVerdict allocates a fresh <num>-<k> id on a repeat verdict for the same card', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const { result: v1 } = st.mutate((s) => db.mintVerdict(s, '#1', 'first', null, 'owner'));
  const { result: v2 } = st.mutate((s) => db.mintVerdict(s, '#1', 'second', null, 'owner'));
  assert.notEqual(v1.id, v2.id);
});

// ---- 9. ratify hook: syntax-group auto-chores -------------------------------

test('ratifying a syntax-group decision appends the standard chores to criteria', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-SYN1', title: 'spelling', group: 'syntax', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-SYN1', 'A', null, 'owner'));
  const texts = st.load().cards[0].criteria.map(i => i.text);
  assert.deepEqual(texts, [
    'Syntax.rs entry updated',
    'syntax-decisions.md log entry',
    'jet devtools grammars regenerated',
    'snapshots re-blessed',
  ]);
});

test('a second syntax-group ratification on the same card does not duplicate chores', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-SYN1', title: 'a', group: 'syntax', ...ballot() }));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-SYN2', title: 'b', group: 'syntax', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-SYN1', 'A', null, 'owner'));
  st.mutate((s) => db.ratify(s, 'D-SYN2', 'A', null, 'owner'));
  assert.equal(st.load().cards[0].criteria.length, 4, 'chores must not duplicate across two syntax ratifications');
});

test('non-syntax-group decisions do not append chores', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', group: 'architecture', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-1', 'A', null, 'owner'));
  assert.equal(st.load().cards[0].criteria.length, 0);
});
