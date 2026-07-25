// Docs tab + OPS2 leftover tests (blocker-unpopulated / ready-across).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';
import { ruleBlockerUnpopulated } from '../app/lint.mjs';
import * as docs from '../app/docs.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-ops2-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

test('OPS2: ready-across returns unblocked cards across epochs, drops blocked', () => {
  const st = fresh();
  st.mutate((s) => db.addEpoch(s, { id: 'e3', name: 'E3', status: 'active' }));
  st.mutate((s) => db.addEpoch(s, { id: 'e4', name: 'E4', status: 'planned' }));
  st.mutate((s, cfg) => db.addCard(s, { title: 'E3 work', track: 'epoch', epoch: 'e3', phase: 'building' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'E4 free', track: 'epoch', epoch: 'e4', phase: 'ready' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'E4 blocked', track: 'epoch', epoch: 'e4', phase: 'ready', blockedBy: ['#1'] }, cfg));

  const s = st.load();
  const titles = db.nextCards(s, { scope: 'ready-across', limit: 20 }).map(c => c.title).sort();
  assert.deepEqual(titles, ['E3 work', 'E4 free']);
});

test('OPS2: blocker-unpopulated flags planning epoch cards with plan + empty blockedBy', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Needs deps', track: 'epoch', phase: 'planning', plan: 'do the thing' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Explicit none', track: 'epoch', phase: 'planning', plan: 'blockedBy: none — leaf work' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Has blocker', track: 'epoch', phase: 'planning', plan: 'wait', blockedBy: ['#1'] }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Sidequest', track: 'sidequest', phase: 'planning', plan: 'x' }, cfg));
  st.mutate((s, cfg) => db.addCard(s, { title: 'Already ready', track: 'epoch', phase: 'ready', plan: 'old plan' }, cfg));

  const findings = ruleBlockerUnpopulated(st.load());
  assert.equal(findings.length, 1);
  assert.equal(findings[0].ref, '#1');
  assert.equal(findings[0].rule, 'blocker-unpopulated');
});

function projectLayout() {
  const proj = mkdtempSync(join(tmpdir(), 'tower-docs-'));
  const dataDir = join(proj, '.tower');
  mkdirSync(dataDir, { recursive: true });
  mkdirSync(join(proj, 'docs', 'proposals'), { recursive: true });
  writeFileSync(join(proj, 'docs', 'proposals', 'idea.md'), '# Idea\n\nhello');
  writeFileSync(join(proj, 'secret.md'), 'nope');
  return { proj, dataDir };
}

test('docs: add/show/update/delete under docs/', () => {
  const { dataDir, proj } = projectLayout();
  const n = docs.addDoc(dataDir, { section: 'research', title: 'Hello World', body: 'body text' });
  assert.equal(n.path, 'docs/research/hello-world.md');
  assert.ok(existsSync(join(proj, n.path)));
  assert.match(docs.showDoc(dataDir, n.path).body, /body text/);
  docs.updateDoc(dataDir, n.path, { body: 'updated\n' });
  assert.equal(docs.showDoc(dataDir, n.path).body.trim(), 'updated');
  docs.deleteDoc(dataDir, n.path);
  assert.equal(existsSync(join(proj, n.path)), false);
});

test('docs: path escape rejected; only docs/*.md writable', () => {
  const { dataDir } = projectLayout();
  const prev = docs.showDoc(dataDir, 'docs/proposals/idea.md');
  assert.match(prev.body, /hello/);
  assert.throws(() => docs.showDoc(dataDir, 'secret.md'), /docs/);
  assert.throws(() => docs.showDoc(dataDir, '../secret.md'), /docs|path/);
  assert.throws(() => docs.showDoc(dataDir, 'docs/proposals/../../secret.md'), /docs|path|escape/);
  assert.throws(() => docs.deleteDoc(dataDir, 'docs/proposals/../../secret.md'), /docs|path|escape/);
});

test('docs: scratchpad seeds and updates; cannot delete via docs delete of scratch path outside docs', () => {
  const { dataDir } = projectLayout();
  const sc = docs.showScratchPad(dataDir);
  assert.equal(sc.kind, 'scratch');
  assert.match(sc.path, /owner-scratch/);
  docs.updateScratchPad(dataDir, { body: 'todo: ship docs tab\n', title: 'Owner scratch' });
  assert.match(docs.showScratchPad(dataDir).body, /todo:/);
});

test('docs: migrates legacy scratch reports into docs/audits|research', () => {
  const { dataDir, proj } = projectLayout();
  const scratch = join(dataDir, 'scratch');
  mkdirSync(scratch, { recursive: true });
  writeFileSync(join(scratch, 'owner-scratch.md'), '---\ntitle: Owner scratch\n---\nkeep\n');
  writeFileSync(join(scratch, 'field-audit-2026-07-23.md'), '# Field audit\n');
  writeFileSync(join(scratch, 'surface-research-2026-07-23.md'), '# Research\n');
  writeFileSync(join(scratch, 'lessons-learned-2026-07-23.md'), '# Lessons\n');

  const moved = docs.migrateScratchReports(dataDir);
  assert.ok(moved.length >= 3);
  assert.ok(existsSync(join(proj, 'docs/audits/field-audit-2026-07-23.md')));
  assert.ok(existsSync(join(proj, 'docs/research/surface-research-2026-07-23.md')));
  assert.ok(existsSync(join(proj, 'docs/research/lessons-learned-2026-07-23.md')));
  assert.ok(existsSync(join(scratch, 'owner-scratch.md')));
  assert.equal(readdirSync(scratch).filter(f => f !== 'owner-scratch.md').length, 0);

  const index = docs.listDocs(dataDir);
  assert.ok(index.sections.find(s => s.id === 'audits').files.some(f => f.path.includes('field-audit')));
  assert.ok(index.sections.find(s => s.id === 'research').files.some(f => f.path.includes('surface-research')));
});

