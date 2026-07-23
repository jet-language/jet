import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  existsSync, mkdtempSync, readFileSync, readdirSync, renameSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  applyRepairManifest, canonicalPayloadHash, commitRepairPair,
} from '../app/repair.mjs';
import {
  beginRepairTransaction, finishRepairTransaction, hasPendingRepair,
} from '../app/repair-journal.mjs';
import { backupRequired } from '../app/paths.mjs';
import { openStore } from '../app/store.mjs';

const CANONICALIZATION = 'Recursive lexicographic object-key ordering; array order preserved; compact UTF-8 JSON; sha256 covers payload only.';

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'tower-repair-'));
  const live = {
    meta: { version: 4, project: 'Repair', nextNum: 2, rev: 7, ui: { toggled: ['keep'] } },
    epochs: [], milestones: [],
    cards: [{ id: 'c1', num: 1, body: 'bad #Marker', title: 'unchanged', nested: { keep: true } }],
    decisions: [], questions: [], ideas: [],
    events: [{ at: '2026-01-01T00:00:00.000Z', by: 'old', action: 'old.event', ref: 'c1', note: 'keep' }],
  };
  const history = {
    version: 1,
    decisions: [{ id: 'D-1', detail: 'bad #Other', keep: ['same'] }],
    cards: [{ id: 'archived', body: 'untouched' }],
    events: [{ at: '2025-01-01T00:00:00.000Z', by: 'old', action: 'history.event', ref: null, note: 'keep' }],
  };
  writeFileSync(join(dir, 'tower.json'), JSON.stringify(live, null, 2) + '\n');
  writeFileSync(join(dir, 'history.json'), JSON.stringify(history, null, 2) + '\n');
  writeFileSync(join(dir, 'config.json'), '{"project":"Repair","backups":20}\n');
  return { dir, live, history, manifest: manifest() };
}

function manifest() {
  const payload = {
    expectedRev: 7,
    revPath: 'tower.json#/meta/rev',
    counts: {
      fields: 2,
      substitutions: 2,
      byCollection: {
        cards: { fields: 1, substitutions: 1 },
        decisions: { fields: 1, substitutions: 1 },
      },
      byStore: {
        'history.json': { fields: 1, substitutions: 1 },
        'tower.json': { fields: 1, substitutions: 1 },
      },
    },
    patches: [
      {
        store: 'tower.json', collection: 'cards', key: { id: 'c1' }, path: '/body',
        current: 'bad #Marker', replacement: 'bad @Marker', substitutions: 1,
      },
      {
        store: 'history.json', collection: 'decisions', key: { id: 'D-1' }, path: '/detail',
        current: 'bad #Other', replacement: 'bad @Other', substitutions: 1,
      },
    ],
  };
  return {
    schema: 'tower.repair-manifest/v1',
    canonicalization: CANONICALIZATION,
    payload,
    sha256: canonicalPayloadHash(payload),
  };
}

const bytes = (dir) => ({
  live: readFileSync(join(dir, 'tower.json'), 'utf8'),
  history: readFileSync(join(dir, 'history.json'), 'utf8'),
});

test('repair applies exact leaves, bumps rev once, audits once, backs up both, and preserves all else', () => {
  const { dir, live, history, manifest: m } = fixture();
  const result = applyRepairManifest(dir, m, { expectRev: 7, by: 'repairer' });
  assert.deepEqual(result, {
    dryRun: false, manifestHash: m.sha256, fields: 2, substitutions: 2, previousRev: 7, rev: 8,
  });

  const afterLive = JSON.parse(readFileSync(join(dir, 'tower.json'), 'utf8'));
  const afterHistory = JSON.parse(readFileSync(join(dir, 'history.json'), 'utf8'));
  const event = afterLive.events.shift();
  assert.deepEqual(afterLive, {
    ...live, meta: { ...live.meta, rev: 8 },
    cards: [{ ...live.cards[0], body: 'bad @Marker' }],
    events: live.events,
  });
  assert.deepEqual(afterHistory, {
    ...history,
    decisions: [{ ...history.decisions[0], detail: 'bad @Other' }],
  });
  assert.equal(event.by, 'repairer');
  assert.equal(event.action, 'repair.apply');
  assert.equal(event.manifestHash, m.sha256);
  assert.equal(event.fields, 2);
  assert.equal(event.substitutions, 2);
  assert.match(event.at, /^\d{4}-\d\d-\d\dT/);

  const backups = readdirSync(join(dir, 'backups'));
  assert.equal(backups.some(x => x.startsWith('tower-')), true);
  assert.equal(backups.some(x => x.startsWith('history-')), true);
});

