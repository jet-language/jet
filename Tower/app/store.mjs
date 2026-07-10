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
// decisions can never desync. Only two lanes ever block the owner: `decide`
// and `activate`. Everything else is an agent's, inert, or done.
import { dataFile, historyFile, readJSON, writeJSON, backup, newId, today, now } from './paths.mjs';
import { withLock } from './lock.mjs';
import { loadConfig } from './config.mjs';

export const VERSION = 4;

export const PHASES = [
  { id: 'triage',   label: 'Triage',   seq: 0, who: 'owner', blurb: 'Captured — give it the go-ahead to start' },
  { id: 'deciding', label: 'Deciding', seq: 1, who: 'owner', blurb: 'Blocked on a decision' },
  { id: 'planning', label: 'Planning', seq: 2, who: 'agent', blurb: 'Build a plan + raise the decisions it needs' },
  { id: 'ready',    label: 'Ready',    seq: 3, who: 'agent', blurb: 'Plan vetted, decisions cleared — implement it' },
  { id: 'building', label: 'Building', seq: 4, who: 'agent', blurb: 'Implementation in progress' },
  { id: 'verify',   label: 'Verify',   seq: 5, who: 'agent', blurb: 'Claimed done — verify 100%, then close' },
  { id: 'done',     label: 'Done',     seq: 6, who: null,    blurb: 'Verified — hidden' },
  { id: 'frozen',   label: 'Frozen',   seq: -1, who: 'owner', blurb: 'Owner-only — untouched until activated' },
];
export const PHASE_IDS = PHASES.map(p => p.id);
export const ACTIVE = ['triage', 'deciding', 'planning', 'ready', 'building', 'verify'];

