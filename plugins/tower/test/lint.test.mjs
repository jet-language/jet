// Card #457 — `tower lint` + `tower next --burndown`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';
import { lint, ruleDoneWithoutEvidence, ruleClaimedIdle, ruleMissingAttribution,
  ruleBallotGaps, ruleStaleDraft, ruleOrphanBlockers, ruleBallotDocGaps } from '../app/lint.mjs';

const TOWER = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-lint-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const ballot = (extra = {}) => ({
  ballotMode: 'full',
  reviewPasses: { base: 'The base pass completed the ballot.', boilOcean: 'The boil-the-ocean pass tested the broad solution space.', hybrid: 'The hybrid pass combined compatible strengths.', cooperative: 'The cooperative pass strengthened each option.', adversarial: 'The adversarial pass attacked the recommendation.' },
  gist: 'a plain sentence', lesson: 'Concept, mechanics, terms, stakes, and a tiny example.', story: 'Dana hits this while shipping X.', inWild: 'real code here', rec: 'A',
  options: [{ key: 'A', name: 'Option A', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'Option B', detail: 'B is brief.', code: 'b()' }],
  recommendation: { why: 'A wins here.', whyNot: [{ key: 'B', reason: 'B loses the needed behavior.' }], tradeoff: 'A adds one visible step.' },
  hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
  ...extra,
});

const OLD_DATE = '2000-01-01';
const OLD_ISO = '2000-01-01T00:00:00.000Z';

// ---- 1. done-without-evidence ------------------------------------------------

test('done-without-evidence: clean when the log mentions verification', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', logEntry: 'ran full suite, all green', by: 'agent-1' }, cfg));
  assert.deepEqual(ruleDoneWithoutEvidence(st.load()), []);
});

test('done-without-evidence: clean when criteria are all verified', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'ship it', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 1, { by: 'verifier' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  assert.deepEqual(ruleDoneWithoutEvidence(st.load()), []);
});

test('done-without-evidence: flags a done card with no evidence and no verified criteria', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  const findings = ruleDoneWithoutEvidence(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'done-without-evidence');
  assert.equal(findings[0].ref, '#1');
});

// ---- 2. claimed-idle ----------------------------------------------------------

test('claimed-idle: clean when recently updated', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  assert.deepEqual(ruleClaimedIdle(st.load()), []);
});

test('claimed-idle: flags a claimed building card untouched 3+ days', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', phase: 'building' }, cfg));
  st.mutate((s) => db.claimCard(s, '#1', 'agent-1'));
  st.mutate((s) => { s.cards[0].updated = OLD_DATE; });
  const findings = ruleClaimedIdle(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'claimed-idle');
  assert.equal(findings[0].ref, '#1');
});

// ---- 3. missing-attribution ----------------------------------------------------

test('missing-attribution: clean when every event carries by', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', by: 'agent-1' }, cfg));
  assert.deepEqual(ruleMissingAttribution(st.load()), []);
});

test('missing-attribution: flags an event with an explicitly empty by', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', by: '' }, cfg));
  const findings = ruleMissingAttribution(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'missing-attribution');
});

// ---- 4. ballot-gaps -------------------------------------------------------------

test('ballot-gaps: clean when an open decision has a complete ballot', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 't', ...ballot() }));
  assert.deepEqual(ruleBallotGaps(st.load()), []);
});

test('ballot-gaps: flags an open non-draft decision missing ballot fields', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'WIP', draft: true }));
  // simulate a gap slipping past the write-time E_BALLOT gate (e.g. hand
  // migration, or a draft flipped without going through --ready)
  st.mutate((s) => { s.decisions[0].draft = false; });
  const findings = ruleBallotGaps(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'ballot-gaps');
  assert.equal(findings[0].ref, 'D-1');
  assert.match(findings[0].msg, /gist/);
});

test('ballot-gaps: excludes drafts and acceptance ballots', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg)); // mints D-ACCEPT-1
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-WIP', title: 'WIP', draft: true }));
  const s = st.load();
  assert.ok(s.decisions.find(d => d.id === 'D-ACCEPT-1'));
  assert.deepEqual(ruleBallotGaps(s), []);
});

// ---- 5. stale-draft ---------------------------------------------------------------

test('stale-draft: clean for a fresh draft', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'WIP', draft: true }));
  assert.deepEqual(ruleStaleDraft(st.load()), []);
});

