// `tower apply`: many board edits as one write. A plan is
// { "ops": [ { "op": "meet", "card": "#12", "n": 3, "evidence": "…" }, … ] }.
// Every op's shape is checked before any runs; then all run in one store
// transaction (one revision and one backup, so one `tower undo` reverts the
// whole plan), and the first refused op leaves the board untouched.
import * as db from './store.mjs';
import { TowerError } from './store.mjs';

const fail = (code, msg) => { throw new TowerError(code, msg); };
const pick = (op, keys) => Object.fromEntries(keys.filter(k => Object.hasOwn(op, k)).map(k => [k, op[k]]));

const CARD_UPDATE_FIELDS = [...db.CARD_FIELDS.filter(k => k !== 'needsAcceptance'), 'addTags', 'removeTags', 'logEntry'];
const EPOCH_FIELDS = ['name', 'goal', 'status', 'order', 'exitCriteria'];
const MILESTONE_FIELDS = ['title', 'goal', 'order', 'exitCriteria', 'epochId'];

// op → the keys it needs, the keys it may carry, what it names, and its write.
export const APPLY_OPS = {
  meet: {
    needs: ['card', 'n', 'evidence'], may: ['kind'],
    ref: o => `${o.card} c${o.n}`,
    run: (s, o, by, cfg, h) => db.meetCriterion(s, o.card, o.n, { evidence: o.evidence, kind: o.kind, by }, cfg, h),
  },
  edit: {
    needs: ['card', 'n'], may: ['text', 'proof', 'blockedBy', 'reason'],
    ref: o => `${o.card} c${o.n}`,
    run: (s, o, by) => db.editCriterion(s, o.card, o.n, { ...pick(o, ['text', 'proof', 'blockedBy', 'reason']), by }),
  },
  add: {
    needs: ['card', 'text'], may: ['proof', 'blockedBy'],
    ref: o => o.card,
    run: (s, o, by) => db.addCriterion(s, o.card, o.text, by, pick(o, ['proof', 'blockedBy'])),
  },
  remove: {
    needs: ['card', 'n', 'reason'], may: [],
    ref: o => `${o.card} c${o.n}`,
    run: (s, o, by) => db.removeCriterion(s, o.card, o.n, { reason: o.reason, by }),
  },
  move: {
    needs: ['card', 'n', 'to'], may: ['reason'],
    ref: o => `${o.card} c${o.n} → ${o.to}`,
    run: (s, o, by) => db.moveCriterion(s, o.card, o.n, { to: o.to, reason: o.reason, by }),
  },
  'card.update': {
    needs: ['card'], may: CARD_UPDATE_FIELDS, change: true,
    ref: o => o.card,
    run: (s, o, by, cfg) => {
      const card = db.findCard(s, o.card);
      if (card?.needsAcceptance && o.phase === 'done' && by === 'owner')
        fail('E_ACCEPTANCE_OWNER_UI', `card #${card.num} requires the dedicated owner verification UI`);
      return db.updateCard(s, o.card, { ...pick(o, CARD_UPDATE_FIELDS), by }, cfg);
    },
  },
  'epoch.update': {
    needs: ['id'], may: EPOCH_FIELDS, change: true,
    ref: o => o.id,
    run: (s, o, by) => db.updateEpoch(s, o.id, { ...pick(o, EPOCH_FIELDS), by }),
  },
  'milestone.add': {
    needs: ['epochId', 'title'], may: ['id', 'goal', 'order', 'exitCriteria'],
    ref: o => o.id || o.title,
    run: (s, o, by) => db.addMilestone(s, { ...pick(o, ['id', 'epochId', 'title', 'goal', 'order', 'exitCriteria']), by }),
  },
  'milestone.update': {
    needs: ['id'], may: MILESTONE_FIELDS, change: true,
    ref: o => o.id,
    run: (s, o, by) => db.updateMilestone(s, o.id, pick(o, MILESTONE_FIELDS), by),
  },
  'milestone.retire': {
    needs: ['id', 'reason'], may: [],
    ref: o => o.id,
    run: (s, o, by) => db.retireMilestone(s, o.id, { reason: o.reason, by }),
  },
};

// Every shape problem in the plan at once, so one fix-up pass clears them.
export function checkPlan(plan) {
  if (!plan || typeof plan !== 'object' || Array.isArray(plan) || !Array.isArray(plan.ops))
    fail('E_USAGE', 'a plan is a JSON object { "ops": [ … ] }');
  const extra = Object.keys(plan).filter(k => k !== 'ops');
  if (extra.length) fail('E_USAGE', `a plan holds only "ops"; unknown key${extra.length > 1 ? 's' : ''}: ${extra.join(', ')}`);
  if (!plan.ops.length) fail('E_USAGE', 'the plan has no ops');
  const problems = [];
  plan.ops.forEach((o, i) => {
    const at = `op ${i + 1}`;
    if (!o || typeof o !== 'object' || Array.isArray(o)) return problems.push(`${at}: not an object`);
    const spec = APPLY_OPS[o.op];
    if (!spec) return problems.push(`${at}: unknown op ${JSON.stringify(o.op)} — one of ${Object.keys(APPLY_OPS).join(', ')}`);
    const missing = spec.needs.filter(k => o[k] === undefined || o[k] === null || o[k] === '');
    if (missing.length) problems.push(`${at} (${o.op}): needs ${missing.join(', ')}`);
    const unknown = Object.keys(o).filter(k => k !== 'op' && !spec.needs.includes(k) && !spec.may.includes(k));
    if (unknown.length) problems.push(`${at} (${o.op}): unknown field${unknown.length > 1 ? 's' : ''} ${unknown.join(', ')}`);
    if (spec.change && !spec.may.some(k => Object.hasOwn(o, k))) problems.push(`${at} (${o.op}): changes nothing`);
    if (o.op === 'edit' && !['text', 'proof', 'blockedBy'].some(k => Object.hasOwn(o, k)))
      problems.push(`${at} (edit): needs text, proof or blockedBy`);
  });
  if (problems.length) fail('E_USAGE', `plan refused, nothing applied:\n  ${problems.join('\n  ')}`);
}

// Run a checked plan against s. Any refused op names its place in the plan
// and keeps its own error code; the caller's transaction then writes nothing.
export function applyPlan(s, plan, by, config, history) {
  checkPlan(plan);
  if (!by || by === true) fail('E_USAGE', 'apply needs --by <agent>');
  const counts = {};
  const ops = plan.ops.map((o, i) => {
    const spec = APPLY_OPS[o.op];
    try {
      spec.run(s, o, by, config, history);
    } catch (error) {
      if (!(error instanceof TowerError)) throw error;
      fail(error.code, `op ${i + 1} (${o.op} ${spec.ref(o)}): ${error.message} — nothing applied`);
    }
    counts[o.op] = (counts[o.op] || 0) + 1;
    return { op: o.op, ref: spec.ref(o) };
  });
  const summary = Object.entries(counts).map(([op, n]) => `${n} ${op}`).join(', ');
  // The cards the plan touched, so each card's history shows the plan.
  const refs = [...new Set(plan.ops.flatMap(o => [o.card, o.to])
    .filter(ref => ref != null).map(ref => db.findCard(s, ref)?.id).filter(Boolean))];
  db.logEvent(s, { by, action: 'board.apply', refs, note: `${ops.length} op${ops.length === 1 ? '' : 's'}: ${summary}` });
  return { applied: ops.length, summary, ops };
}
