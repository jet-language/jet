const DEFAULT_PRIORITIES = ['P0', 'P1', 'P2', 'P3'];
const AGENT_LANE_RANK = { verify: 0, building: 1, implement: 2, plan: 3 };

// Default board sort: review, building, ready, plan, then blocked.
export const workflowRank = (card) => {
  const lane = card.lane?.lane;
  if (lane === 'building' || lane === 'verify') return 0;
  if (lane === 'implement') return 1;
  if (lane === 'plan') return 2;
  if (lane === 'blocked' || lane === 'decide') return 3;
  if (lane === 'done') return 4;
  return 5;
};

// Apply at rendering boundaries too, including an already-open card refreshed
// after a ballot changes from ready to draft.
export const ownerDecisions = (decisions = []) => decisions.filter(d => !d.draft);

// Owner Now/beacon review queue: ONLY needsAcceptance cards the owner can act
// on. Bare phase=verify is a legacy agent state, not an owner duty, and so is
// a visual card still waiting on agent criteria or on its screen capture:
// every card in the owner's lane offers both Accept and Bounce.
export const openAcceptanceBallot = (card) =>
  ownerDecisions(card.decisions).find(d => d.id === `D-ACCEPT-${card.num}` && d.status !== 'ratified') || null;

export const evidenceMedia = (card, ballot) => ballot ? (ballot.visualMedia || []) : (card.visualMedia || []);

export function acceptanceReadiness(card, ballot) {
  const hasOpenCriteria = (card.criteria || []).some(i => !['met', 'verified'].includes(i.status));
  const waitingOnAgent = !!card.needsAcceptance && (!ballot || hasOpenCriteria);
  const capturePending = !!card.needsAcceptance && evidenceMedia(card, ballot).length === 0;
  return { waitingOnAgent, capturePending, ready: !waitingOnAgent && !capturePending };
}

export function ownerVerifyQueue(cards) {
  return (cards || [])
    .filter(c => c.phase === 'verify' && !!c.needsAcceptance)
    .map(c => ({ card: c, ballot: openAcceptanceBallot(c) }))
    .filter(({ card, ballot }) => acceptanceReadiness(card, ballot).ready);
}

