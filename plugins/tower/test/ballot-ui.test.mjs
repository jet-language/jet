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
    "['beginner', '◌', 'Beginner']",
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
    /\.reviewpass--beginner \{ --pass-color: var\(--blue\); \}/,
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

test('Board search and Recent flatten like a milestone filter', () => {
  assert.match(js, /function searchFilterBar/);
  assert.match(js, /function recentFilterBar/);
  assert.match(js, /id="radar-recent"/);
  assert.match(js, /data-docs-sort="created"/);
  assert.match(js, /created \$\{esc\(dateDay\(c\.created\)\)\}/);
  assert.match(css, /\.card__dates \{/);
  assert.match(css, /\.docs__dates \{/);
});

test('Focus Mode renders a reading surface before the options', () => {
  const gist = js.indexOf('surface.gist');
  const trio = js.indexOf('const trio = surface.trio');
  const opts = js.indexOf('id="f-opts">${optionHtml}');
  assert.ok(gist >= 0 && gist < trio && trio < opts, 'surface gist, trio, and options must stay in order');
});

test('Focus Mode puts the recommended surface option first without mutating the record', () => {
  assert.match(js, /return \[\.\.\.options\]\.sort\(\(a, b\) => a\.key === rec \? -1 : b\.key === rec \? 1 : 0\)/);
});

test('Focus Mode keeps long ballot details behind one Full ballot fold', () => {
  assert.match(js, /const fullBallot = `<details class="fullballot">/);
  for (const id of ['f-facets', 'f-facetbody']) assert.match(js, new RegExp(`id="${id}"`));
  assert.match(js, /const fullBallot[\s\S]*reviewPassesBody\(d\)/);
});

test('Focus Mode wraps deck code and only enables a wide trio at 960px', () => {
  assert.match(css, /\.fdeck--surface \.code \{[^}]*font-size: 13\.5px;[^}]*line-height: 1\.6;[^}]*white-space: pre-wrap;[^}]*overflow-wrap: anywhere;[^}]*max-width: none;/s);
  assert.match(css, /@media \(min-width: 960px\) \{\s+\.trio\.trio--side \{/);
});

test('Focus Mode keeps the legacy deck path behind the surface guard', () => {
  assert.match(js, /const surfaceHtml = d\.surface \? surfaceDeck\(d, c, chosen\) : ''/);
  assert.match(js, /\$\{d\.surface \? surfaceHtml : `/);
});
