import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { canonicalDataDir, findDataDir, writeJSON } from '../app/paths.mjs';

delete process.env.TOWER_DATA;

// main checkout with a linked worktree at .claude/worktrees/x, both carrying
// a tracked .tower copy — the worktree one must never win.
function repoPair({ mainBoard = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'tower-wt-'));
  const main = join(root, 'main');
  const wt = join(main, '.claude', 'worktrees', 'x');
  mkdirSync(join(main, '.git', 'worktrees', 'x'), { recursive: true });
  mkdirSync(join(main, '.tower'), { recursive: true });
  mkdirSync(join(wt, '.tower'), { recursive: true });
  writeFileSync(join(wt, '.git'), `gitdir: ${join(main, '.git', 'worktrees', 'x')}\n`);
  if (mainBoard) writeJSON(join(main, '.tower', 'tower.json'), { meta: {} });
  writeJSON(join(wt, '.tower', 'tower.json'), { meta: {} });
  return { main, wt };
}

test('a board hit inside a linked worktree resolves to the canonical main-checkout board', () => {
  const { main, wt } = repoPair();
  assert.equal(findDataDir(wt), join(main, '.tower'));
  assert.equal(findDataDir(join(wt, 'plugins', 'tower')), join(main, '.tower'));
});

test('a worktree copy with no canonical counterpart is refused, never used', () => {
  const { wt } = repoPair({ mainBoard: false });
  assert.equal(canonicalDataDir(join(wt, '.tower')), null);
  assert.equal(findDataDir(wt), null);
});

test('a plain checkout (.git directory) resolves in place', () => {
  const root = mkdtempSync(join(tmpdir(), 'tower-plain-'));
  mkdirSync(join(root, '.git'), { recursive: true });
  mkdirSync(join(root, '.tower'), { recursive: true });
  writeJSON(join(root, '.tower', 'tower.json'), { meta: {} });
  assert.equal(findDataDir(root), join(root, '.tower'));
  assert.equal(canonicalDataDir(join(root, '.tower')), join(root, '.tower'));
});

test('TOWER_DATA stays an explicit override, worktree or not', () => {
  const { wt } = repoPair();
  process.env.TOWER_DATA = join(wt, '.tower');
  try {
    assert.equal(findDataDir(wt), join(wt, '.tower'));
  } finally {
    delete process.env.TOWER_DATA;
  }
});
