// #6563 c3: a met or verified criterion, and a card moved to done, must name a
// commit reachable from dev or master, or declare a non-code closure kind.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { openStore, empty, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';

const TOWER = fileURLToPath(new URL('../tower.mjs', import.meta.url));
const gitEnv = { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' };
delete gitEnv.GIT_INDEX_FILE;
delete gitEnv.GIT_DIR;
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', env: gitEnv }).trim();

// A project repo with one commit on master, one more on dev, and one commit
// on no branch at all; the board lives in <repo>/.tower.
function project() {
  const root = mkdtempSync(join(tmpdir(), 'tower-durable-'));
  git(root, 'init', '-q', '-b', 'master');
  writeFileSync(join(root, 'a.txt'), 'a\n');
  git(root, 'add', 'a.txt');
  git(root, 'commit', '-q', '-m', 'on master');
  const onMaster = git(root, 'rev-parse', 'HEAD');
  git(root, 'checkout', '-q', '-b', 'dev');
  writeFileSync(join(root, 'b.txt'), 'b\n');
  git(root, 'add', 'b.txt');
  git(root, 'commit', '-q', '-m', 'on dev');
  const onDev = git(root, 'rev-parse', 'HEAD');
  const unlanded = git(root, 'commit-tree', `${onDev}^{tree}`, '-p', onDev, '-m', 'unlanded');
  const data = join(root, '.tower');
  mkdirSync(data);
  writeJSON(join(data, 'tower.json'), empty('Durable'));
  const st = openStore(data);
  st.mutate((s, cfg) => db.addCard(s, { title: 'Work' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Earlier copy of the work' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'the change works', 'planner'));
  return { root, data, st, onMaster, onDev, unlanded };
}

const meet = (st, evidence, kind) => st.mutate((s, cfg, history) =>
  db.meetCriterion(s, '#1', 1, { evidence, by: 'builder', kind }, cfg, history));
const refused = (pattern) => (e) => e instanceof TowerError && e.code === 'E_EVIDENCE' && pattern.test(e.message);

test('meet refuses evidence without a durable commit and accepts a commit on dev or master', () => {
  const { st, onMaster, onDev, unlanded } = project();
  assert.throws(() => meet(st, 'ran the tests, all green'), refused(/names no commit on dev or master/));
  assert.throws(() => meet(st, 'patch at $JET_SCRATCH_ROOT/lanes/X/fix.patch'), refused(/only scratch output/));
  assert.throws(() => meet(st, 'patch at /mnt/scratch/lanes/X/fix.patch'), refused(/only scratch output/));
  assert.throws(() => meet(st, 'log in ~/.cache/jet/scratch/X/run.log'), refused(/only scratch output/));
  assert.throws(() => meet(st, 'log in $JET_STATE_ROOT/scratch/X/run.log'), refused(/only scratch output/));
  assert.throws(() => meet(st, 'result in $JET_RUN_ROOT/land-results/X.result'), refused(/only scratch output/));
  assert.throws(() => meet(st, `built ${unlanded.slice(0, 10)}`), refused(new RegExp(`${unlanded.slice(0, 10)} is not on dev or master`)));
  assert.equal(st.load().cards[0].criteria[0].status, 'open', 'a refused meet changes nothing');

  meet(st, `landed ${onDev.slice(0, 10)}; log at $JET_SCRATCH_ROOT/lanes/X/run.log`);
  assert.equal(st.load().cards[0].criteria[0].status, 'met');
  const { result } = st.mutate((s, cfg, history) =>
    db.verifyCriterion(s, '#1', 1, { evidence: `reviewed ${onMaster}`, by: 'reviewer' }, cfg, history));
  assert.equal(result.status, 'verified');
});

test('verify re-checks the evidence it records', () => {
  const { st, onDev } = project();
  meet(st, `landed ${onDev.slice(0, 12)}`);
  assert.throws(
    () => st.mutate((s, cfg, history) => db.verifyCriterion(s, '#1', 1, { evidence: 'looked fine to me', by: 'reviewer' }, cfg, history)),
    refused(/criterion #1: evidence names no commit/));
  // Without new evidence the verifier signs off on the recorded commit.
  const { result } = st.mutate((s, cfg, history) => db.verifyCriterion(s, '#1', 1, { by: 'reviewer' }, cfg, history));
  assert.equal(result.status, 'verified');
});

test('done refuses a row whose evidence names no durable commit, then passes once it does', () => {
  const { st, onDev } = project();
  // A row settled before the guard existed (or through a criteria patch).
  st.mutate((s, cfg) => db.updateCard(s, '#1', { criteria: [{ text: 'the change works', status: 'met', metBy: 'builder', evidence: 'ran it' }], by: 'planner' }, cfg));
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg)),
    refused(/card #1 cannot close: 1 of 1 criteria fail\n  c1: evidence names no commit on dev or master/));
  assert.equal(st.load().cards[0].phase, 'planning', 'a refused close changes nothing');
  meet(st, `landed ${onDev}`);
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg));
  assert.equal(result.phase, 'done');
});

test('non-code closures declare their kind and name what settles them', () => {
  const { st } = project();
  assert.throws(() => meet(st, 'superseded', 'merged'), refused(/unknown closure kind "merged"/));
  assert.throws(() => meet(st, 'no code needed', 'decision'), refused(/ratified decision/));
  st.mutate((s) => {
    const reviewPasses = { adversarial: 'Author model family: family-a. Adversarial model family: family-b. Fresh agent: reader-2. The adversarial pass attacked the recommendation.' };
    db.addDecision(s, { cardId: '#1', id: 'D-KEEP', title: 'Keep the format', draft: true, ballotMode: 'full', reviewPasses });
    Object.assign(s.decisions.find(d => d.id === 'D-KEEP'), { status: 'ratified', draft: false, outcome: 'A' });
  });
  meet(st, 'D-KEEP ratified A: the format stays, no code change', 'decision');
  assert.equal(st.load().cards[0].criteria[0].closure, 'decision');

  assert.throws(() => meet(st, 'same as #99', 'duplicate'), refused(/must name the card/));
  assert.throws(() => meet(st, 'same as #1', 'duplicate'), refused(/must name the card/));
  meet(st, 'merged into #2', 'duplicate');
  assert.equal(st.load().cards[0].criteria[0].closure, 'duplicate');

  assert.throws(() => meet(st, '', 'obsolete'), refused(/needs --evidence/));
  assert.throws(() => meet(st, 'work is in /tmp/lane/x.patch', 'obsolete'), refused(/scratch output/));
  meet(st, 'the feature was removed from the plan', 'obsolete');
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg));
  assert.equal(result.phase, 'done', 'a declared non-code closure passes the done gate');
});

test('a code re-meet clears the kind, and reopen clears it too', () => {
  const { st, onDev } = project();
  meet(st, 'retired by the plan', 'obsolete');
  meet(st, `landed ${onDev}`);
  assert.equal(st.load().cards[0].criteria[0].closure, undefined);
  meet(st, 'retired by the plan', 'obsolete');
  st.mutate((s) => db.reopenCriterion(s, '#1', 1, { reason: 'not obsolete after all', by: 'planner' }));
  assert.equal(st.load().cards[0].criteria[0].closure, undefined);
});

test('the owner keeps the closure bypass', () => {
  const { st } = project();
  st.mutate((s, cfg, history) => db.meetCriterion(s, '#1', 1, { evidence: 'owner says done', by: 'owner' }, cfg, history));
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  assert.equal(result.phase, 'done');
});

test('a board outside git has no commits to name, so code rows pass', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-durable-nogit-'));
  writeJSON(join(dir, 'tower.json'), empty('Plain'));
  const st = openStore(dir);
  st.mutate((s, cfg) => db.addCard(s, { title: 'Work' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'done', 'planner'));
  meet(st, 'ran it');
  assert.equal(st.load().cards[0].criteria[0].status, 'met');
});

test('cli: criteria --meet refuses with E_EVIDENCE and passes with a landed commit or --kind', () => {
  const { data, onDev } = project();
  const cli = (...args) => spawnSync(process.execPath, [TOWER, '--data', data, ...args], { encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
  const bad = cli('card', 'criteria', '#1', '--meet', '1', '--evidence', 'ran it', '--by', 'builder', '--json');
  assert.equal(bad.status, 1);
  assert.equal(JSON.parse(bad.stdout).error, 'E_EVIDENCE');
  assert.match(bad.stderr, /criterion #1: evidence names no commit on dev or master/);
  const good = cli('card', 'criteria', '#1', '--meet', '1', '--evidence', `landed ${onDev.slice(0, 9)}`, '--by', 'builder');
  assert.equal(good.status, 0, good.stderr);
  const kind = cli('card', 'criteria', '#1', '--meet', '1', '--kind', 'duplicate', '--evidence', 'merged into #2', '--by', 'builder');
  assert.equal(kind.status, 0, kind.stderr);
  const stray = cli('card', 'criteria', '#1', '--kind', 'obsolete', '--by', 'builder');
  assert.notEqual(stray.status, 0);
  assert.match(stray.stderr + stray.stdout, /--kind goes with --meet or --verify/);
});

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
const local = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });

test('a refused close lists every failing criterion with its reason, in one refusal', () => {
  const { st, onDev, unlanded } = project();
  st.mutate((s, cfg) => db.updateCard(s, '#1', { by: 'planner', criteria: [
    { text: 'parser accepts it', status: 'met', metBy: 'builder', evidence: `landed ${onDev}` },
    { text: 'checker reports it' },
    { text: 'docs say so', status: 'met', metBy: 'builder', evidence: 'ran it' },
    { text: 'lowering emits it', status: 'met', metBy: 'builder', evidence: `built ${unlanded.slice(0, 10)}` },
  ] }, cfg));
  const close = () => st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg));
  assert.throws(close, (e) => {
    assert.equal(e.code, 'E_CRITERIA', 'an unsettled row makes it a criteria refusal');
    assert.equal(e.message, [
      'card #1 cannot close: 3 of 4 criteria fail',
      '  c2: not met — checker reports it',
      '  c3: evidence names no commit on dev or master; cite the landed commit, or declare --kind decision, duplicate or obsolete',
      `  c4: commit ${unlanded.slice(0, 10)} is not on dev or master; land it, then cite the landed commit`,
    ].join('\n'));
    return true;
  });
  // With every row settled the same list is an evidence refusal.
  st.mutate((s, cfg, history) => db.meetCriterion(s, '#1', 2, { evidence: `landed ${onDev}`, by: 'builder' }, cfg, history));
  assert.throws(close, (e) => e.code === 'E_EVIDENCE' && /2 of 4 criteria fail\n  c3: .*\n  c4: /.test(e.message) && !/c2:/.test(e.message));
  assert.equal(st.load().cards[0].phase, 'planning', 'a refused close changes nothing');
});

