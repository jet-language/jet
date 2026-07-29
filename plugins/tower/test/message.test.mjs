import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  addCard, addMessage, buildBrief, deleteCard, doneMessage, empty, normalize,
  openStore, project, setCompletionCursor, TowerError,
} from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';

function fresh(config = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'tower-message-'));
  writeJSON(join(dir, 'tower.json'), empty('Messages'));
  writeJSON(configFile(dir), { project: 'Messages', ...config });
  return { dir, store: openStore(dir) };
}

test('message notes survive a store restart and stay out of question counts', () => {
  const { dir, store } = fresh();
  store.mutate((s, cfg) => addCard(s, { title: 'Ship it', by: 'agent' }, cfg));
  const { result } = store.mutate((s) => addMessage(s, {
    cardId: '#1', text: 'Read the migration note.', by: 'agent',
  }));

  assert.equal(result.kind, 'message');
  assert.equal(result.status, 'open');
  const restarted = openStore(dir);
  assert.equal(restarted.load().questions.find(note => note.id === result.id)?.text, 'Read the migration note.');
  assert.equal(project(restarted.load()).counts.openQuestions, 0);
  assert.equal(project(restarted.load()).cards[0].openQ, 0);
  assert.deepEqual(buildBrief(restarted.load(), '#1').questions, []);
});

test('clearing completed cards does not clear open messages', () => {
  const { store } = fresh();
  store.mutate((s, cfg) => addCard(s, { title: 'Ship it', by: 'agent' }, cfg));
  store.mutate((s) => addMessage(s, { cardId: '#1', text: 'One owner note.', by: 'agent' }));
  store.mutate((s, cfg) => {
    const card = s.cards[0];
    card.phase = 'done';
    card.completedAt = '2026-07-25T11:00:00.000Z';
    card.updated = '2026-07-25T11:00:00.000Z';
  });
  store.mutate((s) => setCompletionCursor(s, '2026-07-25T12:00:00.000Z'));

  const state = store.load();
  assert.equal(state.meta.completionCursor, '2026-07-25T12:00:00.000Z');
  assert.equal(state.questions[0].status, 'open');
});

test('legacy digest cursors migrate to the completion cursor', () => {
  const state = empty('Legacy');
  state.meta.digestCursor = '2026-07-25T12:00:00.000Z';
  const migrated = normalize(state);
  assert.equal(migrated.meta.completionCursor, '2026-07-25T12:00:00.000Z');
  assert.equal('digestCursor' in migrated.meta, false);
});

test('only the owner can mark a message done', () => {
  const { store } = fresh();
  store.mutate((s, cfg) => addCard(s, { title: 'Ship it', by: 'agent' }, cfg));
  const { result: message } = store.mutate((s) => addMessage(s, {
    cardId: '#1', text: 'One owner note.', by: 'agent',
  }));

  assert.throws(
    () => store.mutate((s) => doneMessage(s, message.id, 'agent')),
    (error) => error instanceof TowerError && error.code === 'E_OWNER_ONLY',
  );
  store.mutate((s) => doneMessage(s, message.id, 'owner'));
  assert.equal(store.load().questions[0].status, 'done');
  assert.equal(store.load().questions[0].doneBy, 'owner');
});

test('an open message keeps an aged done card live until the owner marks it done', () => {
  const { store } = fresh({ retireAfterDays: 0 });
  store.mutate((s, cfg) => addCard(s, { title: 'Ship it', by: 'agent' }, cfg));
  const { result: message } = store.mutate((s) => addMessage(s, {
    cardId: '#1', text: 'One owner note.', by: 'agent',
  }));
  store.mutate((s) => {
    s.cards[0].phase = 'done';
    s.cards[0].updated = '2020-01-01';
    s.cards[0].completedAt = '2020-01-01T12:00:00.000Z';
  });

  assert.equal(store.load().cards.length, 1);
  assert.equal(store.load().questions.length, 1);

  store.mutate((s) => doneMessage(s, message.id, 'owner'));
  assert.equal(store.load().cards.length, 0);
  assert.equal(store.load().questions.length, 0);
  assert.equal(store.loadHistory().cards[0].questions[0].status, 'done');
});

test('an open message prevents card deletion', () => {
  const { store } = fresh();
  store.mutate((s, cfg) => addCard(s, { title: 'Ship it', by: 'agent' }, cfg));
  store.mutate((s) => addMessage(s, { cardId: '#1', text: 'One owner note.', by: 'agent' }));
  assert.throws(
    () => store.mutate((s) => deleteCard(s, '#1', { by: 'owner' })),
    (error) => error instanceof TowerError && /open message/.test(error.message),
  );
});
