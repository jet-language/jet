// Card #515 — Now page owner-verification queue. Covers the two layers this
// repo actually unit-tests (store business logic + HTTP API); tower.js is a
// DOM-only client with no test harness anywhere in this repo (importing it
// under plain Node throws — it wires `document.addEventListener` at module
// scope), so the section's live rendering is proven by the temp-server curl
// self-check in the card's verification notes, not here.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, acceptanceCheckInstructions } from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';
import * as db from '../app/store.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-avq-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

// ---- store: checkInstructions assembly --------------------------------------

test('acceptanceCheckInstructions: criteria text becomes toCheck, evidence becomes confirms', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', refs: ['tests/foo.rs'] }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'section renders on Now', 'builder'));
  st.mutate((s) => db.addCriterion(s, '#1', 'accept closes the card', 'builder'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'curl showed the section', by: 'builder' }));
  const c = st.load().cards[0];
  const ci = acceptanceCheckInstructions(c);
  assert.deepEqual(ci.toCheck, ['section renders on Now', 'accept closes the card', 'Check tests/foo.rs']);
  assert.deepEqual(ci.confirms, ['section renders on Now — curl showed the section']);
});

test('acceptanceCheckInstructions: no criteria and no refs yields null (client falls back)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.equal(acceptanceCheckInstructions(st.load().cards[0]), null);
});

test('mintAcceptance stamps checkInstructions onto D-ACCEPT and re-mint refreshes it', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'ran it', by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 1, { evidence: 'checked it', by: 'verifier' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'verifier' }, cfg));
  let d = st.load().decisions.find(x => x.id === 'D-ACCEPT-1');
  assert.deepEqual(d.checkInstructions.toCheck, ['thing works']);
  assert.deepEqual(d.checkInstructions.confirms, ['thing works — checked it (verified by verifier)']);

  // bounce, add a second criterion, re-attempt — the ballot's instructions
  // must refresh to match, not keep serving round-1 evidence.
  st.mutate((s) => db.ratify(s, 'D-ACCEPT-1', 'bounce', 'add more coverage', 'owner'));
  st.mutate((s) => db.addCriterion(s, '#1', 'second thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 2, { evidence: 'ran it too', by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 2, { evidence: 'checked it too', by: 'verifier' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'verifier' }, cfg));
  d = st.load().decisions.find(x => x.id === 'D-ACCEPT-1');
  assert.equal(d.status, 'open');
  assert.deepEqual(d.checkInstructions.toCheck, ['thing works', 'second thing works']);
  assert.deepEqual(d.checkInstructions.confirms, [
    'thing works — checked it (verified by verifier)',
    'second thing works — checked it too (verified by verifier)',
  ]);
});

test('a verify-phase card with no needsAcceptance flag carries no D-ACCEPT ballot — the client renders it as a bare row', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Manually parked in verify' }, cfg));
  st.mutate((s, cfg) => db.activate(s, '#1', { by: 'owner' }, cfg));
  // no criteria, no flag — an agent (or a stray CLI call) parked it in verify
  // directly, never going through the done-gate at all.
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'verify', by: 'some-agent' }, cfg));
  const c = st.load().cards[0];
  assert.equal(c.phase, 'verify');
  assert.equal(c.needsAcceptance, false);
  assert.equal(st.load().decisions.find(x => x.id === 'D-ACCEPT-1'), undefined);
});

// ---- server: the Accept / Bounce actions the Now page's buttons drive -------

const dir = mkdtempSync(join(tmpdir(), 'tower-avq-srv-'));
writeJSON(join(dir, 'tower.json'), empty('Srv'));
writeJSON(configFile(dir), { project: 'Srv' });
const store = openStore(dir);
const PORT = 7959;
const server = serve(store, PORT, false);
after(() => server.close());
const url = (p) => `http://localhost:${PORT}${p}`;
const post = async (route, body) => {
  const r = await fetch(url('/api/' + route), { method: 'POST', body: JSON.stringify(body) });
  return { status: r.status, json: await r.json() };
};

test('needsAcceptance card in verify: state carries the D-ACCEPT ballot until owner acts', async () => {
  await post('card/add', { title: 'Flagged card', needsAcceptance: true, by: 'owner' });
  await post('card/criteria-add', { id: '#1', text: 'does the thing', by: 'planner' });
  await post('card/criteria-meet', { id: '#1', n: 1, evidence: 'built', by: 'builder' });
  await post('card/criteria-verify', { id: '#1', n: 1, evidence: 'checked', by: 'verifier' });
  const done = await post('card/update', { id: '#1', phase: 'done', by: 'verifier' });
  assert.equal(done.status, 200);
  assert.equal(done.json.result.phase, 'verify');
  const state = await (await fetch(url('/api/state'))).json();
  const ballot = state.decisions.find(d => d.id === 'D-ACCEPT-1');
  assert.ok(ballot, 'ballot must be in projected state for the Now page to read');
  assert.equal(ballot.status, 'open');
  assert.deepEqual(ballot.checkInstructions.confirms, ['does the thing — checked (verified by verifier)']);
});

test('Accept via POST /api/clearance closes the card with owner attribution logged', async () => {
  const r = await post('clearance', { decisionId: 'D-ACCEPT-1', outcome: 'accept', by: 'owner' });
  assert.equal(r.status, 200);
  assert.equal(r.json.result.status, 'ratified');
  const state = await (await fetch(url('/api/state'))).json();
  const card = state.cards.find(c => c.num === 1);
  assert.equal(card.phase, 'done');
  const ratifyEvent = state.events.find(e => e.action === 'decision.ratify' && e.ref === 'D-ACCEPT-1');
  assert.ok(ratifyEvent, 'ratification must be in the event log');
  assert.equal(ratifyEvent.by, 'owner');
});

test('Bounce via POST /api/clearance returns the card to building with the comment logged', async () => {
  await post('card/add', { title: 'Second flagged card', needsAcceptance: true, by: 'owner' });
  await post('card/update', { id: '#2', phase: 'done', by: 'builder' }); // no criteria — mints straight away
  const r = await post('clearance', { decisionId: 'D-ACCEPT-2', outcome: 'bounce', comment: 'missing the edge case', by: 'owner' });
  assert.equal(r.status, 200);
  const state = await (await fetch(url('/api/state'))).json();
  const card = state.cards.find(c => c.num === 2);
  assert.equal(card.phase, 'building');
  assert.match(card.log[0].text, /Bounced back to building: missing the edge case/);
  const ratifyEvent = state.events.find(e => e.action === 'decision.ratify' && e.ref === 'D-ACCEPT-2');
  assert.equal(ratifyEvent.by, 'owner');
});

test('a card parked in verify without the flag has no D-ACCEPT ballot but is still in state.cards for the Now page to list', async () => {
  await post('card/add', { title: 'Unflagged in verify', by: 'owner' });
  await post('card/activate', { id: '#3', by: 'owner' });
  const upd = await post('card/update', { id: '#3', phase: 'verify', by: 'some-agent' });
  assert.equal(upd.status, 200);
  const state = await (await fetch(url('/api/state'))).json();
  const card = state.cards.find(c => c.num === 3);
  assert.equal(card.phase, 'verify');
  assert.equal(card.needsAcceptance, false);
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-3'), undefined);
});
