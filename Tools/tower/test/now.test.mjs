import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import * as db from '../app/store.mjs';
import { DEFAULTS } from '../app/config.mjs';
import { serve } from '../app/server.mjs';
import { renderNowReports, relativeTime } from '../app/ui/now.js';

const towerRoot = dirname(dirname(fileURLToPath(import.meta.url)));
// Exercise this checkout's CLI; tower.mjs intentionally redirects worktrees to main.
const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
const sample = JSON.parse(readFileSync(join(towerRoot, 'examples/status.sample.json'), 'utf8'));
const snapshot = () => ({ milestones: [{ title: 'Full Rust port', state: 'Building',
  progress: { 'Source ported': 75, Verified: 25, Gated: 0 } }],
  workstreams: [{ title: 'Compiler', state: 'Active', workers: ['Pip'], blockers: ['Waiting for integration'] }] });
const invalid = fn => assert.throws(fn, error => error.code === 'E_INVALID');

function board() {
  const s = db.empty('Now');
  const card = db.addCard(s, { title: 'Report work', by: 'Pip' }, DEFAULTS);
  // Projection fixtures: open, ratified, draft and owner-acceptance decisions;
  // D-WAIT is a visual check still waiting for its screen capture.
  s.decisions.push(
    { id: 'D-OPEN', cardId: card.id, title: 'Pick behavior', status: 'open' },
    { id: 'D-DONE', cardId: card.id, title: 'Already voted', status: 'ratified' },
    { id: 'D-DRAFT', cardId: card.id, title: 'Not ready', status: 'open', draft: true },
    { id: 'D-CHECK', cardId: card.id, title: 'Check presentation', status: 'open', group: 'acceptance',
      visualMedia: [{ kind: 'image', path: 'docs/proposals/visual-acceptance/media/now/screen.png', alt: 'Now page briefing', caption: 'after' }] },
    { id: 'D-WAIT', cardId: card.id, title: 'Capture missing', status: 'open', group: 'acceptance' },
  );
  return { s: db.normalize(s), card };
}

test('reports are attributed, retained/projected, link canonical cards and do not change gates', () => {
  const { s, card } = board();
  const before = JSON.stringify({ cards: s.cards, milestones: s.milestones, decisions: s.decisions });
  const first = db.postBriefing(s, { title: 'Yesterday', body: '# Report\n\n**Shipped** work.', by: 'Pip' });
  const latest = db.postBriefing(s, { title: 'Morning', body: 'Integration pending.', by: 'Pip',
    sections: [{ title: 'Blockers', body: '- Await vote', links: [{ card: '#1' }, { decision: 'D-DONE' }] }] });
  assert.equal(latest.sections[0].links[0].card, card.id);
  assert.equal(latest.sections[0].links[1].cardId, card.id, 'ratified/retired ballots remain navigable through their card');
  const p = snapshot();
  p.milestones[0].links = [{ decision: 'D-OPEN', label: 'Vote' }];
  const status = db.postStatus(s, { snapshot: p, by: 'Pip', created: 'fake' });
  assert.deepEqual(s.briefings.map(r => r.id), [latest.id, first.id]);
  assert.ok(Number.isFinite(Date.parse(status.created)));
  assert.equal(status.by, 'Pip');
  assert.equal(p.milestones[0].links[0].cardId, undefined, 'input is not mutated');
  const projected = db.projectBoard(db.normalize(s));
  assert.equal(projected.statusSnapshot.id, status.id);
  assert.equal(projected.briefings.length, 2);
  assert.equal(Object.hasOwn(projected.statusSnapshot, 'ownerActions'), false);
  assert.equal(JSON.stringify({ cards: s.cards, milestones: s.milestones, decisions: s.decisions }), before);
  assert.equal(s.events[0].action, 'status.post');
  db.postStatus(s, { snapshot: sample, by: 'Pip' });
  assert.equal(s.statusSnapshot.milestones.length, 4);
});