test('repair dry-run validates and changes no bytes', () => {
  const { dir, manifest: m } = fixture();
  const before = bytes(dir);
  const output = applyRepairManifest(dir, m, {
    expectRev: 7, by: 'repairer', dryRun: true,
  });
  assert.equal(output.dryRun, true);
  assert.deepEqual(bytes(dir), before);
  assert.equal(existsSync(join(dir, 'backups')), false);
});

test('repair refuses expected-rev drift without any write', () => {
  const { dir, manifest: m } = fixture();
  const before = bytes(dir);
  assert.throws(
    () => applyRepairManifest(dir, m, { expectRev: 6, by: 'repairer' }),
    (error) => error.code === 'E_CONFLICT');
  assert.deepEqual(bytes(dir), before);
});

test('repair refuses leaf drift without any write', () => {
  const { dir, manifest: m } = fixture();
  const live = JSON.parse(readFileSync(join(dir, 'tower.json'), 'utf8'));
  live.cards[0].body = 'changed concurrently';
  writeFileSync(join(dir, 'tower.json'), JSON.stringify(live, null, 2) + '\n');
  const before = bytes(dir);
  assert.throws(
    () => applyRepairManifest(dir, m, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_REPAIR_DRIFT');
  assert.deepEqual(bytes(dir), before);
});

test('repair refuses hash and independently valid count mismatches', () => {
  const { dir, manifest: m } = fixture();
  const before = bytes(dir);
  assert.throws(
    () => applyRepairManifest(dir, { ...m, sha256: '0'.repeat(64) }, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST_HASH');

  const badCounts = structuredClone(m);
  badCounts.payload.counts.fields = 3;
  badCounts.sha256 = canonicalPayloadHash(badCounts.payload);
  assert.throws(
    () => applyRepairManifest(dir, badCounts, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST_COUNTS');
  assert.deepEqual(bytes(dir), before);
});

test('pair commit rolls live back when history commit fails', () => {
  const { dir } = fixture();
  const before = bytes(dir);
  let renames = 0;
  assert.throws(() => commitRepairPair({
    dataDir: dir,
    liveFile: join(dir, 'tower.json'),
    historyFile: join(dir, 'history.json'),
    live: { changed: 'live' },
    history: { changed: 'history' },
    originalLive: before.live,
    originalHistory: before.history,
    liveBackup: backupRequired(join(dir, 'tower.json')),
    historyBackup: backupRequired(join(dir, 'history.json')),
    manifestHash: 'a'.repeat(64),
    rename: (from, to) => {
      renames += 1;
      if (renames === 2) throw new Error('injected history rename failure');
      renameSync(from, to);
    },
  }), (error) => error.code === 'E_REPAIR_IO');
  assert.deepEqual(bytes(dir), before);
  assert.equal(hasPendingRepair(dir), false);
});

test('repair rejects duplicate targets and unstable object keys', () => {
  const { dir, manifest: m } = fixture();
  const duplicate = structuredClone(m);
  duplicate.payload.patches.push(structuredClone(duplicate.payload.patches[0]));
  duplicate.payload.counts.fields += 1;
  duplicate.payload.counts.substitutions += 1;
  duplicate.payload.counts.byCollection.cards.fields += 1;
  duplicate.payload.counts.byCollection.cards.substitutions += 1;
  duplicate.payload.counts.byStore['tower.json'].fields += 1;
  duplicate.payload.counts.byStore['tower.json'].substitutions += 1;
  duplicate.sha256 = canonicalPayloadHash(duplicate.payload);
  assert.throws(
    () => applyRepairManifest(dir, duplicate, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST');

  const unstable = structuredClone(m);
  unstable.payload.patches[0].key = { id: 'c1', title: 'unchanged' };
  unstable.sha256 = canonicalPayloadHash(unstable.payload);
  assert.throws(
    () => applyRepairManifest(dir, unstable, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST');
});

test('store recovers a crash after either repair rename before returning normal reads', () => {
  for (const phase of ['prepared', 'live-renamed', 'both-renamed']) {
    const { dir } = fixture();
    const before = bytes(dir);
    const store = openStore(dir);
    const liveBackup = backupRequired(join(dir, 'tower.json'));
    const historyBackup = backupRequired(join(dir, 'history.json'));
    beginRepairTransaction(dir, { liveBackup, historyBackup, manifestHash: 'b'.repeat(64) });
    if (phase !== 'prepared') writeFileSync(join(dir, 'tower.json'), '{"split":"live"}\n');
    if (phase === 'both-renamed') writeFileSync(join(dir, 'history.json'), '{"split":"history"}\n');

    const state = store.load();
    assert.equal(state.meta.rev, 7);
    assert.deepEqual(bytes(dir), before);
    assert.equal(hasPendingRepair(dir), false);
  }
});

test('existing store recovers a crashed repair before its next write', () => {
  const { dir, history } = fixture();
  const store = openStore(dir);
  const liveBackup = backupRequired(join(dir, 'tower.json'));
  const historyBackup = backupRequired(join(dir, 'history.json'));
  beginRepairTransaction(dir, { liveBackup, historyBackup, manifestHash: 'c'.repeat(64) });
  writeFileSync(join(dir, 'tower.json'), '{"split":"live"}\n');
  writeFileSync(join(dir, 'history.json'), '{"split":"history"}\n');

  store.mutate(state => { state.meta.project = 'Recovered then written'; });
  assert.equal(store.load().meta.project, 'Recovered then written');
  assert.equal(store.load().meta.rev, 8);
  assert.deepEqual(JSON.parse(readFileSync(join(dir, 'history.json'), 'utf8')), history);
  assert.equal(hasPendingRepair(dir), false);
});

test('existing store drops warm history after a successful external repair', () => {
  const { dir, manifest: m } = fixture();
  const existing = openStore(dir);
  assert.equal(existing.loadHistory().decisions[0].detail, 'bad #Other');
  applyRepairManifest(dir, m, { expectRev: 7, by: 'external-repair' });
  assert.equal(hasPendingRepair(dir), false);
  assert.equal(existing.loadHistory().decisions[0].detail, 'bad @Other');
});

test('pair commit syncs both renames before removing the journal commit marker', () => {
  const { dir } = fixture();
  const before = bytes(dir);
  const liveBackup = backupRequired(join(dir, 'tower.json'));
  const historyBackup = backupRequired(join(dir, 'history.json'));
  const operations = [];
  commitRepairPair({
    dataDir: dir,
    liveFile: join(dir, 'tower.json'),
    historyFile: join(dir, 'history.json'),
    live: { changed: 'live' },
    history: { changed: 'history' },
    originalLive: before.live,
    originalHistory: before.history,
    liveBackup,
    historyBackup,
    manifestHash: 'd'.repeat(64),
    rename: (from, to) => {
      operations.push(`rename:${to.endsWith('tower.json') ? 'live' : 'history'}`);
      renameSync(from, to);
    },
    syncParent: () => operations.push('sync:data-dir'),
    finishTransaction: dataDir => {
      operations.push('remove-and-sync:journal');
      finishRepairTransaction(dataDir);
    },
  });
  assert.deepEqual(operations, [
    'rename:live', 'rename:history', 'sync:data-dir', 'remove-and-sync:journal',
  ]);
});

test('repair rejects protected identity, key, timestamp, and ordering leaves', () => {
  const { dir, manifest: m } = fixture();
  for (const path of ['/id', '/created', '/workOrder', '/options/0/key']) {
    const guarded = structuredClone(m);
    guarded.payload.patches[0].path = path;
    guarded.sha256 = canonicalPayloadHash(guarded.payload);
    assert.throws(
      () => applyRepairManifest(dir, guarded, { expectRev: 7, by: 'repairer' }),
      (error) => error.code === 'E_MANIFEST');
  }
  for (const [collection, key, path] of [
    ['decisions', { id: 'D-1' }, '/cardId'],
    ['ideas', { id: 'I-1' }, '/updated'],
    ['questions', { id: 'Q-1' }, '/answeredAt'],
  ]) {
    const guarded = structuredClone(m);
    Object.assign(guarded.payload.patches[0], { collection, key, path });
    guarded.sha256 = canonicalPayloadHash(guarded.payload);
    assert.throws(
      () => applyRepairManifest(dir, guarded, { expectRev: 7, by: 'repairer' }),
      (error) => error.code === 'E_MANIFEST');
  }
  const event = structuredClone(m);
  event.payload.patches[0] = {
    store: 'tower.json',
    collection: 'events',
    key: { at: 'x', by: 'x', action: 'x', ref: 'x', occurrence: 0 },
    path: '/at',
    current: 'x',
    replacement: 'y',
    substitutions: 1,
  };
  event.sha256 = canonicalPayloadHash(event.payload);
  assert.throws(
    () => applyRepairManifest(dir, event, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST');
});

test('repair requires declared canonicalization and a numeric expect-rev', () => {
  const { dir, manifest: m } = fixture();
  assert.throws(
    () => applyRepairManifest(dir, { ...m, canonicalization: 'other' }, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST');
  assert.throws(
    () => applyRepairManifest(dir, m, { expectRev: true, by: 'repairer' }),
    (error) => error.code === 'E_MANIFEST');
});

test('mandatory backup failure is reported and leaves both stores unchanged', () => {
  const { dir, manifest: m } = fixture();
  writeFileSync(join(dir, 'backups'), 'not a directory');
  const before = bytes(dir);
  assert.throws(
    () => applyRepairManifest(dir, m, { expectRev: 7, by: 'repairer' }),
    (error) => error.code === 'E_REPAIR_IO');
  assert.deepEqual(bytes(dir), before);
});
