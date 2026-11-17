// Tower store: one JSON data file per host project, one source of truth.
//
// Model (v4):
//   epochs      — major groupings of work
//   milestones  — measurable targets WITHIN an epoch; cards can point at one
//   cards       — the work items; every card computes to exactly ONE lane
//   decisions   — ballot-ready choices blocking a card, owner-only to ratify
//   questions   — owner ⇄ agent notes/questions on a card
//   ideas       — lightweight capture; promotable to cards
//   briefings   — attributed markdown reports, newest first
//   statusSnapshot — latest posted progress report (not a card/closure gate)
//   events      — append-only audit trail of every mutation
//
// Lane state is DERIVED on every read (never stored), so a card and its
// decisions can never desync. Only one lane ever blocks the owner: `decide`.
// Everything else is an agent's, inert, or done. #516: there is no
// greenlight/activate gate — a fresh card lands straight in an agent lane;
// the owner's only confirmation mechanism is ratifying a decision ballot.
import { basename, join } from 'node:path';
import {
  backupBytesAt, backupRequiredAt, dataFile, historyFile, projectRoot,
  withDirectoryAuthority, newId, today, now,
} from './paths.mjs';
import {
  fileKey, historyKeyAt, historyShardPaths, readHistoryAt, tornMessage, writeHistoryAt,
} from './history-shards.mjs';
import { withLock, withLockAsync } from './lock.mjs';
import { DEFAULTS, loadConfig, publicConfig } from './config.mjs';
import { CLOSURE_KINDS, SCRATCH_PATH_RE, createCommitResolver } from './durable.mjs';
import { checkBoard, commitBoard, pushBoardVerified } from './board-git.mjs';
import { migrateMilestones, migrateRecommendationLosses, migrateRetiredPlanning, migrateReviewPasses } from './migrate.mjs';
import { checkProof, migrateCriterionProof, proofInput } from './proof.mjs';
import { chainRank, criteriaCounts, epochChain, epochLabel, proofCommand, sortEpochs, sortMilestones } from './UI/board-state.js';
import { machinePersonalInfo, personalInfoAdded, redactBoard, tallyValue } from './personal-info.mjs';
import {
  HardeningInputError, hardeningFixtureIssue, hasHardeningPayload,
  formatHardeningEvidence, normalizeHardeningFixture, prepareHardening,
} from './hardening.mjs';
import {
  beginRepairTransaction, finishRepairTransaction, hasPendingRepairAt,
  recoverPendingRepair, recoverPendingRepairLocked,
} from './repair-journal.mjs';

export const VERSION = 4;
export const CLAIM_TTL_MS = 24 * 60 * 60 * 1000;

export const PHASES = [
  { id: 'deciding', label: 'Deciding', seq: 0, who: 'owner', blurb: 'Blocked on a decision' },
  { id: 'planning', label: 'Planning', seq: 1, who: 'agent', blurb: 'Build a plan + raise the decisions it needs' },
  { id: 'ready',    label: 'Ready',    seq: 2, who: 'agent', blurb: 'Plan vetted, decisions cleared — implement it' },
  { id: 'building', label: 'Building', seq: 3, who: 'agent', blurb: 'Implementation in progress' },
  { id: 'verify',   label: 'Review',   seq: 4, who: 'agent', blurb: 'Owner visual review for flagged cards; no separate agent verify step' },
  { id: 'done',     label: 'Done',     seq: 5, who: null,    blurb: 'Closed — exit criteria met' },
  { id: 'frozen',   label: 'Frozen',   seq: -1, who: 'owner', blurb: 'Owner-only — paused; the owner unpauses with a phase update' },
];
export const PHASE_IDS = PHASES.map(p => p.id);
// `triage` is a read-only legacy spelling still present in older boards. It
// remains accepted as data, but no other phase value may enter a projection.
const STORED_PHASE_IDS = new Set([...PHASE_IDS, 'triage']);
export const ACTIVE = ['deciding', 'planning', 'ready', 'building', 'verify'];

export const LANES = {
  decide:    { who: 'owner', label: 'Decide',    rank: 0 },
  plan:      { who: 'agent', label: 'Plan',      rank: 1 },
  implement: { who: 'agent', label: 'Implement', rank: 2 },
  building:  { who: 'agent', label: 'Building',  rank: 3 },
  verify:    { who: 'agent', label: 'Review',    rank: 4 },
  blocked:   { who: null,    label: 'Blocked',   rank: 5 },
  frozen:    { who: 'owner', label: 'Frozen',    rank: 6 },
  done:      { who: null,    label: 'Done',      rank: 7 },
};

export class TowerError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
const fail = (code, msg) => { throw new TowerError(code, msg); };

// Both JSON stores share one journal and one pre-write backup pair. The journal
// stays present until both atomic renames and the containing directory sync
// complete; any error or process crash therefore recovers one coherent pair.
function writeJSONHeld(root, name, value) {
  root.writeAtomic(name, JSON.stringify(value, null, 2) + '\n');
}

// The archive is only ever replaced by atomic renames, so while its files
// and the newest history backup this process made all keep their identity,
// that backup still holds the exact pre-write text. Most writes never touch
// the archive, so they reuse it instead of copying the whole archive again.
let reusableHistoryBackup = null; // { dir, source, name, backup }

// A fresh process (every CLI write) has no identity to compare, but it has
// just read the archive bytes under the lock. When the newest history backup
// holds exactly those bytes it is already the pre-write copy, so the archive
// (the largest file by far) is not copied and synced again on every write.
function identicalNewestBackup(backups, prefix, bytes) {
  const newest = backups.list().filter(name => name.startsWith(prefix)).sort().at(-1);
  if (!newest) return null;
  const st = backups.tryStat(newest);
  if (!st?.isFile() || st.nlink !== 1 || st.size !== bytes.length) return null;
  const held = backups.read(newest, bytes.length, `refusing unsafe history backup ${newest}`);
  return held.equals(bytes) ? newest : null;
}

// The backup holds the archive's single-file text, which a sharded archive
// assembles from its shards (history-shards.mjs), so recovery and undo read
// one file either way.
function historyBackupAt(root, archive, keep, backups, currentBytes = null) {
  const source = historyKeyAt(root);
  const last = reusableHistoryBackup;
  if (last && source && last.dir === root.expectedPath && last.source === source) {
    const st = backups.tryStat(last.name);
    if (st?.isFile() && st.nlink === 1 && fileKey(st) === last.backup)
      return join(root.expectedPath, 'backups', last.name);
  }
  reusableHistoryBackup = null;
  let bytes = currentBytes;
  if (!bytes) {
    const read = readHistoryAt(root, { parse: false });
    if (read.torn) fail('E_INVALID', tornMessage(read.torn));
    if (!read.bytes) throw new Error(`cannot back up missing file ${archive}`);
    bytes = read.bytes;
  }
  const reused = identicalNewestBackup(backups, 'history-', bytes);
  const path = reused ? join(root.expectedPath, 'backups', reused) : backupBytesAt(root, archive, bytes, keep, backups);
  const name = basename(path);
  reusableHistoryBackup = { dir: root.expectedPath, source, name, backup: fileKey(backups.tryStat(name)) };
  return path;
}

function withStorePairTransaction(dataDir, liveFile, config, work, prepare = null) {
  const archive = historyFile(dataDir);
  return withDirectoryAuthority(dataDir, root => {
    // Keep the backup directory pinned for the full pair transaction. A
    // second path lookup between the two backups or journal phases could
    // otherwise follow an attacker-controlled replacement.
    const backupAuthority = root.ensureDirectory('backups');
    try {
      const removeEmptyHistory = !root.tryStat('history.json');
      if (removeEmptyHistory) writeJSONHeld(root, 'history.json', emptyHistory());
      const prepared = prepare ? prepare(root, backupAuthority) : undefined;
      const liveBackup = backupRequiredAt(root, liveFile, config.backups, backupAuthority);
      const historyBackup = historyBackupAt(root, archive, config.backups, backupAuthority, prepared?.historyBytes);
      beginRepairTransaction(dataDir, {
        liveBackup,
        historyBackup,
        historyLayout: prepared?.historyLayout,
        manifestHash: `store:${process.pid}:${Date.now()}`,
      }, root, backupAuthority);
      const result = work(root, prepared);
      root.sync();
      finishRepairTransaction(dataDir, root, backupAuthority);
      if (removeEmptyHistory) {
        let current = emptyHistory();
        try { current = JSON.parse(root.read('history.json').toString('utf8')); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
        if (JSON.stringify(current) === JSON.stringify(emptyHistory())) {
          root.remove('history.json');
          root.sync();
        }
      }
      return result;
    } catch (error) {
      try { recoverPendingRepairLocked(dataDir, root, backupAuthority); } catch { /* leave journal for next reader */ }
      throw error;
    } finally {
      backupAuthority.close();
    }
  });
}

// A board is born with one active epoch: every card must live in an epoch or
// be a sidequest (owner rulings 2026-08-05 and 2026-10-04), so an epoch-less
// board would have nowhere to put epoch-track work.
export const empty = (project = 'Project') => ({
  meta: { version: VERSION, project, currentEpoch: null, nextNum: 1, rev: 0, ui: { toggled: [] } },
  epochs: [{ id: 'e1', name: 'First epoch', goal: '', status: 'active' }],
  milestones: [], cards: [], decisions: [], questions: [], ideas: [], events: [],
  briefings: [], statusSnapshot: null,
});

// ---- store handle ---------------------------------------------------------

export function openStore(dataDir) {
  const file = dataFile(dataDir);
  if (!file) fail('E_NO_DATA', 'no Tower data found — run `tower init` in your project root (or set TOWER_DATA)');
  const config = loadConfig(dataDir);
  // The fixture guard needs the host root, but publicConfig/UI must never see
  // filesystem routing details. Non-enumerable keeps this runtime context out
  // of persisted config and projected JSON.
  Object.defineProperties(config, {
    dataDir: { value: dataDir, enumerable: false },
    projectRoot: { value: projectRoot(dataDir), enumerable: false },
    // #6563 c3: resolves the commits criterion evidence names (durable.mjs).
    durability: { value: createCommitResolver(projectRoot(dataDir), config.durableRefs), enumerable: false },
  });

  // A repair journal is the two-store commit marker. Normal reads stay
  // unlocked, but take two bounded snapshots through one held data-directory
  // descriptor and require both files plus the live revision to remain
  // identical. A completed pair commit therefore cannot return the old live
  // file with the new history (or vice versa).
  recoverPendingRepair(dataDir, file);
  const snapshotAt = (root, name, fallback, label, parse = true) => {
    try {
      const path = `${root.expectedPath}/${name}`;
      const bytes = root.read(name, undefined, `cannot read unsafe ${label}: ${path}`);
      return { bytes, value: parse ? JSON.parse(bytes.toString('utf8')) : null };
    } catch (error) {
      if (error.code === 'ENOENT') return { bytes: null, value: fallback };
      throw error;
    }
  };
  const sameBytes = (left, right) => (left === null && right === null)
    || (!!left && !!right && left.equals(right));
  // The archive as one history (history-shards.mjs): bytes are its
  // single-file text, layout says how it is stored, and torn names a shard
  // that does not match history.json.
  const historySnapshotAt = (root, parse) => {
    const read = readHistoryAt(root, { parse });
    const missing = read.bytes === null && !read.torn;
    return { bytes: read.bytes, value: missing ? emptyHistory() : read.value, layout: read.layout, torn: read.torn };
  };
  // The parsed archive this process last read or committed, keyed by the
  // identity of its files. Every history write is an atomic rename (a new
  // inode), so an unchanged key under the store lock means unchanged bytes. A
  // transaction takes the cached object (fn may mutate it); only a committed
  // mutate puts it back, so a failed or foreign write can never leave a stale
  // copy behind.
  let historyCache = null;
  const pairSnapshotAt = (root, heldBackups = null, parse = true, cachedHistory = null) => {
    const pendingBefore = hasPendingRepairAt(root, heldBackups);
    const live = snapshotAt(root, 'tower.json', empty(config.project), 'live store', parse);
    const history = cachedHistory
      ? { bytes: null, value: cachedHistory.value, layout: cachedHistory.layout, cached: true }
      : historySnapshotAt(root, parse);
    const pendingAfter = hasPendingRepairAt(root, heldBackups);
    root.guard('read board pair');
    return { pending: pendingBefore || pendingAfter, live, history };
  };
  const preparedPairAt = (root, heldBackups = null) => {
    const cached = historyCache;
    historyCache = null;
    const key = historyKeyAt(root);
    const pair = pairSnapshotAt(root, heldBackups, true, cached && key && cached.key === key ? cached : null);
    if (pair.pending) fail('E_CONFLICT', 'repair transaction is still pending; retry the operation');
    if (pair.history.torn) fail('E_INVALID', tornMessage(pair.history.torn));
    validateStoredState(pair.live.value, 'live store');
    const historyLayout = pair.history.layout;
    if (pair.history.cached) {
      // Already validated and migrated when it was read or written.
      const state = normalize(pair.live.value, null, false);
      syncMilestones(state, undefined, pair.history.value.cards);
      return { state, history: pair.history.value, historyLayout, historyMigrated: false, historyCached: true };
    }
    validateStoredHistory(pair.history.value, 'history store');
    const history = { ...emptyHistory(), ...pair.history.value };
    let historyMigrated = migrateRetiredPlanning(history);
    for (const decision of history.decisions)
      historyMigrated = migrateRecommendationLosses(decision) || historyMigrated;
    return {
      state: normalize(pair.live.value, pair.history.value.cards),
      history, historyLayout, historyMigrated, historyBytes: pair.history.bytes,
    };
  };
  let liveCache = null;
  // Every store write is an atomic rename (a new inode), so a pair whose file
  // identities match before and after one snapshot was read whole from one
  // committed version; reading the files a second time to compare bytes
  // only doubled the cost of every CLI read.
  const pairKeyAt = root => `${fileKey(root.tryStat('tower.json'))}|${historyKeyAt(root)}`;
  const readConsistentPair = () => {
    let torn = null;
    for (let attempt = 0; attempt < 8; attempt++) {
      const { first, before, after } = withDirectoryAuthority(dataDir, root => {
        const backupAuthority = root.tryStat('backups') ? root.child('backups') : null;
        try {
          const before = pairKeyAt(root);
          const first = pairSnapshotAt(root, backupAuthority);
          return { first, before, after: pairKeyAt(root) };
        } finally {
          backupAuthority?.close();
        }
      });
      if (first.pending) {
        recoverPendingRepair(dataDir, file);
        continue;
      }
      if (before !== after) continue;
      // A shard that does not match history.json with no write in flight is
      // a mixed checkout, not a race; it is reported once retries run out.
      torn = first.history.torn ?? null;
      if (torn) continue;
      validateStoredState(first.live.value, 'live store');
      validateStoredHistory(first.history.value, 'history store');
      migrateRetiredPlanning(first.history.value);
      for (const decision of first.history.value.decisions || [])
        migrateRecommendationLosses(decision);
      const state = normalize(first.live.value, first.history.value.cards);
      liveCache = { bytes: first.live.bytes, state };
      return {
        state,
        history: { ...emptyHistory(), ...first.history.value },
        historyLayout: first.history.layout,
      };
    }
    if (torn) fail('E_INVALID', tornMessage(torn));
    fail('E_CONFLICT', 'board changed during read; retry the operation');
  };
  const readConsistentLive = () => {
    for (let attempt = 0; attempt < 8; attempt++) {
      const snapshot = withDirectoryAuthority(dataDir, root => {
        const backupAuthority = root.tryStat('backups') ? root.child('backups') : null;
        try {
          const pendingBefore = hasPendingRepairAt(root, backupAuthority);
          const live = snapshotAt(root, 'tower.json', empty(config.project), 'live store', false);
          const pendingAfter = hasPendingRepairAt(root, backupAuthority);
          root.guard('read live board');
          return { pending: pendingBefore || pendingAfter, live };
        } finally {
          backupAuthority?.close();
        }
      });
      if (snapshot.pending) {
        recoverPendingRepair(dataDir, file);
        continue;
      }
      if (liveCache && sameBytes(liveCache.bytes, snapshot.live.bytes)) return liveCache.state;
      const value = snapshot.live.bytes === null
        ? snapshot.live.value
        : JSON.parse(snapshot.live.bytes.toString('utf8'));
      validateStoredState(value, 'live store');
      const state = normalize(value, null, false);
      liveCache = { bytes: snapshot.live.bytes, state };
      return state;
    }
    fail('E_CONFLICT', 'board changed during read; retry the operation');
  };

  const loadPair = () => readConsistentPair();
  const loadLive = () => readConsistentLive();
  const load = () => loadPair().state;

  // history.json can change through another CLI process. Read it fresh so a
  // long-lived server handle cannot retain a pre-repair archive indefinitely.
  const loadHistory = () => loadPair().history;

  // Read-modify-write under the cross-process lock; rev bumps on every write.
  // `expectRev` (optional) enables optimistic concurrency for API callers.
  // `historyLayout: 'shards'` writes the archive sharded from this write on
  // (`tower archive shard --apply`, #6564 c7), in shards of at most
  // `historyShardBytes` (default SHARD_BYTES); otherwise it keeps its layout.
  const mutateLocked = (fn, { expectRev, historyLayout, historyShardBytes } = {}) => {
    recoverPendingRepairLocked(dataDir);
    let committedHistory = null;
    const out = withStorePairTransaction(dataDir, file, config, (root, pair) => {
      const s = pair.state;
      if (expectRev != null && Number(expectRev) !== s.meta.rev)
        fail('E_CONFLICT', `stale rev: expected ${expectRev}, store is at ${s.meta.rev} — re-read state and retry`);
      const history = pair.history;
      // Most writes never look at the archive. fn sees history through a view
      // that snapshots it on first touch, so only a write that touches it pays
      // for the before/after comparison.
      let historyBefore = null;
      const touch = () => { historyBefore ??= JSON.stringify(history); };
      const historyView = new Proxy(history, Object.fromEntries(Reflect.ownKeys(Reflect)
        .filter(k => typeof Reflect[k] === 'function')
        .map(k => [k, (...args) => { touch(); return Reflect[k](...args); }])));
      // #6564 c5: tally personal-info spans before the write so the write may
      // keep (or redact) old ones but never add one.
      const personal = machinePersonalInfo();
      const personalBefore = personal.rules.length ? tallyValue(s, personal) : null;
      stampDayStart(s);
      const result = fn(s, config, historyView);
      const personalIssue = personalBefore && personalInfoAdded(personal, personalBefore, s);
      if (personalIssue) fail('E_PERSONAL_INFO', personalIssue);
      const layout = historyLayout ?? pair.historyLayout;
      let historyWritten = layout !== pair.historyLayout || pair.historyMigrated
        || (historyBefore !== null && JSON.stringify(history) !== historyBefore);
      if (historyWritten) writeHistoryAt(root, history, layout, { bound: historyShardBytes });
      // #461: single chokepoint — every write gets a chance to retire aged-out
      // cards/decisions/events to history.json before tower.json is persisted.
      syncMilestones(s, undefined, history.cards);
      historyWritten = retire(s, config, history, root, layout) || historyWritten;
      validateStoredState(s, 'live store');
      if (historyWritten || !pair.historyCached) validateStoredHistory(history, 'history store');
      s.meta.rev += 1;
      writeJSONHeld(root, 'tower.json', s);
      committedHistory = { key: historyKeyAt(root), value: history, layout };
      return { result, state: s };
    }, preparedPairAt);
    if (committedHistory?.key) historyCache = committedHistory;
    return out;
  };
  // #6563 c4 (board-git.mjs): every boardBatch-th write commits the board pair
  // while the lock still holds it. A failed commit never fails the write; the
  // next batch retries and `tower board check` reports the lag.
  const boardFiles = () => [file, historyFile(dataDir), ...historyShardPaths(dataDir)];
  const commitBoardLocked = (by) => commitBoard(dataDir, boardFiles(), config, { rev: readConsistentLive().meta.rev, by });
  const afterWrite = (out) => {
    const rev = out?.state?.meta?.rev;
    const batch = Number(config.boardBatch);
    if (!batch || !Number.isSafeInteger(rev) || rev % batch !== 0) return out;
    const done = commitBoard(dataDir, boardFiles(), config, { rev });
    if (!done.committed && !/not in a git work tree|not tracked|board is current/.test(done.reason))
      process.stderr.write(`tower: board commit skipped at rev ${rev}: ${done.reason}\n`);
    return out;
  };
  const mutate = (fn, options) => withLock(file, () => afterWrite(mutateLocked(fn, options)));
  // Same write, but waits for the lock without blocking the event loop
  // (`tower serve`), and long enough that busy agents never reject it.
  const mutateAsync = (fn, options) => withLockAsync(file, () => afterWrite(mutateLocked(fn, options)));
  const commitBoardNow = (by) => withLock(file, () => commitBoardLocked(by));
  const checkBoardNow = () => checkBoard(dataDir, boardFiles(), config, readConsistentLive().meta.rev);
  // `tower publish`: commit under the lock, then push and read origin back
  // outside it, so writers never wait on the network.
  const publishBoardNow = (by) => pushBoardVerified(dataDir, boardFiles(), config,
    withLock(file, () => commitBoard(dataDir, boardFiles(), config, { rev: readConsistentLive().meta.rev, by, push: false })));

  // Replace the whole state (undo). Guarded by expectRev so an interleaved
  // write from another agent can never be silently reverted. Undo touches
  // ONLY tower.json — history.json is append-only and never rolled back
  // (see Test/history.test.mjs for the duplicate-tolerance this buys).
  // #1738: expectRev is mandatory — a whole-board replace with no rev proof
  // is exactly the overwrite class that once deleted 112 cards.
  const restore = (prevState, { expectRev, by } = {}) => withLock(file, () => {
    recoverPendingRepairLocked(dataDir);
    const currentPair = readConsistentPair();
    const cur = currentPair.state;
    if (expectRev == null)
      fail('E_USAGE', `restore requires expectRev — read the board first and pass its meta.rev (currently ${cur.meta.rev})`);
    if (Number(expectRev) !== cur.meta.rev)
      fail('E_CONFLICT', `undo refused: board changed since (rev ${cur.meta.rev} ≠ ${expectRev})`);
    return withStorePairTransaction(dataDir, file, config, (root, pair) => {
      if (pair.state.meta.rev !== cur.meta.rev)
        fail('E_CONFLICT', `undo refused: board changed during restore (rev ${pair.state.meta.rev} ≠ ${cur.meta.rev})`);
      const s = normalize(prevState, pair.history.cards);
      s.meta.rev = cur.meta.rev + 1;
      logEvent(s, { by, action: 'board.undo', note: `back to rev ${prevState.meta?.rev ?? '?'} content` });
      writeJSONHeld(root, 'tower.json', s);
      return { result: { restored: true }, state: s };
    }, preparedPairAt);
  });

  // Bring a single archived card or decision back to the live board
  // (D-TWR-ARCHIVE1=B). Resets its clock (updated/ratifiedAt = today) so it
  // doesn't immediately re-retire on the next write.
  const restoreArchived = (ref, by) => withLock(file, () => {
    recoverPendingRepairLocked(dataDir);
    return withStorePairTransaction(dataDir, file, config, (root, pair) => {
      const s = pair.state;
      const h = pair.history;
      const result = restoreFromHistory(s, h, ref, by);
      syncMilestones(s, undefined, h.cards);
      s.meta.rev += 1;
      writeJSONHeld(root, 'tower.json', s);
      writeHistoryAt(root, h, pair.historyLayout);
      return { result, state: s };
    }, preparedPairAt);
  });

  return {
    file, dataDir, config, load, loadLive, loadPair, mutate, mutateAsync, restore, restoreArchived, loadHistory,
    commitBoard: commitBoardNow, checkBoard: checkBoardNow, publishBoard: publishBoardNow,
    project: () => { const pair = loadPair(); return project(pair.state, config, pair.history); },
  };
}

// ---- history: split live/archive store (#461) ------------------------------
// D-TWR-ARCHIVE1=B MODIFIED by owner comment: nothing retires immediately —
// a buffer window (config.retireAfterDays, default 3) lets the owner walk
// back a fresh ratification before it's out of easy reach. Append-only
// ledger at <dataDir>/history.json, written under the SAME lock as
// tower.json (see `mutate`/`restoreArchived` above), committed to git.
export const emptyHistory = () => ({ version: 1, decisions: [], cards: [], events: [] });

const CRITERION_STATUSES = new Set(['open', 'met', 'verified']);
// A milestone is open until its review signs it off as met; a retired one is
// kept for the record and hidden from lists, next, briefings and the radar.
const MILESTONE_STATUSES = new Set(['open', 'met', 'retired']);
const EPOCH_STATUSES = new Set(['active', 'arrived', 'planned', 'done', 'open']);
// A rejected ballot is a draft its reviewer sent back; it never reaches the owner.
const DECISION_STATUSES = new Set(['open', 'ratified', 'rejected']);

const plainObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);

function validateStoredString(value, label, { nullable = true, nonEmpty = false } = {}) {
  if (value === undefined) return;
  if (value === null && nullable) return;
  if (typeof value !== 'string' || (nonEmpty && !value.trim()))
    fail('E_INVALID', `${label} must be ${nullable ? 'a string or null' : 'a string'}`);
}

// Board identifier namespace for captured visual evidence. Ratified decisions
// record it verbatim, so it stays fixed; server.mjs serves these files from
// `${DOCS_DIR}/Proposals/visual-acceptance/media`.
const VISUAL_MEDIA_ROOT = 'docs/proposals/visual-acceptance/media/';
const VISUAL_IMAGE_EXT = /\.(png|jpe?g|webp|gif)$/i;
const VISUAL_VIDEO_EXT = /\.(webm|mp4)$/i;

