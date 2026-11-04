// #6563 c3: a met or verified criterion promises that its change is durable.
// Evidence must name a commit reachable from one of the project's durable
// branches (config.durableRefs, default dev and master), unless the row
// declares a non-code closure kind. The guard applies when the project root is
// a git work tree holding at least one durable branch; other projects have no
// commits to name.
import { spawnSync } from 'node:child_process';

export const CLOSURE_KINDS = ['code', 'decision', 'duplicate', 'obsolete'];

// Paths that hold only throwaway output: machine scratch mounts and the scratch
// root variables that name them, per-user caches and tmpfs.
export const SCRATCH_PATH_RE = /\/mnt\/[^\s/]+\/|\$\{?JET_(?:SCRATCH|STATE)_ROOT\}?\/|(?:~|\$HOME|\/home\/[^\s/]+)\/\.cache\/|(?:^|[\s"'`(=:])\/tmp\//;
const COMMIT_TOKEN_RE = /\b[0-9a-f]{7,40}\b/g;

// Git must look at the project root, not at a repository a hook or a caller
// selected through the environment.
const gitEnv = () => {
  const env = { ...process.env };
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR', 'GIT_OBJECT_DIRECTORY'])
    delete env[key];
  return env;
};

export function createCommitResolver(root, refs) {
  let branches = null;
  const known = new Map();
  const git = (...args) => spawnSync('git', ['-C', root, ...args], { encoding: 'utf8', env: gitEnv() });
  const durableBranches = () => {
    if (branches) return branches;
    branches = [];
    if (!root || git('rev-parse', '--is-inside-work-tree').stdout.trim() !== 'true') return branches;
    for (const ref of refs || []) {
      if (git('rev-parse', '--verify', '-q', `refs/heads/${ref}`).status === 0) branches.push(ref);
    }
    return branches;
  };
  // { sha, ref } for a commit on a durable branch, { sha, ref: null } for a
  // commit on none of them, null for a token that names no commit.
  const lookup = (token) => {
    if (known.has(token)) return known.get(token);
    let found = null;
    const parsed = git('rev-parse', '--verify', '-q', `${token}^{commit}`);
    if (parsed.status === 0) {
      const sha = parsed.stdout.trim();
      const ref = durableBranches().find(b => git('merge-base', '--is-ancestor', sha, `refs/heads/${b}`).status === 0);
      found = { sha, ref: ref ?? null };
    }
    known.set(token, found);
    return found;
  };
  return {
    refs: () => durableBranches(),
    active: () => durableBranches().length > 0,
    // The commits a text names, resolved; tokens naming no commit are dropped.
    commits: (text) => [...new Set(String(text || '').match(COMMIT_TOKEN_RE) || [])]
      .map(token => ({ token, ...lookup(token) }))
      .filter(found => found.sha),
  };
}
