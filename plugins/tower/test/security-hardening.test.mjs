import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  existsSync, linkSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync,
  renameSync, symlinkSync, unlinkSync, writeFileSync,
} from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

import * as docs from '../app/docs.mjs';
import { commitRepairPair } from '../app/repair.mjs';
import { empty, emptyHistory, openStore } from '../app/store.mjs';
import {
  backupRequired, backupRequiredAt, historyFile, readFileNoFollow, readJSON, readLatestJSON,
  withDirectoryAuthority, writeJSON, writeTextAtomic,
} from '../app/paths.mjs';
import { beginRepairTransaction, hasPendingRepair } from '../app/repair-journal.mjs';

function project() {
  const root = mkdtempSync(join(tmpdir(), 'tower-security-'));
  const dataDir = join(root, '.tower');
  mkdirSync(join(dataDir, 'backups'), { recursive: true });
  mkdirSync(join(root, 'docs', 'research'), { recursive: true });
  writeJSON(join(dataDir, 'tower.json'), empty('Security test'));
  writeJSON(historyFile(dataDir), emptyHistory());
  writeFileSync(join(root, 'docs', 'research', 'inside.md'), 'inside secret\n');
  return { root, dataDir };
}

function invalid(operation) {
  assert.throws(operation, error => error?.code === 'E_INVALID');
}

