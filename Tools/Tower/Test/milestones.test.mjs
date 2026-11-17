// Milestones are open, met or retired; they are worked in `order` within an
// epoch, carry card-shaped `exitCriteria`, and move between epochs with their
// linked cards.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../App/store.mjs';
import { configFile, writeJSON } from '../App/paths.mjs';
import { serve } from '../App/server.mjs';

const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
const gitEnv = { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' };
for (const key of ['GIT_INDEX_FILE', 'GIT_DIR', 'GIT_WORK_TREE']) delete gitEnv[key];
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...gitEnv, TOWER_DATA: '' } });
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', env: gitEnv }).trim();

function board() {
  const data = mkdtempSync(join(tmpdir(), 'tower-milestones-'));
  writeJSON(join(data, 'tower.json'), db.empty('Milestones'));
  return { data, st: db.openStore(data) };
}

// A board inside a git repo with a commit on dev, so evidence is checked.
function gitBoard() {
  const root = mkdtempSync(join(tmpdir(), 'tower-milestones-git-'));
  git(root, 'init', '-q', '-b', 'dev');
  writeFileSync(join(root, 'a.txt'), 'a\n');
  git(root, 'add', 'a.txt');
  git(root, 'commit', '-q', '-m', 'on dev');
  const data = join(root, '.tower');
  mkdirSync(data);
  writeJSON(join(data, 'tower.json'), db.empty('Milestones'));
  const st = db.openStore(data);
  st.mutate((s) => db.addMilestone(s, { id: 'm-1', epochId: 'e1', title: 'MVP', exitCriteria: ['user path works'] }));
  return { data, st, onDev: git(root, 'rev-parse', 'HEAD') };
}

test('load migrates the old milestone shape: review-ready is open, archived is retired, criteria are exitCriteria', () => {
  const { data } = board();
  const raw = db.empty('Old');
  raw.milestones = [
    { id: 'm-ready', epochId: 'e1', title: 'Ready', goal: '', status: 'review-ready', created: '2026-01-01T00:00:00.000Z',
      criteria: [{ n: 1, text: 'works', status: 'met', metBy: 'builder', verifiedBy: null, evidence: 'ran it', at: '2026-01-02T00:00:00.000Z' }] },
    { id: 'm-old', epochId: 'e1', title: 'Old', goal: '', status: 'open', created: '2026-01-03T00:00:00.000Z', archived: true, criteria: [] },
    { id: 'm-live', epochId: 'e1', title: 'Live', goal: '', status: 'open', created: '2026-01-04T00:00:00.000Z', archived: false },
  ];
  writeJSON(join(data, 'tower.json'), raw);
  const s = db.openStore(data).load();
  const [ready, old, live] = s.milestones;
  assert.equal(ready.status, 'open');
  assert.deepEqual(ready.exitCriteria.map(i => [i.n, i.text, i.status]), [[1, 'works', 'met']]);
  assert.equal('criteria' in ready, false);
  assert.equal(old.status, 'retired');
  assert.deepEqual(old.retired, { reason: 'archived', by: 'tower', at: '2026-01-03T00:00:00.000Z' });
  assert.equal('archived' in old, false);
  assert.equal(live.status, 'open');
  assert.equal('archived' in live, false);
  assert.deepEqual(live.exitCriteria, []);
});

