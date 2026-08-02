import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const js = readFileSync(join(root, 'app/ui/tower.js'), 'utf8');
const css = readFileSync(join(root, 'app/ui/tower.css'), 'utf8');

test('Focus Mode renders every review pass in the required order', () => {
  const labels = [
    "['base', '●', 'Base']",
    "['boilOcean', '◎', 'Boil the ocean']",
    "['hybrid', '◇', 'Hybrid']",
    "['cooperative', '＋', 'Cooperative']",
    "['adversarial', '⚑', 'Adversarial']",
  ];
  for (let i = 1; i < labels.length; i++)
    assert.ok(js.indexOf(labels[i - 1]) < js.indexOf(labels[i]), `${labels[i - 1]} must precede ${labels[i]}`);
  assert.match(js, /reviewPassesBody\(d\).*Recommendation:/s);
});

test('Focus Mode preserves the ballot color and non-color meaning', () => {
  for (const rule of [
    /\.reviewpass--base \{ --pass-color: var\(--slate\); \}/,
    /\.reviewpass--boilOcean \{ --pass-color: var\(--frost\); \}/,
    /\.reviewpass--hybrid \{ --pass-color: var\(--cyan\); \}/,
    /\.reviewpass--cooperative \{ --pass-color: var\(--ok\); \}/,
    /\.reviewpass--adversarial \{ --pass-color: var\(--amber\); \}/,
    /\.recline \{[^}]*var\(--blue\)/,
    /\.recline__why-not \{[^}]*var\(--red\)/,
  ]) assert.match(css, rule);
  assert.match(js, /class="recline__why-not"/);
});

test('Now urgency counts ballots while completed cards use a blue count', () => {
  assert.match(js, /const openDecisions = \(\) => S\.decisions\.filter\(d => d\.status !== 'ratified' && !d\.draft\)/);
  assert.match(js, /const ballotCount = \(\) => openDecisions\(\)\.length/);
  assert.match(js, /id: 'now'.*count: \(\) => ballotCount\(\)/);
  assert.match(js, /function updatePill\(\) \{\s+const fy = ballotCount\(\)/);
  assert.match(js, /const ballots = ballotCount\(\)/);
  assert.doesNotMatch(js, /id: 'now'.*count: \(\) => duties\(\)\.length/);
  assert.match(css, /\.queue__done-count \{[^}]*color: var\(--blue\)/);
});
