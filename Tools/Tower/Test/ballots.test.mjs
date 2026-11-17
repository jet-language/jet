// Ballot stages: drafted → reviewed → posted → ratified, or rejected back to
// its author; `tower ballots` lists each stage with the work waiting on it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { once } from 'node:events';
import * as db from '../App/store.mjs';
import { today, writeJSON } from '../App/paths.mjs';
import { serve } from '../App/server.mjs';

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });

const reviewPasses = { adversarial: 'Author model family: family-a. Adversarial model family: family-b. Fresh agent: reader-2.' };

function board() {
  const data = mkdtempSync(join(tmpdir(), 'tower-ballots-'));
  writeJSON(join(data, 'tower.json'), db.empty('Ballots'));
  const st = db.openStore(data);
  st.mutate((s, cfg) => {
    db.addCard(s, { title: 'Pick a syntax' }, cfg);
    db.addCard(s, { title: 'Build on the choice' }, cfg);
    db.addDecision(s, { cardId: '#1', id: 'D-SYN1', title: 'Which syntax?', draft: true, ballotMode: 'full', reviewPasses, by: 'author-1' });
    db.addCriterion(s, '#2', 'uses the chosen syntax', 'author-1', { blockedBy: 'D-SYN1' });
  });
  return { data, st };
}

test('decision review and post move a ballot through its stages from the CLI', () => {
  const { data, st } = board();
  const early = cli(data, 'decision', 'post', 'D-SYN1', '--by', 'author-1');
  assert.notEqual(early.status, 0);
  assert.match(early.stderr, /D-SYN1 is drafted; decision post needs a reviewed ballot \(run decision review D-SYN1 --by <reviewer>\)/);
  const self = cli(data, 'decision', 'review', 'D-SYN1', '--by', 'author-1');
  assert.notEqual(self.status, 0);
  assert.match(self.stderr, /drafted by author-1; another agent must review it/);
  const noReason = cli(data, 'decision', 'review', 'D-SYN1', '--reject', '--by', 'reviewer-1');
  assert.notEqual(noReason.status, 0);
  assert.match(noReason.stderr, /--reject needs --reason/);
  const rejected = cli(data, 'decision', 'review', 'D-SYN1', '--reject', '--reason', 'Option B is missing its cost.', '--by', 'reviewer-1');
  assert.equal(rejected.status, 0, rejected.stderr);
  assert.match(rejected.stdout, /rejected D-SYN1: Option B is missing its cost\./);
  const list = cli(data, 'decision', 'list', '--drafts');
  assert.match(list.stdout, /D-SYN1\s+rejected\s+Which syntax\?/);
  const revised = cli(data, 'decision', 'update', 'D-SYN1', '--title', 'Which syntax, with costs?', '--by', 'author-1');
  assert.equal(revised.status, 0, revised.stderr);
  assert.match(revised.stdout, /updated decision D-SYN1 \(drafted\)/);
  const passed = cli(data, 'decision', 'review', 'D-SYN1', '--by', 'reviewer-1');
  assert.equal(passed.status, 0, passed.stderr);
  assert.match(passed.stdout, /reviewed D-SYN1 — post it with decision post D-SYN1/);
  const incomplete = cli(data, 'decision', 'post', 'D-SYN1', '--by', 'author-1');
  assert.notEqual(incomplete.status, 0);
  assert.match(incomplete.stderr, /ballot not ready — missing: /);
  const d = st.load().decisions[0];
  assert.deepEqual([db.decisionStage(d), d.draftedBy, d.reviewedBy, d.draft], ['reviewed', 'author-1', 'reviewer-1', true]);
  const ready = cli(data, 'decision', 'update', 'D-SYN1', '--ready', '--by', 'author-1');
  assert.notEqual(ready.status, 0, 'decision update --ready is gone; decision post replaces it');
  assert.match(ready.stderr, /unknown flag for `tower decision update`: --ready — run `tower help`/);
});

test('tower ballots lists every stage with the cards and criteria waiting on each ballot', () => {
  const { data, st } = board();
  st.mutate((s) => {
    db.addDecision(s, { cardId: '#2', id: 'D-OLD1', title: 'Settled law', draft: true, ballotMode: 'short' });
    Object.assign(s.decisions.find(d => d.id === 'D-OLD1'), { draft: false, status: 'ratified', outcome: 'A', ratifiedAt: today() });
  });
  const json = cli(data, 'ballots', '--json');
  assert.equal(json.status, 0, json.stderr);
  const stages = JSON.parse(json.stdout);
  assert.deepEqual(stages.map(x => [x.stage, x.count]), [['drafted', 1], ['reviewed', 0], ['posted', 0], ['ratified', 1], ['rejected', 0]]);
  assert.deepEqual(stages[0].ballots, [{ id: 'D-SYN1', title: 'Which syntax?', card: 1, draftedBy: 'author-1', reviewedBy: null, waiting: { cards: [], criteria: ['#2 c1'] } }]);
  assert.equal(stages[3].ballots[0].outcome, 'A');
  const text = cli(data, 'ballots');
  assert.equal(text.status, 0, text.stderr);
  assert.match(text.stdout, /^drafted \(1\)\n {2}D-SYN1 +#1 +Which syntax\? {2}\(author-1\)\n {4}waiting: #2 c1\nreviewed \(0\)\nposted \(0\)\nratified \(1\)\n {2}D-OLD1 +#2 +Settled law {2}\(→ A\)\nrejected \(0\)\n$/);
});

test('only a ratified ballot ends a criterion wait; a rejected one still holds it', () => {
  const { st } = board();
  st.mutate((s) => db.reviewDecision(s, 'D-SYN1', { by: 'reviewer-1', reject: true, reason: 'Too thin.' }));
  assert.equal(st.load().cards[1].criteria[0].blockedBy, 'D-SYN1');
  st.mutate((s) => { const d = s.decisions[0]; Object.assign(d, { draft: false, status: 'ratified', outcome: 'A' }); delete d.rejectReason; });
  const item = st.load().cards[1].criteria[0];
  assert.deepEqual([item.blockedBy, item.unblockedFrom], [undefined, 'D-SYN1']);
});

test('the server reviews and posts ballots through decision/review and decision/post', async () => {
  const { st } = board();
  const server = serve(st, 0, false);
  if (!server.listening) await once(server, 'listening');
  const post = async (route, payload) => {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/api/${route}`, { method: 'POST',
      headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify(payload) });
    return { status: response.status, body: await response.json() };
  };
  try {
    const early = await post('decision/post', { id: 'D-SYN1', by: 'author-1' });
    assert.deepEqual([early.status, early.body.error], [400, 'E_STAGE']);
    const reviewed = await post('decision/review', { id: 'D-SYN1', by: 'reviewer-1' });
    assert.equal(reviewed.status, 200, JSON.stringify(reviewed.body));
    assert.equal(st.load().decisions[0].reviewedBy, 'reviewer-1');
    const incomplete = await post('decision/post', { id: 'D-SYN1', by: 'author-1' });
    assert.deepEqual([incomplete.status, incomplete.body.error], [400, 'E_BALLOT']);
    assert.equal(st.load().decisions[0].draft, true, 'a refused post leaves the ballot a reviewed draft');
  } finally {
    server.close();
    await once(server, 'close');
  }
});
