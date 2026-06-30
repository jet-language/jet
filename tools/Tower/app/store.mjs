// Tower store: one JSON file, one source of truth.
//
// Every card computes to exactly ONE lane that says who owns the next move and
// what it is. Only two lanes ever block the owner: `decide` and `activate`.
// Everything else is an agent's to pick up, inert, or done. Decision state is
// derived from the decisions on every read — the card and its decisions are the
// same data, so they can't drift (the v1 desync bug, fixed structurally).
import { DATA, readJSON, writeJSON, newId, today, now } from './paths.mjs';

// Stages in honest order. `who` = who owns a card sitting in this stage with no
// other signal. `frozen` is off the line (owner-only); `done` is hidden.
export const PHASES = [
  { id: 'triage',   label: 'Triage',   seq: 0, who: 'owner', blurb: 'Captured — give it the go-ahead to start' },
  { id: 'deciding', label: 'Deciding', seq: 1, who: 'owner', blurb: 'Blocked on a decision' },
  { id: 'planning', label: 'Planning', seq: 2, who: 'agent', blurb: 'Build a plan + raise the decisions it needs' },
  { id: 'ready',    label: 'Ready',    seq: 3, who: 'agent', blurb: 'Plan vetted, decisions cleared — implement it' },
  { id: 'building', label: 'Building', seq: 4, who: 'agent', blurb: 'Implementation in progress' },
  { id: 'verify',   label: 'Verify',   seq: 5, who: 'agent', blurb: 'Claimed done — verify 100%, then close' },
  { id: 'done',     label: 'Done',     seq: 6, who: null,    blurb: 'Verified — hidden' },
  { id: 'frozen',   label: 'Frozen',   seq: -1, who: 'owner', blurb: 'Owner-only — untouched until you activate it' },
];
export const PHASE_IDS = PHASES.map(p => p.id);
const PHASE = Object.fromEntries(PHASES.map(p => [p.id, p]));
export const ACTIVE = ['triage', 'deciding', 'planning', 'ready', 'building', 'verify'];

export const PRIORITIES = ['P0', 'P1', 'P2', 'P3'];
export const KINDS = ['task', 'feature', 'idea', 'bug'];
export const TRACKS = ['epoch', 'sidequest'];

// Lane metadata for the client (labels, who owns it, ordering).
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

const empty = () => ({
  meta: { version: 3, currentEpoch: null, nextNum: 1, ui: { toggled: [] } },
  epochs: [], cards: [], decisions: [], binder: [], questions: [],
});

export function load() {
  const s = readJSON(DATA, empty());
  s.meta = { version: 3, currentEpoch: null, nextNum: 1, ...(s.meta || {}) };
  s.meta.ui = { toggled: [], ...(s.meta.ui || {}) };
  if (s.meta.ui.open && !s.meta.ui.toggled.length) { s.meta.ui.toggled = s.meta.ui.open; delete s.meta.ui.open; }
  s.epochs ||= []; s.cards ||= []; s.decisions ||= []; s.binder ||= []; s.questions ||= [];
  return s;
}
export const save = (s) => writeJSON(DATA, s);

// ---- derivation: clearance + lane (the one place this is decided) --------

export function clearanceOf(card, decisions) {
  const linked = decisions.filter(d => d.cardId === card.id);
  if (!linked.length) return { state: 'none', open: [], total: 0, ratified: 0 };
  const open = linked.filter(d => d.status !== 'ratified');
  return { state: open.length ? 'pending' : 'cleared', open: open.map(d => d.id), total: linked.length, ratified: linked.length - open.length };
}

