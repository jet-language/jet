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
  ruleBallotGaps, ruleStaleDraft, ruleOrphanBlockers, ruleSpecReferenceGaps,
  ruleCriteriaEvidenceConflicts, ruleDuplicateSuspects } from '../app/lint.mjs';

const TOWER = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-lint-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const ballot = (extra = {}) => ({
  ballotMode: 'full',
  reviewPasses: { base: 'The base pass completed the ballot.', boilOcean: 'The breadth review checked for missing choices.', hybrid: 'The hybrid pass combined compatible strengths.', cooperative: 'The cooperative pass strengthened every option.', adversarial: 'Author model family: family-a. Adversarial model family: family-b. The adversarial pass attacked the recommendation.' },
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
  st.mutate((s) => db.addCriterion(s, '#1', 'ship it', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built', by: 'agent-1' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', logEntry: 'ran full suite, all green', by: 'agent-1' }, cfg));
  assert.deepEqual(ruleDoneWithoutEvidence(st.load()), []);
});

test('done-without-evidence: clean when criteria are all met or verified', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'ship it', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 1, { by: 'verifier' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  assert.deepEqual(ruleDoneWithoutEvidence(st.load()), []);
});

test('done-without-evidence: flags an owner-bypassed card with no evidence', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
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
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'WIP', draft: true, ballotMode: 'full', reviewPasses: ballot().reviewPasses }));
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
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'built', by: 'agent-1' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'agent-1' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-WIP', title: 'WIP', draft: true, ballotMode: 'full', reviewPasses: ballot().reviewPasses }));
  const s = st.load();
  assert.ok(s.decisions.find(d => d.id === 'D-ACCEPT-1'));
  assert.deepEqual(ruleBallotGaps(s), []);
});

// ---- 5. stale-draft ---------------------------------------------------------------

test('stale-draft: clean for a fresh draft', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'WIP', draft: true, ballotMode: 'full', reviewPasses: ballot().reviewPasses }));
  assert.deepEqual(ruleStaleDraft(st.load()), []);
});

test('stale-draft: flags a draft older than 7 days', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'WIP', draft: true, ballotMode: 'full', reviewPasses: ballot().reviewPasses }));
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

// ---- 7. --docs mode: spec-reference-gaps -----------------------------------------

test('ruleSpecReferenceGaps: reports missing card and decision records in docs/spec', () => {
  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'spec', 'nested'), { recursive: true });
  writeFileSync(join(docsRoot, 'spec', 'law.md'),
    'Card #999 and #998 own this law. D-MISSING1 has the decision.\n');
  writeFileSync(join(docsRoot, 'spec', 'nested', 'example.md'),
    'D-MISSING1 is repeated here.\n');

  const findings = ruleSpecReferenceGaps(empty('T'), { cards: [], decisions: [] }, { docsRoot });
  assert.deepEqual(findings.map(f => [f.rule, f.ref]), [
    ['spec-card-ref-missing', '#998'],
    ['spec-card-ref-missing', '#999'],
    ['spec-decision-ref-missing', 'D-MISSING1'],
  ]);
  assert.match(findings[0].msg, /docs\/spec\/law\.md:1/);
});

test('ruleSpecReferenceGaps: accepts live/history records and ignores code literals', () => {
  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'spec'), { recursive: true });
  writeFileSync(join(docsRoot, 'spec', 'law.md'),
    'Card #1 and card #2 are recorded.\nD-LIVE1 is recorded. D-XXX is a placeholder.\n`[U8#4096]` is a type.\n');
  const s = { cards: [{ num: 1 }], decisions: [{ id: 'D-LIVE1' }] };
  const history = { cards: [{ num: 2 }], decisions: [] };
  assert.deepEqual(ruleSpecReferenceGaps(s, history, { docsRoot }), []);
});

test('ruleSpecReferenceGaps: no docs/spec dir is clean, never throws', () => {
  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-nodocs-'));
  assert.deepEqual(ruleSpecReferenceGaps(empty('T'), { cards: [], decisions: [] }, { docsRoot }), []);
});

// ---- 8. criteria integrity ---------------------------------------------------

test('criteria-evidence-conflict: flags met and verified rows with disputed evidence', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Criteria evidence' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'first', 'planner'));
  st.mutate((s) => db.addCriterion(s, '#1', 'second', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'NOT SATISFIED: missing case', by: 'builder' }));
  st.mutate((s) => db.meetCriterion(s, '#1', 2, { evidence: 'built', by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 2, { evidence: 'could not run the check', by: 'verifier' }));
  const findings = ruleCriteriaEvidenceConflicts(st.load());
  assert.equal(findings.length, 2);
  assert.ok(findings.every(f => f.rule === 'criteria-evidence-conflict' && f.ref === '#1'));
  assert.match(findings[0].msg + findings[1].msg, /criterion #1|criterion #2/);
});

test('criteria-evidence-conflict: ignores historical and unrelated negative narration', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Clean criteria evidence' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'first', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, {
    evidence: 'PROOF: the suite passes. Before #2085 it could not finish; #2085 fixed the guard and it now fits. The unrelated rerun remains blocked.',
    by: 'builder',
  }));
  assert.deepEqual(ruleCriteriaEvidenceConflicts(st.load()), []);
});

test('criteria-evidence-conflict: flags a present negative admission without proof', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Open proof' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'first', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'The check could not be run in this environment.', by: 'builder' }));
  const findings = ruleCriteriaEvidenceConflicts(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].ref, '#1');
});

