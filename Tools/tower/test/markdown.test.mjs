import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderMarkdown, splitBlocks } from '../app/ui/markdown.js';

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
