// A criterion carries its proof, the `jet test` run that settles it; `tower
// prove` runs the proofs and meets the passing rows, and `tower lint --proofs`
// finds filters that match no test file.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { migrateCriterionProof, parseProofCommand } from '../app/proof.mjs';

const gitEnv = { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' };
for (const key of ['GIT_DIR', 'GIT_INDEX_FILE', 'GIT_WORK_TREE']) delete gitEnv[key];
const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', env: gitEnv }).trim();

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });

// A project repo whose dev branch holds a Tests tree, with one unlanded
// commit; the board lives in <repo>/.tower with card #1.
function project() {
  const root = mkdtempSync(join(tmpdir(), 'tower-proof-'));
  git(root, 'init', '-q', '-b', 'master');
  mkdirSync(join(root, 'Tests/Cases/parser'), { recursive: true });
  mkdirSync(join(root, 'Examples/Apps/Game'), { recursive: true });
  writeFileSync(join(root, 'Tests/Cases/parser/Accepts_Tabs.case'), 'x\n');
  writeFileSync(join(root, 'Examples/Apps/Game/GameAssets.jet'), 'x\n');
  writeFileSync(join(root, '.gitignore'), '.tower/\n');
  git(root, 'add', '.');
  git(root, 'commit', '-q', '-m', 'on master');
  git(root, 'checkout', '-q', '-b', 'dev');
  const onDev = git(root, 'rev-parse', 'HEAD');
  const unlanded = git(root, 'commit-tree', `${onDev}^{tree}`, '-p', onDev, '-m', 'unlanded');
  const data = join(root, '.tower');
  mkdirSync(data);
  writeJSON(join(data, 'tower.json'), db.empty('Proofs'));
  const st = db.openStore(data);
  st.mutate((s, cfg) => db.addCard(s, { title: 'Work' }, cfg));
  return { root, data, st, onDev, unlanded };
}

// A stand-in `jet`: `--list` names three cases; a `--cases` run fails every
// case under b/; any other run exits 0 without result rows.
function fakeJet(dir) {
  const script = join(dir, 'fake-jet.mjs');
  writeFileSync(script, `
import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
const args = process.argv.slice(2);
appendFileSync(${JSON.stringify(join(dir, 'calls.txt'))}, args.join(' ') + '\\n');
if (args.includes('--list')) { console.log('cases\\tparser/a_one.case\\ncases\\tparser/a_two.case\\ncases\\tchecker/b_three.case'); process.exit(0); }
const value = (name) => args.find(a => a.startsWith(name + '='))?.slice(name.length + 1);
const cases = value('--cases');
const results = value('--results');
if (cases) {
  const ids = readFileSync(cases, 'utf8').split('\\n').filter(Boolean);
  const rows = ids.flatMap(id => ['O0', 'O1'].map(tier => ({ suite: 'cases', case: id, tier, status: id.includes('b_') ? 'FAIL' : 'PASS' })));
  writeFileSync(results, JSON.stringify({ jet: 'fake', counts: {}, results: rows }));
  process.exit(rows.some(r => r.status !== 'PASS') ? 1 : 0);
}
process.exit(0);
`);
  const jet = join(dir, 'jet');
  writeFileSync(jet, `#!/bin/sh\nexec "${process.execPath}" "${script}" "$@"\n`);
  chmodSync(jet, 0o755);
  return jet;
}

test('a jet test command parses into a profile with a filter, or into cmd arguments', () => {
  assert.deepEqual(parseProofCommand('jet test Compiler --profile cases --filter game/asset_hot_reload'), { profile: 'cases', filter: 'game/asset_hot_reload' });
  assert.deepEqual(parseProofCommand('`jet test Compiler --profile=golden --filter=Apps/Game/GameAssets`'), { profile: 'golden', filter: 'Apps/Game/GameAssets' });
  assert.deepEqual(parseProofCommand('jet test Tests/Guards --filter docs_links_resolve'), { profile: 'cmd', args: ['Tests/Guards', '--filter', 'docs_links_resolve'] });
  assert.deepEqual(parseProofCommand('jet test Compiler --profile golden --to c'), { profile: 'cmd', args: ['Compiler', '--profile', 'golden', '--to', 'c'] });
  assert.equal(parseProofCommand('git grep -n x'), null);
  assert.equal(parseProofCommand('jet test Compiler --filter "a b"'), null);
});

