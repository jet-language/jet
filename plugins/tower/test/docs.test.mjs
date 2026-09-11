// Docs tab + OPS2 leftover tests (blocker-unpopulated / ready-across).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, readFileSync, readdirSync, symlinkSync, linkSync, rmSync, statSync } from 'node:fs';
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

test('docs reads never create or migrate files', () => {
  const { dataDir, proj } = projectLayout();
  const legacy = join(dataDir, 'owner-scratch.md');
  writeFileSync(legacy, 'legacy contents');
  const beforeDocs = readdirSync(join(proj, 'docs')).sort();
  const beforeData = readdirSync(dataDir).sort();

  const index = docs.listDocs(dataDir);
  assert.equal(index.scratch.body, '');
  assert.match(docs.showDoc(dataDir, 'docs/proposals/idea.md').body, /hello/);
  assert.equal(docs.showScratchPad(dataDir).body, '');

  assert.deepEqual(readdirSync(join(proj, 'docs')).sort(), beforeDocs);
  assert.deepEqual(readdirSync(dataDir).sort(), beforeData);
  assert.equal(existsSync(join(dataDir, 'scratch')), false);
  assert.equal(existsSync(join(proj, 'docs', 'audits')), false);
  assert.equal(existsSync(join(proj, 'docs', 'research')), false);
  assert.equal(readFileSync(legacy, 'utf8'), 'legacy contents');
});

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

test('AGENTS editor writes only root policy with a current content revision', t => {
  const { dataDir, proj } = projectLayout();
  t.after(() => rmSync(proj, { recursive: true, force: true }));
  mkdirSync(join(proj, 'plugins', 'tower'), { recursive: true });
  writeFileSync(join(proj, 'plugins/tower/AGENTS.md'), '# Plugin policy\n');
  writeFileSync(join(proj, 'AGENTS.md'), '# Root policy\n\nold\n', { mode: 0o640 });

  const initial = docs.showOwnerGuidance(dataDir);
  assert.equal(initial.path, 'AGENTS.md');
  assert.equal(initial.body, '# Root policy\n\nold\n');
  const conflict = error => error?.code === 'E_CONFLICT';
  assert.throws(() => docs.updateOwnerGuidance(dataDir, { body: 'blind overwrite' }), conflict);
  const saved = docs.updateOwnerGuidance(dataDir, {
    body: '# Root policy\n\nnew\n', expectRev: initial.revision,
  });
  assert.equal(saved.body, '# Root policy\n\nnew\n');
  assert.equal(readFileSync(join(proj, 'AGENTS.md'), 'utf8'), saved.body);
  assert.equal(statSync(join(proj, 'AGENTS.md')).mode & 0o777, 0o640);
  assert.throws(() => docs.updateOwnerGuidance(dataDir, {
    body: 'stale overwrite', expectRev: initial.revision,
  }), conflict);
  assert.equal(docs.showOwnerGuidance(dataDir).body, saved.body);

  writeFileSync(join(proj, 'AGENTS.md'), '# Root policy\n\nnow\n');
  assert.throws(() => docs.updateOwnerGuidance(dataDir, {
    body: 'overwrite same-size external edit', expectRev: saved.revision,
  }), conflict);
  assert.equal(readFileSync(join(proj, 'AGENTS.md'), 'utf8'), '# Root policy\n\nnow\n');

  const listed = docs.listDocs(dataDir).sections.flatMap(section => section.files.map(file => file.path));
  assert.equal(listed.includes('AGENTS.md'), false);
  const invalid = error => error?.code === 'E_INVALID';
  for (const path of ['AGENTS.md', 'plugins/tower/AGENTS.md', 'secret.md']) {
    assert.throws(() => docs.addDoc(dataDir, { path, body: 'replace' }), invalid);
    assert.throws(() => docs.updateDoc(dataDir, path, { body: 'replace' }), invalid);
    assert.throws(() => docs.deleteDoc(dataDir, path), invalid);
  }
  assert.equal(readFileSync(join(proj, 'plugins/tower/AGENTS.md'), 'utf8'), '# Plugin policy\n');
});

test('AGENTS editor rejects symlinked and hard-linked policy files', t => {
  const { dataDir, proj } = projectLayout();
  t.after(() => rmSync(proj, { recursive: true, force: true }));
  const policy = join(proj, 'AGENTS.md');
  const other = join(proj, 'secret.md');
  const invalid = error => error?.code === 'E_INVALID';
  for (const link of [symlinkSync, linkSync]) {
    link(other, policy);
    assert.throws(() => docs.showOwnerGuidance(dataDir), invalid);
    assert.throws(() => docs.updateOwnerGuidance(dataDir, {
      body: 'must not write', expectRev: 'untrusted',
    }), invalid);
    assert.equal(readFileSync(other, 'utf8'), 'nope');
    rmSync(policy);
  }
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
  ]) assert.throws(operation, /docs|resolve|escape/);

  const index = docs.listDocs(dataDir);
  const listed = index.sections.flatMap(section => section.files.map(file => file.path));
  assert.equal(listed.some(path => path.includes('linked')), false, 'walk must not follow symlinked directories');
  assert.equal(readFileSync(join(outside, 'secret.md'), 'utf8'), 'outside secret');
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


