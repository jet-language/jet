import { basename, join, resolve } from 'node:path';
import { withLock } from './lock.mjs';
import { MAX_JSON_BYTES, withDirectoryAuthority } from './paths.mjs';

const SCHEMA = 'tower.repair-transaction/v1';
const journalFile = (dataDir) => join(dataDir, 'backups', 'repair-transaction.json');

function invalid(message) {
  const error = new Error(message);
  error.code = 'E_INVALID';
  throw error;
}

function safeRegular(stat) {
  return !!stat?.isFile() && stat.nlink === 1;
}

const JOURNAL = 'repair-transaction.json';

function backupName(name, prefix) {
  if (typeof name !== 'string' || basename(name) !== name || !name.startsWith(prefix) || !name.endsWith('.json'))
    invalid(`tower: invalid ${prefix.slice(0, -1)} repair backup in transaction journal`);
  return name;
}

function withBackups(dataDir, operation, heldRoot = null, heldBackups = null) {
  const dir = join(dataDir, 'backups');
  const run = root => {
    if (resolve(dataDir) !== root.expectedPath)
      invalid(`tower: repair authority does not contain ${dataDir}`);
    let backups = heldBackups;
    const ownsBackups = !backups;
    try {
      if (!backups) {
        try { backups = root.child('backups'); }
        catch (error) {
          if (error.code === 'ENOENT') {
            const missing = new Error(`tower: repair backup directory is missing: ${dir}`);
            missing.code = 'ENOENT';
            throw missing;
          }
          throw error;
        }
      }
      if (backups.expectedPath !== resolve(dir))
        invalid(`tower: repair backup authority does not contain ${dir}`);
      root.guard('open repair backups');
      backups.guard('open repair backups');
      return operation(root, backups, dir);
    } finally {
      if (ownsBackups) backups?.close();
    }
  };
  return heldRoot ? run(heldRoot) : withDirectoryAuthority(dataDir, run);
}

function checkedJournal(dataDir) {
  const file = journalFile(dataDir);
  try {
    return withBackups(dataDir, (_root, backups) => {
      const stat = backups.tryStat(JOURNAL);
      if (!stat) return null;
      if (!safeRegular(stat)) invalid(`tower: repair transaction journal is unsafe: ${file}`);
      return file;
    });
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

// Read the marker through an already-held data-directory descriptor. Pair
// readers use this before and after both JSON reads so a completed repair is
// never mistaken for a stable pair while its journal is still present.
export function hasPendingRepairAt(root, heldBackups = null) {
  try {
    return withBackups(root.expectedPath, (_root, backups) => {
      const stat = backups.tryStat(JOURNAL);
      if (!stat) return false;
      if (!safeRegular(stat))
        invalid(`tower: repair transaction journal is unsafe: ${join(root.expectedPath, 'backups', JOURNAL)}`);
      return true;
    }, root, heldBackups);
  } catch (error) {
    if (error.code === 'ENOENT') return false;
    throw error;
  }
}

export const hasPendingRepair = (dataDir) => checkedJournal(dataDir) !== null;

export function beginRepairTransaction(dataDir, { liveBackup, historyBackup, manifestHash }, heldRoot = null, heldBackups = null) {
  withBackups(dataDir, (_root, backups) => {
    backups.writeAtomic(JOURNAL, JSON.stringify({
      schema: SCHEMA,
      manifestHash,
      liveBackup: backupName(basename(liveBackup), 'tower-'),
      historyBackup: backupName(basename(historyBackup), 'history-'),
    }, null, 2) + '\n');
    backups.sync();
  }, heldRoot, heldBackups);
}

export function finishRepairTransaction(dataDir, heldRoot = null, heldBackups = null) {
  const file = journalFile(dataDir);
  try {
    withBackups(dataDir, (_root, backups) => {
      const stat = backups.tryStat(JOURNAL);
      if (!stat) return;
      if (!safeRegular(stat)) invalid(`tower: repair transaction journal is unsafe: ${file}`);
      backups.remove(JOURNAL, stat);
      backups.sync();
    }, heldRoot, heldBackups);
  } catch (error) {
    if (error.code === 'ENOENT') return;
    throw error;
  }
}

// Caller owns tower.json's write lock. Recovery always rolls both stores back
// to their mandatory pre-repair backups. Journal deletion is the commit point.
export function recoverPendingRepairLocked(dataDir, heldRoot = null, heldBackups = null) {
  const file = journalFile(dataDir);
  try {
    return withBackups(dataDir, (root, backups) => {
      const journalStat = backups.tryStat(JOURNAL);
      if (!journalStat) return false;
      if (!safeRegular(journalStat)) invalid(`tower: repair transaction journal is unsafe: ${file}`);
      let journal;
      try { journal = JSON.parse(backups.read(JOURNAL, MAX_JSON_BYTES, `tower: repair transaction journal is unsafe: ${file}`).toString('utf8')); }
      catch (error) { if (error.code === 'ENOENT') return false; throw error; }
      if (!journal || typeof journal !== 'object' || journal.schema !== SCHEMA)
        throw new Error(`tower: unknown repair transaction journal schema at ${file}`);
      const liveName = backupName(journal.liveBackup, 'tower-');
      const historyName = backupName(journal.historyBackup, 'history-');
      const liveBackup = backups.read(liveName, MAX_JSON_BYTES, `tower: repair backup is unsafe: ${join(join(dataDir, 'backups'), liveName)}`);
      const historyBackup = backups.read(historyName, MAX_JSON_BYTES, `tower: repair backup is unsafe: ${join(join(dataDir, 'backups'), historyName)}`);
      root.writeAtomic('tower.json', liveBackup);
      root.writeAtomic('history.json', historyBackup);
      root.sync();
      backups.remove(JOURNAL, journalStat);
      backups.sync();
      root.sync();
      return true;
    }, heldRoot, heldBackups);
  } catch (error) {
    if (error.code === 'ENOENT') return false;
    throw error;
  }
}

export function recoverPendingRepair(dataDir, liveFile) {
  if (!hasPendingRepair(dataDir)) return false;
  return withLock(liveFile, () => recoverPendingRepairLocked(dataDir));
}
