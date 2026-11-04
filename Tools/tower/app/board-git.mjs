// #6563 c4: the board is durable only once it is committed. Every
// config.boardBatch-th write commits tower.json and history.json (and nothing
// else: never the owner's scratch notes) to config.boardBranch, so the
// committed board is never more than one batch behind the working copy.
// `tower board check` proves that bound; `tower board commit` closes a batch
// early. Boards that git does not track, or a checkout on another branch,
// are left alone.
import { spawnSync } from 'node:child_process';
import { relative } from 'node:path';

const gitEnv = () => {
  const env = { ...process.env };
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR', 'GIT_OBJECT_DIRECTORY'])
    delete env[key];
  return env;
};

const runGit = (cwd, args) => spawnSync('git', ['-C', cwd, ...args], { encoding: 'utf8', env: gitEnv(), maxBuffer: 1 << 30 });

// Where the board commits go, or { skip } saying why it is not committed.
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
  return { root, branch, paths: tracked };
}

// Commit the tracked board files as they are now. The caller holds the store
// lock, so the pair is one committed board version.
export function commitBoard(dataDir, files, config, { rev, by }) {
  const target = boardTarget(dataDir, files, config);
  if (target.skip) return { committed: null, reason: target.skip };
  const { root, branch, paths } = target;
  const dirty = runGit(root, ['status', '--porcelain', '--', ...paths]);
  if (dirty.status !== 0) return { committed: null, reason: dirty.stderr.trim() };
  if (!dirty.stdout.trim()) return { committed: null, reason: 'the committed board is current' };
  const message = `tower: board at rev ${rev}${by ? ` (${by})` : ''}`;
  // Hooks stay off: this commit is board data, and the card-link hook would
  // re-enter the store this process holds locked.
  const commit = runGit(root, ['-c', 'core.hooksPath=/dev/null', 'commit', '--only', '-q', '-m', message, '--', ...paths]);
  if (commit.status !== 0) return { committed: null, reason: (commit.stderr || commit.stdout).trim() };
  const sha = runGit(root, ['rev-parse', 'HEAD']).stdout.trim();
  return { committed: sha, branch, paths, rev };
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