test('migration moves a trailing Proof sentence into the field and leaves other text alone', () => {
  const row = { n: 1, text: 'The parser accepts tabs. Proof: `jet test Compiler --profile cases --filter parser/tabs`.' };
  assert.equal(migrateCriterionProof(row), true);
  assert.deepEqual(row, { n: 1, text: 'The parser accepts tabs.', proof: { profile: 'cases', filter: 'parser/tabs' } });
  assert.equal(migrateCriterionProof(row), false, 'idempotent');
  const middle = { n: 2, text: 'Proof: `jet test Compiler --profile golden --filter Apps/Game` on O0, O1 and O2.' };
  assert.equal(migrateCriterionProof(middle), false);
  const only = { n: 3, text: 'Proof: `jet test Tests/Guards --filter grammar`' };
  assert.equal(migrateCriterionProof(only), true);
  assert.equal(only.text, 'Proof: `jet test Tests/Guards --filter grammar`', 'a row that is only its proof keeps its text');
  const { st } = project();
  const raw = st.load();
  raw.cards[0].criteria = [{ n: 1, text: 'It works. Proof: `jet test Compiler --profile ui --filter E0441`.', status: 'open', metBy: null, verifiedBy: null, evidence: '', at: '2026-01-01' }];
  const loaded = db.normalize(raw);
  assert.deepEqual(loaded.cards[0].criteria[0].proof, { profile: 'ui', filter: 'E0441' });
  assert.equal(loaded.cards[0].criteria[0].text, 'It works.');
});

