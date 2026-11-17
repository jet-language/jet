// #6563 c4: the board is durable only once it is committed. Every
// config.boardBatch-th write commits tower.json and history.json, with the
// shards of a sharded archive (history-shards.mjs), and nothing else (never
// the owner's scratch notes) to config.boardBranch, so the committed board is
// never more than one batch behind the working copy. `tower board check`
// proves that bound; `tower board commit` closes a batch early. Boards that
// git does not track, or a checkout on another branch, are left alone.
import { spawn, spawnSync } from 'node:child_process';
import { relative } from 'node:path';

const gitEnv = () => {
  const env = { ...process.env };
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR', 'GIT_OBJECT_DIRECTORY'])
    delete env[key];
  return env;
};

const runGit = (cwd, args) => spawnSync('git', ['-C', cwd, ...args], { encoding: 'utf8', env: gitEnv(), maxBuffer: 1 << 30 });

// Where the board commits go, or { skip } saying why it is not committed.
// files is [tower.json, history.json, ...history shards]. Shards git does not
// track yet are listed in `added` when history.json is tracked, so the
// commit that first holds a shard adds it.
export function boardTarget(dataDir, files, config) {
  const top = runGit(dataDir, ['rev-parse', '--show-toplevel']);
  if (top.status !== 0) return { skip: 'the board is not in a git work tree' };
  const root = top.stdout.trim();
  const branch = runGit(root, ['symbolic-ref', '-q', '--short', 'HEAD']).stdout.trim();
  if (branch !== config.boardBranch)
    return { skip: `the checkout is on ${branch || 'a detached HEAD'}; board commits go to ${config.boardBranch}` };
  const paths = files.map(file => relative(root, file));
  const tracked = paths.filter(path => runGit(root, ['ls-files', '--error-unmatch', '--', path]).status === 0);
  if (!tracked.includes(paths[0])) return { skip: `${paths[0]} is not tracked by git` };
  const added = tracked.includes(paths[1]) ? paths.slice(2).filter(path => !tracked.includes(path)) : [];
  return { root, branch, paths: [...tracked, ...added], added };
}

// Commit the tracked board files as they are now. The caller holds the store
// lock, so the pair is one committed board version. `push: false` leaves the
// push to the caller (`tower publish` pushes and checks the remote itself).
export function commitBoard(dataDir, files, config, { rev, by, push = true }) {
  const target = boardTarget(dataDir, files, config);
  if (target.skip) return { committed: null, reason: target.skip };
  const { root, branch, paths, added } = target;
  const dirty = runGit(root, ['status', '--porcelain', '--', ...paths]);
  if (dirty.status !== 0) return { committed: null, reason: dirty.stderr.trim() };
  if (!dirty.stdout.trim()) return { committed: null, reason: 'the committed board is current' };
  if (added.length) {
    const add = runGit(root, ['add', '--', ...added]);
    if (add.status !== 0) return { committed: null, reason: (add.stderr || add.stdout).trim() };
  }
  const message = `tower: board at rev ${rev}${by ? ` (${by})` : ''}`;
  // Hooks stay off: this commit is board data, and the card-link hook would
  // re-enter the store this process holds locked.
  const commit = runGit(root, ['-c', 'core.hooksPath=/dev/null', 'commit', '--only', '-q', '-m', message, '--', ...paths]);
  if (commit.status !== 0) return { committed: null, reason: (commit.stderr || commit.stdout).trim() };
  const sha = runGit(root, ['rev-parse', 'HEAD']).stdout.trim();
  if (push) pushBoard(root, branch);
  return { committed: sha, branch, paths, rev };
}

// The remote never lags the board: every board commit is pushed to origin at
// once. Hooks are off for the commit, so no post-commit hook can do it. The
// push runs detached so the writer holding the store lock never waits on the
// network; a repository with no origin (tests, scratch boards) skips it.
function pushBoard(root, branch) {
  if (runGit(root, ['remote', 'get-url', 'origin']).status !== 0) return;
  const child = spawn('git', ['-C', root, 'push', '-q', 'origin', branch], { detached: true, stdio: 'ignore', env: gitEnv() });
  child.on('error', () => {});
  child.unref();
}

// `tower publish`: the board is published only when origin holds it. After
// `committed` (commitBoard's result), push the board branch and wait for the
// push, then read the remote branch back; ok only when it names local HEAD.
export function pushBoardVerified(dataDir, files, config, committed) {
  if (!committed.committed && !/board is current/.test(committed.reason)) return { ok: false, reason: committed.reason };
  const target = boardTarget(dataDir, files, config);
  if (target.skip) return { ok: false, reason: target.skip };
  const { root, branch } = target;
  const head = runGit(root, ['rev-parse', 'HEAD']).stdout.trim();
  if (runGit(root, ['remote', 'get-url', 'origin']).status !== 0)
    return { ok: false, branch, head, remote: null, reason: 'the repository has no origin remote' };
  const pushed = runGit(root, ['push', '-q', 'origin', branch]);
  if (pushed.status !== 0)
    return { ok: false, branch, head, remote: null, reason: `git push origin ${branch} failed: ${(pushed.stderr || pushed.stdout).trim()}` };
  const listed = runGit(root, ['ls-remote', 'origin', `refs/heads/${branch}`]);
  const remote = listed.status === 0 ? listed.stdout.trim().split(/\s+/)[0] || null : null;
  if (remote !== head)
    return { ok: false, branch, head, remote, reason: `origin/${branch} is ${remote || 'missing'}, not the local board commit ${head}` };
  return { ok: true, branch, head, remote };
}

// How far the committed board trails the working copy, in writes.
export function checkBoard(dataDir, files, config, workingRev) {
  const target = boardTarget(dataDir, files, config);
  if (target.skip) return { ok: false, reason: target.skip };
  const { root, branch, paths } = target;
  const shown = runGit(root, ['show', `HEAD:${paths[0]}`]);
  if (shown.status !== 0) return { ok: false, reason: shown.stderr.trim() };
  let committedRev;
  try { committedRev = JSON.parse(shown.stdout).meta?.rev; } catch { committedRev = undefined; }
  if (!Number.isSafeInteger(committedRev)) return { ok: false, reason: `HEAD:${paths[0]} holds no board revision` };
  const behind = workingRev - committedRev;
  const head = runGit(root, ['rev-parse', '--short', 'HEAD']).stdout.trim();
  return { ok: behind >= 0 && behind <= config.boardBatch, branch, head, committedRev, workingRev, behind, batch: config.boardBatch };
}
