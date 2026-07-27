import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, laneOf, TowerError, nextCards, project } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-test-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

import * as db from '../app/store.mjs';

test('card add defaults: #1, planning, plan lane — no greenlight step', () => {
  const st = fresh();
  const { result: c } = st.mutate((s, cfg) => db.addCard(s, { title: 'Do the thing' }, cfg));
  assert.equal(c.num, 1);
  assert.equal(c.phase, 'planning');
  assert.equal(laneOf(c, [], [c]).lane, 'plan');
});

test('a legacy triage-phase card (pre-#516 data) is treated as planning by lane derivation, no data rewrite needed', () => {
  const c = { id: 'c1', phase: 'triage', blockedBy: [] };
  assert.equal(laneOf(c, [], [c]).lane, 'plan');
});

test('lane derivation follows phases and decisions', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  let s = st.load();
  assert.equal(db.laneOf(db.findCard(s, '#1'), s.decisions, s.cards).lane, 'plan');

  st.mutate((s2) => db.addDecision(s2, { cardId: '#1', id: 'D-T1', title: 'Pick one',
    gist: 'g', lesson: 'teach from zero', story: 's', inWild: 'w', rec: 'A',
    recommendation: { why: 'A wins here.', whyNot: [{ key: 'B', reason: 'B loses the needed behavior.' }], tradeoff: 'A adds one visible step.' },
    hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
    options: [{ key: 'A', name: 'a', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'b', detail: 'B is brief.', code: 'b()' }] }));
  s = st.load();
  assert.equal(db.laneOf(db.findCard(s, '#1'), s.decisions, s.cards).lane, 'decide');

  st.mutate((s2) => db.ratify(s2, 'D-T1', 'A', 'looks right', 'owner'));
  s = st.load();
  assert.equal(s.decisions[0].status, 'ratified');
  assert.equal(db.laneOf(db.findCard(s, '#1'), s.decisions, s.cards).lane, 'plan');

  st.mutate((s2, cfg) => db.updateCard(s2, '#1', { plan: 'the plan', phase: 'ready' }, cfg));
  s = st.load();
  assert.equal(db.laneOf(db.findCard(s, '#1'), s.decisions, s.cards).lane, 'implement');
});

test('deciding card auto-advances when last decision ratifies', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'deciding', plan: 'plan' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-X', title: 't',
    gist: 'g', lesson: 'teach from zero', story: 's', inWild: 'w', rec: 'B',
    recommendation: { why: 'B wins here.', whyNot: [{ key: 'A', reason: 'A loses the needed behavior.' }], tradeoff: 'B adds one visible step.' },
    hybrid: { result: 'B', synthesis: 'B combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Borrow its clear names.' }, { key: 'B', aspect: 'B is brief.', use: 'Keep it.' }] },
    options: [{ key: 'A', name: 'a', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'b', detail: 'B is brief.', code: 'b()' }] }));
  st.mutate((s) => db.ratify(s, 'D-X', 'B', null, 'owner'));
  const s = st.load();
  assert.equal(db.findCard(s, '#1').phase, 'ready');
});

test('validation: bad enums and dangling refs are rejected, state unchanged', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const revBefore = st.load().meta.rev;
  assert.throws(() => st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'bogus' }, cfg)), TowerError);
  assert.throws(() => st.mutate((s, cfg) => db.updateCard(s, '#1', { epoch: 'nope' }, cfg)), TowerError);
  assert.throws(() => st.mutate((s, cfg) => db.addCard(s, { title: '' }, cfg)), TowerError);
  assert.throws(() => st.mutate((s) => db.addQuestion(s, { cardId: '#99', text: 'hi' })), TowerError);
  assert.equal(st.load().meta.rev, revBefore, 'failed mutations must not bump rev');
});

test('optimistic concurrency: expectRev mismatch throws E_CONFLICT', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const rev = st.load().meta.rev;
  st.mutate((s, cfg) => db.updateCard(s, '#1', { title: 'B' }, cfg), { expectRev: rev });
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { title: 'C' }, cfg), { expectRev: rev }),
    (e) => e.code === 'E_CONFLICT');
});

