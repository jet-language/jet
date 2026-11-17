// A card past `criteriaSoftLimit` criteria takes a logged reason to grow, and
// `tower card split` moves rows onto a new card; status, chain and the board
// carry the open cards and criteria with today's change.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../App/store.mjs';
import { readJSON, writeJSON } from '../App/paths.mjs';

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
const json = (r) => JSON.parse(r.stdout);

const rows = (n, prefix = 'row') => Array.from({ length: n }, (_, i) => `${prefix} ${i + 1}`);

function board(config = {}) {
  const data = mkdtempSync(join(tmpdir(), 'tower-limits-'));
  writeJSON(join(data, 'tower.json'), db.empty('Limits'));
  writeJSON(join(data, 'config.json'), { project: 'Limits', ...config });
  const st = db.openStore(data);
  st.mutate((s, cfg) => {
    db.addCard(s, { title: 'Parser', criteria: rows(6), overLimit: 'seeded as one card', by: 'seed' }, cfg);
    db.addCard(s, { title: 'Checker', criteria: rows(2, 'check'), by: 'seed' }, cfg);
  });
  return { data, st };
}

test('growing a card past the soft limit needs a reason, which is logged and listed by lint --limits', () => {
  const { data, st } = board();
  const refused = cli(data, 'card', 'criteria', '#1', '--add', 'row 7', '--by', 'agent-a', '--json');
  assert.equal(refused.status, 1);
  assert.equal(json(refused).error.code, 'E_OVER_LIMIT');
  assert.match(json(refused).error.message, /card #1 would hold 7 criteria, over the soft limit of 6 — split it with `tower card split`, or give --over-limit "reason"/);
  assert.equal(st.load().cards[0].criteria.length, 6, 'a refused add changes nothing');
  assert.equal(json(cli(data, 'lint', '--limits', '--json')).length, 0);

  const added = cli(data, 'card', 'criteria', '#1', '--add', 'row 7', '--over-limit', 'one grammar pass', '--by', 'agent-a', '--json');
  assert.equal(added.status, 0, added.stderr);
  const logged = st.load().events.find(e => e.action === 'card.over-limit');
  assert.equal(logged.by, 'agent-a');
  assert.equal(logged.note, '7 criteria, soft limit 6: one grammar pass');

  const lint = cli(data, 'lint', '--limits', '--json');
  assert.equal(lint.status, 0, 'over-limit cards are warnings');
  assert.deepEqual(json(lint).map(f => [f.rule, f.ref, f.count, f.limit, f.reason]),
    [['criteria-over-limit', '#1', 7, 6, 'one grammar pass']]);
  assert.match(cli(data, 'lint', '--limits').stdout, /#1 "Parser" holds 7 criteria, over the soft limit of 6 — kept for: one grammar pass/);
  assert.equal(cli(data, 'lint', '--limits', '--evidence', '--json').status, 1, 'one sweep at a time');

  // A card already over the limit is not blocked until it grows again.
  const edited = cli(data, 'card', 'criteria', '#1', '--edit', '1', '--text', 'row one', '--reason', 'wording', '--by', 'agent-a', '--json');
  assert.equal(edited.status, 0, edited.stderr);
  assert.equal(json(cli(data, 'card', 'criteria', '#1', '--add', 'row 8', '--by', 'agent-a', '--json')).error.code, 'E_OVER_LIMIT');
});

test('card add, card update and a move onto a full card are held to the limit too', () => {
  const { data, st } = board();
  writeFileSync(join(data, 'big.json'), JSON.stringify({ criteria: rows(7) }));
  const add = cli(data, 'card', 'add', '--title', 'Big', '--file', join(data, 'big.json'), '--by', 'agent-a', '--json');
  assert.equal(json(add).error.code, 'E_OVER_LIMIT');
  assert.match(json(add).error.message, /the new card would hold 7 criteria/);
  const forced = cli(data, 'card', 'add', '--title', 'Big', '--file', join(data, 'big.json'), '--over-limit', 'tracked as one', '--by', 'agent-a', '--json');
  assert.equal(forced.status, 0, forced.stderr);

  const update = cli(data, 'card', 'update', '#2', '--file', join(data, 'big.json'), '--by', 'agent-a', '--json');
  assert.equal(json(update).error.code, 'E_OVER_LIMIT');

  const move = cli(data, 'card', 'criteria', '#2', '--move', '1', '--to', '#1', '--by', 'agent-a', '--json');
  assert.equal(json(move).error.code, 'E_OVER_LIMIT');
  assert.match(json(move).error.message, /card #1 would hold 7 criteria/);
  const moved = cli(data, 'card', 'criteria', '#2', '--move', '1', '--to', '#1', '--over-limit', 'same parser pass', '--by', 'agent-a', '--json');
  assert.equal(moved.status, 0, moved.stderr);
  const notes = st.load().events.filter(e => e.action === 'card.over-limit').map(e => e.note);
  assert.deepEqual(notes, ['7 criteria, soft limit 6: same parser pass', '7 criteria, soft limit 6: tracked as one']);
});

test('config criteriaSoftLimit sets the limit and must be a positive whole number', () => {
  const { data } = board({ criteriaSoftLimit: 2 });
  const refused = cli(data, 'card', 'criteria', '#2', '--add', 'check 3', '--by', 'agent-a', '--json');
  assert.match(json(refused).error.message, /would hold 3 criteria, over the soft limit of 2/);
  writeJSON(join(data, 'config.json'), { project: 'Limits', criteriaSoftLimit: 0 });
  const bad = cli(data, 'card', 'criteria', '#2', '--add', 'check 3', '--by', 'agent-a', '--json');
  assert.equal(json(bad).error.code, 'E_INVALID');
  assert.match(json(bad).error.message, /criteriaSoftLimit must be a positive whole number/);
});

test('card split moves open rows onto a new card in the same epoch, linked both ways', () => {
  const { data, st } = board();
  st.mutate((s, cfg, history) => { db.meetCriterion(s, '#1', 2, { evidence: 'parsed in the suite', by: 'agent-a', kind: 'obsolete' }, cfg, history); });
  for (const [criteria, message] of [['2', /criterion #2 on card #1 is met; only an open criterion moves/],
    ['9', /no criterion #9 on card #1/], ['3,3', /criterion #3 is listed twice/], ['x', /split needs --criteria N,N/]]) {
    const r = cli(data, 'card', 'split', '#1', '--criteria', criteria, '--title', 'Back half', '--by', 'agent-a', '--json');
    assert.equal(r.status, 1, criteria);
    assert.match(json(r).error.message, message);
  }
  assert.equal(st.load().cards.length, 2, 'a refused split writes nothing');
  assert.match(cli(data, 'card', 'split', '#1', '--criteria', '4', '--by', 'agent-a').stderr, /--title/);

  const r = cli(data, 'card', 'split', '#1', '--criteria', '4,5,6', '--title', 'Back half', '--by', 'agent-a', '--json');
  assert.equal(r.status, 0, r.stderr);
  const result = json(r);
  assert.equal(result.card.num, 3);
  assert.deepEqual(result.moved, [{ from: 4, to: 1 }, { from: 5, to: 2 }, { from: 6, to: 3 }]);
  const s = st.load();
  const [parser, , half] = s.cards;
  assert.deepEqual([half.title, half.epoch, half.track, half.body], ['Back half', parser.epoch, parser.track, 'Split from #1.']);
  assert.deepEqual(half.criteria.map(i => [i.n, i.status, i.text, i.movedFrom]),
    [[1, 'open', 'row 4', { card: 1, n: 4 }], [2, 'open', 'row 5', { card: 1, n: 5 }], [3, 'open', 'row 6', { card: 1, n: 6 }]]);
  assert.deepEqual(parser.criteria.slice(3).map(i => [i.status, i.closure, i.movedTo]),
    [['met', 'duplicate', { card: 3, n: 1 }], ['met', 'duplicate', { card: 3, n: 2 }], ['met', 'duplicate', { card: 3, n: 3 }]]);
  assert.equal(db.liveCriteriaCount(parser), 3, 'moved rows no longer count toward the limit');
  const split = s.events.find(e => e.action === 'card.split');
  assert.deepEqual([split.by, split.refs, split.note], ['agent-a', [parser.id, half.id], 'c4, c5, c6 → #3 Back half']);
  const history = json(cli(data, 'card', 'history', '#3', '--json'));
  assert.ok(history.events.some(e => e.action === 'card.split'), 'the new card\'s history shows the split');
  assert.match(cli(data, 'card', 'split', '#1', '--criteria', '1', '--title', 'Front', '--by', 'agent-a').stdout,
    /split c1 off card #1 into card #4 \(c1\)/);
});

test('apply runs card.split with the plan, and a split past the limit needs a reason', () => {
  const { data, st } = board();
  const plan = join(data, 'plan.json');
  writeFileSync(plan, JSON.stringify({ ops: [{ op: 'card.split', card: '#1', criteria: [1, 2], title: 'Lexer' }] }));
  const applied = cli(data, 'apply', '--file', plan, '--by', 'planner', '--json');
  assert.equal(applied.status, 0, applied.stderr);
  const lexer = st.load().cards.find(c => c.title === 'Lexer');
  assert.deepEqual(lexer.criteria.map(i => i.text), ['row 1', 'row 2']);

  st.mutate((s, cfg) => db.addCard(s, { title: 'Huge', criteria: rows(8), overLimit: 'seeded large', by: 'seed' }, cfg));
  writeFileSync(plan, JSON.stringify({ ops: [{ op: 'card.split', card: '#4', criteria: '1,2,3,4,5,6,7', title: 'Most' }] }));
  assert.match(cli(data, 'apply', '--file', plan, '--by', 'planner').stderr, /the split-off card would hold 7 criteria/);
  writeFileSync(plan, JSON.stringify({ ops: [{ op: 'card.split', card: '#4', criteria: '1,2,3,4,5,6,7', title: 'Most', overLimit: 'one pass' }] }));
  assert.equal(cli(data, 'apply', '--file', plan, '--by', 'planner', '--json').status, 0);
});

test('status, chain and the board carry open cards and criteria with today\'s change', () => {
  const { data, st } = board();
  // The board was created today, so everything on it opened today.
  const status = json(cli(data, 'status', '--json')).counts.today;
  assert.deepEqual([status.openCards, status.openCardsDelta, status.openCriteria, status.openCriteriaDelta], [2, 2, 8, 8]);

  // A board last written on an earlier day has not changed today ...
  const file = join(data, 'tower.json');
  const raw = readJSON(file);
  raw.meta.dayStart = { date: '2000-01-01', openCards: 0, openCriteria: 0, epochs: {} };
  writeJSON(file, raw);
  const quiet = json(cli(data, 'status', '--json')).counts.today;
  assert.deepEqual([quiet.openCards, quiet.openCardsDelta, quiet.openCriteriaDelta], [2, 0, 0]);

  // ... until today's first write records where the day started.
  assert.equal(cli(data, 'card', 'add', '--title', 'Emitter', '--by', 'agent-a', '--json').status, 0);
  assert.equal(cli(data, 'card', 'criteria', '#2', '--add', 'check 3', '--by', 'agent-a', '--json').status, 0);
  st.mutate((s, cfg, history) => { db.meetCriterion(s, '#1', 1, { evidence: 'parsed in the suite', by: 'agent-a', kind: 'obsolete' }, cfg, history); });
  const day = json(cli(data, 'status', '--json')).counts.today;
  assert.deepEqual([day.openCards, day.openCardsDelta, day.openCriteria, day.openCriteriaDelta], [3, 1, 8, 0]);
  assert.deepEqual(day.epochs.e1, { openCards: 3, openCriteria: 8, openCardsDelta: 1, openCriteriaDelta: 0 });
  assert.match(cli(data, 'status').stdout, /OPEN TODAY +3 \+1 cards · 8 criteria since the start of \d{4}-\d{2}-\d{2}/);

  const chain = json(cli(data, 'chain', '--json')).find(e => e.id === 'e1');
  assert.deepEqual([chain.openCards, chain.openCardsDelta, chain.openCriteriaDelta], [3, 1, 0]);
  assert.match(cli(data, 'chain').stdout, /E1 .*3 cards +8 open .*\(today \+1 cards\)/);

  assert.deepEqual(db.projectBoard(st.load()).counts.today, day);
});
