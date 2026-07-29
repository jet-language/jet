// Tower store: one JSON data file per host project, one source of truth.
//
// Model (v4):
//   epochs      — major groupings of work
//   milestones  — goals WITHIN an epoch; cards can point at one
//   cards       — the work items; every card computes to exactly ONE lane
//   decisions   — ballot-ready choices blocking a card, owner-only to ratify
//   questions   — owner ⇄ agent notes/questions on a card
//   ideas       — lightweight capture; promotable to cards
//   events      — append-only audit trail of every mutation
//
// Lane state is DERIVED on every read (never stored), so a card and its
// decisions can never desync. Only one lane ever blocks the owner: `decide`.
// Everything else is an agent's, inert, or done. #516: there is no
// greenlight/activate gate — a fresh card lands straight in an agent lane;
// the owner's only confirmation mechanism is ratifying a decision ballot.
import { dataFile, historyFile, readJSON, writeJSON, backup, newId, today, now } from './paths.mjs';
import { withLock } from './lock.mjs';
import { loadConfig, publicConfig } from './config.mjs';
import {
  hasPendingRepair, recoverPendingRepair, recoverPendingRepairLocked,
} from './repair-journal.mjs';

export const VERSION = 4;
export const CLAIM_TTL_MS = 24 * 60 * 60 * 1000;

export const PHASES = [
  { id: 'deciding', label: 'Deciding', seq: 0, who: 'owner', blurb: 'Blocked on a decision' },
  { id: 'planning', label: 'Planning', seq: 1, who: 'agent', blurb: 'Build a plan + raise the decisions it needs' },
  { id: 'ready',    label: 'Ready',    seq: 2, who: 'agent', blurb: 'Plan vetted, decisions cleared — implement it' },
  { id: 'building', label: 'Building', seq: 3, who: 'agent', blurb: 'Implementation in progress' },
  { id: 'verify',   label: 'Verify',   seq: 4, who: 'agent', blurb: 'Claimed done — verify 100%, then close' },
  { id: 'done',     label: 'Done',     seq: 5, who: null,    blurb: 'Verified — hidden' },
  { id: 'frozen',   label: 'Frozen',   seq: -1, who: 'owner', blurb: 'Owner-only — paused; the owner unpauses with a phase update' },
];
export const PHASE_IDS = PHASES.map(p => p.id);
export const ACTIVE = ['deciding', 'planning', 'ready', 'building', 'verify'];

export const LANES = {
  decide:    { who: 'owner', label: 'Decide',    rank: 0 },
  plan:      { who: 'agent', label: 'Plan',      rank: 1 },
  implement: { who: 'agent', label: 'Implement', rank: 2 },
  building:  { who: 'agent', label: 'Building',  rank: 3 },
  verify:    { who: 'agent', label: 'Verify',    rank: 4 },
  blocked:   { who: null,    label: 'Blocked',   rank: 5 },
  frozen:    { who: 'owner', label: 'Frozen',    rank: 6 },
  done:      { who: null,    label: 'Done',      rank: 7 },
};

export class TowerError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
const fail = (code, msg) => { throw new TowerError(code, msg); };

export const empty = (project = 'Project') => ({
  meta: { version: VERSION, project, currentEpoch: null, nextNum: 1, rev: 0, ui: { toggled: [] } },
  epochs: [], milestones: [], cards: [], decisions: [], questions: [], ideas: [], events: [],
});

// ---- store handle ---------------------------------------------------------

export function openStore(dataDir) {
  const file = dataFile(dataDir);
  if (!file) fail('E_NO_DATA', 'no Tower data found — run `tower init` in your project root (or set TOWER_DATA)');
  const config = loadConfig(dataDir);

  // A repair journal is the two-store commit marker. Check before and after
  // unlocked reads so a concurrent repair cannot expose a persistent split.
  recoverPendingRepair(dataDir, file);
  const consistentRead = (read) => {
    if (hasPendingRepair(dataDir)) recoverPendingRepair(dataDir, file);
    let value = read();
    if (hasPendingRepair(dataDir)) {
      recoverPendingRepair(dataDir, file);
      value = read();
    }
    return value;
  };
  const loadRaw = () => normalize(readJSON(file, empty(config.project)));
  const load = () => consistentRead(loadRaw);

  // history.json can change through another CLI process. Read it fresh so a
  // long-lived server handle cannot retain a pre-repair archive indefinitely.
  const loadHistory = () => consistentRead(() => loadHistoryRaw(dataDir));

  // Read-modify-write under the cross-process lock; rev bumps on every write.
  // `expectRev` (optional) enables optimistic concurrency for API callers.
  const mutate = (fn, { expectRev } = {}) => withLock(file, () => {
    recoverPendingRepairLocked(dataDir);
    const s = loadRaw();
    if (expectRev != null && Number(expectRev) !== s.meta.rev)
      fail('E_CONFLICT', `stale rev: expected ${expectRev}, store is at ${s.meta.rev} — re-read state and retry`);
    const result = fn(s, config);
    // #461: single chokepoint — every write gets a chance to retire aged-out
    // cards/decisions/events to history.json before tower.json is persisted.
    syncMilestones(s, undefined, loadHistoryRaw(dataDir).cards);
    retire(s, config, dataDir);
    s.meta.rev += 1;
    backup(file, config.backups);
    writeJSON(file, s);
    return { result, state: s };
  });

  // Replace the whole state (undo). Guarded by expectRev so an interleaved
  // write from another agent can never be silently reverted. Undo touches
  // ONLY tower.json — history.json is append-only and never rolled back
  // (see test/history.test.mjs for the duplicate-tolerance this buys).
  const restore = (prevState, { expectRev } = {}) => withLock(file, () => {
    recoverPendingRepairLocked(dataDir);
    const cur = loadRaw();
    if (expectRev != null && Number(expectRev) !== cur.meta.rev)
      fail('E_CONFLICT', `undo refused: board changed since (rev ${cur.meta.rev} ≠ ${expectRev})`);
    const s = normalize(prevState);
    s.meta.rev = cur.meta.rev + 1;
    backup(file, config.backups);
    writeJSON(file, s);
    return { result: { restored: true }, state: s };
  });

  // Bring a single archived card or decision back to the live board
  // (D-TWR-ARCHIVE1=B). Resets its clock (updated/ratifiedAt = today) so it
  // doesn't immediately re-retire on the next write.
  const restoreArchived = (ref, by) => withLock(file, () => {
    recoverPendingRepairLocked(dataDir);
    const s = loadRaw();
    const h = loadHistoryRaw(dataDir);
    const result = restoreFromHistory(s, h, ref, by);
    syncMilestones(s, undefined, h.cards);
    s.meta.rev += 1;
    backup(file, config.backups);
    writeJSON(file, s);
    writeJSON(historyFile(dataDir), h);
    return { result, state: s };
  });

  return {
    file, dataDir, config, load, mutate, restore, restoreArchived, loadHistory,
    project: () => project(load(), config, loadHistory()),
  };
}