test('move re-homes an open criterion: a new open row on the target, the old row met as a duplicate, both linked', () => {
  const { st, onDev } = project();
  st.mutate((s) => db.addCriterion(s, '#2', 'earlier row', 'planner'));
  st.mutate((s) => db.addCriterion(s, '#1', 'second row', 'planner'));
  const { result } = st.mutate((s) => db.moveCriterion(s, '#1', 2, { to: '#2', reason: 'belongs with the earlier copy', by: 'planner' }));
  assert.equal(result.to.n, 2);
  const [one, two] = st.load().cards;
  assert.deepEqual(one.criteria[1].movedTo, { card: 2, n: 2 });
  assert.equal(one.criteria[1].status, 'met');
  assert.equal(one.criteria[1].closure, 'duplicate');
  assert.equal(one.criteria[1].evidence, 'Moved to #2 c2');
  assert.deepEqual(two.criteria[1], { ...two.criteria[1], text: 'second row', status: 'open', movedFrom: { card: 1, n: 2 } });
  const events = st.load().events.filter(e => e.action === 'card.criteria-move');
  assert.match(events.at(-1).note, /#2 → #2 c2: belongs with the earlier copy/);
  // The moved row satisfies the close gate on a board that checks commits.
  st.mutate((s, cfg, history) => db.meetCriterion(s, '#1', 1, { evidence: `landed ${onDev}`, by: 'builder' }, cfg, history));
  assert.equal(st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg)).result.phase, 'done');
});