test('docs reject hardlinked documents and every available special file', (t) => {
  const { root, dataDir } = project();
  try {
    const original = join(root, 'docs', 'research', 'inside.md');
    const hardlink = join(root, 'docs', 'research', 'hardlink.md');
    linkSync(original, hardlink);
    for (const operation of [
      () => docs.showDoc(dataDir, 'docs/research/hardlink.md'),
      () => docs.addDoc(dataDir, { path: 'docs/research/hardlink.md', body: 'overwrite' }),
      () => docs.updateDoc(dataDir, 'docs/research/hardlink.md', { body: 'overwrite' }),
      () => docs.deleteDoc(dataDir, 'docs/research/hardlink.md'),
      () => docs.archiveDoc(dataDir, 'docs/research/hardlink.md'),
    ]) invalid(operation);
    assert.equal(readFileSync(original, 'utf8'), 'inside secret\n');

    const specialPaths = [];
    const docsRoot = join(root, 'docs', 'research');
    const fifo = join(root, 'docs', 'research', 'special-fifo.md');
    if (spawnSync('mkfifo', [fifo]).status === 0) specialPaths.push(fifo);
    const device = join(root, 'docs', 'research', 'special-device.md');
    if (spawnSync('mknod', [device, 'c', '1', '3']).status === 0) specialPaths.push(device);
    if (!specialPaths.length) return t.skip('host cannot create a FIFO or device node');
    for (const special of specialPaths) {
      const rel = `docs/research/${special.slice(docsRoot.length + 1)}`;
      for (const operation of [
        () => docs.showDoc(dataDir, rel),
        () => docs.addDoc(dataDir, { path: rel, body: 'overwrite' }),
        () => docs.updateDoc(dataDir, rel, { body: 'overwrite' }),
        () => docs.deleteDoc(dataDir, rel),
        () => docs.archiveDoc(dataDir, rel),
      ]) invalid(operation);
    }
    const listed = docs.listDocs(dataDir);
    assert.equal(listed.sections.flatMap(section => section.files).some(file => file.path.includes('special-')), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('held docs directories fail closed when moved outside the project root', async () => {
  const { root, dataDir } = project();
  const outside = mkdtempSync(join(tmpdir(), 'tower-security-outside-'));
  const held = join(root, 'docs', 'research');
  const moved = join(outside, 'research');
  renameSync(held, moved);
  try {
    assert.throws(() => docs.showDoc(dataDir, 'docs/research/inside.md'), error =>
      ['E_INVALID', 'E_NOT_FOUND'].includes(error?.code));
  } finally {
    renameSync(moved, held);
  }
  const attacker = spawn(process.execPath, ['-e', `
    const fs = require('node:fs');
    const [held, moved] = process.argv.slice(1);
    const end = Date.now() + 2000;
    while (Date.now() < end) {
      try { fs.renameSync(held, moved); } catch {}
      try { fs.renameSync(moved, held); } catch {}
    }
  `, held, moved], { stdio: 'ignore' });
  const attackerDone = new Promise((resolve, reject) => {
    attacker.once('error', reject);
    attacker.once('close', resolve);
  });
  let failedClosed = 0;
  try {
    for (let i = 0; i < 500; i++) {
      try {
        const result = docs.showDoc(dataDir, 'docs/research/inside.md');
        assert.notEqual(result.body, 'outside secret\n');
      } catch (error) {
        assert.ok(['E_INVALID', 'E_NOT_FOUND'].includes(error.code), error.message);
        failedClosed++;
      }
    }
    assert.ok(failedClosed > 0, 'held-directory relocation must fail closed');
  } finally {
    attacker.kill('SIGKILL');
    await attackerDone;
    if (existsSync(join(moved, 'inside.md'))) {
      assert.equal(readFileSync(join(moved, 'inside.md'), 'utf8'), 'inside secret\n', 'outside file changed');
      if (!existsSync(held)) {
        mkdirSync(join(root, 'docs'), { recursive: true });
        renameSync(moved, held);
      }
    }
    assert.equal(readFileSync(join(held, 'inside.md'), 'utf8'), 'inside secret\n', 'held file changed');
    rmSync(root, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test('atomic writes use unpredictable exclusive temps and refuse link or special destinations', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-atomic-'));
  const outside = mkdtempSync(join(tmpdir(), 'tower-atomic-outside-'));
  try {
    const target = join(dir, 'state.json');
    const secret = join(outside, 'secret.txt');
    writeFileSync(secret, 'keep\n');
    symlinkSync(secret, target);
    invalid(() => writeTextAtomic(target, 'must not follow\n'));
    assert.equal(lstatSync(target).isSymbolicLink(), true);
    assert.equal(readFileSync(secret, 'utf8'), 'keep\n');

    const safe = join(dir, 'safe.json');
    symlinkSync(secret, `${safe}.tmp.${process.pid}`);
    writeTextAtomic(safe, 'safe\n');
    assert.equal(readFileSync(safe, 'utf8'), 'safe\n');
    assert.equal(readFileSync(secret, 'utf8'), 'keep\n');

    const hardlink = join(dir, 'hardlink.json');
    linkSync(secret, hardlink);
    invalid(() => writeTextAtomic(hardlink, 'must not replace a hardlink\n'));
    assert.equal(readFileSync(secret, 'utf8'), 'keep\n');

    const special = join(dir, 'special.json');
    if (spawnSync('mkfifo', [special]).status === 0) {
      invalid(() => writeTextAtomic(special, 'must not open a fifo\n'));
      assert.equal(lstatSync(special).isFIFO(), true);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(outside, { recursive: true, force: true });
  }
});

test('backups reject symlink, hardlink, and special sources or a linked destination directory', () => {
  const { root, dataDir } = project();
  try {
    const source = join(dataDir, 'source.json');
    const outside = join(root, 'outside-secret.txt');
    const outsideDir = join(root, 'outside-backups');
    mkdirSync(outsideDir);
    writeFileSync(outside, 'keep\n');

    writeFileSync(source, 'source\n');
    unlinkSync(source);
    symlinkSync(outside, source);
    invalid(() => backupRequired(source));

    unlinkSync(source);
    linkSync(outside, source);
    invalid(() => backupRequired(source));

    unlinkSync(source);
    const fifo = join(dataDir, 'source-fifo.json');
    if (spawnSync('mkfifo', [fifo]).status === 0) {
      invalid(() => backupRequired(fifo));
      unlinkSync(fifo);
    }

    writeFileSync(source, 'source\n');
    const backupDir = join(dataDir, 'backups');
    const realBackupDir = join(dataDir, 'backups-real');
    renameSync(backupDir, realBackupDir);
    symlinkSync(outsideDir, backupDir);
    invalid(() => backupRequired(source));
    assert.deepEqual([], readdirSync(outsideDir));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('backup and journal operations fail closed after a held backup directory moves', () => {
  const { root, dataDir } = project();
  const outside = join(root, 'outside-backups');
  const backupDir = join(dataDir, 'backups');
  const moved = join(root, 'backups-held');
  const source = join(dataDir, 'source.json');
  mkdirSync(outside);
  writeFileSync(source, 'source\n');
  try {
    withDirectoryAuthority(dataDir, rootAuthority => {
      const backupAuthority = rootAuthority.child('backups');
      renameSync(backupDir, moved);
      symlinkSync(outside, backupDir);
      try {
        invalid(() => backupRequiredAt(rootAuthority, source, 20, backupAuthority));
        invalid(() => beginRepairTransaction(dataDir, {
          liveBackup: join(moved, 'tower-old.json'),
          historyBackup: join(moved, 'history-old.json'),
          manifestHash: 'held-backup-moved',
        }, rootAuthority, backupAuthority));
        assert.deepEqual(readdirSync(outside), []);
      } finally {
        unlinkSync(backupDir);
        renameSync(moved, backupDir);
        backupAuthority.close();
      }
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('live, history, and undo JSON reads are bounded and no-follow', () => {
  const bounded = mkdtempSync(join(tmpdir(), 'tower-bounded-'));
  try {
    const outside = join(bounded, 'outside.json');
    writeFileSync(outside, '{"outside":true}\n');
    const tooLong = join(bounded, 'too-long.json');
    writeFileSync(tooLong, '123456789');
    invalid(() => readFileNoFollow(tooLong, 'bounded probe', 8));
    invalid(() => readJSON(tooLong, null, 8));

    const liveCase = project();
    try {
      const live = join(liveCase.dataDir, 'tower.json');
      unlinkSync(live);
      symlinkSync(outside, live);
      invalid(() => openStore(liveCase.dataDir).load());
      assert.equal(readFileSync(outside, 'utf8'), '{"outside":true}\n');
    } finally {
      rmSync(liveCase.root, { recursive: true, force: true });
    }

    const historyCase = project();
    try {
      const archive = historyFile(historyCase.dataDir);
      unlinkSync(archive);
      symlinkSync(outside, archive);
      invalid(() => openStore(historyCase.dataDir).loadHistory());
      assert.equal(readFileSync(outside, 'utf8'), '{"outside":true}\n');
    } finally {
      rmSync(historyCase.root, { recursive: true, force: true });
    }

    const undoCase = project();
    try {
      symlinkSync(outside, join(undoCase.dataDir, 'backups', 'tower-evil.json'));
      invalid(() => readLatestJSON(join(undoCase.dataDir, 'backups'), 'tower-', null));
      assert.equal(readFileSync(outside, 'utf8'), '{"outside":true}\n');
    } finally {
      rmSync(undoCase.root, { recursive: true, force: true });
    }
  } finally {
    rmSync(bounded, { recursive: true, force: true });
  }
});

test('journal recovery restores one coherent pair after a crash between files', () => {
  for (const phase of ['prepared', 'history', 'both']) {
    const { root, dataDir } = project();
    try {
      const live = join(dataDir, 'tower.json');
      const archive = historyFile(dataDir);
      const beforeLive = readFileSync(live, 'utf8');
      const beforeHistory = readFileSync(archive, 'utf8');
      beginRepairTransaction(dataDir, {
        liveBackup: backupRequired(live),
        historyBackup: backupRequired(archive),
        manifestHash: `crash-${phase}`,
      });
      if (phase !== 'prepared') writeFileSync(archive, '{"split":"history"}\n');
      if (phase === 'both') writeFileSync(live, '{"split":"live"}\n');
      assert.equal(hasPendingRepair(dataDir), true);

      openStore(dataDir).load();
      assert.equal(readFileSync(live, 'utf8'), beforeLive);
      assert.equal(readFileSync(archive, 'utf8'), beforeHistory);
      assert.equal(hasPendingRepair(dataDir), false);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test('journal recovery rejects unsafe journal or backup sources and ignores predictable recovery temps', () => {
  const sourceKinds = ['symlink', 'hardlink'];
  if (spawnSync('mkfifo', [join(tmpdir(), `tower-security-probe-${process.pid}`)]).status === 0) {
    try { unlinkSync(join(tmpdir(), `tower-security-probe-${process.pid}`)); } catch { /* probe cleanup */ }
    sourceKinds.push('special');
  }
  for (const kind of sourceKinds) {
    const { root, dataDir } = project();
    try {
      const live = join(dataDir, 'tower.json');
      const archive = historyFile(dataDir);
      const outside = join(root, 'outside-secret.txt');
      writeFileSync(outside, 'keep\n');
      const liveBackup = backupRequired(live);
      const historyBackup = backupRequired(archive);
      unlinkSync(liveBackup);
      if (kind === 'symlink') symlinkSync(outside, liveBackup);
      else if (kind === 'hardlink') linkSync(outside, liveBackup);
      else assert.equal(spawnSync('mkfifo', [liveBackup]).status, 0);
      beginRepairTransaction(dataDir, {
        liveBackup, historyBackup, manifestHash: `unsafe-${kind}`,
      });
      writeFileSync(live, '{"split":"live"}\n');

      invalid(() => openStore(dataDir).load());
      assert.equal(hasPendingRepair(dataDir), true, `${kind} backup must leave the journal for retry`);
      assert.equal(readFileSync(outside, 'utf8'), 'keep\n');
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }

  const { root, dataDir } = project();
  try {
    const live = join(dataDir, 'tower.json');
    const archive = historyFile(dataDir);
    const outside = join(root, 'outside-secret.txt');
    writeFileSync(outside, 'keep\n');
    const liveBackup = backupRequired(live);
    const historyBackup = backupRequired(archive);
    const predictable = `${live}.repair-recovery-${process.pid}`;
    symlinkSync(outside, predictable);
    beginRepairTransaction(dataDir, { liveBackup, historyBackup, manifestHash: 'random-recovery-temp' });
    writeFileSync(live, '{"split":"live"}\n');
    openStore(dataDir).load();
    assert.equal(lstatSync(predictable).isSymbolicLink(), true);
    assert.equal(readFileSync(outside, 'utf8'), 'keep\n');
    assert.equal(hasPendingRepair(dataDir), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }

  const unsafeJournal = project();
  try {
    const outside = join(unsafeJournal.root, 'journal-secret.txt');
    writeFileSync(outside, 'keep\n');
    symlinkSync(outside, join(unsafeJournal.dataDir, 'backups', 'repair-transaction.json'));
    invalid(() => hasPendingRepair(unsafeJournal.dataDir));
    assert.equal(readFileSync(outside, 'utf8'), 'keep\n');
  } finally {
    rmSync(unsafeJournal.root, { recursive: true, force: true });
  }
});

test('repair pair removes an attacker entry when a closed stage is swapped before rename', () => {
  const { root, dataDir } = project();
  try {
    const liveFile = join(dataDir, 'tower.json');
    const archiveFile = historyFile(dataDir);
    const outside = join(root, 'outside-secret.txt');
    writeFileSync(outside, 'keep\n');
    const originalLive = readFileSync(liveFile, 'utf8');
    const originalHistory = readFileSync(archiveFile, 'utf8');
    const liveBackup = backupRequired(liveFile);
    const historyBackup = backupRequired(archiveFile);
    let swapped = false;
    assert.throws(() => commitRepairPair({
      dataDir,
      liveFile,
      historyFile: archiveFile,
      live: empty('replacement'),
      history: emptyHistory(),
      originalLive,
      originalHistory,
      liveBackup,
      historyBackup,
      manifestHash: 'stage-swap',
      rename: (from, to) => {
        if (!swapped && to === liveFile) {
          renameSync(from, join(dataDir, 'attacker-stage'));
          symlinkSync(outside, from);
          swapped = true;
        }
        renameSync(from, to);
      },
    }), error => error?.code === 'E_REPAIR_IO');
    assert.equal(swapped, true);
    assert.equal(lstatSync(liveFile).isSymbolicLink(), false);
    assert.equal(readFileSync(liveFile, 'utf8'), originalLive);
    assert.equal(readFileSync(outside, 'utf8'), 'keep\n');
    assert.equal(hasPendingRepair(dataDir), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('tracked phase injection is rejected at state ingestion and phase HTML uses an allowlist', () => {
  const liveCase = project();
  try {
    const file = join(liveCase.dataDir, 'tower.json');
    const state = JSON.parse(readFileSync(file, 'utf8'));
    state.cards.push({ id: 'phase-evil', phase: 'ready"><img src=x onerror=alert(1)>', title: 'phase' });
    writeFileSync(file, JSON.stringify(state));
    invalid(() => openStore(liveCase.dataDir).load());
  } finally {
    rmSync(liveCase.root, { recursive: true, force: true });
  }

  const historyCase = project();
  try {
    const file = historyFile(historyCase.dataDir);
    const history = JSON.parse(readFileSync(file, 'utf8'));
    history.cards.push({ id: 'archived-phase-evil', phase: 'done<script>alert(1)</script>' });
    writeFileSync(file, JSON.stringify(history));
    invalid(() => openStore(historyCase.dataDir).loadHistory());
  } finally {
    rmSync(historyCase.root, { recursive: true, force: true });
  }

  const ui = readFileSync(new URL('../app/ui/tower.js', import.meta.url), 'utf8');
  assert.equal((ui.match(/\$\{c\.phase\}/g) || []).length, 0);
  assert.match(ui, /const safePhase =/);
  assert.match(ui, /style="--stage:var\(--s-\$\{esc\(phase\)\}\)"/);
  assert.match(ui, /phaseLabel\(c\.phase\)/);
});

test('stored ordering fields reject hostile values and UI identity fields stay escaped', () => {
  const cases = [
    ['num', state => state.cards.push({ id: 'evil-num', num: '<img src=x onerror=1>', title: 'num' })],
    ['workOrder', state => state.cards.push({ id: 'evil-order', workOrder: '<img src=x onerror=1>', title: 'order' })],
    ['criterion n', state => state.cards.push({
      id: 'evil-criterion-n', title: 'criterion', criteria: [{ n: '<img src=x onerror=1>', status: 'open', text: 'x' }],
    })],
    ['criterion status', state => state.cards.push({
      id: 'evil-criterion-status', title: 'criterion', criteria: [{ n: 1, status: '<img src=x onerror=1>', text: 'x' }],
    })],
  ];
  for (const [label, mutate] of cases) {
    const { root, dataDir } = project();
    try {
      const file = join(dataDir, 'tower.json');
      const state = JSON.parse(readFileSync(file, 'utf8'));
      mutate(state);
      writeFileSync(file, JSON.stringify(state));
      assert.throws(() => openStore(dataDir).load(), error => error?.code === 'E_INVALID', label);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }

  const fractional = project();
  try {
    const file = join(fractional.dataDir, 'tower.json');
    const state = JSON.parse(readFileSync(file, 'utf8'));
    state.cards.push({ id: 'fractional-order', workOrder: 1.5, title: 'order' });
    writeFileSync(file, JSON.stringify(state));
    assert.doesNotThrow(() => openStore(fractional.dataDir).load());
  } finally {
    rmSync(fractional.root, { recursive: true, force: true });
  }

  const ui = readFileSync(new URL('../app/ui/tower.js', import.meta.url), 'utf8');
  assert.match(ui, /const ticket = \(c\) => '#' \+ esc\(c\.num/);
  assert.match(ui, /class="order">\$\{esc\(c\.workOrder\)\}/);
  assert.match(ui, /title="\$\{esc\(ms\.title\)\}"/);
  assert.match(ui, /epochTag\(e\) : r\.id\)\}/);
  assert.match(ui, /critrow__n">#\$\{esc\(it\.n\)\}/);
  assert.match(ui, /critrow__badge--\$\{safeCriterionStatus\(it\.status\)\}/);
  assert.match(ui, /function recordLabel\(ids\) \{[\s\S]*?return `Record · \$\{ids\[0\]\} = \$\{pick\[ids\[0\]\]\}`;/);
  assert.match(ui, /\$\{esc\(recordLabel\(pickedIds\)\)\}/);
  assert.match(ui, /rec\.textContent = recordLabel\(pickedIds\)/);
});

test('token-host report keeps the canonical trust boundary wording', () => {
  const wording = 'Host does not authenticate a request: the no-token server trusts only a loopback socket whose Host is literal loopback and whose X-Forwarded-For is empty or literal loopback; when a token is configured, remote access is allowed only after the configured token matches.';
  for (const report of [
    new URL('../../../docs/audits/security-deep-scan-2026-08-03.md', import.meta.url),
    new URL('../../../docs/audits/security-deep-scan-2026-08-03-full-tower-control-plane.md', import.meta.url),
  ]) assert.match(readFileSync(report, 'utf8'), new RegExp(wording.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
});