// ---- history: split live/archive store (#461) ------------------------------
// D-TWR-ARCHIVE1=B MODIFIED by owner comment: nothing retires immediately —
// a buffer window (config.retireAfterDays, default 3) lets the owner walk
// back a fresh ratification before it's out of easy reach. Append-only
// ledger at <dataDir>/history.json, written under the SAME lock as
// tower.json (see `mutate`/`restoreArchived` above), committed to git.
export const emptyHistory = () => ({ version: 1, decisions: [], cards: [], events: [] });

function loadHistoryRaw(dataDir) {
  return { ...emptyHistory(), ...(readJSON(historyFile(dataDir), null) || {}) };
}

// Treat a 'YYYY-MM-DD' stamp as UTC midnight; "older than N days" = more
// than N*86400000ms have elapsed since then.
function isOlderThanDays(dateStr, days) {
  if (!dateStr) return false;
  const t = Date.parse(`${dateStr}T00:00:00Z`);
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

// The one retirement chokepoint (called only from `mutate`, right after the
// caller's fn() runs, before the write). Idempotent: an id already present
// in history.json is removed from live without a duplicate append — undo
// can reintroduce a stale live copy of something already retired (history is
// never rolled back), and this is how that self-heals on the next write.
function retire(s, config, dataDir) {
  const days = config.retireAfterDays ?? 3;
  const h = loadHistoryRaw(dataDir);
  const hasCard = (id) => h.cards.some(x => x.id === id);
  const hasDecision = (id) => h.decisions.some(x => x.id === id);
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

  // (c) events: keep the newest 500 live; archive the overflow (oldest-first
  // within the archived batch, appended to the tail of the ledger).
  if (s.events.length > LIVE_EVENTS) {
    const overflow = s.events.slice(LIVE_EVENTS).reverse();
    h.events.push(...overflow);
    s.events = s.events.slice(0, LIVE_EVENTS);
    dirty = true;
  }

  if (dirty) writeJSON(historyFile(dataDir), h);
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

export function normalize(s) {
  s = s && typeof s === 'object' ? s : empty();
  s.meta = { version: VERSION, project: 'Project', nextNum: 1, rev: 0, ...(s.meta || {}) };
  s.meta.version = VERSION;
  s.meta.ui = { toggled: [], ...(s.meta.ui || {}) };
  if (s.meta.completionCursor == null && s.meta.digestCursor != null)
    s.meta.completionCursor = s.meta.digestCursor;
  delete s.meta.digestCursor;
  for (const k of ['epochs', 'milestones', 'cards', 'decisions', 'questions', 'ideas', 'events']) s[k] ||= [];
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
    c.blockedBy ||= [];
    c.log ||= [];
    c.criteria ||= [];
    c.refs ||= [];
    c.tags ||= [];
    if (!('parentId' in c)) c.parentId = null;
    c.needsAcceptance = !!c.needsAcceptance;
  }
  for (const d of s.decisions) d.draft = !!d.draft;
  return s;
}

// D-TWR-OPS1=A: the single source of truth for "which epoch is live".
export const activeEpoch = (s) => s.epochs.find(e => e.status === 'active')?.id ?? null;

// ---- derivation: clearance + lane (the ONE place this is decided) ---------

// A draft decision (card #458, D-TWRGUARD1=C) is a scratch ballot still being
// written — it never blocks a card and never shows in the owner's queue.
const isBlocking = (d) => d.status !== 'ratified' && !d.draft;

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
    // needsAcceptance = owner visual/UX/DX taste only. Bare verify = agent technical closeout.
    if (card.needsAcceptance) {
      const acceptOpen = decisions.some(d => d.id === `D-ACCEPT-${card.num}` && d.status !== 'ratified');
      return acceptOpen
        ? { lane: 'verify', who: 'owner', label: 'Owner visual/UX acceptance' }
        : { lane: 'verify', who: 'agent', label: 'Finish criteria, then owner visual check' };
    }
    return { lane: 'verify', who: 'agent', label: 'Agent technical verify, then close' };
  }
  return { lane: 'blocked', who: null, label: '' };
}

function milestoneCards(id, cards, historyCards = []) {
  const linked = new Map();
  for (const c of historyCards) if (c.milestoneId === id) linked.set(c.id, c);
  for (const c of cards) if (c.milestoneId === id) linked.set(c.id, c);
  return [...linked.values()];
}

