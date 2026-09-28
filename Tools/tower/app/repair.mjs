import { createHash, randomBytes } from 'node:crypto';
import {
  closeSync, constants, fstatSync, fsyncSync, openSync, renameSync, writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { withLock } from './lock.mjs';
import {
  MAX_JSON_BYTES, backupRequiredAt, now, syncDir, withDirectoryAuthority,
} from './paths.mjs';
import { TowerError, validateStoredHistory, validateStoredState } from './store.mjs';
import {
  beginRepairTransaction, finishRepairTransaction, recoverPendingRepairLocked,
} from './repair-journal.mjs';

const SCHEMA = 'tower.repair-manifest/v1';
const CANONICALIZATION = 'Recursive lexicographic object-key ordering; array order preserved; compact UTF-8 JSON; sha256 covers payload only.';
const STORES = new Set(['tower.json', 'history.json']);
const COLLECTIONS = new Set(['cards', 'decisions', 'events', 'ideas', 'questions']);
const PROTECTED_LEAVES = new Set([
  'id', 'key', 'num', 'n', 'at', 'by', 'ref', 'action', 'cardId', 'milestoneId',
  'created', 'updated', 'ratifiedAt', 'answeredAt', 'retiredAt', 'workOrder',
  'order', 'seq', 'occurrence',
]);
const PROTECTED_ROOTS = {
  cards: new Set(['id', 'num', 'created', 'updated', 'workOrder']),
  decisions: new Set(['id', 'cardId', 'created', 'ratifiedAt']),
  events: new Set(['at', 'by', 'action', 'ref']),
  ideas: new Set(['id', 'created', 'updated']),
  questions: new Set(['id', 'cardId', 'created', 'answeredAt']),
};

const fail = (code, message) => { throw new TowerError(code, message); };
const plainObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (!plainObject(value)) return value;
  return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonicalize(value[key])]));
}

export function canonicalPayloadHash(payload) {
  return createHash('sha256').update(JSON.stringify(canonicalize(payload)), 'utf8').digest('hex');
}

function countChanges(current, replacement) {
  const before = [...current];
  const after = [...replacement];
  if (before.length !== after.length) return null;
  return before.reduce((count, char, index) => count + (char === after[index] ? 0 : 1), 0);
}

function tally(patches) {
  const counts = { fields: patches.length, substitutions: 0, byCollection: {}, byStore: {} };
  for (const patch of patches) {
    counts.substitutions += patch.substitutions;
    for (const [bag, key] of [[counts.byCollection, patch.collection], [counts.byStore, patch.store]]) {
      bag[key] ||= { fields: 0, substitutions: 0 };
      bag[key].fields += 1;
      bag[key].substitutions += patch.substitutions;
    }
  }
  return counts;
}

function sameCanonical(left, right) {
  return JSON.stringify(canonicalize(left)) === JSON.stringify(canonicalize(right));
}

