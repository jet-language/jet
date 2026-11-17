// Planning: epoch `order` is the chain that drives `next`, briefs and the
// radar; `tower closest` ranks open cards by open criteria; a criterion can
// wait on a card, another criterion or a decision.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../App/store.mjs';
import { writeJSON } from '../App/paths.mjs';
import { proveTargets } from '../App/proof.mjs';
import { closestCards, epochChain } from '../App/UI/board-state.js';

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });

// e1 is finished; the chain then runs e7, e5, e9 by `order`, not by id.
// #1 (e5, ready) and #2 (e7, planning) are the cards `next` must order.
function board() {
  const data = mkdtempSync(join(tmpdir(), 'tower-planning-'));
  writeJSON(join(data, 'tower.json'), db.empty('Planning'));
  const st = db.openStore(data);
  st.mutate((s, cfg) => {
    db.updateEpoch(s, 'e1', { status: 'done', order: 1 });
    db.addEpoch(s, { id: 'e9', name: 'Last', order: 30, status: 'planned' });
    db.addEpoch(s, { id: 'e5', name: 'Second', order: 20, status: 'planned' });
    db.addEpoch(s, { id: 'e7', name: 'First', order: 10, status: 'planned' });
    db.addCard(s, { title: 'Second epoch work', epoch: 'e5', phase: 'ready' }, cfg);
    db.addCard(s, { title: 'First epoch work', epoch: 'e7', phase: 'planning' }, cfg);
    db.addCard(s, { title: 'More first epoch work', epoch: 'e7', phase: 'planning' }, cfg);
  });
  return { data, st };
}

const crit = (st, ref, text, extra = {}) => st.mutate(s => db.addCriterion(s, ref, text, 'planner', extra)).result;

test('the chain follows epoch order: the first unfinished epoch is active with three agents, the next has one', () => {
  const { st } = board();
  crit(st, '#2', 'one');
  crit(st, '#2', 'two');
  crit(st, '#3', 'three', { blockedBy: '#2 c1' });
  crit(st, '#1', 'four');
  const proj = db.project(st.load());
  const chain = epochChain(proj.epochs, proj.cards);
  assert.deepEqual(chain.map(e => [e.id, e.role, e.staff]),
    [['e1', 'finished', 0], ['e7', 'active', 3], ['e5', 'next', 1], ['e9', 'later', 0]]);
  assert.deepEqual(chain.find(e => e.id === 'e7'), {
    id: 'e7', label: 'E7', name: 'First', status: 'planned', role: 'active', staff: 3,
    openCards: 2, openCriteria: 2, blockedCriteria: 1,
  });
  const radar = db.radarData(st.load());
  assert.deepEqual(radar.map(r => [r.id, r.role, r.staff, r.openCriteria]), [['e7', 'active', 3, 2], ['e5', 'next', 1, 1], ['e9', 'later', 0, 0]]);
});

test('next picks the active epoch first even when a later epoch has further-along work', () => {
  const { data, st } = board();
  const s = st.load();
  assert.deepEqual(db.nextCards(s).map(c => c.num), [2, 3, 1]);
  assert.deepEqual(db.nextCards(s, { scope: 'burndown' }).map(c => c.num), [2, 3]);
  // ready-across stays lane first: the ready card leads, then chain order.
  assert.deepEqual(db.nextCards(s, { scope: 'ready-across' }).map(c => c.num), [1, 2, 3]);
  assert.equal(db.buildBrief(s, '#2').card.epoch.role, 'active');
  const run = cli(data, 'next');
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /next up — E7 first \(3 agents\), then E5 \(1 agent\)/);
});

test('tower chain names the active epoch, its open criteria and the next epoch', () => {
  const { data, st } = board();
  crit(st, '#2', 'one');
  crit(st, '#2', 'two', { blockedBy: '#1' });
  const run = cli(data, 'chain');
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /^active E7: 1 open criteria \(\+1 blocked\) on 2 cards · next E5$/m);
  assert.match(run.stdout, /E7 +active · 3 agents +2 cards +1 open 1 blocked  First/);
  const json = JSON.parse(cli(data, 'chain', '--json').stdout);
  assert.deepEqual(json.map(e => e.role), ['finished', 'active', 'next', 'later']);
});

