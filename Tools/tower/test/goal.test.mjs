// Goals: long-term aims that cards point at. A goal has no phase, criteria,
// lane, or closure gate, and it never blocks a card.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { lint, ruleUnknownGoal } from '../app/lint.mjs';
import { writeJSON } from '../app/paths.mjs';

const { empty, openStore, TowerError } = db;

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-goal-'));
  writeJSON(join(dir, 'tower.json'), empty('Goals'));
  return openStore(dir);
};
const code = (expected) => (e) => e instanceof TowerError && e.code === expected;

test('a goal is added, updated, and logged with attribution', () => {
  const st = fresh();
  const { result: g1 } = st.mutate((s) => db.addGoal(s, { title: 'Self-compile in about 50 ms', body: 'Whole compiler, warm cache.', by: 'owner' }));
  const { result: g2 } = st.mutate((s) => db.addGoal(s, { title: 'Remove Rust', by: 'owner' }));
  assert.equal(g1.id, 'G1');
  assert.equal(g2.id, 'G2');
  assert.equal(g1.status, 'active');

  st.mutate((s) => db.updateGoal(s, 'g2', { status: 'achieved', by: 'owner' }));
  st.mutate((s) => db.updateGoal(s, '1', { title: 'Self-compile in about 50 ms (warm)', by: 'planner' }));
  st.mutate((s) => db.updateGoal(s, 'G1', { logEntry: 'measured 410 ms today', by: 'bench-agent' }));
  st.mutate((s) => db.updateGoal(s, 'G1', { logEntry: 'measured 380 ms', by: 'bench-agent' }));

  const s = st.load();
  const goal = db.findGoal(s, 'G1');
  assert.equal(goal.title, 'Self-compile in about 50 ms (warm)');
  assert.equal(goal.updatedBy, 'bench-agent');
  assert.deepEqual(goal.log.map(e => e.text), ['measured 380 ms', 'measured 410 ms today'], 'newest log entry first');
  assert.equal(goal.log[0].by, 'bench-agent');
  assert.deepEqual(db.listGoals(s).map(g => g.id), ['G1'], 'default list shows active goals');
  assert.deepEqual(db.listGoals(s, { all: true }).map(g => g.id), ['G1', 'G2']);
  const actions = s.events.filter(e => e.action.startsWith('goal.')).map(e => `${e.action} ${e.ref}`);
  assert.deepEqual(actions.sort(), ['goal.add G1', 'goal.add G2', 'goal.log G1', 'goal.log G1', 'goal.update G1', 'goal.update G2']);
});

test('goal writes reject a missing title, unknown status, empty log, missing author, and unknown goal', () => {
  const st = fresh();
  assert.throws(() => st.mutate((s) => db.addGoal(s, { title: ' ', by: 'owner' })), code('E_INVALID'));
  assert.throws(() => st.mutate((s) => db.addGoal(s, { title: 'No author' })), code('E_INVALID'));
  st.mutate((s) => db.addGoal(s, { title: 'Port all scripts to Jet', by: 'owner' }));
  assert.throws(() => st.mutate((s) => db.updateGoal(s, 'G1', { status: 'done', by: 'owner' })), code('E_INVALID'));
  assert.throws(() => st.mutate((s) => db.updateGoal(s, 'G1', { logEntry: '  ', by: 'owner' })), code('E_INVALID'));
  assert.throws(() => st.mutate((s) => db.updateGoal(s, 'G1', { title: 'x' })), code('E_INVALID'));
  assert.throws(() => st.mutate((s) => db.updateGoal(s, 'G7', { status: 'dropped', by: 'owner' })), code('E_NOT_FOUND'));
  assert.equal(st.load().goals[0].status, 'active', 'refused writes change nothing');
});

