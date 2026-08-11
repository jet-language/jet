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
  run(cwd, ['epoch', 'update', 'e1', '--name', 'Epoch One', '--goal', 'ship']);
  run(cwd, ['epoch', 'current', 'e1']);
  const m = JSON.parse(run(cwd, ['milestone', 'add', '--epoch', 'e1', '--title', 'MVP', '--json']).out);
  const c = JSON.parse(run(cwd, ['card', 'add', '--title', 'Build it', '--priority', 'P1', '--milestone', m.id, '--json']).out);
  assert.equal(c.num, 1);
  run(cwd, ['card', 'update', '#1', '--work-order', '1', '--by', 'owner']);

  // decision via stdin-less file
  const ballot = JSON.stringify({ cardId: '#1', id: 'D-CLI1', title: 'Choose',
    ballotMode: 'full', reviewPasses: { base: 'The base pass completed the ballot.', boilOcean: 'The boil-the-ocean pass tested the broad solution space.', hybrid: 'The hybrid pass combined compatible strengths.', cooperative: 'The cooperative pass strengthened each option.', adversarial: 'The adversarial pass attacked the recommendation.' },
    gist: 'g', lesson: 'teach from zero', story: 's', inWild: 'w', rec: 'B',
    recommendation: { why: 'B wins here.', whyNot: [{ key: 'A', reason: 'A loses the needed behavior.' }], tradeoff: 'B adds one visible step.' },
    hybrid: { result: 'B', synthesis: 'B combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Borrow its clear names.' }, { key: 'B', aspect: 'B is brief.', use: 'Keep it.' }] },
    options: [{ key: 'A', name: 'a', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'b', detail: 'B is brief.', code: 'b()' }] });
  const bp = join(cwd, 'ballot.json');
  writeFileSync(bp, ballot);
  run(cwd, ['decision', 'add', '--file', bp, '--by', 'tester']);
  const saved = JSON.parse(run(cwd, ['decision', 'show', 'D-CLI1', '--json']).out);
  assert.equal(saved.ballotMode, 'full');
  assert.equal(saved.reviewPasses.adversarial, 'The adversarial pass attacked the recommendation.');
  const brief = run(cwd, ['brief', '#1', '--color=never']).out;
  const ordered = ['base pass:', 'boil-the-ocean pass:', 'hybrid pass:', 'cooperative pass:', 'adversarial pass:', 'rec:'];
  for (let i = 1; i < ordered.length; i++)
    assert.ok(brief.indexOf(ordered[i - 1]) < brief.indexOf(ordered[i]), `${ordered[i - 1]} must precede ${ordered[i]}`);

  const list = JSON.parse(run(cwd, ['card', 'list', '--lane', 'decide', '--json']).out);
  assert.equal(list.length, 1);

  // show without --json prints the record, not "null"
  assert.match(run(cwd, ['card', 'show', '#1']).out, /"title": "Build it"/);
  assert.match(run(cwd, ['decision', 'show', 'D-CLI1']).out, /"title": "Choose"/);

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
  run(cwd, ['card', 'criteria', '#1', '--add', 'the card works', '--by', 'tester']);
  run(cwd, ['card', 'criteria', '#1', '--meet', '1', '--evidence', 'built', '--by', 'tester']);
  run(cwd, ['card', 'update', '#1', '--phase', 'done', '--by', 'tester']);
  const ms = JSON.parse(run(cwd, ['milestone', 'list', '--json']).out);
  assert.deepEqual(ms[0].progress, { total: 1, done: 1, reviewReady: true, met: false });
  run(cwd, ['milestone', 'criteria', m.id, '--add', 'milestone review', '--by', 'planner']);
  run(cwd, ['milestone', 'criteria', m.id, '--meet', '1', '--evidence', 'reviewed the card', '--by', 'builder']);
  run(cwd, ['milestone', 'criteria', m.id, '--verify', '1', '--evidence', 'checked independently', '--by', 'reviewer']);
  run(cwd, ['milestone', 'verify', m.id, '--evidence', 'owner reviewed the milestone', '--by', 'owner']);
  const verified = JSON.parse(run(cwd, ['milestone', 'list', '--json']).out);
  assert.equal(verified[0].status, 'met');
  assert.equal(verified[0].verification.by, 'owner');
});

test('cli without init fails with a helpful hint', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-noinit-'));
  const r = run(cwd, ['status'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /tower init/);
});

