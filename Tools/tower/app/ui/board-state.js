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

// Owner Now/beacon review queue: ONLY needsAcceptance cards the owner can act
// on. Bare phase=verify is a legacy agent state, not an owner duty, and so is
// a visual card still waiting on agent criteria or on its screen capture:
// every card in the owner's lane offers both Accept and Bounce.
export const openAcceptanceBallot = (card) =>
  (card.decisions || []).find(d => d.id === `D-ACCEPT-${card.num}` && d.status !== 'ratified') || null;

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

// Frozen cards sit in their epoch or sidequest section like any other card
// (owner 2026-10-04); the `frozen` workflow filter narrows the board to them.
export function cardMatches(card, { text = '', workflow = 'all', priority = 'all', kind = 'all', showClosed = false, milestone = null } = {}) {
  if (card.phase === 'done' && !showClosed) return false;
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

const value = (card, col, priorities) => {
  if (col === 'workflow') return workflowRank(card);
  if (col === 'workOrder') return card.workOrder ?? Infinity;
  if (col === 'priority') return priorityRank(card, priorities);
  if (col === 'updated') return card.updated || '';
  if (col === 'created') return card.created || '';
  if (col === 'milestone') return card.milestoneId || '￿';  // unassigned sorts last
  if (col === 'lane') return card.lane?.label || '';
  if (col === 'num') return card.num ?? 0;
  return String(card[col] || '').toLowerCase();
};

export function sortCards(cards, { col = 'workflow', dir = 'asc' } = {}, priorities = DEFAULT_PRIORITIES) {
  const direction = dir === 'asc' ? 1 : -1;
  return [...cards].sort((a, b) => {
    const rank = workflowRank(a) - workflowRank(b);
    if (col === 'workflow') {
      if (rank) return rank * direction;
      const laneRank = (AGENT_LANE_RANK[a.lane?.lane] ?? 4) - (AGENT_LANE_RANK[b.lane?.lane] ?? 4);
      if (laneRank) return laneRank * direction;
    }
    const av = value(a, col, priorities), bv = value(b, col, priorities);
    if (av < bv) return -direction;
    if (av > bv) return direction;
    return (a.workOrder ?? Infinity) - (b.workOrder ?? Infinity)
      || priorityRank(a, priorities) - priorityRank(b, priorities)
      || (a.num ?? 0) - (b.num ?? 0);
  });
}

// The radar lists active epochs. A finished epoch still gets a section when
// it holds frozen cards (always shown, like every card) or, with Show closed
// on, done cards — a card is never left without its epoch section.
export function boardEpochs(radar, epochs, cards, milestones, showClosed) {
  const shown = new Set(radar.map(e => e.id));
  const extras = epochs.flatMap(epoch => {
    if (shown.has(epoch.id)) return [];
    const linked = cards.filter(c => c.epoch === epoch.id && c.track !== 'sidequest');
    const done = linked.filter(c => c.phase === 'done').length;
    const frozen = linked.some(c => c.phase === 'frozen');
    if (!frozen && !(showClosed && done)) return [];
    const active = linked.filter(c => !['done', 'frozen'].includes(c.phase)).length;
    const epochMilestones = milestones.filter(m => m.epochId === epoch.id && !m.archived);
    const milestonesMet = epochMilestones.filter(m => m.progress?.met === true).length;
    const milestoneTotal = epochMilestones.length;
    return [{
      id: epoch.id,
      name: epoch.name,
      goal: epoch.goal,
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
  return extras.length ? [...radar, ...extras] : radar;
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