test('docs: migrates legacy dataDir/owner-scratch.md into scratchpad', () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'tower-docs-mig-'));
  writeFileSync(join(dataDir, 'owner-scratch.md'), 'legacy contents');
  const n = docs.migrateOwnerScratch(dataDir);
  assert.match(n.body, /legacy/);
  assert.ok(existsSync(join(dataDir, 'scratch', 'owner-scratch.md')));
});

test('docs: list groups sections and ignores non-md', () => {
  const { dataDir, proj } = projectLayout();
  mkdirSync(join(proj, 'docs', 'research', '_scripts'), { recursive: true });
  writeFileSync(join(proj, 'docs', 'research', '_scripts', 'out.json'), '{}');
  writeFileSync(join(proj, 'docs', 'research', 'note.md'), '# Note\n');
  const index = docs.listDocs(dataDir);
  const research = index.sections.find(s => s.id === 'research').files;
  assert.ok(research.some(f => f.path.endsWith('note.md')));
  assert.ok(!research.some(f => f.path.endsWith('.json')));
});

test('docs: Spec section, sidequests fold into Plans, archive hidden from list', () => {
  const { dataDir, proj } = projectLayout();
  mkdirSync(join(proj, 'docs', 'spec'), { recursive: true });
  mkdirSync(join(proj, 'docs', 'sidequests'), { recursive: true });
  mkdirSync(join(proj, 'docs', 'archive'), { recursive: true });
  mkdirSync(join(proj, 'docs', 'plans'), { recursive: true });
  writeFileSync(join(proj, 'docs', 'spec', 'philosophy.md'), '# Philosophy\n');
  writeFileSync(join(proj, 'docs', 'sidequests', 'web.md'), '# Web sidequest\n');
  writeFileSync(join(proj, 'docs', 'plans', 'e3.md'), '# Epoch 3\n');
  writeFileSync(join(proj, 'docs', 'archive', 'old.md'), '# Old\n');
  mkdirSync(join(proj, 'docs', 'ballots'), { recursive: true });
  writeFileSync(join(proj, 'docs', 'ballots', 'x.md'), '# Ballot\n');

  const index = docs.listDocs(dataDir);
  const ids = index.sections.map(s => s.id);
  assert.ok(ids.includes('spec'));
  assert.ok(!ids.includes('sidequests'));
  assert.ok(!ids.includes('archive'));

  const spec = index.sections.find(s => s.id === 'spec').files;
  assert.ok(spec.some(f => f.path === 'docs/spec/philosophy.md'));

  const plans = index.sections.find(s => s.id === 'plans').files;
  assert.ok(plans.some(f => f.path === 'docs/plans/e3.md'));
  assert.ok(plans.some(f => f.path === 'docs/sidequests/web.md'));

  const allPaths = index.sections.flatMap(s => s.files.map(f => f.path));
  assert.ok(!allPaths.some(p => p.startsWith('docs/archive/')));
  assert.ok(!allPaths.some(p => p.startsWith('docs/ballots/')));
  assert.equal(allPaths.filter(p => p.includes('old.md')).length, 0);
});

test('docs: archive moves to docs/archive and drops from list; delete still removes', () => {
  const { dataDir, proj } = projectLayout();
  mkdirSync(join(proj, 'docs', 'research'), { recursive: true });
  mkdirSync(join(proj, 'docs', 'spec'), { recursive: true });
  writeFileSync(join(proj, 'docs', 'research', 'done-idea.md'), '# Done idea\n');
  writeFileSync(join(proj, 'docs', 'spec', 'philosophy.md'), '# Philosophy\n');

  const archived = docs.archiveDoc(dataDir, 'docs/research/done-idea.md');
  assert.equal(archived.from, 'docs/research/done-idea.md');
  assert.equal(archived.path, 'docs/archive/done-idea.md');
  assert.equal(existsSync(join(proj, 'docs/research/done-idea.md')), false);
  assert.ok(existsSync(join(proj, archived.path)));

  const index = docs.listDocs(dataDir);
  const allPaths = index.sections.flatMap(s => s.files.map(f => f.path));
  assert.ok(!allPaths.includes('docs/research/done-idea.md'));
  assert.ok(!allPaths.includes('docs/archive/done-idea.md'));

  assert.throws(() => docs.archiveDoc(dataDir, 'docs/spec/philosophy.md'), /spec|binding/);
  assert.throws(() => docs.archiveDoc(dataDir, 'docs/archive/done-idea.md'), /already archived/);

  docs.deleteDoc(dataDir, 'docs/proposals/idea.md');
  assert.equal(existsSync(join(proj, 'docs/proposals/idea.md')), false);
});
