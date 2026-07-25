import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildDigestTimeline, isFieldListNote } from '../app/ui/digest.js';
import { renderMarkdown, splitBlocks } from '../app/ui/markdown.js';

test('isFieldListNote detects raw card.update field lists', () => {
  assert.equal(isFieldListNote('phase,logEntry'), true);
  assert.equal(isFieldListNote('started: parser'), false);
});

test('digest timeline: done titles, change one-liners, no agent names, chronological', () => {
  const cursor = '2026-07-25T10:00:00.000Z';
  const cards = [
    {
      id: 'c1', num: 10, title: 'Ship docs tab', phase: 'done',
      log: [{ at: '2026-07-25', text: 'Verified — closed.', by: 'agent-x' }],
    },
    {
      id: 'c2', num: 11, title: 'Fix digest', phase: 'building',
      log: [{ at: '2026-07-25', text: 'rewrote timeline UI', by: 'sol' }],
    },
  ];
  const decisions = [{ id: 'D-1', title: 'Pick renderer', status: 'open' }];
  const events = [
    { at: '2026-07-25T12:00:00.000Z', by: 'sol', action: 'card.update', ref: 'c2', note: 'phase,logEntry' },
    { at: '2026-07-25T11:00:00.000Z', by: 'agent-x', action: 'card.update', ref: 'c1', note: 'Verified — closed.' },
    { at: '2026-07-25T10:30:00.000Z', by: 'planner', action: 'decision.add', ref: 'D-1', note: 'Pick renderer' },
    { at: '2026-07-25T09:00:00.000Z', by: 'sol', action: 'card.update', ref: 'c2', note: 'too old' },
    { at: '2026-07-25T12:30:00.000Z', by: 'owner', action: 'card.update', ref: 'c2', note: 'owner noise' },
  ];
  const { items } = buildDigestTimeline({ cursor, events, cards, decisions });
  assert.ok(items.length >= 3);
  assert.equal(items[0].kind, 'ballot');
  assert.match(items[0].text, /D-1/);
  assert.ok(!items.some(i => /sol|agent-x|planner/i.test(i.text)));
  const done = items.find(i => i.kind === 'done');
  assert.ok(done);
  assert.match(done.text, /#10 Ship docs tab/);
  const changed = items.find(i => i.kind === 'changed' && /#11/.test(i.text));
  assert.ok(changed);
  assert.match(changed.text, /rewrote timeline UI/);
  for (let i = 1; i < items.length; i++) assert.ok(items[i].at >= items[i - 1].at);
});

test('digest empty when no cursor or no events', () => {
  assert.deepEqual(buildDigestTimeline({ cursor: null, events: [{ at: 'x' }] }).items, []);
  assert.equal(buildDigestTimeline({ cursor: '2026-07-25T10:00:00.000Z', events: [] }).items.length, 0);
});

test('dismiss (cursor=now) clears date-only done-card fallback', () => {
  const cursor = '2026-07-25T18:00:00.000Z';
  const cards = [
    {
      id: 'c1', num: 10, title: 'Old done', phase: 'done',
      updated: '2026-07-25T12:00:00.000Z',
      log: [{ at: '2026-07-25', text: 'Verified — closed.' }],
    },
    {
      id: 'c2', num: 11, title: 'Fresh done', phase: 'done',
      updated: '2026-07-25T19:00:00.000Z',
      log: [{ at: '2026-07-25', text: 'Verified — closed.' }],
    },
  ];
  const afterDismiss = buildDigestTimeline({ cursor, events: [], cards, decisions: [] });
  assert.equal(afterDismiss.items.length, 1);
  assert.match(afterDismiss.items[0].text, /#11 Fresh done/);
  const cleared = buildDigestTimeline({
    cursor: '2026-07-25T20:00:00.000Z', events: [], cards, decisions: [],
  });
  assert.equal(cleared.items.length, 0);
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
