import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { once } from 'node:events';
import * as db from '../app/store.mjs';
import { DEFAULTS } from '../app/config.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';
import { ownerDecisions, ownerVerifyQueue } from '../app/ui/board-state.js';
import { renderNowReports } from '../app/ui/now.js';

const reviewPasses = {
  beginner: 'Fresh agent: reader-1. Skill: rli5. The beginner pass tested the complete ballot.',
  adversarial: 'Author model family: family-a. Adversarial model family: family-b. Fresh agent: reader-2.',
};
function board() {
  const s = db.empty('Drafts');
  const card = db.addCard(s, { title: 'Review work', by: 'agent' }, DEFAULTS);
  db.addDecision(s, { cardId: card.id, id: 'D-DRAFT', title: 'Unreviewed choice', draft: true, reviewPasses, by: 'agent' });
  s.decisions.push({ id: 'D-READY', cardId: card.id, title: 'Ready choice', status: 'open', draft: false });
  return { s, card };
}
const draftError = e => e.code === 'E_DRAFT' && /D-DRAFT.*draft ballot.*marked ready/.test(e.message);

test('owner projections hide drafts, associated activity/questions and Now links without changing agent data', () => {
  const { s, card } = board();
  s.questions.push({ id: 'q-draft', cardId: card.id, decisionId: 'D-DRAFT', status: 'open' });
  const links = [{ decision: 'D-DRAFT', label: 'Unreviewed choice' }, { decision: 'D-READY' }];
  db.postBriefing(s, { title: 'Progress', body: 'Work continues.', by: 'agent', sections: [{ title: 'Choices', body: 'Review pending.', links }] });
  db.postStatus(s, { by: 'agent', snapshot: {
    milestones: [{ title: 'Shipping', state: 'Active', progress: {}, links }],
    workstreams: [{ title: 'Implementation', state: 'Active', workers: ['agent'], blockers: [], links }],
  } });
  const before = JSON.stringify(s);
  for (const p of [db.project(s), db.projectBoard(s)]) {
    assert.deepEqual(p.decisions.map(d => d.id), ['D-READY']);
    assert.equal(p.counts.decide, 1);
    assert.equal(p.counts.forYou, 1);
    assert.equal(p.questions.length, 0);
    assert.equal(p.events.some(e => e.ref === 'D-DRAFT'), false);
  }
  const index = db.projectBoard(s);
  assert.doesNotMatch(renderNowReports(index), /D-DRAFT|Unreviewed choice/);
  assert.match(renderNowReports(index), /D-READY/);
  assert.deepEqual(db.projectCard(s, card.id).decisions.map(d => d.id), ['D-READY']);
  assert.equal(db.projectCard(s, card.id).clearance.total, 1);
  assert.equal(JSON.stringify(s), before, 'owner projections never modify the stored ballot');
  assert.equal(db.buildBrief(s, card.id).decisions.find(d => d.id === 'D-DRAFT').draft, true);

  const history = { ...db.emptyHistory(), cards: [{ ...card, phase: 'done' }], decisions: s.decisions };
  const live = db.empty('Archived');
  assert.deepEqual(db.projectCard(live, card.id, history).decisions.map(d => d.id), ['D-READY']);
  assert.doesNotMatch(JSON.stringify(db.projectClosed(live, null, history)), /D-DRAFT/);
});

test('draft acceptance ballots never enter Now or its owner-check count, even in stale client data', () => {
  const { s, card } = board();
  card.phase = 'verify'; card.needsAcceptance = true;
  s.decisions = [{ id: `D-ACCEPT-${card.num}`, cardId: card.id, group: 'acceptance', status: 'open', draft: true,
    visualMedia: [{ kind: 'image', path: 'screen.png' }] }];
  card.decisions = s.decisions;
  assert.deepEqual(ownerDecisions(card.decisions), []);
  assert.deepEqual(ownerVerifyQueue([card]), []);
  assert.deepEqual(ownerVerifyQueue(db.projectBoard(s).cards), []);
  assert.equal(db.projectBoard(s).counts.forYou, 0);
  assert.equal(db.projectBoard(s).cards[0].decisions, undefined);
  assert.throws(() => db.createAcceptanceResolver()(s, s.decisions[0].id, 'bounce', '', {}), e => e.code === 'E_DRAFT');
});

test('drafts refuse ratification, quoted ratification and superseding verdicts before any mutation', () => {
  const { s } = board();
  const before = JSON.stringify(s);
  for (const [by, quote] of [['owner', undefined], ['agent', 'Owner says choose A']]) {
    assert.throws(() => db.ratify(s, 'D-DRAFT', 'A', '', by, quote), draftError);
  }
  assert.throws(() => db.mintVerdict(s, '#1', 'A', 'Ruling', 'owner', 'D-DRAFT'), draftError);
  assert.equal(JSON.stringify(s), before);
  const ruling = db.mintVerdict(s, '#1', 'A', 'Ruling', 'owner', 'D-READY');
  assert.equal(ruling.status, 'ratified');
});

