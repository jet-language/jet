import { test } from 'node:test';
import assert from 'node:assert/strict';
import { migrate } from '../app/migrate.mjs';
import { project, laneOf, findCard, normalize, updateEpoch, activeEpoch, setCurrentEpoch } from '../app/store.mjs';

const V3 = {
  meta: { version: 3, currentEpoch: 'e3', nextNum: 5, ui: { toggled: ['epoch:e3'] } },
  epochs: [{ id: 'e3', num: 3, name: 'Third', status: 'active', order: 3, goal: 'ship', exitCriteria: ['x'] }],
  cards: [
    { id: 'c1', num: 1, title: 'Old card', body: '', kind: 'task', track: 'epoch', epoch: 'e3', phase: 'building', priority: 'P1', plan: 'p', blockedBy: [], log: [], workOrder: 2 },
    { id: 'c2', num: 2, title: 'Deciding card', body: '', kind: 'feature', track: 'sidequest', epoch: null, phase: 'planning', priority: 'P2', plan: null, blockedBy: [], log: [] },
  ],
  decisions: [{ id: 'D-OLD1', cardId: 'c2', title: 'old choice', options: [{ key: 'A', name: 'a' }], status: 'open' }],
  binder: [{ id: 'b1', text: 'an idea', status: 'open' }],
  questions: [{ id: 'q1', cardId: 'c1', by: 'owner', kind: 'question', text: '?', status: 'open', answer: '' }],
};

test('v3 import: binder→ideas, lossless fields, lanes still compute', () => {
  const s = migrate(V3, { project: 'Jet' });
  assert.equal(s.meta.version, 4);
  assert.equal(s.meta.project, 'Jet');
  assert.equal(s.meta.rev, 0);
  assert.equal(s.meta.currentEpoch, undefined);      // D-TWR-OPS1=A: retired field is dropped
  assert.equal(s.epochs[0].status, 'active');         // active epoch is the single source of truth
  assert.equal(s.meta.nextNum, 5);
  assert.deepEqual(s.meta.ui.toggled, ['epoch:e3']);
  assert.equal(s.ideas.length, 1);
  assert.equal(s.milestones.length, 0);
  // lossless: epoch extras survive
  assert.deepEqual(s.epochs[0].exitCriteria, ['x']);
  assert.equal(s.epochs[0].num, 3);
  // lanes compute on migrated data
  assert.equal(laneOf(findCard(s, '#1'), s.decisions, s.cards).lane, 'building');
  assert.equal(laneOf(findCard(s, '#2'), s.decisions, s.cards).lane, 'decide');
  // projection doesn't throw and counts add up
  const proj = project(s);
  assert.equal(proj.counts.decide, 1);
  assert.equal(proj.counts.ideas, 1);
  assert.equal(proj.counts.openQuestions, 1);
});

test('very old string epochs normalize to objects', () => {
  const s = migrate({ epochs: ['e1'], cards: [], decisions: [] });
  assert.deepEqual(s.epochs[0], { id: 'e1', name: 'e1', goal: '', status: 'open' });
});

// D-TWR-OPS1=A: active epoch derives from epoch.status; meta.currentEpoch retired.
test('OPS1: dangling currentEpoch reconciles to an active status, then drops', () => {
  const s = normalize({ meta: { currentEpoch: 'e3' }, epochs: [{ id: 'e3', status: 'planned' }] });
  assert.equal(s.epochs[0].status, 'active');   // the pointed-at epoch is promoted
  assert.equal(activeEpoch(s), 'e3');
  assert.equal(s.meta.currentEpoch, undefined); // pointer is gone for good
});

test('OPS1: currentEpoch ignored when some epoch is already active', () => {
  const s = normalize({ meta: { currentEpoch: 'e3' }, epochs: [{ id: 'e2', status: 'active' }, { id: 'e3', status: 'planned' }] });
  assert.equal(activeEpoch(s), 'e2');           // existing active wins; no second active created
  assert.equal(s.epochs[1].status, 'planned');
});

test('OPS1: activating a second epoch is rejected', () => {
  const s = normalize({ epochs: [{ id: 'e3', status: 'active' }, { id: 'e4', status: 'planned' }] });
  assert.throws(() => updateEpoch(s, 'e4', { status: 'active' }), /already active/);
  setCurrentEpoch(s, null);                     // demote the live epoch
  assert.equal(activeEpoch(s), null);
  updateEpoch(s, 'e4', { status: 'active' });   // now it is free to activate
  assert.equal(activeEpoch(s), 'e4');
});
