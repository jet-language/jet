// Card #462 — `tower brief`, the one-shot agent work packet.
// Goal an agent reading ONE brief needs zero other reads to start a card:
// card + live blockedBy state (card or decision refs, #458), full criteria
// checklist (#463), decisions copied VERBATIM off the live store (never
// paraphrased), open questions, refs (explicit + harvested from body/plan),
// recent log, standing rules footer.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { openStore, empty, buildBrief, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-brief-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

const ballot = (extra = {}) => ({
  gist: 'a plain sentence', lesson: 'Concept, mechanics, terms, stakes, and a tiny example.', story: 'Dana hits this while shipping X.', inWild: 'real code in Source/foo.rs',
  rec: 'A', options: [{ key: 'A', name: 'Option A', detail: 'does A', code: 'a()' }, { key: 'B', name: 'Option B', detail: 'does B', code: 'b()' }],
  recommendation: { why: 'A best serves this decision.', whyNot: [{ key: 'B', reason: 'B loses the needed guarantee.' }], tradeoff: 'A adds one visible step.' },
  hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
  ...extra,
});

// ---- 1. card fields, epoch/milestone -----------------------------------------

test('packet.card carries epoch name/goal and milestone title/goal/criteria when set', () => {
  const st = fresh();
  st.mutate((s) => db.addEpoch(s, { id: 'e1', name: 'Epoch One', goal: 'ship it' }));
  const m = st.mutate((s) => db.addMilestone(s, { epochId: 'e1', title: 'MVP', goal: 'usable v1', criteria: '9/9 features' })).result;
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', body: 'do the thing', plan: '1. x 2. y', epoch: 'e1', milestoneId: m.id, priority: 'P1' }, cfg));
  const s = st.load();
  const p = buildBrief(s, '#1');
  assert.equal(p.card.num, 1);
  assert.equal(p.card.title, 'A');
  assert.equal(p.card.body, 'do the thing');
  assert.equal(p.card.plan, '1. x 2. y');
  assert.equal(p.card.priority, 'P1');
  assert.deepEqual(p.card.epoch, { id: 'e1', name: 'Epoch One', goal: 'ship it' });
  assert.deepEqual(p.card.milestone, { id: m.id, title: 'MVP', goal: 'usable v1', criteria: '9/9 features' });
});

test('packet.card.epoch/milestone are null when unset', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const p = buildBrief(st.load(), '#1');
  assert.equal(p.card.epoch, null);
  assert.equal(p.card.milestone, null);
});

// ---- 2. blockedBy: live state, card AND decision refs (#458) -----------------

test('blockers resolve a card ref (done/not-done) and a decision ref (ratified/not) independently', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Blocker card' }, cfg));       // #1
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-BLOCK', title: 'pick', ...ballot() }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'B', blockedBy: ['D-BLOCK'] }, cfg));   // #2, blocked by an open decision
  let p = buildBrief(st.load(), '#2');
  assert.equal(p.blockers.length, 1);
  assert.deepEqual(p.blockers[0], { id: 'D-BLOCK', kind: 'decision', title: 'pick', status: 'open', done: false });

  st.mutate((s) => db.ratify(s, 'D-BLOCK', 'A', 'go with A', 'owner'));
  p = buildBrief(st.load(), '#2');
  assert.equal(p.blockers[0].done, true);
  assert.equal(p.blockers[0].status, 'ratified');

  const c1id = st.load().cards.find(c => c.num === 1).id;
  st.mutate((s, cfg) => db.updateCard(s, '#2', { blockedBy: [c1id] }, cfg));
  p = buildBrief(st.load(), '#2');
  assert.deepEqual(p.blockers[0], { id: c1id, kind: 'card', num: 1, title: 'Blocker card', phase: 'planning', done: false });
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'owner' }, cfg));
  p = buildBrief(st.load(), '#2');
  assert.equal(p.blockers[0].done, true);
});

test('a dangling blockedBy ref surfaces as kind unknown, done: false — never throws', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => { s.cards[0].blockedBy = ['ghost-ref']; });
  const p = buildBrief(st.load(), '#1');
  assert.deepEqual(p.blockers[0], { id: 'ghost-ref', kind: 'unknown', done: false });
});

// ---- 3. criteria + needsAcceptance --------------------------------------------

test('packet.criteria carries full checklist state plus the needsAcceptance flag', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'ran it', by: 'builder' }));
  const p = buildBrief(st.load(), '#1');
  assert.equal(p.criteria.needsAcceptance, true);
  assert.equal(p.criteria.items.length, 1);
  assert.deepEqual(p.criteria.items[0], { n: 1, text: 'thing works', status: 'met', metBy: 'builder', verifiedBy: null, evidence: 'ran it', at: p.criteria.items[0].at });
});