function visualMediaPath(value, label, extensions) {
  validateStoredString(value, label, { nullable: false, nonEmpty: true });
  if (typeof value !== 'string') return;
  if (value.includes('\\') || value.startsWith('/') || /^[a-z][a-z0-9+.-]*:/i.test(value)
    || /[?#%]/.test(value) || value.split('/').some(part => part === '.' || part === '..')
    || !value.startsWith(VISUAL_MEDIA_ROOT)
    || value.length === VISUAL_MEDIA_ROOT.length || !extensions.test(value))
    fail('E_INVALID', `${label} must be a safe ${VISUAL_MEDIA_ROOT} path`);
}

function normalizeVisualMedia(value, label = 'visualMedia') {
  if (value == null) return [];
  if (!Array.isArray(value)) fail('E_INVALID', `${label} must be an array`);
  return value.map((item, index) => {
    const at = `${label}[${index}]`;
    if (!plainObject(item)) fail('E_INVALID', `${at} must be an object`);
    if (item.kind !== 'image' && item.kind !== 'video') fail('E_INVALID', `${at}.kind must be image or video`);
    visualMediaPath(item.path, `${at}.path`, item.kind === 'image' ? VISUAL_IMAGE_EXT : VISUAL_VIDEO_EXT);
    validateStoredString(item.alt, `${at}.alt`, { nullable: false, nonEmpty: true });
    if (item.kind === 'image' && item.alt.trim().length < 8) fail('E_INVALID', `${at}.alt must meaningfully describe the image`);
    validateStoredString(item.caption, `${at}.caption`, { nullable: false, nonEmpty: true });
    validateStoredString(item.state, `${at}.state`, { nullable: false, nonEmpty: true });
    if (item.poster != null) {
      if (item.kind !== 'video') fail('E_INVALID', `${at}.poster is only valid for video`);
      visualMediaPath(item.poster, `${at}.poster`, VISUAL_IMAGE_EXT);
    }
    return {
      kind: item.kind,
      path: item.path,
      alt: item.alt.trim(),
      caption: item.caption.trim(),
      ...(item.state != null ? { state: item.state.trim() } : {}),
      ...(item.poster != null ? { poster: item.poster } : {}),
    };
  });
}

function validateStoredNumber(value, label, { integer = false, min = null } = {}) {
  if (value === undefined || value === null) return;
  if (typeof value !== 'number' || !Number.isFinite(value) || Math.abs(value) > Number.MAX_SAFE_INTEGER
    || (integer && !Number.isSafeInteger(value)) || (min != null && value < min))
    fail('E_INVALID', `${label} must be a canonical ${integer ? 'integer' : 'finite number'}`);
}

function validateStoredCriterion(item, index, source) {
  const label = `${source} criterion ${index + 1}`;
  if (!plainObject(item)) fail('E_INVALID', `${label} must be an object`);
  validateStoredNumber(item.n, `${label}.n`, { integer: true, min: 1 });
  if (item.status !== undefined && !CRITERION_STATUSES.has(item.status))
    fail('E_INVALID', `${label}.status must be open, met, or verified`);
  validateStoredString(item.text, `${label}.text`, { nullable: false });
  for (const key of ['metBy', 'verifiedBy', 'evidence', 'at'])
    validateStoredString(item[key], `${label}.${key}`);
  if (item.closure !== undefined && !CLOSURE_KINDS.includes(item.closure))
    fail('E_INVALID', `${label}.closure must be one of ${CLOSURE_KINDS.join(', ')}`);
  for (const key of ['movedTo', 'movedFrom']) {
    if (item[key] === undefined) continue;
    if (!plainObject(item[key])) fail('E_INVALID', `${label}.${key} must be an object`);
    validateStoredNumber(item[key].card, `${label}.${key}.card`, { integer: true, min: 1 });
    validateStoredNumber(item[key].n, `${label}.${key}.n`, { integer: true, min: 1 });
  }
  if (item.proof !== undefined) checkProof(item.proof, `${label}.proof`);
  for (const key of ['blockedBy', 'unblockedFrom'])
    if (item[key] !== undefined && !BLOCKER_REF.test(String(item[key])))
      fail('E_INVALID', `${label}.${key} must be '#M', '#M cK' or a decision id`);
}

function validateStoredCriteria(criteria, source, field = 'criteria') {
  if (criteria === undefined) return;
  if (!Array.isArray(criteria)) fail('E_INVALID', `${source}.${field} must be an array`);
  const seen = new Set();
  criteria.forEach((item, index) => {
    validateStoredCriterion(item, index, source);
    if (item.n !== undefined) {
      if (seen.has(item.n)) fail('E_INVALID', `${source}.${field} has duplicate n ${item.n}`);
      seen.add(item.n);
    }
  });
}

function validateStoredEpoch(epoch, index, source) {
  const label = `${source} epoch ${index + 1}`;
  if (!plainObject(epoch)) fail('E_INVALID', `${label} must be an object`);
  validateStoredString(epoch.id, `${label}.id`, { nullable: false, nonEmpty: true });
  validateStoredString(epoch.name, `${label}.name`, { nullable: false });
  validateStoredString(epoch.goal, `${label}.goal`, { nullable: false });
  validateStoredNumber(epoch.num, `${label}.num`, { integer: true, min: 1 });
  validateStoredNumber(epoch.order, `${label}.order`, { integer: true, min: 0 });
  if (epoch.status !== undefined && !EPOCH_STATUSES.has(epoch.status))
    fail('E_INVALID', `${label}.status is not a canonical epoch status`);
  if (epoch.exitCriteria !== undefined) {
    if (!Array.isArray(epoch.exitCriteria) || !epoch.exitCriteria.every(item => typeof item === 'string'))
      fail('E_INVALID', `${label}.exitCriteria must be an array of strings`);
  }
}

function validateStoredMilestone(milestone, index, source) {
  const label = `${source} milestone ${index + 1}`;
  if (!plainObject(milestone)) fail('E_INVALID', `${label} must be an object`);
  validateStoredString(milestone.id, `${label}.id`, { nullable: false, nonEmpty: true });
  validateStoredString(milestone.epochId, `${label}.epochId`, { nullable: false, nonEmpty: true });
  validateStoredString(milestone.title, `${label}.title`, { nullable: false });
  validateStoredString(milestone.goal, `${label}.goal`, { nullable: false });
  validateStoredNumber(milestone.order, `${label}.order`, { integer: true, min: 0 });
  if (milestone.status !== undefined && !MILESTONE_STATUSES.has(milestone.status))
    fail('E_INVALID', `${label}.status is not a canonical milestone status`);
  validateStoredCriteria(milestone.exitCriteria, label, 'exitCriteria');
  if (milestone.retired !== undefined) {
    if (!plainObject(milestone.retired)) fail('E_INVALID', `${label}.retired must be an object`);
    validateStoredString(milestone.retired.reason, `${label}.retired.reason`, { nullable: false, nonEmpty: true });
    validateStoredString(milestone.retired.by, `${label}.retired.by`, { nullable: false, nonEmpty: true });
    validateStoredString(milestone.retired.at, `${label}.retired.at`);
  }
  if (milestone.closeout !== undefined) {
    if (!plainObject(milestone.closeout)) fail('E_INVALID', `${label}.closeout must be an object`);
    validateStoredString(milestone.closeout.sourceCommit, `${label}.closeout.sourceCommit`, { nullable: false, nonEmpty: true });
    validateStoredString(milestone.closeout.by, `${label}.closeout.by`, { nullable: false, nonEmpty: true });
    validateStoredString(milestone.closeout.at, `${label}.closeout.at`, { nullable: false, nonEmpty: true });
  }
}

function validateStoredCard(card, index, source, seenNums, seenIds) {
  const label = `${source} card ${index + 1}`;
  if (!plainObject(card)) fail('E_INVALID', `${label} must be an object`);
  validateStoredString(card.id, `${label}.id`, { nullable: false, nonEmpty: true });
  if (card.id !== undefined) {
    if (seenIds.has(card.id)) fail('E_INVALID', `${source} has duplicate card id ${card.id}`);
    seenIds.add(card.id);
  }
  validateStoredNumber(card.num, `${label}.num`, { integer: true, min: 1 });
  if (card.num !== undefined) {
    if (seenNums.has(card.num)) fail('E_INVALID', `${source} has duplicate card number ${card.num}`);
    seenNums.add(card.num);
  }
  validateStoredNumber(card.workOrder, `${label}.workOrder`, { min: 0 });
  validateStoredPhase(card, source);
  validateStoredString(card.epoch, `${label}.epoch` , { nullable: true, nonEmpty: true });
  validateStoredString(card.milestoneId, `${label}.milestoneId`, { nullable: true, nonEmpty: true });
  validateStoredString(card.probe, `${label}.probe`, { nullable: true, nonEmpty: true });
  normalizeVisualMedia(card.visualMedia, `${label}.visualMedia`);
  validateStoredCriteria(card.criteria, label);
}

function validateStoredDecision(decision, index, source) {
  const label = `${source} decision ${index + 1}`;
  if (!plainObject(decision)) fail('E_INVALID', `${label} must be an object`);
  validateStoredString(decision.id, `${label}.id`, { nullable: false, nonEmpty: true });
  validateStoredString(decision.cardId, `${label}.cardId`, { nullable: true, nonEmpty: true });
  if (decision.status !== undefined && !DECISION_STATUSES.has(decision.status))
    fail('E_INVALID', `${label}.status is not a canonical decision status`);
  if (decision.status === 'rejected' && !decision.draft)
    fail('E_INVALID', `${label} is rejected but not a draft; a rejected ballot never reaches the owner`);
  validateStoredString(decision.draftedBy, `${label}.draftedBy`, { nullable: true, nonEmpty: true });
  validateStoredString(decision.reviewedBy, `${label}.reviewedBy`, { nullable: true, nonEmpty: true });
  if (decision.visualMedia !== undefined) normalizeVisualMedia(decision.visualMedia, `${label}.visualMedia`);
  validateStoredString(decision.situation, `${label}.situation`);
  if (decision.options !== undefined) {
    if (!Array.isArray(decision.options)) fail('E_INVALID', `${label}.options must be an array`);
    for (const [optionIndex, option] of decision.options.entries()) {
      if (!plainObject(option)) fail('E_INVALID', `${label}.options[${optionIndex}] must be an object`);
      validateStoredString(option.key, `${label}.options[${optionIndex}].key`, { nullable: false });
      validateStoredString(option.name, `${label}.options[${optionIndex}].name`, { nullable: false });
    }
  }
}

function validateStoredPhase(card, source) {
  if (!card || !Object.hasOwn(card, 'phase') || card.phase == null) return;
  if (typeof card.phase !== 'string' || !STORED_PHASE_IDS.has(card.phase))
    fail('E_INVALID', `${source} has an invalid phase value`);
}

function validateStoredPhases(cards, source) {
  for (const card of Array.isArray(cards) ? cards : []) validateStoredPhase(card, source);
}

export function validateStoredState(state, source = 'live store') {
  if (!plainObject(state)) fail('E_INVALID', `${source} must be an object`);
  if (state.meta !== undefined && !plainObject(state.meta)) fail('E_INVALID', `${source}.meta must be an object`);
  for (const key of ['nextNum', 'rev']) validateStoredNumber(state[key], `${source}.${key}`, { integer: true, min: key === 'nextNum' ? 1 : 0 });
  if (state.meta) {
    validateStoredNumber(state.meta.nextNum, `${source}.meta.nextNum`, { integer: true, min: 1 });
    validateStoredNumber(state.meta.rev, `${source}.meta.rev`, { integer: true, min: 0 });
  }
  for (const key of ['epochs', 'milestones', 'cards', 'decisions', 'questions', 'ideas', 'events', 'briefings']) {
    if (state[key] !== undefined && !Array.isArray(state[key])) fail('E_INVALID', `${source}.${key} must be an array`);
  }
  const seenNums = new Set();
  const seenIds = new Set();
  (state.epochs || []).forEach((epoch, index) => validateStoredEpoch(epoch, index, source));
  migrateMilestones(state);
  (state.milestones || []).forEach((milestone, index) => validateStoredMilestone(milestone, index, source));
  (state.cards || []).forEach((card, index) => validateStoredCard(card, index, source, seenNums, seenIds));
  (state.decisions || []).forEach((decision, index) => validateStoredDecision(decision, index, source));
  (state.briefings || []).forEach(validateBriefing);
  // The owner-actions report was removed 2026-10-04; a board written before then still carries the key. Drop it on read; the next write persists the board without it.
  if (state.statusSnapshot != null) delete state.statusSnapshot.ownerActions;
  if (state.statusSnapshot != null) validateStatusSnapshot(state.statusSnapshot);
  return state;
}

export function validateStoredHistory(history, source = 'history store') {
  if (!plainObject(history)) fail('E_INVALID', `${source} must be an object`);
  for (const key of ['decisions', 'cards', 'events']) {
    if (history[key] !== undefined && !Array.isArray(history[key])) fail('E_INVALID', `${source}.${key} must be an array`);
  }
  const seenNums = new Set();
  const seenIds = new Set();
  (history.cards || []).forEach((card, index) => validateStoredCard(card, index, source, seenNums, seenIds));
  (history.decisions || []).forEach((decision, index) => validateStoredDecision(decision, index, source));
  return history;
}

// One read of the archive alone (no live-board pair check): the parsed
// history, or null when there is none. Read-only archive views use this;
// writes go through the locked pair transaction. A shard that does not match
// history.json is retried while a write may be in flight, then reported.
export function readHistoryDoc(dataDir) {
  let torn = null;
  for (let attempt = 0; attempt < 40; attempt++) {
    if (attempt) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
    let read;
    try { read = withDirectoryAuthority(dataDir, root => readHistoryAt(root)); }
    catch (error) { if (error.code === 'ENOENT') return null; throw error; }
    if (!read.torn) return read.value;
    torn = read.torn;
  }
  fail('E_INVALID', tornMessage(torn));
}

export function loadHistoryRaw(dataDir) {
  const raw = readHistoryDoc(dataDir);
  if (raw == null) return emptyHistory();
  validateStoredHistory(raw);
  migrateRetiredPlanning(raw);
  for (const decision of raw.decisions || []) migrateRecommendationLosses(decision);
  return { ...emptyHistory(), ...raw };
}

// Treat a 'YYYY-MM-DD' stamp (decisions' ratifiedAt) as UTC midnight and a
// full ISO timestamp (cards' `updated`) as itself; "older than N days" = more
// than N*86400000ms have elapsed since then.
function isOlderThanDays(dateStr, days) {
  if (!dateStr) return false;
  const text = String(dateStr);
  const t = Date.parse(text.length === 10 ? `${text}T00:00:00Z` : text);
  if (Number.isNaN(t)) return false;
  return (Date.now() - t) > days * 86_400_000;
}

function completionTime(card) {
  if (card.completedAt) return card.completedAt;
  return String(card.updated || '').length > 10 ? card.updated : null;
}

function hasUnclearedCompletion(card, cursor) {
  const completedAt = completionTime(card);
  return !!completedAt && (!cursor || completedAt > cursor);
}

const LIVE_EVENTS = 500;
// Archiving rewrites the archive (a single-file history.json whole, a sharded
// one its last events shard), so overflow moves in batches rather than one
// event per write.
const EVENT_ARCHIVE_BATCH = 100;

// The one retirement chokepoint (called only from `mutate`, right after the
// caller's fn() runs, before the write). Idempotent: an id already present
// in history.json is removed from live without a duplicate append — undo
// can reintroduce a stale live copy of something already retired (history is
// never rolled back), and this is how that self-heals on the next write.
function retire(s, config, h, root, layout) {
  const days = config.retireAfterDays ?? 3;
  let cardIds = null, decisionIds = null;
  const hasCard = (id) => (cardIds ||= new Set(h.cards.map(x => x.id))).has(id);
  const hasDecision = (id) => (decisionIds ||= new Set(h.decisions.map(x => x.id))).has(id);
  let dirty = false;

  // (b) done cards aged out: card + ALL its live decisions + questions
  // retire together, regardless of the decisions' own ratifiedAt. An open
  // agent message keeps its card live until the owner marks that message done.
  const messageCardIds = new Set(s.questions
    .filter(q => q.kind === 'message' && q.status === 'open')
    .map(q => q.cardId));
  const retireCardIds = new Set(s.cards
    .filter(c => c.phase === 'done'
      && !messageCardIds.has(c.id)
      && !hasUnclearedCompletion(c, s.meta.completionCursor)
      && isOlderThanDays(c.updated, days))
    .map(c => c.id));
  if (retireCardIds.size) {
    for (const c of s.cards) {
      if (!retireCardIds.has(c.id)) continue;
      const questions = s.questions.filter(q => q.cardId === c.id);
      if (!hasCard(c.id)) { h.cards.push({ ...c, questions, retiredAt: now() }); dirty = true; }
      for (const d of s.decisions.filter(x => x.cardId === c.id)) {
        if (!hasDecision(d.id)) { h.decisions.push({ ...d, retiredAt: now() }); dirty = true; }
      }
    }
    s.cards = s.cards.filter(c => !retireCardIds.has(c.id));
    s.decisions = s.decisions.filter(d => !retireCardIds.has(d.cardId));
    s.questions = s.questions.filter(q => !retireCardIds.has(q.cardId));
  }

  // (a) standalone ratified decisions age out only when their card is gone.
  // A live card keeps its decisions until the card retires, so its view can
  // never become half-archived while a completion or message holds it live.
  const liveCardById = new Map(s.cards.map(c => [c.id, c]));
  const standaloneIds = new Set(s.decisions.filter(d => {
    if (d.status !== 'ratified' || !isOlderThanDays(d.ratifiedAt, days)) return false;
    const c = liveCardById.get(d.cardId);
    return !c;
  }).map(d => d.id));
  if (standaloneIds.size) {
    for (const d of s.decisions) {
      if (standaloneIds.has(d.id) && !hasDecision(d.id)) { h.decisions.push({ ...d, retiredAt: now() }); dirty = true; }
    }
    s.decisions = s.decisions.filter(d => !standaloneIds.has(d.id));
  }

  // (c) events: keep the newest 500 live; once a batch of overflow builds up,
  // archive it (oldest-first within the archived batch, appended to the tail
  // of the ledger).
  if (s.events.length > LIVE_EVENTS + EVENT_ARCHIVE_BATCH) {
    const overflow = s.events.slice(LIVE_EVENTS).reverse();
    h.events.push(...overflow);
    s.events = s.events.slice(0, LIVE_EVENTS);
    dirty = true;
  }

  if (dirty) writeHistoryAt(root, h, layout);
  return dirty;
}

// Accept a card by id or tracking number in a history{cards} bag.
export function findInHistory(history, ref) {
  if (ref == null) return null;
  const str = String(ref);
  const byId = history.cards.find(c => c.id === str);
  if (byId) return byId;
  const num = Number(str.replace(/^#/, ''));
  return Number.isInteger(num) ? history.cards.find(c => c.num === num) : null;
}

// Bring one archived card (+ its decisions + its embedded questions) or one
// archived decision back to the live state `s`, removing it from history
// bag `h` in place. Mutates both; returns a small summary.
export function restoreFromHistory(s, h, ref, by) {
  const str = String(ref);
  const num = Number(str.replace(/^#/, ''));
  const cardIdx = h.cards.findIndex(c => c.id === str || (Number.isInteger(num) && c.num === num));
  if (cardIdx >= 0) {
    const archived = h.cards[cardIdx];
    const decs = h.decisions.filter(d => d.cardId === archived.id);
    const questions = archived.questions || [];
    const card = { ...archived };
    delete card.retiredAt; delete card.questions;
    touchCard(card, by || 'owner');
    card.log = [{ at: today(), by: by || 'owner', text: 'Restored from archive.' }, ...(card.log || [])];
    s.cards.push(card);
    // Reset each decision's clock too — otherwise a still-'done' card's
    // stale ratifiedAt would make the very next write's retire() pass
    // standalone-retire it right back out from under the card we just
    // brought back whole.
    for (const d of decs) { const rd = { ...d }; delete rd.retiredAt; if (rd.status === 'ratified') rd.ratifiedAt = today(); s.decisions.push(rd); }
    for (const q of questions) s.questions.push(q);
    h.cards.splice(cardIdx, 1);
    h.decisions = h.decisions.filter(d => d.cardId !== archived.id);
    logEvent(s, { by, action: 'archive.restore', ref: card.id, note: `card #${card.num}` });
    return { kind: 'card', id: card.id, num: card.num };
  }
  const decIdx = h.decisions.findIndex(d => d.id === str);
  if (decIdx >= 0) {
    const archived = h.decisions[decIdx];
    const liveCard = s.cards.find(c => c.id === archived.cardId);
    if (!liveCard) fail('E_NOT_FOUND', `${str}'s card is archived too — restore the card (its id or #num), not just the decision`);
    const d = { ...archived };
    delete d.retiredAt; d.ratifiedAt = today();
    s.decisions.push(d);
    h.decisions.splice(decIdx, 1);
    touchCard(liveCard, by || 'owner');
    logEvent(s, { by, action: 'archive.restore', ref: d.id, note: 'decision' });
    return { kind: 'decision', id: d.id };
  }
  fail('E_NOT_FOUND', `no archived card or decision ${ref}`);
}

export function normalize(s, historyCards = null, sync = true) {
  if (s !== undefined && s !== null) validateStoredState(s, 'live store');
  s = s && typeof s === 'object' ? s : empty();
  s.meta = { version: VERSION, project: 'Project', nextNum: 1, rev: 0, ...(s.meta || {}) };
  s.meta.version = VERSION;
  s.meta.ui = { toggled: [], ...(s.meta.ui || {}) };
  if (s.meta.completionCursor == null && s.meta.digestCursor != null)
    s.meta.completionCursor = s.meta.digestCursor;
  delete s.meta.digestCursor;
  migrateRetiredPlanning(s);
  for (const k of ['epochs', 'milestones', 'cards', 'decisions', 'questions', 'ideas', 'events', 'briefings']) s[k] ||= [];
  s.statusSnapshot ??= null;
  delete s.messages;   // messaging was removed; drop the legacy key on next write
  // D-TWR-OPS1=A: active epoch is derived solely from epoch.status === 'active'.
  // One-time reconcile of the retired meta.currentEpoch pointer, then drop it so
  // the two-source-of-truth drift (null pointer vs an active epoch) cannot recur.
  if (s.meta.currentEpoch != null && !s.epochs.some(e => e.status === 'active')) {
    const e = s.epochs.find(x => x.id === s.meta.currentEpoch);
    if (e) e.status = 'active';
  }
  delete s.meta.currentEpoch;
  for (const c of s.cards) {
    validateStoredPhase(c, 'live card');
    c.blockedBy ||= [];
    c.log ||= [];
    c.criteria ||= [];
    for (const item of c.criteria) migrateCriterionProof(item);
    c.refs ||= [];
    c.tags ||= [];
    if (c.hardeningDedupAliases != null && !Array.isArray(c.hardeningDedupAliases)) c.hardeningDedupAliases = [];
    if (c.hardeningEvidence != null && !Array.isArray(c.hardeningEvidence)) c.hardeningEvidence = [];
    if (!('parentId' in c)) c.parentId = null;
    if (!('probe' in c)) c.probe = null;
    c.visualMedia = normalizeVisualMedia(c.visualMedia);
    c.needsAcceptance = !!c.needsAcceptance;
  }
  if (historyCards != null) validateStoredHistory({ cards: historyCards }, 'history store');
  for (const m of s.milestones) {
    m.exitCriteria ??= [];
    if (m.closeout != null && (
      typeof m.closeout !== 'object'
      || !/^[0-9a-f]{40}(?:[0-9a-f]{24})?$/.test(String(m.closeout.sourceCommit || ''))
      || !m.closeout.by
      || !m.closeout.at
    )) delete m.closeout;
    if (!MILESTONE_STATUSES.has(m.status)) m.status = 'open';
    if (m.status === 'met' && !m.verification) {
      m.status = 'open';
      delete m.metAt;
    }
  }
  for (const d of s.decisions) {
    migrateRecommendationLosses(d);
    migrateReviewPasses(d);
    d.draft = !!d.draft;
    if (d.visualMedia !== undefined) d.visualMedia = normalizeVisualMedia(d.visualMedia);
  }
  // D-ONCE-LEDGER1=A: preserve the first imported supersession edge even
  // when an older board snapshot predates the decision-link field.
  // ponytail: explicit legacy map; add only ratified pre-field edges.
  const legacySupersededBy = { 'D-VERDICT-1254-1': 'D-ONCE-TIER1' };
  const decisionIds = new Set(s.decisions.map(d => d.id));
  for (const d of s.decisions) {
    const replacement = legacySupersededBy[d.id];
    if (replacement && !d.supersededBy && decisionIds.has(replacement)) d.supersededBy = replacement;
  }
  releaseResolvedBlockers(s);
  if (sync) syncMilestones(s, undefined, historyCards == null ? [] : historyCards);
  return s;
}

// D-TWR-OPS1=A: the single source of truth for "which epoch is live".
export const activeEpoch = (s) => s.epochs.find(e => e.status === 'active')?.id ?? null;

// ---- derivation: clearance + lane (the ONE place this is decided) ---------

// A draft decision (card #458, D-TWRGUARD1=C) is a scratch ballot still being
// written — it never blocks a card and never shows in the owner's queue.
const isBlocking = (d) => d.status === 'open' && !d.draft;

// A ballot's stage, derived from the stored draft flag, reviewer and status:
// drafted → reviewed (a reviewer other than the author passed it) → posted
// (the owner's queue) → ratified. A reviewer may instead reject a draft; the
// author's next edit returns it to drafted.
export const DECISION_STAGES = ['drafted', 'reviewed', 'posted', 'ratified', 'rejected'];
export function decisionStage(d) {
  if (d.status === 'ratified' || d.status === 'rejected') return d.status;
  if (!d.draft) return 'posted';
  return d.reviewedBy ? 'reviewed' : 'drafted';
}

export function clearanceOf(card, decisions) {
  const linked = decisions.filter(d => d.cardId === card.id && !d.draft);
  if (!linked.length) return { state: 'none', open: [], total: 0, ratified: 0 };
  const open = linked.filter(d => d.status !== 'ratified');
  return { state: open.length ? 'pending' : 'cleared', open: open.map(d => d.id), total: linked.length, ratified: linked.length - open.length };
}

export function laneOf(card, decisions, cards) {
  if (card.phase === 'done')   return { lane: 'done', who: null, label: 'Done' };
  if (card.phase === 'frozen') return { lane: 'frozen', who: 'owner', label: 'Frozen — owner reactivates it' };
  // Acceptance ballots (D-ACCEPT-*) get the dedicated verify treatment on
  // the Now page, not the generic decide deck — exclude them here too, or a
  // verify-phase card with an open acceptance ballot mislabels as 'decide'
  // (#515/#516 bug: card tile click sent the owner into focusAll() looking
  // for a ballot that's deliberately excluded from that deck — dead end).
  const open = decisions.filter(d => d.cardId === card.id && isBlocking(d) && d.group !== 'acceptance');
  if (open.length) return { lane: 'decide', who: 'owner', label: `${open.length} decision${open.length > 1 ? 's' : ''} to make`, decisions: open.map(d => d.id) };
  const blockers = (card.blockedBy || []).filter(id => {
    // Resolve by id OR #num — same contract as findCard / blockedBy validation.
    const str = String(id);
    const b = cards.find(c => c.id === str) || cards.find(c => c.num === Number(str.replace(/^#/, '')));
    if (b) return b.phase !== 'done';
    const d = decisions.find(x => x.id === id);
    if (d) return d.status !== 'ratified';
    return false; // dangling ref — don't block on it
  });
  if (blockers.length) {
    const labels = blockers.map(id => {
      const str = String(id);
      const b = cards.find(c => c.id === str) || cards.find(c => c.num === Number(str.replace(/^#/, '')));
      if (b) return `#${b.num}`;
      return str; // decision id or unresolved — keep as written
    });
    return { lane: 'blocked', who: null, label: `Blocked by ${labels.join(', ')}`, blockers };
  }
  if (card.phase === 'deciding') return card.plan
    ? { lane: 'implement', who: 'agent', label: 'Ready to implement' }
    : { lane: 'plan', who: 'agent', label: 'Build a plan + raise decisions' };
  // #516: legacy 'triage' cards (phase predates the greenlight-gate removal)
  // land in the same lane a fresh card now does — treated here, never
  // rewritten in stored data.
  if (card.phase === 'planning' || card.phase === 'triage') return { lane: 'plan', who: 'agent', label: card.plan ? 'Vet the plan + raise decisions' : 'Build a plan + raise decisions' };
  if (card.phase === 'ready')    return { lane: 'implement', who: 'agent', label: 'Ready to implement' };
  if (card.phase === 'building') return { lane: 'building', who: 'agent', label: 'Continue building' };
  if (card.phase === 'verify') {
    // needsAcceptance = owner visual/UX/DX taste only. Bare verify is legacy state.
    if (card.needsAcceptance) {
      const acceptOpen = decisions.some(d => d.id === `D-ACCEPT-${card.num}` && d.status !== 'ratified');
      return acceptOpen
        ? { lane: 'verify', who: 'owner', label: 'Owner visual/UX acceptance' }
        : { lane: 'verify', who: 'agent', label: 'Finish criteria before owner visual review' };
    }
    return { lane: 'verify', who: 'agent', label: 'Close when exit criteria are met' };
  }
  return { lane: 'blocked', who: null, label: '' };
}

// Cards grouped by milestone in one pass over both lists; a live card replaces
// its archived copy. Callers that walk every milestone build this once instead
// of rescanning thousands of archived cards per milestone.
export function cardsByMilestone(cards, historyCards = []) {
  const index = new Map();
  for (const list of [historyCards, cards]) for (const c of list) {
    if (!c.milestoneId) continue;
    let linked = index.get(c.milestoneId);
    if (!linked) index.set(c.milestoneId, linked = new Map());
    linked.set(c.id, c);
  }
  return index;
}

function milestoneCards(id, cards, historyCards = [], index = cardsByMilestone(cards, historyCards)) {
  return [...(index.get(id)?.values() || [])];
}

export function milestoneProgress(m, cards, historyCards = [], index = undefined) {
  const linked = milestoneCards(m.id, cards, historyCards, index);
  const done = linked.filter(c => c.phase === 'done').length;
  const reviewReady = linked.length > 0 && done === linked.length;
  return { total: linked.length, done, reviewReady, met: reviewReady && m.status === 'met' && !!m.verification };
}

function clearMilestoneVerification(m, { keepCloseout = false } = {}) {
  if (m.status === 'retired') return;
  delete m.verification;
  delete m.metAt;
  if (!keepCloseout) delete m.closeout;
  m.status = 'open';
}

// A milestone's status follows its cards: it is open until `verifyMilestone`
// signs it off as met, and falls back to open when a linked card reopens or
// leaves. A retired milestone keeps its record as it was retired.
function syncMilestone(s, id, historyCards = [], index = undefined) {
  const m = s.milestones.find(x => x.id === id);
  if (!m || m.status === 'retired') return;
  const linked = milestoneCards(id, s.cards, historyCards, index);
  if (!linked.length || !linked.every(c => c.phase === 'done')) {
    clearMilestoneVerification(m);
    return;
  }
  if (m.status === 'met' && m.verification) return;
  // Card completion only opens the milestone review (`progress.reviewReady`).
  // A closeout token survives evidence recording while every linked card
  // remains closed.
  clearMilestoneVerification(m, { keepCloseout: true });
}

function syncMilestones(s, ids = s.milestones.map(m => m.id), historyCards = []) {
  const index = cardsByMilestone(s.cards, historyCards);
  for (const id of new Set(ids.filter(Boolean))) syncMilestone(s, id, historyCards, index);
}

function invalidateCardMilestone(s, card) {
  if (!card?.milestoneId) return;
  const m = s.milestones.find(x => x.id === card.milestoneId);
  if (!m) return;
  clearMilestoneVerification(m);
  syncMilestone(s, m.id);
}

// ---- radar: roadmap-ledger + ops-table hybrid (#464, D-TWR-BOARD1=A) ------
// NEW page only — never touches Board/Now. Per active epoch (status not
// arrived/done), current epoch first then epoch order: burndown sparkline,
// milestone stall badges, done/active counts. Ops-table rows (the actual
// active cards) are NOT embedded here — the UI reads S.cards directly per
// epoch, same source Board already uses, so radarData stays a light,
// always-on computation (measured: 212 live cards × 500 events, cheap).

const RADAR_DAYS = 30;
const DAY_MS = 86_400_000;
const dayKey = (iso) => (iso ? String(iso).slice(0, 10) : null);

// Burndown is an approximation, not an audit trail: it buckets LIVE events
// (capped at 500 by retire(), see LIVE_EVENTS above) where a card.update
// touched `phase` and that card is CURRENTLY done, by the day the event
// fired. Two known undercounts, both accepted for a burndown sparkline: (1)
// once the 500-event window rolls past a day, that day's true count is lost
// (archived history isn't walked here — keeps this cheap on every read);
// (2) a card closed via an acceptance ballot (D-ACCEPT-*) flips phase inside
// `ratify()`, which logs a `decision.ratify` event, not `card.update`, so
// that closure doesn't register a burndown tick. Good enough for "is this
// epoch moving", not for a certified done-count (that's `doneArchivedHint`
// + the ledger line, fetched from history.json).
function burndown30(s, epochId) {
  const doneIds = new Set(s.cards.filter(c => c.epoch === epochId && c.phase === 'done' && c.track !== 'sidequest').map(c => c.id));
  const days = [];
  const base = Date.parse(`${today()}T00:00:00Z`);
  for (let i = RADAR_DAYS - 1; i >= 0; i--) days.push(new Date(base - i * DAY_MS).toISOString().slice(0, 10));
  const counts = Object.fromEntries(days.map(d => [d, 0]));
  for (const e of s.events) {
    if (e.action !== 'card.update' || !doneIds.has(e.ref)) continue;
    if (!String(e.note || '').split(',').includes('phase')) continue;
    const k = dayKey(e.at);
    if (k && k in counts) counts[k]++;
  }
  return days.map(day => ({ day, n: counts[day] }));
}

// Days since any event touched a card linked to this milestone (any phase —
// "stalled" means the milestone itself went quiet, not just its open cards).
// null when never touched.
function milestoneStallDays(m, cards, events) {
  const linkedIds = new Set(cards.filter(c => c.milestoneId === m.id).map(c => c.id));
  if (!linkedIds.size) return null;
  const hit = events.find(e => linkedIds.has(e.ref)); // events are newest-first
  if (!hit) return null;
  return Math.floor((Date.now() - new Date(hit.at).getTime()) / DAY_MS);
}

export function radarData(s, historyCards = []) {
  const sorted = sortEpochs(s.epochs.filter(e => !['arrived', 'done'].includes(e.status)));
  const byMilestone = cardsByMilestone(s.cards, historyCards);
  const chain = new Map(epochChain(s.epochs, s.cards).map(row => [row.id, row]));
  return sorted.map(e => {
    const all = s.cards.filter(c => c.epoch === e.id && c.track !== 'sidequest');
    const active = all.filter(c => !['done', 'frozen'].includes(c.phase));
    const done = all.filter(c => c.phase === 'done');
    const milestones = sortMilestones(s.milestones.filter(m => m.epochId === e.id && m.status !== 'retired')).map(m => {
      const progress = milestoneProgress(m, s.cards, historyCards, byMilestone);
      return { id: m.id, title: m.title, goal: m.goal, status: m.status, order: m.order, met: progress.met,
        ...progress, stalledDays: milestoneStallDays(m, s.cards, s.events) };
    });
    const milestonesMet = milestones.filter(m => m.met).length;
    const milestoneTotal = milestones.length;
    const pct = milestoneTotal ? Math.round(milestonesMet / milestoneTotal * 100) : 0;
    const { role, staff, openCriteria, blockedCriteria } = chain.get(e.id);
    return {
      id: e.id, name: e.name, goal: e.goal, role, staff, openCriteria, blockedCriteria,
      active: active.length, done: done.length, doneArchivedHint: null,
      milestoneTotal, milestonesMet, pct, burndown: burndown30(s, e.id), milestones,
    };
  });
}

// Group records by their cardId once, so projecting many cards does not
// rescan every decision and question per card.
function byCardId(list) {
  const groups = new Map();
  for (const item of list) {
    let group = groups.get(item.cardId);
    if (!group) groups.set(item.cardId, group = []);
    group.push(item);
  }
  return groups;
}

function projectCardRecord(c, decisions, cards, questions, groups = null) {
  const linkedDecisions = groups?.decisions ? (groups.decisions.get(c.id) || []) : decisions.filter(d => d.cardId === c.id);
  const linkedQuestions = groups?.questions ? (groups.questions.get(c.id) || []) : questions.filter(q => q.cardId === c.id);
  const clearance = clearanceOf(c, linkedDecisions);
  const openQ = linkedQuestions.filter(q => q.kind !== 'message' && q.status === 'open').length;
  return { ...c, clearance, decisions: linkedDecisions, questions: linkedQuestions, openQ, lane: laneOf(c, decisions, cards) };
}

// Board index milestones: navigation and progress only. Criteria evidence,
// verification, and closeout records stay in the store and CLI.
function projectMilestoneSummary(m, progress) {
  const criteria = m.exitCriteria || [];
  return {
    id: m.id, epochId: m.epochId, title: m.title, goal: m.goal, status: m.status, order: m.order,
    retired: m.retired, created: m.created, metAt: m.metAt, progress,
    exitCriteriaCount: { total: criteria.length, settled: criteria.filter(i => i.status === 'met' || i.status === 'verified').length },
  };
}

// One board row. Question threads live once in the index's top-level
// `questions`; refs, probes, and blockers (already reflected in `lane`) load
// with the full card. `crit` counts the criteria the summary leaves out, for
// the epoch chain and the Closest view.
function projectCardSummary(c) {
  const summary = {
    id: c.id, num: c.num, title: c.title, kind: c.kind, track: c.track,
    epoch: c.epoch, milestoneId: c.milestoneId, phase: c.phase,
    priority: c.priority, workOrder: c.workOrder, assignee: c.assignee,
    needsAcceptance: c.needsAcceptance, visualMedia: c.visualMedia || [], updated: c.updated, created: c.created,
    completedAt: c.completedAt, lane: c.lane, openQ: c.openQ, crit: criteriaCounts(c),
  };
  if (c.hardeningDedupKey) {
    summary.hardeningDedupKey = c.hardeningDedupKey;
    summary.hardeningDedupAliases = c.hardeningDedupAliases || [];
    summary.hardeningFindingId = c.hardeningFindingId || null;
    summary.hardeningSeverity = c.hardeningSeverity || c.priority;
  }
  // The Now view needs only the small owner-verification ballot slice before
  // it can fetch the full card detail on demand.
  const acceptance = (c.decisions || []).filter(d => (d.group === 'acceptance' || d.id?.startsWith('D-ACCEPT-')) && d.status !== 'ratified')
    .map(d => ({ id: d.id, status: d.status, created: d.created, gist: d.gist, detail: d.detail, checkInstructions: d.checkInstructions, visualMedia: d.visualMedia || [] }));
  if (acceptance.length) summary.decisions = acceptance;
  if (c.needsAcceptance && c.phase === 'verify') summary.criteria = c.criteria || [];
  return summary;
}

const cardSummary = (c) => ({
  id: c.id, num: c.num, title: c.title, phase: c.phase, epoch: c.epoch,
  track: c.track, milestoneId: c.milestoneId, completedAt: c.completedAt,
  updated: c.updated, archived: !!c.archived,
});

function validateProjectedPhases(s, history) {
  validateStoredPhases(s?.cards, 'live card');
  validateStoredPhases(history?.cards, 'archived card');
}

// Owner projections must not expose scratch ballots through secondary surfaces
// (card details, activity, questions, or Now report links). Agent CLI reads and
// briefs use the raw store, so drafts remain available for review.
function withoutDrafts(s) {
  const hidden = new Set(s.decisions.filter(d => d.draft).map(d => d.id));
  for (const e of s.events || []) if (e.draft && e.ref) hidden.add(e.ref);
  if (!hidden.size) return s;
  const questions = items => (items || []).filter(q => !hidden.has(q.decisionId));
  const linked = item => ({ ...item, links: (item.links || []).filter(link => !hidden.has(link.decision)) });
  return {
    ...s,
    decisions: s.decisions.filter(d => !d.draft),
    cards: s.cards.map(c => ({
      ...c, blockedBy: (c.blockedBy || []).filter(id => !hidden.has(id)),
      ...(c.questions ? { questions: questions(c.questions) } : {}),
    })),
    questions: questions(s.questions),
    events: (s.events || []).filter(e => !hidden.has(e.ref)),
    briefings: (s.briefings || []).map(b => ({ ...b, sections: (b.sections || []).map(linked) })),
    statusSnapshot: s.statusSnapshot ? {
      ...s.statusSnapshot,
      milestones: s.statusSnapshot.milestones.map(linked),
      workstreams: s.statusSnapshot.workstreams.map(linked),
    } : null,
  };
}

export function project(s, config = null, history = null) {
  s = withoutDrafts(s);
  validateProjectedPhases(s, history);
  const cards = s.cards.map(c => projectCardRecord(c, s.decisions, s.cards, s.questions));
  const historyCards = history?.cards || [];
  const byMilestone = cardsByMilestone(s.cards, historyCards);
  const milestones = s.milestones.map(m => ({ ...m, progress: milestoneProgress(m, s.cards, historyCards, byMilestone) }));
  const inLane = (l) => cards.filter(c => c.lane.lane === l);
  // Acceptance ballots are a distinct owner duty (verify queue), not a
  // generic decision — keep the decide/push-notification counts to the
  // plain ballot deck, same split laneOf() and openGenericDecisions() use.
  const openDecisions = s.decisions.filter(d => isBlocking(d) && d.group !== 'acceptance');
  // #461 walk-back buffer: ratifications still inside the retire window —
  // reopenable in one tap while fresh. Older ratified decisions can stay
  // live because their card is active; they aren't "recent" and would bury
  // the strip.
  const bufferDays = (config && config.retireAfterDays) ?? 3;
  const recentlyDecided = s.decisions.filter(d => d.status === 'ratified' && !d.draft && !isOlderThanDays(d.ratifiedAt, bufferDays))
    .map(d => ({ id: d.id, title: d.title, outcome: d.outcome, comment: d.comment || '', ratifiedAt: d.ratifiedAt, cardId: d.cardId }))
    .sort((a, b) => (b.ratifiedAt || '').localeCompare(a.ratifiedAt || ''));
  const counts = {
    byPhase: Object.fromEntries(PHASE_IDS.map(p => [p, cards.filter(c => c.phase === p).length])),
    forYou: openDecisions.length,
    decide: openDecisions.length,
    agentReady: inLane('plan').length + inLane('implement').length + inLane('building').length + inLane('verify').length,
    sidequests: cards.filter(c => c.track === 'sidequest' && ACTIVE.includes(c.phase)).length,
    frozen: cards.filter(c => c.phase === 'frozen').length,
    ideas: s.ideas.filter(b => b.status !== 'tagged').length,
    openQuestions: s.questions.filter(q => q.kind !== 'message' && q.status === 'open').length,
    today: openToday(s),
  };
  return { meta: s.meta, config: publicConfig(config) || undefined, epochs: sortEpochs(s.epochs), milestones, phases: PHASES, lanes: LANES,
    cards, decisions: s.decisions, questions: s.questions, ideas: s.ideas,
    events: s.events.slice(0, 300), counts, recentlyDecided, radar: radarData(s, historyCards) };
}

// Board HTTP reads use this projection. Keep the initial payload useful for
// navigation, but leave descriptions, logs, criteria, and ratified decisions
// for the card detail endpoint.
export function projectBoard(s, config = null) {
  s = withoutDrafts(s);
  validateProjectedPhases(s);
  const active = s.cards.filter(c => c.phase !== 'done');
  const activeIds = new Set(active.map(c => c.id));
  const groups = { decisions: byCardId(s.decisions), questions: byCardId(s.questions) };
  const cards = active.map(c => projectCardSummary(projectCardRecord(c, s.decisions, s.cards, s.questions, groups)));
  const milestones = s.milestones.map(m => projectMilestoneSummary(m, milestoneProgress(m, s.cards)));
  const inLane = (lane) => cards.filter(c => c.lane.lane === lane);
  const openDecisions = s.decisions.filter(d => isBlocking(d) && !d.draft);
  const genericDecisions = openDecisions.filter(d => d.group !== 'acceptance');
  const bufferDays = (config && config.retireAfterDays) ?? 3;
  const recentlyDecided = s.decisions.filter(d => d.status === 'ratified' && !d.draft && !isOlderThanDays(d.ratifiedAt, bufferDays))
    .map(d => ({ id: d.id, title: d.title, outcome: d.outcome, comment: d.comment || '', ratifiedAt: d.ratifiedAt, cardId: d.cardId }))
    .sort((a, b) => (b.ratifiedAt || '').localeCompare(a.ratifiedAt || ''));
  const noticeMessages = s.questions.filter(q => q.kind === 'message' && q.status === 'open');
  const noticeIds = new Set(noticeMessages.map(q => q.cardId));
  const cursor = s.meta.completionCursor;
  if (cursor) {
    for (const c of s.cards) {
      if (c.phase === 'done' && completionTime(c) > cursor) noticeIds.add(c.id);
    }
  }
  const noticeCards = s.cards.filter(c => c.phase === 'done' && noticeIds.has(c.id)).map(cardSummary);
  const counts = {
    byPhase: Object.fromEntries(PHASE_IDS.map(p => [p, s.cards.filter(c => c.phase === p).length])),
    forYou: genericDecisions.length,
    decide: genericDecisions.length,
    agentReady: inLane('plan').length + inLane('implement').length + inLane('building').length + inLane('verify').length,
    sidequests: s.cards.filter(c => c.track === 'sidequest' && ACTIVE.includes(c.phase)).length,
    frozen: s.cards.filter(c => c.phase === 'frozen').length,
    ideas: s.ideas.filter(b => b.status !== 'tagged').length,
    openQuestions: s.questions.filter(q => q.kind !== 'message' && q.status === 'open').length,
    today: openToday(s),
  };
  return {
    meta: s.meta, config: publicConfig(config) || undefined, epochs: s.epochs, milestones,
    phases: PHASES, lanes: LANES, cards, decisions: openDecisions,
    questions: s.questions.filter(q => activeIds.has(q.cardId) || (q.decisionId && openDecisions.some(d => d.id === q.decisionId))), ideas: s.ideas,
    events: s.events.slice(0, 300), counts, recentlyDecided,
    briefings: s.briefings || [], statusSnapshot: s.statusSnapshot || null,
    notices: { cards: noticeCards, messages: noticeMessages }, radar: radarData(s),
  };
}

// Return one full card for the detail modal without building the full board.
export function projectCard(s, ref, history = null) {
  s = withoutDrafts(s);
  validateProjectedPhases(s, history);
  const live = findCard(s, ref);
  if (live) return projectCardRecord(live, s.decisions, s.cards, s.questions);
  const h = withoutDrafts(history || emptyHistory());
  const str = String(ref ?? '');
  const num = Number(str.replace(/^#/, ''));
  const archived = h.cards.find(c => c.id === str || (Number.isInteger(num) && c.num === num));
  return archived ? { ...projectCardRecord(archived, h.decisions, h.cards, archived.questions || []), archived: true } : null;
}

const timestamp = (value) => {
  if (!value) return null;
  const raw = String(value);
  const parsed = Date.parse(raw.length <= 10 ? `${raw}T00:00:00Z` : raw);
  return Number.isNaN(parsed) ? null : parsed;
};

function cardEventTime(events, card, predicate, after = -Infinity) {
  const refs = new Set([card.id, `#${card.num}`, String(card.num)]);
  return events
    .filter(event => refs.has(String(event.ref)) && predicate(event))
    .map(event => timestamp(event.at))
    .filter(at => at != null && at > after)
    .reduce((latest, at) => Math.max(latest, at), null);
}

function wasOpenAt(card, cutoff, events) {
  const created = timestamp(card.created)
    ?? cardEventTime(events, card, event => event.action === 'card.add');
  if (created != null && created > cutoff) return false;
  if (card.phase !== 'done' && card.phase !== 'frozen') return true;
  const completed = card.phase === 'done'
    ? timestamp(card.completedAt)
      ?? cardEventTime(events, card, event => event.action === 'card.update' && /(?:^|,)phase(?:,|$)/.test(event.note || ''), cutoff)
      ?? timestamp(completionTime(card))
    : null;
  return completed != null && completed > cutoff;
}

// The store already keeps card creation/completion timestamps and archived
// card snapshots. Reconstruct the window's opening count from those records;
// no separate counter can drift from the board.
export function openCountTrend(s, history, days = 7, at = Date.now()) {
  const windowDays = Math.max(1, Math.floor(Number(days) || 7));
  const end = Number.isFinite(at) ? at : Date.now();
  const cutoff = end - windowDays * DAY_MS;
  const events = [...(history?.events || []), ...(s?.events || [])];
  const known = new Map();
  for (const card of [...(s?.cards || []), ...(history?.cards || [])]) {
    const key = card.id ?? `#${card.num}`;
    if (!known.has(key)) known.set(key, card);
  }
  const openNow = (s?.cards || []).filter(card => card.phase !== 'done' && card.phase !== 'frozen').length;
  const openAtStart = [...known.values()].filter(card => wasOpenAt(card, cutoff, events)).length;
  return {
    windowDays,
    since: new Date(cutoff).toISOString(),
    openAtStart,
    openNow,
    delta: openNow - openAtStart,
  };
}

// Open cards (not done or frozen) and their unsettled criteria, board-wide,
// and per epoch the way the chain counts them (no sidequests; open criteria
// leave blocked rows out).
export function openTally(s) {
  let openCards = 0;
  let openCriteria = 0;
  for (const c of s.cards || []) {
    if (c.phase === 'done' || c.phase === 'frozen') continue;
    const counts = criteriaCounts(c);
    openCards++;
    openCriteria += counts.open + counts.blocked;
  }
  const epochs = Object.fromEntries(epochChain(s.epochs || [], s.cards || [])
    .map(e => [e.id, { openCards: e.openCards, openCriteria: e.openCriteria }]));
  return { openCards, openCriteria, epochs };
}

// The first write of each (UTC) day records the open tally it started from,
// so today's change is the board now against that start.
function stampDayStart(s) {
  const date = today();
  if (s.meta.dayStart?.date !== date) s.meta.dayStart = { date, ...openTally(s) };
}

// Open cards and criteria now, each with today's change, board-wide and per
// epoch. A board not written today has not changed today.
export function openToday(s) {
  const date = today();
  const current = openTally(s);
  const start = s.meta?.dayStart?.date === date ? s.meta.dayStart : current;
  const delta = (now, before = { openCards: 0, openCriteria: 0 }) => ({
    openCards: now.openCards, openCriteria: now.openCriteria,
    openCardsDelta: now.openCards - before.openCards, openCriteriaDelta: now.openCriteria - before.openCriteria,
  });
  return {
    date, ...delta(current, start),
    epochs: Object.fromEntries(Object.entries(current.epochs).map(([id, e]) => [id, delta(e, start.epochs?.[id])])),
  };
}

// ---- resolution helpers ----------------------------------------------------

// Accept a card by id or by tracking number ("#12" or "12").
export function findCard(s, ref) {
  if (ref == null) return null;
  const str = String(ref);
  const byId = s.cards.find(c => c.id === str);
  if (byId) return byId;
  const num = Number(str.replace(/^#/, ''));
  return Number.isInteger(num) ? s.cards.find(c => c.num === num) : null;
}
const mustCard = (s, ref) => findCard(s, ref) || fail('E_NOT_FOUND', `no card ${ref}`);

function hasHardeningKey(card, key) {
  const wanted = String(key ?? '').trim();
  return !!wanted && (card?.hardeningDedupKey === wanted
    || (card?.hardeningDedupAliases || []).includes(wanted));
}

// Resolve a hardening identity across the live board and append-only archive.
// Live wins when an undo or interrupted repair briefly exposes both copies.
export function findHardeningMatch(s, history, key) {
  const live = (s?.cards || []).find(card => hasHardeningKey(card, key));
  if (live) return { card: live, archived: false };
  const archived = (history?.cards || []).find(card => hasHardeningKey(card, key));
  return archived ? { card: archived, archived: true } : null;
}

export function findHardeningCard(s, history, key) {
  return findHardeningMatch(s, history, key)?.card || null;
}

// Everything that happened to one card, newest first: events on the card
// itself, on its decisions and questions, and board.apply plans that touched
// it. The card may be live or archived; events may be live or spilled to
// history.json.
export function cardHistory(s, history, ref) {
  const live = findCard(s, ref);
  const archived = live ? null : findCard({ cards: history?.cards || [] }, ref);
  const card = live || archived || fail('E_NOT_FOUND', `no card ${ref}`);
  const related = new Set([card.id,
    ...[...s.decisions, ...(history?.decisions || [])].filter(d => d.cardId === card.id).map(d => d.id),
    ...s.questions.filter(q => q.cardId === card.id).map(q => q.id)]);
  const events = [...s.events, ...(history?.events || [])]
    .filter(e => related.has(e.ref) || (e.refs || []).includes(card.id))
    .sort((a, b) => String(b.at).localeCompare(String(a.at)));
  return { card: { id: card.id, num: card.num, title: card.title, ...(archived ? { archived: true } : {}) }, events };
}

// Store known blocker refs canonically. Card lanes accept #N for convenience;
// preserve dangling add-time refs so lint can report them, while update keeps
// the existing hard rejection for an unknown replacement.
function normalizeBlockedBy(s, raw, { rejectUnknown = false } = {}) {
  const refs = raw == null ? [] : Array.isArray(raw) ? raw : [raw];
  return refs.map((value) => {
    const ref = String(value ?? '').trim();
    if (!ref) fail('E_INVALID', 'blockedBy needs a card or decision ref');
    const card = findCard(s, ref);
    if (card) return card.id;
    const decision = s.decisions.find(d => d.id === ref);
    if (decision) return decision.id;
    if (rejectUnknown) fail('E_NOT_FOUND', `blockedBy: no card or decision ${ref}`);
    return ref;
  });
}

const checkEnum = (val, list, what) => {
  if (val != null && !list.includes(val)) fail('E_INVALID', `${what} must be one of: ${list.join(', ')} (got ${JSON.stringify(val)})`);
};
const checkEpoch = (s, id) => { if (id != null && !s.epochs.find(e => e.id === id)) fail('E_NOT_FOUND', `no epoch ${id}`); };

// Owner rulings 2026-08-05 / 2026-10-04: every card lives in an epoch or is a
// sidequest — frozen cards included; frozen is a phase, not a home. An
// epoch-track card with no epoch is unreachable from every board view, so the
// state is rejected at the store boundary (CLI and API alike).
const checkCardHome = ({ track, epoch }) => {
  if (track === 'epoch' && epoch == null)
    fail('E_INVALID', 'a card must live in an epoch or be a sidequest (frozen cards too) — pass --epoch <id> (no epoch is active to inherit) or --track sidequest');
};
const checkMilestone = (s, id) => { if (id != null && !s.milestones.find(m => m.id === id)) fail('E_NOT_FOUND', `no milestone ${id}`); };
// A card's milestone belongs to the card's epoch. A retired milestone keeps
// the cards already linked to it but takes no new ones.
function checkCardMilestone(s, { epoch, track, milestoneId, linking = true }) {
  if (milestoneId == null) return;
  const milestone = s.milestones.find(m => m.id === milestoneId);
  if (track === 'sidequest') fail('E_INVALID', 'sidequest cards cannot link to milestones');
  if (linking && milestone.status === 'retired')
    fail('E_INVALID', `milestone ${milestone.id} is retired (${milestone.retired?.reason || 'no reason given'}); link the card to a live milestone`);
  if (milestone.epochId !== epoch)
    fail('E_INVALID', `milestone ${milestone.id} belongs to epoch ${milestone.epochId}, not ${epoch || 'no epoch'}`);
}
// #462: refs — free-form doc-path pointers a card carries explicitly (in
// addition to whatever `tower brief` harvests out of body/plan).
const checkRefs = (val) => {
  if (val !== undefined && !(Array.isArray(val) && val.every(x => typeof x === 'string')))
    fail('E_INVALID', 'refs must be an array of strings');
};

// `refs`: further records the event touched, beyond its primary `ref`.
export function logEvent(s, { by = 'agent', action, ref = null, refs, note = '', draft = false }) {
  s.events.unshift({ at: now(), by, action, ref, ...(refs?.length ? { refs } : {}), note, ...(draft ? { draft: true } : {}) });
  if (s.events.length > 2000) s.events.length = 2000;
}

// `tower redact --apply` (#6564 c5): one write that rewrites every personal or
// machine span in tower.json and history.json with its neutral form, and one
// event that names the rule table and the per-section counts.
export function applyRedaction(s, history, by, matcher = machinePersonalInfo()) {
  const report = redactBoard(s, history, matcher);
  if (report.total) {
    const sections = Object.entries(report.sections).map(([name, n]) => `${name} ${n}`).join(', ');
    logEvent(s, { by, action: 'board.redact', note: `${report.ruleTable}: ${report.total} spans redacted (${sections})` });
  }
  return report;
}

// The dry run: the same counts over copies, so the board is not touched.
export function redactBoardCopy(s, history, matcher = machinePersonalInfo()) {
  return redactBoard(structuredClone(s), structuredClone(history), matcher);
}

// ---- mutations: cards ------------------------------------------------------

// One exit-criteria item: 1-based stable n, open -> met (orchestrator after
// integrated proof) -> verified (a different reviewer).
function normalizeCriterion(it, i) {
  const source = typeof it === 'string' ? { text: it } : (it || {});
  const n = source.n ?? (i + 1);
  const status = source.status ?? 'open';
  validateStoredNumber(n, `criterion ${i + 1}.n`, { integer: true, min: 1 });
  if (!CRITERION_STATUSES.has(status)) fail('E_INVALID', `criterion ${i + 1}.status must be open, met, or verified`);
  const item = {
    n,
    text: String(source.text || '').trim(),
    status,
    metBy: source.metBy ?? null,
    verifiedBy: source.verifiedBy ?? null,
    evidence: source.evidence || '',
    at: source.at || now(),
  };
  if (source.proof != null) item.proof = proofInput(source.proof);
  return item;
}

function assertCriterionText(raw) {
  if (raw == null) return;
  if (typeof raw === 'string' && !raw.trim()) fail('E_INVALID', 'criterion needs text');
  const items = Array.isArray(raw) ? raw : raw && typeof raw === 'object' && Array.isArray(raw.items)
    ? raw.items : [raw];
  for (const item of items) {
    const text = typeof item === 'string' ? item : item?.text;
    if (!text || !String(text).trim()) fail('E_INVALID', 'criterion needs text');
  }
}

// A milestone's exit criteria take a card criterion's shape: text, an
// optional proof, and the open → met → verified lifecycle.
function normalizeExitCriteria(raw) {
  const list = Array.isArray(raw) ? raw : raw == null || raw === '' ? [] : [raw];
  return list.map((it, i) => normalizeCriterion(it, i));
}

function touchCard(card, by) {
  card.updated = now();
  card.updatedBy = by || 'agent';
}

function normalizeTags(raw) {
  if (raw == null) return [];
  const list = Array.isArray(raw) ? raw : String(raw).split(',');
  const out = [];
  const seen = new Set();
  for (const item of list) {
    const tag = String(item ?? '').trim();
    if (!tag) continue;
    if (seen.has(tag)) continue;
    seen.add(tag);
    out.push(tag);
  }
  return out;
}

function resolveParentId(s, parent) {
  if (parent == null || parent === '') return null;
  const found = findCard(s, parent);
  if (!found) fail('E_NOT_FOUND', `parent: no card ${parent}`);
  return found.id;
}

export function addCard(s, p, config, history = emptyHistory()) {
  if (hasHardeningPayload(p)) return addOrUpdateHardeningCard(s, p, config, history).card;
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'card needs a title');
  checkEnum(p.kind, config.kinds, 'kind');
  checkEnum(p.track, config.tracks, 'track');
  checkEnum(p.priority, config.priorities, 'priority');
  checkEnum(p.phase, PHASE_IDS, 'phase');
  const epoch = p.epoch ?? activeEpoch(s);
  const track = p.track || config.tracks[0];
  checkEpoch(s, epoch); checkMilestone(s, p.milestoneId);
  checkCardHome({ track, epoch });
  checkCardMilestone(s, { epoch, track, milestoneId: p.milestoneId });
  checkRefs(p.refs);
  validateStoredString(p.probe, 'card.probe', { nullable: true, nonEmpty: true });
  const visualMedia = normalizeVisualMedia(p.visualMedia);
  if (p.criteria !== undefined) assertCriterionText(p.criteria);
  const overLimit = admitCriteria(config, 0, Array.isArray(p.criteria) ? liveCriteriaCount(p) : 0, p.overLimit, 'the new card');
  const num = p.num == null ? s.meta.nextNum++ : Number(p.num);
  validateStoredNumber(num, 'card.num', { integer: true, min: 1 });
  const workOrder = p.workOrder == null || p.workOrder === '' ? undefined : Number(p.workOrder);
  validateStoredNumber(workOrder, 'card.workOrder', { min: 0 });
  const blockedBy = normalizeBlockedBy(s, p.blockedBy);
  const parentId = 'parentId' in p || 'parent' in p
    ? resolveParentId(s, p.parentId ?? p.parent)
    : null;
  if (parentId && parentId === (p.id || null))
    fail('E_INVALID', 'card cannot parent itself');
  const card = {
    id: p.id || newId('c'),
    num,
    title: String(p.title).trim(),
    body: p.body || '',
    kind: p.kind || config.kinds[0],
    track,
    epoch,
    milestoneId: p.milestoneId || null,
    phase: p.phase || 'planning',
    priority: p.priority || config.priorities[2] || config.priorities.at(-1),
    plan: p.plan || null,
    checkSteps: p.checkSteps || null,
    probe: p.probe || null,
    visualMedia,
    blockedBy,
    workOrder,
    assignee: p.assignee || null,
    log: p.log || [],
    criteria: Array.isArray(p.criteria) ? p.criteria.map((it, i) => normalizeCriterion(it, i)) : [],
    refs: Array.isArray(p.refs) ? p.refs : [],
    tags: normalizeTags(p.tags),
    parentId,
    needsAcceptance: !!p.needsAcceptance,
    created: now(), updated: now(), updatedBy: p.by || 'agent',
  };
  s.cards.push(card);
  syncMilestones(s, [card.milestoneId]);
  logEvent(s, { by: p.by, action: 'card.add', ref: card.id, note: card.title });
  noteOverLimit(s, card, overLimit, p.by);
  return card;
}

function prepareHardeningOrFail(p, previous = null) {
  try { return prepareHardening(p, previous); }
  catch (error) {
    if (error instanceof HardeningInputError) fail(error.code, error.message);
    throw error;
  }
}

function appendHardeningEvidence(card, prepared) {
  card.hardeningEvidence ||= [];
  if (prepared.evidence) {
    const digest = prepared.evidence.bundleDigest;
    if (!card.hardeningEvidence.some(item => item.bundleDigest === digest)) {
      card.hardeningEvidence.push(prepared.evidence);
      const block = formatHardeningEvidence(prepared.evidence);
      card.body = [String(card.body || '').trim(), block].filter(Boolean).join('\n\n');
    }
  }
  if (prepared.body && !String(card.body || '').includes(prepared.body))
    card.body = [String(card.body || '').trim(), prepared.body].filter(Boolean).join('\n\n');
}

function applyHardeningMetadata(s, card, prepared, by, action) {
  const oldKey = card.hardeningDedupKey;
  card.hardeningSchemaVersion = prepared.schemaVersion;
  card.hardeningSeam = prepared.seam;
  card.hardeningRelation = prepared.relation;
  card.hardeningWrongTierMask = prepared.wrongTierMask;
  card.hardeningInputPartition = prepared.inputPartition;
  card.hardeningDedupKey = prepared.key;
  card.hardeningDedupAliases = [...new Set([
    ...(card.hardeningDedupAliases || []),
    ...(prepared.aliases || []),
    ...(oldKey && oldKey !== prepared.key ? [oldKey] : []),
  ])].filter(key => key !== prepared.key);
  card.hardeningFindingId = prepared.findingId;
  card.hardeningSeverity = card.hardeningSeverity === 'P0' ? 'P0' : prepared.severity;
  card.hardeningState = 'open';
  if (prepared.fixture) card.hardeningFixture = prepared.fixture;
  appendHardeningEvidence(card, prepared);
  card.log ||= [];
  const digest = prepared.evidence?.bundleDigest || card.hardeningEvidence.at(-1)?.bundleDigest || 'none';
  card.log.unshift({ at: today(), by, text: `Hardening ${action}: ${prepared.findingId} (${digest})` });
  touchCard(card, by);
  logEvent(s, { by, action: 'hardening.card-upsert', ref: card.id, note: `${action} ${prepared.key}` });
  return card;
}

function hardeningMatches(s, history, keys) {
  const wanted = new Set(keys.filter(Boolean));
  const matches = [];
  for (const card of s.cards || []) {
    if ([card.hardeningDedupKey, ...(card.hardeningDedupAliases || [])].some(key => wanted.has(key)))
      matches.push({ card, archived: false });
  }
  for (const card of history?.cards || []) {
    if ([card.hardeningDedupKey, ...(card.hardeningDedupAliases || [])].some(key => wanted.has(key)))
      matches.push({ card, archived: true });
  }
  return matches;
}

const HARDENING_IDENTITY_FIELDS = [
  'hardeningSchemaVersion', 'schemaVersion', 'hardeningSeam', 'rootSeam', 'semanticPrimitive',
  'hardeningRelation', 'violatedRelation', 'relation', 'hardeningWrongTierMask', 'wrongTierMask',
  'hardeningInputPartition', 'inputPartition', 'hardeningDedupKey', 'hardening_dedup_key',
  'hardeningDedupAliases', 'hardeningFindingId', 'findingId',
];
const HARDENING_COMPONENT_FIELDS = HARDENING_IDENTITY_FIELDS.filter(key => ![
  'hardeningDedupKey', 'hardening_dedup_key', 'hardeningDedupAliases', 'hardeningFindingId', 'findingId',
].includes(key));

// The only hardening write entry point. The caller must invoke it from
// store.mutate, so key lookup, archive reuse, and card creation share one lock.
export function addOrUpdateHardeningCard(s, p, config, history = emptyHistory()) {
  if (!p?.by || !String(p.by).trim()) fail('E_INVALID', 'hardening card writes need --by <agent>');
  const ref = p.ref ?? p.cardRef;
  const referenced = ref == null ? null : findCard(s, ref);
  let prepared = prepareHardeningOrFail(p, referenced);
  const keys = [prepared.key, ...prepared.aliases, String(p.hardeningDedupKey || '').trim()].filter(Boolean);
  let matches = hardeningMatches(s, history, keys);
  if (referenced && referenced.hardeningDedupKey && !matches.some(item => item.card.id === referenced.id))
    matches = [{ card: referenced, archived: false }, ...matches];
  const ids = [...new Set(matches.map(item => item.card.id))];
  if (ids.length > 1)
    fail('E_DUPLICATE', `hardening key resolves to multiple cards: ${ids.join(', ')}`);
  if (matches.length) {
    const existing = matches[0].card;
    const findingProvided = ['hardeningFindingId', 'findingId', 'finding_id']
      .some(key => p[key] !== undefined && p[key] !== null && String(p[key]).trim());
    // A recurrence carrying an old key is evidence for the existing root
    // seam, not permission to demote that seam to an unclassified new card.
    // The finding ID is likewise card-stable unless triage explicitly changes it.
    const identity = !HARDENING_COMPONENT_FIELDS.some(key => p[key] !== undefined)
      ? { hardeningDedupKey: existing.hardeningDedupKey }
      : {};
    if (!findingProvided) identity.hardeningFindingId = existing.hardeningFindingId;
    if (Object.keys(identity).length)
      prepared = prepareHardeningOrFail({ ...p, ...identity }, existing);
  }
  if (!matches.length) {
    if (p.phase === 'done' || p.phase === 'frozen')
      fail('E_INVALID', 'hardening runner cannot create a terminal card');
    const card = addCard(s, {
      title: prepared.title,
      body: '',
      kind: p.kind,
      track: p.track,
      epoch: p.epoch,
      milestoneId: p.milestoneId,
      phase: p.phase,
      priority: prepared.priority,
      plan: p.plan,
      checkSteps: p.checkSteps,
      blockedBy: p.blockedBy,
      workOrder: p.workOrder,
      refs: p.refs,
      parent: p.parent,
      by: p.by,
      needsAcceptance: false,
    }, config);
    applyHardeningMetadata(s, card, prepared, p.by, 'added');
    return { card, action: 'added', archived: false };
  }

  let match = matches[0];
  if (match.archived) {
    restoreFromHistory(s, history, match.card.id, p.by);
    match = { card: findCard(s, match.card.id), archived: false, restored: true };
  }
  const card = match.card;
  if (!card) fail('E_NOT_FOUND', `hardening card ${prepared.key} disappeared during restore`);
  if (card.phase === 'frozen' && p.by !== 'owner')
    fail('E_OWNER_LANE', `card #${card.num} is frozen — owner-only until the owner moves it out`);
  if (p.phase === 'done' || p.phase === 'frozen')
    fail('E_INVALID', 'hardening runner cannot close or freeze a card');
  const reopen = card.phase === 'done' || match.restored;
  const patch = { by: p.by, priority: card.priority === 'P0' ? 'P0' : prepared.priority };
  if (p.title !== undefined) patch.title = prepared.title;
  if (p.phase !== undefined && !reopen) patch.phase = p.phase;
  if (reopen) patch.phase = 'building';
  if (p.hardeningFixture !== undefined) patch.hardeningFixture = prepared.fixture;
  if (Object.keys(patch).length > 1)
    updateCard(s, card.id, patch, config);
  applyHardeningMetadata(s, card, prepared, p.by, reopen ? 'reopened' : 'updated');
  return { card, action: reopen ? 'reopened' : 'updated', archived: !!match.restored };
}

export function updateHardeningCard(s, ref, p, config, history = emptyHistory()) {
  return addOrUpdateHardeningCard(s, { ...p, ref }, config, history);
}

export const CARD_FIELDS = ['title', 'body', 'kind', 'track', 'epoch', 'milestoneId', 'phase', 'priority', 'plan', 'checkSteps', 'probe', 'visualMedia', 'blockedBy', 'workOrder', 'criteria', 'needsAcceptance', 'refs', 'tags', 'parentId', 'hardeningFixture', 'hardeningState'];

// D-TWR-CRIT1=C / D-TWRGUARD1=C: gate --phase done. Agent closure needs a
// nonempty checklist with every row met or verified on durable evidence. A
// refusal lists every failing criterion with its reason, so one read shows all
// that is left. Owner writes keep the legacy bypass and audit event.
// needsAcceptance remains transport-hard.
function applyDoneGate(s, c, targetPhase, by, criteria = c.criteria, needsAcceptance = c.needsAcceptance, config = {}) {
  if (targetPhase !== 'done') return null;
  if (needsAcceptance && by === 'owner')
    fail('E_ACCEPTANCE_OWNER_UI', `card #${c.num} requires owner verification — caller-supplied by:owner cannot close it; use the dedicated owner verification UI`);
  const fixtureIssue = hardeningFixtureIssue(c, config);
  if (fixtureIssue) fail('E_HARDENING_FIXTURE', fixtureIssue);
  const items = criteria || [];
  const unsettled = items.filter(i => !['met', 'verified'].includes(i.status));
  if (by !== 'owner' && !items.length)
    fail('E_CRITERIA', `card #${c.num} needs at least one exit criterion before an agent can close it`);
  if (by !== 'owner') {
    const failing = items.map(i => {
      if (unsettled.includes(i)) return `c${i.n}: not met — ${shorten(i.text, 120)}`;
      const issue = criterionDurabilityIssue(s, c, i.closure || 'code', i.evidence, config, null);
      return issue && `c${i.n}: ${issue}`;
    }).filter(Boolean);
    if (failing.length)
      fail(unsettled.length ? 'E_CRITERIA' : 'E_EVIDENCE',
        `card #${c.num} cannot close: ${failing.length} of ${items.length} criteria fail\n  ${failing.join('\n  ')}`);
  }
  // A closed card leaves the board view, which strands any ballot still open on
  // it: the owner can no longer ask about it from the ballot deck. The card's own
  // acceptance ballot is the closing handoff itself, so it does not count.
  const openBallots = by === 'owner' ? [] : (s.decisions || []).filter(d => d.cardId === c.id && d.status === 'open' && !d.draft && d.group !== 'acceptance');
  if (openBallots.length)
    fail('E_CRITERIA', `card #${c.num} still has open ballots (${openBallots.map(d => d.id).join(', ')}); close it after the owner decides them`);
  if (by === 'owner' && (!items.length || unsettled.length)) {
    logEvent(s, { by, action: 'card.criteria-bypass', ref: c.id, note: 'owner bypass' });
    return null;
  }
  if (needsAcceptance) {
    mintAcceptance(s, c);
    return 'verify';
  }
  return null;
}

// A needsAcceptance card parked in review with every criterion met or verified is the
// same owner handoff as asking for done — without this the owner's Accept
// button stays disabled forever, because the ballot only minted on a `--phase
// done` attempt and agents park in verify directly. Captured media may be
// attached earlier, but evidence never bypasses incomplete machine criteria.
function maybeMintAcceptance(s, c) {
  const items = c.criteria || [];
  if (!c.needsAcceptance || c.phase !== 'verify' || !items.length) return;
  if (items.every(i => ['met', 'verified'].includes(i.status))) mintAcceptance(s, c);
}

// Acceptance is an evidence review, never an instruction to run commands.
const PROOF_LINE_MAX = 100;
const shorten = (s, n = PROOF_LINE_MAX) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

export function acceptanceCheckInstructions(c) {
  const items = c.criteria || [];
  // Every criterion, including the ones still open: a request that hides its
  // open rows tells the owner the card is finished when it is not.
  const proof = items.map(i => shorten(
    i.status === 'open'
      ? `STILL OPEN — ${i.text}`
      : `${i.text} — ${i.status}${i.evidence ? ` (${i.evidence})` : ''}`,
  ));
  const visualCheck = c.needsAcceptance ? 'Review the captured evidence below.' : null;
  return (proof.length || visualCheck) ? { proof, visualCheck } : null;
}

function mintAcceptance(s, c) {
  const id = `D-ACCEPT-${c.num}`;
  const existing = s.decisions.find(d => d.id === id);
  const items = c.criteria || [];
  const evidence = items.length
    ? items.map(i => `${i.n}. ${i.text} — ${i.status}${i.evidence ? ` (${i.evidence})` : ''}${i.verifiedBy ? ` [verified by ${i.verifiedBy}]` : ''}`).join('\n')
    : '(no exit criteria on this card — direct acceptance request)';
  const checkInstructions = acceptanceCheckInstructions(c);
  if (existing && existing.status !== 'ratified') {
    existing.detail = evidence;
    existing.checkInstructions = checkInstructions;
    existing.visualMedia = normalizeVisualMedia(c.visualMedia);
    return;
  }
  if (existing) {
    // a prior round was bounced; re-open the same ballot id for round 2
    existing.status = 'open';
    existing.detail = evidence;
    existing.checkInstructions = checkInstructions;
    existing.visualMedia = normalizeVisualMedia(c.visualMedia);
    delete existing.outcome; delete existing.comment; delete existing.ratifiedAt;
  } else {
    addDecision(s, {
      [SYSTEM_ACCEPTANCE]: true,
      id, cardId: c.id, group: 'acceptance',
      title: `Accept #${c.num} — ${c.title}`,
      gist: `Close #${c.num}, or bounce it back to building.`,
      detail: evidence,
      checkInstructions,
      visualMedia: normalizeVisualMedia(c.visualMedia),
      options: [
        { key: 'accept', name: 'Accept — close the card' },
        { key: 'bounce', name: 'Bounce — back to building (comment why)' },
      ],
      by: 'agent',
    });
  }
  c.log.unshift({ at: today(), text: `Requested acceptance — minted ${id}.` });
}

// D-TWRGUARD1=C (#458): frozen cards are owner-only for any write — the
// owner unpauses one with a plain phase update (`--by owner` bypasses this
// guard). #516 removed the separate triage/activation gate: a fresh card
// lands straight in an agent lane, no owner greenlight step.
// Agent-hard, owner-soft: by === 'owner' bypasses this check outright.
function assertOwnerLane(c, patch, by) {
  if (by === 'owner') return;
  if (c.phase === 'frozen')
    fail('E_OWNER_LANE', `card #${c.num} is frozen — owner-only until the owner moves it out (\`tower card update --phase ... --by owner\`)`);
}

export function updateCard(s, ref, patch, config) {
  const c = mustCard(s, ref);
  const oldPhase = c.phase;
  const oldMilestoneId = c.milestoneId;
  if (HARDENING_IDENTITY_FIELDS.some(key => Object.hasOwn(patch, key)))
    fail('E_INVALID', 'hardening identity changes must use the atomic hardening card upsert');
  if (c.hardeningDedupKey && Object.hasOwn(patch, 'body'))
    patch.body = [String(c.body || '').trim(), String(patch.body || '').trim()].filter(Boolean).join('\n\n');
  // Basic shape validation runs before the owner-lane authorization check, so
  // a malformed request always reports E_INVALID/E_NOT_FOUND regardless of
  // who sent it or what lane the card is in.
  checkEnum(patch.kind, config.kinds, 'kind');
  checkEnum(patch.track, config.tracks, 'track');
  checkEnum(patch.priority, config.priorities, 'priority');
  checkEnum(patch.phase, PHASE_IDS, 'phase');
  if ('epoch' in patch) checkEpoch(s, patch.epoch);
  if ('milestoneId' in patch) checkMilestone(s, patch.milestoneId);
  checkCardHome({
    track: 'track' in patch ? patch.track : c.track,
    epoch: 'epoch' in patch ? patch.epoch : c.epoch,
  });
  checkCardMilestone(s, {
    epoch: 'epoch' in patch ? patch.epoch : c.epoch,
    track: 'track' in patch ? patch.track : c.track,
    milestoneId: 'milestoneId' in patch ? patch.milestoneId : c.milestoneId,
    linking: 'milestoneId' in patch && patch.milestoneId !== c.milestoneId,
  });
  if ('blockedBy' in patch) patch.blockedBy = normalizeBlockedBy(s, patch.blockedBy, { rejectUnknown: true });
  if ('refs' in patch) checkRefs(patch.refs);
  if ('probe' in patch) validateStoredString(patch.probe, 'card.probe', { nullable: true, nonEmpty: true });
  if ('visualMedia' in patch) patch.visualMedia = normalizeVisualMedia(patch.visualMedia);
  if ('tags' in patch) patch.tags = normalizeTags(patch.tags);
  if ('workOrder' in patch) {
    const workOrder = patch.workOrder == null || patch.workOrder === '' ? undefined : Number(patch.workOrder);
    validateStoredNumber(workOrder, 'card.workOrder', { min: 0 });
    patch.workOrder = workOrder;
  }
  if ('parentId' in patch || 'parent' in patch) {
    const resolved = resolveParentId(s, 'parentId' in patch ? patch.parentId : patch.parent);
    if (resolved === c.id) fail('E_INVALID', 'card cannot parent itself');
    patch.parentId = resolved;
  }
  if ('hardeningFixture' in patch) {
    try { patch.hardeningFixture = normalizeHardeningFixture(patch.hardeningFixture, c.hardeningFindingId); }
    catch (error) {
      if (error instanceof HardeningInputError) fail(error.code, error.message);
      throw error;
    }
  }
  // Incremental tag edits (CLI --add-tag / --remove-tag) compose onto current tags.
  if ('addTags' in patch || 'removeTags' in patch) {
    const cur = new Set(c.tags || []);
    for (const t of normalizeTags(patch.addTags)) cur.add(t);
    for (const t of normalizeTags(patch.removeTags)) cur.delete(t);
    patch.tags = [...cur];
  }
  const openAcceptance = s.decisions.find(d => d.cardId === c.id && d.group === 'acceptance' && d.status !== 'ratified');
  if (openAcceptance && 'needsAcceptance' in patch && !(patch.needsAcceptance === true || patch.needsAcceptance === 'true'))
    fail('E_ACCEPTANCE_OWNER_UI', `${openAcceptance.id} is open — needsAcceptance cannot be cleared to bypass owner verification`);
  assertOwnerLane(c, patch, patch.by);
  const overLimit = Array.isArray(patch.criteria)
    ? admitCriteria(config, liveCriteriaCount(c), liveCriteriaCount(patch), patch.overLimit, `card #${c.num}`)
    : null;
  if ('criteria' in patch) {
    assertCriterionText(patch.criteria);
    invalidateCardMilestone(s, c);
  }
  const candidateCriteria = 'criteria' in patch && Array.isArray(patch.criteria)
    ? patch.criteria.map((it, i) => normalizeCriterion(it, i))
    : c.criteria;
  const candidateAcceptance = 'needsAcceptance' in patch
    ? patch.needsAcceptance === true || patch.needsAcceptance === 'true'
    : c.needsAcceptance;
  const candidateCard = {
    ...c,
    ...(Object.hasOwn(patch, 'hardeningFixture') ? { hardeningFixture: patch.hardeningFixture } : {}),
  };
  const phaseOverride = 'phase' in patch
    ? applyDoneGate(s, candidateCard, patch.phase, patch.by, candidateCriteria, candidateAcceptance, config)
    : null;
  for (const k of CARD_FIELDS) {
    if (k in patch) {
      if (k === 'phase') c.phase = phaseOverride || patch.phase;
      else if (k === 'workOrder') c[k] = patch[k];
      else if (k === 'needsAcceptance') c.needsAcceptance = patch.needsAcceptance === true || patch.needsAcceptance === 'true';
      else if (k === 'criteria') c.criteria = Array.isArray(patch.criteria) ? patch.criteria.map((it, i) => normalizeCriterion(it, i)) : c.criteria;
      else if (k === 'tags') c.tags = patch.tags;
      else c[k] = patch[k];
    }
  }
  if (c.phase === 'verify' && (oldPhase !== 'verify' || 'visualMedia' in patch)) maybeMintAcceptance(s, c);
  if (c.assignee && c.assignee === patch.by) c.claimedAt = now();
  if (c.phase === 'done' || c.phase === 'frozen') {
    c.assignee = null;
    delete c.claimedAt;
  }
  if (oldPhase !== 'done' && c.phase === 'done') c.completedAt = now();
  if (oldPhase === 'done' && c.phase !== 'done') delete c.completedAt;
  if (c.hardeningDedupKey && c.phase === 'done') c.hardeningState = 'fixed';
  if (patch.logEntry) c.log.unshift({ at: today(), by: patch.by || 'agent', text: patch.logEntry });
  touchCard(c, patch.by);
  syncMilestones(s, [oldMilestoneId, c.milestoneId]);
  logEvent(s, { by: patch.by, action: 'card.update', ref: c.id, note: Object.keys(patch).filter(k => !['id', 'by', 'overLimit'].includes(k)).join(',') });
  noteOverLimit(s, c, overLimit, patch.by);
  return c;
}

// ---- mutations: exit criteria ----------------------------------------------

// A card's size is the rows still on it: a row moved to another card stays
// behind only as a pointer and does not count.
export const liveCriteriaCount = (card) => (card?.criteria || []).filter(i => !i?.movedTo).length;

export function criteriaSoftLimit(config) {
  const limit = config?.criteriaSoftLimit ?? DEFAULTS.criteriaSoftLimit;
  if (!Number.isInteger(limit) || limit < 1)
    fail('E_INVALID', `config criteriaSoftLimit must be a positive whole number, not ${JSON.stringify(limit)}`);
  return limit;
}

// Growing a card past the soft limit takes a reason. Returns what to log, or
// null when the card stays within the limit or does not grow.
function admitCriteria(config, before, after, overLimit, what) {
  const limit = criteriaSoftLimit(config);
  if (after <= limit || after <= before) return null;
  const reason = typeof overLimit === 'string' ? overLimit.trim() : '';
  if (!reason)
    fail('E_OVER_LIMIT', `${what} would hold ${after} criteria, over the soft limit of ${limit} — split it with \`tower card split\`, or give --over-limit "reason"`);
  return { count: after, limit, reason };
}

function noteOverLimit(s, card, over, by) {
  if (over) logEvent(s, { by, action: 'card.over-limit', ref: card.id, note: `${over.count} criteria, soft limit ${over.limit}: ${over.reason}` });
}

export function addCriterion(s, ref, text, by, { proof, blockedBy, overLimit } = {}, config = {}) {
  const c = mustCard(s, ref);
  if (!text || !String(text).trim()) fail('E_INVALID', 'criterion needs text');
  const checked = proof === undefined ? null : proofInput(proof);
  const over = admitCriteria(config, liveCriteriaCount(c), liveCriteriaCount(c) + 1, overLimit, `card #${c.num}`);
  c.criteria ||= [];
  const n = nextCriterionNumber(c);
  const blocker = blockedBy === undefined ? null : criterionBlockerInput(s, c, n, blockedBy);
  const item = { n, text: String(text).trim(), status: 'open', metBy: null, verifiedBy: null, evidence: '', at: now() };
  if (checked) item.proof = checked;
  if (blocker) item.blockedBy = blocker;
  c.criteria.push(item);
  invalidateCardMilestone(s, c);
  touchCard(c, by);
  logEvent(s, { by, action: 'card.criteria-add', ref: c.id, note: `#${n} ${item.text.slice(0, 60)}${blocker ? ` (blocked by ${blocker})` : ''}` });
  noteOverLimit(s, c, over, by);
  return { ...item, cardId: c.id, cardNum: c.num };
}

// A criterion waits on a card (`#M`), another criterion (`#M cK`) or a
// decision (`D-…`), stored in that form. The wait ends when the card is done,
// the criterion settles or the decision is ratified; a blocker gone from the
// live board (archived) has ended too.
const BLOCKER_REF = /^(?:#(\d+)(?: c(\d+))?|(D-[A-Za-z0-9][\w.-]*))$/;

function blockerResolved(ref, cardsByNum, decisionsById) {
  const [, num, n, decision] = BLOCKER_REF.exec(ref);
  if (decision) return (decisionsById.get(decision)?.status ?? 'ratified') === 'ratified';
  const card = cardsByNum.get(Number(num));
  if (!card || card.phase === 'done') return true;
  if (n === undefined) return false;
  return (card.criteria || []).find(i => i.n === Number(n))?.status !== 'open';
}

// The load step that reports a resolved wait: the row keeps the ref it waited
// on as `unblockedFrom`. A settled row has nothing left to wait for.
function releaseResolvedBlockers(s) {
  if (!s.cards.some(c => (c.criteria || []).some(i => i.blockedBy))) return;
  const cardsByNum = new Map(s.cards.map(c => [c.num, c]));
  const decisionsById = new Map(s.decisions.map(d => [d.id, d]));
  for (const c of s.cards) {
    for (const item of c.criteria || []) {
      if (!item.blockedBy) continue;
      if (item.status === 'open' && !blockerResolved(item.blockedBy, cardsByNum, decisionsById)) continue;
      if (item.status === 'open') item.unblockedFrom = item.blockedBy;
      delete item.blockedBy;
    }
  }
}

// The canonical blocker ref for criterion n of card c, refused when it names
// nothing live to wait on or would close a waiting cycle.
function criterionBlockerInput(s, c, n, raw) {
  const text = String(raw ?? '').trim();
  const numbered = /^#?(\d+)(?:\s*c(\d+))?$/i.exec(text);
  const ref = numbered ? `#${Number(numbered[1])}${numbered[2] ? ` c${Number(numbered[2])}` : ''}`
    : /^D-[A-Za-z0-9][\w.-]*$/.test(text) ? text
    : fail('E_INVALID', `--blocked-by takes '#M', '#M cK' or a decision id (got "${text}")`);
  const [, num, k, decision] = BLOCKER_REF.exec(ref);
  if (decision) {
    const d = s.decisions.find(x => x.id === decision) || fail('E_NOT_FOUND', `no decision ${decision}`);
    if (d.status === 'ratified') fail('E_INVALID', `decision ${decision} is already ratified; nothing to wait on`);
    return ref;
  }
  const target = s.cards.find(x => x.num === Number(num)) || fail('E_NOT_FOUND', `no live card #${num}`);
  if (target.phase === 'done') fail('E_INVALID', `card #${target.num} is done; nothing to wait on`);
  if (k === undefined) {
    if (target === c) fail('E_INVALID', `criterion #${n} cannot wait on its own card #${c.num}`);
    return ref;
  }
  let item = mustCriterion(target, k);
  if (item.status !== 'open') fail('E_INVALID', `criterion #${target.num} c${item.n} is already ${item.status}; nothing to wait on`);
  const seen = new Set();
  for (let card = target; item; ) {
    const at = `#${card.num} c${item.n}`;
    if (card === c && item.n === Number(n)) fail('E_INVALID', `criterion #${c.num} c${n} would wait on itself through ${ref}`);
    if (seen.has(at)) break;
    seen.add(at);
    const next = item.blockedBy && BLOCKER_REF.exec(item.blockedBy);
    if (!next || next[2] === undefined) break;
    card = s.cards.find(x => x.num === Number(next[1]));
    item = card && (card.criteria || []).find(i => i.n === Number(next[2]));
  }
  return ref;
}

// End a criterion's wait by hand, for a blocker that no longer applies.
export function unblockCriterion(s, ref, n, { reason, by } = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'unblock needs --by <agent>');
  if (!item.blockedBy) fail('E_INVALID', `criterion #${item.n} on card #${c.num} is not blocked`);
  const was = item.blockedBy;
  delete item.blockedBy;
  delete item.unblockedFrom;
  touchCard(c, by);
  const why = reason == null ? '' : String(reason).trim();
  logEvent(s, { by, action: 'card.criteria-unblock', ref: c.id, note: `#${item.n}: was blocked by ${was}${why ? `; ${why}` : ''}` });
  return { ...item, unblockedFrom: was, cardId: c.id, cardNum: c.num };
}

// Criterion numbers are never reused: a removed row's number stays retired, so
// evidence, logs and briefs that cite `cN` keep pointing at one criterion.
function nextCriterionNumber(holder, list = holder.criteria) {
  const highest = Math.max(0, holder.criteriaLastN || 0, ...(list || []).map(i => i.n));
  holder.criteriaLastN = highest + 1;
  return highest + 1;
}

function needsReason(verb, reason) {
  if (!reason || !String(reason).trim()) fail('E_INVALID', `${verb} needs --reason <text>`);
  return String(reason).trim();
}

// Rewording a criterion changes what it asks for, so a met or verified row
// reopens: its evidence proved the old wording, not the new one.
function rewordCriterion(item, text) {
  const next = String(text ?? '').trim();
  if (!next) fail('E_INVALID', 'criterion needs text');
  if (next === item.text) fail('E_INVALID', `criterion #${item.n} already has that text`);
  const before = item.text;
  const reopened = item.status !== 'open';
  item.text = next;
  if (reopened) {
    item.status = 'open';
    item.metBy = null;
    item.verifiedBy = null;
    item.evidence = '';
    delete item.closure;
    delete item.movedTo;
  }
  item.at = now();
  return { before, reopened };
}

// Text, proof and blocker change separately. A new proof says how the row is
// checked and a blocker what it waits on, not what it asks for, so neither
// changes the row's status or needs a reason.
export function editCriterion(s, ref, n, { text, proof, blockedBy, reason, by } = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'edit needs --by <agent>');
  if (text === undefined && proof === undefined && blockedBy === undefined)
    fail('E_INVALID', 'edit needs --text "…", --proof "jet test …" or --blocked-by REF');
  const why = text === undefined ? (reason == null ? '' : String(reason).trim()) : needsReason('edit', reason);
  const nextProof = proof === undefined ? undefined : proofInput(proof);
  const { before, reopened } = text === undefined ? { before: item.text, reopened: false } : rewordCriterion(item, text);
  if (blockedBy !== undefined) {
    if (item.status !== 'open') fail('E_INVALID', `criterion #${item.n} on card #${c.num} is ${item.status}; only an open criterion waits on a blocker`);
    item.blockedBy = criterionBlockerInput(s, c, item.n, blockedBy);
    delete item.unblockedFrom;
  }
  if (nextProof === null) delete item.proof;
  else if (nextProof) item.proof = nextProof;
  invalidateCardMilestone(s, c);
  touchCard(c, by);
  const changes = [
    text === undefined ? null : `was: ${before.slice(0, 200)}`,
    proof === undefined ? null : `proof: ${nextProof ? proofCommand(nextProof) : 'none'}`,
    blockedBy === undefined ? null : `blocked by ${item.blockedBy}`,
  ].filter(Boolean).join('; ');
  logEvent(s, { by, action: 'card.criteria-edit', ref: c.id, note: `#${item.n}: ${why ? `${why}${reopened ? ' (reopened)' : ''}; ` : ''}${changes}` });
  return { ...item, reopened, cardId: c.id, cardNum: c.num };
}

export function removeCriterion(s, ref, n, { reason, by } = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'remove needs --by <agent>');
  const why = needsReason('remove', reason);
  c.criteriaLastN = Math.max(c.criteriaLastN || 0, ...c.criteria.map(i => i.n));
  c.criteria = c.criteria.filter(i => i !== item);
  invalidateCardMilestone(s, c);
  touchCard(c, by);
  logEvent(s, { by, action: 'card.criteria-remove', ref: c.id, note: `#${item.n}: ${why}; was [${item.status}]: ${item.text.slice(0, 200)}` });
  return { ...item, removed: true, cardId: c.id, cardNum: c.num };
}

// Moving a criterion re-homes its work: the text becomes a new open criterion
// on the target card, and the source row closes as a duplicate that points at
// it. Both rows carry the link (movedTo / movedFrom), so either side leads to
// the other.
export function moveCriterion(s, ref, n, { to, reason, by, overLimit } = {}, config = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'move needs --by <agent>');
  if (to == null || to === '' || to === true) fail('E_INVALID', 'move needs --to <card>');
  const target = mustCard(s, to);
  if (target === c) fail('E_INVALID', `criterion #${item.n} is already on card #${c.num}`);
  assertMovable(c, item);
  if (target.phase === 'done') fail('E_INVALID', `card #${target.num} is done; reopen it before moving work onto it`);
  const over = admitCriteria(config, liveCriteriaCount(target), liveCriteriaCount(target) + 1, overLimit, `card #${target.num}`);
  const moved = relocateCriterion(s, c, item, target, reason == null ? '' : String(reason).trim(), by);
  noteOverLimit(s, target, over, by);
  return {
    from: { ...item, cardId: c.id, cardNum: c.num },
    to: { ...moved, cardId: target.id, cardNum: target.num },
  };
}

function assertMovable(c, item) {
  if (item.status !== 'open')
    fail('E_INVALID', `criterion #${item.n} on card #${c.num} is ${item.status}; only an open criterion moves`);
}

function relocateCriterion(s, c, item, target, why, by) {
  target.criteria ||= [];
  const k = nextCriterionNumber(target);
  const moved = {
    n: k, text: item.text, status: 'open', metBy: null, verifiedBy: null, evidence: '', at: now(),
    movedFrom: { card: c.num, n: item.n },
  };
  if (item.proof) moved.proof = checkProof(item.proof);
  if (item.blockedBy) moved.blockedBy = item.blockedBy;
  target.criteria.push(moved);
  item.status = 'met';
  item.metBy = by;
  item.verifiedBy = null;
  item.evidence = `Moved to #${target.num} c${k}`;
  item.closure = 'duplicate';
  item.movedTo = { card: target.num, n: k };
  item.at = now();
  for (const card of [c, target]) {
    invalidateCardMilestone(s, card);
    touchCard(card, by);
  }
  maybeMintAcceptance(s, c);
  logEvent(s, { by, action: 'card.criteria-move', ref: c.id, note: `#${item.n} → #${target.num} c${k}${why ? `: ${why}` : ''}` });
  logEvent(s, { by, action: 'card.criteria-add', ref: target.id, note: `#${k} moved from #${c.num} c${item.n}` });
  return moved;
}

// Splitting a card moves some of its open criteria onto a new card in the same
// epoch, milestone and track. Each row moves as `move` would, so both cards
// keep the link; the new card's body says where it came from.
export function splitCard(s, ref, { criteria, title, by, overLimit } = {}, config = {}) {
  const c = mustCard(s, ref);
  if (!by || by === true) fail('E_INVALID', 'split needs --by <agent>');
  if (!title || title === true || !String(title).trim()) fail('E_INVALID', 'split needs --title "…" for the new card');
  const raw = Array.isArray(criteria) ? criteria : String(criteria ?? '').split(',');
  const ns = raw.map(v => String(v).trim().replace(/^c/, '')).filter(Boolean).map(Number);
  if (!ns.length || ns.some(n => !Number.isInteger(n) || n < 1))
    fail('E_INVALID', 'split needs --criteria N,N,… (criterion numbers on the card)');
  const repeated = ns.find((n, i) => ns.indexOf(n) !== i);
  if (repeated) fail('E_INVALID', `criterion #${repeated} is listed twice`);
  if (c.phase === 'done') fail('E_INVALID', `card #${c.num} is done; reopen it before splitting it`);
  const items = ns.map(n => mustCriterion(c, n));
  for (const item of items) assertMovable(c, item);
  const over = admitCriteria(config, 0, items.length, overLimit, 'the split-off card');
  const card = addCard(s, {
    title, body: `Split from #${c.num}.`, kind: c.kind, track: c.track, epoch: c.epoch,
    milestoneId: c.milestoneId, priority: c.priority, by,
  }, config);
  const moved = items.map(item => ({ from: item.n, to: relocateCriterion(s, c, item, card, `split into #${card.num}`, by).n }));
  logEvent(s, { by, action: 'card.split', ref: c.id, refs: [c.id, card.id], note: `${ns.map(n => `c${n}`).join(', ')} → #${card.num} ${card.title.slice(0, 60)}` });
  noteOverLimit(s, card, over, by);
  return { from: { id: c.id, num: c.num }, card: { id: card.id, num: card.num, title: card.title, epoch: card.epoch }, moved };
}

function mustCriterion(c, n) {
  const item = (c.criteria || []).find(i => i.n === Number(n));
  if (!item) fail('E_NOT_FOUND', `no criterion #${n} on card #${c.num}`);
  return item;
}

// #6563 c3: why a criterion's evidence does not keep the closure promise, or
// null when it does. A `code` row must name a commit reachable from a durable
// branch (durable.mjs). The other kinds close without code: `decision` names a
// ratified decision, `duplicate` names the card it duplicates or merged into,
// `obsolete` gives the reason. No kind may rest on scratch output alone. With
// history null (the done gate) the decision and card lookups are skipped: they
// ran when the row was met, and the target may since have retired.
function criterionDurabilityIssue(s, c, kind, evidence, config, history) {
  if (!CLOSURE_KINDS.includes(kind))
    return `unknown closure kind "${kind}" (use ${CLOSURE_KINDS.join(', ')})`;
  const text = String(evidence || '').trim();
  const resolver = config?.durability;
  const active = !!resolver?.active();
  const commits = active ? resolver.commits(text) : [];
  const durable = commits.some(found => found.ref);
  const branches = active ? resolver.refs().join(' or ') : '';
  if (kind === 'code') {
    if (!active || durable) return null;
    if (commits.length)
      return `commit ${commits.map(found => found.token).join(', ')} is not on ${branches}; land it, then cite the landed commit`;
    if (SCRATCH_PATH_RE.test(text))
      return `evidence cites only scratch output, which is throwaway; land the work and cite its commit on ${branches}`;
    return `evidence names no commit on ${branches}; cite the landed commit, or declare --kind decision, duplicate or obsolete`;
  }
  if (!text) return `--kind ${kind} needs --evidence`;
  if (SCRATCH_PATH_RE.test(text) && !durable)
    return `evidence cites scratch output, which is throwaway; land the work and cite its commit${branches ? ` on ${branches}` : ''}`;
  if (history === null) return null;
  if (kind === 'decision') {
    const decisions = [...(s.decisions || []), ...(history?.decisions || [])];
    const ids = text.match(/\bD-[A-Za-z0-9][A-Za-z0-9-]*/g) || [];
    if (!ids.some(id => decisions.some(d => d.id === id && d.status === 'ratified')))
      return '--kind decision evidence must name the ratified decision (D-...) that settles it';
  }
  if (kind === 'duplicate') {
    const cards = [...(s.cards || []), ...(history?.cards || [])];
    const nums = [...text.matchAll(/#(\d+)\b/g)].map(m => Number(m[1])).filter(num => num !== c.num);
    if (!nums.some(num => cards.some(card => card.num === num)))
      return '--kind duplicate evidence must name the card (#N) this one duplicates or merged into';
  }
  return null;
}

function assertCriterionDurable(s, c, n, kind, evidence, by, config, history) {
  if (by === 'owner') return;
  const issue = criterionDurabilityIssue(s, c, kind, evidence, config, history ?? {});
  if (issue) fail('E_EVIDENCE', `card #${c.num} criterion #${n}: ${issue}`);
}

// A recorded non-code closure kind; `code` is the default and is not stored.
function setClosureKind(item, kind) {
  if (kind === 'code') delete item.closure;
  else item.closure = kind;
}

export function meetCriterion(s, ref, n, { evidence, by, kind = 'code' } = {}, config = {}, history = null) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'meet needs --by <agent>');
  assertCriterionDurable(s, c, item.n, kind, evidence ?? item.evidence, by, config, history);
  item.status = 'met';
  item.metBy = by;
  if (evidence != null) item.evidence = evidence;
  setClosureKind(item, kind);
  item.at = now();
  invalidateCardMilestone(s, c);
  touchCard(c, by);
  maybeMintAcceptance(s, c);
  logEvent(s, { by, action: 'card.criteria-meet', ref: c.id, note: `#${item.n}` });
  return { ...item, cardId: c.id, cardNum: c.num };
}

export function verifyCriterion(s, ref, n, { evidence, by, kind } = {}, config = {}, history = null) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'verify needs --by <agent>');
  if (item.status === 'open') fail('E_INVALID', `criterion #${n} not met yet — meet it before verifying`);
  if (by === item.metBy) fail('E_CRITERIA_SELF', `criterion #${n} verifier must differ from the agent that marked it met (${by})`);
  const closure = kind ?? item.closure ?? 'code';
  assertCriterionDurable(s, c, item.n, closure, evidence ?? item.evidence, by, config, history);
  item.status = 'verified';
  item.verifiedBy = by;
  if (evidence != null) item.evidence = evidence;
  setClosureKind(item, closure);
  item.at = now();
  invalidateCardMilestone(s, c);
  touchCard(c, by);
  maybeMintAcceptance(s, c);
  logEvent(s, { by, action: 'card.criteria-verify', ref: c.id, note: `#${item.n}` });
  return { ...item, cardId: c.id, cardNum: c.num };
}

export function reopenCriterion(s, ref, n, { reason, by } = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'reopen needs --by <agent>');
  if (!reason || !String(reason).trim()) fail('E_INVALID', 'reopen needs --reason <text>');
  if (item.status === 'open') fail('E_INVALID', 'criterion #' + n + ' is already open');
  item.status = 'open';
  item.metBy = null;
  item.verifiedBy = null;
  item.evidence = '';
  delete item.closure;
  delete item.movedTo;
  item.at = now();
  invalidateCardMilestone(s, c);
  touchCard(c, by);
  logEvent(s, { by, action: 'card.criteria-reopen', ref: c.id, note: '#' + item.n + ': ' + String(reason).trim() });
  return { ...item, cardId: c.id, cardNum: c.num };
}

// D-TWRGUARD1=C (#458): a card with any ratified decision refuses delete — a
// ratified decision is durable record, never a casualty of tidying up. The
// only override is the owner's own Tower UI session (owner 2026-10-04): the
// server passes `ownerUi` provenance only for a same-origin request carrying
// the owner session cookie, and the owner must tick a separate box for every
// ratified decision (`p.deleteRatified`). CLI and agent callers, `--by owner`
// included, never carry that provenance, so for them the guard still holds:
// let the decisions retire to history.json (`tower archive status`), or
// `tower archive restore <id>` and re-detach, then delete.
//
// Open ballots go with the card. `p.decisions`, when given, is the ballot list
// the caller showed for confirmation; any drift since then is E_CONFLICT so a
// ballot added after the dialog opened is never deleted unseen. Every removed
// ballot is written to the event log with its status and outcome.
export function deleteCard(s, ref, p = {}, ownerUi = null) {
  const c = mustCard(s, ref);
  const oldMilestoneId = c.milestoneId;
  const openMessages = s.questions.filter(q => q.cardId === c.id && q.kind === 'message' && q.status === 'open');
  if (openMessages.length)
    fail('E_INVALID', `card #${c.num} has ${openMessages.length} open message${openMessages.length === 1 ? '' : 's'} — mark each message done before deleting the card`);
  const attached = s.decisions.filter(d => d.cardId === c.id);
  const attachedIds = new Set(attached.map(d => d.id));
  const owner = ownerUi?.kind === 'owner-ui' && !!ownerUi.session;
  const visibleIds = owner ? new Set(attached.filter(d => !d.draft).map(d => d.id)) : attachedIds;
  if (p.decisions != null) {
    const shown = new Set(idList(p.decisions));
    if (shown.size !== visibleIds.size || [...visibleIds].some(id => !shown.has(id)))
      fail('E_CONFLICT', `card #${c.num} ballots changed since the delete confirmation was shown (now: ${[...visibleIds].join(', ') || 'none'}) — review them and confirm again`);
  }
  const ratified = attached.filter(d => d.status === 'ratified');
  if (ratified.length) {
    const confirmed = new Set(idList(p.deleteRatified));
    const unconfirmed = ratified.filter(d => !confirmed.has(d.id));
    if (!owner || unconfirmed.length) {
      const ids = (owner ? unconfirmed : ratified).map(d => d.id).join(', ');
      fail('E_HAS_RATIFIED', owner
        ? `card #${c.num} still has ratified decision${unconfirmed.length > 1 ? 's' : ''} ${ids} — tick "also delete ratified decision" for each one to delete them with the card`
        : `card #${c.num} has ${ratified.length} ratified decision${ratified.length > 1 ? 's' : ''} (${ids}) — they retire to \`tower archive\` on their own once the buffer window passes; only the owner can delete them with the card, from the card's Delete button in the Tower UI`);
    }
  }
  const referencing = s.decisions.filter(d => !attachedIds.has(d.id) && attachedIds.has(d.supersededBy));
  if (referencing.length)
    fail('E_REFERENCED', `cannot delete card #${c.num}; its decisions are the supersession link of ${referencing.map(d => d.id).join(', ')}`);
  const by = owner ? 'owner' : p.by;
  s.cards = s.cards.filter(x => x.id !== c.id);
  s.decisions = s.decisions.filter(d => d.cardId !== c.id);
  s.questions = s.questions.filter(q => q.cardId !== c.id);
  for (const x of s.cards) x.blockedBy = (x.blockedBy || []).filter(id => id !== c.id && !attachedIds.has(id));
  syncMilestones(s, [oldMilestoneId]);
  const removed = attached.map(d => ({ id: d.id, title: d.title, status: d.status, outcome: d.outcome ?? null, ...(d.draft ? { draft: true } : {}) }));
  for (const d of removed)
    logEvent(s, { by, action: 'decision.delete', ref: d.id, draft: !!d.draft,
      note: `deleted with card #${c.num} — ${d.status}${d.status === 'ratified' ? ` (outcome ${d.outcome})` : ''}: ${d.title}` });
  const visibleRemoved = removed.filter(d => !d.draft);
  const withBallots = visibleRemoved.length ? ` — with ballot${visibleRemoved.length > 1 ? 's' : ''} ${visibleRemoved.map(d => `${d.id} (${d.status})`).join(', ')}` : '';
  logEvent(s, { by, action: 'card.delete', ref: c.id, note: `${c.title}${withBallots}${owner ? ` · owner-ui session=${ownerUi.session}` : ''}` });
  return { ok: true, id: c.id, num: c.num, decisions: owner ? visibleRemoved : removed };
}

const idList = (value) => (value == null ? [] : Array.isArray(value) ? value : String(value).split(','))
  .map(x => String(x).trim()).filter(Boolean);

// Owner-only gate used by ratify (D-TWRGUARD1=C #458). An agent may act "on
// behalf of" the owner by quoting his words verbatim — recorded in the event
// log note — otherwise refused.
function assertOwnerOr(by, quote, code, what) {
  if (by === 'owner') return null;
  if (!quote || !String(quote).trim()) fail(code, `${what} is owner-only — pass --quote "owner's words" if this is on his behalf`);
  return `by ${by}, quoting owner: "${quote}"`;
}

// Claims are renewable coordination leases, not durable ownership. An expired
// lease never blocks selection or takeover; normal writes by its holder renew
// it, and terminal/paused phases clear it.
export function hasActiveClaim(c, at = Date.now()) {
  if (!c?.assignee || !c.claimedAt) return false;
  const claimed = Date.parse(c.claimedAt);
  return Number.isFinite(claimed) && at - claimed < CLAIM_TTL_MS;
}

export function closureReadyClaims(s, at = Date.now()) {
  return s.cards.filter(c => {
    if (!hasActiveClaim(c, at) || c.phase === 'done' || c.phase === 'frozen' || c.needsAcceptance) return false;
    const criteria = c.criteria || [];
    if (!criteria.length || !criteria.every(item => ['met', 'verified'].includes(item.status))) return false;
    if (s.questions.some(q => q.cardId === c.id && q.kind !== 'message' && q.status === 'open')) return false;
    const lane = laneOf(c, s.decisions, s.cards).lane;
    return lane !== 'blocked' && lane !== 'decide';
  });
}

export function assertClosureBarrier(s) {
  const ready = closureReadyClaims(s);
  if (!ready.length) return;
  const refs = ready.map(c => `#${c.num}`).join(', ');
  fail('E_CLOSE_READY', `${refs} ${ready.length === 1 ? 'has' : 'have'} an active lease, all criteria met, and no open gate — close, reopen, or block ${ready.length === 1 ? 'it' : 'them'} before any further claim or brief`);
}

export function claimCard(s, ref, by) {
  const c = mustCard(s, ref);
  if (!by) fail('E_INVALID', 'claim needs --by <agent>');
  if (by !== 'owner' && c.phase === 'frozen')
    fail('E_OWNER_LANE', `card #${c.num} is frozen — owner-only until the owner moves it out (\`tower card update --phase ... --by owner\`)`);
  assertClosureBarrier(s);
  if (hasActiveClaim(c) && c.assignee !== by)
    fail('E_CLAIMED', `card #${c.num} has an active work lease held by ${c.assignee} — pick another or release it first`);
  c.assignee = by; c.claimedAt = now(); touchCard(c, by);
  logEvent(s, { by, action: 'card.claim', ref: c.id });
  return c;
}
// D-TWRGUARD1=C (#458): releasing a card mid-`building` without a handoff
// note leaves the next agent to restart from zero — require one from agents.
export function releaseCard(s, ref, by, handoff) {
  const c = mustCard(s, ref);
  if (by !== 'owner' && c.phase === 'building') {
    if (!handoff || !String(handoff).trim())
      fail('E_HANDOFF', `releasing #${c.num} while building needs --handoff "what's done, what's left, gotchas" so the next agent doesn't restart from zero`);
    c.log.unshift({ at: today(), by, text: `[handoff] ${handoff}` });
  }
  c.assignee = null; delete c.claimedAt; touchCard(c, by);
  logEvent(s, { by, action: 'card.release', ref: c.id, note: handoff ? '[handoff] logged' : '' });
  return c;
}

// ---- mutations: decisions --------------------------------------------------

const PLAIN_SENTENCE_WORDS = 32;
const PLAIN_PARAGRAPH_WORDS = 90;
// Stored review-pass layouts by ballot process. Earlier layouts are historical
// evidence: they keep their stored keys and render unchanged, and Tower checks
// only that they hold no unknown pass. Process 2 stored six passes; process 3
// added the reading surface; process 4 added structured losses and stored two
// readers. Process 5 (owner rule 2026-10-28) records one fresh adversarial pass.
const LEGACY_REVIEW_PASS_KEYS = ['base', 'boilOcean', 'hybrid', 'adversarial'];
const SIX_PASS_REVIEW_KEYS = ['base', 'boilOcean', 'hybrid', 'cooperative', 'beginner', 'adversarial'];
const TWO_READER_REVIEW_KEYS = ['beginner', 'adversarial'];
const CURRENT_REVIEW_PASS_KEYS = ['adversarial'];
const SIX_PASS_PROCESS_VERSION = 2;
const SURFACE_PROCESS_VERSION = 3;
const STRUCTURED_LOSS_PROCESS_VERSION = 4;
const BALLOT_PROCESS_VERSION = 5;
const SYSTEM_ACCEPTANCE = Symbol('system acceptance');
const words = (text) => String(text || '').match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) || [];
const sentences = (text) => String(text || '').trim().split(/(?<=[.!?])\s+/).filter(Boolean);
const reviewPassKeysFor = (version) => {
  const v = Number(version || 0);
  if (v >= BALLOT_PROCESS_VERSION) return CURRENT_REVIEW_PASS_KEYS;
  if (v >= STRUCTURED_LOSS_PROCESS_VERSION) return TWO_READER_REVIEW_KEYS;
  if (v >= SIX_PASS_PROCESS_VERSION) return SIX_PASS_REVIEW_KEYS;
  return LEGACY_REVIEW_PASS_KEYS;
};
const orderedReviewPasses = (passes, version = BALLOT_PROCESS_VERSION) =>
  Object.fromEntries(reviewPassKeysFor(version).map(key => [key, passes?.[key]]));
// A family name may hold periods (`GPT-6.1 Sol`): the author family runs to
// `. Adversarial model family: `, and the adversarial family to
// `. Fresh agent: ` when the pass names its reviewer, else to the first period.
const dissentPrefix = /^Author model family: (.+?)\. Adversarial model family: /;
const freshReviewerPrefix = /^Fresh agent: ([^.]+)\./;

function dissentMetadata(pass) {
  if (typeof pass !== 'string') return null;
  const match = dissentPrefix.exec(pass);
  if (!match) return null;
  const rest = pass.slice(match[0].length);
  const named = rest.indexOf('. Fresh agent: ');
  const cut = named > 0 ? named : rest.indexOf('.');
  if (cut <= 0) return null;
  const body = rest.slice(cut + 1).trim();
  const reviewer = freshReviewerPrefix.exec(body);
  return {
    author: match[1].trim(),
    adversarial: rest.slice(0, cut).trim(),
    agent: reviewer?.[1].trim() || '',
    summary: reviewer ? body.slice(reviewer[0].length).trim() : body,
  };
}

function dissentMetadataGaps(p, { requireFresh = true } = {}) {
  const metadata = dissentMetadata(p.reviewPasses?.adversarial);
  if (!metadata?.author || !metadata.adversarial) return ['reviewPasses.adversarial (must begin with `Author model family: <family>. Adversarial model family: <family>.`)'];
  // Owner direction 2026-09-05: fresh reviewers may share the author's family.
  // Family labels record provenance; they do not establish independence.
  if (requireFresh && !metadata.agent)
    return ['reviewPasses.adversarial (model provenance must be followed by `Fresh agent: <agent-id>.`)'];
  return [];
}

const reviewPassSummary = (key, pass) =>
  key === 'adversarial' ? dissentMetadata(pass)?.summary || pass : pass;
function proseDensityGaps(label, text) {
  if (!text || !String(text).trim()) return [];
  const gaps = [];
  // Markdown code examples are not prose sentences. Keep the prose around them.
  let fence = null;
  const prose = String(text).split('\n').map(line => {
    const opener = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
    if (fence) {
      if (opener && opener[1][0] === fence[0] && opener[1].length >= fence.length && !opener[2].trim()) fence = null;
      return '';
    }
    if (opener && !(opener[1][0] === '`' && opener[2].includes('`'))) {
      fence = opener[1];
      return '';
    }
    return line;
  }).join('\n');
  const paragraphs = prose.trim().split(/\n\s*\n/);
  paragraphs.forEach((paragraph, pi) => {
    const paragraphWords = words(paragraph).length;
    if (paragraphWords > PLAIN_PARAGRAPH_WORDS)
      gaps.push(`${label} paragraph ${pi + 1} has ${paragraphWords} words (max ${PLAIN_PARAGRAPH_WORDS})`);
    const sentences = paragraph.split(/(?<=[.!?])\s+/).filter(Boolean);
    sentences.forEach((sentence, si) => {
      const sentenceWords = words(sentence).length;
      if (sentenceWords > PLAIN_SENTENCE_WORDS)
        gaps.push(`${label} sentence ${si + 1} has ${sentenceWords} words (max ${PLAIN_SENTENCE_WORDS})`);
    });
  });
  return gaps;
}

function mitigationGaps(label, item) {
  const gaps = [];
  if (Object.hasOwn(item, 'whyUnavoidable'))
    gaps.push(`${label} (use mitigation, not whyUnavoidable)`);
  if (typeof item.mitigation === 'string'
      && /^\s*(?:[-*+•]\s|\d+[.)]\s|#{1,6}\s|`{3,}|~{3,})/m.test(item.mitigation))
    gaps.push(`${label}.mitigation (plain prose, no lists, headings, or code blocks)`);
  return gaps;
}

function exampleProseDensityGaps(label, text) {
  if (!text || !String(text).trim()) return [];
  const gaps = [];
  String(text).split('\n').forEach((line, li) => {
    line.split(/(?<=[.!?])\s+/).filter(Boolean).forEach((sentence, si) => {
      const sentenceWords = words(sentence).length;
      if (sentenceWords > PLAIN_SENTENCE_WORDS)
        gaps.push(`${label} line ${li + 1} sentence ${si + 1} has ${sentenceWords} words (max ${PLAIN_SENTENCE_WORDS})`);
    });
  });
  return gaps;
}

export function plainLanguageGaps(p) {
  const gaps = [
    ...proseDensityGaps('title', p.title),
    ...proseDensityGaps('gist', p.gist),
    ...proseDensityGaps('lesson', p.lesson),
    ...proseDensityGaps('explainer', p.explainer),
    ...proseDensityGaps('story', p.story),
    ...exampleProseDensityGaps('inWild', p.inWild),
    ...proseDensityGaps('detail', p.detail),
  ];
  for (const option of p.options || []) {
    gaps.push(...proseDensityGaps(`option ${option?.key || '?'} name`, option?.name));
    gaps.push(...proseDensityGaps(`option ${option?.key || '?'}`, option?.detail));
    gaps.push(...proseDensityGaps(`option ${option?.key || '?'} technical`, option?.technical));
  }
  for (const comparison of p.comparisons || [])
    gaps.push(...proseDensityGaps(`comparison ${comparison?.lang || '?'}`, comparison?.note));
  const recommendation = p.recommendation || {};
  gaps.push(...proseDensityGaps('recommendation why', recommendation.why));
  gaps.push(...proseDensityGaps('recommendation tradeoff', recommendation.tradeoff));
  for (const rejected of recommendation.whyNot || [])
    gaps.push(...proseDensityGaps(`recommendation why not ${rejected?.key || '?'}`, rejected?.reason));
  for (const [i, item] of (Array.isArray(recommendation.losses) ? recommendation.losses : []).entries()) {
    const loss = plainObject(item) ? item.loss : item;
    const mitigation = plainObject(item) ? item.mitigation : '';
    gaps.push(...proseDensityGaps(`recommendation loss ${i + 1}`, loss));
    gaps.push(...proseDensityGaps(`recommendation loss ${i + 1} mitigation`, mitigation));
  }
  gaps.push(...proseDensityGaps('hybrid synthesis', p.hybrid?.synthesis));
  for (const item of p.hybrid?.harvest || []) {
    gaps.push(...proseDensityGaps(`hybrid ${item?.key || '?'} aspect`, item?.aspect));
    gaps.push(...proseDensityGaps(`hybrid ${item?.key || '?'} use`, item?.use));
  }
  for (const key of reviewPassKeysFor(p.ballotProcessVersion))
    gaps.push(...proseDensityGaps(`reviewPasses.${key}`, reviewPassSummary(key, p.reviewPasses?.[key])));
  if (typeof p.checkInstructions === 'string')
    gaps.push(...proseDensityGaps('checkInstructions', p.checkInstructions));
  return gaps;
}

// The reading surface: the short owner-facing layer the UI shows first. Caps are
// owner law (2026-09-02): one question, one plain lesson, a current / proposed /
// in-the-wild code trio, real gains and losses per option (never padded), why-not
// for every losing option, no project jargon, and the recommended option is
// always A, listed first, so it sits next to the current and in-the-wild code.
const SURFACE_MITIGATION_WORDS = 24;
const SURFACE_GIST_WORDS = 22;
const SURFACE_LESSON_WORDS = 70;
const SURFACE_WHY_WORDS = 40;
const SURFACE_SENTENCE_WORDS = 24;
const SURFACE_BULLET_WORDS = 14;
const SURFACE_TOTAL_WORDS = 430;
const SURFACE_CODE_LINES = 14;
const SURFACE_JARGON = /\b(ratchet|seam|facet|substrate|tier[- ]parity|ring [01]|RINGS)\b/i;
export function surfaceGaps(p, { requireRecFirst = false } = {}) {
  const s = p.surface;
  if (!plainObject(s)) return ['surface (object)'];
  const gaps = [];
  const text = (v) => (typeof v === 'string' ? v.trim() : '');
  const count = (v) => words(text(v)).length;
  const prose = [];
  const processVersion = Number(p.ballotProcessVersion || 0);
  const structuredLosses = processVersion >= STRUCTURED_LOSS_PROCESS_VERSION;
  const need = (label, v, max) => {
    if (!text(v)) { gaps.push(`surface.${label}`); return; }
    prose.push([label, text(v)]);
    if (max && count(v) > max) gaps.push(`surface.${label} has ${count(v)} words (max ${max})`);
  };
  need('gist', s.gist, SURFACE_GIST_WORDS);
  if (text(s.gist) && !/\?$/.test(text(s.gist))) gaps.push('surface.gist (must be a question)');
  need('lesson', s.lesson, SURFACE_LESSON_WORDS);
  if (text(s.lesson).split(/\n\s*\n/).length > 1) gaps.push('surface.lesson (one paragraph maximum)');
  const trio = plainObject(s.trio) ? s.trio : {};
  for (const side of ['current', 'wild']) {
    const pane = plainObject(trio[side]) ? trio[side] : {};
    need(`trio.${side}.note`, pane.note);
    if (!text(pane.code)) gaps.push(`surface.trio.${side}.code`);
  }
  if (!text(trio.wild?.lang)) gaps.push('surface.trio.wild.lang');
  const optionKeys = (Array.isArray(p.options) ? p.options : []).map(o => o?.key).filter(Boolean);
  const surfaceOptions = Array.isArray(s.options) ? s.options : [];
  if (surfaceOptions.map(o => o?.key).join('\u0000') !== optionKeys.join('\u0000'))
    gaps.push('surface.options (keys must match options[] in order)');
  for (const o of surfaceOptions) {
    const key = o?.key || '?';
    need(`options[${key}].name`, o?.name);
    need(`options[${key}].gist`, o?.gist);
    for (const list of ['gains', 'losses']) {
      const items = Array.isArray(o?.[list]) ? o[list] : [];
      if (items.length > 3 || (list === 'gains' && items.length < 1)) gaps.push(`surface.options[${key}].${list} (${list === 'gains' ? 'need 1-3 real items' : 'at most 3 real items'})`);
      items.forEach((item, i) => {
        need(`options[${key}].${list}[${i + 1}]`, item);
        if (count(item) > SURFACE_BULLET_WORDS) gaps.push(`surface.options[${key}].${list}[${i + 1}] has ${count(item)} words (max ${SURFACE_BULLET_WORDS})`);
      });
    }
    const code = text(o?.proposed?.code);
    if (!code) gaps.push(`surface.options[${key}].proposed.code`);
    else if (code.split('\n').length > SURFACE_CODE_LINES) gaps.push(`surface.options[${key}].proposed.code (max ${SURFACE_CODE_LINES} lines)`);
  }
  const r = plainObject(s.recommendation) ? s.recommendation : {};
  if (r.rec !== p.rec) gaps.push('surface.recommendation.rec (must equal rec)');
  // New ballots only (owner, 2026-09-02): the recommended option is A and listed first so it sits next to the current and in-the-wild code. Ballots already open keep their letters.
  if (requireRecFirst && optionKeys.length && (optionKeys[0] !== 'A' || p.rec !== 'A')) gaps.push('rec (new ballots list the recommended option first, as A)');
  need('recommendation.why', r.why, SURFACE_WHY_WORDS);
  for (const list of ['gains', 'losses']) {
    const items = Array.isArray(r[list]) ? r[list] : [];
    if (list === 'gains' && !items.length) gaps.push('surface.recommendation.gains');
    if (items.length > 3) gaps.push(`surface.recommendation.${list} (at most 3 real items)`);
    items.forEach((item, i) => {
      const label = `recommendation.${list}[${i + 1}]`;
      if (structuredLosses && list === 'losses') {
        const loss = text(item?.loss);
        const mitigation = text(item?.mitigation);
        if (!plainObject(item)) {
          const shown = text(item);
          gaps.push(`surface.${label}${shown ? ` "${shown}"` : ''} (must include mitigation)`);
        } else {
          gaps.push(...mitigationGaps(`surface.${label}`, item));
          if (!loss) gaps.push(`surface.${label}.loss (required)`);
          else {
            prose.push([`${label}.loss`, loss]);
            if (count(loss) >= SURFACE_BULLET_WORDS)
              gaps.push(`surface.${label}.loss has ${count(loss)} words (under ${SURFACE_BULLET_WORDS} words)`);
          }
          if (!mitigation) gaps.push(`surface.${label}.mitigation (required for "${loss || '?'}")`);
          else {
            prose.push([`${label}.mitigation`, mitigation]);
            if (count(mitigation) >= SURFACE_MITIGATION_WORDS)
              gaps.push(`surface.${label}.mitigation has ${count(mitigation)} words (under ${SURFACE_MITIGATION_WORDS} words)`);
          }
        }
      } else {
        need(label, item);
        if (count(item) > SURFACE_BULLET_WORDS) gaps.push(`surface.${label} has ${count(item)} words (max ${SURFACE_BULLET_WORDS})`);
      }
    });
  }
  const whyNot = Array.isArray(r.whyNot) ? r.whyNot : [];
  for (const key of optionKeys.filter(key => key !== p.rec)) {
    const item = whyNot.find(x => x?.key === key);
    if (!text(item?.reason)) gaps.push(`surface.recommendation.whyNot[${key}]`);
    else prose.push([`recommendation.whyNot[${key}]`, text(item.reason)]);
  }
  need('recommendation.tradeoff', r.tradeoff);
  let total = 0;
  for (const [label, value] of prose) {
    total += words(value).length;
    for (const sentence of sentences(value))
      if (words(sentence).length > SURFACE_SENTENCE_WORDS)
        gaps.push(`surface.${label} sentence has ${words(sentence).length} words (max ${SURFACE_SENTENCE_WORDS})`);
    const jargon = value.match(SURFACE_JARGON);
    if (jargon) gaps.push(`surface.${label} uses project jargon "${jargon[0]}"`);
  }
  if (total > SURFACE_TOTAL_WORDS) gaps.push(`surface prose has ${total} words (max ${SURFACE_TOTAL_WORDS})`);
  return gaps;
}

function shortBallotGaps(p, card, processVersion) {
  const gaps = [];
  if (processVersion >= STRUCTURED_LOSS_PROCESS_VERSION) {
    const tags = new Set([
      ...(Array.isArray(card?.tags) ? card.tags : []),
      ...(Array.isArray(p.cardTags) ? p.cardTags : []),
      ...(Array.isArray(p.card?.tags) ? p.card.tags : []),
    ].map(tag => String(tag).trim().toLowerCase()));
    const blocked = [];
    if (String(p.group || '').trim().toLowerCase() === 'syntax')
      blocked.push('syntax-group decisions require a full ballot');
    if (tags.has('carve-out'))
      blocked.push('cards tagged carve-out require a full ballot');
    if (tags.has('full'))
      blocked.push('cards tagged full require a full ballot');
    if (blocked.length) gaps.push(`short ballot refused: ${blocked.join('; ')}`);
    else if ((Array.isArray(p.options) ? p.options : []).length > 3
      && (!p.shortAuthorizedBy || !String(p.shortAuthorizedBy).trim()))
      gaps.push('shortAuthorizedBy (short profile without owner authorization is limited to one mechanism with at most three options)');
  } else if (!p.shortAuthorizedBy || !String(p.shortAuthorizedBy).trim()) {
    gaps.push('shortAuthorizedBy');
  }
  if (p.reviewPasses != null) gaps.push('short ballots must omit reviewPasses');
  return gaps;
}

// The situation summary is the first thing every ballot shows (owner ballot
// standard, 2026-10-04): 4-7 plain sentences saying what the thing is, what
// goes wrong today with one concrete example, why it matters and to whom, what
// is being decided, and the recommendation in one sentence. It is prose, never
// a list of facts, files, or IDs.
const SITUATION_MIN_CHARS = 300;
const SITUATION_MAX_CHARS = 1200;
function situationGaps(situation) {
  const text = typeof situation === 'string' ? situation.trim() : '';
  if (!text) return ['situation'];
  const gaps = [];
  if (text.length < SITUATION_MIN_CHARS || text.length > SITUATION_MAX_CHARS)
    gaps.push(`situation has ${text.length} characters (need ${SITUATION_MIN_CHARS}-${SITUATION_MAX_CHARS})`);
  if (/^\s*(?:[-*+•]|\d+[.)])\s/m.test(text)) gaps.push('situation (plain prose, not a bullet list)');
  if (/^\s*#{1,6}\s/m.test(text)) gaps.push('situation (plain prose, no headings)');
  if (/^\s*(?:`{3,}|~{3,})/m.test(text)) gaps.push('situation (plain prose, no code blocks)');
  for (const [i, sentence] of sentences(text).entries())
    if (words(sentence).length > PLAIN_SENTENCE_WORDS)
      gaps.push(`situation sentence ${i + 1} has ${words(sentence).length} words (max ${PLAIN_SENTENCE_WORDS})`);
  return gaps;
}

export function ballotGaps(p, {
  requireSurface = Number(p.ballotProcessVersion || 0) >= SURFACE_PROCESS_VERSION,
  requireRecFirst = false,
  card = null,
} = {}) {
  const missing = [];
  const processVersion = Number(p.ballotProcessVersion || 0);
  if (p.surface != null || requireSurface) missing.push(...surfaceGaps(p, { requireRecFirst }));
  const ballotMode = p.ballotMode || 'full';
  if (!['full', 'short'].includes(ballotMode)) missing.push('ballotMode (full or short)');
  if (ballotMode === 'full' || (typeof p.situation === 'string' && p.situation.trim()))
    missing.push(...situationGaps(p.situation));
  if (!p.gist || !String(p.gist).trim()) missing.push('gist');
  if (!p.lesson || !String(p.lesson).trim()) missing.push('lesson');
  else if (String(p.lesson).trim().split(/\n\s*\n/).length > 1) missing.push('lesson (one paragraph maximum)');
  if (!p.story || !String(p.story).trim()) missing.push('story');
  if (!p.inWild || !String(p.inWild).trim()) missing.push('inWild');
  const opts = Array.isArray(p.options) ? p.options : [];
  const optionKeys = opts.map(o => o?.key).filter(Boolean);
  if (opts.length < 2) missing.push('options (need at least 2)');
  else {
    const noKey = opts.filter(o => !o || !o.key || !String(o.key).trim());
    if (noKey.length) missing.push('options[].key');
    const noName = opts.filter(o => !o || !o.name || !String(o.name).trim());
    if (noName.length) missing.push(`options[].name (missing on ${noName.map((o, i) => (o && o.key) || `#${i + 1}`).join(', ')})`);
    const noDetail = opts.filter(o => !o || !o.detail || !String(o.detail).trim());
    if (noDetail.length) missing.push(`options[].detail (missing on ${noDetail.map((o, i) => (o && o.key) || `#${i + 1}`).join(', ')})`);
    const noCode = opts.filter(o => !o || !o.code || !String(o.code).trim());
    if (noCode.length) missing.push(`options[].code (missing on ${noCode.map((o, i) => (o && o.key) || `#${i + 1}`).join(', ')})`);
    const duplicateKeys = [...new Set(optionKeys.filter((key, i) => optionKeys.indexOf(key) !== i))];
    if (duplicateKeys.length) missing.push(`options[].key (duplicate: ${duplicateKeys.join(', ')})`);
  }
  if (!p.rec || !String(p.rec).trim()) missing.push('rec');
  if (p.rec && !optionKeys.includes(p.rec)) missing.push('rec (must match an option key)');
  const recommendation = p.recommendation;
  if (!recommendation || typeof recommendation !== 'object') {
    missing.push('recommendation');
  } else {
    if (!recommendation.why || !String(recommendation.why).trim()) missing.push('recommendation.why');
    if (!recommendation.tradeoff || !String(recommendation.tradeoff).trim()) missing.push('recommendation.tradeoff');
    const whyNot = Array.isArray(recommendation.whyNot) ? recommendation.whyNot : [];
    for (const key of optionKeys.filter(key => key !== p.rec)) {
      const item = whyNot.find(x => x?.key === key);
      if (!item || !item.reason || !String(item.reason).trim()) missing.push(`recommendation.whyNot[${key}]`);
    }
    const recommendationLosses = Array.isArray(recommendation.losses) ? recommendation.losses : [];
    if (processVersion >= STRUCTURED_LOSS_PROCESS_VERSION) {
      recommendationLosses.forEach((item, i) => {
        const loss = typeof item?.loss === 'string' ? item.loss.trim() : '';
        const mitigation = typeof item?.mitigation === 'string' ? item.mitigation.trim() : '';
        if (!plainObject(item))
          missing.push(`recommendation.losses[${i + 1}] "${String(item || '').trim()}" (must include mitigation)`);
        else {
          missing.push(...mitigationGaps(`recommendation.losses[${i + 1}]`, item));
          if (!loss) missing.push(`recommendation.losses[${i + 1}].loss (required)`);
          if (!mitigation) missing.push(`recommendation.losses[${i + 1}].mitigation (required for "${loss || '?'}")`);
        }
      });
    }
  }
  if (ballotMode === 'full') {
    const passes = p.reviewPasses;
    if (!passes || typeof passes !== 'object' || Array.isArray(passes)) {
      missing.push('reviewPasses');
    } else {
      for (const key of Object.keys(passes).filter(key => !reviewPassKeysFor(processVersion).includes(key)))
        missing.push(`reviewPasses.${key} (unexpected)`);
      // Earlier-process review records are immutable historical evidence.
      if (processVersion >= BALLOT_PROCESS_VERSION) {
        for (const key of CURRENT_REVIEW_PASS_KEYS) {
          const summary = reviewPassSummary(key, passes[key]);
          if (typeof summary !== 'string' || !summary.trim()) missing.push(`reviewPasses.${key} (need text)`);
          else if (sentences(summary).length > 2) missing.push(`reviewPasses.${key} (need 1-2 sentences)`);
        }
        // A ratified record keeps the provenance it was recorded with.
        missing.push(...dissentMetadataGaps(p, { requireFresh: p.status !== 'ratified' }));
      }
    }
  } else if (ballotMode === 'short') {
    missing.push(...shortBallotGaps(p, card, processVersion));
  }
  const dense = plainLanguageGaps(p);
  if (dense.length) missing.push(`plain language: ${dense.join('; ')}`);
  return missing;
}

function verdictSupersededBy(s, p) {
  if (p.group !== 'verdict') return null;
  const id = typeof p.supersededBy === 'string' ? p.supersededBy.trim() : '';
  if (!id) fail('E_INVALID', 'verdict needs a supersession link (`supersededBy`)');
  if (id === p.id) fail('E_INVALID', 'verdict cannot supersede itself');
  const previous = s.decisions.find(d => d.id === id);
  if (!previous) fail('E_NOT_FOUND', `verdict supersession link points to unknown decision ${id}`);
  assertReadyForVerdict(previous);
  return id;
}

// The reports a ballot links to, so the owner can read the research,
// measurements and audits behind it. A report is a Tower doc under Docs/
// (`path`) or an outside https source (`url`), with a title and an optional
// one-line note on what it shows.
const REPORT_PATH_RE = /^Docs\/(?:audits|research|proposals|spec)\/(?:[\w.-]+\/)*[\w.-]+\.(?:md|html)$/;
const MAX_REPORTS = 20;
export function normalizeReports(list) {
  if (list == null) return [];
  if (!Array.isArray(list)) fail('E_INVALID', 'decision reports must be a list');
  if (list.length > MAX_REPORTS) fail('E_INVALID', `a decision links at most ${MAX_REPORTS} reports`);
  const seen = new Set();
  return list.map((r, i) => {
    if (!r || typeof r !== 'object' || Array.isArray(r)) fail('E_INVALID', `report ${i + 1} must be an object`);
    const title = typeof r.title === 'string' ? r.title.trim() : '';
    if (!title) fail('E_INVALID', `report ${i + 1} needs a title`);
    if (title.length > 160) fail('E_INVALID', `report ${i + 1} title is longer than 160 characters`);
    const path = typeof r.path === 'string' ? r.path.trim() : '';
    const url = typeof r.url === 'string' ? r.url.trim() : '';
    if (!!path === !!url) fail('E_INVALID', `report ${i + 1} needs exactly one of path (a Tower doc) or url`);
    if (path && (!REPORT_PATH_RE.test(path) || path.split('/').includes('..')))
      fail('E_INVALID', `report ${i + 1} path ${path} is not a Tower doc under Docs/Audits, Docs/Research, Docs/Proposals or Docs/Spec`);
    if (url && !/^https:\/\/[^\s<>"]+$/.test(url)) fail('E_INVALID', `report ${i + 1} url must be an https address`);
    const key = path || url;
    if (seen.has(key)) fail('E_INVALID', `report ${key} is linked twice`);
    seen.add(key);
    const note = typeof r.note === 'string' ? r.note.trim() : '';
    if (note.length > 400) fail('E_INVALID', `report ${i + 1} note is longer than 400 characters`);
    return { title, ...(path ? { path } : { url }), ...(note ? { note } : {}) };
  });
}

export function addDecision(s, p) {
  const card = mustCard(s, p.cardId);
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'decision needs a title');
  if (p.id && s.decisions.find(d => d.id === p.id)) fail('E_INVALID', `decision id ${p.id} already exists`);
  const systemAcceptance = p[SYSTEM_ACCEPTANCE] === true;
  if (!systemAcceptance && (p.group === 'acceptance' || String(p.id || '').startsWith('D-ACCEPT-')))
    fail('E_INVALID', 'acceptance ballots are system-generated; use the card acceptance workflow');
  if (p.situation != null && typeof p.situation !== 'string') fail('E_INVALID', 'decision situation must be a string');
  // Every newly authored narrative ballot is process 5. Historical records
  // enter through the store and keep their stored version unchanged.
  const candidate = systemAcceptance ? p : { ...p, status: 'open', ballotProcessVersion: BALLOT_PROCESS_VERSION };
  const draft = !!candidate.draft;
  const authored = !systemAcceptance && candidate.group !== 'acceptance';
  const draftedBy = authored && typeof candidate.by === 'string' && candidate.by.trim() ? candidate.by.trim() : null;
  const reviewedBy = authored && candidate.reviewedBy != null ? ballotReviewer({ id: candidate.id || 'this ballot', draftedBy }, candidate.reviewedBy) : null;
  if (authored) {
    const gaps = ballotGaps(candidate, { requireSurface: true, requireRecFirst: true, card });
    const metadataGaps = (candidate.ballotMode || 'full') === 'full' ? dissentMetadataGaps(candidate) : [];
    if (draft) {
      if (metadataGaps.length)
        fail('E_BALLOT', `ballot draft missing required review metadata: ${metadataGaps.join(', ')}`);
    } else if (gaps.length) {
      fail('E_BALLOT', `ballot not ready — missing: ${gaps.join(', ')} (pass --draft to save a work-in-progress ballot)`);
    } else if (!reviewedBy) {
      fail('E_STAGE', 'a ballot reaches the owner only after review — pass --reviewed-by <reviewer>, or add it with --draft and post it after decision review');
    }
  }
  const supersededBy = verdictSupersededBy(s, candidate);
  const ballotMode = candidate.group === 'acceptance' ? null : (candidate.ballotMode || 'full');
  const d = { id: candidate.id || newId('D-'), cardId: card.id, group: candidate.group || 'other',
    title: String(candidate.title).trim(), situation: candidate.situation || '', gist: candidate.gist || '', lesson: candidate.lesson || '', explainer: candidate.explainer || '', story: candidate.story || '',
    inWild: candidate.inWild || '', detail: candidate.detail || '', options: candidate.options || [], comparisons: candidate.comparisons || [],
    rec: candidate.rec || null, recommendation: candidate.recommendation || null, hybrid: candidate.hybrid || null,
    ballotMode, shortAuthorizedBy: ballotMode === 'short' ? (candidate.shortAuthorizedBy || null) : null,
    ballotProcessVersion: candidate.group === 'acceptance' ? null : candidate.ballotProcessVersion,
    reviewPasses: ballotMode === 'full' && candidate.reviewPasses
      ? orderedReviewPasses(candidate.reviewPasses, candidate.ballotProcessVersion) : null,
    checkInstructions: candidate.checkInstructions || null,
    ...(candidate.group === 'acceptance' ? { visualMedia: normalizeVisualMedia(candidate.visualMedia) } : {}),
    surface: candidate.surface ?? null, ...(supersededBy ? { supersededBy } : {}),
    // Owner law 2026-09-03: each loss records two elimination attempts before
    // any mitigation. Own field so it survives ballot edits.
    ...(candidate.designAway ? { designAway: candidate.designAway } : {}),
    ...(candidate.reports != null && normalizeReports(candidate.reports).length ? { reports: normalizeReports(candidate.reports) } : {}),
    ...(authored ? { draftedBy, reviewedBy } : {}),
    draft, status: 'open', created: now() };
  s.decisions.push(d);
  touchCard(card, candidate.by);
  logEvent(s, { by: candidate.by, action: 'decision.add', ref: d.id, note: draft ? `${d.title} (draft)` : d.title });
  return { ...d, cardNum: card.num };
}

const SYNTAX_RATIFY_CHORES = ['Data/Syntax registry entry updated', 'syntax-decisions.md log entry', 'editor grammars regenerated (jet self devtools grammars)', 'snapshots re-blessed'];

// D-TWRGUARD1=C (#458): ratifying a syntax-group decision auto-appends the
// standard post-ratification chores to the card's exit-criteria checklist
// (#463 model), skipping any that already exist.
function appendSyntaxChores(s, c, by) {
  if (!c) return;
  const have = new Set((c.criteria || []).map(i => i.text));
  for (const text of SYNTAX_RATIFY_CHORES)
    if (!have.has(text)) addCriterion(s, c.id, text, by || 'agent', { overLimit: 'the standard syntax ratification chores' });
}

function assertReadyForVerdict(d) {
  if (d.draft) fail('E_DRAFT', `${d.id} is a draft ballot (${decisionStage(d)}) — no verdict is allowed until it is reviewed and posted`);
}

export function ratify(s, decisionId, outcome, comment, by, quote) {
  const d = s.decisions.find(x => x.id === decisionId) || fail('E_NOT_FOUND', `no decision ${decisionId}`);
  assertReadyForVerdict(d);
  if (d.group === 'acceptance' || d.id.startsWith('D-ACCEPT-'))
    fail('E_ACCEPTANCE_OWNER_UI', `${d.id} is an owner-verification ballot — generic ratify, --by owner, and --quote cannot resolve it; use the dedicated owner verification UI`);
  if (!outcome) fail('E_INVALID', 'ratify needs an outcome (option key)');
  if (Array.isArray(d.options) && d.options.length && !d.options.some(o => o.key === outcome))
    fail('E_INVALID', `outcome "${outcome}" is not one of this decision's option keys: ${d.options.map(o => o.key).join(', ')}`);
  const quoteNote = assertOwnerOr(by, quote, 'E_OWNER_ONLY', 'ratify');
  d.status = 'ratified'; d.outcome = outcome;
  if (comment != null) d.comment = comment;
  d.ratifiedAt = today();
  const c = s.cards.find(x => x.id === d.cardId);
  if (d.group === 'syntax') appendSyntaxChores(s, c, by);
  advanceClearedCard(s, d.cardId);
  if (c) touchCard(c, by || 'owner');
  logEvent(s, { by: by || 'owner', action: 'decision.ratify', ref: d.id, note: quoteNote ? `${outcome} (${quoteNote})` : outcome });
  return d;
}

// Acceptance has a transport-distinct mutation. The only resolver is minted
// in server memory and never exposed through the CLI or generic route table.
// This prevents caller-controlled `by`, quotes, and batch payloads from
// crossing the owner-verification boundary.
export function createAcceptanceResolver() {
  return (s, decisionId, outcome, comment, provenance) => {
    const d = s.decisions.find(x => x.id === decisionId) || fail('E_NOT_FOUND', `no decision ${decisionId}`);
    assertReadyForVerdict(d);
    if (d.group !== 'acceptance' || !d.id.startsWith('D-ACCEPT-'))
      fail('E_ACCEPTANCE_OWNER_UI', `${d.id} is not an owner-verification ballot`);
    if (d.status === 'ratified') fail('E_INVALID', `${d.id} is already resolved`);
    if (outcome !== 'accept' && outcome !== 'bounce') fail('E_INVALID', 'acceptance outcome must be accept or bounce');
    if (!provenance || provenance.kind !== 'owner-ui' || !provenance.session || !provenance.challenge)
      fail('E_ACCEPTANCE_OWNER_UI', 'missing owner-verification provenance');
    const c = s.cards.find(x => x.id === d.cardId) || fail('E_NOT_FOUND', `no card for ${d.id}`);
    if (outcome === 'accept') {
      const unsettled = (c.criteria || []).filter(i => !['met', 'verified'].includes(i.status));
      if (unsettled.length)
        fail('E_CRITERIA', `${unsettled.length} of ${c.criteria.length} criteria not met or verified (${unsettled.map(i => i.n).join(',')})`);
    }

    d.status = 'ratified';
    d.outcome = outcome;
    if (comment != null) d.comment = comment;
    d.ratifiedAt = today();
    d.provenance = Object.freeze({ ...provenance });
    d.provenanceHistory = [...(d.provenanceHistory || []), Object.freeze({ ...provenance })];
    if (outcome === 'accept') {
      c.phase = 'done';
      c.completedAt = now();
      c.log.unshift({ at: today(), by: 'owner', text: `Accepted — ${d.id} resolved through owner verification UI.` });
    } else {
      c.phase = 'building';
      delete c.completedAt;
      c.log.unshift({ at: today(), by: 'owner', text: `Bounced back to building: ${comment || '(no comment)'}` });
    }
    touchCard(c, 'owner');
    logEvent(s, { by: 'owner', action: 'acceptance.resolve', ref: d.id,
      note: `owner-ui session=${provenance.session} challenge=${provenance.challenge} outcome=${outcome}` });
    return d;
  };
}

export function auditAcceptanceRejection(s, decisionId, route, reason, by) {
  logEvent(s, { by: by || 'unknown', action: 'acceptance.reject', ref: decisionId || 'unknown',
    note: `${route}: ${reason}` });
}

export function reopenDecision(s, decisionId, by) {
  const d = s.decisions.find(x => x.id === decisionId) || fail('E_NOT_FOUND', `no decision ${decisionId}`);
  if (typeof by !== 'string' || !by.trim()) fail('E_INVALID', 'reopen needs --by <actor>');
  if (d.status === 'ratified' && by !== 'owner')
    fail('E_OWNER_ONLY', 'reopening a ratified decision is owner-only');
  if (d.status === 'rejected') { d.reviewedBy = null; delete d.rejectReason; }
  d.status = 'open'; delete d.outcome; delete d.ratifiedAt; const card = s.cards.find(c => c.id === d.cardId);
  // A reopened narrative ballot is re-authored under the current process, so
  // its next update is gated by today's adversarial pass, not retired ones.
  if (d.group !== 'acceptance' && Number(d.ballotProcessVersion || 0) < BALLOT_PROCESS_VERSION)
    d.ballotProcessVersion = BALLOT_PROCESS_VERSION;
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'decision.reopen', ref: d.id });
  return d;
}

export function updateDecision(s, id, patch, by) {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  if (d.group === 'acceptance' || d.id.startsWith('D-ACCEPT-') || patch.group === 'acceptance')
    fail('E_INVALID', 'acceptance ballots are system-generated and cannot use decision update');
  if (d.status === 'ratified' && by !== 'owner')
    fail('E_OWNER_ONLY', 'updating a ratified decision is owner-only');
  if (patch.situation != null && typeof patch.situation !== 'string') fail('E_INVALID', 'decision situation must be a string');
  const card = s.cards.find(c => c.id === d.cardId);
  const storedVersion = Number(d.ballotProcessVersion || 0);
  const edits = BALLOT_CONTENT_KEYS.filter(k => k in patch);
  for (const k of edits) d[k] = patch[k];
  if ('reports' in patch) {
    const reports = normalizeReports(patch.reports);
    if (reports.length) d.reports = reports; else delete d.reports;
  }
  // A review covers the text it read: editing a reviewed or rejected draft
  // returns it to drafted.
  const reviewCleared = d.draft && edits.length > 0 && (d.reviewedBy || d.status === 'rejected');
  if (reviewCleared) {
    d.reviewedBy = null; delete d.rejectReason;
    if (d.status === 'rejected') d.status = 'open';
  }
  const supersededBy = verdictSupersededBy(s, d);
  if (supersededBy) d.supersededBy = supersededBy;
  if (d.group !== 'acceptance' && d.status !== 'ratified' && d.ballotMode !== 'short') {
    const metadataGaps = dissentMetadataGaps(d);
    if (metadataGaps.length)
      fail('E_BALLOT', `ballot update missing required review metadata: ${metadataGaps.join(', ')}`);
  }
  // Every edit to a posted ballot re-runs the gate. Ratified records remain
  // historical law.
  if (isBlocking(d)) gateBallot(d, card, storedVersion);
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'decision.update', ref: d.id, note: reviewCleared ? 'review cleared by the edit' : '' });
  return d;
}

const BALLOT_CONTENT_KEYS = ['title', 'situation', 'gist', 'lesson', 'explainer', 'story', 'inWild', 'detail', 'options', 'comparisons', 'rec', 'recommendation', 'hybrid', 'checkInstructions', 'group', 'ballotMode', 'shortAuthorizedBy', 'reviewPasses', 'supersededBy', 'surface', 'designAway'];

// The ballot gate a posted ballot passes: every gap closed, review passes in
// canonical order, and the current process version.
function gateBallot(d, card, storedVersion) {
  d.ballotMode ||= 'full';
  const gaps = ballotGaps(d, { requireSurface: storedVersion >= SURFACE_PROCESS_VERSION, card });
  if (gaps.length) fail('E_BALLOT', `ballot not ready — missing: ${gaps.join(', ')}`);
  if (d.ballotMode === 'full') {
    d.reviewPasses = orderedReviewPasses(d.reviewPasses, storedVersion);
    d.shortAuthorizedBy = null;
  } else {
    d.reviewPasses = null;
  }
  if (storedVersion >= BALLOT_PROCESS_VERSION) d.ballotProcessVersion = BALLOT_PROCESS_VERSION;
}

// The reviewer of a ballot: named, and not the agent that drafted it.
function ballotReviewer(d, by) {
  const reviewer = typeof by === 'string' ? by.trim() : '';
  if (!reviewer) fail('E_INVALID', `reviewing ${d.id} needs --by <reviewer>`);
  if (d.draftedBy && reviewer === d.draftedBy)
    fail('E_STAGE', `${d.id} was drafted by ${reviewer}; another agent must review it`);
  return reviewer;
}

const mustAuthoredBallot = (s, id) => {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  if (d.group === 'acceptance' || d.id.startsWith('D-ACCEPT-'))
    fail('E_INVALID', `${d.id} is an owner-verification ballot; it has no review stages`);
  return d;
};

// `decision review`: a reviewer passes a draft (stage reviewed) or rejects it
// with a reason (stage rejected) for its author to revise.
export function reviewDecision(s, id, { by, reject = false, reason } = {}) {
  const d = mustAuthoredBallot(s, id);
  const stage = decisionStage(d);
  if (stage !== 'drafted' && stage !== 'reviewed')
    fail('E_STAGE', `${d.id} is ${stage}; only a drafted or reviewed ballot can be reviewed${stage === 'rejected' ? ' (revise it with decision update first)' : ''}`);
  const reviewer = ballotReviewer(d, by);
  const why = typeof reason === 'string' ? reason.trim() : '';
  if (reject) {
    if (!why) fail('E_INVALID', 'decision review --reject needs --reason "…"');
    d.status = 'rejected';
    d.rejectReason = why;
  }
  d.reviewedBy = reviewer;
  const card = s.cards.find(c => c.id === d.cardId);
  if (card) touchCard(card, reviewer);
  logEvent(s, { by: reviewer, action: 'decision.review', ref: d.id, note: reject ? `rejected: ${why}` : 'passed' });
  return d;
}

// `decision post`: a reviewed ballot passes the ballot gate and enters the
// owner's queue.
export function postDecision(s, id, by) {
  const d = mustAuthoredBallot(s, id);
  const stage = decisionStage(d);
  if (stage !== 'reviewed')
    fail('E_STAGE', `${d.id} is ${stage}; decision post needs a reviewed ballot${stage === 'drafted' ? ` (run decision review ${d.id} --by <reviewer>)` : ''}`);
  const card = s.cards.find(c => c.id === d.cardId);
  const metadataGaps = d.ballotMode === 'short' ? [] : dissentMetadataGaps(d);
  if (metadataGaps.length) fail('E_BALLOT', `ballot missing required review metadata: ${metadataGaps.join(', ')}`);
  gateBallot(d, card, Number(d.ballotProcessVersion || 0));
  d.draft = false;
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'decision.post', ref: d.id, note: `reviewed by ${d.reviewedBy}` });
  return d;
}

// `tower ballots`: every authored ballot by stage, with its card and the live
// cards and open criteria waiting on it. Ratified ballots are counted in full
// but listed only inside the retire window, like the owner's recent strip.
export function ballotsReport(s, config = null) {
  const bufferDays = config?.retireAfterDays ?? 3;
  const cardsById = new Map(s.cards.map(c => [c.id, c]));
  const waits = new Map();
  const waitsOn = (id) => waits.get(id) || waits.set(id, { cards: [], criteria: [] }).get(id);
  for (const c of s.cards) {
    if (c.phase === 'done') continue;
    for (const ref of c.blockedBy || []) if (String(ref).startsWith('D-')) waitsOn(ref).cards.push(c.num);
    for (const item of c.criteria || [])
      if (item.status === 'open' && String(item.blockedBy || '').startsWith('D-')) waitsOn(item.blockedBy).criteria.push(`#${c.num} c${item.n}`);
  }
  const ballots = s.decisions.filter(d => d.group !== 'acceptance' && !d.id.startsWith('D-ACCEPT-'));
  return DECISION_STAGES.map(stage => {
    const inStage = ballots.filter(d => decisionStage(d) === stage);
    const listed = stage === 'ratified'
      ? inStage.filter(d => !isOlderThanDays(d.ratifiedAt, bufferDays)).sort((a, b) => (b.ratifiedAt || '').localeCompare(a.ratifiedAt || ''))
      : inStage;
    return { stage, count: inStage.length, ballots: listed.map(d => ({
      id: d.id, title: d.title, card: cardsById.get(d.cardId)?.num ?? null,
      draftedBy: d.draftedBy || null, reviewedBy: d.reviewedBy || null,
      ...(d.rejectReason ? { rejectReason: d.rejectReason } : {}),
      ...(d.outcome ? { outcome: d.outcome, ratifiedAt: d.ratifiedAt || null } : {}),
      waiting: (stage !== 'ratified' && waits.get(d.id)) || { cards: [], criteria: [] },
    })) };
  });
}

// D-TWRGUARD1=C (#458): tower verdict — an owner ruling recorded as an
// already-ratified decision (never a log note) so it's durable + auditable.
// Owner-only, no quote exception (this IS the owner speaking).
export function mintVerdict(s, ref, outcome, title, by, supersedes) {
  const c = mustCard(s, ref);
  if (by !== 'owner') fail('E_OWNER_ONLY', 'tower verdict is owner-only');
  if (!outcome || !String(outcome).trim()) fail('E_INVALID', 'verdict needs an outcome');
  let k = 1;
  while (s.decisions.find(x => x.id === `D-VERDICT-${c.num}-${k}`)) k++;
  const id = `D-VERDICT-${c.num}-${k}`;
  // The canonical CLI verdict path validates its supersession link through the
  // same helper `addDecision` uses, so a verdict minted here cannot skip the
  // rule that a verdict names the decision it replaces.
  const supersededBy = verdictSupersededBy(s, { group: 'verdict', id, supersededBy: supersedes });
  const d = { id, cardId: c.id, group: 'verdict',
    title: title || `Verdict on #${c.num} — ${c.title}`,
    situation: '', gist: '', lesson: '', explainer: '', story: '', inWild: '', detail: '', options: [], comparisons: [],
    rec: null, recommendation: null, hybrid: null, ballotMode: null, shortAuthorizedBy: null, reviewPasses: null,
    draft: false, status: 'ratified', outcome, comment: outcome, supersededBy,
    created: now(), ratifiedAt: today() };
  s.decisions.push(d);
  c.log.unshift({ at: today(), by, text: `Verdict recorded (${id}): ${outcome}` });
  touchCard(c, by);
  logEvent(s, { by, action: 'decision.verdict', ref: id, note: outcome });
  return { ...d, cardNum: c.num };
}

export function deleteDecision(s, id, by) {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  if (d.status === 'ratified' && by !== 'owner')
    fail('E_OWNER_ONLY', 'deleting a ratified decision is owner-only');
  const dependents = s.decisions.filter(x => x.supersededBy === id);
  if (dependents.length)
    fail('E_REFERENCED', `cannot delete decision ${id}; supersession link used by ${dependents.map(x => x.id).join(', ')}`);
  s.decisions = s.decisions.filter(x => x.id !== id);
  const card = s.cards.find(c => c.id === d.cardId);
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'decision.delete', ref: id, note: d.title, draft: !!d.draft });
  return { ok: true, id };
}

function advanceClearedCard(s, cardId) {
  const c = s.cards.find(x => x.id === cardId);
  if (!c || c.phase !== 'deciding') return;
  const stillOpen = s.decisions.some(d => d.cardId === cardId && isBlocking(d));
  if (stillOpen) return;
  c.phase = c.plan ? 'ready' : 'planning';
  c.log.unshift({ at: today(), text: 'All decisions ratified; advanced out of deciding.' });
}

// ---- mutations: questions --------------------------------------------------

export function addQuestion(s, p) {
  const card = mustCard(s, p.cardId);
  if (!p.text || !String(p.text).trim()) fail('E_INVALID', 'question needs text');
  if (p.kind === 'message') fail('E_INVALID', 'use message add for agent messages');
  if (!p.by) fail('E_INVALID', 'question add needs --by <owner or agent>');
  const q = { id: newId('q'), cardId: card.id, decisionId: p.decisionId || null,
    by: p.by, kind: p.kind || 'question',
    text: String(p.text).trim(), status: 'open', answer: '', created: now() };
  s.questions.push(q);
  touchCard(card, p.by);
  logEvent(s, { by: p.by, action: 'question.add', ref: q.id });
  return { ...q, cardNum: card.num };
}
export function answerQuestion(s, id, answer, by) {
  const q = s.questions.find(x => x.id === id) || fail('E_NOT_FOUND', `no question ${id}`);
  if (q.kind === 'message') fail('E_INVALID', `use message done for ${id}`);
  if (!answer || !String(answer).trim()) fail('E_INVALID', 'answer needs text');
  q.answer = answer; q.status = 'answered'; q.answeredAt = today(); q.answeredBy = by || 'agent';
  const card = s.cards.find(c => c.id === q.cardId);
  if (card) touchCard(card, by || 'agent');
  logEvent(s, { by: by || 'agent', action: 'question.answer', ref: q.id });
  return q;
}
export function deleteQuestion(s, id, by) {
  const q = s.questions.find(x => x.id === id);
  if (q?.kind === 'message') fail('E_INVALID', `use message done for ${id}`);
  s.questions = s.questions.filter(q => q.id !== id);
  const card = q && s.cards.find(c => c.id === q.cardId);
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'question.delete', ref: id });
  return { ok: true, id };
}

