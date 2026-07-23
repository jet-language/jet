import {
  copyFileSync, existsSync, readFileSync, renameSync, unlinkSync,
} from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { withLock } from './lock.mjs';
import { syncDir, syncFile, writeJSON } from './paths.mjs';

const SCHEMA = 'tower.repair-transaction/v1';
const journalFile = (dataDir) => join(dataDir, 'backups', 'repair-transaction.json');

function checkedBackup(dataDir, name, prefix) {
  if (typeof name !== 'string' || basename(name) !== name || !name.startsWith(prefix) || !name.endsWith('.json'))
    throw new Error(`tower: invalid ${prefix.slice(0, -1)} repair backup in transaction journal`);
  const file = join(dataDir, 'backups', name);
  if (!existsSync(file)) throw new Error(`tower: repair backup is missing: ${file}`);
  return file;
}

function restore(file, backup) {
  const tmp = `${file}.repair-recovery-${process.pid}`;
  copyFileSync(backup, tmp);
  syncFile(tmp);
  renameSync(tmp, file);
  syncDir(dirname(file));
}

export const hasPendingRepair = (dataDir) => existsSync(journalFile(dataDir));

export function beginRepairTransaction(dataDir, { liveBackup, historyBackup, manifestHash }) {
  writeJSON(journalFile(dataDir), {
    schema: SCHEMA,
    manifestHash,
    liveBackup: basename(liveBackup),
    historyBackup: basename(historyBackup),
  });
}

export function finishRepairTransaction(dataDir) {
  const file = journalFile(dataDir);
  if (existsSync(file)) {
    unlinkSync(file);
    syncDir(dirname(file));
  }
}

// Caller owns tower.json's write lock. Recovery always rolls both stores back
// to their mandatory pre-repair backups. Journal deletion is the commit point.
export function recoverPendingRepairLocked(dataDir) {
  const file = journalFile(dataDir);
  if (!existsSync(file)) return false;
  const journal = JSON.parse(readFileSync(file, 'utf8'));
  if (journal.schema !== SCHEMA) throw new Error(`tower: unknown repair transaction journal schema at ${file}`);
  const liveBackup = checkedBackup(dataDir, journal.liveBackup, 'tower-');
  const historyBackup = checkedBackup(dataDir, journal.historyBackup, 'history-');
  restore(join(dataDir, 'tower.json'), liveBackup);
  restore(join(dataDir, 'history.json'), historyBackup);
  finishRepairTransaction(dataDir);
  return true;
}

export function recoverPendingRepair(dataDir, liveFile) {
  if (!hasPendingRepair(dataDir)) return false;
  return withLock(liveFile, () => recoverPendingRepairLocked(dataDir));
}
