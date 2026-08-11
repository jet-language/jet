// Process-close contract: card closure and explicit milestone review.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { empty, openStore, TowerError } from '../app/store.mjs';
import * as db from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-process-close-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const addCard = (st, extra = {}) => st.mutate((s, cfg) => db.addCard(s, { title: 'Card', ...extra }, cfg));

test('agent done rejects a card with zero exit criteria', () => {
  const st = fresh();
  addCard(st);
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent' }, cfg)),
    (e) => e instanceof TowerError && e.code === 'E_CRITERIA' && /at least one exit criterion/.test(e.message),
  );
  assert.equal(st.load().cards[0].phase, 'planning');
});

test('agent done accepts all met rows and all verified rows', () => {
  const met = fresh();
  addCard(met);
  met.mutate((s) => db.addCriterion(s, '#1', 'built', 'planner'));
  met.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built', by: 'builder' }));
  met.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'orchestrator' }, cfg));
  assert.equal(met.load().cards[0].phase, 'done');

  const verified = fresh();
  addCard(verified);
  verified.mutate((s) => db.addCriterion(s, '#1', 'reviewed', 'planner'));
  verified.mutate((s) => db.meetCriterion(s, '#1', 1, { by: 'builder' }));
  verified.mutate((s) => db.verifyCriterion(s, '#1', 1, { by: 'reviewer' }));
  verified.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'orchestrator' }, cfg));
  assert.equal(verified.load().cards[0].phase, 'done');
});

test('owner legacy close bypasses the card guard and records the audit event', () => {
  const st = fresh();
  addCard(st);
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  assert.equal(st.load().cards[0].phase, 'done');
  const event = st.load().events.find(e => e.action === 'card.criteria-bypass');
  assert.equal(event.by, 'owner');
  assert.equal(event.ref, st.load().cards[0].id);
  assert.equal(event.note, 'owner bypass');
});

test('all linked cards done makes a milestone review-ready, not met', () => {
  const st = fresh();
  const milestone = st.mutate((s) => db.addMilestone(s, { epochId: 'e1', title: 'MVP' })).result;
  addCard(st, { milestoneId: milestone.id });
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  const state = st.load();
  assert.equal(state.milestones[0].status, 'review-ready');
  assert.equal(state.milestones[0].verification, undefined);
  assert.deepEqual(st.project().milestones[0].progress, { total: 1, done: 1, reviewReady: true, met: false });
});

test('milestone status cannot bypass explicit verification', () => {
  const st = fresh();
  const milestone = st.mutate((s) => db.addMilestone(s, { epochId: 'e1', title: 'MVP' })).result;
  assert.throws(
    () => st.mutate((s) => db.updateMilestone(s, milestone.id, { status: 'met' }, 'owner')),
    (e) => e instanceof TowerError && e.code === 'E_MILESTONE_VERIFY' && /milestone verify/.test(e.message),
  );
});

test('milestone verify needs all cards done and all milestone criteria verified', () => {
  const st = fresh();
  const milestone = st.mutate((s) => db.addMilestone(s, {
    epochId: 'e1', title: 'MVP', criteria: [{ text: 'user path' }],
  })).result;
  addCard(st, { milestoneId: milestone.id });
  st.mutate((s) => db.meetMilestoneCriterion(s, milestone.id, 1, { by: 'builder' }));
  assert.throws(
    () => st.mutate((s) => db.verifyMilestone(s, milestone.id, { evidence: 'not yet', by: 'reviewer' })),
    (e) => e instanceof TowerError && e.code === 'E_MILESTONE' && /every linked card must be done/.test(e.message),
  );
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  assert.throws(
    () => st.mutate((s) => db.verifyMilestone(s, milestone.id, { evidence: 'still not yet', by: 'reviewer' })),
    (e) => e instanceof TowerError && e.code === 'E_MILESTONE' && /unverified criteria/.test(e.message),
  );
  st.mutate((s) => db.verifyMilestoneCriterion(s, milestone.id, 1, { by: 'reviewer' }));
  const { result } = st.mutate((s) => db.verifyMilestone(s, milestone.id, { evidence: 'review complete', by: 'reviewer' }));
  assert.equal(result.status, 'met');
  assert.deepEqual(result.verification, { by: 'reviewer', evidence: 'review complete', at: result.verification.at });
  assert.match(result.verification.at, /^\d{4}-\d{2}-\d{2}T/);
});

test('milestone verifier must differ from the criterion builder', () => {
  const st = fresh();
  const milestone = st.mutate((s) => db.addMilestone(s, {
    epochId: 'e1', title: 'MVP', criteria: [{ text: 'user path' }],
  })).result;
  st.mutate((s) => db.meetMilestoneCriterion(s, milestone.id, 1, { by: 'builder' }));
  assert.throws(
    () => st.mutate((s) => db.verifyMilestoneCriterion(s, milestone.id, 1, { by: 'builder' })),
    (e) => e instanceof TowerError && e.code === 'E_CRITERIA_SELF',
  );
});

test('reopening a linked card or milestone criterion clears milestone signoff', () => {
  const st = fresh();
  const milestone = st.mutate((s) => db.addMilestone(s, {
    epochId: 'e1', title: 'MVP', criteria: [{ text: 'user path' }],
  })).result;
  addCard(st, { milestoneId: milestone.id });
  st.mutate((s) => db.meetMilestoneCriterion(s, milestone.id, 1, { by: 'builder' }));
  st.mutate((s) => db.verifyMilestoneCriterion(s, milestone.id, 1, { by: 'criterion-reviewer' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  st.mutate((s) => db.verifyMilestone(s, milestone.id, { evidence: 'first review', by: 'milestone-reviewer' }));
  assert.equal(st.load().milestones[0].status, 'met');

  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'building', by: 'owner' }, cfg));
  assert.equal(st.load().milestones[0].status, 'open');
  assert.equal(st.load().milestones[0].verification, undefined);

  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  st.mutate((s) => db.verifyMilestone(s, milestone.id, { evidence: 'second review', by: 'milestone-reviewer' }));
  st.mutate((s) => db.reopenMilestoneCriterion(s, milestone.id, 1, { reason: 'new case', by: 'repairer' }));
  assert.equal(st.load().milestones[0].status, 'review-ready');
  assert.equal(st.load().milestones[0].verification, undefined);
});