// ---- mutations: durable agent messages ------------------------------------

export function listMessages(s, { cardId, status = 'open' } = {}) {
  const card = cardId == null ? null : mustCard(s, cardId);
  return s.questions.filter(q => q.kind === 'message'
    && (!card || q.cardId === card.id)
    && (!status || q.status === status));
}

export function addMessage(s, p) {
  const card = mustCard(s, p.cardId);
  if (!p.text || !String(p.text).trim()) fail('E_INVALID', 'message needs text');
  if (!p.by || p.by === 'owner') fail('E_INVALID', 'message add needs --by <agent>');
  const lane = laneOf(card, s.decisions, s.cards).lane;
  if (lane === 'frozen' || lane === 'decide')
    fail('E_OWNER_LANE', `card #${card.num} is in the ${lane} owner lane — agents cannot add messages`);
  const message = {
    id: newId('q'),
    cardId: card.id,
    decisionId: null,
    by: p.by,
    kind: 'message',
    text: String(p.text).trim(),
    status: 'open',
    answer: '',
    created: now(),
  };
  s.questions.push(message);
  logEvent(s, { by: p.by, action: 'message.add', ref: message.id, note: `#${card.num}` });
  return { ...message, cardNum: card.num };
}

export function doneMessage(s, id, by) {
  if (by !== 'owner') fail('E_OWNER_ONLY', 'message done is owner-only');
  const message = s.questions.find(q => q.id === id && q.kind === 'message')
    || fail('E_NOT_FOUND', `no message ${id}`);
  message.status = 'done';
  message.completedAt = now();
  message.doneBy = by;
  logEvent(s, { by, action: 'message.done', ref: id });
  return message;
}

