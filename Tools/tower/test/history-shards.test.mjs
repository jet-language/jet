// #6564 c7: the sharded archive. `tower archive shard --apply` turns
// history.json into a manifest and moves the records into size-bounded
// shards under history/, which Tower reads back as one history. Nothing is
// deleted: a shard can be emptied but never removed.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import * as db from '../app/store.mjs';
import { empty, emptyHistory, openStore, readHistoryDoc, validateStoredHistory } from '../app/store.mjs';
import { configFile, historyFile, withDirectoryAuthority, writeJSON } from '../app/paths.mjs';
import { HISTORY_KINDS, HISTORY_LAYOUT, readHistoryAt, writeHistoryAt } from '../app/history-shards.mjs';
import { applyRepairManifest, canonicalPayloadHash } from '../app/repair.mjs';
import { beginRepairTransaction, hasPendingRepair } from '../app/repair-journal.mjs';

const TOWER = fileURLToPath(new URL('../tower.mjs', import.meta.url));
const BOUND = 2048;
const OLD = '2020-01-01T12:00:00.000Z';
const EVENTS = 120;

const event = (i) => ({
  at: `2026-01-01T00:00:${String(i % 60).padStart(2, '0')}.000Z`,
  by: 'tester', action: 'note', ref: null, note: `event ${i} ${'x'.repeat(i % 50)}`,
});

// A board whose single-file archive holds three retired cards and EVENTS events.
function legacyBoard() {
  const dir = mkdtempSync(join(tmpdir(), 'tower-shards-'));
  writeJSON(join(dir, 'tower.json'), empty('Shards'));
  writeJSON(configFile(dir), { project: 'Shards' });
  const st = openStore(dir);
  for (const title of ['one', 'two', 'three']) st.mutate((s, cfg) => db.addCard(s, { title }, cfg));
  for (const ref of ['#1', '#2', '#3']) st.mutate((s, cfg) => db.updateCard(s, ref, { phase: 'done', by: 'owner' }, cfg));
  st.mutate((s) => { for (const card of s.cards) card.updated = OLD; });
  st.mutate((s) => db.setDigestCursor(s));
  st.mutate((s, cfg, h) => { for (let i = 0; i < EVENTS; i++) h.events.push(event(i)); });
  return { dir, st };
}

const shard = (st, options = {}) => st.mutate(
  (s) => db.logEvent(s, { by: 'tester', action: 'archive.shard', ref: 'history.json' }),
  { historyLayout: 'shards', historyShardBytes: BOUND, ...options },
);
const manifestOf = (dir) => JSON.parse(readFileSync(historyFile(dir), 'utf8'));
const shardsOf = (manifest) => HISTORY_KINDS.flatMap(kind => manifest[kind].shards);
const assembled = (dir) => withDirectoryAuthority(dir, root => readHistoryAt(root, { parse: false })).bytes;
const identities = (dir) => Object.fromEntries(readdirSync(join(dir, 'history')).map(name => {
  const st = statSync(join(dir, 'history', name));
  return [name, `${st.ino}:${st.mtimeMs}`];
}));

test('a single-file archive stays one file until the cutover', () => {
  const { dir, st } = legacyBoard();
  assert.equal(existsSync(join(dir, 'history')), false);
  assert.equal(manifestOf(dir).layout, undefined);
  assert.equal(st.loadHistory().cards.length, 3);
  assert.equal(st.loadHistory().events.length, EVENTS);
});

test('the cutover keeps the history byte for byte, in shards under the bound', () => {
  const { dir, st } = legacyBoard();
  const before = readFileSync(historyFile(dir));
  const history = st.loadHistory();
  const { state } = shard(st);
  assert.equal(state.events[0].action, 'archive.shard');
  const manifest = manifestOf(dir);
  assert.equal(manifest.layout, HISTORY_LAYOUT);
  assert.equal(manifest.shardBytes, BOUND);
  assert.ok(manifest.events.shards.length > 2, 'the events need several shards at this bound');
  for (const entry of shardsOf(manifest)) {
    const data = readFileSync(join(dir, 'history', entry.file));
    assert.equal(data.length, entry.bytes);
    assert.ok(entry.bytes <= BOUND || entry.items === 1, `${entry.file} holds ${entry.bytes} bytes`);
  }
  assert.ok(assembled(dir).equals(before), 'the shards assemble to the single-file text');
  assert.deepEqual(st.loadHistory(), history);
  assert.deepEqual(readHistoryDoc(dir), JSON.parse(before.toString('utf8')));
});

test('an archive write rewrites only the shards that changed', () => {
  const { dir, st } = legacyBoard();
  shard(st);
  const before = identities(dir);
  const manifestBefore = statSync(historyFile(dir)).ino;
  st.mutate((s, cfg) => db.addCard(s, { title: 'live only' }, cfg));
  assert.deepEqual(identities(dir), before, 'a write that leaves the archive alone writes no shard');
  assert.equal(statSync(historyFile(dir)).ino, manifestBefore);
  const last = manifestOf(dir).events.shards.at(-1).file;
  st.mutate((s, cfg, h) => { h.events.push(event(EVENTS)); });
  const after = identities(dir);
  for (const name of Object.keys(before))
    if (name !== last) assert.equal(after[name], before[name], `${name} must not be rewritten`);
  assert.equal(st.loadHistory().events.at(-1).note, event(EVENTS).note);
});