// ---- 4. decisions VERBATIM — the regression this card exists to prevent -----

test('a ratified decision surfaces the owner comment IN FULL, never truncated or paraphrased', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-1', title: 'Pick a memory model', ...ballot() }));
  const longComment = 'Go with A. Reasoning: B breaks second-class borrows (see docs/spec/philosophy.md), '
    + 'and the "quoted" edge-case with a — dash and unicode ✓ must survive verbatim.';
  st.mutate((s) => db.ratify(s, 'D-1', 'A', longComment, 'owner'));
  const p = buildBrief(st.load(), '#1');
  const d = p.decisions.find(x => x.id === 'D-1');
  assert.equal(d.status, 'ratified');
  assert.equal(d.outcome, 'A');
  assert.equal(d.comment, longComment, 'comment must be byte-for-byte identical — never paraphrased');
  // ratified decisions don't need to re-carry the full ballot narrative
  assert.equal(d.story, undefined);
  assert.equal(d.options, undefined);
});

test('an open decision carries its full options text verbatim (owner decides from the ballot alone)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-2', title: 'Pick a syntax', ...ballot({
    story: 'Priya wants a terse literal.',
    inWild: 'x := [1,2,3] in examples/features/lists.jet',
  }) }));
  const p = buildBrief(st.load(), '#1');
  const d = p.decisions.find(x => x.id === 'D-2');
  assert.equal(d.status, 'open');
  assert.equal(d.lesson, 'Concept, mechanics, terms, stakes, and a tiny example.');
  assert.equal(d.story, 'Priya wants a terse literal.');
  assert.equal(d.inWild, 'x := [1,2,3] in examples/features/lists.jet');
  assert.equal(d.rec, 'A');
  assert.equal(d.recommendation.whyNot[0].key, 'B');
  assert.equal(d.hybrid.result, 'A');
  assert.deepEqual(d.options, [
    { key: 'A', name: 'Option A', detail: 'does A', code: 'a()' },
    { key: 'B', name: 'Option B', detail: 'does B', code: 'b()' },
  ]);
});

test('a draft decision is marked draft and still carries its options', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addDecision(s, { cardId: '#1', id: 'D-3', title: 'WIP ballot', draft: true, gist: 'unfinished' }));
  const p = buildBrief(st.load(), '#1');
  const d = p.decisions.find(x => x.id === 'D-3');
  assert.equal(d.draft, true);
  assert.equal(d.status, 'open');
});

// ---- 5. open questions ---------------------------------------------------------

test('open questions surface (id, by, text); answered ones are excluded', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  const q1 = st.mutate((s) => db.addQuestion(s, { cardId: '#1', text: 'why not B?', by: 'owner' })).result;
  const q2 = st.mutate((s) => db.addQuestion(s, { cardId: '#1', text: 'answered one', by: 'owner' })).result;
  st.mutate((s) => db.answerQuestion(s, q2.id, 'because A', 'agent-1'));
  const p = buildBrief(st.load(), '#1');
  assert.equal(p.questions.length, 1);
  assert.deepEqual(p.questions[0], { id: q1.id, by: 'owner', text: 'why not B?' });
});

// ---- 6. refs: explicit field + auto-harvest from body/plan, deduped ------------

test('refs merges the explicit refs[] field with paths harvested from body + plan, deduped', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, {
    title: 'A',
    body: 'See docs/spec/architecture.md for R1-R12, and examples/features/basics/hello.jet for the golden case.',
    plan: '1. read crates/jet-sema/src/lib.rs. 2. update docs/spec/architecture.md (already read).',
    refs: ['plugins/tower/AGENTS.md', 'docs/spec/architecture.md'],
  }, cfg));
  const p = buildBrief(st.load(), '#1');
  assert.deepEqual(new Set(p.refs), new Set([
    'plugins/tower/AGENTS.md',
    'docs/spec/architecture.md',
    'examples/features/basics/hello.jet',
    'crates/jet-sema/src/lib.rs',
  ]));
  // deduped, not doubled, even though architecture.md appears 3 times total
  assert.equal(p.refs.filter(r => r === 'docs/spec/architecture.md').length, 1);
});