test('move refuses a settled row, the same card and a done target, and changes nothing', () => {
  const { st, onDev } = project();
  const before = JSON.stringify(st.load().cards);
  const move = (n, to) => () => st.mutate((s) => db.moveCriterion(s, '#1', n, { to, by: 'planner' }));
  assert.throws(move(1, '#1'), (e) => e.code === 'E_INVALID' && /already on card #1/.test(e.message));
  assert.throws(move(1, '#9'), (e) => e.code === 'E_NOT_FOUND');
  assert.throws(move(5, '#2'), (e) => e.code === 'E_NOT_FOUND');
  assert.equal(JSON.stringify(st.load().cards), before);
  st.mutate((s) => db.addCriterion(s, '#2', 'its row', 'planner'));
  st.mutate((s, cfg, history) => db.meetCriterion(s, '#2', 1, { evidence: `landed ${onDev}`, by: 'builder' }, cfg, history));
  st.mutate((s, cfg) => db.updateCard(s, '#2', { phase: 'done', by: 'builder' }, cfg));
  assert.throws(move(1, '#2'), (e) => e.code === 'E_INVALID' && /card #2 is done/.test(e.message));
  meet(st, `landed ${onDev}`);
  st.mutate((s) => db.addCriterion(s, '#2', 'reopen target', 'planner'));
  assert.throws(move(1, '#2'), (e) => e.code === 'E_INVALID' && /is met; only an open criterion moves/.test(e.message));
});

test('reopening or rewording a moved row drops its link', () => {
  const { st } = project();
  st.mutate((s) => db.moveCriterion(s, '#1', 1, { to: '#2', by: 'planner' }));
  st.mutate((s) => db.reopenCriterion(s, '#1', 1, { reason: 'stays here after all', by: 'planner' }));
  const row = st.load().cards[0].criteria[0];
  assert.equal(row.status, 'open');
  assert.equal(row.movedTo, undefined);
  assert.equal(row.closure, undefined);
});

test('cli: criteria --move links both rows and --list shows the links', () => {
  const { data } = project();
  const moved = local(data, 'card', 'criteria', '#1', '--move', '1', '--to', '#2', '--reason', 'belongs there', '--by', 'planner');
  assert.equal(moved.status, 0, moved.stderr);
  assert.match(moved.stdout, /criterion #1 moved from card #1 to card #2 as c1/);
  assert.match(local(data, 'card', 'criteria', '#1', '--list').stdout, /#1 \[met\] the change works {2}→ moved to #2 c1 {2}met:planner/);
  assert.match(local(data, 'card', 'criteria', '#2', '--list').stdout, /#1 \[open\] the change works {2}← from #1 c1/);
  const json = JSON.parse(local(data, 'card', 'criteria', '#2', '--json').stdout);
  assert.deepEqual(json[0].movedFrom, { card: 1, n: 1 });
  const lonely = local(data, 'card', 'criteria', '#1', '--move', '1', '--by', 'planner');
  assert.notEqual(lonely.status, 0);
  assert.match(lonely.stderr, /--move n needs --to <card>/);
});
