import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildDoneMessageQueue, renderDoneMessageQueue,
} from '../app/ui/done-messages.js';

test('done and messages queue includes only completed cards after the cursor', () => {
  const cursor = '2026-07-25T10:00:00.000Z';
  const cards = [
    { id: 'c1', num: 10, title: 'Ship docs tab', phase: 'done', completedAt: '2026-07-25T11:00:00.000Z' },
    { id: 'c2', num: 11, title: 'Still building', phase: 'building', updated: '2026-07-25T12:00:00.000Z' },
    { id: 'c3', num: 12, title: 'Already cleared', phase: 'done', completedAt: '2026-07-25T09:00:00.000Z' },
  ];
  assert.deepEqual(buildDoneMessageQueue({ cursor, cards }).done, [{
    at: '2026-07-25T11:00:00.000Z',
    cardId: 'c1',
    kind: 'done',
    marker: 'DONE',
    ref: '#10',
    text: '#10 Ship docs tab',
  }]);
});

test('existing done cards use their precise updated timestamp as a compatible fallback', () => {
  const cards = [{
    id: 'c1', num: 10, title: 'Legacy close', phase: 'done',
    updated: '2026-07-25T11:00:00.000Z',
  }];
  assert.equal(buildDoneMessageQueue({
    cursor: '2026-07-25T10:00:00.000Z', cards,
  }).done[0].text, '#10 Legacy close');
});

test('open card-linked messages ignore the completion cursor and done messages stay out', () => {
  const cards = [{ id: 'c1', num: 10, title: 'Ship docs tab', phase: 'done' }];
  const questions = [
    { id: 'q1', cardId: 'c1', kind: 'message', status: 'open', by: 'sol', text: 'Check the migration note.', created: '2026-07-25T08:00:00.000Z' },
    { id: 'q2', cardId: 'c1', kind: 'message', status: 'done', by: 'agent-x', text: 'Old message.', created: '2026-07-25T07:00:00.000Z' },
    { id: 'q3', cardId: 'c1', kind: 'question', status: 'open', by: 'owner', text: 'Why?', created: '2026-07-25T06:00:00.000Z' },
  ];
  assert.deepEqual(buildDoneMessageQueue({
    cursor: '2026-07-25T12:00:00.000Z', cards, questions,
  }).messages, [{
    at: '2026-07-25T08:00:00.000Z',
    by: 'sol',
    cardId: 'c1',
    id: 'q1',
    kind: 'message',
    marker: 'MESSAGE',
    ref: '#10',
    text: 'Check the migration note.',
    title: 'Ship docs tab',
  }]);
});

test('empty done and message queue renders nothing', () => {
  assert.equal(renderDoneMessageQueue({ done: [], messages: [] }), '');
});

test('done-only queue renders the flared heading and clear control', () => {
  const html = renderDoneMessageQueue({
    done: [{ cardId: 'c1', marker: 'DONE', text: '#10 Ship docs tab' }],
    messages: [],
  });
  assert.match(html, /Done &amp; messages/);
  assert.match(html, /queue__signal[^>]*aria-hidden="true">✦</);
  assert.match(html, />DONE</);
  assert.match(html, /data-clear-done>Clear done cards</);
  assert.doesNotMatch(html, />MESSAGE</);
  assert.doesNotMatch(html, /data-message-done/);
  assert.doesNotMatch(html, /2026-|queue__flare|★/);
});

test('message-only queue renders a per-message Done control without clear', () => {
  const html = renderDoneMessageQueue({
    done: [],
    messages: [{
      cardId: 'c1',
      id: 'q1',
      marker: 'MESSAGE',
      ref: '#10',
      text: 'Read <this> note.',
      title: 'Ship & docs',
    }],
  });
  assert.match(html, />MESSAGE</);
  assert.match(html, /data-message-done="q1">Done</);
  assert.doesNotMatch(html, /data-clear-done/);
  assert.match(html, /Read &lt;this&gt; note/);
  assert.match(html, /Ship &amp; docs/);
});

test('mixed queue keeps completed cards and messages independently actionable', () => {
  const html = renderDoneMessageQueue({
    done: [{ cardId: 'c1', marker: 'DONE', text: '#10 Ship docs tab' }],
    messages: [{
      cardId: 'c2', id: 'q2', marker: 'MESSAGE', ref: '#11',
      text: 'Read the note.', title: 'Migration',
    }],
  });
  assert.match(html, />DONE</);
  assert.match(html, />MESSAGE</);
  assert.match(html, /data-clear-done/);
  assert.match(html, /data-message-done="q2"/);
});
