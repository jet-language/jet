// Import an older Tower data file (v3 era: single tower.json, `binder` bay,
// no milestones/events/rev) into the v4 shape. Lossless: unknown fields on
// cards/decisions ride along untouched.
import { VERSION, normalize } from './store.mjs';

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
    epochs: src.epochs || [],
    milestones: src.milestones || [],
    cards: (src.cards || []).map(c => ({ milestoneId: null, assignee: null, ...c })),
    decisions: src.decisions || [],
    questions: src.questions || [],
    ideas: src.ideas || src.binder || [],
    events: src.events || [],
  });
  // Epochs may be plain strings in very old files.
  s.epochs = s.epochs.map(e => (typeof e === 'string' ? { id: e, name: e, goal: '', status: 'open' } : { status: 'open', goal: '', ...e, name: e.name || e.label || e.id }));
  return s;
}