// ---- Now reports ----------------------------------------------------------
// Reports describe progress; posting one never changes a card, milestone gate,
// or ballot. Attribution/time are assigned here, not trusted from a file.
function reportText(value, label) {
  if (typeof value !== 'string' || !value.trim()) fail('E_INVALID', `${label} needs nonempty text`);
  return value;
}

function reportArray(value, label) {
  if (!Array.isArray(value)) fail('E_INVALID', `${label} must be an array`);
  return value;
}

function reportTime(value, label) {
  reportText(value, label);
  if (!/^\d{4}-\d\d-\d\dT/.test(value) || !Number.isFinite(Date.parse(value)))
    fail('E_INVALID', `${label} must be an ISO timestamp`);
}

function reportLinks(links = []) {
  for (const link of reportArray(links, 'links')) {
    if (!plainObject(link) || (link.card !== undefined) === (link.decision !== undefined))
      fail('E_INVALID', 'each link needs exactly one card or decision reference');
    reportText(link.card !== undefined ? link.card : link.decision, 'link reference');
    if (link.label !== undefined) reportText(link.label, 'link label');
  }
}

function validateBriefing(record) {
  if (!plainObject(record)) fail('E_INVALID', 'briefing must be an object');
  for (const key of ['id', 'title', 'body', 'by']) reportText(record[key], `briefing.${key}`);
  reportTime(record.created, 'briefing.created');
  for (const section of reportArray(record.sections, 'briefing.sections')) {
    if (!plainObject(section)) fail('E_INVALID', 'briefing section must be an object');
    reportText(section.title, 'section.title');
    reportText(section.body, 'section.body');
    reportLinks(section.links);
  }
}

