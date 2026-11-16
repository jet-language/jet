// Ballots link the reports behind them (audits, research, measurements), so
// the owner can read the evidence from the ballot itself.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, normalizeReports } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';
import { renderReports, renderBallotIntro } from '../app/ui/ballot.js';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-reports-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const AUDIT = { title: 'Port bars, first run', path: 'Docs/audits/2026-11-10-port-bars-first-run.md', note: 'Every tool twin measured.' };
const SOURCE = { title: 'RFC 9106', url: 'https://www.rfc-editor.org/rfc/rfc9106' };

test('normalizeReports keeps a Tower doc or an https source, each with a title', () => {
  assert.deepEqual(normalizeReports([AUDIT, { ...SOURCE, note: '  ' }]), [AUDIT, SOURCE]);
  assert.deepEqual(normalizeReports(undefined), []);
});

test('normalizeReports refuses anything that is not a titled Tower doc or https source', () => {
  const refused = [
    [{ path: AUDIT.path }, /needs a title/],
    [{ title: 'x' }, /exactly one of path/],
    [{ title: 'x', path: AUDIT.path, url: SOURCE.url }, /exactly one of path/],
    [{ title: 'x', path: '/home/user/notes.md' }, /not a Tower doc/],
    [{ title: 'x', path: 'Docs/audits/../../secret.md' }, /not a Tower doc/],
    [{ title: 'x', path: 'Compiler/Main.jet' }, /not a Tower doc/],
    [{ title: 'x', url: 'http://example.com/a' }, /https address/],
    [{ title: 'x', url: 'javascript:alert(1)' }, /https address/],
  ];
  for (const [report, message] of refused)
    assert.throws(() => normalizeReports([report]), message, JSON.stringify(report));
  assert.throws(() => normalizeReports([AUDIT, AUDIT]), /linked twice/);
  assert.throws(() => normalizeReports('Docs/audits/a.md'), /must be a list/);
});

test('a ballot stores, replaces and clears its reports through add and update', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Card' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-R1', title: 'Pick one', draft: true, ballotMode: 'short', reports: [AUDIT] }));
  assert.deepEqual(st.load().decisions.find(d => d.id === 'D-R1').reports, [AUDIT]);
  st.mutate((s) => db.updateDecision(s, 'D-R1', { reports: [AUDIT, SOURCE] }, 'Main'));
  assert.deepEqual(st.load().decisions.find(d => d.id === 'D-R1').reports, [AUDIT, SOURCE]);
  st.mutate((s) => db.updateDecision(s, 'D-R1', { reports: [] }, 'Main'));
  assert.equal('reports' in st.load().decisions.find(d => d.id === 'D-R1'), false);
  assert.throws(() => st.mutate((s) => db.updateDecision(s, 'D-R1', { reports: [{ title: 'x', url: 'ftp://a' }] }, 'Main')), /https address/);
});

test('the ballot intro shows its reports right under the situation, escaped', () => {
  const html = renderBallotIntro({ id: 'D-R1', situation: 'Pick one.', reports: [AUDIT, { title: 'A <b> source', url: SOURCE.url }] });
  assert.ok(html.indexOf('class="situation"') < html.indexOf('class="reports"'));
  assert.match(html, /<a class="reports__link" href="#docs" data-doc-path="Docs\/audits\/2026-11-10-port-bars-first-run.md">Port bars, first run<\/a>/);
  assert.match(html, /<div class="reports__note">Every tool twin measured.<\/div>/);
  assert.match(html, /href="https:\/\/www.rfc-editor.org\/rfc\/rfc9106" target="_blank" rel="noopener noreferrer">A &lt;b&gt; source<\/a>/);
  assert.equal(renderReports([]), '');
  assert.equal(renderReports(undefined), '');
  assert.doesNotMatch(renderBallotIntro({ id: 'D-R2', situation: 'x' }), /reports/);
});