test('stale-draft: flags a draft older than 7 days', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'WIP', draft: true }));
  st.mutate((s) => { s.decisions[0].created = OLD_ISO; });
  const findings = ruleStaleDraft(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'stale-draft');
  assert.equal(findings[0].ref, 'D-1');
});

// ---- 6. orphan-blockers -------------------------------------------------------------

test('orphan-blockers: clean when blockedBy resolves to a live card', () => {
  const st = fresh();
  const { result: a } = st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'B' }, cfg));
  // blockedBy is stored by literal id, same as laneOf's own resolution.
  st.mutate((s, cfg) => db.updateCard(s, '#2', { blockedBy: [a.id] }, cfg));
  assert.deepEqual(ruleOrphanBlockers(st.load(), { cards: [], decisions: [] }), []);
});

test('orphan-blockers: flags a blockedBy ref resolving nowhere', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', blockedBy: ['ghost-id'] }, cfg));
  const findings = ruleOrphanBlockers(st.load(), { cards: [], decisions: [] });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'orphan-blockers');
  assert.equal(findings[0].ref, '#1');
});

test('orphan-blockers: a ref resolvable via history cards is not orphaned', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', blockedBy: ['c-archived'] }, cfg));
  const history = { cards: [{ id: 'c-archived', num: 99, title: 'gone' }], decisions: [] };
  assert.deepEqual(ruleOrphanBlockers(st.load(), history), []);
});

// ---- 7. --docs mode: ballot-doc-gaps -------------------------------------------------

test('ruleBallotDocGaps: flags a ratified decision id still listed in docs/ballots/*.md', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-XYZ1', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-XYZ1', 'A', null, 'owner'));

  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'ballots'), { recursive: true });
  writeFileSync(join(docsRoot, 'ballots', 'open.md'), '# Open ballots\n\n- D-XYZ1 still needs owner sign-off\n');

  const findings = ruleBallotDocGaps(st.load(), { decisions: [] }, { docsRoot });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'ratified-in-open-ballot-doc');
  assert.equal(findings[0].ref, 'D-XYZ1');

  // A doc that declares itself decided history is skipped entirely.
  writeFileSync(join(docsRoot, 'ballots', 'review.md'),
    '# Review\n\nStatus: ratified 2026-07-06.\n\n- D-XYZ1 chosen option B\n');
  const again = ruleBallotDocGaps(st.load(), { decisions: [] }, { docsRoot });
  assert.equal(again.length, 1, 'historical doc adds no findings');
});

test('ruleBallotDocGaps: clean when the doc only mentions an unratified id', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-OPEN1', title: 't', ...ballot() }));

  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'ballots'), { recursive: true });
  writeFileSync(join(docsRoot, 'ballots', 'open.md'), '# Open ballots\n\n- D-OPEN1 still pending\n');

  assert.deepEqual(ruleBallotDocGaps(st.load(), { decisions: [] }, { docsRoot }), []);
});

test('ruleBallotDocGaps: a ratified id reachable via history decisions is also flagged', () => {
  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'ballots'), { recursive: true });
  writeFileSync(join(docsRoot, 'ballots', 'open.md'), '# Open ballots\n\n- D-HIST1 pending\n');
  const s = empty('T');
  const history = { decisions: [{ id: 'D-HIST1', status: 'ratified' }] };
  const findings = ruleBallotDocGaps(s, history, { docsRoot });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].ref, 'D-HIST1');
});

test('ruleBallotDocGaps: no docs/ballots dir is clean, never throws', () => {
  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-nodocs-'));
  assert.deepEqual(ruleBallotDocGaps(empty('T'), { decisions: [] }, { docsRoot }), []);
});

// ---- 8. lint() aggregator ------------------------------------------------------------

test('lint(): combines core rules; --docs adds the doc-gap rule only when asked', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-XYZ2', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-XYZ2', 'A', null, 'owner'));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg)); // done, no evidence → finding

  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'ballots'), { recursive: true });
  writeFileSync(join(docsRoot, 'ballots', 'open.md'), '- D-XYZ2 still open?\n');

  const s = st.load();
  const history = st.loadHistory();
  const withoutDocs = lint(s, history, { docs: false, docsRoot });
  assert.ok(withoutDocs.some(f => f.rule === 'done-without-evidence'));
  assert.ok(!withoutDocs.some(f => f.rule === 'ratified-in-open-ballot-doc'));

  const withDocs = lint(s, history, { docs: true, docsRoot });
  assert.ok(withDocs.some(f => f.rule === 'ratified-in-open-ballot-doc'));
});