function validateStatusSnapshot(record) {
  if (!plainObject(record)) fail('E_INVALID', 'status snapshot must be an object');
  if (Object.hasOwn(record, 'ownerActions')) fail('E_INVALID', 'status.ownerActions has been removed; omit this field');
  for (const key of ['id', 'by']) reportText(record[key], `status.${key}`);
  reportTime(record.created, 'status.created');
  if (record.updatedAt !== undefined) reportTime(record.updatedAt, 'status.updatedAt');
  if (record.summary !== undefined) reportText(record.summary, 'status.summary');
  for (const milestone of reportArray(record.milestones, 'status.milestones')) {
    if (!plainObject(milestone)) fail('E_INVALID', 'milestone must be an object');
    reportText(milestone.title, 'milestone.title');
    reportText(milestone.state, 'milestone.state');
    if (!plainObject(milestone.progress)) fail('E_INVALID', 'milestone.progress must map labels to percentages');
    for (const [label, percent] of Object.entries(milestone.progress)) {
      reportText(label, 'progress label');
      if (typeof percent !== 'number' || !Number.isFinite(percent) || percent < 0 || percent > 100)
        fail('E_INVALID', 'progress percentages must be numbers from 0 to 100');
    }
    reportLinks(milestone.links);
  }
  for (const stream of reportArray(record.workstreams, 'status.workstreams')) {
    if (!plainObject(stream)) fail('E_INVALID', 'workstream must be an object');
    reportText(stream.title, 'workstream.title');
    reportText(stream.state, 'workstream.state');
    for (const worker of reportArray(stream.workers, 'workstream.workers')) reportText(worker, 'worker');
    for (const blocker of reportArray(stream.blockers, 'workstream.blockers')) reportText(blocker, 'blocker');
    if (stream.updatedAt !== undefined) reportTime(stream.updatedAt, 'workstream.updatedAt');
    reportLinks(stream.links);
  }
}