test('owner delete confirmation excludes drafts and deleted drafts stay out of owner activity', () => {
  const { s, card } = board();
  const result = db.deleteCard(s, card.id, { decisions: ['D-READY'] }, { kind: 'owner-ui', session: 'test' });
  assert.deepEqual(result.decisions.map(d => d.id), ['D-READY']);
  assert.doesNotMatch(JSON.stringify(db.projectBoard(s)), /D-DRAFT|Unreviewed choice/);
  assert.ok(s.events.some(e => e.ref === 'D-DRAFT'), 'agent audit history is retained');
  const another = board().s;
  db.deleteDecision(another, 'D-DRAFT', 'agent');
  assert.doesNotMatch(JSON.stringify(db.projectBoard(another)), /D-DRAFT|Unreviewed choice/);
});

test('HTTP board/card and mutation state hide drafts; clearance, batch and verdict reject them; CLI can inspect them', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-drafts-'));
  const { s, card } = board();
  writeJSON(join(dir, 'tower.json'), s);
  const store = db.openStore(dir);
  const server = serve(store, 0, false);
  if (!server.listening) await once(server, 'listening');
  const base = `http://127.0.0.1:${server.address().port}`;
  const post = async (route, payload) => {
    const response = await fetch(`${base}/api/${route}`, { method: 'POST',
      headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify(payload) });
    return { status: response.status, body: await response.json() };
  };
  // Import this checkout's CLI directly: tower.mjs intentionally loads the main checkout.
  const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
  const cli = args => spawnSync(process.execPath, ['--input-type=module', '-e',
    `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, ...args],
    { encoding: 'utf8', env: { ...process.env, TOWER_DATA: dir } });
  try {
    for (const path of ['/api/state', `/api/card?id=${card.id}`, `/api/card?id=${card.id}&live=1`, '/api/closed']) {
      const response = await fetch(base + path);
      assert.equal(response.status, 200);
      assert.doesNotMatch(await response.text(), /D-DRAFT|Unreviewed choice/);
    }
    const changed = await post('card/update', { id: card.id, title: 'Still reviewing', by: 'agent' });
    assert.equal(changed.status, 200);
    assert.doesNotMatch(JSON.stringify(changed.body.state), /D-DRAFT|Unreviewed choice/);
    for (const [route, payload] of [
      ['clearance', { decisionId: 'D-DRAFT', outcome: 'A', by: 'owner' }],
      ['clearance/batch', { decisions: [{ decisionId: 'D-READY', outcome: 'A' }, { decisionId: 'D-DRAFT', outcome: 'A' }], by: 'owner' }],
      ['verdict', { id: card.id, outcome: 'A', supersedes: 'D-DRAFT', by: 'owner' }],
    ]) {
      const response = await post(route, payload);
      assert.equal(response.status, 400);
      assert.equal(response.body.error, 'E_DRAFT');
      assert.match(response.body.message, /draft ballot/);
    }
    assert.equal(store.load().decisions.find(d => d.id === 'D-READY').status, 'open', 'failed batch is atomic');
    const normal = cli(['decision', 'list', '--open', '--json']);
    assert.equal(normal.status, 0, normal.stderr);
    assert.deepEqual(JSON.parse(normal.stdout).map(d => d.id), ['D-READY']);
    const drafts = cli(['decision', 'list', '--open', '--drafts', '--json']);
    assert.equal(drafts.status, 0, drafts.stderr);
    assert.deepEqual(JSON.parse(drafts.stdout).map(d => d.id), ['D-DRAFT', 'D-READY']);
    const show = cli(['decision', 'show', 'D-DRAFT', '--json']);
    assert.equal(show.status, 0, show.stderr);
    assert.equal(JSON.parse(show.stdout).draft, true);
    const verdict = cli(['verdict', '#1', '--outcome', 'A', '--supersedes', 'D-DRAFT', '--by', 'owner']);
    assert.notEqual(verdict.status, 0);
    assert.match(verdict.stderr, /D-DRAFT.*draft ballot.*marked ready/);
    const readyVerdict = await post('verdict', { id: card.id, outcome: 'A', supersedes: 'D-READY', by: 'owner' });
    assert.equal(readyVerdict.status, 200, JSON.stringify(readyVerdict.body));
  } finally {
    server.close();
    await once(server, 'close');
    rmSync(dir, { recursive: true, force: true });
  }
});