export function milestoneProgress(m, cards, historyCards = []) {
  const linked = milestoneCards(m.id, cards, historyCards);
  const done = linked.filter(c => c.phase === 'done').length;
  return { total: linked.length, done, met: linked.length > 0 && done === linked.length };
}

function syncMilestone(s, id, historyCards = []) {
  const m = s.milestones.find(x => x.id === id);
  if (!m) return;
  const linked = milestoneCards(id, s.cards, historyCards);
  if (!linked.length) {
    m.status = 'open';
    delete m.metAt;
    return;
  }
  const met = linked.every(c => c.phase === 'done');
  m.status = met ? 'met' : 'open';
  if (met) m.metAt ||= today();
  else delete m.metAt;
}

function syncMilestones(s, ids = s.milestones.map(m => m.id), historyCards = []) {
  for (const id of new Set(ids.filter(Boolean))) syncMilestone(s, id, historyCards);
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
  const activeEpochs = s.epochs.filter(e => !['arrived', 'done'].includes(e.status));
  const cur = activeEpoch(s);
  const sorted = [...activeEpochs].sort((a, b) => {
    const aCur = a.id === cur, bCur = b.id === cur;
    if (aCur !== bCur) return aCur ? -1 : 1;
    return (a.order ?? a.num ?? 999) - (b.order ?? b.num ?? 999);
  });
  return sorted.map(e => {
    const all = s.cards.filter(c => c.epoch === e.id && c.track !== 'sidequest');
    const active = all.filter(c => !['done', 'frozen'].includes(c.phase));
    const done = all.filter(c => c.phase === 'done');
    const milestones = s.milestones.filter(m => m.epochId === e.id).map(m => {
      const progress = milestoneProgress(m, s.cards, historyCards);
      return { id: m.id, title: m.title, goal: m.goal, met: m.status === 'met',
        ...progress, stalledDays: milestoneStallDays(m, s.cards, s.events) };
    });
    const milestonesMet = milestones.filter(m => m.met).length;
    const milestoneTotal = milestones.length;
    const pct = milestoneTotal ? Math.round(milestonesMet / milestoneTotal * 100) : 0;
    return {
      id: e.id, name: e.name, goal: e.goal,
      active: active.length, done: done.length, doneArchivedHint: null,
      milestoneTotal, milestonesMet, pct, burndown: burndown30(s, e.id), milestones,
    };
  });
}

export function project(s, config = null, history = null) {
  const cards = s.cards.map(c => {
    const clearance = clearanceOf(c, s.decisions);
    const decisions = s.decisions.filter(d => d.cardId === c.id);
    const questions = s.questions.filter(q => q.cardId === c.id);
    const openQ = questions.filter(q => q.kind !== 'message' && q.status === 'open').length;
    return { ...c, clearance, decisions, questions, openQ, lane: laneOf(c, s.decisions, s.cards) };
  });
  const historyCards = history?.cards || [];
  const milestones = s.milestones.map(m => ({ ...m, progress: milestoneProgress(m, s.cards, historyCards) }));
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
  };
  return { meta: s.meta, config: publicConfig(config) || undefined, epochs: s.epochs, milestones, phases: PHASES, lanes: LANES,
    cards, decisions: s.decisions, questions: s.questions, ideas: s.ideas,
    events: s.events.slice(0, 300), counts, recentlyDecided, radar: radarData(s, historyCards) };
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

const checkEnum = (val, list, what) => {
  if (val != null && !list.includes(val)) fail('E_INVALID', `${what} must be one of: ${list.join(', ')} (got ${JSON.stringify(val)})`);
};
const checkEpoch = (s, id) => { if (id != null && !s.epochs.find(e => e.id === id)) fail('E_NOT_FOUND', `no epoch ${id}`); };
const checkMilestone = (s, id) => { if (id != null && !s.milestones.find(m => m.id === id)) fail('E_NOT_FOUND', `no milestone ${id}`); };
function checkCardMilestone(s, { epoch, track, milestoneId }) {
  if (milestoneId == null) return;
  const milestone = s.milestones.find(m => m.id === milestoneId);
  if (track === 'sidequest') fail('E_INVALID', 'sidequest cards cannot link to milestones');
  if (milestone.epochId !== epoch)
    fail('E_INVALID', `milestone ${milestone.id} belongs to epoch ${milestone.epochId}, not ${epoch || 'no epoch'}`);
}
// #462: refs — free-form doc-path pointers a card carries explicitly (in
// addition to whatever `tower brief` harvests out of body/plan).
const checkRefs = (val) => {
  if (val !== undefined && !(Array.isArray(val) && val.every(x => typeof x === 'string')))
    fail('E_INVALID', 'refs must be an array of strings');
};

export function logEvent(s, { by = 'agent', action, ref = null, note = '' }) {
  s.events.unshift({ at: now(), by, action, ref, note });
  if (s.events.length > 2000) s.events.length = 2000;
}

// ---- mutations: cards ------------------------------------------------------

