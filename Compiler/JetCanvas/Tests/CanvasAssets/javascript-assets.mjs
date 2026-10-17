// DEV-A2: actual Jet canvas_js() bytes arrive on stdin. This harness proves
// independent JavaScript syntax and exact asset load-order assembly, NOT DOM,
// browser execution, host authentication, or checked graph projections.
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { Script } from 'node:vm';
import { fileURLToPath } from 'node:url';

// Independent oracle from Rust canvas.rs:6081–6094, not extracted from the
// Jet implementation. URLs are module-relative, never arbitrary cwd assets.
const order = [
  'runtime-state.js',
  'session-workbench.js',
  'editing-history.js',
  'diagnostics-query.js',
  'drawing-palette.js',
  'project-navigation.js',
  'graph-rendering.js',
  'inspector-connections.js',
  'input-events.js',
  'transactions-catalog.js',
  'review.js',
  'bootstrap.js',
];
const assetDirectory = new URL('../../Source/CanvasAssets/js/', import.meta.url);
const discovered = (await readdir(assetDirectory)).filter(name => name.endsWith('.js')).sort();
assert.deepEqual(discovered, [...order].sort(), 'retained Canvas asset set drifted');

const parts = [Buffer.from('(function () {\n')];
for (const name of order) {
  const path = new URL(name, assetDirectory);
  const source = await readFile(path);
  // Each file is checked independently by the real Node executable. Do not
  // substitute a brace counter, source-count assertion, or success fallback.
  const checked = spawnSync(process.execPath, ['--check', fileURLToPath(path)], { encoding: 'utf8' });
  if (checked.error) throw checked.error;
  assert.equal(checked.status, 0, `${name}: node --check failed\n${checked.stderr}`);
  parts.push(source);
}
parts.push(Buffer.from('})();\n'));
const expected = Buffer.concat(parts);
const input = [];
for await (const chunk of process.stdin) input.push(chunk);
const actual = Buffer.concat(input);

// Unlike substring searches this rejects changed asset bytes, wrong order,
// extra separators, trailing statements, and wrapper drift. The value under
// test was emitted by public canvas_js(), not reconstructed from Jet source.
assert(actual.equals(expected), 'canvas_js() is not the exact ordered IIFE assembly');
new Script(actual.toString('utf8'), { filename: 'canvas-app.js' });

console.log('Canvas JS: independent node --check assets and exact ordered, syntax-valid IIFE OK');