function validateManifest(manifest) {
  if (!plainObject(manifest) || manifest.schema !== SCHEMA
    || manifest.canonicalization !== CANONICALIZATION || !plainObject(manifest.payload))
    fail('E_MANIFEST', `manifest must use schema ${SCHEMA}`);
  if (!/^[a-f0-9]{64}$/.test(manifest.sha256 || ''))
    fail('E_MANIFEST_HASH', 'manifest sha256 must be 64 lowercase hexadecimal characters');
  const hash = canonicalPayloadHash(manifest.payload);
  if (hash !== manifest.sha256)
    fail('E_MANIFEST_HASH', `payload hash mismatch: manifest says ${manifest.sha256}, computed ${hash}`);

  const { expectedRev, revPath, counts, patches } = manifest.payload;
  if (!Number.isInteger(expectedRev) || expectedRev < 0 || revPath !== 'tower.json#/meta/rev')
    fail('E_MANIFEST', 'payload must name a non-negative expectedRev at tower.json#/meta/rev');
  if (!Array.isArray(patches) || patches.length === 0)
    fail('E_MANIFEST', 'payload patches must be a non-empty array');

  const targets = new Set();
  for (const [index, patch] of patches.entries()) {
    const label = `patch ${index + 1}`;
    if (!plainObject(patch) || !STORES.has(patch.store) || !COLLECTIONS.has(patch.collection))
      fail('E_MANIFEST', `${label} has an unsupported store or collection`);
    if (typeof patch.path !== 'string' || !patch.path.startsWith('/') || patch.path === '/')
      fail('E_MANIFEST', `${label} path must be a non-root JSON pointer`);
    const segments = pointerSegments(patch.path, label);
    const leaf = segments.at(-1);
    if (PROTECTED_ROOTS[patch.collection].has(segments[0])
      || PROTECTED_LEAVES.has(leaf) || /(?:Id|At)$/u.test(leaf))
      fail('E_MANIFEST', `${label} cannot repair protected identity, key, timestamp, or ordering leaf ${JSON.stringify(leaf)}`);
    if (patch.collection === 'decisions' && leaf === 'status' && patch.replacement === 'ratified')
      fail('E_MANIFEST', `${label} cannot ratify a decision through repair; use the owner decision flow`);
    if (typeof patch.current !== 'string' || typeof patch.replacement !== 'string'
      || !Number.isInteger(patch.substitutions) || patch.substitutions < 1)
      fail('E_MANIFEST', `${label} must carry string current/replacement leaves and a positive substitution count`);
    if (countChanges(patch.current, patch.replacement) !== patch.substitutions)
      fail('E_MANIFEST_COUNTS', `${label} substitution count does not match its string changes`);

    const keyNames = plainObject(patch.key) ? Object.keys(patch.key).sort() : [];
    if (patch.collection === 'events') {
      if (!sameCanonical(keyNames, ['action', 'at', 'by', 'occurrence', 'ref'])
        || typeof patch.key.at !== 'string' || typeof patch.key.by !== 'string'
        || typeof patch.key.action !== 'string'
        || !(typeof patch.key.ref === 'string' || patch.key.ref === null)
        || !Number.isInteger(patch.key.occurrence) || patch.key.occurrence < 0)
        fail('E_MANIFEST', `${label} event key must be {at,by,action,ref,occurrence}`);
    } else if (!sameCanonical(keyNames, ['id']) || typeof patch.key.id !== 'string' || !patch.key.id) {
      fail('E_MANIFEST', `${label} object key must be one stable id`);
    }
    const target = JSON.stringify(canonicalize([patch.store, patch.collection, patch.key, patch.path]));
    if (targets.has(target)) fail('E_MANIFEST', `${label} duplicates a repair target`);
    targets.add(target);
  }

  const actualCounts = tally(patches);
  if (!plainObject(counts) || !sameCanonical(counts, actualCounts))
    fail('E_MANIFEST_COUNTS', 'manifest aggregate field/substitution counts do not match patches');
  return { hash, expectedRev, counts, patches };
}

function pointerSegments(pointer, label) {
  return pointer.slice(1).split('/').map(segment => {
    if (/~(?![01])/u.test(segment)) fail('E_MANIFEST', `${label} has an invalid JSON pointer escape`);
    return segment.replace(/~1/g, '/').replace(/~0/g, '~');
  });
}

function findTarget(store, patch, label) {
  const collection = store[patch.collection];
  if (!Array.isArray(collection))
    fail('E_REPAIR_DRIFT', `${label}: ${patch.store}#/${patch.collection} is not an array`);
  if (patch.collection !== 'events') {
    const matches = collection.filter(item => plainObject(item) && item.id === patch.key.id);
    if (matches.length !== 1)
      fail('E_REPAIR_DRIFT', `${label}: expected exactly one ${patch.collection} object with id ${patch.key.id}, found ${matches.length}`);
    return matches[0];
  }
  const { occurrence, ...key } = patch.key;
  const matches = collection.filter(item => plainObject(item)
    && Object.entries(key).every(([field, value]) => item[field] === value));
  if (occurrence >= matches.length)
    fail('E_REPAIR_DRIFT', `${label}: event key occurrence ${occurrence} not found`);
  return matches[occurrence];
}

function resolveLeaf(target, pointer, label) {
  const segments = pointerSegments(pointer, label);
  let parent = target;
  for (const segment of segments.slice(0, -1)) {
    if ((plainObject(parent) || Array.isArray(parent)) && Object.hasOwn(parent, segment)) parent = parent[segment];
    else fail('E_REPAIR_DRIFT', `${label}: path ${pointer} does not exist`);
  }
  const leaf = segments.at(-1);
  if (!(plainObject(parent) || Array.isArray(parent)) || !Object.hasOwn(parent, leaf))
    fail('E_REPAIR_DRIFT', `${label}: path ${pointer} does not exist`);
  return { parent, leaf };
}

function stagePatches(stores, patches) {
  for (const [index, patch] of patches.entries()) {
    const label = `patch ${index + 1} (${patch.store}#/${patch.collection}${patch.path})`;
    const target = findTarget(stores[patch.store], patch, label);
    if (patch.collection === 'decisions' && target.status === 'ratified')
      fail('E_MANIFEST', `${label}: ratified decisions are immutable; record a superseding decision`);
    const { parent, leaf } = resolveLeaf(target, patch.path, label);
    if (typeof parent[leaf] !== 'string' || parent[leaf] !== patch.current)
      fail('E_REPAIR_DRIFT', `${label}: current leaf does not match manifest`);
    parent[leaf] = patch.replacement;
  }
}