export const LANES = {
  decide:    { who: 'owner', label: 'Decide',    rank: 0 },
  activate:  { who: 'owner', label: 'Activate',  rank: 1 },
  plan:      { who: 'agent', label: 'Plan',      rank: 2 },
  implement: { who: 'agent', label: 'Implement', rank: 3 },
  building:  { who: 'agent', label: 'Building',  rank: 4 },
  verify:    { who: 'agent', label: 'Verify',    rank: 5 },
  blocked:   { who: null,    label: 'Blocked',   rank: 6 },
  frozen:    { who: 'owner', label: 'Frozen',    rank: 7 },
  done:      { who: null,    label: 'Done',      rank: 8 },
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

  const load = () => normalize(readJSON(file, empty(config.project)));

  // #461: loadHistory() is lazy + cached per store handle — read once, reuse
  // across calls in the same process, invalidated after any write that could
  // have touched history.json (mutate/restoreArchived).
  let historyCache = null;
  const loadHistory = () => (historyCache ||= loadHistoryRaw(dataDir));

  // Read-modify-write under the cross-process lock; rev bumps on every write.
  // `expectRev` (optional) enables optimistic concurrency for API callers.
  const mutate = (fn, { expectRev } = {}) => withLock(file, () => {
    const s = load();
    if (expectRev != null && Number(expectRev) !== s.meta.rev)
      fail('E_CONFLICT', `stale rev: expected ${expectRev}, store is at ${s.meta.rev} — re-read state and retry`);
    const result = fn(s, config);
    // #461: single chokepoint — every write gets a chance to retire aged-out
    // cards/decisions/events to history.json before tower.json is persisted.
    retire(s, config, dataDir);
    historyCache = null;
    s.meta.rev += 1;
    backup(file, config.backups);
    writeJSON(file, s);
    return { result, state: s };
  });

  // Replace the whole state (undo). Guarded by expectRev so an interleaved
  // write from another agent can never be silently reverted. Undo touches
  // ONLY tower.json — history.json is append-only and never rolled back
  // (see Tower/test/history.test.mjs for the duplicate-tolerance this buys).
  const restore = (prevState, { expectRev } = {}) => withLock(file, () => {
    const cur = load();
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
    const s = load();
    const h = loadHistoryRaw(dataDir);
    const result = restoreFromHistory(s, h, ref, by);
    s.meta.rev += 1;
    backup(file, config.backups);
    writeJSON(file, s);
    writeJSON(historyFile(dataDir), h);
    historyCache = null;
    return { result, state: s };
  });

  return { file, dataDir, config, load, mutate, restore, restoreArchived, loadHistory, project: () => project(load(), config) };
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
  // retire together, regardless of the decisions' own ratifiedAt.
  const retireCardIds = new Set(s.cards.filter(c => c.phase === 'done' && isOlderThanDays(c.updated, days)).map(c => c.id));
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

  // (a) standalone ratified decisions aged out on their own — only once
  // their card is done (or gone); a still-active card keeps its decisions
  // live no matter how old, so the card's own view stays whole.
  const liveCardById = new Map(s.cards.map(c => [c.id, c]));
  const standaloneIds = new Set(s.decisions.filter(d => {
    if (d.status !== 'ratified' || !isOlderThanDays(d.ratifiedAt, days)) return false;
    const c = liveCardById.get(d.cardId);
    return !c || c.phase === 'done';
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
    card.updated = today();
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
    logEvent(s, { by, action: 'archive.restore', ref: d.id, note: 'decision' });
    return { kind: 'decision', id: d.id };
  }
  fail('E_NOT_FOUND', `no archived card or decision ${ref}`);
}

export function normalize(s) {
  s = s && typeof s === 'object' ? s : empty();
  s.meta = { version: VERSION, project: 'Project', currentEpoch: null, nextNum: 1, rev: 0, ...(s.meta || {}) };
  s.meta.version = VERSION;
  s.meta.ui = { toggled: [], ...(s.meta.ui || {}) };
  for (const k of ['epochs', 'milestones', 'cards', 'decisions', 'questions', 'ideas', 'events']) s[k] ||= [];
  delete s.messages;   // messaging was removed; drop the legacy key on next write
  for (const c of s.cards) { c.blockedBy ||= []; c.log ||= []; c.criteria ||= []; c.refs ||= []; c.needsAcceptance = !!c.needsAcceptance; }
  for (const d of s.decisions) d.draft = !!d.draft;
  return s;
}

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
  if (card.phase === 'frozen') return { lane: 'frozen', who: 'owner', label: 'Frozen — activate to work it' };
  const open = decisions.filter(d => d.cardId === card.id && isBlocking(d));
  if (open.length) return { lane: 'decide', who: 'owner', label: `${open.length} decision${open.length > 1 ? 's' : ''} to make`, decisions: open.map(d => d.id) };
  if (card.phase === 'triage') return { lane: 'activate', who: 'owner', label: 'Greenlight to start' };
  const blockers = (card.blockedBy || []).filter(id => {
    const b = cards.find(c => c.id === id);
    if (b) return b.phase !== 'done';
    const d = decisions.find(x => x.id === id);
    if (d) return d.status !== 'ratified';
    return false; // dangling ref — don't block on it
  });
  if (blockers.length) return { lane: 'blocked', who: null, label: `Blocked by ${blockers.join(', ')}`, blockers };
  if (card.phase === 'deciding') return card.plan
    ? { lane: 'implement', who: 'agent', label: 'Ready to implement' }
    : { lane: 'plan', who: 'agent', label: 'Build a plan + raise decisions' };
  if (card.phase === 'planning') return { lane: 'plan', who: 'agent', label: card.plan ? 'Vet the plan + raise decisions' : 'Build a plan + raise decisions' };
  if (card.phase === 'ready')    return { lane: 'implement', who: 'agent', label: 'Ready to implement' };
  if (card.phase === 'building') return { lane: 'building', who: 'agent', label: 'Continue building' };
  if (card.phase === 'verify')   return { lane: 'verify', who: 'agent', label: 'Verify 100%, then close' };
  return { lane: 'blocked', who: null, label: '' };
}

export function milestoneProgress(m, cards) {
  const linked = cards.filter(c => c.milestoneId === m.id);
  const done = linked.filter(c => c.phase === 'done').length;
  return { total: linked.length, done, met: m.status === 'met' };
}

export function project(s, config = null) {
  const cards = s.cards.map(c => {
    const clearance = clearanceOf(c, s.decisions);
    const decisions = s.decisions.filter(d => d.cardId === c.id);
    const questions = s.questions.filter(q => q.cardId === c.id);
    const openQ = questions.filter(q => q.status === 'open').length;
    return { ...c, clearance, decisions, questions, openQ, lane: laneOf(c, s.decisions, s.cards) };
  });
  const milestones = s.milestones.map(m => ({ ...m, progress: milestoneProgress(m, s.cards) }));
  const inLane = (l) => cards.filter(c => c.lane.lane === l);
  const openDecisions = s.decisions.filter(isBlocking);
  // #461 walk-back buffer: every ratified decision still on the live board —
  // i.e. not yet retired to history — surfaces here so the owner can reopen
  // it in one tap while it's fresh.
  const recentlyDecided = s.decisions.filter(d => d.status === 'ratified' && !d.draft)
    .map(d => ({ id: d.id, title: d.title, outcome: d.outcome, comment: d.comment || '', ratifiedAt: d.ratifiedAt, cardId: d.cardId }))
    .sort((a, b) => (b.ratifiedAt || '').localeCompare(a.ratifiedAt || ''));
  const counts = {
    byPhase: Object.fromEntries(PHASE_IDS.map(p => [p, cards.filter(c => c.phase === p).length])),
    forYou: openDecisions.length + inLane('activate').length,
    decide: openDecisions.length,
    activate: inLane('activate').length,
    agentReady: inLane('plan').length + inLane('implement').length + inLane('building').length + inLane('verify').length,
    sidequests: cards.filter(c => c.track === 'sidequest' && ACTIVE.includes(c.phase)).length,
    frozen: cards.filter(c => c.phase === 'frozen').length,
    ideas: s.ideas.filter(b => b.status !== 'tagged').length,
    openQuestions: s.questions.filter(q => q.status === 'open').length,
  };
  return { meta: s.meta, config: config || undefined, epochs: s.epochs, milestones, phases: PHASES, lanes: LANES,
    cards, decisions: s.decisions, questions: s.questions, ideas: s.ideas,
    events: s.events.slice(0, 300), counts, recentlyDecided };
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

export function addCard(s, p, config) {
  if (!p.title || !String(p.title).trim()) fail('E_INVALID', 'card needs a title');
  checkEnum(p.kind, config.kinds, 'kind');
  checkEnum(p.track, config.tracks, 'track');
  checkEnum(p.priority, config.priorities, 'priority');
  checkEnum(p.phase, PHASE_IDS, 'phase');
  checkEpoch(s, p.epoch); checkMilestone(s, p.milestoneId);
  checkRefs(p.refs);
  const card = {
    id: p.id || newId('c'),
    num: p.num || s.meta.nextNum++,
    title: String(p.title).trim(),
    body: p.body || '',
    kind: p.kind || config.kinds[0],
    track: p.track || config.tracks[0],
    epoch: p.epoch ?? s.meta.currentEpoch ?? null,
    milestoneId: p.milestoneId || null,
    phase: p.phase || 'triage',
    priority: p.priority || config.priorities[2] || config.priorities.at(-1),
    plan: p.plan || null,
    blockedBy: p.blockedBy || [],
    workOrder: p.workOrder != null ? Number(p.workOrder) : undefined,
    assignee: p.assignee || null,
    log: p.log || [],
    criteria: Array.isArray(p.criteria) ? p.criteria.map((it, i) => normalizeCriterion(it, i)) : [],
    refs: Array.isArray(p.refs) ? p.refs : [],
    needsAcceptance: !!p.needsAcceptance,
    created: now(), updated: today(),
  };
  s.cards.push(card);
  logEvent(s, { by: p.by, action: 'card.add', ref: card.id, note: card.title });
  return card;
}

const CARD_FIELDS = ['title', 'body', 'kind', 'track', 'epoch', 'milestoneId', 'phase', 'priority', 'plan', 'blockedBy', 'workOrder', 'assignee', 'criteria', 'needsAcceptance', 'refs'];

// D-TWR-CRIT1=C / D-TWRGUARD1=C: gate --phase done. Agent-hard, owner-soft —
// a write with by !== 'owner' is refused while any criterion is unverified;
// by === 'owner' always passes (bypass logged). Once the gate clears, a card
// flagged needsAcceptance mints an owner accept/bounce ballot instead of
// closing outright (again: owner writes go straight to done). Returns a
// phase override ('verify') when acceptance was minted, else null.
function applyDoneGate(s, c, targetPhase, by) {
  if (targetPhase !== 'done') return null;
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
  if (c.needsAcceptance && by !== 'owner') {
    mintAcceptance(s, c);
    return 'verify';
  }
  return null;
}

function mintAcceptance(s, c) {
  const id = `D-ACCEPT-${c.num}`;
  const existing = s.decisions.find(d => d.id === id);
  if (existing && existing.status !== 'ratified') return; // already awaiting owner — no duplicate mint
  const items = c.criteria || [];
  const evidence = items.length
    ? items.map(i => `${i.n}. ${i.text} — ${i.status}${i.evidence ? ` (${i.evidence})` : ''}${i.verifiedBy ? ` [verified by ${i.verifiedBy}]` : ''}`).join('\n')
    : '(no exit criteria on this card — direct acceptance request)';
  if (existing) {
    // a prior round was bounced; re-open the same ballot id for round 2
    existing.status = 'open';
    existing.detail = evidence;
    delete existing.outcome; delete existing.comment; delete existing.ratifiedAt;
  } else {
    addDecision(s, {
      id, cardId: c.id, group: 'acceptance',
      title: `Accept #${c.num} — ${c.title}`,
      gist: `Close #${c.num}, or bounce it back to building.`,
      detail: evidence,
      options: [
        { key: 'accept', name: 'Accept — close the card' },
        { key: 'bounce', name: 'Bounce — back to building (comment why)' },
      ],
      by: 'agent',
    });
  }
  c.log.unshift({ at: today(), text: `Requested acceptance — minted ${id}.` });
}

// D-TWRGUARD1=C (#458): frozen cards are owner-only for any write; triage
// cards are owner-only for a phase change (greenlight is `activate`'s job) —
// except the phase-'done' exit, which the done-gate above already governs.
// Body/plan/log edits on a triage card stay open to agents (normal prep).
// Agent-hard, owner-soft: by === 'owner' bypasses both checks outright.
function assertOwnerLane(c, patch, by) {
  if (by === 'owner') return;
  if (c.phase === 'frozen')
    fail('E_OWNER_LANE', `card #${c.num} is frozen — owner-only until it's activated (\`tower card activate\`)`);
  if (c.phase === 'triage' && 'phase' in patch && patch.phase !== 'done')
    fail('E_OWNER_LANE', `card #${c.num} is in triage — only the owner greenlights it out of triage (\`tower card activate\`); body/plan/log edits are fine`);
}

export function updateCard(s, ref, patch, config) {
  const c = mustCard(s, ref);
  // Basic shape validation runs before the owner-lane authorization check, so
  // a malformed request always reports E_INVALID/E_NOT_FOUND regardless of
  // who sent it or what lane the card is in.
  checkEnum(patch.kind, config.kinds, 'kind');
  checkEnum(patch.track, config.tracks, 'track');
  checkEnum(patch.priority, config.priorities, 'priority');
  checkEnum(patch.phase, PHASE_IDS, 'phase');
  if ('epoch' in patch) checkEpoch(s, patch.epoch);
  if ('milestoneId' in patch) checkMilestone(s, patch.milestoneId);
  // blockedBy accepts a card ref OR a decision id (D-TWRGUARD1=C #458).
  if ('blockedBy' in patch) for (const id of patch.blockedBy || []) {
    if (!findCard(s, id) && !s.decisions.find(d => d.id === id))
      fail('E_NOT_FOUND', `blockedBy: no card or decision ${id}`);
  }
  if ('refs' in patch) checkRefs(patch.refs);
  assertOwnerLane(c, patch, patch.by);
  const phaseOverride = 'phase' in patch ? applyDoneGate(s, c, patch.phase, patch.by) : null;
  for (const k of CARD_FIELDS) {
    if (k in patch) {
      if (k === 'phase') c.phase = phaseOverride || patch.phase;
      else if (k === 'workOrder') c[k] = patch[k] == null || patch[k] === '' ? undefined : Number(patch[k]);
      else if (k === 'needsAcceptance') c.needsAcceptance = patch.needsAcceptance === true || patch.needsAcceptance === 'true';
      else if (k === 'criteria') c.criteria = Array.isArray(patch.criteria) ? patch.criteria.map((it, i) => normalizeCriterion(it, i)) : c.criteria;
      else c[k] = patch[k];
    }
  }
  if (patch.logEntry) c.log.unshift({ at: today(), by: patch.by || 'agent', text: patch.logEntry });
  c.updated = today();
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
  c.updated = today();
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
  c.updated = today();
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
  c.updated = today();
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
  const ratified = s.decisions.filter(d => d.cardId === c.id && d.status === 'ratified');
  if (ratified.length)
    fail('E_HAS_RATIFIED', `card #${c.num} has ${ratified.length} ratified decision${ratified.length > 1 ? 's' : ''} (${ratified.map(d => d.id).join(', ')}) — they retire to \`tower archive\` on their own once the buffer window passes; delete once none are live on the card`);
  s.cards = s.cards.filter(x => x.id !== c.id);
  s.decisions = s.decisions.filter(d => d.cardId !== c.id);
  s.questions = s.questions.filter(q => q.cardId !== c.id);
  for (const x of s.cards) x.blockedBy = (x.blockedBy || []).filter(id => id !== c.id);
  logEvent(s, { by: p.by, action: 'card.delete', ref: c.id, note: c.title });
  return { ok: true, id: c.id };
}

// Owner-only gate shared by activate/ratify (D-TWRGUARD1=C #458). An agent
// may act "on behalf of" the owner by quoting his words verbatim — recorded
// in the event log note — otherwise refused.
function assertOwnerOr(by, quote, code, what) {
  if (by === 'owner') return null;
  if (!quote || !String(quote).trim()) fail(code, `${what} is owner-only — pass --quote "owner's words" if this is on his behalf`);
  return `by ${by}, quoting owner: "${quote}"`;
}

// Activate a triaged/frozen card into a working track. Owner-only (greenlight).
export function activate(s, ref, { track, epoch, milestoneId, phase, workOrder, by, quote } = {}, config) {
  const c = mustCard(s, ref);
  const quoteNote = assertOwnerOr(by, quote, 'E_OWNER_ONLY', 'activate');
  checkEnum(track, config.tracks, 'track');
  checkEnum(phase, PHASE_IDS, 'phase');
  if (epoch !== undefined) checkEpoch(s, epoch);
  if (milestoneId !== undefined) checkMilestone(s, milestoneId);
  if (track) c.track = track;
  if (epoch !== undefined) c.epoch = epoch;
  if (milestoneId !== undefined) c.milestoneId = milestoneId;
  if (workOrder != null) c.workOrder = Number(workOrder);
  const hasOpen = s.decisions.some(d => d.cardId === c.id && isBlocking(d));
  const requestedPhase = phase || (hasOpen ? 'deciding' : 'planning');
  c.phase = applyDoneGate(s, c, requestedPhase, by) || requestedPhase;
  c.updated = today();
  c.log.unshift({ at: today(), by: by || 'owner', text: `Activated into ${c.track === 'epoch' ? 'epoch ' + (c.epoch || '?') : c.track} track${quoteNote ? ` (${quoteNote})` : ''}` });
  logEvent(s, { by, action: 'card.activate', ref: c.id, note: quoteNote || '' });
  return c;
}

// Claim/release: soft assignment so parallel agents don't double-work a card.
export function claimCard(s, ref, by) {
  const c = mustCard(s, ref);
  if (!by) fail('E_INVALID', 'claim needs --by <agent>');
  if (by !== 'owner' && c.phase === 'frozen')
    fail('E_OWNER_LANE', `card #${c.num} is frozen — owner-only until it's activated (\`tower card activate\`)`);
  if (c.assignee && c.assignee !== by) fail('E_CLAIMED', `card #${c.num} already claimed by ${c.assignee} — pick another or release it first`);
  c.assignee = by; c.claimedAt = now(); c.updated = today();
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
  c.assignee = null; delete c.claimedAt; c.updated = today();
  logEvent(s, { by, action: 'card.release', ref: c.id, note: handoff ? '[handoff] logged' : '' });
  return c;
}

// ---- mutations: decisions --------------------------------------------------

// D-TWRGUARD1=C (#458): the ballot-ready standard (tower-ballot skill) —
// gist/story/inWild/options[].code/rec — enforced at write time. Acceptance
// ballots (`mintAcceptance` above) are a fixed system-generated evidence
// format, not a narrative ballot, and are exempt.
export function ballotGaps(p) {
  const missing = [];
  if (!p.gist || !String(p.gist).trim()) missing.push('gist');
  if (!p.story || !String(p.story).trim()) missing.push('story');
  if (!p.inWild || !String(p.inWild).trim()) missing.push('inWild');
  const opts = Array.isArray(p.options) ? p.options : [];
  if (opts.length < 2) missing.push('options (need at least 2)');
  else {
    const noCode = opts.filter(o => !o || !o.code || !String(o.code).trim());
    if (noCode.length) missing.push(`options[].code (missing on ${noCode.map((o, i) => (o && o.key) || `#${i + 1}`).join(', ')})`);
  }
  if (!p.rec || !String(p.rec).trim()) missing.push('rec');
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
    title: String(p.title).trim(), gist: p.gist || '', explainer: p.explainer || '', story: p.story || '',
    inWild: p.inWild || '', detail: p.detail || '', options: p.options || [], comparisons: p.comparisons || [],
    rec: p.rec || null, draft, status: 'open', created: now() };
  s.decisions.push(d);
  logEvent(s, { by: p.by, action: 'decision.add', ref: d.id, note: draft ? `${d.title} (draft)` : d.title });
  return d;
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
  if (!outcome) fail('E_INVALID', 'ratify needs an outcome (option key)');
  if (Array.isArray(d.options) && d.options.length && !d.options.some(o => o.key === outcome))
    fail('E_INVALID', `outcome "${outcome}" is not one of this decision's option keys: ${d.options.map(o => o.key).join(', ')}`);
  const quoteNote = assertOwnerOr(by, quote, 'E_OWNER_ONLY', 'ratify');
  d.status = 'ratified'; d.outcome = outcome;
  if (comment != null) d.comment = comment;
  d.ratifiedAt = today();
  const c = s.cards.find(x => x.id === d.cardId);
  // Acceptance ballots (D-ACCEPT-<num>, minted by the done-gate) resolve the
  // card directly: accept closes it, bounce sends it back to building.
  if (d.id.startsWith('D-ACCEPT-') && c) {
    if (outcome === 'accept') {
      c.phase = 'done';
      c.log.unshift({ at: today(), by: by || 'owner', text: `Accepted — ${d.id} ratified accept.` });
    } else if (outcome === 'bounce') {
      c.phase = 'building';
      c.log.unshift({ at: today(), by: by || 'owner', text: `Bounced back to building: ${comment || '(no comment)'}` });
    }
    c.updated = today();
  }
  if (d.group === 'syntax') appendSyntaxChores(s, c, by);
  advanceClearedCard(s, d.cardId);
  logEvent(s, { by: by || 'owner', action: 'decision.ratify', ref: d.id, note: quoteNote ? `${outcome} (${quoteNote})` : outcome });
  return d;
}

export function reopenDecision(s, decisionId, by) {
  const d = s.decisions.find(x => x.id === decisionId) || fail('E_NOT_FOUND', `no decision ${decisionId}`);
  d.status = 'open'; delete d.outcome; delete d.ratifiedAt;
  logEvent(s, { by: by || 'owner', action: 'decision.reopen', ref: d.id });
  return d;
}

export function updateDecision(s, id, patch, by) {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  for (const k of ['title', 'gist', 'explainer', 'story', 'inWild', 'detail', 'options', 'comparisons', 'rec', 'group'])
    if (k in patch) d[k] = patch[k];
  // --ready clears draft, but only once the ballot standard is actually met.
  if (patch.ready) {
    if (d.group !== 'acceptance') {
      const gaps = ballotGaps(d);
      if (gaps.length) fail('E_BALLOT', `ballot not ready — missing: ${gaps.join(', ')}`);
    }
    d.draft = false;
  }
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
    gist: '', explainer: '', story: '', inWild: '', detail: '', options: [], comparisons: [],
    rec: null, draft: false, status: 'ratified', outcome, comment: outcome,
    created: now(), ratifiedAt: today() };
  s.decisions.push(d);
  c.log.unshift({ at: today(), by, text: `Verdict recorded (${id}): ${outcome}` });
  c.updated = today();
  logEvent(s, { by, action: 'decision.verdict', ref: id, note: outcome });
  return { ...d, cardNum: c.num };
}

export function deleteDecision(s, id, by) {
  const d = s.decisions.find(x => x.id === id) || fail('E_NOT_FOUND', `no decision ${id}`);
  s.decisions = s.decisions.filter(x => x.id !== id);
  logEvent(s, { by, action: 'decision.delete', ref: id, note: d.title });
  return { ok: true, id };
}

function advanceClearedCard(s, cardId) {
  const c = s.cards.find(x => x.id === cardId);
  if (!c || c.phase !== 'deciding') return;
  const stillOpen = s.decisions.some(d => d.cardId === cardId && isBlocking(d));
  if (stillOpen) return;
  c.phase = c.plan ? 'ready' : 'planning';
  c.updated = today();
  c.log.unshift({ at: today(), text: 'All decisions ratified; advanced out of deciding.' });
}

// ---- mutations: questions --------------------------------------------------

export function addQuestion(s, p) {
  const card = mustCard(s, p.cardId);
  if (!p.text || !String(p.text).trim()) fail('E_INVALID', 'question needs text');
  const q = { id: newId('q'), cardId: card.id, decisionId: p.decisionId || null,
    by: p.by || 'owner', kind: p.kind || 'question',
    text: String(p.text).trim(), status: 'open', answer: '', created: now() };
  s.questions.push(q);
  logEvent(s, { by: p.by || 'owner', action: 'question.add', ref: q.id });
  return q;
}
export function answerQuestion(s, id, answer, by) {
  const q = s.questions.find(x => x.id === id) || fail('E_NOT_FOUND', `no question ${id}`);
  if (!answer || !String(answer).trim()) fail('E_INVALID', 'answer needs text');
  q.answer = answer; q.status = 'answered'; q.answeredAt = today(); q.answeredBy = by || 'agent';
  logEvent(s, { by: by || 'agent', action: 'question.answer', ref: q.id });
  return q;
}
export function deleteQuestion(s, id, by) {
  s.questions = s.questions.filter(q => q.id !== id);
  logEvent(s, { by, action: 'question.delete', ref: id });
  return { ok: true, id };
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
    phase: 'triage',
    priority: extra.priority || config.priorities.at(-1),
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
  for (const k of ['name', 'goal', 'status']) if (k in patch) e[k] = patch[k];
  return e;
}
export function setCurrentEpoch(s, id) {
  if (id != null) checkEpoch(s, id);
  s.meta.currentEpoch = id;
  return s.meta;
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
  if ('epochId' in patch) checkEpoch(s, patch.epochId);
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

const LANE_PREF = { building: 0, verify: 1, implement: 2, plan: 3 };

// #457 — `scope: 'burndown'` narrows the pool to exactly the current
// epoch's epoch-track cards plus all sidequests (agent lanes only) — the
// tower skill's "burndown loop" scope, made a real filter instead of
// something an agent has to hand-derive from meta.currentEpoch each time.
export function nextCards(s, { epoch, track, agent, limit = 5, scope } = {}) {
  const proj = project(s);
  const pool = proj.cards.filter(c => {
    if (!(c.lane.lane in LANE_PREF)) return false;
    if (epoch && c.epoch !== epoch) return false;
    if (track && c.track !== track) return false;
    if (c.assignee && agent && c.assignee !== agent) return false;
    if (scope === 'burndown') {
      const inEpoch = c.track === 'epoch' && c.epoch === s.meta.currentEpoch;
      if (!inEpoch && c.track !== 'sidequest') return false;
    }
    return true;
  });
  pool.sort((a, b) =>
    (a.workOrder ?? Infinity) - (b.workOrder ?? Infinity)
    || LANE_PREF[a.lane.lane] - LANE_PREF[b.lane.lane]
    || (a.priority || '').localeCompare(b.priority || '')
    || a.num - b.num);
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
  'verify → done only after real verification; needsAcceptance cards wait for owner ballot.',
  'Release mid-card needs --handoff.',
];

// Ratified decisions surface the owner's ratification comment IN FULL (never
// truncated/paraphrased); open/draft ones surface the whole ballot — options
// included — since that's what the owner would need to decide from.
function decisionForBrief(d) {
  const base = { id: d.id, cardId: d.cardId, group: d.group, status: d.status, draft: !!d.draft,
    title: d.title, gist: d.gist, outcome: d.outcome ?? null, comment: d.comment ?? '', ratifiedAt: d.ratifiedAt ?? null };
  if (d.status === 'ratified') return base;
  return { ...base, story: d.story, explainer: d.explainer, inWild: d.inWild, detail: d.detail, rec: d.rec,
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
    questions: s.questions.filter(q => q.cardId === card.id && q.status === 'open').map(q => ({ id: q.id, by: q.by, text: q.text })),
    refs: [...new Set([...explicitRefs, ...harvested])],
    log: (card.log || []).slice(0, 5),
    rules: BRIEF_RULES,
  };
}

// Digest cursor: everything in events[] after this instant is "since you
// were away". The owner's "Caught up" button advances it.
export function setDigestCursor(s, at) {
  s.meta.digestCursor = at || now();
  return { digestCursor: s.meta.digestCursor };
}

// ---- ui state ---------------------------------------------------------------

export function toggleOpen(s, key) {
  const set = new Set(s.meta.ui.toggled || []);
  set.has(key) ? set.delete(key) : set.add(key);
  s.meta.ui.toggled = [...set];
  return s.meta.ui.toggled;
}