// ---- 9. burndown scope --------------------------------------------------------------

test('nextCards burndown scope: current-epoch epoch-track + sidequests, other epochs excluded', () => {
  const st = fresh();
  st.mutate((s) => db.addEpoch(s, { id: 'e3', name: 'E3' }));
  st.mutate((s) => db.addEpoch(s, { id: 'e4', name: 'E4' }));
  st.mutate((s) => db.setCurrentEpoch(s, 'e3'));
  st.mutate((s, cfg) => db.addCard(s, { title: 'In current epoch', track: 'epoch', epoch: 'e3', phase: 'building' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Other epoch', track: 'epoch', epoch: 'e4', phase: 'building' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Sidequest', track: 'sidequest', phase: 'building' }, cfg));

  const s = st.load();
  const titles = db.nextCards(s, { scope: 'burndown', limit: 10 }).map(c => c.title).sort();
  assert.deepEqual(titles, ['In current epoch', 'Sidequest']);

  const unscoped = db.nextCards(s, { limit: 10 }).map(c => c.title).sort();
  assert.deepEqual(unscoped, ['In current epoch', 'Other epoch', 'Sidequest'], 'no scope → all agent-lane cards');
});

// ---- 10. CLI wiring: tower lint + tower next --burndown ------------------------------

const run = (cwd, args, ok = true) => {
  try {
    return { out: execFileSync(process.execPath, [TOWER, ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } }), code: 0 };
  } catch (e) {
    if (ok) throw e;
    return { out: (e.stdout || '') + (e.stderr || ''), code: e.status };
  }
};

test('cli: tower lint is clean on a fresh board, exit 0', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-lint-cli-'));
  run(cwd, ['init', '--name', 'Lint Test']);
  const r = run(cwd, ['lint', '--json']);
  assert.deepEqual(JSON.parse(r.out), []);
  assert.equal(r.code, 0);
});

test('cli: tower lint exits 1 and reports a finding once one exists', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-lint-cli-'));
  run(cwd, ['init', '--name', 'Lint Test']);
  run(cwd, ['card', 'add', '--title', 'A', '--json']);
  run(cwd, ['card', 'update', '#1', '--phase', 'done', '--by', 'agent-1']);
  const r = run(cwd, ['lint'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /done-without-evidence\s+#1/);
});

test('cli: tower lint --docs finds a ratified id in a doc rooted at --docs-root', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-lint-cli-docs-'));
  run(cwd, ['init', '--name', 'Lint Docs Test']);
  run(cwd, ['card', 'add', '--title', 'A', '--json']);
  const bp = join(cwd, 'ballot.json');
  writeFileSync(bp, JSON.stringify({ cardId: '#1', id: 'D-CLIDOC1', title: 't', ...ballot() }));
  run(cwd, ['decision', 'add', '--file', bp, '--by', 'tester']);
  run(cwd, ['decision', 'ratify', 'D-CLIDOC1', '--outcome', 'A', '--by', 'owner']);

  mkdirSync(join(cwd, 'docs', 'ballots'), { recursive: true });
  writeFileSync(join(cwd, 'docs', 'ballots', 'open.md'), '- D-CLIDOC1 open?\n');

  const r = run(cwd, ['lint', '--docs', '--json'], false);
  const findings = JSON.parse(r.out);
  assert.ok(findings.some(f => f.rule === 'ratified-in-open-ballot-doc' && f.ref === 'D-CLIDOC1'));
});

test('cli: tower next --burndown scopes to current epoch + sidequests', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-next-burndown-'));
  run(cwd, ['init', '--name', 'Burndown Test']);
  run(cwd, ['epoch', 'add', 'e3', '--name', 'E3']);
  run(cwd, ['epoch', 'add', 'e4', '--name', 'E4']);
  run(cwd, ['epoch', 'current', 'e3']);
  run(cwd, ['card', 'add', '--title', 'In', '--track', 'epoch', '--epoch', 'e3', '--phase', 'building', '--json']);
  run(cwd, ['card', 'add', '--title', 'Out', '--track', 'epoch', '--epoch', 'e4', '--phase', 'building', '--json']);
  run(cwd, ['card', 'add', '--title', 'Side', '--track', 'sidequest', '--phase', 'building', '--json']);
  const r = JSON.parse(run(cwd, ['next', '--burndown', '--json']).out);
  assert.deepEqual(r.map(c => c.title).sort(), ['In', 'Side']);
});