const STAGE_FLAGS = constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL
  | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0);

function safeStage(stat) {
  return !!stat?.isFile() && stat.nlink === 1;
}

function sameIdentity(left, right) {
  return !!left && !!right && left.dev === right.dev && left.ino === right.ino;
}

function removeStageIfIdentity(authority, name, expected) {
  try { authority.remove(name, expected); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
}

function writeStage(authority, name, value) {
  const file = join(authority.expectedPath, name);
  const contents = JSON.stringify(value, null, 2) + '\n';
  let fd;
  let stat;
  try {
    fd = openSync(authority.path(name), STAGE_FLAGS, 0o600);
    writeFileSync(fd, contents);
    stat = fstatSync(fd);
    if (!safeStage(stat)) fail('E_REPAIR_IO', `repair stage is unsafe: ${file}`);
    fsyncSync(fd);
    return { fd, stat };
  } catch (error) {
    if (fd !== undefined) {
      if (!stat) try { stat = fstatSync(fd); } catch { /* best effort */ }
      try { closeSync(fd); } catch { /* best effort */ }
    }
    if (stat) try { removeStageIfIdentity(authority, name, stat); } catch { /* best effort */ }
    throw error;
  }
}

function removeUnexpectedStageDestination(authority, name, expected) {
  try { authority.removeUnexpected(name, expected); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
}

function verifyStageDestination(authority, name, expected) {
  const file = join(authority.expectedPath, name);
  let current;
  try { current = authority.stat(name); }
  catch (error) { fail('E_REPAIR_IO', `repair destination disappeared: ${file}: ${error.message}`); }
  if (sameIdentity(current, expected) && safeStage(current)) return;
  try { removeUnexpectedStageDestination(authority, name, expected); } catch { /* preserve the repair failure */ }
  fail('E_REPAIR_IO', `repair destination changed during replacement: ${file}`);
}

export function commitRepairPair({
  dataDir, liveFile, historyFile, live, history, originalLive, originalHistory,
  liveBackup, historyBackup, manifestHash, rename = renameSync,
  syncParent = syncDir, finishTransaction = finishRepairTransaction,
  heldRoot = null, heldBackups = null,
}) {
  const stageSuffix = randomBytes(16).toString('hex');
  const liveName = 'tower.json';
  const historyName = 'history.json';
  const liveStageName = `${liveName}.repair-stage-${stageSuffix}`;
  const historyStageName = `${historyName}.repair-stage-${stageSuffix}`;
  const liveStage = join(dataDir, liveStageName);
  const historyStage = join(dataDir, historyStageName);
  const commit = root => {
    let backupAuthority = heldBackups;
    const ownsBackupAuthority = !backupAuthority;
    try {
      // Keep the backup directory pinned through staging, journal creation,
      // both renames, rollback, and journal removal.
      if (!backupAuthority) backupAuthority = root.child('backups');
      root.guard('repair transaction');
      backupAuthority.guard('repair transaction');

      let liveHandle;
      let historyHandle;
      let liveStageStat;
      let historyStageStat;
      let liveCommitted = false;
      let historyCommitted = false;
      let journalBegun = false;
      const descriptorRename = rename === renameSync;
      try {
        // Capture each identity before close. The post-rename check therefore
        // detects a fresh regular file as well as symlink/hardlink/special swaps.
        liveHandle = writeStage(root, liveStageName, live);
        liveStageStat = liveHandle.stat;
        historyHandle = writeStage(root, historyStageName, history);
        historyStageStat = historyHandle.stat;
        beginRepairTransaction(dataDir, { liveBackup, historyBackup, manifestHash }, root, backupAuthority);
        journalBegun = true;
        closeSync(liveHandle.fd);
        liveHandle = null;
        closeSync(historyHandle.fd);
        historyHandle = null;
        // A descriptor-relative rename can fail after replacing the destination
        // (for example when the source is swapped between the precheck and the
        // rename). Mark it committed before the call so rollback restores the
        // original even when the helper reports that post-rename validation saw
        // an attacker entry.
        liveCommitted = true;
        if (descriptorRename) root.rename(liveStageName, liveName, liveStageStat);
        else rename(liveStage, liveFile);
        verifyStageDestination(root, liveName, liveStageStat);
        historyCommitted = true;
        if (descriptorRename) root.rename(historyStageName, historyName, historyStageStat);
        else rename(historyStage, historyFile);
        verifyStageDestination(root, historyName, historyStageStat);
      } catch (error) {
        try {
          if (liveHandle) closeSync(liveHandle.fd);
          if (historyHandle) closeSync(historyHandle.fd);
          if (liveCommitted) root.writeAtomic(liveName, originalLive);
          if (historyCommitted) root.writeAtomic(historyName, originalHistory);
          if (journalBegun) finishTransaction(dataDir, root, backupAuthority);
        } catch (rollbackError) {
          fail('E_REPAIR_IO', `repair commit failed and rollback failed: ${error.message}; ${rollbackError.message}`);
        } finally {
          for (const [stage, handle, expected] of [
            [liveStageName, liveHandle, liveStageStat],
            [historyStageName, historyHandle, historyStageStat],
          ]) {
            const identity = handle?.stat || expected;
            if (identity) try { removeStageIfIdentity(root, stage, identity); } catch { /* best effort after rollback */ }
          }
        }
        fail('E_REPAIR_IO', `repair commit failed; both stores rolled back: ${error.message}`);
      }

      // Both file contents and directory entries must reach disk before deleting
      // the durable rollback marker. Journal removal is the commit point.
      if (syncParent === syncDir) root.sync();
      else syncParent(dataDir);
      finishTransaction(dataDir, root, backupAuthority);
    } finally {
      if (ownsBackupAuthority) backupAuthority?.close();
    }
  };
  return heldRoot ? commit(heldRoot) : withDirectoryAuthority(dataDir, commit);
}

export function applyRepairManifest(dataDir, manifest, {
  expectRev, by, dryRun = false, backups = 20,
} = {}) {
  const checked = validateManifest(manifest);
  if (!/^(?:0|[1-9]\d*)$/u.test(String(expectRev)))
    fail('E_MANIFEST', '--expect-rev is required and must be a non-negative integer');
  const expected = Number(expectRev);
  if (expected !== checked.expectedRev)
    fail('E_CONFLICT', `manifest expects rev ${checked.expectedRev}, but command expects ${expected}`);
  if (typeof by !== 'string' || !by.trim())
    fail('E_MANIFEST', '--by is required for the repair audit event');

  const liveFile = join(dataDir, 'tower.json');
  const historyFile = join(dataDir, 'history.json');
  return withLock(liveFile, () => {
    try {
      recoverPendingRepairLocked(dataDir);
    } catch (error) {
      fail('E_REPAIR_IO', `repair recovery failed: ${error.message}`);
    }
    return withDirectoryAuthority(dataDir, root => {
      const originalLive = root.read('tower.json', MAX_JSON_BYTES, `repair source is unsafe: ${liveFile}`).toString('utf8');
      const originalHistory = root.read('history.json', MAX_JSON_BYTES, `repair source is unsafe: ${historyFile}`).toString('utf8');
      const live = JSON.parse(originalLive);
      const history = JSON.parse(originalHistory);
      if (live.meta?.rev !== expected)
        fail('E_CONFLICT', `stale rev: expected ${expected}, store is at ${live.meta?.rev} — re-read state and retry`);

      stagePatches({ 'tower.json': live, 'history.json': history }, checked.patches);
      validateStoredState(live, 'live store');
      validateStoredHistory(history, 'history store');
      const result = {
        dryRun: !!dryRun,
        manifestHash: checked.hash,
        fields: checked.counts.fields,
        substitutions: checked.counts.substitutions,
        previousRev: expected,
        rev: dryRun ? expected : expected + 1,
      };
      if (dryRun) return result;

      live.meta.rev = expected + 1;
      live.events.unshift({
        at: now(),
        by: by.trim(),
        action: 'repair.apply',
        ref: checked.hash,
        note: `${checked.counts.fields} fields; ${checked.counts.substitutions} substitutions`,
        manifestHash: checked.hash,
        fields: checked.counts.fields,
        substitutions: checked.counts.substitutions,
      });
      const backupAuthority = root.ensureDirectory('backups');
      try {
        const keep = Math.max(1, Number(backups) || 20);
        let liveBackup;
        let historyBackup;
        try {
          liveBackup = backupRequiredAt(root, liveFile, keep, backupAuthority);
          historyBackup = backupRequiredAt(root, historyFile, keep, backupAuthority);
        } catch (error) {
          fail('E_REPAIR_IO', `repair backup failed; stores unchanged: ${error.message}`);
        }
        commitRepairPair({
          dataDir, liveFile, historyFile, live, history, originalLive, originalHistory,
          liveBackup, historyBackup, manifestHash: checked.hash,
          heldRoot: root, heldBackups: backupAuthority,
        });
        return result;
      } finally {
        backupAuthority.close();
      }
    });
  });
}
