// `tower apply` runs a plan of board edits as one all-or-nothing write;
// `tower publish` posts a briefing and proves origin holds the board commit.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { configFile, historyFile, writeJSON } from '../app/paths.mjs';

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
const gitEnv = { ...process.env };
for (const key of ['GIT_INDEX_FILE', 'GIT_DIR', 'GIT_WORK_TREE']) delete gitEnv[key];
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...gitEnv, TOWER_DATA: '' } });
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', env: gitEnv }).trim();

function board() {
  const data = mkdtempSync(join(tmpdir(), 'tower-apply-'));
  writeJSON(join(data, 'tower.json'), db.empty('Apply'));
  const st = db.openStore(data);
  st.mutate((s, cfg) => {
    db.addCard(s, { title: 'Parser', criteria: ['parses a', 'parses b', 'parses c'] }, cfg);
    db.addCard(s, { title: 'Checker' }, cfg);
  });
  return { data, st };
}
const planFile = (data, plan) => {
  const path = join(data, 'plan.json');
  writeFileSync(path, JSON.stringify(plan));
  return path;
};

const PLAN = { ops: [
  { op: 'add', card: '#2', text: 'checks a' },
  { op: 'meet', card: '#1', n: 1, evidence: 'parsed a in the suite', kind: 'obsolete' },
  { op: 'edit', card: '#1', n: 2, text: 'parses b and B', reason: 'case matters' },
  { op: 'remove', card: '#1', n: 3, reason: 'covered by c2' },
  { op: 'move', card: '#1', n: 2, to: '#2', reason: 'the checker owns it' },
  { op: 'card.update', card: '#2', phase: 'building', addTags: ['types'] },
  { op: 'epoch.update', id: 'e1', order: 5 },
  { op: 'milestone.add', id: 'm-front', epochId: 'e1', title: 'Front end' },
  { op: 'milestone.update', id: 'm-front', title: 'Front end works' },
] };

test('apply runs every op in one write, and one undo reverts the plan', () => {
  const { data, st } = board();
  const before = st.load().meta.rev;
  const applied = cli(data, 'apply', '--file', planFile(data, PLAN), '--by', 'planner', '--json');
  assert.equal(applied.status, 0, applied.stderr);
  const result = JSON.parse(applied.stdout);
  assert.equal(result.applied, 9);
  assert.equal(result.rev, before + 1, 'the whole plan is one revision');
  const s = st.load();
  const [parser, checker] = s.cards;
  assert.deepEqual(parser.criteria.map(i => [i.n, i.status, i.text]),
    [[1, 'met', 'parses a'], [2, 'met', 'parses b and B']]);
  assert.deepEqual(parser.criteria[1].movedTo, { card: checker.num, n: 2 });
  assert.deepEqual(checker.criteria.map(i => i.text), ['checks a', 'parses b and B']);
  assert.deepEqual([checker.phase, checker.tags], ['building', ['types']]);
  assert.equal(s.epochs.find(e => e.id === 'e1').order, 5);
  assert.equal(s.milestones.find(m => m.id === 'm-front').title, 'Front end works');
  assert.deepEqual([s.events[0].action, s.events[0].note],
    ['board.apply', '9 ops: 1 add, 1 meet, 1 edit, 1 remove, 1 move, 1 card.update, 1 epoch.update, 1 milestone.add, 1 milestone.update']);
  const undo = cli(data, 'undo');
  assert.equal(undo.status, 0, undo.stderr);
  const undone = st.load();
  assert.deepEqual(undone.cards[0].criteria.map(i => i.status), ['open', 'open', 'open']);
  assert.deepEqual([undone.cards[1].criteria, undone.milestones], [[], []]);
  const text = cli(data, 'apply', '--file', planFile(data, { ops: [{ op: 'card.update', card: '#1', priority: 'P1' }] }), '--by', 'planner');
  assert.match(text.stdout, new RegExp(`^applied 1 op in one write at rev ${before + 3} \\(1 card\\.update\\)\\n$`));
});

