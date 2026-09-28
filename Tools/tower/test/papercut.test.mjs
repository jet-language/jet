import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  addCard, addPapercut, empty, listPapercuts, normalize, openStore,
  project, resolvePapercut, TowerError,
} from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';

function fresh(config = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'tower-papercut-'));
  writeJSON(join(dir, 'tower.json'), empty('Papercuts'));
  writeJSON(configFile(dir), { project: 'Papercuts', ...config });
  return { dir, store: openStore(dir) };
}

test('a papercut is added, listed newest-first, and resolved by the owner', () => {
  const { store } = fresh();
  store.mutate((s) => addPapercut(s, { text: 'jet-env swallowed stderr', by: 'agent' }));
  const { result: second } = store.mutate((s) => addPapercut(s, { text: 'stale build cache', by: 'agent' }));

  const list = listPapercuts(store.load(), {});
  assert.equal(list.length, 2);
  assert.equal(list[0].id, second.id, 'newest first');
  assert.equal(list[0].status, 'open');

  store.mutate((s) => resolvePapercut(s, second.id, 'owner'));
  const resolved = store.load().papercuts.find(p => p.id === second.id);
  assert.equal(resolved.status, 'resolved');
  assert.equal(resolved.resolvedBy, 'owner');
  assert.ok(resolved.resolvedAt);
  assert.equal(listPapercuts(store.load(), { status: 'open' }).length, 1);
});

test('only the owner can resolve a papercut', () => {
  const { store } = fresh();
  const { result } = store.mutate((s) => addPapercut(s, { text: 'confusing error', by: 'agent' }));
  assert.throws(
    () => store.mutate((s) => resolvePapercut(s, result.id, 'agent')),
    (error) => error instanceof TowerError && error.code === 'E_OWNER_ONLY',
  );
});

test('a papercut needs an agent attribution and non-empty text', () => {
  const { store } = fresh();
  assert.throws(
    () => store.mutate((s) => addPapercut(s, { text: 'no author', by: undefined })),
    (error) => error instanceof TowerError && error.code === 'E_INVALID',
  );
  assert.throws(
    () => store.mutate((s) => addPapercut(s, { text: 'owner cannot log', by: 'owner' })),
    (error) => error instanceof TowerError && error.code === 'E_INVALID',
  );
  assert.throws(
    () => store.mutate((s) => addPapercut(s, { text: '   ', by: 'agent' })),
    (error) => error instanceof TowerError && error.code === 'E_INVALID',
  );
});

test('papercuts survive a store restart and ship in the projected state', () => {
  const { dir, store } = fresh();
  const { result } = store.mutate((s) => addPapercut(s, { text: 'misleading docs', by: 'agent' }));

  const restarted = openStore(dir);
  assert.equal(restarted.load().papercuts.find(p => p.id === result.id)?.text, 'misleading docs');
  assert.equal(project(restarted.load()).papercuts.length, 1);
});

test('normalize() defaults the papercuts array on legacy data', () => {
  const legacy = empty('Legacy');
  delete legacy.papercuts;
  const migrated = normalize(legacy);
  assert.deepEqual(migrated.papercuts, []);
});

test('logging a papercut is never blocked by a frozen owner lane', () => {
  const { store } = fresh();
  store.mutate((s, cfg) => addCard(s, { title: 'Frozen', phase: 'frozen', by: 'owner' }, cfg));
  const { result } = store.mutate((s) => addPapercut(s, {
    text: 'friction while the card was frozen', cardId: '#1', by: 'agent',
  }));
  assert.equal(result.status, 'open');
  assert.equal(result.cardNum, 1);
  assert.equal(store.load().papercuts.length, 1);
});

test('a card link is validated when provided', () => {
  const { store } = fresh();
  assert.throws(
    () => store.mutate((s) => addPapercut(s, { text: 'bad link', cardId: '#999', by: 'agent' })),
    (error) => error instanceof TowerError && error.code === 'E_NOT_FOUND',
  );
});
