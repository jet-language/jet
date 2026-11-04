// #6564 c5: the board holds no personal or machine information. `tower redact`
// rewrites old spans; a write that adds one is refused with the exact fix.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { openStore, empty, TowerError } from '../app/store.mjs';
import * as db from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import {
  personalInfoMatcher, personalInfoRules, personalInfoAdded, redactBoard, tallyValue,
} from '../app/personal-info.mjs';

const TOWER = fileURLToPath(new URL('../tower.mjs', import.meta.url));
const home = homedir();

const identity = {
  home: '/home/zed', user: 'zed', host: 'zedbox', scratchRoot: '/mnt/scr',
  mounts: ['/mnt/scr', '/media/usb'], email: '123+ZedQ@users.noreply.github.com', name: 'ZedQ',
};
const matcher = personalInfoMatcher(personalInfoRules(identity));

test('the rule table maps each machine fact to its neutral form', () => {
  const cases = [
    ['log at /mnt/scr/lanes/X/run.log', 'log at $JET_SCRATCH_ROOT/lanes/X/run.log'],
    ['cd /home/zed && ls /home/zed/.cache/jet-dev', 'cd ~ && ls ~/.cache/jet-dev'],
    ['/Users/zed/src', '~/src'],
    ['/etc/profiles/per-user/zed/bin/fish', '/etc/profiles/per-user/$USER/bin/fish'],
    ['copied to /media/usb/backup', 'copied to /mnt/volume/backup'],
    ['Zed wants zedbox; ZEDBOX and Zedbox reboot', 'User wants devhost; DEVHOST and Devhost reboot'],
    ['authored by ZedQ <123+ZedQ@users.noreply.github.com>', 'authored by owner <user@example.com>'],
    ['/home/zedd and /mnt/scratch and zedboxes stay', '/home/zedd and /mnt/scratch and zedboxes stay'],
  ];
  for (const [input, expected] of cases) assert.equal(matcher.replace(input), expected, input);
});

test('generic and short names never become rules', () => {
  const rules = personalInfoRules({ home: '/home/runner', user: 'runner', host: 'localhost', scratchRoot: '', mounts: [], email: '', name: '' });
  assert.deepEqual(rules.map(rule => rule.kind), ['path', 'path', 'path']);
  assert.equal(personalInfoMatcher(rules).replace('the runner on localhost'), 'the runner on localhost');
  assert.equal(personalInfoMatcher(rules).replace('/home/runner/work'), '~/work');
});

test('a refusal names only the added span and the text to write instead', () => {
  const before = tallyValue({ body: 'old note in /home/zed/notes' }, matcher);
  assert.equal(personalInfoAdded(matcher, before, { body: 'old note in /home/zed/notes' }), null);
  const issue = personalInfoAdded(matcher, before, { body: 'old note in /home/zed/notes', log: ['see /mnt/scr/lanes/Main/x.md'] });
  assert.match(issue, /#6564 c5/);
  assert.match(issue, /write "\$JET_SCRATCH_ROOT\/lanes\/Main\/x\.md" instead of "\/mnt\/scr\/lanes\/Main\/x\.md"/);
  assert.doesNotMatch(issue, /notes/);
});

test('redactBoard rewrites every section, ratified decisions included, and counts them', () => {
  const state = {
    cards: [{ id: 'c1', body: 'built on zedbox', log: [{ text: 'log at /mnt/scr/a.log' }] }],
    decisions: [{ id: 'D-A', status: 'ratified', detail: 'Zed ruled from /home/zed' }, { id: 'D-B', status: 'open', detail: 'none' }],
    briefings: [{ body: 'zedbox' }],
    papercuts: [{ text: '/home/zed/x' }],
  };
  const history = { decisions: [{ id: 'D-C', status: 'ratified', code: 'u zed' }], cards: [], events: [{ note: 'by ZedQ' }] };
  const report = redactBoard(state, history, matcher);
  assert.equal(report.total, 8);
  assert.deepEqual(report.sections, {
    'tower.json cards': 2, 'tower.json decisions (ratified)': 2, 'tower.json briefings': 1,
    'tower.json papercuts': 1, 'history.json decisions (ratified)': 1, 'history.json events': 1,
  });
  assert.equal(state.decisions[0].detail, 'User ruled from ~');
  assert.equal(state.cards[0].log[0].text, 'log at $JET_SCRATCH_ROOT/a.log');
  assert.equal(history.events[0].note, 'by owner');
  assert.equal(tallyValue([state, history], matcher).size, 0);
});

function board() {
  const data = join(mkdtempSync(join(tmpdir(), 'tower-personal-')), '.tower');
  mkdirSync(data);
  writeJSON(join(data, 'tower.json'), empty('Personal'));
  const st = openStore(data);
  st.mutate((s, cfg) => db.addCard(s, { title: 'Work' }, cfg));
  return { data, st };
}

test('a write that adds this machine\'s home path is refused; old text may stay', () => {
  const { data, st } = board();
  const before = JSON.stringify(st.load().cards[0]);
  assert.throws(
    () => st.mutate((s, cfg) => db.updateCard(s, '#1', { logEntry: `notes in ${home}/notes`, by: 'agent' }, cfg)),
    (e) => e instanceof TowerError && e.code === 'E_PERSONAL_INFO'
      && e.message.includes('write "~/notes" instead of'),
  );
  assert.equal(JSON.stringify(st.load().cards[0]), before, 'a refused write changes nothing');

  // Text already on the board (written before the guard) does not block other writes.
  const raw = JSON.parse(readFileSync(join(data, 'tower.json'), 'utf8'));
  raw.cards[0].body = `legacy note in ${home}/old`;
  writeJSON(join(data, 'tower.json'), raw);
  st.mutate((s, cfg) => db.updateCard(s, '#1', { logEntry: 'unrelated progress', by: 'agent' }, cfg));

  const dry = spawnSync(process.execPath, [TOWER, 'redact', '--data', data, '--json'], { encoding: 'utf8' });
  assert.equal(dry.status, 0, dry.stderr);
  assert.equal(JSON.parse(dry.stdout).applied, false);
  assert.ok(st.load().cards[0].body.includes(home), 'the dry run writes nothing');

  const applied = spawnSync(process.execPath, [TOWER, 'redact', '--apply', '--by', 'main', '--data', data], { encoding: 'utf8' });
  assert.equal(applied.status, 0, applied.stderr);
  assert.match(applied.stdout, /^redacted \d+ spans at rev \d+/);
  const s = st.load();
  assert.equal(s.cards[0].body, 'legacy note in ~/old');
  assert.equal(s.events[0].action, 'board.redact');
  assert.match(s.events[0].note, /^personal-info\/v1: \d+ spans redacted \(tower\.json cards \d+/);
  assert.ok(!JSON.stringify(s).includes(home));
});