// ---- 9. duplicate-suspect ---------------------------------------------------

test('duplicate-suspect: flags shared test references only among open cards', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, {
    title: 'Parser symptom A', body: 'fails in tests/parser.rs at some_test_name',
  }, cfg));
  st.mutate((s, cfg) => db.addCard(s, {
    title: 'Parser symptom B', body: 'fails in tests/parser.rs at some_test_name',
  }, cfg));
  st.mutate((s, cfg) => db.addCard(s, {
    title: 'Closed parser symptom', phase: 'done', body: 'fails in tests/parser.rs at some_test_name',
  }, cfg));
  const findings = ruleDuplicateSuspects(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].rule, 'duplicate-suspect');
  assert.equal(findings[0].ref, '#1,#2');
  assert.match(findings[0].msg, /tests\/parser\.rs/);
});

// ---- 9. lint() aggregator ------------------------------------------------------------

test('lint(): combines core rules; --docs adds the spec scan only when asked', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-XYZ2', title: 't', ...ballot() }));
  st.mutate((s) => db.ratify(s, 'D-XYZ2', 'A', null, 'owner'));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg)); // owner bypass, no evidence → finding

  const docsRoot = mkdtempSync(join(tmpdir(), 'tower-lint-docs-'));
  mkdirSync(join(docsRoot, 'spec'), { recursive: true });
  writeFileSync(join(docsRoot, 'spec', 'open.md'), 'Card #999 and D-MISSING2 still need a record.\n');

  const s = st.load();
  const history = st.loadHistory();
  const withoutDocs = lint(s, history, { docs: false, docsRoot });
  assert.ok(withoutDocs.some(f => f.rule === 'done-without-evidence'));
  assert.ok(!withoutDocs.some(f => f.rule === 'spec-card-ref-missing'));

  const withDocs = lint(s, history, { docs: true, docsRoot });
  assert.ok(withDocs.some(f => f.rule === 'spec-card-ref-missing' && f.ref === '#999'));
  assert.ok(withDocs.some(f => f.rule === 'spec-decision-ref-missing' && f.ref === 'D-MISSING2'));
});

// ---- 10. burndown scope --------------------------------------------------------------

test('nextCards burndown scope: current-epoch epoch-track + sidequests, other epochs excluded', () => {
  const st = fresh();
  st.mutate((s) => db.updateEpoch(s, 'e1', { status: 'planned' }));
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

// ---- 11. CLI wiring: tower lint + tower next --burndown ------------------------------

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
  // An agent cannot close a card with no criteria (E_CRITERIA), so the
  // done-without-evidence board state this rule reports is one only the owner
  // can create.
  run(cwd, ['card', 'update', '#1', '--phase', 'done', '--by', 'owner']);
  const r = run(cwd, ['lint'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /done-without-evidence\s+#1/);
});

test('cli: tower lint --json exits 1 for shared open test references', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-lint-duplicate-cli-'));
  run(cwd, ['init', '--name', 'Lint Duplicate Test']);
  run(cwd, ['card', 'add', '--title', 'A', '--body', 'fails in tests/shared.rs at shared_test_name']);
  run(cwd, ['card', 'add', '--title', 'B', '--body', 'fails in tests/shared.rs at shared_test_name', '--force']);
  const r = run(cwd, ['lint', '--json'], false);
  assert.equal(r.code, 1);
  const findings = JSON.parse(r.out);
  assert.ok(findings.some(f => f.rule === 'duplicate-suspect' && f.ref === '#1,#2'));
});

test('cli: tower lint --docs finds missing references in docs/spec rooted at --docs-root', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-lint-cli-docs-'));
  run(cwd, ['init', '--name', 'Lint Docs Test']);
  mkdirSync(join(cwd, 'docs', 'spec'), { recursive: true });
  writeFileSync(join(cwd, 'docs', 'spec', 'open.md'), 'Card #999 and D-CLIDOC1 have no record.\n');

  const r = run(cwd, ['lint', '--docs', '--json'], false);
  const findings = JSON.parse(r.out);
  assert.ok(findings.some(f => f.rule === 'spec-card-ref-missing' && f.ref === '#999'));
  assert.ok(findings.some(f => f.rule === 'spec-decision-ref-missing' && f.ref === 'D-CLIDOC1'));
});

test('cli: tower next --burndown scopes to current epoch + sidequests', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-next-burndown-'));
  run(cwd, ['init', '--name', 'Burndown Test']);
  run(cwd, ['epoch', 'update', 'e1', '--status', 'planned']);
  run(cwd, ['epoch', 'add', 'e3', '--name', 'E3']);
  run(cwd, ['epoch', 'add', 'e4', '--name', 'E4']);
  run(cwd, ['epoch', 'current', 'e3']);
  run(cwd, ['card', 'add', '--title', 'In', '--track', 'epoch', '--epoch', 'e3', '--phase', 'building', '--json']);
  run(cwd, ['card', 'add', '--title', 'Out', '--track', 'epoch', '--epoch', 'e4', '--phase', 'building', '--json']);
  run(cwd, ['card', 'add', '--title', 'Side', '--track', 'sidequest', '--phase', 'building', '--json']);
  const r = JSON.parse(run(cwd, ['next', '--burndown', '--json']).out);
  assert.deepEqual(r.map(c => c.title).sort(), ['In', 'Side']);
});