test('invalid report structures and progress fail without modifying reports', () => {
  const s = db.empty();
  for (const patch of [{ by: '' }, { title: null }, { body: ' ' }, { sections: {} },
    { sections: [{ title: 'Blockers', body: 'x', links: [{ card: '#1', decision: 'D' }] }] }]) {
    invalid(() => db.postBriefing(s, { title: 'Report', body: 'Body', by: 'Pip', ...patch }));
  }
  for (const percent of [-1, 101, '50', NaN, Infinity]) {
    const p = snapshot(); p.milestones[0].progress.Verified = percent;
    invalid(() => db.postStatus(s, { snapshot: p, by: 'Pip' }));
  }
  for (const p of [null, [], { ...snapshot(), workstreams: null }, { ...snapshot(), updatedAt: 'today' },
    { ...snapshot(), workstreams: [{ ...snapshot().workstreams[0], links: [{ card: null, decision: 'D' }] }] }]) {
    invalid(() => db.postStatus(s, { snapshot: p, by: 'Pip' }));
  }
  const p = snapshot(); p.workstreams[0].links = [{ card: '#99' }];
  assert.throws(() => db.postStatus(s, { snapshot: p, by: 'Pip' }), error => error.code === 'E_NOT_FOUND');
  assert.deepEqual(s.briefings, []);
  assert.equal(s.statusSnapshot, null);
  const old = db.empty(); delete old.briefings; delete old.statusSnapshot;
  assert.deepEqual(db.normalize(old).briefings, []);
  const corrupt = db.empty(); corrupt.statusSnapshot = { ...sample, id: 's', by: 'Pip', created: 'not a timestamp' };
  invalid(() => db.normalize(corrupt));
});

