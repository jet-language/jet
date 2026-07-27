import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
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
  assert.match(readFileSync(join(cwd, '.tower', '.gitignore'), 'utf8'), /^secrets\.json$/m);
  assert.match(readFileSync(join(cwd, '.tower', '.gitignore'), 'utf8'), /^\.secrets\.json\.tmp-\*$/m);
  run(cwd, ['epoch', 'add', 'e1', '--name', 'Epoch One', '--goal', 'ship']);
  run(cwd, ['epoch', 'current', 'e1']);
  const m = JSON.parse(run(cwd, ['milestone', 'add', '--epoch', 'e1', '--title', 'MVP', '--json']).out);
  const c = JSON.parse(run(cwd, ['card', 'add', '--title', 'Build it', '--priority', 'P1', '--milestone', m.id, '--json']).out);
  assert.equal(c.num, 1);
  run(cwd, ['card', 'update', '#1', '--work-order', '1', '--by', 'owner']);

  // decision via stdin-less file
  const ballot = JSON.stringify({ cardId: '#1', id: 'D-CLI1', title: 'Choose',
    gist: 'g', lesson: 'teach from zero', story: 's', inWild: 'w', rec: 'B',
    recommendation: { why: 'B wins here.', whyNot: [{ key: 'A', reason: 'A loses the needed behavior.' }], tradeoff: 'B adds one visible step.' },
    hybrid: { result: 'B', synthesis: 'B combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Borrow its clear names.' }, { key: 'B', aspect: 'B is brief.', use: 'Keep it.' }] },
    options: [{ key: 'A', name: 'a', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'b', detail: 'B is brief.', code: 'b()' }] });
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
  assert.deepEqual(ms[0].progress, { total: 1, done: 1, met: true });
});

test('cli without init fails with a helpful hint', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-noinit-'));
  const r = run(cwd, ['status'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /tower init/);
});

test('status lists verification before building work', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-status-'));
  run(cwd, ['init', '--name', 'CLI Status']);
  run(cwd, ['card', 'add', '--title', 'Build']);
  run(cwd, ['card', 'update', '#1', '--phase', 'building', '--by', 'owner']);
  run(cwd, ['card', 'add', '--title', 'Verify']);
  run(cwd, ['card', 'update', '#2', '--phase', 'verify', '--by', 'owner']);
  const out = run(cwd, ['status', '--color=never']).out;
  assert.ok(out.indexOf('AGENT — verify') < out.indexOf('AGENT — building'));
});

test('CLI --by owner and --quote cannot resolve acceptance; rejection is audited', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-accept-'));
  run(cwd, ['init', '--name', 'CLI Accept Guard']);
  run(cwd, ['card', 'add', '--title', 'Owner must inspect', '--by', 'builder']);
  run(cwd, ['card', 'update', '#1', '--needs-acceptance', 'true', '--by', 'builder']);
  run(cwd, ['card', 'update', '#1', '--phase', 'done', '--by', 'builder']);
  for (const args of [
    ['card', 'update', '#1', '--phase', 'done', '--by', 'owner'],
    ['card', 'update', '#1', '--needs-acceptance', 'false', '--by', 'owner'],
  ]) {
    const rejected = run(cwd, args, false);
    assert.equal(rejected.code, 1);
    assert.match(rejected.out, /dedicated owner verification UI/);
  }
  for (const args of [
    ['decision', 'ratify', 'D-ACCEPT-1', '--outcome', 'accept', '--by', 'owner'],
    ['decision', 'ratify', 'D-ACCEPT-1', '--outcome', 'accept', '--by', 'agent', '--quote', 'accept it'],
  ]) {
    const rejected = run(cwd, args, false);
    assert.equal(rejected.code, 1);
    assert.match(rejected.out, /dedicated owner verification UI/);
  }
  const state = JSON.parse(run(cwd, ['state']).out);
  assert.equal(state.cards[0].phase, 'verify');
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-1').status, 'open');
  assert.equal(state.events.filter(e => e.action === 'acceptance.reject').length, 4);
});

test('init appends the secrets ignore to an existing data ignore file', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-existing-ignore-'));
  const dataDir = join(cwd, '.tower');
  mkdirSync(dataDir);
  writeFileSync(join(dataDir, '.gitignore'), 'local-only-entry\n');
  run(cwd, ['init', '--name', 'Existing Ignore']);
  const ignores = readFileSync(join(dataDir, '.gitignore'), 'utf8').split(/\r?\n/);
  assert.equal(ignores.includes('local-only-entry'), true);
  assert.equal(ignores.includes('secrets.json'), true);
  assert.equal(ignores.includes('.secrets.json.tmp-*'), true);
});

test('secret final and crash-residue files stay ignored while public config remains tracked', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-secret-crash-ignore-'));
  run(cwd, ['init', '--name', 'Crash Ignore']);
  const dataDir = join(cwd, '.tower');
  const residueName = '.secrets.json.tmp-crash-after-fsync';
  const child = spawnSync(process.execPath, ['-e', `
    const fs = require('node:fs');
    const path = require('node:path');
    const file = path.join(process.argv[1], ${JSON.stringify(residueName)});
    const fd = fs.openSync(file, 'wx', 0o600);
    fs.writeFileSync(fd, '{}\\n');
    fs.fsyncSync(fd);
    process.exit(73);
  `, dataDir], { stdio: 'ignore' });
  assert.equal(child.status, 73, 'child exits after fsync and before rename');
  writeFileSync(join(dataDir, 'secrets.json'), '{}\n', { mode: 0o600 });

  execFileSync('git', ['init', '-q'], { cwd, stdio: 'ignore' });
  const ignored = (path) => spawnSync('git', ['check-ignore', '-q', '--', path], { cwd }).status === 0;
  assert.equal(ignored('.tower/secrets.json'), true);
  assert.equal(ignored(`.tower/${residueName}`), true);
  assert.equal(ignored('.tower/config.json'), false, 'public config remains trackable');

  execFileSync('git', ['add', '.'], { cwd, stdio: 'ignore' });
  const tracked = execFileSync('git', ['ls-files', '-z'], { cwd })
    .toString('utf8').split('\0').filter(Boolean);
  assert.equal(tracked.includes('.tower/config.json'), true);
  assert.equal(tracked.includes('.tower/secrets.json'), false);
  assert.equal(tracked.includes(`.tower/${residueName}`), false);
});

test('cli refuses legacy secrets in tracked config with safe migration guidance', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-legacy-config-'));
  run(cwd, ['init', '--name', 'Legacy']);
  const marker = 'never-echo-this-value';
  writeFileSync(join(cwd, '.tower', 'config.json'), JSON.stringify({ project: 'Legacy', auth: { token: marker } }));
  const r = run(cwd, ['status'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /rotate/);
  assert.match(r.out, /\.tower\/secrets\.json/);
  assert.equal(r.out.includes(marker), false);
});