function resolveReportLinks(s, items) {
  for (const item of items) {
    item.links = (item.links || []).map(link => {
      if (link.card !== undefined) return { ...link, card: mustCard(s, link.card).id };
      const decision = s.decisions.find(d => d.id === link.decision)
        || fail('E_NOT_FOUND', `no decision ${link.decision}`);
      return { ...link, cardId: decision.cardId };
    });
  }
}

export function postBriefing(s, p) {
  const record = { id: newId('briefing'), title: p.title, body: p.body, by: p.by,
    created: now(), sections: structuredClone(p.sections ?? []) };
  validateBriefing(record);
  resolveReportLinks(s, record.sections);
  s.briefings ||= [];
  s.briefings.unshift(record);
  logEvent(s, { by: record.by, action: 'briefing.post', ref: record.id, note: record.title });
  return record;
}

export function postStatus(s, p) {
  if (!plainObject(p.snapshot)) fail('E_INVALID', 'status post needs a JSON snapshot object');
  if (Object.hasOwn(p.snapshot, 'ownerActions')) fail('E_INVALID', 'status.ownerActions has been removed; omit this field');
  const { summary, updatedAt, milestones, workstreams } = structuredClone(p.snapshot);
  const record = { id: newId('status'), by: p.by, created: now(),
    ...(summary !== undefined ? { summary } : {}), ...(updatedAt !== undefined ? { updatedAt } : {}),
    milestones, workstreams };
  validateStatusSnapshot(record);
  resolveReportLinks(s, [...record.milestones, ...record.workstreams]);
  s.statusSnapshot = record;
  logEvent(s, { by: record.by, action: 'status.post', ref: record.id, note: record.summary || '' });
  return record;
}

