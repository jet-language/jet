// #464 — radarData(s) unit tests (D-TWR-BOARD1=A). Pure function over the
// raw store shape (no store handle needed); a hand-built fixture keeps event
// timestamps exact so burndown bucketing and stall-day math are checkable
// without waiting on real clock drift.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { radarData } from '../app/store.mjs';

const DAY_MS = 86_400_000;
const iso = (daysAgo) => new Date(Date.now() - daysAgo * DAY_MS).toISOString();
const dayKey = (d) => d.slice(0, 10);

function fixture() {
  return {
    meta: {},
    epochs: [
      { id: 'e1', name: 'Epoch One', goal: 'g1', status: 'planned', order: 1 },
      { id: 'e2', name: 'Epoch Two', goal: 'g2', status: 'active', order: 2 },
      { id: 'e3', name: 'Arrived Epoch', status: 'arrived', order: 3 },
    ],
    milestones: [
      { id: 'm1', epochId: 'e1', title: 'Stalled milestone', goal: '', status: 'open' },
      { id: 'm2', epochId: 'e1', title: 'Untouched milestone', goal: '', status: 'open' },
      { id: 'm3', epochId: 'e2', title: 'No linked cards', goal: '', status: 'open' },
      { id: 'm4', epochId: 'e1', title: 'Finished milestone', goal: '', status: 'open' },
    ],
    cards: [
      { id: 'c1', num: 1, epoch: 'e1', track: 'epoch', phase: 'done', milestoneId: 'm4' },
      { id: 'c2', num: 2, epoch: 'e1', track: 'epoch', phase: 'building' },
      { id: 'c_ms', num: 3, epoch: 'e1', track: 'epoch', phase: 'ready', milestoneId: 'm1' },
      { id: 'c_none', num: 4, epoch: 'e1', track: 'epoch', phase: 'ready', milestoneId: 'm2' },
      { id: 'c3', num: 5, epoch: 'e2', track: 'epoch', phase: 'done' },
      { id: 'c4', num: 6, epoch: 'e2', track: 'epoch', phase: 'building' },
      { id: 'c5', num: 7, epoch: 'e2', track: 'sidequest', phase: 'building' },
      { id: 'c6', num: 8, epoch: 'e3', track: 'epoch', phase: 'done' },
    ],
    events: [
      { at: iso(0), by: 'agent', action: 'card.update', ref: 'c1', note: 'phase' },          // today, c1 done → e1 burndown day0
      { at: iso(1), by: 'agent', action: 'card.update', ref: 'c3', note: 'title,phase' },     // day1, c3 done → e2 burndown day1
      { at: iso(2), by: 'agent', action: 'card.update', ref: 'c2', note: 'phase' },           // c2 not done → ignored
      { at: iso(3), by: 'agent', action: 'card.update', ref: 'c3', note: 'title' },           // no 'phase' in note → ignored
      { at: iso(10), by: 'agent', action: 'card.update', ref: 'c_ms', note: 'body' },         // touches m1's only linked card
      { at: iso(40), by: 'agent', action: 'card.update', ref: 'c1', note: 'phase' },          // outside 30-day window → ignored
      { at: iso(5), by: 'agent', action: 'decision.ratify', ref: 'c1', note: '' },            // wrong action → ignored
    ],
  };
}

test('radar: excludes arrived/done epochs, active epoch sorts first', () => {
  const r = radarData(fixture());
  assert.deepEqual(r.map(x => x.id), ['e2', 'e1']); // e2 is the active epoch, bumped ahead of e1 despite order 1 < 2; e3 (arrived) dropped
});

test('radar: epoch grouping excludes sidequests and progress counts milestones', () => {
  const r = radarData(fixture());
  const e1 = r.find(x => x.id === 'e1');
  const e2 = r.find(x => x.id === 'e2');
  assert.equal(e1.done, 1);   // c1
  assert.equal(e1.active, 3); // c2, c_ms, c_none
  assert.equal(e1.milestonesMet, 0);
  assert.equal(e1.milestoneTotal, 3);
  assert.equal(e1.pct, 0);
  assert.equal(e2.done, 1);   // c3 (c5 sidequest excluded)
  assert.equal(e2.active, 1); // c4 (c5 sidequest excluded)
  assert.equal(e2.milestonesMet, 0);
  assert.equal(e2.milestoneTotal, 1);
  assert.equal(e2.pct, 0);
  assert.equal(e1.doneArchivedHint, null);
  assert.equal(e2.doneArchivedHint, null);
});

test('radar: burndown buckets by day, filters non-done/no-phase/out-of-window/wrong-action', () => {
  const r = radarData(fixture());
  const e1 = r.find(x => x.id === 'e1');
  const e2 = r.find(x => x.id === 'e2');
  assert.equal(e1.burndown.length, 30);
  assert.equal(e2.burndown.length, 30);
  const e1Today = e1.burndown.find(d => d.day === dayKey(iso(0)));
  const e2Day1 = e2.burndown.find(d => d.day === dayKey(iso(1)));
  assert.equal(e1Today.n, 1);
  assert.equal(e2Day1.n, 1);
  const totalE1 = e1.burndown.reduce((a, d) => a + d.n, 0);
  const totalE2 = e2.burndown.reduce((a, d) => a + d.n, 0);
  assert.equal(totalE1, 1); // only the day0 tick — the day40 tick is out of window, c2 isn't done
  assert.equal(totalE2, 1); // only the day1 tick — the title-only note doesn't count
});

test('radar: milestone stall days — touched, untouched, no linked cards', () => {
  const r = radarData(fixture());
  const e1 = r.find(x => x.id === 'e1');
  const e2 = r.find(x => x.id === 'e2');
  const m1 = e1.milestones.find(m => m.id === 'm1');
  const m2 = e1.milestones.find(m => m.id === 'm2');
  const m3 = e2.milestones.find(m => m.id === 'm3');
  assert.equal(m1.total, 1); assert.equal(m1.done, 0);
  assert.equal(m1.stalledDays, 10); // its only linked card (c_ms) was last touched 10 days ago
  assert.equal(m2.stalledDays, null); // c_none is linked but never referenced by an event
  assert.equal(m3.total, 0);
  assert.equal(m3.stalledDays, null); // no linked cards at all
});

test('radar: empty store yields empty list, no throw', () => {
  const r = radarData({ meta: {}, epochs: [], milestones: [], cards: [], events: [] });
  assert.deepEqual(r, []);
});