test('retire hides a milestone from list and radar, refuses new links, and keeps existing ones', () => {
  const { data, st } = board();
  st.mutate((s) => db.addMilestone(s, { id: 'm-1', epochId: 'e1', title: 'MVP' }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Linked', epoch: 'e1', milestoneId: 'm-1' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Other', epoch: 'e1' }, cfg));
  assert.match(cli(data, 'milestone', 'retire', 'm-1', '--by', 'owner').stderr, /needs --reason/);
  const retired = cli(data, 'milestone', 'retire', 'm-1', '--reason', 'folded into m-2', '--by', 'owner', '--json');
  assert.equal(retired.status, 0, retired.stderr);
  assert.deepEqual(JSON.parse(retired.stdout).retired.reason, 'folded into m-2');
  assert.match(cli(data, 'milestone', 'retire', 'm-1', '--reason', 'again', '--by', 'owner').stderr, /already retired/);

  assert.deepEqual(JSON.parse(cli(data, 'milestone', 'list', '--json').stdout), []);
  const all = JSON.parse(cli(data, 'milestone', 'list', '--retired', '--json').stdout);
  assert.deepEqual(all.map(m => [m.id, m.status]), [['m-1', 'retired']]);
  assert.match(cli(data, 'milestone', 'list', '--retired').stdout, /\[retired: folded into m-2\]/);
  assert.deepEqual(db.radarData(st.load()).flatMap(r => r.milestones || []), []);

  const s = st.load();
  assert.equal(s.cards[0].milestoneId, 'm-1', 'an existing link stays');
  assert.equal(s.events.find(e => e.action === 'milestone.retire')?.ref, 'm-1');
  assert.throws(() => st.mutate((s, cfg) => db.updateCard(s, '#2', { milestoneId: 'm-1', by: 'agent' }, cfg)),
    /milestone m-1 is retired \(folded into m-2\); link the card to a live milestone/);
  st.mutate((s, cfg) => db.updateCard(s, '#1', { title: 'Linked, renamed', by: 'agent' }, cfg));
  assert.throws(() => st.mutate((s) => db.addMilestoneCriterion(s, 'm-1', 'more', 'agent')), /m-1 is retired/);
});

test('milestones list in epoch order, then milestone order', () => {
  const { data, st } = board();
  st.mutate((s) => {
    db.updateEpoch(s, 'e1', { order: 2 });
    db.addEpoch(s, { id: 'e2', name: 'Two', order: 1 });
    db.addMilestone(s, { id: 'm-late', epochId: 'e1', title: 'Late', order: 2 });
    db.addMilestone(s, { id: 'm-loose', epochId: 'e1', title: 'Unordered' });
    db.addMilestone(s, { id: 'm-early', epochId: 'e1', title: 'Early', order: 1 });
    db.addMilestone(s, { id: 'm-first', epochId: 'e2', title: 'First epoch' });
  });
  const ids = JSON.parse(cli(data, 'milestone', 'list', '--json').stdout).map(m => m.id);
  assert.deepEqual(ids, ['m-first', 'm-early', 'm-late', 'm-loose']);
  const moved = cli(data, 'milestone', 'update', 'm-loose', '--order', '0', '--by', 'owner', '--json');
  assert.equal(moved.status, 0, moved.stderr);
  assert.equal(JSON.parse(cli(data, 'milestone', 'list', '--epoch', 'e1', '--json').stdout)[0].id, 'm-loose');
  assert.match(cli(data, 'milestone', 'update', 'm-loose', '--order', 'soon', '--by', 'owner').stderr, /milestone order must be a whole number/);
});

test('cli: --exit-criteria sets a milestone checklist shaped like card criteria', () => {
  const { data, st } = board();
  const added = cli(data, 'milestone', 'add', '--id', 'm-1', '--epoch', 'e1', '--title', 'MVP',
    '--exit-criteria', 'parses', '--exit-criteria', 'checks', '--order', '3', '--by', 'planner', '--json');
  assert.equal(added.status, 0, added.stderr);
  const m = st.load().milestones[0];
  assert.equal(m.order, 3);
  assert.deepEqual(m.exitCriteria.map(i => [i.n, i.text, i.status]), [[1, 'parses', 'open'], [2, 'checks', 'open']]);
  cli(data, 'milestone', 'update', 'm-1', '--exit-criteria', 'ships', '--by', 'planner');
  assert.deepEqual(st.load().milestones[0].exitCriteria.map(i => i.text), ['ships']);
  assert.deepEqual(JSON.parse(cli(data, 'milestone', 'criteria', 'm-1', '--json').stdout).map(i => i.text), ['ships']);
  assert.match(cli(data, 'milestone', 'update', 'm-1', '--archive', '--by', 'owner').stderr, /--archive/);
});

