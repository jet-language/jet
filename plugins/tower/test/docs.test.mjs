// Docs tab + OPS2 leftover tests (blocker-unpopulated / ready-across).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, readFileSync, readdirSync, symlinkSync } from 'node:fs';
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
  st.mutate((s, cfg) => db.addCard(s, { title: 'E4 verify', track: 'epoch', epoch: 'e4', phase: 'verify', workOrder: 99 }, cfg));

  const s = st.load();
  const titles = db.nextCards(s, { scope: 'ready-across', limit: 20 }).map(c => c.title);
  assert.deepEqual(titles, ['E4 verify', 'E3 work', 'E4 free']);
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

test('owner guidance is hidden from Docs and writable only through its owner API', () => {
  const { dataDir, proj } = projectLayout();
  mkdirSync(join(proj, 'docs', 'agents'), { recursive: true });
  writeFileSync(join(proj, docs.OWNER_GUIDANCE_PATH), '# Owner guidance\n\nold\n');

  const listed = docs.listDocs(dataDir).sections.flatMap(section => section.files.map(file => file.path));
  assert.equal(listed.includes(docs.OWNER_GUIDANCE_PATH), false);
  assert.match(docs.showOwnerGuidance(dataDir).body, /old/);

  const ownerOnly = error => error?.code === 'E_OWNER_ONLY';
  assert.throws(() => docs.addDoc(dataDir, { path: docs.OWNER_GUIDANCE_PATH, body: 'replace' }), ownerOnly);
  assert.throws(() => docs.updateDoc(dataDir, docs.OWNER_GUIDANCE_PATH, { body: 'replace' }), ownerOnly);
  assert.throws(() => docs.deleteDoc(dataDir, docs.OWNER_GUIDANCE_PATH), ownerOnly);
  assert.throws(() => docs.archiveDoc(dataDir, docs.OWNER_GUIDANCE_PATH), ownerOnly);

  docs.updateOwnerGuidance(dataDir, { body: '# Owner guidance\n\nnew\n' });
  assert.match(docs.showOwnerGuidance(dataDir).body, /new/);
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

test('docs: symlinked files and directories cannot redirect reads or writes', () => {
  const { dataDir, proj } = projectLayout();
  const outside = mkdtempSync(join(tmpdir(), 'tower-docs-outside-'));
  writeFileSync(join(outside, 'secret.md'), 'outside secret');
  symlinkSync(outside, join(proj, 'docs', 'linked'));

  for (const operation of [
    () => docs.showDoc(dataDir, 'docs/linked/secret.md'),
    () => docs.addDoc(dataDir, { path: 'docs/linked/new.md', body: 'must not write' }),
    () => docs.updateDoc(dataDir, 'docs/linked/secret.md', { body: 'must not write' }),
    () => docs.deleteDoc(dataDir, 'docs/linked/secret.md'),
    () => docs.archiveDoc(dataDir, 'docs/linked/secret.md'),
  ]) assert.throws(operation, /docs|resolve|escape/);

  const index = docs.listDocs(dataDir);
  const listed = index.sections.flatMap(section => section.files.map(file => file.path));
  assert.equal(listed.some(path => path.includes('linked')), false, 'walk must not follow symlinked directories');
  assert.equal(readFileSync(join(outside, 'secret.md'), 'utf8'), 'outside secret');

  const archiveOutside = mkdtempSync(join(tmpdir(), 'tower-docs-archive-outside-'));
  symlinkSync(archiveOutside, join(proj, 'docs', 'archive'));
  assert.throws(() => docs.archiveDoc(dataDir, 'docs/proposals/idea.md'), /docs|resolve|escape/);
  assert.ok(existsSync(join(proj, 'docs', 'proposals', 'idea.md')), 'archive must not move through a symlink');
});

test('docs: descriptor-relative operations survive hostile directory swaps', async () => {
  const swapper = `
    const fs = require('node:fs');
    const [target, alternate] = process.argv.slice(1);
    const temp = target + '.swap-temp';
    const end = Date.now() + 500;
    while (Date.now() < end) {
      try { fs.renameSync(target, temp); } catch {}
      try { fs.renameSync(alternate, target); } catch {}
      try { fs.renameSync(temp, alternate); } catch {}
    }
  `;
  const cases = [
    ['read', (dataDir) => docs.showDoc(dataDir, 'docs/race/doc.md')],
    ['write', (dataDir) => docs.updateDoc(dataDir, 'docs/race/doc.md', { body: 'inside update' })],
    ['create', (dataDir) => docs.addDoc(dataDir, { path: 'docs/race/new.md', body: 'inside create' })],
    ['delete', (dataDir) => docs.deleteDoc(dataDir, 'docs/race/doc.md')],
    ['rename', (dataDir) => docs.archiveDoc(dataDir, 'docs/race/doc.md')],
  ];

  for (const [name, operation] of cases) {
    const { dataDir, proj } = projectLayout();
    const outside = mkdtempSync(join(tmpdir(), `tower-docs-race-outside-${name}-`));
    writeFileSync(join(outside, 'doc.md'), 'outside secret');
    writeFileSync(join(outside, 'new.md'), 'outside new secret');
    const race = join(proj, 'docs', 'race');
    const alternate = join(proj, 'docs', 'race.swap');
    mkdirSync(race, { recursive: true });
    writeFileSync(join(race, 'doc.md'), 'inside secret');
    symlinkSync(outside, alternate);

    const attacker = spawn(process.execPath, ['-e', swapper, race, alternate], { stdio: 'ignore' });
    for (let i = 0; i < 1_000; i++) {
      try {
        const result = operation(dataDir);
        if (name === 'read') assert.notEqual(result.body, 'outside secret');
      } catch (error) {
        assert.ok(['E_INVALID', 'E_EXISTS', 'E_NOT_FOUND'].includes(error.code), `${name}: ${error.message}`);
      }
    }
    await new Promise((resolve, reject) => {
      attacker.once('error', reject);
      attacker.once('close', resolve);
    });
    assert.equal(readFileSync(join(outside, 'doc.md'), 'utf8'), 'outside secret', `${name} touched outside document`);
    assert.equal(readFileSync(join(outside, 'new.md'), 'utf8'), 'outside new secret', `${name} touched outside new document`);
  }
});

test('docs: scratch migration ignores symlinked report sources', () => {
  const { dataDir, proj } = projectLayout();
  const outside = mkdtempSync(join(tmpdir(), 'tower-scratch-outside-'));
  writeFileSync(join(outside, 'secret.md'), 'outside secret');
  const scratch = join(dataDir, 'scratch');
  mkdirSync(scratch, { recursive: true });
  symlinkSync(join(outside, 'secret.md'), join(scratch, 'audit-leak.md'));

  assert.deepEqual(docs.migrateScratchReports(dataDir), []);
  assert.equal(existsSync(join(proj, 'docs', 'audits', 'audit-leak.md')), false);
  assert.equal(readFileSync(join(outside, 'secret.md'), 'utf8'), 'outside secret');
});

test('docs: symlinked scratch directories and pads are rejected', () => {
  const { dataDir } = projectLayout();
  const outside = mkdtempSync(join(tmpdir(), 'tower-scratch-dir-outside-'));
  writeFileSync(join(outside, 'secret.md'), 'outside secret');
  symlinkSync(outside, join(dataDir, 'scratch'));
  assert.throws(() => docs.migrateScratchReports(dataDir), /scratch/);
  assert.throws(() => docs.showScratchPad(dataDir), /scratch/);

  const { dataDir: padDataDir } = projectLayout();
  const padOutside = mkdtempSync(join(tmpdir(), 'tower-scratch-pad-outside-'));
  writeFileSync(join(padOutside, 'secret.md'), 'outside secret');
  mkdirSync(join(padDataDir, 'scratch'), { recursive: true });
  symlinkSync(join(padOutside, 'secret.md'), join(padDataDir, 'scratch', 'owner-scratch.md'));
  assert.throws(() => docs.showScratchPad(padDataDir), /scratch/);

  const { dataDir: legacyDataDir } = projectLayout();
  const legacyOutside = mkdtempSync(join(tmpdir(), 'tower-legacy-scratch-outside-'));
  writeFileSync(join(legacyOutside, 'secret.md'), 'outside secret');
  symlinkSync(join(legacyOutside, 'secret.md'), join(legacyDataDir, 'owner-scratch.md'));
  assert.throws(() => docs.showScratchPad(legacyDataDir), /legacy scratch/);
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