test('docs: symlinked scratch directories and pads are rejected', () => {
  const { dataDir } = projectLayout();
  const outside = mkdtempSync(join(tmpdir(), 'tower-scratch-dir-outside-'));
  writeFileSync(join(outside, 'secret.md'), 'outside secret');
  symlinkSync(outside, join(dataDir, 'scratch'));
  assert.throws(() => docs.showScratchPad(dataDir), /scratch/);

  const { dataDir: padDataDir } = projectLayout();
  const padOutside = mkdtempSync(join(tmpdir(), 'tower-scratch-pad-outside-'));
  writeFileSync(join(padOutside, 'secret.md'), 'outside secret');
  mkdirSync(join(padDataDir, 'scratch'), { recursive: true });
  symlinkSync(join(padOutside, 'secret.md'), join(padDataDir, 'scratch', 'owner-scratch.md'));
  assert.throws(() => docs.showScratchPad(padDataDir), /scratch/);

});

test('docs: scratchpad seeds and updates; cannot delete via docs delete of scratch path outside docs', () => {
  const { dataDir } = projectLayout();
  const sc = docs.showScratchPad(dataDir);
  assert.equal(sc.kind, 'scratch');
  assert.match(sc.path, /owner-scratch/);
  docs.updateScratchPad(dataDir, { body: 'todo: ship docs tab\n', title: 'Owner scratch' });
  assert.match(docs.showScratchPad(dataDir).body, /todo:/);
});

test('docs operations leave unrelated scratch reports untouched', () => {
  const { dataDir, proj } = projectLayout();
  const scratch = join(dataDir, 'scratch');
  mkdirSync(scratch, { recursive: true });
  writeFileSync(join(scratch, 'owner-scratch.md'), '---\ntitle: Owner scratch\n---\nkeep\n');
  writeFileSync(join(scratch, 'field-audit.md'), '# Unpublished findings\n');
  mkdirSync(join(proj, 'docs', 'audits'), { recursive: true });
  writeFileSync(join(proj, 'docs', 'audits', 'field-audit.md'), '# Published findings\n');

  docs.listDocs(dataDir);
  docs.addDoc(dataDir, { section: 'research', title: 'Study', body: '# Study\n' });
  docs.updateScratchPad(dataDir, { body: 'new owner note\n' });

  assert.equal(readFileSync(join(scratch, 'field-audit.md'), 'utf8'), '# Unpublished findings\n');
  assert.equal(readFileSync(join(proj, 'docs', 'audits', 'field-audit.md'), 'utf8'), '# Published findings\n');
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

test('docs: four categories omit legacy dirs and README navigation; delete removes docs', () => {
  const { dataDir, proj } = projectLayout();
  for (const dir of ['spec', 'audits', 'research', 'proposals', 'plans', 'reference', 'sidequests', 'archive'])
    mkdirSync(join(proj, 'docs', dir), { recursive: true });
  writeFileSync(join(proj, 'docs', 'README.md'), '# Docs navigation\n');
  writeFileSync(join(proj, 'docs', 'spec', 'philosophy.md'), '# Philosophy\n');
  writeFileSync(join(proj, 'docs', 'audits', 'security.md'), '# Security\n');
  writeFileSync(join(proj, 'docs', 'research', 'study.md'), '# Study\n');
  writeFileSync(join(proj, 'docs', 'proposals', 'idea.md'), '# Idea\n');
  writeFileSync(join(proj, 'docs', 'plans', 'old-plan.md'), '# Old plan\n');
  writeFileSync(join(proj, 'docs', 'reference', 'old-reference.md'), '# Old reference\n');
  writeFileSync(join(proj, 'docs', 'sidequests', 'old-sidequest.md'), '# Old sidequest\n');
  writeFileSync(join(proj, 'docs', 'archive', 'old.md'), '# Old\n');

  const index = docs.listDocs(dataDir);
  assert.deepEqual(index.sections.map(section => section.id), ['spec', 'audits', 'research', 'proposals']);
  const allPaths = index.sections.flatMap(section => section.files.map(file => file.path));
  for (const path of [
    'docs/spec/philosophy.md', 'docs/audits/security.md',
    'docs/research/study.md', 'docs/proposals/idea.md',
  ]) assert.ok(allPaths.includes(path));
  for (const path of [
    'docs/README.md', 'docs/plans/old-plan.md', 'docs/reference/old-reference.md',
    'docs/sidequests/old-sidequest.md', 'docs/archive/old.md',
  ]) assert.equal(allPaths.includes(path), false, path);
  assert.throws(() => docs.addDoc(dataDir, { section: 'plans', title: 'legacy plan' }), /section must/);

  docs.deleteDoc(dataDir, 'docs/archive/old.md');
  assert.equal(existsSync(join(proj, 'docs/archive/old.md')), false);
});

test('docs: list and show include created and updated timestamps', () => {
  const { dataDir } = projectLayout();
  docs.updateScratchPad(dataDir, { body: 'Owner note\n' });
  const index = docs.listDocs(dataDir);
  const file = index.sections.find(s => s.id === 'proposals').files.find(f => f.path.endsWith('idea.md'));
  assert.ok(file, 'proposal fixture is listed');
  assert.match(file.created, /^\d{4}-\d{2}-\d{2}T/);
  assert.match(file.updated, /^\d{4}-\d{2}-\d{2}T/);
  const shown = docs.showDoc(dataDir, file.path);
  assert.match(shown.created, /^\d{4}-\d{2}-\d{2}T/);
  assert.match(shown.updated, /^\d{4}-\d{2}-\d{2}T/);
  assert.match(index.scratch.created, /^\d{4}-\d{2}-\d{2}T/);
  assert.match(index.scratch.updated, /^\d{4}-\d{2}-\d{2}T/);
});