test('cli: --proof on add and edit sets, shows, moves and clears the proof; a bad proof changes nothing', () => {
  const { data, st } = project();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Other' }, cfg));
  const added = cli(data, 'card', 'criteria', '#1', '--add', 'tabs parse', '--proof', 'jet test Compiler --profile cases --filter parser/tabs', '--by', 'planner');
  assert.equal(added.status, 0, added.stderr);
  assert.match(cli(data, 'card', 'criteria', '#1', '--list').stdout, /#1 \[open\] tabs parse {2}proof: jet test Compiler --profile cases --filter parser\/tabs/);
  const bad = cli(data, 'card', 'criteria', '#1', '--edit', '1', '--proof', 'make test', '--by', 'planner');
  assert.notEqual(bad.status, 0);
  assert.match(bad.stderr, /--proof takes a jet test command/);
  const edited = cli(data, 'card', 'criteria', '#1', '--edit', '1', '--proof', 'jet test Tests/Guards --filter grammar', '--by', 'planner');
  assert.equal(edited.status, 0, edited.stderr);
  assert.deepEqual(st.load().cards[0].criteria[0].proof, { profile: 'cmd', args: ['Tests/Guards', '--filter', 'grammar'] });
  assert.equal(st.load().cards[0].criteria[0].text, 'tabs parse');
  st.mutate((s) => db.moveCriterion(s, '#1', 1, { to: '#2', by: 'planner' }));
  assert.deepEqual(st.load().cards[1].criteria[0].proof, { profile: 'cmd', args: ['Tests/Guards', '--filter', 'grammar'] });
  assert.equal(cli(data, 'card', 'criteria', '#2', '--edit', '1', '--proof', '', '--by', 'planner').status, 0);
  assert.equal(st.load().cards[1].criteria[0].proof, undefined);
});

test('prove prints the plan without --yes, then meets the passing rows at a landed commit', () => {
  const { root, data, st, onDev, unlanded } = project();
  const jet = fakeJet(mkdtempSync(join(tmpdir(), 'tower-fake-jet-')));
  const add = (text, proof) => st.mutate((s) => db.addCriterion(s, '#1', text, 'planner', { proof }));
  add('parser cases pass', 'jet test Compiler --profile cases --filter parser/');
  add('checker cases pass', 'jet test Compiler --profile cases --filter checker/');
  add('guards pass', 'jet test Tests/Guards --filter grammar');
  add('nothing matches', 'jet test Compiler --profile cases --filter nowhere/');
  add('no proof here');
  const plan = cli(data, 'prove', '--card', '#1');
  assert.equal(plan.status, 0, plan.stderr);
  assert.match(plan.stdout, /plan for card #1: 2 runs for 4 open criteria with a proof; pass --yes to run them/);
  assert.match(plan.stdout, /jet test Compiler --profile cases --cases FILE\n {2}#1 c1 {2}jet test Compiler --profile cases --filter parser\//);
  assert.equal(st.load().cards[0].criteria.filter(i => i.status === 'met').length, 0, 'a plan meets nothing');

  const off = cli(data, 'prove', '--card', '#1', '--yes', '--jet', jet, '--tree', root, '--commit', unlanded, '--by', 'prover');
  assert.notEqual(off.status, 0);
  assert.match(off.stderr, /is not on dev or master/);
  git(root, 'checkout', '-q', '--detach', unlanded);
  const elsewhere = cli(data, 'prove', '--card', '#1', '--yes', '--jet', jet, '--tree', root, '--commit', onDev, '--by', 'prover');
  assert.match(elsewhere.stderr, /--tree .* is at .*, not /);
  git(root, 'checkout', '-q', 'dev');

  const dry = cli(data, 'prove', '--card', '#1', '--yes', '--dry-run', '--jet', jet, '--tree', root, '--commit', onDev, '--json');
  assert.equal(JSON.parse(dry.stdout).met, 0);
  assert.equal(st.load().cards[0].criteria.filter(i => i.status === 'met').length, 0, 'a dry run meets nothing');

  const ran = cli(data, 'prove', '--card', '#1', '--yes', '--jet', jet, '--tree', root, '--commit', onDev, '--by', 'prover');
  assert.equal(ran.status, 1, 'a failing proof makes the run fail');
  const sha = onDev.slice(0, 10);
  const rows = st.load().cards[0].criteria;
  assert.equal(rows[0].status, 'met');
  assert.equal(rows[0].evidence, `${sha}: jet test Compiler --profile cases --filter parser/ PASS 4/4`);
  assert.equal(rows[0].metBy, 'prover');
  assert.equal(rows[1].status, 'open');
  assert.match(ran.stdout, /FAIL #1 c2 {2}FAIL checker\/b_three\.case O0; FAIL checker\/b_three\.case O1/);
  assert.equal(rows[2].status, 'met');
  assert.equal(rows[2].evidence, `${sha}: jet test Tests/Guards --filter grammar PASS`);
  assert.equal(rows[3].status, 'open');
  assert.match(ran.stdout, /FAIL #1 c4 {2}filter nowhere\/ matches no test/);
  assert.equal(rows[4].status, 'open');
  assert.match(ran.stdout, /met 2 of 4 criteria at /);
});

test('lint --proofs lists every proof whose filter matches no Tests or Examples file, case-insensitively', () => {
  const { root, data, st } = project();
  const add = (text, proof) => st.mutate((s) => db.addCriterion(s, '#1', text, 'planner', { proof }));
  add('tabs', 'jet test Compiler --profile cases --filter parser/accepts_tabs');
  add('assets', 'jet test Compiler --profile golden --filter Apps/Game/GameAssets');
  add('gone', 'jet test Compiler --profile cases --filter parser/removed_case');
  add('guard', 'jet test Tests/Guards --filter grammar');
  add('no filter', 'jet test Compiler --profile golden --to c');
  const r = cli(data, 'lint', '--proofs', '--tree', root, '--json');
  assert.equal(r.status, 1);
  assert.deepEqual(JSON.parse(r.stdout).map(f => f.ref), ['#1 c3', '#1 c4']);
  const usage = cli(data, 'lint', '--proofs');
  assert.match(usage.stderr, /lint --proofs needs --tree DIR/);
});
