// #6563 c4: every boardBatch-th write commits tower.json and history.json to
// the board branch, never the owner's scratch notes, and `tower board check`
// holds the committed board to within one batch of the working copy.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { openStore, empty, emptyHistory } from '../app/store.mjs';
import { configFile, historyFile, writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';

const TOWER = fileURLToPath(new URL('../tower.mjs', import.meta.url));
const gitEnv = { ...process.env };
for (const key of ['GIT_INDEX_FILE', 'GIT_DIR', 'GIT_WORK_TREE']) delete gitEnv[key];
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', env: gitEnv }).trim();
const BATCH = 3;

// A project on master whose board (tower.json, history.json and the owner's
// scratch notes) is tracked; the board commits every BATCH writes.
function project({ track = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'tower-board-'));
  git(root, 'init', '-q', '-b', 'master');
  git(root, 'config', 'user.name', 't');
  git(root, 'config', 'user.email', 't@t');
  const data = join(root, '.tower');
  mkdirSync(join(data, 'scratch'), { recursive: true });
  writeJSON(join(data, 'tower.json'), empty('Board'));
  writeJSON(historyFile(data), emptyHistory());
  writeJSON(configFile(data), { project: 'Board', boardBatch: BATCH });
  writeFileSync(join(data, 'scratch', 'owner-scratch.md'), 'owner notes\n');
  writeFileSync(join(root, 'a.txt'), 'a\n');
  git(root, 'add', 'a.txt', ...(track ? ['.tower/tower.json', '.tower/history.json', '.tower/config.json', '.tower/scratch/owner-scratch.md'] : []));
  git(root, 'commit', '-q', '-m', 'start');
  return { root, data, st: openStore(data) };
}

const addCard = (st, title) => st.mutate((s, cfg) => db.addCard(s, { title }, cfg));
const committedRev = (root) => JSON.parse(git(root, 'show', 'HEAD:.tower/tower.json')).meta.rev;
const commits = (root) => Number(git(root, 'rev-list', '--count', 'HEAD'));

test('every batch-th write commits the board pair and nothing else', () => {
  const { root, data, st } = project();
  writeFileSync(join(data, 'scratch', 'owner-scratch.md'), 'owner notes, edited\n');
  writeFileSync(join(root, 'a.txt'), 'a, edited\n');
  addCard(st, 'one');
  addCard(st, 'two');
  assert.equal(commits(root), 1, 'no commit before the batch fills');
  const { state } = addCard(st, 'three');
  assert.equal(state.meta.rev % BATCH, 0);
  assert.equal(commits(root), 2);
  assert.equal(committedRev(root), state.meta.rev);
  assert.equal(git(root, 'log', '-1', '--format=%s'), `tower: board at rev ${state.meta.rev}`);
  assert.deepEqual(git(root, 'show', '--name-only', '--format=', 'HEAD').split('\n'), ['.tower/tower.json']);
  const dirty = execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8', env: gitEnv });
  assert.match(dirty, /^ M \.tower\/scratch\/owner-scratch\.md$/m, 'the owner scratch stays uncommitted');
  assert.match(dirty, /^ M a\.txt$/m, 'unrelated work stays uncommitted');
  assert.doesNotMatch(dirty, /tower\.json|history\.json/);
});

test('board check passes within one batch and fails past it', () => {
  const { root, st } = project();
  for (let i = 0; i < BATCH; i++) addCard(st, `card ${i}`);
  const fresh = st.checkBoard();
  assert.equal(fresh.ok, true);
  assert.equal(fresh.behind, 0);
  // Writes that bypass the batch hook (another branch checked out) leave the
  // committed board behind until check reports it.
  git(root, 'checkout', '-q', '-b', 'side');
  for (let i = 0; i < BATCH + 1; i++) addCard(st, `late ${i}`);
  git(root, 'checkout', '-q', 'master');
  const stale = st.checkBoard();
  assert.equal(stale.ok, false);
  assert.equal(stale.behind, BATCH + 1);
  const done = st.commitBoard('closer');
  assert.ok(done.committed);
  assert.equal(git(root, 'log', '-1', '--format=%s'), `tower: board at rev ${stale.workingRev} (closer)`);
  assert.equal(st.checkBoard().ok, true);
  assert.equal(st.commitBoard('closer').reason, 'the committed board is current');
});

test('a board git does not track, or a checkout on another branch, is left alone', () => {
  const untracked = project({ track: false });
  for (let i = 0; i < BATCH; i++) addCard(untracked.st, `card ${i}`);
  assert.equal(commits(untracked.root), 1);
  assert.match(untracked.st.commitBoard('x').reason, /not tracked by git/);

  const side = project();
  git(side.root, 'checkout', '-q', '-b', 'side');
  for (let i = 0; i < BATCH; i++) addCard(side.st, `card ${i}`);
  assert.equal(commits(side.root), 1);
  assert.match(side.st.commitBoard('x').reason, /checkout is on side; board commits go to master/);
});

test('cli: board commit and board check', () => {
  const { root, data, st } = project();
  const cli = (...args) => spawnSync(process.execPath, [TOWER, '--data', data, ...args], { encoding: 'utf8', env: { ...gitEnv, TOWER_DATA: '' } });
  addCard(st, 'one');
  const ok = cli('board', 'check');
  assert.equal(ok.status, 0, ok.stderr);
  assert.match(ok.stdout, /^OK committed board rev 0 at master [0-9a-f]+; working copy rev 1: 1 write behind \(batch 3\)/);
  assert.match(cli('board', 'commit').stderr, /board commit needs --by/);
  const commit = cli('board', 'commit', '--by', 'closer');
  assert.equal(commit.status, 0, commit.stderr);
  assert.match(commit.stdout, /^committed board rev 1 to master as [0-9a-f]{10} \(\.tower\/tower\.json, \.tower\/history\.json\)/);
  assert.equal(committedRev(root), 1);
  git(root, 'checkout', '-q', '-b', 'side');
  for (let i = 0; i < BATCH + 1; i++) addCard(st, `late ${i}`);
  git(root, 'checkout', '-q', 'master');
  const stale = cli('board', 'check', '--json');
  assert.equal(stale.status, 1);
  assert.equal(JSON.parse(stale.stdout).error, 'E_BOARD_STALE');
  assert.match(stale.stderr, /4 writes behind \(batch 3\) — run `tower board commit --by <agent>`/);
});
