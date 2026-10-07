import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import childProcess from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { collectDigest, decisionState, parseNameStatus, parseWorktrees, queryMatcher, renderText,
  subjectRows, workingFingerprint } from './jet-state.mjs';
import { normalize } from '../tower/app/store.mjs';
import { readLedger } from './lexical-ledger.mjs';
import { render, transcript } from '../../.agents/skills/mine-for-jet-v2/scripts/transcript.mjs';

const sha = value => createHash('sha256').update(value).digest('hex');
const HEAD = 'a'.repeat(40);
const card = (num, phase, title = 'memory model', extra = {}) => ({ id: `c${num}`, num, title, phase,
  kind: 'feature', refs: [], blockedBy: [], criteria: [], ...extra });
const decision = (id, cardId, extra = {}) => ({ id, cardId, title: 'memory model', status: 'ratified',
  draft: false, outcome: 'A', options: [{ key: 'A', title: 'Selected contract' }], ...extra });
const stateOf = (cards, decisions, extra = {}) => normalize({ meta: { rev: 7 }, cards, decisions, ...extra }, [], false);
function fixture(t) {
  const scratch = process.env.JET_SCRATCH_ROOT || join(homedir(), '.cache/jet-dev/scratch');
  mkdirSync(scratch, { recursive: true });
  const root = mkdtempSync(join(scratch, 'jet-state-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const file = (path, value) => { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), value); };
  const git = (...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  git('init', '-b', 'master'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
  file('memory.jet', '// memory model initial\n'); file('delete-memory.jet', '// memory deleted\n');
  file('neutral.jet', '// unrelated initial\n');
  file('crates/jet-foundation/src/Syntax.rs', 'pub const LEXICAL_LEDGER: &[LexicalEntry] = &[\n    LexicalEntry { spelling: "borrow", meaning: "memory model borrow", decision: "D-MEM" },\n];\n');
  file('Docs/research/memory.md', '# Memory model\n\nMechanism: https://example.org/paper?edition=2#evaluation\n');
  file('Docs/proposals/build.md', '# Build system\n\nUnratified packaging proposal.\n');
  const state = stateOf([card(1, 'ready')], [decision('D-MEM', 'c1')]);
  file('.tower/tower.json', JSON.stringify(state));
  // Only enumerated fixture files are staged, never broad user checkout operations.
  git('add', 'memory.jet', 'delete-memory.jet', 'neutral.jet', 'crates/jet-foundation/src/Syntax.rs',
    'Docs/research/memory.md', 'Docs/proposals/build.md', '.tower/tower.json');
  git('commit', '-m', 'fixture'); git('branch', 'dev');
  return { root, file, git, state };
}

// Classification deliberately separates law, observed proof, and work phase.
test('ratified ready is unbuilt-or-partial evidence, not an open ballot or runtime proof', () => {
  const result = decisionState(decision('D-MEM', 'c1'), card(1, 'ready'), { head: HEAD, dirty: false });
  assert.equal(result.built, 'ratified-unbuilt-or-partial'); assert.equal(result.intended, 'ratified');
  assert.equal(result.observed, 'unknown'); assert.equal(result.work, 'ready');
});
test('done alone does not prove implementation; proposal-only done stays a proposal', () => {
  assert.equal(decisionState(decision('D-MEM', 'c1'), card(1, 'done')).built, 'ratified-build-evidence-unknown');
  assert.equal(decisionState(decision('D-MEM', 'c1'), card(1, 'done', 'memory proposal-only')).built, 'proposal-only');
});
test('built requires matching clean commit, exercised whole-scope receipt, command and source witness hash', () => {
  const receipt = { decision_id: 'D-MEM', scope: 'decision-acceptance', commit: HEAD, exercised: true,
    command: 'node --test fixture', witnesses: [{ path: 'memory.jet', sha256: sha('source') }] };
  const context = { head: HEAD, dirty: false, receipts: [receipt], witnesses: { 'memory.jet': sha('source') } };
  assert.equal(decisionState(decision('D-MEM', 'c1'), card(1, 'done'), context).built, 'ratified-built');
  for (const patch of [{ dirty: true }, { head: 'b'.repeat(40) }, { witnesses: {} },
    { receipts: [{ ...receipt, scope: 'one-test-only' }] }, { receipts: [{ ...receipt, exercised: false }] },
    { receipts: [{ ...receipt, command: '' }] }, { receipts: [{ ...receipt, witnesses: [] }] }]) {
    assert.notEqual(decisionState(decision('D-MEM', 'c1'), card(1, 'done'), { ...context, ...patch }).built, 'ratified-built');
  }
});
test('frozen, ready and building remain distinct; exact links retrieve a differently named owner', () => {
  const state = stateOf([card(1, 'frozen'), card(2, 'ready'), card(3, 'building', 'canonical owner')],
    [decision('D-MEM', 'c3')]);
  const rows = subjectRows({ subject: 'memory model', state });
  assert.equal(rows.cards.find(c => c.ref === '#1').in_flight, false);
  assert.equal(rows.cards.find(c => c.ref === '#1').lane.lane, 'frozen');
  assert.equal(rows.cards.find(c => c.ref === '#2').in_flight, false);
  assert.equal(rows.cards.find(c => c.ref === '#3').in_flight, true);
  assert.match(rows.cards.find(c => c.ref === '#3').match_reasons[0], /decision owner/);
});
test('open and draft ballots do not become planned law; rejected/deleted historical context survives', () => {
  const state = stateOf([card(1, 'deciding')], [decision('D-OPEN', 'c1', { status: 'open' }),
    decision('D-DRAFT', 'c1', { status: 'open', draft: true })]);
  const history = { cards: [], decisions: [decision('D-REJECT', 'c1', { status: 'rejected' })],
    events: [{ ref: 'D-REJECT', action: 'decision.delete', note: 'memory duplicate deletion, not feature rejection' }] };
  const rows = subjectRows({ subject: 'memory', state, history });
  assert.deepEqual(rows.open_ballots.map(d => d.ref), ['D-OPEN', 'D-DRAFT']);
  assert.equal(rows.decisions.find(d => d.ref === 'D-REJECT').archived, true);
  assert.equal(rows.historical_context[0].action, 'decision.delete');
  assert.equal(rows.decisions[0].axes.built, 'not-ratified');
});
test('explicit verdict supersession beats dated prose, unlinked decisions stay unresolved', () => {
  const state = stateOf([card(1, 'done')], [decision('D-OLD', 'c1'),
    decision('D-NEW', 'c1', { title: 'owner ruling', group: 'verdict', supersededBy: 'D-OLD', comment: 'one meaning' }),
    decision('D-OTHER', 'c1', { ratifiedAt: '2099-01-01' })]);
  const rows = subjectRows({ subject: 'memory', state });
  assert.deepEqual(rows.decisions[0].replaced_by, ['D-NEW']);
  assert.equal(rows.decisions[0].authority, 'explicitly-superseded');
  assert.equal(rows.decisions.find(d => d.ref === 'D-OTHER').authority, 'no-explicit-successor-known');
  assert.equal(rows.decisions.find(d => d.ref === 'D-NEW').owner_comment, 'one meaning');
});
test('merged-owner evidence is retained without claiming separately built products', () => {
  const state = stateOf([card(1, 'done', 'memory work merged into #2', { refs: ['#2'] }), card(2, 'ready')],
    [decision('D-MEM', 'c1')], { events: [{ ref: 'c1', action: 'card.merge', note: 'merged into #2' }] });
  const rows = subjectRows({ subject: 'memory', state });
  assert.deepEqual(rows.cards[0].refs, ['#2']); assert.equal(rows.historical_context[0].action, 'card.merge');
  assert.notEqual(rows.decisions[0].axes.built, 'ratified-built');
});
test('candidate words discover but do not join equivalent meanings', () => {
  assert.deepEqual(queryMatcher('memory model')('memory ownership'), ['memory']);
  assert.throws(() => queryMatcher('!!!'), /subject/);
});
test('name-status parser preserves spaces, staged deletions, copies and rename source/destination', () => {
  assert.deepEqual(parseNameStatus('M\0memory model.jet\0D\0deleted.jet\0R100\0old.jet\0new.jet\0C75\0x.jet\0y.jet\0'),
    [{ status: 'M', path: 'memory model.jet' }, { status: 'D', path: 'deleted.jet' },
      { status: 'R100', old_path: 'old.jet', path: 'new.jet' }, { status: 'C75', old_path: 'x.jet', path: 'y.jet' }]);
  assert.throws(() => parseNameStatus('R100\0old\0'), /incomplete/);
});
test('named and detached worktree metadata is explicit', () => {
  assert.deepEqual(parseWorktrees(`worktree /repo\nHEAD ${HEAD}\nbranch refs/heads/dev\n\nworktree /other\nHEAD ${HEAD}\ndetached\nlocked reason\n`),
    [{ path: '/repo', head: HEAD, branch: 'dev', locked: false, prunable: false },
      { path: '/other', head: HEAD, branch: null, locked: true, prunable: false }]);
});
test('live digest is read-only, root-relative, and records research identity/selectors and registry rows', t => {
  const f = fixture(t); const before = readFileSync(join(f.root, '.tower/tower.json'));
  const digest = collectDigest({ root: f.root, subject: 'memory model' });
  assert.equal(digest.schema, 'jet-state:v1'); assert.equal(digest.providers.tower.detail.revision, 7);
  assert.equal(digest.consistent, true); assert.equal(digest.cards[0].phase, 'ready');
  assert.equal(digest.idioms[0].spelling, 'borrow');
  assert.equal(digest.research[0].path, 'Docs/research/memory.md');
  assert.deepEqual(digest.research[0].source_urls, ['https://example.org/paper?edition=2#evaluation']);
  assert.equal(digest.providers.receipts.status, 'unknown'); assert.equal(digest.providers.pending_cache.status, 'unknown');
  assert.deepEqual(readFileSync(join(f.root, '.tower/tower.json')), before);
  assert.match(renderText(digest), /done ≠ built/);
});
test('two roots produce identical subject rows with no machine/cache prerequisites', t => {
  const a = fixture(t); const b = fixture(t);
  const one = collectDigest({ root: a.root, subject: 'memory model' });
  const two = collectDigest({ root: b.root, subject: 'memory model' });
  assert.deepEqual(one.decisions, two.decisions); assert.deepEqual(one.cards, two.cards);
  assert.deepEqual(one.research, two.research); assert.deepEqual(one.idioms, two.idioms);
});
test('linked checkout uses main authoritative board, never its stale tracked copy', t => {
  const f = fixture(t); const linked = join(f.root, '.agent-worktrees/linked');
  f.git('worktree', 'add', '-b', 'fixture-linked', linked, 'master');
  const updated = structuredClone(f.state); updated.meta.rev = 8; updated.cards[0].phase = 'frozen';
  f.file('.tower/tower.json', JSON.stringify(updated));
  const digest = collectDigest({ root: linked, subject: 'memory model' });
  assert.equal(digest.providers.tower.detail.revision, 8); assert.equal(digest.cards[0].phase, 'frozen');
  rmSync(join(f.root, '.tower/tower.json'));
  const missing = collectDigest({ root: linked, subject: 'memory' });
  assert.equal(missing.providers.tower.status, 'unknown'); assert.deepEqual(missing.cards, []);
});
test('dev/worktree diff includes hunk-only matches, renamed/deleted paths and staged/unstaged/untracked lanes', t => {
  const f = fixture(t); f.git('checkout', 'dev'); f.file('neutral.jet', '// memory model change\n');
  f.git('add', 'neutral.jet'); f.git('commit', '-m', 'memory semantic candidate');
  f.git('mv', 'memory.jet', 'memory-renamed.jet'); f.git('rm', 'delete-memory.jet');
  f.file('neutral.jet', '// memory model unstaged\n'); f.file('new-memory.jet', '// untracked\n');
  const digest = collectDigest({ root: f.root, subject: 'memory model' });
  const dev = digest.branches.find(b => b.name === 'dev' && !b.branch);
  assert.equal(dev.committed.relevant_files.find(f => f.path === 'neutral.jet').match, 'hunk');
  const wt = digest.branches.find(b => b.name === '.');
  assert.ok(wt.staged.relevant_files.some(f => f.status.startsWith('R') && f.old_path === 'memory.jet'));
  assert.ok(wt.staged.relevant_files.some(f => f.status === 'D'));
  assert.ok(wt.unstaged.relevant_files.some(f => f.path === 'neutral.jet'));
  assert.ok(wt.untracked.some(f => f.path === 'new-memory.jet'));
  assert.ok(wt.unstaged.relevant_files.every(f => f.semantic_state === 'unreviewed'));
});
test('dirty file content changes alter freshness fingerprint even when XY status is unchanged', t => {
  const f = fixture(t); f.file('memory.jet', '// memory dirty a\n');
  const status = ' M memory.jet\0'; const before = workingFingerprint(f.root, status);
  f.file('memory.jet', '// memory dirty changed again\n');
  assert.notEqual(workingFingerprint(f.root, status), before);
});
test('missing dev and unsupported registry form are unknown, not empty current facts', t => {
  const f = fixture(t); f.git('branch', '-D', 'dev');
  f.file('crates/jet-foundation/src/Syntax.rs', 'pub const LEXICAL_LEDGER: &[LexicalEntry] = &[\n LexicalEntry { spelling: computed(), meaning: "memory", decision: "D-MEM" },\n];\n');
  const digest = collectDigest({ root: f.root, subject: 'memory' });
  assert.equal(digest.providers.development_branch.status, 'unknown');
  assert.equal(digest.providers.syntax.status, 'unknown'); assert.deepEqual(digest.idioms, []);
  assert.throws(() => readLedger(f.root), /parse every/);
});
test('provided exercised receipt with exact source hash establishes built only at clean matching checkout', t => {
  const f = fixture(t);
  const receipt = { decision_id: 'D-MEM', scope: 'decision-acceptance', commit: f.git('rev-parse', 'HEAD'),
    exercised: true, command: 'fixture acceptance', witnesses: [{ path: 'memory.jet', sha256: sha(readFileSync(join(f.root, 'memory.jet'))) }] };
  // Out-of-checkout receipt keeps the tested checkout clean.
  const path = join(f.root, '.git/fixture-receipts.json'); writeFileSync(path, JSON.stringify([receipt]));
  const digest = collectDigest({ root: f.root, subject: 'memory', receiptsPath: path });
  assert.equal(digest.decisions[0].axes.built, 'ratified-built');
  f.file('memory.jet', '// modified memory\n');
  assert.notEqual(collectDigest({ root: f.root, subject: 'memory', receiptsPath: path }).decisions[0].axes.built, 'ratified-built');
});
test('compact view labels truncated previews rather than silently shrinking declared scope', t => {
  const f = fixture(t); const digest = collectDigest({ root: f.root, subject: 'memory' });
  const view = renderText({ ...digest, cards: Array.from({ length: 10 }, (_, i) => ({ ref: `#${i}`, title: 'memory', phase: 'ready' })) });
  assert.match(view, /3 more in JSON/);
});
test('transcript helper preserves start time, merges nearby progressive captions, and never drops later repeated claims', () => {
  const rows = transcript({ events: [{ tStartMs: 1000, segs: [{ utf8: 'hello' }] },
    { tStartMs: 2000, segs: [{ utf8: 'hello world' }] }, { tStartMs: 2000, segs: [{ utf8: 'hello world' }] },
    { tStartMs: 9000, segs: [{ utf8: 'hello world' }] }, { tStartMs: 10000, segs: [{ utf8: ' ' }] }] });
  assert.equal(rows.length, 2); assert.equal(rows[0].start_ms, 1000);
  assert.equal(render(rows), '[00:01] hello world\n[00:09] hello world\n');
  assert.throws(() => transcript({}), /events/);
});

test('Tower or dirty file changing during collection marks inconsistent and requires reinvocation', t => {
  const f = fixture(t);
  const original = fs.readFileSync;
  let changed = false;
  const patched = t.mock.method(fs, 'readFileSync', (...args) => {
    const result = original(...args);
    if (!changed && args[0] === join(f.root, 'Docs/research/memory.md')) {
      changed = true;
      f.file('.tower/tower.json', JSON.stringify({ ...f.state, meta: { ...f.state.meta, rev: 8 } }));
      f.file('memory.jet', '// changed memory identity during collection\n');
    }
    return result;
  });
  syncBuiltinESMExports();
  t.after(() => { patched.mock.restore(); syncBuiltinESMExports(); });
  const digest = collectDigest({ root: f.root, subject: 'memory' });
  assert.equal(digest.consistent, false);
  assert.ok(digest.consistency_warnings.some(w => w.includes('Tower changed')));
  assert.ok(digest.consistency_warnings.some(w => w.includes('Git/worktree identity changed')));
});

test('explicit linked TOWER_DATA cannot bypass canonical live board or fall back to stale copy', t => {
  const f = fixture(t); const linked = join(f.root, '.agent-worktrees/linked');
  f.git('worktree', 'add', '-b', 'fixture-linked', linked, 'master');
  const saved = process.env.TOWER_DATA;
  process.env.TOWER_DATA = join(linked, '.tower/tower.json');
  t.after(() => { if (saved === undefined) delete process.env.TOWER_DATA; else process.env.TOWER_DATA = saved; });
  f.file('.tower/tower.json', JSON.stringify({ ...f.state, meta: { ...f.state.meta, rev: 9 } }));
  assert.equal(collectDigest({ root: linked, subject: 'memory' }).providers.tower.detail.revision, 9);
  rmSync(join(f.root, '.tower/tower.json'));
  assert.equal(collectDigest({ root: linked, subject: 'memory' }).providers.tower.status, 'unknown');
});

test('pending Tower pair transaction is unknown/inconsistent without repair writes', t => {
  const f = fixture(t);
  f.file('.tower/backups/repair-transaction.json', '{"fixture":"pending"}');
  const before = readFileSync(join(f.root, '.tower/tower.json'));
  const digest = collectDigest({ root: f.root, subject: 'memory' });
  assert.equal(digest.providers.tower.status, 'unknown'); assert.equal(digest.consistent, false);
  assert.match(digest.consistency_warnings.join('\n'), /transaction pending/);
  assert.deepEqual(readFileSync(join(f.root, '.tower/tower.json')), before);
  assert.equal(readFileSync(join(f.root, '.tower/backups/repair-transaction.json'), 'utf8'), '{"fixture":"pending"}');
});

test('detached checkout remains a usable git provider, not a guessed branch', t => {
  const f = fixture(t); f.git('checkout', '--detach', 'HEAD');
  const digest = collectDigest({ root: f.root, subject: 'memory' });
  assert.equal(digest.providers.git.status, 'available'); assert.equal(digest.checkout.branch, null);
  assert.equal(digest.consistent, true);
});

test('malformed receipt or unknown clean identity never establishes exercised built status', () => {
  const options = { head: HEAD, dirty: false,
    receipts: [{ decision_id: 'D-MEM', scope: 'decision-acceptance', commit: HEAD, exercised: true,
      command: 'acceptance', witnesses: [{}] }], witnesses: {} };
  assert.notEqual(decisionState(decision('D-MEM', 'c1'), card(1, 'done'), options).built, 'ratified-built');
  assert.notEqual(decisionState(decision('D-MEM', 'c1'), card(1, 'done'), { ...options, dirty: undefined }).built, 'ratified-built');
});

test('compact candidate order prioritizes subject titles without dropping broad or linked leads', () => {
  const state = stateOf([card(1, 'ready', 'memory allocation'), card(2, 'ready', 'memory model soundness'),
    card(3, 'ready', 'other owner', { refs: ['c2'] })],
    [decision('D-BROAD', 'c1', { title: 'memory allocation' }), decision('D-TOPIC', 'c2', { title: 'memory model law' })]);
  const rows = subjectRows({ subject: 'memory model', state });
  assert.equal(rows.decisions[0].ref, 'D-TOPIC');
  assert.equal(rows.decisions[0].retrieval_class, 'subject-in-title');
  assert.ok(rows.decisions.some(d => d.ref === 'D-BROAD'));
});

test('work-ledger changes retain exact paths separately without bloating semantic hunks', t => {
  const f = fixture(t);
  f.file('.tower/tower.json', JSON.stringify({ ...f.state, meta: { ...f.state.meta, rev: 8 } }));
  const digest = collectDigest({ root: f.root, subject: 'memory' });
  const wt = digest.branches.find(b => b.name === '.');
  assert.equal(wt.unstaged.ledger_changes[0].path, '.tower/tower.json');
  assert.equal(wt.unstaged.ledger_changes[0].semantic_state, 'work-ledger-not-code');
  assert.deepEqual(wt.unstaged.relevant_files, []);
  assert.equal(digest.providers.tower.detail.revision, 8);
});

test('identical committed worktree heads reuse one diff per invocation', t => {
  const f = fixture(t); const linked = join(f.root, '.agent-worktrees/linked');
  f.git('worktree', 'add', '-b', 'fixture-linked', linked, 'master');
  const original = childProcess.execFileSync; const committedDiffs = [];
  const patched = t.mock.method(childProcess, 'execFileSync', (command, args, options) => {
    if (command === 'git' && args.includes('diff') && args.some(arg => arg.includes('...'))) committedDiffs.push(args);
    return original(command, args, options);
  });
  syncBuiltinESMExports();
  t.after(() => { patched.mock.restore(); syncBuiltinESMExports(); });
  const digest = collectDigest({ root: f.root, subject: 'memory' });
  assert.equal(digest.consistent, true); assert.equal(committedDiffs.length, 1);
});

test('--root resolves a vendored board outside the tools own checkout, including linked checkout redirection', t => {
  const f = fixture(t);
  f.file('Tools/tower/.tower/tower.json', JSON.stringify(f.state));
  rmSync(join(f.root, '.tower/tower.json'));
  f.git('add', '.tower/tower.json', 'Tools/tower/.tower/tower.json');
  f.git('commit', '-m', 'fixture vendored board');
  const digest = collectDigest({ root: f.root, subject: 'memory model' });
  assert.equal(digest.providers.tower.status, 'available');
  assert.equal(digest.providers.tower.detail.path, 'Tools/tower/.tower/tower.json');
  assert.equal(digest.providers.tower.detail.revision, 7);
  const linked = join(f.root, '.agent-worktrees/linked');
  f.git('worktree', 'add', '-b', 'fixture-linked', linked, 'master');
  f.file('Tools/tower/.tower/tower.json', JSON.stringify({ ...f.state, meta: { ...f.state.meta, rev: 10 } }));
  const linkedDigest = collectDigest({ root: linked, subject: 'memory model' });
  assert.equal(linkedDigest.providers.tower.detail.revision, 10);
  rmSync(join(f.root, 'Tools/tower/.tower/tower.json'));
  const missing = collectDigest({ root: linked, subject: 'memory model' });
  assert.equal(missing.providers.tower.status, 'unknown');
  assert.deepEqual(missing.cards, []);
});