// One exit-criteria item: 1-based stable n, open -> met (builder) -> verified
// (a different agent). Card-embedded, no own id — addressed by (card, n).
function normalizeCriterion(it, i) {
  return {
    n: it.n ?? (i + 1),
    text: String(it.text || '').trim(),
    status: ['open', 'met', 'verified'].includes(it.status) ? it.status : 'open',
    metBy: it.metBy ?? null,
    verifiedBy: it.verifiedBy ?? null,
    evidence: it.evidence || '',
    at: it.at || now(),
  };
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

export function addCard(s, p, config) {
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'card needs a title');
  checkEnum(p.kind, config.kinds, 'kind');
  checkEnum(p.track, config.tracks, 'track');
  checkEnum(p.priority, config.priorities, 'priority');
  checkEnum(p.phase, PHASE_IDS, 'phase');
  const epoch = p.epoch ?? activeEpoch(s);
  const track = p.track || config.tracks[0];
  checkEpoch(s, epoch); checkMilestone(s, p.milestoneId);
  checkCardMilestone(s, { epoch, track, milestoneId: p.milestoneId });
  checkRefs(p.refs);
  const parentId = 'parentId' in p || 'parent' in p
    ? resolveParentId(s, p.parentId ?? p.parent)
    : null;
  if (parentId && parentId === (p.id || null))
    fail('E_INVALID', 'card cannot parent itself');
  const card = {
    id: p.id || newId('c'),
    num: p.num || s.meta.nextNum++,
    title: String(p.title).trim(),
    body: p.body || '',
    kind: p.kind || config.kinds[0],
    track,
    epoch,
    milestoneId: p.milestoneId || null,
    phase: p.phase || 'planning',
    priority: p.priority || config.priorities[2] || config.priorities.at(-1),
    plan: p.plan || null,
    blockedBy: p.blockedBy || [],
    workOrder: p.workOrder != null ? Number(p.workOrder) : undefined,
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
  return card;
}

const CARD_FIELDS = ['title', 'body', 'kind', 'track', 'epoch', 'milestoneId', 'phase', 'priority', 'plan', 'blockedBy', 'workOrder', 'criteria', 'needsAcceptance', 'refs', 'tags', 'parentId'];

// D-TWR-CRIT1=C / D-TWRGUARD1=C: gate --phase done. Criteria remain
// owner-soft, but needsAcceptance is transport-hard: caller attribution can
// never stand in for the dedicated owner UI provenance. Once criteria clear,
// an agent write mints the acceptance ballot and parks the card in verify.
function applyDoneGate(s, c, targetPhase, by) {
  if (targetPhase !== 'done') return null;
  if (c.needsAcceptance && by === 'owner')
    fail('E_ACCEPTANCE_OWNER_UI', `card #${c.num} requires owner verification — caller-supplied by:owner cannot close it; use the dedicated owner verification UI`);
  const items = c.criteria || [];
  const unverified = items.filter(i => i.status !== 'verified');
  const gated = items.length > 0 && unverified.length > 0;
  if (gated && by !== 'owner') {
    fail('E_CRITERIA', `${unverified.length} of ${items.length} criteria unverified (${unverified.map(i => i.n).join(',')}); verifier must not be the builder`);
  }
  if (gated && by === 'owner') {
    logEvent(s, { by, action: 'card.criteria-bypass', ref: c.id, note: 'owner bypass' });
    return null;
  }
  if (c.needsAcceptance) {
    mintAcceptance(s, c);
    return 'verify';
  }
  return null;
}

// #515 pass 2 (2026-07-12, owner directive): acceptance entries were too
// long and demanded commands the owner can't run away from his computer.
// `proof` is one short machine-evidence line per criterion (what already
// ran, not what to go run); `visualCheck` is AT MOST one line, and only
// present when the change is actually visual — never an instruction to run
// a command. Additive/replacing field on the acceptance ballot; old ballots
// minted before this carry the old {toCheck,confirms} shape until a bounce
// or an explicit remint refreshes them — the Now page renders both.
const PROOF_LINE_MAX = 100;
const shorten = (s, n = PROOF_LINE_MAX) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);
const VISUAL_REF_RE = /Tower\/app\/ui\/|\.(png|jpe?g|gif|svg)$|canvas|screenshot|\/web(\/|$)/i;

export function acceptanceCheckInstructions(c) {
  const items = c.criteria || [];
  const refs = c.refs || [];
  const proof = items
    .filter(i => i.status !== 'open')
    .map(i => shorten(`${i.text} — ${i.status}${i.evidence ? ` (${i.evidence})` : ''}`));
  const visualRef = refs.find(r => VISUAL_REF_RE.test(r));
  const visualCheck = visualRef ? `Open ${visualRef} — glance, confirm it looks right.` : null;
  return (proof.length || visualCheck) ? { proof, visualCheck } : null;
}