test('cards link goals by id; unknown goals are rejected and links can be cleared', () => {
  const st = fresh();
  st.mutate((s) => db.addGoal(s, { title: 'Remove TIR/MIR/LIR', by: 'owner' }));
  st.mutate((s) => db.addGoal(s, { title: 'Typed IR with a Core lint', by: 'owner' }));
  const { result: card } = st.mutate((s, cfg) => db.addCard(s, { title: 'Delete MIR', goals: 'g1, G2,1', by: 'planner' }, cfg));
  assert.deepEqual(card.goals, ['G1', 'G2'], 'refs normalize and dedupe');
  assert.throws(() => st.mutate((s, cfg) => db.addCard(s, { title: 'Bad', goals: ['G9'], by: 'planner' }, cfg)), code('E_NOT_FOUND'));
  assert.throws(() => st.mutate((s, cfg) => db.updateCard(s, '#1', { goals: ['G1', 'G9'], by: 'planner' }, cfg)), code('E_NOT_FOUND'));
  assert.deepEqual(st.load().cards[0].goals, ['G1', 'G2'], 'refused update keeps links');
  st.mutate((s, cfg) => db.updateCard(s, '#1', { goals: [], by: 'planner' }, cfg));
  assert.deepEqual(st.load().cards[0].goals, []);
});

test('goalCards lists live and archived linked cards with their phases', () => {
  const st = fresh();
  st.mutate((s) => db.addGoal(s, { title: 'Remove Rust', by: 'owner' }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Port lexer', goals: ['G1'], by: 'planner' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Unrelated', by: 'planner' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Port parser', goals: ['G1'], phase: 'building', by: 'planner' }, cfg));
  const s = st.load();
  const archived = [{ id: 'old', num: 0, title: 'Port driver', phase: 'done', goals: ['G1'] }];
  const rows = db.goalCards(db.findGoal(s, 'G1'), s.cards, archived);
  assert.deepEqual(rows.map(r => [r.num, r.title, r.phase, !!r.archived]), [
    [1, 'Port lexer', 'planning', false],
    [3, 'Port parser', 'building', false],
    [0, 'Port driver', 'done', true],
  ]);
  const board = db.projectBoard(s);
  assert.deepEqual(board.goals.map(g => [g.id, g.cards.map(c => c.num)]), [['G1', [1, 3]]]);
  const full = db.project(s, null, { cards: archived });
  assert.deepEqual(full.goals[0].cards.map(c => c.num), [1, 3, 0]);
});

test('a goal never blocks: lane, next picks, and closure ignore it', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Faster codegen', by: 'planner' }, cfg));
  const before = st.load();
  const laneBefore = db.laneOf(before.cards[0], before.decisions, before.cards);
  st.mutate((s) => db.addGoal(s, { title: 'Self-compile in about 50 ms', by: 'owner' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { goals: ['G1'], by: 'planner' }, cfg));
  const after = st.load();
  assert.deepEqual(db.laneOf(after.cards[0], after.decisions, after.cards), laneBefore);
  assert.deepEqual(db.nextCards(after).map(c => c.num), db.nextCards(before).map(c => c.num));
  st.mutate((s) => db.addCriterion(s, '#1', 'codegen is faster', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'bench shows 12% less time', by: 'builder' }));
  const { result } = st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg));
  assert.equal(result.phase, 'done', 'an active goal does not hold the card open');
  assert.equal(st.load().goals[0].status, 'active', 'closing a card does not change its goal');
});

test('normalize defaults goals on old boards and stored goal shape is validated', () => {
  const old = empty('Old');
  delete old.goals;
  old.cards.push({ id: 'c1', num: 1, title: 'Legacy', phase: 'planning', track: 'sidequest', criteria: [] });
  const s = db.normalize(old);
  assert.deepEqual(s.goals, []);
  assert.deepEqual(s.cards[0].goals, []);

  const dir = mkdtempSync(join(tmpdir(), 'tower-goal-bad-'));
  writeJSON(join(dir, 'tower.json'), { ...empty('Bad'), goals: [{ id: 'G1', title: 'x', status: 'finished' }] });
  assert.throws(() => openStore(dir).load(), code('E_INVALID'));
  const dir2 = mkdtempSync(join(tmpdir(), 'tower-goal-bad-'));
  const bad = empty('Bad');
  bad.cards.push({ id: 'c1', num: 1, title: 'x', phase: 'planning', track: 'sidequest', criteria: [], goals: 'G1' });
  bad.meta.nextNum = 2;
  writeJSON(join(dir2, 'tower.json'), bad);
  assert.throws(() => openStore(dir2).load(), code('E_INVALID'));
});