export function cardNumberQuery(text) {
  const parts = String(text || '').trim().split(/[\s,]+/).filter(Boolean);
  if (!parts.length) return null;
  const nums = [];
  for (const part of parts) {
    const m = part.match(/^#?(\d+)$/);
    if (!m) return null;
    nums.push(Number(m[1]));
  }
  return nums;
}

// Text references as links, over HTML that is already escaped: `#N` and
// `#N cM` become buttons that open that card (and criterion); with a commit
// URL configured (`{sha}` is replaced), a commit SHA links to that commit.
// Tags, and text inside <code>, <pre>, <a> and <button>, are left alone.
const REF_RE = /(?<![\w&/#])#(\d+)(?: c(\d+))?\b|(?<![\w/.-])((?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,40})\b/g;
const attr = (s) => String(s).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
export function refLinks(html, { commitUrl = '' } = {}) {
  let inside = 0;
  return String(html ?? '').split(/(<[^>]*>)/).map((part) => {
    if (part.startsWith('<')) {
      const tag = /^<(\/?)(?:code|pre|a|button)\b/i.exec(part);
      if (tag) inside = Math.max(0, inside + (tag[1] ? -1 : 1));
      return part;
    }
    if (inside) return part;
    return part.replace(REF_RE, (all, num, crit, sha) => {
      if (num) return `<button class="linkbtn" data-card-num="${num}"${crit ? ` data-crit="${crit}"` : ''}>${all}</button>`;
      if (!commitUrl) return all;
      return `<a class="sha" href="${attr(commitUrl.replaceAll('{sha}', sha))}" target="_blank" rel="noopener">${sha}</a>`;
    });
  }).join('');
}

// The board lists unfinished cards. Frozen cards sit in their epoch or
// sidequest section like any other card (owner 2026-10-04); the `frozen`
// workflow filter narrows the board to them.
export function cardMatches(card, { text = '', workflow = 'all', priority = 'all', kind = 'all', milestone = null } = {}) {
  if (card.phase === 'done') return false;
  if (milestone && card.milestoneId !== milestone) return false;
  if (priority !== 'all' && card.priority !== priority) return false;
  if (kind !== 'all' && card.kind !== kind) return false;
  if (workflow === 'frozen') { if (card.phase !== 'frozen') return false; }
  else if (workflow !== 'all' && workflowRank(card) !== Number(workflow)) return false;
  const nums = cardNumberQuery(text);
  if (nums) return nums.includes(Number(card.num));
  const needle = String(text).trim().toLowerCase();
  return !needle || (`#${card.num}`).includes(needle) || String(card.title || '').toLowerCase().includes(needle);
}

const priorityRank = (card, priorities) => {
  const rank = priorities.indexOf(card.priority);
  return rank < 0 ? priorities.length : rank;
};

// The ways a card list sorts, each with the direction it starts in. The board,
// the Closest view and `tower card list|closest --sort` share them.
export const CARD_SORTS = [
  { key: 'workflow', label: 'Workflow', dir: 'asc' },
  { key: 'workOrder', label: 'Work order', dir: 'asc' },
  { key: 'priority', label: 'Priority', dir: 'asc' },
  { key: 'open', label: 'Open criteria', dir: 'asc' },
  { key: 'updated', label: 'Modified', dir: 'desc' },
  { key: 'created', label: 'Created', dir: 'desc' },
  { key: 'num', label: 'Card number', dir: 'asc' },
  { key: 'title', label: 'Title', dir: 'asc' },
  { key: 'milestone', label: 'Milestone', dir: 'asc' },
  { key: 'lane', label: 'Lane', dir: 'asc' },
];

// A card's criteria in the order the owner picks: by number, by status (open,
// blocked, met, verified) or by text.
export const CRITERIA_SORTS = [
  { key: 'n', label: 'Number', dir: 'asc' },
  { key: 'status', label: 'Status', dir: 'asc' },
  { key: 'text', label: 'Text', dir: 'asc' },
];

// `key` or `key:asc|desc` → { col, dir }, from a --sort flag or a saved
// choice; null for an unknown key or direction.
export function parseSort(text, sorts) {
  const [col, given] = String(text ?? '').split(':');
  const known = sorts.find(s => s.key === col);
  const dir = given ?? known?.dir;
  return known && (dir === 'asc' || dir === 'desc') ? { col, dir } : null;
}

const value = (card, col, priorities) => {
  if (col === 'workflow') return workflowRank(card);
  if (col === 'workOrder') return card.workOrder ?? Infinity;
  if (col === 'priority') return priorityRank(card, priorities);
  if (col === 'open') return (card.crit || criteriaCounts(card)).open;
  if (col === 'updated') return card.updated || '';
  if (col === 'created') return card.created || '';
  if (col === 'milestone') return card.milestoneId || '￿';  // unassigned sorts last
  if (col === 'lane') return card.lane?.label || '';
  if (col === 'num') return card.num ?? 0;
  return String(card[col] || '').toLowerCase();
};

export function sortCards(cards, { col = 'workflow', dir = 'asc' } = {}, priorities = DEFAULT_PRIORITIES) {
  const direction = dir === 'asc' ? 1 : -1;
  const keyed = cards.map(card => ({ card, key: value(card, col, priorities) }));
  return keyed.sort(({ card: a, key: av }, { card: b, key: bv }) => {
    const rank = workflowRank(a) - workflowRank(b);
    if (col === 'workflow') {
      if (rank) return rank * direction;
      const laneRank = (AGENT_LANE_RANK[a.lane?.lane] ?? 4) - (AGENT_LANE_RANK[b.lane?.lane] ?? 4);
      if (laneRank) return laneRank * direction;
    }
    if (av < bv) return -direction;
    if (av > bv) return direction;
    return (a.workOrder ?? Infinity) - (b.workOrder ?? Infinity)
      || priorityRank(a, priorities) - priorityRank(b, priorities)
      || (a.num ?? 0) - (b.num ?? 0);
  }).map(({ card }) => card);
}

const CRITERION_RANK = { open: 0, blocked: 1, met: 2, verified: 3 };
const criterionValue = (item, col) => {
  if (col === 'status') return CRITERION_RANK[item.status === 'open' && item.blockedBy ? 'blocked' : item.status] ?? 4;
  if (col === 'text') return String(item.text || '').toLowerCase();
  return item.n ?? 0;
};

export function sortCriteria(items = [], { col = 'n', dir = 'asc' } = {}) {
  const direction = dir === 'asc' ? 1 : -1;
  return [...items].sort((a, b) => {
    const av = criterionValue(a, col), bv = criterionValue(b, col);
    if (av < bv) return -direction;
    if (av > bv) return direction;
    return (a.n ?? 0) - (b.n ?? 0);
  });
}

// An epoch's number: its stored `num`, else the digits of an `e18`-style id.
export function epochNumber(epoch) {
  if (Number.isInteger(epoch?.num)) return epoch.num;
  const match = /^e(\d+)$/i.exec(String(epoch?.id ?? ''));
  return match ? Number(match[1]) : null;
}

// The one way an epoch is named on screen: `E18`, or `E18 Declarations as
// types` where a name is shown. An id without a number shows as itself.
export function epochLabel(epoch, { name = true } = {}) {
  if (!epoch) return '';
  const num = epochNumber(epoch);
  const tag = num == null ? String(epoch.id ?? '') : `E${num}`;
  return name && epoch.name && epoch.name !== epoch.id ? `${tag} ${epoch.name}` : tag;
}

// Epochs are worked in `order`; epochs without one follow, by number, then id.
export function compareEpochs(a, b) {
  return (a.order ?? Infinity) - (b.order ?? Infinity)
    || (epochNumber(a) ?? Infinity) - (epochNumber(b) ?? Infinity)
    || String(a.id).localeCompare(String(b.id));
}

export const sortEpochs = (epochs = []) => [...epochs].sort(compareEpochs);

// Milestones are worked in `order` within their epoch; those without one
// follow in the order they were added (the sort is stable).
export const sortMilestones = (milestones = []) =>
  [...milestones].sort((a, b) => ((a.order ?? Infinity) - (b.order ?? Infinity)) || 0);

// The `jet test` command a criterion's proof runs (app/proof.mjs): a Compiler
// profile with a filter, or the arguments of another `jet test` run.
export const proofCommand = (proof) => proof.profile === 'cmd'
  ? `jet test ${proof.args.join(' ')}`
  : `jet test Compiler --profile ${proof.profile} --filter ${proof.filter}`;

// A card's criteria by state: open rows split into workable and blocked (they
// wait on a card, a criterion or a decision); settled rows are met or verified.
export function criteriaCounts(card) {
  const counts = { open: 0, blocked: 0, settled: 0, total: 0 };
  for (const item of card?.criteria || []) {
    counts.total++;
    if (item.status !== 'open') counts.settled++;
    else if (item.blockedBy) counts.blocked++;
    else counts.open++;
  }
  return counts;
}

// The epoch chain: epochs in `order`. The first unfinished epoch is active and
// the one after it is next; staffing puts three agents on the active epoch and
// one on the next. Finished epochs (done or arrived) keep their place.
export const CHAIN_STAFF = [3, 1];
const FINISHED_EPOCHS = new Set(['done', 'arrived']);
export const epochFinished = (epoch) => FINISHED_EPOCHS.has(epoch?.status);

// Rank for ordering work by the chain: unfinished epochs in order, then
// finished ones; a card without a known epoch sorts last.
export function chainRank(epochs = []) {
  const sorted = sortEpochs(epochs);
  const ranked = [...sorted.filter(e => !epochFinished(e)), ...sorted.filter(epochFinished)];
  const rank = new Map(ranked.map((e, index) => [e.id, index]));
  return (id) => rank.get(id) ?? Infinity;
}

// One row per epoch in chain order with its role (active, next, later,
// finished), staffing, open cards and open criteria. Done and frozen cards
// and sidequests do not count. Counts read each card's `crit`
// (criteriaCounts), the field the board projection carries.
export function epochChain(epochs = [], cards = []) {
  const tally = new Map();
  for (const c of cards) {
    if (c.phase === 'done' || c.phase === 'frozen' || c.track === 'sidequest' || !c.epoch) continue;
    const row = tally.get(c.epoch) || { cards: 0, open: 0, blocked: 0 };
    const crit = c.crit || criteriaCounts(c);
    row.cards++; row.open += crit.open; row.blocked += crit.blocked;
    tally.set(c.epoch, row);
  }
  let slot = 0;
  return sortEpochs(epochs).map(e => {
    const finished = epochFinished(e);
    const role = finished ? 'finished' : ['active', 'next'][slot] || 'later';
    const staff = finished ? 0 : CHAIN_STAFF[slot] ?? 0;
    if (!finished) slot++;
    const row = tally.get(e.id) || { cards: 0, open: 0, blocked: 0 };
    return { id: e.id, label: epochLabel(e, { name: false }), name: e.name || '', status: e.status, role, staff,
      openCards: row.cards, openCriteria: row.open, blockedCriteria: row.blocked };
  });
}

// Cards nearest to done: unfinished, unfrozen cards with criteria, fewest
// workable open criteria first, then fewest blocked, then chain order. Any
// other card sort (CARD_SORTS) can order the same rows instead.
export function closestCards(cards = [], epochs = [], { epoch = null, limit = Infinity, sort = null, priorities } = {}) {
  const rank = chainRank(epochs);
  const rows = cards
    .map(c => ({ card: c, crit: c.crit || criteriaCounts(c) }))
    .filter(({ card, crit }) => card.phase !== 'done' && card.phase !== 'frozen' && crit.total && (!epoch || card.epoch === epoch));
  if (sort && sort.col !== 'open') {
    const row = new Map(rows.map(r => [r.card, r]));
    return sortCards(rows.map(r => r.card), sort, priorities).map(c => row.get(c)).slice(0, limit);
  }
  const direction = sort?.dir === 'desc' ? -1 : 1;
  return rows
    .sort((a, b) => (a.crit.open - b.crit.open || a.crit.blocked - b.crit.blocked) * direction
      || rank(a.card.epoch) - rank(b.card.epoch) || a.card.num - b.card.num)
    .slice(0, limit);
}

// The radar lists active epochs. A finished epoch still gets a section when
// it holds frozen cards (shown like every unfinished card), so a card is never
// left without its epoch section. Sections follow epoch order.
export function boardEpochs(radar, epochs, cards, milestones) {
  const shown = new Set(radar.map(e => e.id));
  const extras = epochs.flatMap(epoch => {
    if (shown.has(epoch.id)) return [];
    const linked = cards.filter(c => c.epoch === epoch.id && c.track !== 'sidequest');
    const done = linked.filter(c => c.phase === 'done').length;
    const frozen = linked.some(c => c.phase === 'frozen');
    if (!frozen) return [];
    const active = linked.filter(c => !['done', 'frozen'].includes(c.phase)).length;
    const epochMilestones = sortMilestones(milestones.filter(m => m.epochId === epoch.id && m.status !== 'retired'));
    const milestonesMet = epochMilestones.filter(m => m.progress?.met === true).length;
    const milestoneTotal = epochMilestones.length;
    return [{
      id: epoch.id,
      name: epoch.name,
      goal: epoch.goal,
      role: 'finished',
      staff: 0,
      openCriteria: linked.reduce((n, c) => n + (c.phase === 'done' ? 0 : (c.crit || criteriaCounts(c)).open), 0),
      blockedCriteria: linked.reduce((n, c) => n + (c.phase === 'done' ? 0 : (c.crit || criteriaCounts(c)).blocked), 0),
      active,
      done,
      milestoneTotal,
      milestonesMet,
      pct: milestoneTotal ? Math.round(milestonesMet / milestoneTotal * 100) : 0,
      burndown: [],
      milestones: epochMilestones.map(m => ({
        ...m,
        done: m.progress?.done ?? 0,
        total: m.progress?.total ?? 0,
        met: m.progress?.met === true,
        reviewReady: m.progress?.reviewReady === true,
        stalledDays: null,
      })),
    }];
  });
  if (!extras.length) return radar;
  const rank = new Map(sortEpochs(epochs).map((epoch, index) => [epoch.id, index]));
  return [...radar, ...extras].sort((a, b) => (rank.get(a.id) ?? Infinity) - (rank.get(b.id) ?? Infinity));
}

// Card delete confirmation (owner 2026-10-04): every ballot on the card is
// deleted with it. A ratified ballot needs its own explicit confirmation, so
// the payload is only built once every ratified id is confirmed. `decisions`
// pins the list the owner saw; the store refuses if it changed meanwhile.
export function cardDeletePlan(card) {
  const ballots = (card.decisions || []).map(d => ({
    id: d.id, title: d.title || '', status: d.status, outcome: d.outcome ?? null, ratified: d.status === 'ratified',
  }));
  return { ballots, ratified: ballots.filter(b => b.ratified).map(b => b.id) };
}

export function cardDeletePayload(card, confirmedRatified = []) {
  const plan = cardDeletePlan(card);
  const confirmed = new Set(confirmedRatified);
  if (plan.ratified.some(id => !confirmed.has(id))) return null;
  return { id: card.id, by: 'owner', decisions: plan.ballots.map(b => b.id), deleteRatified: plan.ratified };
}
