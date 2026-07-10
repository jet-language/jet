import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const TOWER = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');
const run = (cwd, args, ok = true) => {
  try {
    return { out: execFileSync(process.execPath, [TOWER, ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } }), code: 0 };
  } catch (e) {
    if (ok) throw e;
    return { out: (e.stdout || '') + (e.stderr || ''), code: e.status };
  }
};

test('cli end-to-end: init → epoch → milestone → card → decision → next', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-'));
  run(cwd, ['init', '--name', 'CLI Test']);
  run(cwd, ['epoch', 'add', 'e1', '--name', 'Epoch One', '--goal', 'ship']);
  run(cwd, ['epoch', 'current', 'e1']);
  const m = JSON.parse(run(cwd, ['milestone', 'add', '--epoch', 'e1', '--title', 'MVP', '--json']).out);
  const c = JSON.parse(run(cwd, ['card', 'add', '--title', 'Build it', '--priority', 'P1', '--milestone', m.id, '--json']).out);
  assert.equal(c.num, 1);
  run(cwd, ['card', 'activate', '#1', '--work-order', '1', '--by', 'owner']);

  // decision via stdin-less file
  const ballot = JSON.stringify({ cardId: '#1', id: 'D-CLI1', title: 'Choose',
    gist: 'g', story: 's', inWild: 'w', rec: 'B',
    options: [{ key: 'A', name: 'a', code: 'a()' }, { key: 'B', name: 'b', code: 'b()' }] });
  const bp = join(cwd, 'ballot.json');
  writeFileSync(bp, ballot);
  run(cwd, ['decision', 'add', '--file', bp, '--by', 'tester']);

  const list = JSON.parse(run(cwd, ['card', 'list', '--lane', 'decide', '--json']).out);
  assert.equal(list.length, 1);

  run(cwd, ['decision', 'ratify', 'D-CLI1', '--outcome', 'B', '--by', 'owner']);
  run(cwd, ['card', 'update', '#1', '--phase', 'building', '--log', 'started', '--by', 'tester']);
  const next = JSON.parse(run(cwd, ['next', '--json']).out);
  assert.equal(next[0].num, 1);

  // exit codes: invalid enum → 1, stale rev → 2
  const bad = run(cwd, ['card', 'update', '#1', '--phase', 'bogus'], false);
  assert.equal(bad.code, 1);
  const stale = run(cwd, ['card', 'update', '#1', '--title', 'x', '--expect-rev', '0'], false);
  assert.equal(stale.code, 2);

  // milestone progress reflects done cards
  run(cwd, ['card', 'update', '#1', '--phase', 'done', '--by', 'tester']);
  const ms = JSON.parse(run(cwd, ['milestone', 'list', '--json']).out);
  assert.deepEqual(ms[0].progress, { total: 1, done: 1, met: false });
});

test('cli without init fails with a helpful hint', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-noinit-'));
  const r = run(cwd, ['status'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /tower init/);
});