test('an emptied shard stays on disk and in the manifest', () => {
  const { dir, st } = legacyBoard();
  shard(st);
  const count = manifestOf(dir).events.shards.length;
  st.mutate((s, cfg, h) => { h.events.splice(0); });
  const manifest = manifestOf(dir);
  assert.equal(manifest.events.shards.length, count);
  for (const entry of manifest.events.shards) {
    assert.equal(entry.items, 0);
    assert.ok(existsSync(join(dir, 'history', entry.file)));
  }
  assert.equal(st.loadHistory().events.length, 0);
  st.mutate((s, cfg, h) => { for (let i = 0; i < EVENTS; i++) h.events.push(event(i)); });
  assert.equal(manifestOf(dir).events.shards.length, count);
  assert.equal(st.loadHistory().events.length, EVENTS);
});

test('a shard that does not match history.json is refused, never read as part of a history', () => {
  const { dir, st } = legacyBoard();
  shard(st);
  const file = join(dir, 'history', manifestOf(dir).events.shards[0].file);
  const good = readFileSync(file, 'utf8');
  const torn = (error) => error.code === 'E_INVALID' && /history shard history\/events-001\.json is missing or does not match history\.json/.test(error.message);
  writeFileSync(file, good.replace('event 0 ', 'event X '));
  assert.throws(() => st.load(), torn);
  const live = readFileSync(join(dir, 'tower.json'));
  assert.throws(() => st.mutate((s, cfg) => db.addCard(s, { title: 'refused' }, cfg)), torn);
  assert.ok(readFileSync(join(dir, 'tower.json')).equals(live), 'a refused write changes nothing');
  writeFileSync(file, good);
  renameSync(file, `${file}.away`);
  assert.throws(() => st.load(), torn);
  renameSync(`${file}.away`, file);
  assert.equal(st.loadHistory().events.length, EVENTS);
});

test('older Tower code refuses a sharded history.json instead of reading it as empty', () => {
  const { dir, st } = legacyBoard();
  shard(st);
  assert.throws(() => validateStoredHistory(manifestOf(dir)), /history store\.decisions must be an array/);
});

test('a pending transaction rolls a sharded archive back from its backup', () => {
  const { dir, st } = legacyBoard();
  shard(st);
  const history = st.loadHistory();
  const live = readFileSync(join(dir, 'tower.json'));
  writeFileSync(join(dir, 'backups', 'tower-crash.json'), live);
  writeFileSync(join(dir, 'backups', 'history-crash.json'), assembled(dir));
  beginRepairTransaction(dir, {
    liveBackup: 'tower-crash.json', historyBackup: 'history-crash.json', historyLayout: 'shards', manifestHash: 'crash',
  });
  // The crash: the live board already replaced and one shard half written.
  writeFileSync(join(dir, 'history', manifestOf(dir).events.shards[0].file), '{\n  "events": [\n');
  const newer = JSON.parse(live.toString('utf8'));
  newer.meta.rev += 1;
  writeJSON(join(dir, 'tower.json'), newer);
  assert.deepEqual(st.loadHistory(), history);
  assert.ok(readFileSync(join(dir, 'tower.json')).equals(live));
  assert.equal(manifestOf(dir).layout, HISTORY_LAYOUT);
  assert.equal(hasPendingRepair(dir), false);
});

test('archive restore keeps a sharded archive sharded', () => {
  const { dir, st } = legacyBoard();
  shard(st);
  const { result } = st.restoreArchived('#2', 'owner');
  assert.equal(result.num, 2);
  assert.ok(st.load().cards.find(card => card.num === 2));
  assert.equal(manifestOf(dir).layout, HISTORY_LAYOUT);
  assert.deepEqual(readHistoryDoc(dir), st.loadHistory());
});