// ---- mutations: ideas ------------------------------------------------------

export function addIdea(s, p) {
  if (!p.text || !String(p.text).trim()) fail('E_INVALID', 'idea needs text');
  const b = { id: newId('b'), text: String(p.text).trim(), note: p.note || '', level: p.level ?? null, tags: p.tags || [], status: 'open', created: now() };
  s.ideas.push(b);
  logEvent(s, { by: p.by, action: 'idea.add', ref: b.id, note: b.text.slice(0, 60) });
  return b;
}
export function updateIdea(s, id, patch) {
  const b = s.ideas.find(x => x.id === id) || fail('E_NOT_FOUND', `no idea ${id}`);
  for (const k of ['text', 'note', 'level', 'tags', 'status']) if (k in patch) b[k] = patch[k];
  return b;
}
export function deleteIdea(s, id, by) {
  s.ideas = s.ideas.filter(b => b.id !== id);
  logEvent(s, { by, action: 'idea.delete', ref: id });
  return { ok: true, id };
}
export function promoteIdea(s, ideaId, extra = {}, config) {
  const b = s.ideas.find(x => x.id === ideaId) || fail('E_NOT_FOUND', `no idea ${ideaId}`);
  const card = addCard(s, {
    title: extra.title || b.text.split(':')[0].slice(0, 80),
    body: extra.body || (b.note ? `${b.text}\n\n${b.note}` : b.text),
    kind: extra.kind || (config.kinds.includes('idea') ? 'idea' : config.kinds[0]),
    track: extra.track || config.tracks.at(-1),
    phase: 'planning',
    priority: extra.priority || config.priorities.at(-1),
    tags: extra.tags || b.tags || [],
    by: extra.by,
  }, config);
  card.log.unshift({ at: today(), text: 'Promoted from Ideas' });
  b.status = 'tagged'; b.cardId = card.id;
  return card;
}