test('closest ranks open cards by open criteria, fewest first, and counts blocked criteria apart', () => {
  const { data, st } = board();
  crit(st, '#1', 'a');
  crit(st, '#2', 'b');
  crit(st, '#2', 'c', { blockedBy: '#1 c1' });
  crit(st, '#3', 'd');
  crit(st, '#3', 'e');
  const proj = db.project(st.load());
  assert.deepEqual(closestCards(proj.cards, proj.epochs).map(r => [r.card.num, r.crit.open, r.crit.blocked]),
    [[1, 1, 0], [2, 1, 1], [3, 2, 0]]);
  assert.deepEqual(closestCards(proj.cards, proj.epochs, { epoch: 'e7', limit: 1 }).map(r => r.card.num), [2]);
  const json = JSON.parse(cli(data, 'closest', '--epoch', 'E7', '--json').stdout);
  assert.deepEqual(json.map(r => [r.num, r.open, r.blocked, r.total]), [[2, 1, 1, 2], [3, 2, 0, 2]]);
  const text = cli(data, 'closest', '--limit', '2');
  assert.equal(text.status, 0, text.stderr);
  assert.match(text.stdout, /#1 +E5 +1 open +Second epoch work/);
  assert.match(text.stdout, /#2 +E7 +1 open +1 blocked +First epoch work/);
  assert.doesNotMatch(text.stdout, /#3/);
  assert.match(cli(data, 'closest', '--epoch', 'e404').stderr, /no epoch e404/);
});

test('a criterion takes a blocker on add and edit, refuses dead or cyclic ones, and unblocks by hand', () => {
  const { data, st } = board();
  crit(st, '#1', 'base');
  let run = cli(data, 'card', 'criteria', '#2', '--add', 'waits', '--blocked-by', '1c1', '--by', 'planner');
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /added criterion #1 to card #2, waiting on #1 c1/);
  assert.equal(st.load().cards.find(c => c.num === 2).criteria[0].blockedBy, '#1 c1');
  crit(st, '#2', 'later');
  run = cli(data, 'card', 'criteria', '#2', '--edit', '2', '--blocked-by', '#3', '--by', 'planner');
  assert.equal(run.status, 0, run.stderr);
  assert.match(cli(data, 'card', 'criteria', '#2', '--list').stdout, /#2 \[open\] later  blocked by #3/);
  const refuse = (ref, text, blockedBy, pattern) =>
    assert.throws(() => st.mutate(s => db.addCriterion(s, ref, text, 'planner', { blockedBy })), pattern);
  assert.throws(() => st.mutate(s => db.editCriterion(s, '#1', 1, { blockedBy: '#2 c1', by: 'planner' })),
    /criterion #1 c1 would wait on itself through #2 c1/);
  refuse('#1', 'self', '#1', /cannot wait on its own card #1/);
  refuse('#1', 'nowhere', '#404', /no live card #404/);
  refuse('#1', 'garbage', 'soon', /--blocked-by takes '#M', '#M cK' or a decision id/);
  refuse('#1', 'ballot', 'D-NONE', /no decision D-NONE/);
  assert.match(cli(data, 'card', 'criteria', '#2', '--blocked-by', '#3', '--by', 'planner').stderr, /--blocked-by goes with --add or --edit n/);
  run = cli(data, 'card', 'criteria', '#2', '--unblock', '2', '--reason', 'no longer needed', '--by', 'planner');
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /criterion #2 on card #2 no longer waits on #3/);
  assert.equal(st.load().cards.find(c => c.num === 2).criteria[1].blockedBy, undefined);
  assert.match(st.load().events.find(e => e.action === 'card.criteria-unblock').note, /#2: was blocked by #3; no longer needed/);
  assert.match(cli(data, 'card', 'criteria', '#2', '--unblock', '2', '--by', 'planner').stderr, /criterion #2 on card #2 is not blocked/);
});

test('a resolved blocker is reported as unblocked on the next load, and prove skips blocked rows', () => {
  const { st } = board();
  crit(st, '#1', 'base', { proof: 'jet test Compiler --profile cases --filter parser/a' });
  crit(st, '#2', 'waits', { blockedBy: '#1 c1', proof: 'jet test Compiler --profile cases --filter parser/b' });
  crit(st, '#3', 'waits on the card', { blockedBy: '#1' });
  assert.deepEqual(proveTargets(st.load(), {}).map(({ card, item }) => [card.num, item.n]), [[1, 1]]);
  st.mutate(s => { const row = s.cards.find(c => c.num === 1).criteria[0]; row.status = 'met'; row.metBy = 'builder'; });
  const s = st.load();
  const waited = s.cards.find(c => c.num === 2).criteria[0];
  assert.equal(waited.blockedBy, undefined);
  assert.equal(waited.unblockedFrom, '#1 c1');
  assert.equal(s.cards.find(c => c.num === 3).criteria[0].blockedBy, '#1');
  assert.deepEqual(proveTargets(s, {}).map(({ card, item }) => [card.num, item.n]), [[2, 1]]);
  // A decision blocker ends when the decision leaves `open`.
  st.mutate(s => s.decisions.push({ id: 'D-PLAN1', cardId: s.cards[0].id, title: 'Pick one', status: 'open', options: [], draft: false, created: '2026-01-01' }));
  crit(st, '#3', 'waits on a ballot', { blockedBy: 'D-PLAN1' });
  assert.throws(() => crit(st, '#3', 'too late', { blockedBy: 'D-NONE' }), /no decision D-NONE/);
  st.mutate(s => { s.decisions[0].status = 'ratified'; });
  assert.equal(st.load().cards.find(c => c.num === 3).criteria[1].unblockedFrom, 'D-PLAN1');
});