test('lint reports a card linking an unknown goal', () => {
  const st = fresh();
  st.mutate((s) => db.addGoal(s, { title: 'Remove Rust', by: 'owner' }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Port lexer', goals: ['G1'], by: 'planner' }, cfg));
  const s = st.load();
  assert.deepEqual(ruleUnknownGoal(s), []);
  s.cards[0].goals.push('G4');
  const findings = lint(s, st.loadHistory()).filter(f => f.rule === 'unknown-goal');
  assert.equal(findings.length, 1);
  assert.equal(findings[0].ref, '#1');
  assert.match(findings[0].msg, /unknown goal G4/);
});

const TOWER = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');
const run = (cwd, args) => execFileSync(process.execPath, [TOWER, ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
const fails = (cwd, args) => {
  try { execFileSync(process.execPath, [TOWER, ...args], { cwd, encoding: 'utf8', stdio: 'pipe', env: { ...process.env, TOWER_DATA: '' } }); }
  catch (e) { return `${e.stdout || ''}${e.stderr || ''}`; }
  assert.fail(`expected failure: tower ${args.join(' ')}`);
};

test('cli: goal add/list/show/update/log and card --goals', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-goal-cli-'));
  run(cwd, ['init', '--name', 'Goal CLI']);
  const goal = JSON.parse(run(cwd, ['goal', 'add', '--title', 'Self-compile in about 50 ms', '--body', 'Warm cache, whole compiler.', '--by', 'owner', '--json']));
  assert.equal(goal.id, 'G1');
  run(cwd, ['goal', 'add', '--title', 'Remove Rust', '--by', 'owner']);
  run(cwd, ['card', 'add', '--title', 'Faster parser', '--goals', 'G1', '--by', 'planner']);
  run(cwd, ['card', 'add', '--title', 'Faster codegen', '--by', 'planner']);
  run(cwd, ['card', 'update', '#2', '--goals', 'G1,G2', '--by', 'planner']);
  run(cwd, ['card', 'update', '#2', '--phase', 'building', '--by', 'planner']);
  assert.match(fails(cwd, ['card', 'update', '#1', '--goals', 'G9', '--by', 'planner']), /no goal G9/);

  const shown = run(cwd, ['goal', 'show', 'G1']);
  assert.match(shown, /^G1 \[active\] Self-compile in about 50 ms/);
  assert.match(shown, /Warm cache, whole compiler\./);
  assert.match(shown, /#1\s+planning\s+Faster parser/);
  assert.match(shown, /#2\s+building\s+Faster codegen/);
  const json = JSON.parse(run(cwd, ['goal', 'show', 'g1', '--json']));
  assert.deepEqual(json.cards.map(c => [c.num, c.phase]), [[1, 'planning'], [2, 'building']]);

  run(cwd, ['goal', 'log', 'G1', '--text', 'measured 410 ms', '--by', 'bench-agent']);
  run(cwd, ['goal', 'update', 'G2', '--status', 'achieved', '--by', 'owner']);
  assert.match(run(cwd, ['goal', 'show', 'G1']), /log:\n\s+\d{4}-\d\d-\d\d \[bench-agent\] measured 410 ms/);
  const listed = run(cwd, ['goal', 'list']);
  assert.match(listed, /G1\s+active\s+0\/2 cards done\s+Self-compile/);
  assert.doesNotMatch(listed, /Remove Rust/);
  assert.match(run(cwd, ['goal', 'list', '--all']), /G2\s+achieved/);
  assert.match(fails(cwd, ['goal', 'update', 'G1', '--by', 'owner']), /needs --title, --body, or --status/);
  assert.match(fails(cwd, ['goal', 'bogus']), /unknown goal verb/);

  run(cwd, ['card', 'update', '#2', '--goals', '', '--by', 'planner']);
  assert.deepEqual(JSON.parse(run(cwd, ['goal', 'show', 'G1', '--json'])).cards.map(c => c.num), [1]);
  const stored = JSON.parse(readFileSync(join(cwd, '.tower', 'tower.json'), 'utf8'));
  assert.deepEqual(stored.cards.map(c => c.goals), [['G1'], []]);
  assert.match(run(cwd, ['help']), /tower goal show/);
  run(cwd, ['help', '--check']);
});