// ---- mutations: epochs + milestones ----------------------------------------

// Epochs are the board's one planning axis, worked in `order`; an epoch's
// `exitCriteria` say what finishing it means. Milestones are worked in their
// own `order` within an epoch.
function planOrder(raw, what = 'epoch') {
  const order = typeof raw === 'string' && raw.trim() ? Number(raw) : raw;
  if (!Number.isSafeInteger(order) || order < 0)
    fail('E_INVALID', `${what} order must be a whole number 0 or above (got ${JSON.stringify(raw)})`);
  return order;
}
function epochExitCriteria(raw) {
  const list = Array.isArray(raw) ? raw : [raw];
  if (list.some(item => typeof item !== 'string' || !item.trim()))
    fail('E_INVALID', 'an epoch exit criterion needs text');
  return list.map(item => item.trim());
}
const mustEpoch = (s, id) => s.epochs.find(x => x.id === id) || fail('E_NOT_FOUND', `no epoch ${id}`);

export function addEpoch(s, p) {
  if (!p.id || !String(p.id).trim()) fail('E_INVALID', 'epoch needs an id (e.g. e1)');
  if (s.epochs.find(e => e.id === p.id)) fail('E_INVALID', `epoch ${p.id} already exists`);
  const e = { id: p.id, name: p.name || p.id, goal: p.goal || '', status: p.status || 'open' };
  if (p.order != null) e.order = planOrder(p.order);
  if (p.exitCriteria != null) e.exitCriteria = epochExitCriteria(p.exitCriteria);
  s.epochs.push(e);
  logEvent(s, { by: p.by, action: 'epoch.add', ref: e.id, note: e.name });
  return e;
}
export function updateEpoch(s, id, patch) {
  const e = mustEpoch(s, id);
  // D-TWR-OPS1=A: at most one active epoch. Reject a second one honestly rather
  // than silently demoting the old epoch (which might not actually be finished).
  if (patch.status === 'active') {
    const other = s.epochs.find(x => x.id !== id && x.status === 'active');
    if (other) fail('E_INVALID', `${other.id} is already active — set it to arrived/planned before activating ${id}`);
  }
  const next = {};
  if ('order' in patch) next.order = planOrder(patch.order);
  if ('exitCriteria' in patch) next.exitCriteria = epochExitCriteria(patch.exitCriteria);
  for (const k of ['name', 'goal', 'status']) if (k in patch) next[k] = patch[k];
  const changed = Object.keys(next).filter(k => JSON.stringify(e[k]) !== JSON.stringify(next[k]));
  Object.assign(e, next);
  if (changed.length) logEvent(s, { by: patch.by, action: 'epoch.update', ref: e.id, note: changed.join(', ') });
  return e;
}
// Removing an epoch must never strand work: it is refused while any card
// (live in any phase, or archived) or milestone still points at it.
export function removeEpoch(s, history, id, by) {
  const e = mustEpoch(s, id);
  const live = s.cards.filter(c => c.epoch === id).length;
  const archived = (history?.cards || []).filter(c => c.epoch === id).length;
  const milestones = s.milestones.filter(m => m.epochId === id).length;
  if (live || archived || milestones) {
    const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;
    fail('E_INVALID', `epoch ${id} is still referenced by ${plural(live, 'card')}, ${plural(archived, 'archived card')} `
      + `and ${plural(milestones, 'milestone')}; move or delete them first`);
  }
  s.epochs = s.epochs.filter(x => x !== e);
  logEvent(s, { by, action: 'epoch.remove', ref: id, note: e.name });
  return { removed: id };
}
// D-TWR-OPS1=A: `epoch current <id>` is now sugar for activating that epoch;
// `epoch current none` demotes the live epoch back to planned. The retired
// meta.currentEpoch field is gone — status is the only source of truth.
export function setCurrentEpoch(s, id, by) {
  if (id == null) {
    const cur = s.epochs.find(e => e.status === 'active');
    if (cur) {
      cur.status = 'planned';
      logEvent(s, { by, action: 'epoch.current', ref: cur.id, note: 'none active' });
    }
    return { active: null };
  }
  updateEpoch(s, id, { status: 'active', by });
  return { active: id };
}

export function addMilestone(s, p) {
  checkEpoch(s, p.epochId || fail('E_INVALID', 'milestone needs --epoch <id>'));
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'milestone needs a title');
  assertCriterionText(p.exitCriteria);
  const m = { id: p.id || newId('m'), epochId: p.epochId, title: String(p.title).trim(), goal: p.goal || '' };
  if (p.order != null) m.order = planOrder(p.order, 'milestone');
  Object.assign(m, { exitCriteria: normalizeExitCriteria(p.exitCriteria), status: 'open', created: now() });
  s.milestones.push(m);
  logEvent(s, { by: p.by, action: 'milestone.add', ref: m.id, note: m.title });
  return m;
}

function mustMilestone(s, id) {
  return s.milestones.find(x => x.id === id) || fail('E_NOT_FOUND', `no milestone ${id}`);
}

// A retired milestone is kept as it was retired: its criteria and review are
// closed to writes.
function mustLiveMilestone(s, id) {
  const m = mustMilestone(s, id);
  if (m.status === 'retired') fail('E_INVALID', `milestone ${m.id} is retired (${m.retired?.reason || 'no reason given'})`);
  return m;
}

function mustMilestoneCriterion(m, n) {
  const item = (m.exitCriteria || []).find(i => i.n === Number(n));
  if (!item) fail('E_NOT_FOUND', `no criterion #${n} on milestone ${m.id}`);
  return item;
}

// Moving a milestone to another epoch takes its live cards along, so every
// card's milestone stays in the card's epoch.
export function updateMilestone(s, id, patch, by) {
  const m = mustMilestone(s, id);
  if ('status' in patch)
    fail('E_MILESTONE_VERIFY', `milestone ${m.id} status is derived — \`tower milestone verify ${m.id} --evidence "..." --by <reviewer>\` signs it off as met, \`tower milestone retire ${m.id} --reason "..."\` retires it`);
  const notes = [];
  if ('exitCriteria' in patch) {
    assertCriterionText(patch.exitCriteria);
    m.exitCriteria = normalizeExitCriteria(patch.exitCriteria);
    clearMilestoneVerification(m);
  }
  if ('order' in patch) m.order = planOrder(patch.order, 'milestone');
  for (const k of ['title', 'goal']) if (k in patch) m[k] = patch[k];
  if ('epochId' in patch && patch.epochId !== m.epochId) {
    checkEpoch(s, patch.epochId || fail('E_INVALID', 'milestone needs --epoch <id>'));
    const moved = s.cards.filter(c => c.milestoneId === m.id);
    for (const c of moved) {
      c.epoch = patch.epochId;
      touchCard(c, by);
    }
    notes.push(`moved from ${m.epochId} to ${patch.epochId}${moved.length ? ` with ${moved.length} linked card${moved.length === 1 ? '' : 's'}` : ''}`);
    m.epochId = patch.epochId;
  }
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.update', ref: m.id, note: notes.join('; ') });
  return m;
}

// Retiring keeps a milestone for the record: its cards keep their link, and it
// leaves milestone lists, briefs and the radar.
export function retireMilestone(s, id, { reason, by } = {}) {
  const m = mustMilestone(s, id);
  if (!by) fail('E_INVALID', 'milestone retire needs --by <agent>');
  const why = needsReason('milestone retire', reason);
  if (m.status === 'retired') fail('E_INVALID', `milestone ${m.id} is already retired (${m.retired?.reason || 'no reason given'})`);
  m.status = 'retired';
  m.retired = { reason: why, by, at: now() };
  logEvent(s, { by, action: 'milestone.retire', ref: m.id, note: why });
  return m;
}

export function addMilestoneCriterion(s, id, text, by) {
  const m = mustLiveMilestone(s, id);
  if (!text || !String(text).trim()) fail('E_INVALID', 'criterion needs text');
  const n = nextCriterionNumber(m, m.exitCriteria);
  const item = normalizeCriterion({ n, text }, n - 1);
  m.exitCriteria.push(item);
  clearMilestoneVerification(m);
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.criteria-add', ref: m.id, note: `#${n} ${item.text.slice(0, 60)}` });
  return { ...item, milestoneId: m.id };
}

// Exit-criterion evidence passes the same gate as a card criterion's
// (`criterionDurabilityIssue`).
function assertExitCriterionDurable(s, m, n, kind, evidence, by, config, history) {
  if (by === 'owner') return;
  const issue = criterionDurabilityIssue(s, { num: null }, kind, evidence, config, history ?? {});
  if (issue) fail('E_EVIDENCE', `milestone ${m.id} criterion #${n}: ${issue}`);
}

export function meetMilestoneCriterion(s, id, n, { evidence, by, kind = 'code' } = {}, config = {}, history = null) {
  const m = mustLiveMilestone(s, id);
  const item = mustMilestoneCriterion(m, n);
  if (!by) fail('E_INVALID', 'meet needs --by <agent>');
  assertExitCriterionDurable(s, m, item.n, kind, evidence ?? item.evidence, by, config, history);
  item.status = 'met';
  item.metBy = by;
  if (evidence != null) item.evidence = evidence;
  setClosureKind(item, kind);
  item.at = now();
  clearMilestoneVerification(m, { keepCloseout: true });
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.criteria-meet', ref: m.id, note: `#${item.n}` });
  return { ...item, milestoneId: m.id };
}

export function verifyMilestoneCriterion(s, id, n, { evidence, by, kind } = {}, config = {}, history = null) {
  const m = mustLiveMilestone(s, id);
  const item = mustMilestoneCriterion(m, n);
  if (!by) fail('E_INVALID', 'verify needs --by <reviewer>');
  if (item.status === 'open') fail('E_INVALID', `criterion #${n} not met yet — meet it before verifying`);
  if (by === item.metBy) fail('E_CRITERIA_SELF', `criterion #${n} verifier must differ from the agent that marked it met (${by})`);
  const closure = kind ?? item.closure ?? 'code';
  assertExitCriterionDurable(s, m, item.n, closure, evidence ?? item.evidence, by, config, history);
  item.status = 'verified';
  item.verifiedBy = by;
  if (evidence != null) item.evidence = evidence;
  setClosureKind(item, closure);
  item.at = now();
  clearMilestoneVerification(m, { keepCloseout: true });
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.criteria-verify', ref: m.id, note: `#${item.n}` });
  return { ...item, milestoneId: m.id };
}

export function reopenMilestoneCriterion(s, id, n, { reason, by } = {}) {
  const m = mustLiveMilestone(s, id);
  const item = mustMilestoneCriterion(m, n);
  if (!by) fail('E_INVALID', 'reopen needs --by <agent>');
  if (!reason || !String(reason).trim()) fail('E_INVALID', 'reopen needs --reason <text>');
  if (item.status === 'open') fail('E_INVALID', `criterion #${n} is already open`);
  item.status = 'open';
  item.metBy = null;
  item.verifiedBy = null;
  item.evidence = '';
  delete item.closure;
  item.at = now();
  clearMilestoneVerification(m);
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.criteria-reopen', ref: m.id, note: `#${item.n}: ${String(reason).trim()}` });
  return { ...item, milestoneId: m.id };
}

export function editMilestoneCriterion(s, id, n, { text, reason, by } = {}) {
  const m = mustLiveMilestone(s, id);
  const item = mustMilestoneCriterion(m, n);
  if (!by) fail('E_INVALID', 'edit needs --by <agent>');
  const why = needsReason('edit', reason);
  const { before, reopened } = rewordCriterion(item, text);
  clearMilestoneVerification(m);
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.criteria-edit', ref: m.id, note: `#${item.n}: ${why}${reopened ? ' (reopened)' : ''}; was: ${before.slice(0, 200)}` });
  return { ...item, reopened, milestoneId: m.id };
}

export function removeMilestoneCriterion(s, id, n, { reason, by } = {}) {
  const m = mustLiveMilestone(s, id);
  const item = mustMilestoneCriterion(m, n);
  if (!by) fail('E_INVALID', 'remove needs --by <agent>');
  const why = needsReason('remove', reason);
  m.criteriaLastN = Math.max(m.criteriaLastN || 0, ...m.exitCriteria.map(i => i.n));
  m.exitCriteria = m.exitCriteria.filter(i => i !== item);
  clearMilestoneVerification(m);
  syncMilestone(s, m.id);
  logEvent(s, { by, action: 'milestone.criteria-remove', ref: m.id, note: `#${item.n}: ${why}; was [${item.status}]: ${item.text.slice(0, 200)}` });
  return { ...item, removed: true, milestoneId: m.id };
}

export function beginMilestoneCloseout(s, id, { sourceCommit, by } = {}, historyCards = []) {
  const m = mustLiveMilestone(s, id);
  if (!by) fail('E_INVALID', 'milestone closeout needs --by <agent>');
  if (!/^[0-9a-f]{40}(?:[0-9a-f]{24})?$/.test(String(sourceCommit || '')))
    fail('E_INVALID', 'milestone closeout needs --commit <40-or-64-character lowercase Git object id>');
  const linked = milestoneCards(m.id, s.cards, historyCards);
  if (!linked.length || !linked.every(c => c.phase === 'done'))
    fail('E_MILESTONE', `milestone ${m.id} is not ready for review — every linked card must be done`);
  m.closeout = { sourceCommit: String(sourceCommit), by, at: now() };
  logEvent(s, { by, action: 'milestone.closeout', ref: m.id, note: m.closeout.sourceCommit });
  return m;
}

export function verifyMilestone(s, id, { evidence, by } = {}, historyCards = []) {
  const m = mustLiveMilestone(s, id);
  if (!by) fail('E_INVALID', 'milestone verify needs --by <reviewer>');
  if (!evidence || !String(evidence).trim()) fail('E_INVALID', 'milestone verify needs --evidence <text>');
  const linked = milestoneCards(m.id, s.cards, historyCards);
  if (!linked.length || !linked.every(c => c.phase === 'done'))
    fail('E_MILESTONE', `milestone ${m.id} is not ready for review — every linked card must be done`);
  const unfinished = m.exitCriteria.filter(i => i.status !== 'verified');
  if (unfinished.length)
    fail('E_MILESTONE', `milestone ${m.id} has unverified criteria (${unfinished.map(i => i.n).join(',')})`);
  if (!m.closeout)
    fail('E_MILESTONE', `milestone ${m.id} has no closeout token — run \`tower milestone closeout ${m.id} --commit <frozen source commit> --by <agent>\` before broad proof or review`);
  const builders = new Set(m.exitCriteria.map(i => i.metBy).filter(Boolean));
  if (builders.has(by)) fail('E_CRITERIA_SELF', `milestone reviewer must differ from an agent that marked its criteria met (${by})`);
  m.status = 'met';
  m.verification = { by, evidence: String(evidence).trim(), at: now() };
  logEvent(s, { by, action: 'milestone.verify', ref: m.id, note: String(evidence).trim() });
  return m;
}

export function deleteMilestone(s, id, by) {
  const m = mustMilestone(s, id);
  s.milestones = s.milestones.filter(x => x.id !== id);
  for (const c of s.cards) if (c.milestoneId === id) c.milestoneId = null;
  logEvent(s, { by, action: 'milestone.delete', ref: id, note: m.title });
  return { ok: true, id };
}

// ---- next: the canonical "what should an agent work on" picker -------------

const LANE_PREF = { verify: 0, building: 1, implement: 2, plan: 3 };

// The epoch chain orders the pick: cards of the active epoch (the first
// unfinished one in `order`) come first, then the next epoch's, and so on;
// within an epoch, review > building > implement > plan, then workOrder.
//
// #457 — `scope: 'burndown'` narrows the pool to the active epoch's
// epoch-track cards plus all sidequests (agent lanes only).
//
// D-TWR-OPS2=A — `scope: 'ready-across'` spans every epoch and returns every
// card an agent could pick up right now, lane first. laneOf already drops a
// card while any blockedBy prerequisite is unfinished, so this list IS the
// parallel-safe set: once dependencies are recorded, work that must wait
// simply isn't in it.
export function nextCards(s, { epoch, track, agent, limit = 5, scope } = {}) {
  const proj = project(s);
  const chainActive = epochChain(s.epochs).find(e => e.role === 'active')?.id ?? null;
  const pool = proj.cards.filter(c => {
    if (!(c.lane.lane in LANE_PREF)) return false;
    if (epoch && c.epoch !== epoch) return false;
    if (track && c.track !== track) return false;
    if (hasActiveClaim(c) && agent && c.assignee !== agent) return false;
    if (scope === 'burndown') {
      const inEpoch = c.track === 'epoch' && c.epoch === chainActive;
      if (!inEpoch && c.track !== 'sidequest') return false;
    }
    return true;
  });
  const rank = chainRank(s.epochs);
  if (scope === 'ready-across') {
    pool.sort((a, b) =>
      LANE_PREF[a.lane.lane] - LANE_PREF[b.lane.lane]
      || rank(a.epoch) - rank(b.epoch)
      || (a.priority || '').localeCompare(b.priority || '')
      || (a.workOrder ?? Infinity) - (b.workOrder ?? Infinity)
      || a.num - b.num);
  } else {
    pool.sort((a, b) =>
      rank(a.epoch) - rank(b.epoch)
      || LANE_PREF[a.lane.lane] - LANE_PREF[b.lane.lane]
      || (a.workOrder ?? Infinity) - (b.workOrder ?? Infinity)
      || (a.priority || '').localeCompare(b.priority || '')
      || a.num - b.num);
  }
  return pool.slice(0, limit);
}

// ---- brief: one-shot agent work packet (#462, D-TWR-BRIEF1=A) -------------
// Goal: an agent that reads ONE `tower brief` needs zero other reads to
// start the card. Decisions are copied VERBATIM off the live store — never
// paraphrased, that's how stale-ballot bugs happen (#458's ballot-ready
// standard: the owner decides from the ballot text alone, so the agent
// briefing off it must see the same text).

// Path-looking tokens auto-harvested out of body/plan text. Trailing
// sentence punctuation (. , ; : ) ] " ') never becomes part of the match —
// the final captured char is always a path char ([\w/]), so a greedy match
// backtracks off any trailing punctuation.
const REF_RE = /\b(?:Docs|Examples|Tools|Source|crates|tests|Tower)\/[\w./-]*[\w/]/g;
function harvestRefs(text) {
  return text ? [...String(text).matchAll(REF_RE)].map(m => m[0]) : [];
}

const BRIEF_RULES = [
  'Log advances with --by.',
  'Code workers return CHECK OK from Tools/Agent/LaneCheck.jet; prose/static-data-only workers return DOCS ONLY.',
  'Workers never write Tower or run cargo test, generators, bless commands, cargo fmt, proof-parallel.sh, or Tools/Agent/VerifyFull.jet.',
  'After integration, the orchestrator runs the exact criterion proof once, records its real evidence, and marks the row met.',
  'The orchestrator closes immediately when every row is met or verified.',
  'E_CLOSE_READY blocks every further brief or claim until the closure-ready card closes, reopens, or gains a real gate.',
  'Verified criteria are milestone-review signoff; the reviewer must differ from the agent that marked the row met (E_CRITERIA_SELF).',
  'Broad proof and fresh review run once after every milestone card closes and a commit-bound closeout token opens.',
  'Integration and no-known-blocker are orchestration evidence, not mandatory card criteria.',
  'needsAcceptance ONLY for visual/UI/UX/DX taste or real-world eyes — never technical correctness.',
  'Release mid-card needs --handoff.',
];

// Ratified decisions surface the owner's ratification comment IN FULL (never
// truncated/paraphrased); open/draft ones surface the whole ballot — options
// included — since that's what the owner would need to decide from.
function decisionForBrief(d) {
  const base = { id: d.id, cardId: d.cardId, group: d.group, status: d.status, draft: !!d.draft,
    title: d.title, gist: d.gist, ballotMode: d.ballotMode ?? null, shortAuthorizedBy: d.shortAuthorizedBy ?? null,
    ballotProcessVersion: d.ballotProcessVersion ?? null,
    visualMedia: d.visualMedia || [], reports: d.reports || [],
    outcome: d.outcome ?? null, comment: d.comment ?? '', ratifiedAt: d.ratifiedAt ?? null };
  if (d.status === 'ratified') return base;
  return { ...base, situation: d.situation ?? '', lesson: d.lesson, story: d.story, explainer: d.explainer, inWild: d.inWild, detail: d.detail, rec: d.rec,
    recommendation: d.recommendation, hybrid: d.hybrid, reviewPasses: d.reviewPasses,
    options: d.options || [], comparisons: d.comparisons || [] };
}

// blockedBy accepts a card ref OR a decision id (#458) — resolve each to its
// live done/ratified state so the packet never needs a second lookup.
function blockerState(s, id) {
  const bc = findCard(s, id);
  if (bc) return { id, kind: 'card', num: bc.num, title: bc.title, phase: bc.phase, done: bc.phase === 'done' };
  const bd = s.decisions.find(x => x.id === id);
  if (bd) return { id, kind: 'decision', title: bd.title, status: bd.status, done: bd.status === 'ratified' };
  return { id, kind: 'unknown', done: false };   // dangling ref — same as laneOf's treatment
}

export function buildBrief(s, ref) {
  const card = mustCard(s, ref);
  const epoch = card.epoch ? s.epochs.find(e => e.id === card.epoch) : null;
  // A retired milestone stays out of briefs; the card keeps its link.
  const milestone = card.milestoneId ? s.milestones.find(m => m.id === card.milestoneId && m.status !== 'retired') : null;
  const explicitRefs = Array.isArray(card.refs) ? card.refs : [];
  const harvested = [...harvestRefs(card.body), ...harvestRefs(card.plan)];
  return {
    card: {
      id: card.id, num: card.num, title: card.title, body: card.body, plan: card.plan,
      probe: card.probe || null,
      visualMedia: card.visualMedia || [],
      phase: card.phase, priority: card.priority, workOrder: card.workOrder ?? null,
      assignee: card.assignee ?? null, track: card.track,
      epoch: card.epoch ? {
        id: card.epoch, label: epochLabel(epoch || { id: card.epoch }), name: epoch?.name ?? null, goal: epoch?.goal ?? null,
        role: epochChain(s.epochs).find(e => e.id === card.epoch)?.role ?? null,
      } : null,
      milestone: milestone ? { id: milestone.id, title: milestone.title, goal: milestone.goal, status: milestone.status, exitCriteria: milestone.exitCriteria, verification: milestone.verification || null } : null,
    },
    blockers: (card.blockedBy || []).map(id => blockerState(s, id)),
    criteria: { items: card.criteria || [], needsAcceptance: !!card.needsAcceptance },
    decisions: s.decisions.filter(d => d.cardId === card.id).map(decisionForBrief),
    questions: s.questions.filter(q => q.cardId === card.id && q.kind !== 'message' && q.status === 'open').map(q => ({ id: q.id, by: q.by, text: q.text })),
    refs: [...new Set([...explicitRefs, ...harvested])],
    log: (card.log || []).slice(0, 5),
    rules: BRIEF_RULES,
  };
}

// Completion cursor: done cards after this instant appear in the owner's
// queue. Agent messages ignore this cursor and stay until marked done.
export function setCompletionCursor(s, at) {
  s.meta.completionCursor = at || now();
  return { completionCursor: s.meta.completionCursor };
}

export const setDigestCursor = setCompletionCursor;

export function clearDoneQueue(s, { at, by } = {}) {
  if (by !== 'owner') fail('E_OWNER_ONLY', 'clearing completed cards is owner-only');
  const result = setCompletionCursor(s, at);
  logEvent(s, {
    by,
    action: 'done.clear',
    note: result.completionCursor,
  });
  return result;
}

// ---- ui state ---------------------------------------------------------------

export function toggleOpen(s, key) {
  const set = new Set(s.meta.ui.toggled || []);
  set.has(key) ? set.delete(key) : set.add(key);
  s.meta.ui.toggled = [...set];
  return s.meta.ui.toggled;
}