test('one refused op leaves the board untouched and names its place in the plan', () => {
  const { data, st } = board();
  const before = JSON.stringify(st.load());
  const refused = cli(data, 'apply', '--file', planFile(data, { ops: [
    { op: 'add', card: '#2', text: 'checks a' },
    { op: 'card.update', card: '#2', phase: 'building' },
    { op: 'meet', card: '#9', n: 1, evidence: 'x' },
  ] }), '--by', 'planner');
  assert.equal(refused.status, 1);
  assert.match(refused.stderr, /^tower: op 3 \(meet #9 c1\): no card #9 — nothing applied\n$/);
  assert.equal(JSON.stringify(st.load()), before);
});

test('every shape problem in a plan is reported before anything runs', () => {
  const { data, st } = board();
  const rev = st.load().meta.rev;
  const refused = cli(data, 'apply', '--file', planFile(data, { ops: [
    { op: 'meet', card: '#1', n: 1 },
    { op: 'retitle', card: '#1' },
    { op: 'card.update', card: '#1', colour: 'red' },
    { op: 'epoch.update', id: 'e1' },
    { op: 'edit', card: '#1', n: 1 },
  ] }), '--by', 'planner', '--json');
  assert.equal(refused.status, 1);
  assert.equal(JSON.parse(refused.stdout).error, 'E_USAGE');
  assert.equal(refused.stderr, [
    'tower: plan refused, nothing applied:',
    '  op 1 (meet): needs evidence',
    '  op 2: unknown op "retitle" — one of meet, edit, add, remove, move, card.update, epoch.update, milestone.add, milestone.update',
    '  op 3 (card.update): unknown field colour',
    '  op 3 (card.update): changes nothing',
    '  op 4 (epoch.update): changes nothing',
    '  op 5 (edit): needs text, proof or blockedBy',
  ].join('\n') + '\n');
  assert.match(cli(data, 'apply', '--file', planFile(data, [{ op: 'add' }]), '--by', 'p').stderr, /a plan is a JSON object \{ "ops": \[ … \] \}/);
  assert.match(cli(data, 'apply', '--file', planFile(data, { ops: [] }), '--by', 'p').stderr, /the plan has no ops/);
  assert.match(cli(data, 'apply', '--file', planFile(data, PLAN)).stderr, /apply needs --by <agent>/);
  assert.equal(st.load().meta.rev, rev);
});

test('apply --dry-run reports what the plan would do and writes nothing', () => {
  const { data, st } = board();
  const before = JSON.stringify(st.load());
  const dry = cli(data, 'apply', '--file', planFile(data, PLAN), '--dry-run', '--by', 'planner');
  assert.equal(dry.status, 0, dry.stderr);
  assert.match(dry.stdout, /^dry run: 9 ops would apply \(1 add, 1 meet, .*1 milestone\.update\); nothing written\n$/);
  const json = JSON.parse(cli(data, 'apply', '--file', planFile(data, PLAN), '--dry-run', '--by', 'planner', '--json').stdout);
  assert.deepEqual([json.dryRun, json.applied, json.ops[4]], [true, 9, { op: 'move', ref: '#1 c2 → #2' }]);
  const refused = cli(data, 'apply', '--file', planFile(data, { ops: [{ op: 'remove', card: '#1', n: 7, reason: 'gone' }] }), '--dry-run', '--by', 'p');
  assert.equal(refused.status, 1);
  assert.match(refused.stderr, /op 1 \(remove #1 c7\): no criterion #7 on card #\d+/);
  assert.equal(JSON.stringify(st.load()), before);
});

test('a plan of 400 card updates applies in seconds', () => {
  const data = mkdtempSync(join(tmpdir(), 'tower-apply-big-'));
  writeJSON(join(data, 'tower.json'), db.empty('Big'));
  const st = db.openStore(data);
  st.mutate((s, cfg) => { for (let i = 0; i < 400; i++) db.addCard(s, { title: `card ${i}`, criteria: ['works'] }, cfg); });
  const ops = Array.from({ length: 400 }, (_, i) => ({ op: 'card.update', card: `#${i + 1}`, phase: 'building', priority: 'P1' }));
  const started = Date.now();
  const applied = cli(data, 'apply', '--file', planFile(data, { ops }), '--by', 'planner');
  const took = Date.now() - started;
  assert.equal(applied.status, 0, applied.stderr);
  assert.ok(took < 5000, `400 updates took ${took}ms`);
  assert.equal(st.load().cards.filter(c => c.phase === 'building').length, 400);
});

// A project on master whose board git tracks, as `tower board commit` expects.
function project() {
  const root = mkdtempSync(join(tmpdir(), 'tower-publish-'));
  git(root, 'init', '-q', '-b', 'master');
  git(root, 'config', 'user.name', 't');
  git(root, 'config', 'user.email', 't@t');
  const data = join(root, '.tower');
  mkdirSync(data, { recursive: true });
  writeJSON(join(data, 'tower.json'), db.empty('Board'));
  writeJSON(historyFile(data), db.emptyHistory());
  writeJSON(configFile(data), { project: 'Board' });
  writeFileSync(join(root, 'briefing.md'), 'Work landed.\n');
  git(root, 'add', '.tower/tower.json', '.tower/history.json', '.tower/config.json', 'briefing.md');
  git(root, 'commit', '-q', '-m', 'start');
  return { root, data };
}

test('publish posts the briefing, commits the board and proves origin holds it', () => {
  const { root, data } = project();
  const remote = mkdtempSync(join(tmpdir(), 'tower-origin-'));
  git(remote, 'init', '-q', '--bare');
  git(root, 'remote', 'add', 'origin', remote);
  git(root, 'push', '-q', 'origin', 'master');
  assert.match(cli(data, 'publish', '--file', join(root, 'briefing.md'), '--by', 'closer').stderr, /publish needs --title/);
  const published = cli(data, 'publish', '--title', 'Day one', '--file', join(root, 'briefing.md'), '--by', 'closer', '--json');
  assert.equal(published.status, 0, published.stderr);
  const result = JSON.parse(published.stdout);
  const head = git(root, 'rev-parse', 'HEAD');
  assert.deepEqual([result.branch, result.commit, result.remote], ['master', head, head]);
  assert.equal(git(remote, 'rev-parse', 'master'), head);
  assert.equal(JSON.parse(git(root, 'show', 'HEAD:.tower/tower.json')).briefings[0].title, 'Day one');
});

test('publish exits non-zero when origin does not end up holding the board commit', () => {
  const { root, data } = project();
  const failed = cli(data, 'publish', '--title', 'Day one', '--file', join(root, 'briefing.md'), '--by', 'closer');
  assert.equal(failed.status, 1);
  assert.match(failed.stderr, /^tower: posted briefing \S+, but the board is not published: the repository has no origin remote\n$/);
  const remote = mkdtempSync(join(tmpdir(), 'tower-origin-'));
  git(remote, 'init', '-q', '--bare');
  git(root, 'remote', 'add', 'origin', remote);
  // A remote branch that moved on rejects the push.
  const other = mkdtempSync(join(tmpdir(), 'tower-other-'));
  git(other, 'init', '-q', '-b', 'master');
  git(other, 'config', 'user.name', 't');
  git(other, 'config', 'user.email', 't@t');
  git(other, 'commit', '-q', '--allow-empty', '-m', 'elsewhere');
  git(other, 'push', '-q', remote, 'master');
  const rejected = cli(data, 'publish', '--title', 'Day two', '--file', join(root, 'briefing.md'), '--by', 'closer', '--json');
  assert.equal(rejected.status, 1);
  assert.equal(JSON.parse(rejected.stdout).error, 'E_PUBLISH');
  assert.match(rejected.stderr, /git push origin master failed: /);
});