test('Now renders only briefing and status reports, with safe markdown, links, metrics and workers', () => {
  const { s } = board();
  db.postBriefing(s, { title: 'Old', body: 'History body', by: 'Pip' });
  db.postBriefing(s, { title: '<script>Morning</script>', body: '# Progress\n\n**Verified**\n\n<script>alert(1)</script>', by: 'Pip',
    sections: [{ title: 'Next', body: '- Integrate', links: [{ card: '#1' }] }] });
  const p = snapshot();
  p.milestones[0].links = [{ decision: 'D-OPEN' }];
  db.postStatus(s, { snapshot: p, by: 'Pip' });
  const html = renderNowReports(db.projectBoard(s));
  assert.ok(html.indexOf('Latest briefing') < html.indexOf('Status board'));
  assert.equal((html.match(/<details class="report /g) || []).length, 2);
  assert.doesNotMatch(html, /Owner action needed|data-report="actions"|data-report-action-done|Vote:|Visual check:/);
  assert.match(html, /&lt;script&gt;Morning/);
  assert.match(html, /<h1 class="md__h md__h1">Progress<\/h1>/);
  assert.match(html, /<strong>Verified<\/strong>/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /<details class="report__history" data-report="briefing-history"><summary>Briefing history · 1/);
  assert.match(html, /<details class="report__past" data-report="briefing:[^"]+"><summary>Old/);
  assert.match(html, /<progress max="100" value="75"/);
  assert.match(html, /Active workers:<\/b> Pip/);
  assert.match(html, /Waiting for integration/);
  assert.match(html, /data-report-decision="D-OPEN"/);
  assert.match(html, /data-report-card="c/);
  assert.doesNotMatch(html, /D-DRAFT|D-DONE/);
  const empty = renderNowReports({ decisions: s.decisions });
  assert.match(empty, /No briefing posted yet/);
  assert.match(empty, /No status snapshot posted yet/);
  assert.doesNotMatch(empty, /Owner action needed|data-report="actions"|Vote:|Visual check:/);
});

test('Now reports fold to a one-line summary with title and update time, honoring saved choices', () => {
  const { s } = board();
  db.postBriefing(s, { title: 'Morning', body: 'Shipped.', by: 'Pip' });
  db.postStatus(s, { snapshot: { ...snapshot(), summary: 'Port is 80% done' }, by: 'Pip' });
  const projected = db.projectBoard(s);
  const open = renderNowReports(projected);
  for (const key of ['briefing', 'status']) assert.match(open, new RegExp(`<details class="report [^"]*" data-report="${key}" open>`));
  const folded = renderNowReports(projected, { isOpen: (key) => key !== 'status' });
  assert.match(folded, /data-report="status">\s*<summary class="report__summary">/, 'status folds when the owner closed it');
  const summary = /data-report="status">\s*<summary class="report__summary">([\s\S]*?)<\/summary>/.exec(folded)[1];
  assert.match(summary, /Status board/);
  assert.match(summary, /Port is 80% done/);
  assert.match(summary, /Updated <time datetime="[^"]+"[^>]*data-rel>just now<\/time>/);
  const briefSummary = /data-report="briefing" open>\s*<summary class="report__summary">([\s\S]*?)<\/summary>/.exec(folded)[1];
  assert.match(briefSummary, /Morning/);
  assert.match(briefSummary, /<time datetime=/);
});

test('relative report times', () => {
  const now = Date.parse('2026-10-04T12:00:00Z');
  assert.equal(relativeTime('2026-10-04T11:59:40Z', now), 'just now');
  assert.equal(relativeTime('2026-10-04T11:48:00Z', now), '12 min ago');
  assert.equal(relativeTime('2026-10-04T07:00:00Z', now), '5 h ago');
  assert.match(relativeTime('2026-09-20T07:00:00Z', now), /2026/);
  assert.equal(relativeTime('not a date', now), 'not a date');
});

test('CLI posts persist with history/list/show, rev guards, help and live HTTP/SSE projection', { timeout: 15000 }, async t => {
  const scratch = process.env.JET_TEST_SCRATCH || join(tmpdir(), 'jet-tower-test');
  mkdirSync(scratch, { recursive: true });
  const cwd = mkdtempSync(join(scratch, 'tower-now-'));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const cli = (args, ok = true) => {
    const r = spawnSync(process.execPath, ['--input-type=module', '--eval',
      `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`,
      '--', ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
    assert.equal(r.status === 0, ok, `${args.join(' ')}: ${r.stderr}\n${r.stdout}`);
    return r.stdout;
  };
  cli(['init', '--name', 'Now CLI']);
  cli(['help', '--check']);
  cli(['status', '--json']); // original summary remains available
  writeFileSync(join(cwd, 'report.md'), '# Morning\n\nShipped work.');
  writeFileSync(join(cwd, 'sections.json'), JSON.stringify([{ title: 'Next', body: 'Integration' }]));
  const posted = JSON.parse(cli(['briefing', 'post', '--file', 'report.md', '--title', 'Morning', '--sections', 'sections.json', '--by', 'Pip', '--json']));
  assert.equal(JSON.parse(cli(['briefing', 'show', posted.id, '--json'])).body, '# Morning\n\nShipped work.');
  assert.equal(JSON.parse(cli(['briefing', 'show', '--json'])).sections[0].title, 'Next');
  assert.equal(JSON.parse(cli(['briefing', 'list', '--json'])).length, 1);
  assert.match(cli(['briefing', 'list']), /\[Pip\] Morning/);
  cli(['briefing', 'show', 'missing'], false);
  cli(['briefing', 'post', '--file', 'report.md', '--title', 'Missing author'], false);
  writeFileSync(join(cwd, 'status.json'), JSON.stringify(sample));
  cli(['status', 'post', '--file', 'status.json', '--by', 'Pip', '--expect-rev', '0', '--json'], false);
  const status = JSON.parse(cli(['status', 'post', '--file', 'status.json', '--by', 'Pip', '--json']));
  writeFileSync(join(cwd, 'removed-field.json'), JSON.stringify({ ...sample, ownerActions: [] }));
  const rejectedCli = JSON.parse(cli(['status', 'post', '--file', 'removed-field.json', '--by', 'Pip', '--json'], false));
  assert.equal(rejectedCli.error, 'E_INVALID');
  assert.match(rejectedCli.message, /status\.ownerActions.*removed/);
  assert.equal(JSON.parse(cli(['status', 'show', '--json'])).id, status.id);
  cli(['status', 'show', '--days', '7'], false);
  const server = serve(db.openStore(join(cwd, '.tower')), 0, false);
  await once(server, 'listening');
  t.after(() => server.close());
  const url = `http://localhost:${server.address().port}`;
  const state = await (await fetch(`${url}/api/state`)).json();
  assert.equal(state.statusSnapshot.id, status.id);
  assert.equal(state.briefings[0].id, posted.id);
  const ui = await (await fetch(`${url}/now.js`)).text();
  assert.match(ui, /renderNowReports/);
  const controller = new AbortController();
  const stream = await fetch(`${url}/api/stream`, { signal: controller.signal });
  const reader = stream.body.getReader();
  t.after(() => controller.abort());
  let events = new TextDecoder().decode((await reader.read()).value);
  assert.match(events, /Morning/);
  cli(['briefing', 'post', '--file', 'report.md', '--title', 'Fresh CLI update', '--by', 'Pip']);
  // A state read must not consume the revision before the SSE clients see it.
  await fetch(`${url}/api/state`);
  while (!events.includes('Fresh CLI update')) {
    const chunk = await reader.read();
    assert.equal(chunk.done, false);
    events += new TextDecoder().decode(chunk.value);
  }
  controller.abort();
  const invalid = await fetch(`${url}/api/status/post`, { method: 'POST',
    headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify({ by: 'Pip', snapshot: {} }) });
  assert.equal(invalid.status, 400);
  assert.equal((await invalid.json()).error, 'E_INVALID');
  const http = await fetch(`${url}/api/briefing/post`, { method: 'POST',
    headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify({ title: 'HTTP', body: 'Report', by: 'Pip' }) });
  assert.equal(http.status, 200);
  assert.equal((await http.json()).state.briefings[0].title, 'HTTP');
  const httpStatus = await fetch(`${url}/api/status/post`, { method: 'POST',
    headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify({ by: 'Pip', snapshot: snapshot() }) });
  assert.equal(httpStatus.status, 200);
  const statusReply = await httpStatus.json();
  assert.equal(statusReply.state.statusSnapshot.workstreams[0].title, 'Compiler');
  assert.equal(Object.hasOwn(statusReply.result, 'ownerActions'), false);
  const removedField = await fetch(`${url}/api/status/post`, { method: 'POST',
    headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' },
    body: JSON.stringify({ by: 'Pip', snapshot: { ...snapshot(), ownerActions: [] } }) });
  assert.equal(removedField.status, 400);
  const rejected = await removedField.json();
  assert.equal(rejected.error, 'E_INVALID');
  assert.match(rejected.message, /status\.ownerActions.*removed/);
  const removedRoute = await fetch(`${url}/api/status/action-done`, { method: 'POST',
    headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' },
    body: JSON.stringify({ snapshotId: statusReply.result.id, id: 'missing', by: 'owner' }) });
  assert.equal(removedRoute.status, 404);
  assert.equal((await removedRoute.json()).error, 'E_USAGE');
  const otherDevice = await (await fetch(`${url}/api/state`)).json();
  assert.deepEqual(otherDevice.statusSnapshot, statusReply.state.statusSnapshot);
  const disk = db.openStore(join(cwd, '.tower')).load();
  assert.deepEqual(disk.statusSnapshot, statusReply.result);
  assert.equal(disk.events[0].action, 'status.post');
});