function mintAcceptance(s, c) {
  const id = `D-ACCEPT-${c.num}`;
  const existing = s.decisions.find(d => d.id === id);
  if (existing && existing.status !== 'ratified') return; // already awaiting owner — no duplicate mint
  const items = c.criteria || [];
  const evidence = items.length
    ? items.map(i => `${i.n}. ${i.text} — ${i.status}${i.evidence ? ` (${i.evidence})` : ''}${i.verifiedBy ? ` [verified by ${i.verifiedBy}]` : ''}`).join('\n')
    : '(no exit criteria on this card — direct acceptance request)';
  const checkInstructions = acceptanceCheckInstructions(c);
  if (existing) {
    // a prior round was bounced; re-open the same ballot id for round 2
    existing.status = 'open';
    existing.detail = evidence;
    existing.checkInstructions = checkInstructions;
    delete existing.outcome; delete existing.comment; delete existing.ratifiedAt;
  } else {
    addDecision(s, {
      id, cardId: c.id, group: 'acceptance',
      title: `Accept #${c.num} — ${c.title}`,
      gist: `Close #${c.num}, or bounce it back to building.`,
      detail: evidence,
      checkInstructions,
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
  // Basic shape validation runs before the owner-lane authorization check, so
  // a malformed request always reports E_INVALID/E_NOT_FOUND regardless of
  // who sent it or what lane the card is in.
  checkEnum(patch.kind, config.kinds, 'kind');
  checkEnum(patch.track, config.tracks, 'track');
  checkEnum(patch.priority, config.priorities, 'priority');
  checkEnum(patch.phase, PHASE_IDS, 'phase');
  if ('epoch' in patch) checkEpoch(s, patch.epoch);
  if ('milestoneId' in patch) checkMilestone(s, patch.milestoneId);
  checkCardMilestone(s, {
    epoch: 'epoch' in patch ? patch.epoch : c.epoch,
    track: 'track' in patch ? patch.track : c.track,
    milestoneId: 'milestoneId' in patch ? patch.milestoneId : c.milestoneId,
  });
  // blockedBy accepts a card ref OR a decision id (D-TWRGUARD1=C #458).
  if ('blockedBy' in patch) for (const id of patch.blockedBy || []) {
    if (!findCard(s, id) && !s.decisions.find(d => d.id === id))
      fail('E_NOT_FOUND', `blockedBy: no card or decision ${id}`);
  }
  if ('refs' in patch) checkRefs(patch.refs);
  if ('tags' in patch) patch.tags = normalizeTags(patch.tags);
  if ('parentId' in patch || 'parent' in patch) {
    const resolved = resolveParentId(s, 'parentId' in patch ? patch.parentId : patch.parent);
    if (resolved === c.id) fail('E_INVALID', 'card cannot parent itself');
    patch.parentId = resolved;
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
  const phaseOverride = 'phase' in patch ? applyDoneGate(s, c, patch.phase, patch.by) : null;
  for (const k of CARD_FIELDS) {
    if (k in patch) {
      if (k === 'phase') c.phase = phaseOverride || patch.phase;
      else if (k === 'workOrder') c[k] = patch[k] == null || patch[k] === '' ? undefined : Number(patch[k]);
      else if (k === 'needsAcceptance') c.needsAcceptance = patch.needsAcceptance === true || patch.needsAcceptance === 'true';
      else if (k === 'criteria') c.criteria = Array.isArray(patch.criteria) ? patch.criteria.map((it, i) => normalizeCriterion(it, i)) : c.criteria;
      else if (k === 'tags') c.tags = patch.tags;
      else c[k] = patch[k];
    }
  }
  if (c.assignee && c.assignee === patch.by) c.claimedAt = now();
  if (c.phase === 'done' || c.phase === 'frozen') {
    c.assignee = null;
    delete c.claimedAt;
  }
  if (oldPhase !== 'done' && c.phase === 'done') c.completedAt = now();
  if (oldPhase === 'done' && c.phase !== 'done') delete c.completedAt;
  if (patch.logEntry) c.log.unshift({ at: today(), by: patch.by || 'agent', text: patch.logEntry });
  touchCard(c, patch.by);
  syncMilestones(s, [oldMilestoneId, c.milestoneId]);
  logEvent(s, { by: patch.by, action: 'card.update', ref: c.id, note: Object.keys(patch).filter(k => k !== 'id' && k !== 'by').join(',') });
  return c;
}

// ---- mutations: exit criteria ----------------------------------------------

export function addCriterion(s, ref, text, by) {
  const c = mustCard(s, ref);
  if (!text || !String(text).trim()) fail('E_INVALID', 'criterion needs text');
  c.criteria ||= [];
  const n = (c.criteria.length ? Math.max(...c.criteria.map(i => i.n)) : 0) + 1;
  const item = { n, text: String(text).trim(), status: 'open', metBy: null, verifiedBy: null, evidence: '', at: now() };
  c.criteria.push(item);
  touchCard(c, by);
  logEvent(s, { by, action: 'card.criteria-add', ref: c.id, note: `#${n} ${item.text.slice(0, 60)}` });
  return { ...item, cardId: c.id, cardNum: c.num };
}

function mustCriterion(c, n) {
  const item = (c.criteria || []).find(i => i.n === Number(n));
  if (!item) fail('E_NOT_FOUND', `no criterion #${n} on card #${c.num}`);
  return item;
}

export function meetCriterion(s, ref, n, { evidence, by } = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'meet needs --by <agent>');
  item.status = 'met';
  item.metBy = by;
  if (evidence != null) item.evidence = evidence;
  item.at = now();
  touchCard(c, by);
  logEvent(s, { by, action: 'card.criteria-meet', ref: c.id, note: `#${item.n}` });
  return { ...item, cardId: c.id, cardNum: c.num };
}

export function verifyCriterion(s, ref, n, { evidence, by } = {}) {
  const c = mustCard(s, ref);
  const item = mustCriterion(c, n);
  if (!by) fail('E_INVALID', 'verify needs --by <agent>');
  if (item.status === 'open') fail('E_INVALID', `criterion #${n} not met yet — meet it before verifying`);
  if (by === item.metBy) fail('E_CRITERIA_SELF', `criterion #${n} verifier must not be the builder (${by})`);
  item.status = 'verified';
  item.verifiedBy = by;
  if (evidence != null) item.evidence = evidence;
  item.at = now();
  touchCard(c, by);
  logEvent(s, { by, action: 'card.criteria-verify', ref: c.id, note: `#${item.n}` });
  return { ...item, cardId: c.id, cardNum: c.num };
}

// D-TWRGUARD1=C (#458): a card with any ratified decision refuses delete for
// everyone, owner included — a ratified decision is durable record, never a
// casualty of tidying up. #461 gives it a real way out: the decisions retire
// to history.json on their own (`tower archive status`) once their buffer
// window passes, or bring one back early with `tower archive restore <id>`;
// either way, delete only once none are live on the card.
export function deleteCard(s, ref, p = {}) {
  const c = mustCard(s, ref);
  const oldMilestoneId = c.milestoneId;
  const openMessages = s.questions.filter(q => q.cardId === c.id && q.kind === 'message' && q.status === 'open');
  if (openMessages.length)
    fail('E_INVALID', `card #${c.num} has ${openMessages.length} open message${openMessages.length === 1 ? '' : 's'} — mark each message done before deleting the card`);
  const ratified = s.decisions.filter(d => d.cardId === c.id && d.status === 'ratified');
  if (ratified.length)
    fail('E_HAS_RATIFIED', `card #${c.num} has ${ratified.length} ratified decision${ratified.length > 1 ? 's' : ''} (${ratified.map(d => d.id).join(', ')}) — they retire to \`tower archive\` on their own once the buffer window passes; delete once none are live on the card`);
  s.cards = s.cards.filter(x => x.id !== c.id);
  s.decisions = s.decisions.filter(d => d.cardId !== c.id);
  s.questions = s.questions.filter(q => q.cardId !== c.id);
  for (const x of s.cards) x.blockedBy = (x.blockedBy || []).filter(id => id !== c.id);
  syncMilestones(s, [oldMilestoneId]);
  logEvent(s, { by: p.by, action: 'card.delete', ref: c.id, note: c.title });
  return { ok: true, id: c.id, num: c.num };
}

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

export function claimCard(s, ref, by) {
  const c = mustCard(s, ref);
  if (!by) fail('E_INVALID', 'claim needs --by <agent>');
  if (by !== 'owner' && c.phase === 'frozen')
    fail('E_OWNER_LANE', `card #${c.num} is frozen — owner-only until the owner moves it out (\`tower card update --phase ... --by owner\`)`);
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

// D-TWRGUARD1=C (#458): the ballot-ready standard (tower-ballot skill) —
// gist/lesson/story/inWild/options[].code/rec/recommendation — enforced at write
// time. Acceptance
// ballots (`mintAcceptance` above) are a fixed system-generated evidence
// format, not a narrative ballot, and are exempt.
const PLAIN_SENTENCE_WORDS = 32;
const PLAIN_PARAGRAPH_WORDS = 90;
const words = (text) => String(text || '').match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) || [];

function proseDensityGaps(label, text) {
  if (!text || !String(text).trim()) return [];
  const gaps = [];
  const paragraphs = String(text).trim().split(/\n\s*\n/);
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

export function plainLanguageGaps(p) {
  const gaps = [
    ...proseDensityGaps('gist', p.gist),
    ...proseDensityGaps('lesson', p.lesson),
    ...proseDensityGaps('story', p.story),
  ];
  for (const option of p.options || [])
    gaps.push(...proseDensityGaps(`option ${option?.key || '?'}`, option?.detail));
  for (const comparison of p.comparisons || [])
    gaps.push(...proseDensityGaps(`comparison ${comparison?.lang || '?'}`, comparison?.note));
  const recommendation = p.recommendation || {};
  gaps.push(...proseDensityGaps('recommendation why', recommendation.why));
  gaps.push(...proseDensityGaps('recommendation tradeoff', recommendation.tradeoff));
  for (const rejected of recommendation.whyNot || [])
    gaps.push(...proseDensityGaps(`recommendation why not ${rejected?.key || '?'}`, rejected?.reason));
  return gaps;
}

export function ballotGaps(p) {
  const missing = [];
  if (!p.gist || !String(p.gist).trim()) missing.push('gist');
  if (!p.lesson || !String(p.lesson).trim()) missing.push('lesson');
  if (!p.story || !String(p.story).trim()) missing.push('story');
  if (!p.inWild || !String(p.inWild).trim()) missing.push('inWild');
  const opts = Array.isArray(p.options) ? p.options : [];
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
  }
  if (!p.rec || !String(p.rec).trim()) missing.push('rec');
  const optionKeys = opts.map(o => o?.key).filter(Boolean);
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
  }
  const dense = plainLanguageGaps(p);
  if (dense.length) missing.push(`plain language: ${dense.join('; ')}`);
  return missing;
}

export function addDecision(s, p) {
  const card = mustCard(s, p.cardId);
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'decision needs a title');
  if (p.id && s.decisions.find(d => d.id === p.id)) fail('E_INVALID', `decision id ${p.id} already exists`);
  const draft = !!p.draft;
  if (!draft && p.group !== 'acceptance') {
    const gaps = ballotGaps(p);
    if (gaps.length) fail('E_BALLOT', `ballot not ready — missing: ${gaps.join(', ')} (pass --draft to save a work-in-progress ballot)`);
  }
  const d = { id: p.id || newId('D-'), cardId: card.id, group: p.group || 'other',
    title: String(p.title).trim(), gist: p.gist || '', lesson: p.lesson || '', explainer: p.explainer || '', story: p.story || '',
    inWild: p.inWild || '', detail: p.detail || '', options: p.options || [], comparisons: p.comparisons || [],
    rec: p.rec || null, recommendation: p.recommendation || null, hybrid: p.hybrid || null,
    checkInstructions: p.checkInstructions || null, draft, status: 'open', created: now() };
  s.decisions.push(d);
  touchCard(card, p.by);
  logEvent(s, { by: p.by, action: 'decision.add', ref: d.id, note: draft ? `${d.title} (draft)` : d.title });
  return { ...d, cardNum: card.num };
}

const SYNTAX_RATIFY_CHORES = ['Syntax.rs entry updated', 'syntax-decisions.md log entry', 'jet devtools grammars regenerated', 'snapshots re-blessed'];

// D-TWRGUARD1=C (#458): ratifying a syntax-group decision auto-appends the
// standard post-ratification chores to the card's exit-criteria checklist
// (#463 model), skipping any that already exist.
function appendSyntaxChores(s, c, by) {
  if (!c) return;
  const have = new Set((c.criteria || []).map(i => i.text));
  for (const text of SYNTAX_RATIFY_CHORES) if (!have.has(text)) addCriterion(s, c.id, text, by || 'agent');
}

export function ratify(s, decisionId, outcome, comment, by, quote) {
  const d = s.decisions.find(x => x.id === decisionId) || fail('E_NOT_FOUND', `no decision ${decisionId}`);
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
    if (d.group !== 'acceptance' || !d.id.startsWith('D-ACCEPT-'))
      fail('E_ACCEPTANCE_OWNER_UI', `${d.id} is not an owner-verification ballot`);
    if (d.status === 'ratified') fail('E_INVALID', `${d.id} is already resolved`);
    if (outcome !== 'accept' && outcome !== 'bounce') fail('E_INVALID', 'acceptance outcome must be accept or bounce');
    if (!provenance || provenance.kind !== 'owner-ui' || !provenance.session || !provenance.challenge)
      fail('E_ACCEPTANCE_OWNER_UI', 'missing owner-verification provenance');
    const c = s.cards.find(x => x.id === d.cardId) || fail('E_NOT_FOUND', `no card for ${d.id}`);

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
  d.status = 'open'; delete d.outcome; delete d.ratifiedAt;
  const card = s.cards.find(c => c.id === d.cardId);
  if (card) touchCard(card, by || 'owner');
  logEvent(s, { by: by || 'owner', action: 'decision.reopen', ref: d.id });
  return d;
}

export function updateDecision(s, id, patch, by) {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  for (const k of ['title', 'gist', 'lesson', 'explainer', 'story', 'inWild', 'detail', 'options', 'comparisons', 'rec', 'recommendation', 'hybrid', 'checkInstructions', 'group'])
    if (k in patch) d[k] = patch[k];
  // --ready clears draft, but only once the ballot standard is actually met.
  if (patch.ready) {
    if (d.group !== 'acceptance') {
      const gaps = ballotGaps(d);
      if (gaps.length) fail('E_BALLOT', `ballot not ready — missing: ${gaps.join(', ')}`);
    }
    d.draft = false;
  }
  const card = s.cards.find(c => c.id === d.cardId);
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'decision.update', ref: d.id, note: patch.ready ? 'marked ready' : '' });
  return d;
}