test('claims: second agent bounces, release frees', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  assert.throws(() => st.mutate((s) => db.claimCard(s, '#1', 'agent-2')), (e) => e.code === 'E_CLAIMED');
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1')); // re-claim by holder is fine
  st.mutate((s) => db.releaseCard(s, '#1', 'agent-1'));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-2'));
  assert.equal(st.load().cards[0].assignee, 'agent-2');
});

test('expired claims do not block selection or takeover', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  st.mutate((s) => { s.cards[0].claimedAt = '2000-01-01T00:00:00.000Z'; });

  const s = st.load();
  assert.equal(nextCards(s, { agent: 'agent-2' })[0].num, 1);
  st.mutate((s2) => db.claimCard(s2, '#1', 'agent-2'));
  assert.equal(st.load().cards[0].assignee, 'agent-2');
});

test('card writes renew active claims and terminal phases clear them', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  st.mutate((s) => { s.cards[0].claimedAt = '2000-01-01T00:00:00.000Z'; });
  st.mutate((s, cfg) => db.updateCard(s, '#1', { title: 'B', by: 'agent-1' }, cfg));
  assert.notEqual(st.load().cards[0].claimedAt, '2000-01-01T00:00:00.000Z');

  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  assert.equal(st.load().cards[0].assignee, null);
  assert.equal(st.load().cards[0].claimedAt, undefined);
});

test('milestones: cards drive progress and completion; delete unlinks', () => {
  const st = fresh();
  st.mutate((s) => db.addEpoch(s, { id: 'e1', name: 'One' }));
  const { result: m } = st.mutate((s) => db.addMilestone(s, { epochId: 'e1', title: 'MVP' }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', epoch: 'e1', milestoneId: m.id, phase: 'done' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'B', epoch: 'e1', milestoneId: m.id }, cfg));
  let proj = project(st.load());
  assert.deepEqual(proj.milestones[0].progress, { total: 2, done: 1, met: false });
  st.mutate((s, cfg) => db.updateCard(s, '#2', { phase: 'done', by: 'agent-1' }, cfg));
  proj = project(st.load());
  assert.deepEqual(proj.milestones[0].progress, { total: 2, done: 2, met: true });
  assert.equal(st.load().milestones[0].status, 'met');
  st.mutate((s, cfg) => db.updateCard(s, '#2', { phase: 'building', by: 'agent-1' }, cfg));
  assert.equal(st.load().milestones[0].status, 'open');
  st.mutate((s, cfg) => db.updateCard(s, '#2', { phase: 'done', by: 'agent-1' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { milestoneId: null, by: 'agent-1' }, cfg));
  assert.equal(st.load().milestones[0].status, 'met', 'remaining linked card is done');
  st.mutate((s, cfg) => db.updateCard(s, '#2', { milestoneId: null, by: 'agent-1' }, cfg));
  assert.equal(st.load().milestones[0].status, 'open', 'empty milestone reopens');
  assert.deepEqual(project(st.load()).milestones[0].progress, { total: 0, done: 0, met: false });
  st.mutate((s) => db.deleteMilestone(s, m.id, 'owner'));
  const s = st.load();
  assert.equal(s.milestones.length, 0);
  assert.equal(s.cards[0].milestoneId, null);
});

test('milestone progress deduplicates live and archived copies by card id', () => {
  const milestone = { id: 'm-1', status: 'open' };
  const live = [{ id: 'c-1', milestoneId: 'm-1', phase: 'building' }];
  const history = [
    { id: 'c-1', milestoneId: 'm-1', phase: 'done' },
    { id: 'c-2', milestoneId: 'm-1', phase: 'done' },
  ];
  assert.deepEqual(db.milestoneProgress(milestone, live, history), { total: 2, done: 1, met: false });
});

test('milestones: cards cannot cross epochs and sidequests stay unlinked', () => {
  const st = fresh();
  st.mutate((s) => {
    db.addEpoch(s, { id: 'e1', name: 'One' });
    db.addEpoch(s, { id: 'e2', name: 'Two' });
  });
  const m = st.mutate((s) => db.addMilestone(s, { epochId: 'e1', title: 'MVP' })).result;
  assert.throws(
    () => st.mutate((s, cfg) => db.addCard(s, { title: 'Wrong epoch', epoch: 'e2', milestoneId: m.id }, cfg)),
    /belongs to epoch e1/,
  );
  assert.throws(
    () => st.mutate((s, cfg) => db.addCard(s, { title: 'Sidequest', track: 'sidequest', epoch: 'e1', milestoneId: m.id }, cfg)),
    /sidequest cards cannot link to milestones/,
  );
  st.mutate((s, cfg) => db.addCard(s, { title: 'Scoped', epoch: 'e1', milestoneId: m.id }, cfg));
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { epoch: 'e2', by: 'agent-1' }, cfg)),
    /belongs to epoch e1/,
  );
  assert.throws(
    () => st.mutate((s) => db.updateMilestone(s, m.id, { epochId: 'e2' }, 'owner')),
    /milestone epoch is fixed/,
  );
});