test('refs harvest strips trailing sentence punctuation off a path', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', body: 'Read docs/spec/diagnostics.md, then tests/ui (snapshot dir).' }, cfg));
  const p = buildBrief(st.load(), '#1');
  assert.ok(p.refs.includes('docs/spec/diagnostics.md'));
  assert.ok(p.refs.includes('tests/ui'));
  assert.ok(!p.refs.some(r => r.endsWith(',') || r.endsWith(')')));
});

// ---- 7. recent log (last 5) + rules footer -------------------------------------

test('log is capped at the 5 most recent entries; rules footer is the standard 5 lines', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  for (let i = 0; i < 8; i++) st.mutate((s, cfg) => db.updateCard(s, '#1', { logEntry: `entry ${i}`, by: 'agent' }, cfg));
  const p = buildBrief(st.load(), '#1');
  assert.equal(p.log.length, 5);
  assert.equal(p.log[0].text, 'entry 7', 'newest first');
  assert.equal(p.rules.length, 5);
  assert.match(p.rules.join(' '), /--by/);
  assert.match(p.rules.join(' '), /E_CRITERIA_SELF/);
  assert.match(p.rules.join(' '), /--handoff/);
});

test('buildBrief throws E_NOT_FOUND for an unknown card', () => {
  const st = fresh();
  assert.throws(() => buildBrief(st.load(), '#99'), (e) => e instanceof TowerError && e.code === 'E_NOT_FOUND');
});

// ---- 8. CLI: claim behavior, pick-next, --json shape --------------------------

const TOWER = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');
const run = (cwd, args, ok = true) => {
  try {
    return { out: execFileSync(process.execPath, [TOWER, ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } }), code: 0 };
  } catch (e) {
    if (ok) throw e;
    return { out: (e.stdout || '') + (e.stderr || ''), code: e.status };
  }
};

test('cli: tower brief --agent claims the card', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Do it', '--json']);
  const p = JSON.parse(run(cwd, ['brief', '#1', '--agent', 'agent-x', '--json']).out);
  assert.equal(p.card.assignee, 'agent-x');
});

test('cli: tower brief --agent respects an existing claim by someone else (E_CLAIMED)', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Do it', '--json']);
  run(cwd, ['card', 'claim', '#1', '--by', 'agent-x']);
  const r = run(cwd, ['brief', '#1', '--agent', 'agent-y'], false);
  assert.equal(r.code, 1);
  assert.match(r.out, /active work lease held by agent-x/);
});

test('cli: tower brief --agent is a no-op when the same agent already holds the claim', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Do it', '--json']);
  run(cwd, ['card', 'claim', '#1', '--by', 'agent-x']);
  const p = JSON.parse(run(cwd, ['brief', '#1', '--agent', 'agent-x', '--json']).out);
  assert.equal(p.card.assignee, 'agent-x');
});

test('cli: --no-claim never assigns, and no --agent is read-only', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Do it', '--json']);
  run(cwd, ['brief', '#1', '--agent', 'agent-x', '--no-claim', '--json']);
  let s = JSON.parse(run(cwd, ['card', 'show', '#1', '--json']).out);
  assert.equal(s.assignee, null);
  run(cwd, ['brief', '#1', '--json']);
  s = JSON.parse(run(cwd, ['card', 'show', '#1', '--json']).out);
  assert.equal(s.assignee, null);
});

test('cli: no ref picks the top card via next\'s picker (workOrder respected)', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Second', '--json']);
  run(cwd, ['card', 'update', '#1', '--work-order', '2', '--by', 'owner']);
  run(cwd, ['card', 'add', '--title', 'First', '--json']);
  run(cwd, ['card', 'update', '#2', '--work-order', '1', '--by', 'owner']);
  const p = JSON.parse(run(cwd, ['brief', '--json']).out);
  assert.equal(p.card.num, 2, 'lowest workOrder wins');
});

test('cli: --json shape is {card, blockers, criteria, decisions, questions, refs, log, rules}', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Do it', '--json']);
  const p = JSON.parse(run(cwd, ['brief', '#1', '--json']).out);
  assert.deepEqual(new Set(Object.keys(p)), new Set(['card', 'blockers', 'criteria', 'decisions', 'questions', 'refs', 'log', 'rules']));
});

test('cli: human render is non-empty and includes the rules footer', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-brief-cli-'));
  run(cwd, ['init', '--name', 'CLI']);
  run(cwd, ['card', 'add', '--title', 'Render me', '--body', 'the body text', '--json']);
  const out = run(cwd, ['brief', '#1']).out;
  assert.match(out, /#1 Render me/);
  assert.match(out, /the body text/);
  assert.match(out, /RULES/);
  assert.match(out, /--handoff/);
});