// D-TWRGUARD1=C (#458): tower verdict — an owner ruling recorded as an
// already-ratified decision (never a log note) so it's durable + auditable.
// Owner-only, no quote exception (this IS the owner speaking).
export function mintVerdict(s, ref, outcome, title, by) {
  const c = mustCard(s, ref);
  if (by !== 'owner') fail('E_OWNER_ONLY', 'tower verdict is owner-only');
  if (!outcome || !String(outcome).trim()) fail('E_INVALID', 'verdict needs an outcome');
  let k = 1;
  while (s.decisions.find(x => x.id === `D-VERDICT-${c.num}-${k}`)) k++;
  const id = `D-VERDICT-${c.num}-${k}`;
  const d = { id, cardId: c.id, group: 'verdict',
    title: title || `Verdict on #${c.num} — ${c.title}`,
    gist: '', lesson: '', explainer: '', story: '', inWild: '', detail: '', options: [], comparisons: [],
    rec: null, recommendation: null, hybrid: null, draft: false, status: 'ratified', outcome, comment: outcome,
    created: now(), ratifiedAt: today() };
  s.decisions.push(d);
  c.log.unshift({ at: today(), by, text: `Verdict recorded (${id}): ${outcome}` });
  touchCard(c, by);
  logEvent(s, { by, action: 'decision.verdict', ref: id, note: outcome });
  return { ...d, cardNum: c.num };
}