test('message CLI adds, lists, validates, and closes card-linked messages', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-message-'));
  run(cwd, ['init', '--name', 'CLI Messages']);
  run(cwd, ['card', 'add', '--title', 'Ship it']);

  const missingBy = run(cwd, ['message', 'add', '#1', '--text', 'Read this.'], false);
  assert.equal(missingBy.code, 1);
  assert.match(missingBy.out, /--by/);

  const message = JSON.parse(run(cwd, [
    'message', 'add', '#1', '--text', 'Read this.', '--by', 'agent-x', '--json',
  ]).out);
  assert.equal(message.kind, 'message');
  assert.equal(message.cardNum, 1);

  // `message list` is open-only by default; `--all` is the widening flag.
  const open = JSON.parse(run(cwd, ['message', 'list', '--json']).out);
  assert.deepEqual(open.map(item => item.id), [message.id]);

  const rejected = run(cwd, ['message', 'done', message.id, '--by', 'agent-x'], false);
  assert.equal(rejected.code, 1);
  assert.match(rejected.out, /owner-only/);

  run(cwd, ['message', 'done', message.id, '--by', 'owner']);
  assert.deepEqual(JSON.parse(run(cwd, ['message', 'list', '--json']).out), []);
});

test('status lists review before building work', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-status-'));
  run(cwd, ['init', '--name', 'CLI Status']);
  run(cwd, ['card', 'add', '--title', 'Build']);
  run(cwd, ['card', 'update', '#1', '--phase', 'building', '--by', 'owner']);
  run(cwd, ['card', 'add', '--title', 'Verify']);
  run(cwd, ['card', 'update', '#2', '--phase', 'verify', '--by', 'owner']);
  const out = run(cwd, ['status', '--color=never']).out;
  assert.ok(out.indexOf('AGENT — review') < out.indexOf('AGENT — building'));
});

test('card add probes duplicates, allows separate work, and --force bypasses', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-duplicate-probe-'));
  run(cwd, ['init', '--name', 'Duplicate Probe']);
  const first = JSON.parse(run(cwd, [
    'card', 'add', '--title', 'Fix parser crash on Linux',
    '--body', 'fails in tests/parser_linux.rs at some_test_name_linux', '--json', '--by', 'tester',
  ]).out);

  const blocked = run(cwd, [
    'card', 'add', '--title', 'Fix parser crash on Linux',
    '--body', 'fails in tests/parser_linux.rs at some_test_name_linux', '--json', '--by', 'tester',
  ], false);
  assert.equal(blocked.code, 1);
  const duplicate = JSON.parse(blocked.out);
  assert.equal(duplicate.error, 'E_DUPLICATE');
  assert.deepEqual(duplicate.candidates[0], {
    id: first.id, num: first.num, title: first.title, phase: first.phase,
    matches: ['strong title overlap', 'shared tests/parser_linux.rs', 'shared some_test_name_linux'],
  });
  assert.equal(JSON.parse(run(cwd, ['card', 'list', '--json']).out).length, 1);

  const separate = JSON.parse(run(cwd, [
    'card', 'add', '--title', 'Fix parser crash on Windows',
    '--body', 'fails in tests/parser_windows.rs at some_test_name_windows', '--json', '--by', 'tester',
  ]).out);
  assert.equal(separate.num, 2, 'different platform/path is not blocked as a duplicate');

  const forced = JSON.parse(run(cwd, [
    'card', 'add', '--title', first.title, '--body', first.body,
    '--force', '--json', '--by', 'tester',
  ]).out);
  assert.equal(forced.num, 3, '--force creates despite duplicate candidates');
});

test('card add does not block short title overlap without reference signal', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-duplicate-floor-'));
  run(cwd, ['init', '--name', 'Duplicate Floor']);
  run(cwd, ['card', 'add', '--title', 'A', '--body', 'first separate work', '--by', 'tester']);
  const second = JSON.parse(run(cwd, [
    'card', 'add', '--title', 'A', '--body', 'second separate work', '--json', '--by', 'tester',
  ]).out);
  assert.equal(second.num, 2);
});

test('card criteria --reopen reopens a verified row and audits the reason', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-criteria-reopen-'));
  run(cwd, ['init', '--name', 'Criteria Reopen']);
  run(cwd, ['card', 'add', '--title', 'Criteria card', '--by', 'planner']);
  run(cwd, ['card', 'criteria', '#1', '--add', 'ship it', '--by', 'planner']);
  run(cwd, ['card', 'criteria', '#1', '--meet', '1', '--evidence', 'built', '--by', 'builder']);
  run(cwd, ['card', 'criteria', '#1', '--verify', '1', '--evidence', 'checked', '--by', 'verifier']);

  const reopened = JSON.parse(run(cwd, [
    'card', 'criteria', '#1', '--reopen', '1', '--reason', 'phase moved back for a missed case',
    '--by', 'repairer', '--json',
  ]).out);
  assert.equal(reopened.status, 'open');
  assert.equal(reopened.evidence, '');
  assert.equal(reopened.metBy, null);
  assert.equal(reopened.verifiedBy, null);

  const event = JSON.parse(run(cwd, ['events', '--json']).out)
    .find(e => e.action === 'card.criteria-reopen');
  assert.equal(event.by, 'repairer');
  assert.match(event.note, /phase moved back for a missed case/);
  const help = run(cwd, ['help']).out;
  assert.match(help, /--reopen n --reason/);
  assert.match(help, /milestone verify/);
});

