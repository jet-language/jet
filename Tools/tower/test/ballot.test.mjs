import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderBallotIntro, renderSituation, lossText } from '../app/ui/ballot.js';

const SITUATION = 'Tower is the shared project board.\nA ballot is where the owner picks one option.\n\nWe recommend option A <today>.';

test('situation renders as escaped prose paragraphs with soft line wraps', () => {
  const html = renderSituation(SITUATION);
  assert.match(html, /^<section class="situation" aria-label="The situation">/);
  assert.match(html, /<div class="situation__label">The situation<\/div>/);
  assert.deepEqual([...html.matchAll(/<p>(.*?)<\/p>/g)].map(m => m[1]), [
    'Tower is the shared project board. A ballot is where the owner picks one option.',
    'We recommend option A &lt;today&gt;.',
  ]);
  assert.doesNotMatch(html, /<ul|<li|<br/);
  assert.equal(renderSituation(''), '');
  assert.equal(renderSituation(undefined), '');
});

test('ballot intro is one quiet line, then the situation before anything else', () => {
  const html = renderBallotIntro({ id: 'D-X1', situation: SITUATION, rec: 'A', status: 'open' },
    { num: 7, title: 'Make <ballots> readable' }, 'open');
  const head = /<div class="fdeck__head">(.*?)<\/div>/s.exec(html)[1];
  assert.equal(head, '<span class="fdeck__for"><span class="fdeck__id">D-X1</span> · card #7 · Make &lt;ballots&gt; readable</span>'
    + '<span class="qchip qchip--open">✎ awaiting answer</span>');
  assert.doesNotMatch(html, /fdeck__rec|class="chip"/, 'status and rec chips no longer crowd the intro');
  assert.ok(html.indexOf('fdeck__head') < html.indexOf('class="situation"'));
  const bare = renderBallotIntro({ id: 'D-X2' });
  assert.match(bare, /<span class="fdeck__id">D-X2<\/span><\/span><\/div>/);
  assert.doesNotMatch(bare, /situation|qchip/);
});

test('recommendation losses show the concrete mitigation with a plain label', () => {
  assert.equal(lossText({ loss: 'A saved price may be old.', mitigation: 'Check the price before payment.' }),
    'A saved price may be old. — How we reduce it: Check the price before payment.');
  assert.equal(lossText('An alternative has a cost.'), 'An alternative has a cost.');
  assert.equal(lossText({ loss: 'A cost.' }), 'A cost.');
});