export function deleteDecision(s, id, by) {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  s.decisions = s.decisions.filter(x => x.id !== id);
  const card = s.cards.find(c => c.id === d.cardId);
  if (card) touchCard(card, by);
  logEvent(s, { by, action: 'decision.delete', ref: id, note: d.title });
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
  const q = { id: newId('q'), cardId: card.id, decisionId: p.decisionId || null,
    by: p.by || 'owner', kind: p.kind || 'question',
    text: String(p.text).trim(), status: 'open', answer: '', created: now() };
  s.questions.push(q);
  touchCard(card, p.by || 'owner');
  logEvent(s, { by: p.by || 'owner', action: 'question.add', ref: q.id });
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

export function addEpoch(s, p) {
  if (!p.id || !String(p.id).trim()) fail('E_INVALID', 'epoch needs an id (e.g. e1)');
  if (s.epochs.find(e => e.id === p.id)) fail('E_INVALID', `epoch ${p.id} already exists`);
  const e = { id: p.id, name: p.name || p.id, goal: p.goal || '', status: p.status || 'open' };
  s.epochs.push(e);
  logEvent(s, { by: p.by, action: 'epoch.add', ref: e.id, note: e.name });
  return e;
}
export function updateEpoch(s, id, patch) {
  const e = s.epochs.find(x => x.id === id) || fail('E_NOT_FOUND', `no epoch ${id}`);
  // D-TWR-OPS1=A: at most one active epoch. Reject a second one honestly rather
  // than silently demoting the old epoch (which might not actually be finished).
  if (patch.status === 'active') {
    const other = s.epochs.find(x => x.id !== id && x.status === 'active');
    if (other) fail('E_INVALID', `${other.id} is already active — set it to arrived/planned before activating ${id}`);
  }
  for (const k of ['name', 'goal', 'status']) if (k in patch) e[k] = patch[k];
  return e;
}
// D-TWR-OPS1=A: `epoch current <id>` is now sugar for activating that epoch;
// `epoch current none` demotes the live epoch back to planned. The retired
// meta.currentEpoch field is gone — status is the only source of truth.
export function setCurrentEpoch(s, id) {
  if (id == null) {
    const cur = s.epochs.find(e => e.status === 'active');
    if (cur) cur.status = 'planned';
    return { active: null };
  }
  updateEpoch(s, id, { status: 'active' });
  return { active: id };
}

export function addMilestone(s, p) {
  checkEpoch(s, p.epochId || fail('E_INVALID', 'milestone needs --epoch <id>'));
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'milestone needs a title');
  const m = { id: p.id || newId('m'), epochId: p.epochId, title: String(p.title).trim(),
    goal: p.goal || '', criteria: p.criteria || '', status: 'open', created: now() };
  s.milestones.push(m);
  logEvent(s, { by: p.by, action: 'milestone.add', ref: m.id, note: m.title });
  return m;
}
export function updateMilestone(s, id, patch, by) {
  const m = s.milestones.find(x => x.id === id) || fail('E_NOT_FOUND', `no milestone ${id}`);
  if ('epochId' in patch && patch.epochId !== m.epochId)
    fail('E_INVALID', 'milestone epoch is fixed after creation — create a new milestone and relink cards');
  if ('status' in patch) checkEnum(patch.status, ['open', 'met'], 'milestone status');
  for (const k of ['title', 'goal', 'criteria', 'status', 'epochId']) if (k in patch) m[k] = patch[k];
  if (patch.status === 'met') m.metAt = today();
  logEvent(s, { by, action: 'milestone.update', ref: m.id });
  return m;
}
export function deleteMilestone(s, id, by) {
  const m = s.milestones.find(x => x.id === id) || fail('E_NOT_FOUND', `no milestone ${id}`);
  s.milestones = s.milestones.filter(x => x.id !== id);
  for (const c of s.cards) if (c.milestoneId === id) c.milestoneId = null;
  logEvent(s, { by, action: 'milestone.delete', ref: id, note: m.title });
  return { ok: true, id };
}