test('cli: a milestone criterion meet passes the evidence gate', () => {
  const { data, st, onDev } = gitBoard();
  const bad = cli(data, 'milestone', 'criteria', 'm-1', '--meet', '1', '--evidence', 'ran it', '--by', 'builder', '--json');
  assert.equal(bad.status, 1);
  assert.equal(JSON.parse(bad.stdout).error.code, 'E_EVIDENCE');
  assert.match(bad.stderr, /milestone m-1 criterion #1: evidence names no commit on dev/);
  assert.equal(st.load().milestones[0].exitCriteria[0].status, 'open');
  const good = cli(data, 'milestone', 'criteria', 'm-1', '--meet', '1', '--evidence', `landed ${onDev.slice(0, 10)}`, '--by', 'builder');
  assert.equal(good.status, 0, good.stderr);
  const declared = cli(data, 'milestone', 'criteria', 'm-1', '--verify', '1', '--kind', 'obsolete', '--evidence', 'superseded by the parser rewrite', '--by', 'reviewer');
  assert.equal(declared.status, 0, declared.stderr);
  assert.deepEqual(st.load().milestones[0].exitCriteria.map(i => [i.status, i.closure]), [['verified', 'obsolete']]);
});

test('server: milestone criteria-meet passes the evidence gate; retire is a route', async () => {
  const { data, st, onDev } = gitBoard();
  writeJSON(configFile(data), { project: 'Milestones' });
  const PORT = 17961;
  const server = serve(db.openStore(data), PORT, false);
  after(() => server.close());
  const post = async (route, body) => {
    const r = await fetch(`http://localhost:${PORT}/api/${route}`, {
      method: 'POST', headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify(body),
    });
    return { status: r.status, json: await r.json() };
  };
  const bad = await post('milestone/criteria-meet', { id: 'm-1', n: 1, evidence: 'ran it', by: 'builder' });
  assert.notEqual(bad.status, 200);
  assert.match(JSON.stringify(bad.json), /evidence names no commit/);
  const good = await post('milestone/criteria-meet', { id: 'm-1', n: 1, evidence: `landed ${onDev.slice(0, 10)}`, by: 'builder' });
  assert.equal(good.status, 200, JSON.stringify(good.json));
  const retired = await post('milestone/retire', { id: 'm-1', reason: 'done elsewhere', by: 'owner' });
  assert.equal(retired.status, 200, JSON.stringify(retired.json));
  assert.equal(st.load().milestones[0].status, 'retired');
});

test('cli: milestone update --epoch moves the milestone and its live cards; apply retires', () => {
  const { data, st } = board();
  st.mutate((s) => {
    db.addEpoch(s, { id: 'e2', name: 'Two' });
    db.addMilestone(s, { id: 'm-1', epochId: 'e1', title: 'MVP' });
  });
  st.mutate((s, cfg) => db.addCard(s, { title: 'Linked', epoch: 'e1', milestoneId: 'm-1' }, cfg));
  const moved = cli(data, 'milestone', 'update', 'm-1', '--epoch', 'e2', '--by', 'planner');
  assert.equal(moved.status, 0, moved.stderr);
  assert.match(moved.stdout, /updated milestone m-1 in e2/);
  assert.equal(st.load().cards[0].epoch, 'e2');

  const plan = join(data, 'plan.json');
  writeFileSync(plan, JSON.stringify({ ops: [{ op: 'milestone.retire', id: 'm-1', reason: 'scope cut' }] }));
  const applied = cli(data, 'apply', '--file', plan, '--by', 'owner');
  assert.equal(applied.status, 0, applied.stderr);
  assert.deepEqual(st.load().milestones[0].retired.reason, 'scope cut');
});