// The single source of "what happens next and who owns it".
export function laneOf(card, decisions, cards) {
  if (card.phase === 'done')   return { lane: 'done', who: null, label: 'Done' };
  if (card.phase === 'frozen') return { lane: 'frozen', who: 'owner', label: 'Frozen — activate to work it' };
  const open = decisions.filter(d => d.cardId === card.id && d.status !== 'ratified');
  if (open.length) return { lane: 'decide', who: 'owner', label: `${open.length} decision${open.length > 1 ? 's' : ''} to make`, decisions: open.map(d => d.id) };
  if (card.phase === 'triage') return { lane: 'activate', who: 'owner', label: 'Greenlight to start' };
  const blockers = (card.blockedBy || []).filter(id => { const b = cards.find(c => c.id === id); return b && b.phase !== 'done'; });
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

export function project(s) {
  const cards = s.cards.map(c => {
    const clearance = clearanceOf(c, s.decisions);
    const decisions = s.decisions.filter(d => d.cardId === c.id);
    const questions = s.questions.filter(q => q.cardId === c.id);
    const openQ = questions.filter(q => q.status === 'open').length;
    return { ...c, clearance, decisions, questions, openQ, lane: laneOf(c, s.decisions, s.cards) };
  });
  const inLane = (l) => cards.filter(c => c.lane.lane === l);
  const counts = {
    byPhase: Object.fromEntries(PHASE_IDS.map(p => [p, cards.filter(c => c.phase === p).length])),
    forYou: inLane('decide').length + inLane('activate').length,
    decide: inLane('decide').length,
    activate: inLane('activate').length,
    agentReady: inLane('plan').length + inLane('implement').length + inLane('building').length + inLane('verify').length,
    sidequests: cards.filter(c => c.track === 'sidequest' && ACTIVE.includes(c.phase)).length,
    frozen: cards.filter(c => c.phase === 'frozen').length,
    binder: s.binder.filter(b => b.status !== 'tagged').length,
    openQuestions: s.questions.filter(q => q.status === 'open').length,
  };
  return { meta: s.meta, epochs: s.epochs, phases: PHASES, lanes: LANES, cards, decisions: s.decisions, binder: s.binder, questions: s.questions, counts };
}

// ---- mutations -----------------------------------------------------------

export function addCard(s, p) {
  const id = p.id || newId('c');
  if (!s.meta.nextNum) s.meta.nextNum = 1;
  const num = p.num || s.meta.nextNum++;
  const card = {
    id, num,
    title: p.title || 'Untitled card',
    body: p.body || '',
    kind: KINDS.includes(p.kind) ? p.kind : 'task',
    track: TRACKS.includes(p.track) ? p.track : 'epoch',
    epoch: p.epoch || s.meta.currentEpoch || null,
    phase: PHASE[p.phase] ? p.phase : 'triage',
    priority: PRIORITIES.includes(p.priority) ? p.priority : 'P2',
    plan: p.plan || null,
    blockedBy: p.blockedBy || [],
    log: p.log || [],
    created: now(), updated: today(),
  };
  s.cards.push(card);
  return card;
}

export function updateCard(s, id, patch) {
  const c = s.cards.find(x => x.id === id);
  if (!c) return null;
  for (const k of ['title', 'body', 'kind', 'track', 'epoch', 'phase', 'priority', 'plan', 'blockedBy', 'workOrder']) {
    if (k in patch) {
      if (k === 'workOrder') c[k] = patch[k] == null || patch[k] === '' ? undefined : Number(patch[k]);
      else c[k] = patch[k];
    }
  }
  if (patch.logEntry) c.log.unshift({ at: today(), text: patch.logEntry });
  c.updated = today();
  return c;
}
export function deleteCard(s, id) {
  s.cards = s.cards.filter(c => c.id !== id);
  s.decisions = s.decisions.filter(d => d.cardId !== id);
  s.questions = s.questions.filter(q => q.cardId !== id);
}

// Activate a triaged/frozen card into a working track.
export function activate(s, id, { track, epoch, phase } = {}) {
  const c = s.cards.find(x => x.id === id);
  if (!c) return null;
  if (track) c.track = track;
  if (epoch !== undefined) c.epoch = epoch;
  // land in deciding if it carries open decisions, else planning — unless told
  const hasOpen = s.decisions.some(d => d.cardId === id && d.status !== 'ratified');
  c.phase = phase || (hasOpen ? 'deciding' : 'planning');
  c.updated = today();
  c.log.unshift({ at: today(), text: `Activated into ${c.track === 'epoch' ? 'epoch ' + (c.epoch || '?') : 'sidequest'} track` });
  return c;
}

export function clear(s, decisionId, outcome, comment) {
  const d = s.decisions.find(x => x.id === decisionId);
  if (!d) return null;
  d.status = 'ratified'; d.outcome = outcome;
  if (comment != null) d.comment = comment;
  d.ratifiedAt = today();
  advanceClearedCard(s, d.cardId);
  return d;
}
export function reopenDecision(s, decisionId) {
  const d = s.decisions.find(x => x.id === decisionId);
  if (!d) return null;
  d.status = 'open'; delete d.outcome; delete d.ratifiedAt;
  return d;
}
export function addDecision(s, p) {
  const d = { id: p.id || newId('D-'), cardId: p.cardId, title: p.title || 'Untitled decision',
    gist: p.gist || '', explainer: p.explainer || '', story: p.story || '', inWild: p.inWild || '',
    detail: p.detail || '', options: p.options || [], comparisons: p.comparisons || [], rec: p.rec || null, status: 'open' };
  s.decisions.push(d);
  return d;
}

function advanceClearedCard(s, cardId) {
  const c = s.cards.find(x => x.id === cardId);
  if (!c || c.phase !== 'deciding') return;
  const stillOpen = s.decisions.some(d => d.cardId === cardId && d.status !== 'ratified');
  if (stillOpen) return;
  c.phase = c.plan ? 'ready' : 'planning';
  c.updated = today();
  c.log.unshift({ at: today(), text: 'All decisions ratified; advanced out of deciding.' });
}

// Owner leaves a note/question on a card; an agent answers it.
export function addQuestion(s, p) {
  const q = { id: newId('q'), cardId: p.cardId, decisionId: p.decisionId || null,
    by: p.by || 'owner', kind: p.kind || 'question',
    text: p.text || '', status: 'open', answer: '', created: now() };
  s.questions.push(q);
  return q;
}
export function answerQuestion(s, id, answer) {
  const q = s.questions.find(x => x.id === id); if (!q) return null;
  q.answer = answer; q.status = 'answered'; q.answeredAt = today(); return q;
}
export function deleteQuestion(s, id) { s.questions = s.questions.filter(q => q.id !== id); }

export function addBinder(s, p) {
  const b = { id: newId('b'), text: p.text || '', note: p.note || '', level: p.level ?? null, tags: p.tags || [], status: 'open', created: now() };
  s.binder.push(b); return b;
}
export function updateBinder(s, id, patch) { const b = s.binder.find(x => x.id === id); if (b) Object.assign(b, patch); return b; }
export function deleteBinder(s, id) { s.binder = s.binder.filter(b => b.id !== id); }
export function promote(s, binderId, extra = {}) {
  const b = s.binder.find(x => x.id === binderId); if (!b) return null;
  const card = addCard(s, { title: extra.title || b.text.split(':')[0].slice(0, 80),
    body: extra.body || (b.note ? `${b.text}\n\n${b.note}` : b.text),
    kind: extra.kind || 'idea', track: extra.track || 'sidequest', phase: 'triage', priority: extra.priority || 'P3' });
  card.log.unshift({ at: today(), text: 'Added from Ideas' });
  b.status = 'tagged'; b.cardId = card.id;
  return card;
}

// Durable UI: which collapsible groups have been FLIPPED from their default
// state. The client knows each group's default (open or closed); a key in this
// set means the owner toggled it the other way. Lets some groups default-open
// (epochs, sidequests) and others default-closed (frozen, done).
export function toggleOpen(s, key) {
  const set = new Set(s.meta.ui.toggled || []);
  set.has(key) ? set.delete(key) : set.add(key);
  s.meta.ui.toggled = [...set];
  return s.meta.ui.toggled;
}