test('repair applies to a sharded archive and keeps it sharded', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-shards-repair-'));
  const live = {
    meta: { version: 4, project: 'Repair', nextNum: 2, rev: 7, ui: { toggled: [] } },
    epochs: [], milestones: [],
    cards: [{ id: 'c1', num: 1, body: 'bad #Marker', title: 'unchanged' }],
    decisions: [], questions: [], ideas: [], events: [],
  };
  const history = {
    version: 1,
    decisions: [{ id: 'D-1', detail: 'bad #Other' }],
    cards: [{ id: 'archived', body: 'untouched' }],
    events: [{ at: '2025-01-01T00:00:00.000Z', by: 'old', action: 'history.event', ref: null, note: 'keep' }],
  };
  writeFileSync(join(dir, 'tower.json'), `${JSON.stringify(live, null, 2)}\n`);
  writeFileSync(join(dir, 'config.json'), '{"project":"Repair"}\n');
  withDirectoryAuthority(dir, root => writeHistoryAt(root, history, 'shards', { bound: 256 }));
  const payload = {
    expectedRev: 7,
    revPath: 'tower.json#/meta/rev',
    counts: {
      fields: 2, substitutions: 2,
      byCollection: { cards: { fields: 1, substitutions: 1 }, decisions: { fields: 1, substitutions: 1 } },
      byStore: { 'history.json': { fields: 1, substitutions: 1 }, 'tower.json': { fields: 1, substitutions: 1 } },
    },
    patches: [
      { store: 'tower.json', collection: 'cards', key: { id: 'c1' }, path: '/body', current: 'bad #Marker', replacement: 'bad @Marker', substitutions: 1 },
      { store: 'history.json', collection: 'decisions', key: { id: 'D-1' }, path: '/detail', current: 'bad #Other', replacement: 'bad @Other', substitutions: 1 },
    ],
  };
  const manifest = {
    schema: 'tower.repair-manifest/v1',
    canonicalization: 'Recursive lexicographic object-key ordering; array order preserved; compact UTF-8 JSON; sha256 covers payload only.',
    payload,
    sha256: canonicalPayloadHash(payload),
  };
  assert.equal(applyRepairManifest(dir, manifest, { expectRev: 7, by: 'repairer' }).rev, 8);
  assert.deepEqual(readHistoryDoc(dir), { ...history, decisions: [{ id: 'D-1', detail: 'bad @Other' }] });
  assert.equal(JSON.parse(readFileSync(join(dir, 'tower.json'), 'utf8')).cards[0].body, 'bad @Marker');
  assert.equal(manifestOf(dir).layout, HISTORY_LAYOUT);
  assert.equal(hasPendingRepair(dir), false);
  const backup = readdirSync(join(dir, 'backups')).find(name => name.startsWith('history-'));
  assert.equal(readFileSync(join(dir, 'backups', backup), 'utf8'), `${JSON.stringify(history, null, 2)}\n`, 'the backup is the single-file text');
});

test('board commits add new shards beside history.json', () => {
  const gitEnv = { ...process.env };
  for (const key of ['GIT_INDEX_FILE', 'GIT_DIR', 'GIT_WORK_TREE']) delete gitEnv[key];
  const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', env: gitEnv }).trim();
  const root = mkdtempSync(join(tmpdir(), 'tower-shards-git-'));
  git(root, 'init', '-q', '-b', 'master');
  git(root, 'config', 'user.name', 't');
  git(root, 'config', 'user.email', 't@t');
  const data = join(root, '.tower');
  mkdirSync(data);
  writeJSON(join(data, 'tower.json'), empty('Board'));
  writeJSON(historyFile(data), emptyHistory());
  writeJSON(configFile(data), { project: 'Board', boardBatch: 0 });
  git(root, 'add', '.tower/tower.json', '.tower/history.json');
  git(root, 'commit', '-q', '-m', 'start');
  const st = openStore(data);
  st.mutate((s, cfg, h) => { for (let i = 0; i < EVENTS; i++) h.events.push(event(i)); });
  shard(st);
  const done = st.commitBoard('closer');
  assert.ok(done.committed, done.reason);
  const tracked = git(root, 'ls-files', '.tower').split('\n');
  for (const entry of shardsOf(manifestOf(data))) assert.ok(tracked.includes(`.tower/history/${entry.file}`), entry.file);
  assert.equal(JSON.parse(git(root, 'show', 'HEAD:.tower/history.json')).layout, HISTORY_LAYOUT);
  assert.doesNotMatch(git(root, 'status', '--porcelain', '--', '.tower/history', '.tower/history.json', '.tower/tower.json'), /\S/);
});

test('cli: archive shard is a dry run until --apply, then reports the shards', () => {
  const { dir } = legacyBoard();
  const cli = (...args) => spawnSync(process.execPath, [TOWER, '--data', dir, ...args], { encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
  const before = readFileSync(historyFile(dir));
  const dry = cli('archive', 'shard');
  assert.equal(dry.status, 0, dry.stderr);
  assert.match(dry.stdout, /^dry run: history\.json \(0\.0 MiB\) would become a manifest and 3 shards under history\/, each at most 8\.0 MiB/);
  assert.ok(readFileSync(historyFile(dir)).equals(before), 'the dry run writes nothing');
  assert.equal(existsSync(join(dir, 'history')), false);
  assert.match(cli('archive', 'shard', '--apply').stderr, /archive shard --apply needs --by/);
  const applied = cli('archive', 'shard', '--apply', '--by', 'tester');
  assert.equal(applied.status, 0, applied.stderr);
  assert.match(applied.stdout, /^sharded at rev \d+: 3 shards under history\//);
  assert.match(cli('archive', 'shard').stdout, /^history is already sharded: 3 shards under history\//);
  const status = JSON.parse(cli('archive', 'status', '--json').stdout);
  assert.equal(status.layout, 'shards');
  assert.equal(status.shards, 3);
  assert.equal(status.events, EVENTS);
  assert.ok(assembled(dir).equals(before));
});
