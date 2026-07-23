// D-TWR-OPS2 + scratch pad tests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import * as db from '../app/store.mjs';
import { ruleBlockerUnpopulated } from '../app/lint.mjs';
import * as scratch from '../app/scratch.mjs';

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

test('scratch: add/show/update/delete round-trip', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-scratch-'));
  const n = scratch.addScratch(dir, { title: 'Hello World', body: '# hi\n\nbody' });
  assert.equal(n.title, 'Hello World');
  assert.ok(existsSync(join(dir, 'scratch', `${n.id}.md`)));
  const shown = scratch.showScratch(dir, n.id);
  assert.match(shown.body, /body/);
  scratch.updateScratch(dir, n.id, { body: 'updated' });
  assert.equal(scratch.showScratch(dir, n.id).body.trim(), 'updated');
  scratch.deleteScratch(dir, n.id);
  assert.equal(scratch.listScratch(dir).length, 0);
});

test('scratch: preview allowlist + path escape rejected', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-scratch-'));
  const root = join(dir, '..'); // dataDir is .tower-like; project root = dirname(dataDir)
  // Our scratch uses dirname(dataDir) as project root — put docs next to dataDir's parent.
  // Structure: /tmp/xxx/.tower as dataDir → project is /tmp/xxx
  const dataDir = join(dir, '.tower');
  mkdirSync(dataDir, { recursive: true });
  mkdirSync(join(dir, 'docs', 'proposals'), { recursive: true });
  writeFileSync(join(dir, 'docs', 'proposals', 'idea.md'), '# Idea\n\nhello');
  writeFileSync(join(dir, 'secret.md'), 'nope');

  const prev = scratch.previewDoc(dataDir, 'docs/proposals/idea.md');
  assert.match(prev.body, /hello/);
  assert.equal(prev.readonly, true);

  assert.throws(() => scratch.previewDoc(dataDir, 'secret.md'), /must be under/);
  assert.throws(() => scratch.previewDoc(dataDir, '../secret.md'), /must be/);
  assert.throws(() => scratch.previewDoc(dataDir, 'docs/proposals/../../secret.md'), /must be/);
});

test('scratch: migrates legacy owner-scratch.md once', () => {
  const dataDir = mkdtempSync(join(tmpdir(), 'tower-scratch-mig-'));
  writeFileSync(join(dataDir, 'owner-scratch.md'), 'legacy contents');
  const n = scratch.migrateOwnerScratch(dataDir);
  assert.ok(n);
  assert.equal(n.id, 'owner-scratch');
  assert.match(scratch.showScratch(dataDir, 'owner-scratch').body, /legacy/);
  assert.equal(scratch.migrateOwnerScratch(dataDir), null); // already has notes
});
