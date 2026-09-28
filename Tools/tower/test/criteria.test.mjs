// Card #463 — exit criteria gate `done` (D-TWR-CRIT1=C) and acceptance
// ballots on flagged cards (D-TWRGUARD1=C: agent-hard, owner-soft).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';

const resolveAcceptance = db.createAcceptanceResolver();
const ownerProvenance = (outcome) => ({ kind: 'owner-ui', session: 'test-session', challenge: `test-${outcome}`, issuedFor: 'D-ACCEPT-1', outcome });

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-test-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

test('done-gate: refuses close while a criterion is not met or verified (E_CRITERIA)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'matrix vs spec', 'planner'));
  st.mutate((s) => db.addCriterion(s, '#1', 'perf budget', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'ran it', by: 'builder' }));
  // criterion 2 is still open; criterion 1 is already met.
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg)),
    (e) => e instanceof TowerError && e.code === 'E_CRITERIA' && /1 of 2 criteria not met or verified \(2\)/.test(e.message));
  assert.equal(st.load().cards[0].phase, 'planning', 'refused write must not change phase');
});

test('verify rejects the builder as its own verifier (E_CRITERIA_SELF)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'tested', by: 'builder-agent' }));
  assert.throws(
    () => st.mutate((s) => db.verifyCriterion(s, '#1', 1, { evidence: 're-tested', by: 'builder-agent' })),
    (e) => e instanceof TowerError && e.code === 'E_CRITERIA_SELF');
  // a different verifier is fine
  st.mutate((s) => db.verifyCriterion(s, '#1', 1, { evidence: 're-tested independently', by: 'verifier-agent' }));
  assert.equal(st.load().cards[0].criteria[0].status, 'verified');
});

test('owner writes bypass the criteria gate entirely', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'unmet thing', 'planner'));
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  assert.equal(result.phase, 'done');
  const bypassEvent = st.load().events.find(e => e.action === 'card.criteria-bypass');
  assert.ok(bypassEvent, 'owner bypass must be logged');
  assert.equal(bypassEvent.note, 'owner bypass');
});

test('a card with no criteria rejects an agent close', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'some-agent' }, cfg)),
    (e) => e instanceof TowerError && e.code === 'E_CRITERIA');
  assert.equal(st.load().cards[0].phase, 'planning');
});

test('flagged card: agent done-attempt with clean checklist mints D-ACCEPT, stays in verify; owner accept closes it', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built it', by: 'builder-agent' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 1, { evidence: 'checked it', by: 'verifier-agent' }));
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'verifier-agent' }, cfg));
  assert.equal(result.phase, 'verify', 'must not close outright — waits on acceptance');
  const s1 = st.load();
  const d = s1.decisions.find(x => x.id === 'D-ACCEPT-1');
  assert.ok(d, 'must mint D-ACCEPT-<num>');
  assert.equal(d.status, 'open');
  assert.equal(d.cardId, s1.cards[0].id);
  assert.match(d.title, /Accept #1/);
  assert.deepEqual(d.options.map(o => o.key), ['accept', 'bounce']);

  st.mutate((s) => resolveAcceptance(s, 'D-ACCEPT-1', 'accept', null, ownerProvenance('accept')));
  const s2 = st.load();
  assert.equal(s2.cards[0].phase, 'done');
  assert.equal(s2.decisions.find(x => x.id === 'D-ACCEPT-1').status, 'ratified');
});

test('flagged card: owner bounce reopens to building with the comment logged', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built it', by: 'builder-agent' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder-agent' }, cfg));
  const before = st.load();
  assert.equal(before.cards[0].phase, 'verify');
  assert.ok(before.decisions.find(x => x.id === 'D-ACCEPT-1'));

  st.mutate((s) => resolveAcceptance(s, 'D-ACCEPT-1', 'bounce', 'not there yet — missing edge case', ownerProvenance('bounce')));
  const s2 = st.load();
  assert.equal(s2.cards[0].phase, 'building');
  const logText = s2.cards[0].log.map(l => l.text).join(' | ');
  assert.match(logText, /Bounced back to building: not there yet — missing edge case/);
});

test('a second done-attempt while D-ACCEPT is still open does not mint a duplicate', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built it', by: 'agent-1' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-2' }, cfg));
  const s1 = st.load();
  const mints = s1.decisions.filter(x => x.id === 'D-ACCEPT-1');
  assert.equal(mints.length, 1, 'must not create a second D-ACCEPT-1');
  assert.equal(s1.cards[0].phase, 'verify');
});

test('after a bounce, a fresh done-attempt re-opens the same D-ACCEPT id (no id collision)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built it', by: 'agent-1' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  st.mutate((s) => resolveAcceptance(s, 'D-ACCEPT-1', 'bounce', 'fix it', ownerProvenance('bounce')));
  assert.equal(st.load().cards[0].phase, 'building');
  // round 2
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  const s2 = st.load();
  assert.equal(s2.decisions.filter(x => x.id === 'D-ACCEPT-1').length, 1);
  assert.equal(s2.decisions.find(x => x.id === 'D-ACCEPT-1').status, 'open');
});

test('addCriterion/meetCriterion/verifyCriterion validate ref, n, and required fields', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.throws(() => st.mutate((s) => db.addCriterion(s, '#1', '', 'planner')), (e) => e.code === 'E_INVALID');
  assert.throws(() => st.mutate((s) => db.meetCriterion(s, '#1', 99, { by: 'x' })), (e) => e.code === 'E_NOT_FOUND');
  st.mutate((s) => db.addCriterion(s, '#1', 'a thing', 'planner'));
  assert.throws(() => st.mutate((s) => db.verifyCriterion(s, '#1', 1, { by: 'x' })), (e) => e.code === 'E_INVALID'); // not met yet
});

test('reopenCriterion resets met and verified rows and records the reason', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Reopen criteria' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'met row', 'planner'));
  st.mutate((s) => db.addCriterion(s, '#1', 'verified row', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built', by: 'builder' }));
  st.mutate((s) => db.meetCriterion(s, '#1', 2, { evidence: 'built', by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 2, { evidence: 'checked', by: 'verifier' }));
  st.mutate((s) => db.reopenCriterion(s, '#1', 1, { reason: 'met result changed', by: 'repairer' }));
  st.mutate((s) => db.reopenCriterion(s, '#1', 2, { reason: 'verified result changed', by: 'repairer' }));
  const state = st.load();
  assert.ok(state.cards[0].criteria.every(item => item.status === 'open' && !item.evidence));
  const events = state.events.filter(e => e.action === 'card.criteria-reopen');
  assert.equal(events.length, 2);
  assert.ok(events.every(e => e.by === 'repairer'));
  assert.match(events[0].note + events[1].note, /met result changed|verified result changed/);
});
