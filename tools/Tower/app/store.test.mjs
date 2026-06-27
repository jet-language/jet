import test from 'node:test';
import assert from 'node:assert/strict';
import { clear, load, project } from './store.mjs';

const base = () => ({
  meta: { version: 3, currentEpoch: 'e3', nextNum: 1, ui: { toggled: [] } },
  epochs: [],
  cards: [{
    id: 'c1',
    num: 1,
    title: 'Decision card',
    body: '',
    kind: 'feature',
    track: 'epoch',
    epoch: 'e3',
    phase: 'deciding',
    priority: 'P2',
    plan: null,
    blockedBy: [],
    log: [],
    created: '2026-06-27',
    updated: '2026-06-27',
  }],
  decisions: [{ id: 'D-ONE', cardId: 'c1', title: 'Pick one', options: [{ key: 'A' }], status: 'open' }],
  binder: [],
  questions: [],
});

test('clear advances a fully ratified deciding card to planning', () => {
  const s = base();
  clear(s, 'D-ONE', 'A');
  assert.equal(s.cards[0].phase, 'planning');
  assert.equal(project(s).cards[0].lane.lane, 'plan');
});

test('clear advances a fully ratified planned card to ready', () => {
  const s = base();
  s.cards[0].plan = 'Build it';
  clear(s, 'D-ONE', 'A');
  assert.equal(s.cards[0].phase, 'ready');
  assert.equal(project(s).cards[0].lane.lane, 'implement');
});

test('clear keeps a card in deciding while another decision is open', () => {
  const s = base();
  s.decisions.push({ id: 'D-TWO', cardId: 'c1', title: 'Pick two', options: [{ key: 'A' }], status: 'open' });
  clear(s, 'D-ONE', 'A');
  assert.equal(s.cards[0].phase, 'deciding');
  assert.equal(project(s).cards[0].lane.lane, 'decide');
});

test('project treats a cleared deciding card as agent work', () => {
  const s = base();
  s.decisions[0].status = 'ratified';
  s.decisions[0].outcome = 'A';
  assert.equal(project(s).cards[0].lane.lane, 'plan');
});

test('current Tower data has no cleared card stuck in deciding', () => {
  const stuck = project(load()).cards
    .filter(c => c.phase === 'deciding' && !c.decisions.some(d => d.status !== 'ratified'))
    .map(c => `#${c.num} ${c.title}`);
  assert.deepEqual(stuck, []);
});
