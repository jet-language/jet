import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildDoneMessageQueue } from '../app/ui/done-messages.js';
import { renderMarkdown, splitBlocks } from '../app/ui/markdown.js';

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

test('markdown renderer: headings, lists, code, bold', () => {
  const html = renderMarkdown('# Title\n\n- a\n- b\n\n```\ncode\n```\n\n**bold** and `x`');
  assert.match(html, /<h1/);
  assert.match(html, /<ul/);
  assert.match(html, /<pre/);
  assert.match(html, /<strong>bold<\/strong>/);
  assert.match(html, /<code class="md__code">x<\/code>/);
});

test('splitBlocks separates headings and paragraphs', () => {
  const blocks = splitBlocks('# A\n\npara one\n\n## B\n\n- x\n- y');
  assert.ok(blocks.length >= 3);
  assert.equal(blocks[0], '# A');
});
