import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildDigestTimeline } from '../app/ui/digest.js';
import { renderMarkdown, splitBlocks } from '../app/ui/markdown.js';

test('catch-up collapses repeated events to one current status per card', () => {
  const cursor = '2026-07-25T10:00:00.000Z';
  const cards = [
    {
      id: 'c1', num: 10, title: 'Ship docs tab', phase: 'done',
      log: [{ at: '2026-07-25', text: 'Verified — closed.', by: 'agent-x' }],
    },
    {
      id: 'c2', num: 11, title: 'Fix digest', phase: 'building',
      lane: { label: 'Continue building' },
      log: [{ at: '2026-07-25', text: 'rewrote timeline UI', by: 'sol' }],
    },
  ];
  const decisions = [{
    id: 'D-1', cardId: 'c2', title: 'Pick renderer', status: 'ratified', outcome: '75%',
  }];
  const events = [
    { at: '2026-07-25T12:00:00.000Z', by: 'sol', action: 'card.update', ref: 'c2', note: 'phase,logEntry' },
    { at: '2026-07-25T11:00:00.000Z', by: 'agent-x', action: 'card.update', ref: 'c1', note: 'Verified — closed.' },
    { at: '2026-07-25T12:15:00.000Z', by: 'owner', action: 'decision.ratify', ref: 'D-1', note: '75%' },
    { at: '2026-07-25T12:30:00.000Z', by: 'sol', action: 'card.criteria-meet', ref: 'c2', note: '#1' },
    { at: '2026-07-25T09:00:00.000Z', by: 'sol', action: 'card.update', ref: 'c2', note: 'too old' },
    { at: '2026-07-25T13:00:00.000Z', by: 'owner', action: 'card.update', ref: 'c2', note: 'owner noise' },
  ];
  const { items } = buildDigestTimeline({ cursor, events, cards, decisions });
  assert.deepEqual(items, [
    { at: '2026-07-25T11:00:00.000Z', kind: 'done', text: '#10 Ship docs tab', ref: '#10' },
    { at: '2026-07-25T12:30:00.000Z', kind: 'changed', text: '#11 Fix digest — Continue building', ref: '#11' },
  ]);
});

test('digest empty when no cursor or no events', () => {
  assert.deepEqual(buildDigestTimeline({ cursor: null, events: [{ at: 'x' }] }).items, []);
  assert.equal(buildDigestTimeline({ cursor: '2026-07-25T10:00:00.000Z', events: [] }).items.length, 0);
});

test('precise card timestamps recover active and done changes outside the event window', () => {
  const cursor = '2026-07-25T18:00:00.000Z';
  const cards = [
    {
      id: 'c1', num: 10, title: 'Old done', phase: 'done',
      updated: '2026-07-25T12:00:00.000Z',
      updatedBy: 'agent',
      log: [{ at: '2026-07-25', text: 'Verified — closed.' }],
    },
    {
      id: 'c2', num: 11, title: 'Fresh done', phase: 'done',
      updated: '2026-07-25T19:00:00.000Z',
      updatedBy: 'agent',
      log: [{ at: '2026-07-25', text: 'Verified — closed.' }],
    },
    {
      id: 'c3', num: 12, title: 'Fresh active', phase: 'verify',
      updated: '2026-07-25T19:30:00.000Z',
      updatedBy: 'agent',
      lane: { label: 'Finish verification' },
    },
  ];
  const afterDismiss = buildDigestTimeline({ cursor, events: [], cards, decisions: [] });
  assert.equal(afterDismiss.items.length, 2);
  assert.equal(afterDismiss.items[0].text, '#11 Fresh done');
  assert.equal(afterDismiss.items[1].text, '#12 Fresh active — Finish verification');
  const cleared = buildDigestTimeline({
    cursor: '2026-07-25T20:00:00.000Z', events: [], cards, decisions: [],
  });
  assert.equal(cleared.items.length, 0);
});

test('owner-only updates stay out of catch-up, including timestamp fallback', () => {
  const cards = [{
    id: 'c1', num: 10, title: 'Owner edit', phase: 'ready',
    updated: '2026-07-25T12:00:00.000Z', updatedBy: 'owner',
    lane: { label: 'Ready to build' },
  }];
  const events = [{
    at: '2026-07-25T12:00:00.000Z', by: 'owner',
    action: 'card.update', ref: 'c1',
  }];
  assert.deepEqual(buildDigestTimeline({
    cursor: '2026-07-25T10:00:00.000Z', events, cards,
  }).items, []);
});

test('decision updates mark the linked card changed without using decision content', () => {
  const cards = [{
    id: 'c1', num: 10, title: 'Keep current status', phase: 'ready',
    lane: { label: 'Ready to build' }, updated: '2026-07-25',
  }];
  const decisions = [{
    id: 'D-1', cardId: 'c1', status: 'ratified', outcome: '92%',
  }];
  const events = [{
    at: '2026-07-25T12:00:00.000Z', by: 'planner',
    action: 'decision.update', ref: 'D-1', note: 'marked ready',
  }];
  const { items } = buildDigestTimeline({
    cursor: '2026-07-25T10:00:00.000Z', events, cards, decisions,
  });
  assert.deepEqual(items, [{
    at: '2026-07-25T12:00:00.000Z', kind: 'changed',
    text: '#10 Keep current status — Ready to build', ref: '#10',
  }]);
});

test('question answers mark the linked card changed', () => {
  const cards = [{
    id: 'c1', num: 10, title: 'Answer owner question', phase: 'building',
    lane: { label: 'Continue building' }, updated: '2026-07-25',
  }];
  const questions = [{ id: 'q1', cardId: 'c1', status: 'answered' }];
  const events = [{
    at: '2026-07-25T12:00:00.000Z', by: 'agent',
    action: 'question.answer', ref: 'q1',
  }];
  assert.equal(buildDigestTimeline({
    cursor: '2026-07-25T10:00:00.000Z', events, cards, questions,
  }).items[0].text, '#10 Answer owner question — Continue building');
});

test('restoring an archived decision marks its live card changed', () => {
  const cards = [{
    id: 'c1', num: 10, title: 'Restore decision', phase: 'ready',
    lane: { label: 'Ready to build' }, updated: '2026-07-25',
  }];
  const decisions = [{ id: 'D-1', cardId: 'c1', status: 'ratified' }];
  const events = [{
    at: '2026-07-25T12:00:00.000Z', by: 'agent',
    action: 'archive.restore', ref: 'D-1',
  }];
  assert.equal(buildDigestTimeline({
    cursor: '2026-07-25T10:00:00.000Z', events, cards, decisions,
  }).items[0].text, '#10 Restore decision — Ready to build');
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