test('nextCards ordering: verify > building > implement > plan, then workOrder', () => {
  const st = fresh();
  st.mutate((s, cfg) => {
    db.addCard(s, { title: 'plan-2', phase: 'planning', workOrder: 2 }, cfg);
    db.addCard(s, { title: 'build-1', phase: 'building', workOrder: 1 }, cfg);
    db.addCard(s, { title: 'verify-99', phase: 'verify', workOrder: 99 }, cfg);
    db.addCard(s, { title: 'implement-1', phase: 'ready', workOrder: 1 }, cfg);
    db.addCard(s, { title: 'no-order', phase: 'building' }, cfg);
    db.addCard(s, { title: 'frozen', phase: 'frozen', workOrder: 1 }, cfg);
  });
  const picks = nextCards(st.load()).map(c => c.title);
  assert.deepEqual(picks, ['verify-99', 'build-1', 'no-order', 'implement-1', 'plan-2']);
});

test('blockedBy gates the lane until blocker closes', () => {
  const st = fresh();
  st.mutate((s, cfg) => {
    const a = db.addCard(s, { title: 'A', phase: 'building' }, cfg);
    db.addCard(s, { title: 'B', phase: 'ready', blockedBy: [a.id] }, cfg);
  });
  let s = st.load();
  const lane = db.laneOf(db.findCard(s, '#2'), s.decisions, s.cards);
  assert.equal(lane.lane, 'blocked');
  assert.equal(lane.label, 'Blocked by #1');
  st.mutate((s2, cfg) => db.updateCard(s2, '#1', { phase: 'done' }, cfg));
  s = st.load();
  assert.equal(db.laneOf(db.findCard(s, '#2'), s.decisions, s.cards).lane, 'implement');
});

test('idea promote → planning card (agent-ready, no greenlight step), idea tagged', () => {
  const st = fresh();
  st.mutate((s) => db.addIdea(s, { text: 'shiny: make it glow', by: 'owner' }));
  const id = st.load().ideas[0].id;
  const { result: card } = st.mutate((s, cfg) => db.promoteIdea(s, id, {}, cfg));
  assert.equal(card.phase, 'planning');
  assert.equal(st.load().ideas[0].status, 'tagged');
});

test('events are recorded with attribution', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', by: 'agent-7' }, cfg));
  assert.equal(st.load().cards[0].updatedBy, 'agent-7');
  const e = st.load().events[0];
  assert.equal(e.action, 'card.add');
  assert.equal(e.by, 'agent-7');
});

test('backups rotate on writes', () => {
  const st = fresh();
  for (let i = 0; i < 3; i++) st.mutate((s, cfg) => db.addCard(s, { title: 'c' + i }, cfg));
  const files = readdirSync(join(st.dataDir, 'backups'));
  assert.ok(files.length >= 1 && files.length <= 20);
});

test('deleteCard cascades decisions/questions and clears blockedBy refs', () => {
  const st = fresh();
  st.mutate((s, cfg) => {
    const a = db.addCard(s, { title: 'A' }, cfg);
    db.addCard(s, { title: 'B', blockedBy: [a.id] }, cfg);
    db.addDecision(s, { cardId: a.id, title: 'd', draft: true });
    db.addQuestion(s, { cardId: a.id, text: 'q?' });
  });
  st.mutate((s) => db.deleteCard(s, '#1', { by: 'owner' }));
  const s = st.load();
  assert.equal(s.cards.length, 1);
  assert.equal(s.decisions.length, 0);
  assert.equal(s.questions.length, 0);
  assert.deepEqual(s.cards[0].blockedBy, []);
});
