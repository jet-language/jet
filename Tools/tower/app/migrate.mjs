// Import an older Tower data file (v3 era: single tower.json, `binder` bay,
// no milestones/events/rev) into the v4 shape. Lossless: unknown fields on
// cards/decisions ride along untouched.
import { VERSION, normalize } from './store.mjs';

// Owner ballot standard: stored losses now explain how we reduce them. Run on
// every load as well as imports; normal Tower writes persist the canonical form.
export function migrateRecommendationLosses(decision) {
  let changed = false;
  for (const recommendation of [decision.recommendation, decision.surface?.recommendation]) {
    if (!Array.isArray(recommendation?.losses)) continue;
    for (const item of recommendation.losses) {
      if (!item || typeof item !== 'object' || Array.isArray(item)) continue;
      if (Object.hasOwn(item, 'whyUnavoidable')) {
        if (!Object.hasOwn(item, 'mitigation')) item.mitigation = item.whyUnavoidable;
        delete item.whyUnavoidable;
        changed = true;
      }
      if (typeof item.mitigation === 'string' && /^Mitigation:\s*/i.test(item.mitigation)) {
        item.mitigation = item.mitigation.replace(/^Mitigation:\s*/i, '');
        changed = true;
      }
    }
  }
  return changed;
}

// Owner ballot rule (2026-10-28): a ballot needs only one fresh adversarial
// pass. Unratified process-4 ballots move to process 5 and drop the retired
// beginner pass. Ratified records keep their passes as historical evidence.
// Runs on every load; normal Tower writes persist the canonical form.
export function migrateReviewPasses(decision) {
  if (decision.status === 'ratified' || decision.group === 'acceptance' || decision.ballotProcessVersion !== 4) return false;
  decision.ballotProcessVersion = 5;
  if (decision.reviewPasses && typeof decision.reviewPasses === 'object') delete decision.reviewPasses.beginner;
  return true;
}

export function migrate(old, { project = 'Project' } = {}) {
  const src = old && typeof old === 'object' ? old : {};
  const meta = src.meta || {};
  const s = normalize({
    meta: {
      version: VERSION,
      project: meta.project || project,
      currentEpoch: meta.currentEpoch ?? null,
      nextNum: meta.nextNum ?? 1,
      rev: 0,
      ui: { toggled: meta.ui?.toggled || meta.ui?.open || [] },
    },
    epochs: (src.epochs || []).map(e => (
      typeof e === 'string'
        ? { id: e, name: e, goal: '', status: 'open' }
        : { status: 'open', goal: '', ...e, name: e.name || e.label || e.id }
    )),
    milestones: src.milestones || [],
    cards: (src.cards || []).map(c => ({ milestoneId: null, assignee: null, ...c })),
    decisions: src.decisions || [],
    questions: src.questions || [],
    ideas: src.ideas || src.binder || [],
    events: src.events || [],
  });
  return s;
}