// ---- next: the canonical "what should an agent work on" picker -------------

const LANE_PREF = { verify: 0, building: 1, implement: 2, plan: 3 };

// #457 — `scope: 'burndown'` narrows the pool to exactly the current
// epoch's epoch-track cards plus all sidequests (agent lanes only) — the
// tower skill's "burndown loop" scope, made a real filter instead of
// something an agent has to hand-derive from the active epoch each time.
//
// D-TWR-OPS2=A — `scope: 'ready-across'` spans every epoch and returns every
// card an agent could pick up right now. laneOf already drops a card while any
// blockedBy prerequisite is unfinished, so this list IS the parallel-safe set:
// once dependencies are recorded, work that must wait simply isn't in it.
export function nextCards(s, { epoch, track, agent, limit = 5, scope } = {}) {
  const proj = project(s);
  const pool = proj.cards.filter(c => {
    if (!(c.lane.lane in LANE_PREF)) return false;
    if (epoch && c.epoch !== epoch) return false;
    if (track && c.track !== track) return false;
    if (hasActiveClaim(c) && agent && c.assignee !== agent) return false;
    if (scope === 'burndown') {
      const inEpoch = c.track === 'epoch' && c.epoch === activeEpoch(s);
      if (!inEpoch && c.track !== 'sidequest') return false;
    }
    return true;
  });
  // Verification always closes before more work continues. Within each lane,
  // ready-across groups by epoch; every other scope follows workOrder.
  if (scope === 'ready-across') {
    pool.sort((a, b) =>
      LANE_PREF[a.lane.lane] - LANE_PREF[b.lane.lane]
      || (a.epoch || '').localeCompare(b.epoch || '')
      || (a.priority || '').localeCompare(b.priority || '')
      || (a.workOrder ?? Infinity) - (b.workOrder ?? Infinity)
      || a.num - b.num);
  } else {
    pool.sort((a, b) =>
      LANE_PREF[a.lane.lane] - LANE_PREF[b.lane.lane]
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
const REF_RE = /\b(?:docs|examples|Source|crates|tests|Tower)\/[\w./-]*[\w/]/g;
function harvestRefs(text) {
  return text ? [...String(text).matchAll(REF_RE)].map(m => m[0]) : [];
}

const BRIEF_RULES = [
  'Log advances with --by.',
  'Phase honesty: building → verify → done.',
  'Criteria: meet as you finish; verifier must differ (E_CRITERIA_SELF).',
  'Technical cards: agents meet+verify criteria then --phase done. Never park them for owner review.',
  'needsAcceptance ONLY for visual/UI/UX/DX taste or real-world eyes — never technical correctness.',
  'Owner Now/beacon shows needsAcceptance only; bare verify is agent work.',
  'Release mid-card needs --handoff.',
];

// Ratified decisions surface the owner's ratification comment IN FULL (never
// truncated/paraphrased); open/draft ones surface the whole ballot — options
// included — since that's what the owner would need to decide from.
function decisionForBrief(d) {
  const base = { id: d.id, cardId: d.cardId, group: d.group, status: d.status, draft: !!d.draft,
    title: d.title, gist: d.gist, outcome: d.outcome ?? null, comment: d.comment ?? '', ratifiedAt: d.ratifiedAt ?? null };
  if (d.status === 'ratified') return base;
  return { ...base, lesson: d.lesson, story: d.story, explainer: d.explainer, inWild: d.inWild, detail: d.detail, rec: d.rec,
    recommendation: d.recommendation, hybrid: d.hybrid,
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
  const milestone = card.milestoneId ? s.milestones.find(m => m.id === card.milestoneId) : null;
  const explicitRefs = Array.isArray(card.refs) ? card.refs : [];
  const harvested = [...harvestRefs(card.body), ...harvestRefs(card.plan)];
  return {
    card: {
      id: card.id, num: card.num, title: card.title, body: card.body, plan: card.plan,
      phase: card.phase, priority: card.priority, workOrder: card.workOrder ?? null,
      assignee: card.assignee ?? null, track: card.track,
      epoch: card.epoch ? { id: card.epoch, name: epoch?.name ?? null, goal: epoch?.goal ?? null } : null,
      milestone: milestone ? { id: milestone.id, title: milestone.title, goal: milestone.goal, criteria: milestone.criteria } : null,
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