test('status renders and reports the open-card trend for a chosen window', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-status-trend-'));
  run(cwd, ['init', '--name', 'Status Trend']);
  run(cwd, ['card', 'add', '--title', 'Old open card']);
  const statePath = join(cwd, '.tower', 'tower.json');
  const state = JSON.parse(readFileSync(statePath, 'utf8'));
  state.cards[0].created = '2000-01-01T00:00:00.000Z';
  writeFileSync(statePath, JSON.stringify(state, null, 2) + '\n');
  run(cwd, ['card', 'add', '--title', 'New open card']);

  const human = run(cwd, ['status', '--days', '7', '--color=never']).out;
  assert.match(human, /OPEN TREND\s+1 → 2\s+\+1\s+over 7d/);
  const json = JSON.parse(run(cwd, ['status', '--window', '7', '--json']).out);
  assert.deepEqual({
    windowDays: json.trend.windowDays,
    openAtStart: json.trend.openAtStart,
    openNow: json.trend.openNow,
    delta: json.trend.delta,
  }, { windowDays: 7, openAtStart: 1, openNow: 2, delta: 1 });
  assert.equal(JSON.parse(run(cwd, ['status', '--json']).out).trend.windowDays, 7);
});

test('CLI --by owner and --quote cannot resolve acceptance; rejection is audited', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-accept-'));
  run(cwd, ['init', '--name', 'CLI Accept Guard']);
  run(cwd, ['card', 'add', '--title', 'Owner must inspect', '--by', 'builder']);
  run(cwd, ['card', 'update', '#1', '--needs-acceptance', 'true', '--by', 'builder']);
  run(cwd, ['card', 'criteria', '#1', '--add', 'thing works', '--by', 'planner']);
  run(cwd, ['card', 'criteria', '#1', '--meet', '1', '--evidence', 'built', '--by', 'builder']);
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

test('cli card tags, parent, and list filters', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-tags-'));
  run(cwd, ['init', '--name', 'Tags']);
  run(cwd, ['epoch', 'update', 'e1', '--name', 'E1']);
  run(cwd, ['epoch', 'current', 'e1']);
  const map = JSON.parse(run(cwd, [
    'card', 'add', '--title', 'Map', '--add-tag', 'wayfinder:map,needs-triage', '--json', '--by', 'tester',
  ]).out);
  assert.deepEqual(map.tags, ['wayfinder:map', 'needs-triage']);
  const child = JSON.parse(run(cwd, [
    'card', 'add', '--title', 'Child', '--parent', `#${map.num}`, '--add-tag', 'wayfinder:research', '--json', '--by', 'tester',
  ]).out);
  assert.equal(child.parentId, map.id);
  run(cwd, ['card', 'update', `#${map.num}`, '--remove-tag', 'needs-triage', '--add-tag', 'ready-for-agent', '--by', 'tester']);
  const tagged = JSON.parse(run(cwd, ['card', 'list', '--tag', 'ready-for-agent', '--json']).out);
  assert.equal(tagged.length, 1);
  assert.equal(tagged[0].num, map.num);
  const kids = JSON.parse(run(cwd, ['card', 'list', '--parent', `#${map.num}`, '--json']).out);
  assert.equal(kids.length, 1);
  assert.equal(kids[0].num, child.num);
  const untagged = JSON.parse(run(cwd, ['card', 'add', '--title', 'Plain', '--json', '--by', 'tester']).out);
  assert.deepEqual(untagged.tags, []);
  const plain = JSON.parse(run(cwd, ['card', 'list', '--untagged', '--json']).out);
  assert.equal(plain.some(c => c.num === untagged.num), true);
  assert.equal(plain.some(c => c.num === map.num), false);
});

test('an unrecognized flag is a usage error, never a silently dropped value', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-cli-flags-'));
  run(cwd, ['init', '--name', 'Flags']);
  run(cwd, ['epoch', 'update', 'e1', '--name', 'E1']);
  run(cwd, ['epoch', 'current', 'e1']);

  // The reported papercut: `--text` belongs to `papercut add`, not `card add`.
  // It used to be dropped, leaving an empty card behind.
  const bad = run(cwd, ['card', 'add', '--title', 'T', '--text', 'body?', '--by', 'tester'], false);
  assert.equal(bad.code, 1);
  assert.match(bad.out, /unknown flag for `tower card`: --text/);
  assert.equal(JSON.parse(run(cwd, ['card', 'list', '--json']).out).length, 0, 'no card was created');

  // Kebab-case is reported the way it was typed, and several are listed at once.
  const two = run(cwd, ['card', 'update', '#1', '--work-oder', '1', '--phse', 'done'], false);
  assert.match(two.out, /unknown flags for `tower card`: --work-oder, --phse/);

  // Globals stay valid everywhere, and real flags still work.
  const ok = JSON.parse(run(cwd, ['card', 'add', '--title', 'Real', '--json', '--by', 'tester']).out);
  assert.equal(ok.num, 1);
});
