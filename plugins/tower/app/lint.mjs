// Card #457 — `tower lint`: rule-based durability sweeper over the live
// board (+ optional docs scan). Each rule is its own function returning
// findings [{rule, ref, msg}]; `lint()` aggregates them. Read-only — never
// mutates the store.
import { readdirSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ballotGaps } from './store.mjs';
import { isOpenCard, referenceLabel, referenceTokens } from './card-matching.mjs';

// `updated`/`ratifiedAt` are 'YYYY-MM-DD' (today()); `created` on decisions
// and `at` on events are full ISO timestamps (now()) — handle both.
function daysSince(dateStr) {
  if (!dateStr) return Infinity;
  const t = Date.parse(String(dateStr).length <= 10 ? `${dateStr}T00:00:00Z` : dateStr);
  return Number.isNaN(t) ? Infinity : (Date.now() - t) / 86_400_000;
}

const EVIDENCE_RE = /verif|green|tests?|evidence/i;
const DISPUTED_EVIDENCE_RE = /\b(?:not\s+satisfied|blocked|could\s+not|unproven|not\s+run)\b/i;
const PHASE_SEQ = { frozen: -1, deciding: 0, planning: 1, triage: 1, ready: 2, building: 3, verify: 4, done: 5 };

// ---- rule: done-without-evidence -------------------------------------------
// A card phase==='done' whose log never mentions verification AND whose
// criteria are empty or not all verified — done with nothing to show for it.
export function ruleDoneWithoutEvidence(s) {
  const findings = [];
  for (const c of s.cards) {
    if (c.phase !== 'done') continue;
    const items = c.criteria || [];
    const criteriaVerified = items.length > 0 && items.every(i => i.status === 'verified');
    if (criteriaVerified) continue;
    const hasEvidence = (c.log || []).some(l => EVIDENCE_RE.test(l.text || ''));
    if (hasEvidence) continue;
    findings.push({ rule: 'done-without-evidence', ref: `#${c.num}`,
      msg: `#${c.num} "${c.title}" is done with no verif/tests/green/evidence mention in its log and no fully-verified criteria` });
  }
  return findings;
}

// ---- rule: claimed-idle ------------------------------------------------------
// Assignee set, card in an active-work phase, but untouched for 3+ days —
// smells like a dropped claim nobody released.
export function ruleClaimedIdle(s) {
  const findings = [];
  for (const c of s.cards) {
    if (!c.assignee) continue;
    if (c.phase !== 'building' && c.phase !== 'ready') continue;
    const age = daysSince(c.updated);
    if (age > 3) findings.push({ rule: 'claimed-idle', ref: `#${c.num}`,
      msg: `#${c.num} "${c.title}" claimed by ${c.assignee}, phase ${c.phase}, untouched ${Math.floor(age)}d` });
  }
  return findings;
}

// ---- rule: missing-attribution ----------------------------------------------
// Every event should carry `by` — an unattributed mutation is untraceable.
export function ruleMissingAttribution(s) {
  const findings = [];
  for (const e of s.events.slice(0, 500)) {
    if (e.by && String(e.by).trim()) continue;
    findings.push({ rule: 'missing-attribution', ref: e.ref || '(no ref)',
      msg: `event "${e.action}" at ${e.at} has no attribution (by)` });
  }
  return findings;
}

// ---- rule: ballot-gaps -------------------------------------------------------
// OPEN, non-draft decisions that would fail addDecision's own E_BALLOT gate
// today — i.e. slipped in before the gate existed, or hand-restored.
// Acceptance ballots are system-generated evidence, not narrative ballots
// (same exemption addDecision itself makes) — excluded here too.
export function ruleBallotGaps(s) {
  const findings = [];
  for (const d of s.decisions) {
    if (d.status === 'ratified' || d.draft || d.group === 'acceptance') continue;
    const gaps = ballotGaps(d);
    if (gaps.length) findings.push({ rule: 'ballot-gaps', ref: d.id,
      msg: `${d.id} incomplete ballot — missing: ${gaps.join(', ')}` });
  }
  return findings;
}

// ---- rule: stale-draft --------------------------------------------------------
// A draft ballot nobody has finished in a week.
export function ruleStaleDraft(s) {
  const findings = [];
  for (const d of s.decisions) {
    if (!d.draft) continue;
    const age = daysSince(d.created);
    if (age > 7) findings.push({ rule: 'stale-draft', ref: d.id,
      msg: `${d.id} draft untouched ${Math.floor(age)}d — finish it (decision update --ready) or drop it` });
  }
  return findings;
}

// ---- rule: orphan-blockers ---------------------------------------------------
// blockedBy pointing at an id that resolves nowhere — live cards, history
// cards, or live decisions. laneOf() silently treats a dangling ref as
// non-blocking; this rule is how that silence gets surfaced.
export function ruleOrphanBlockers(s, history) {
  const findings = [];
  const liveCardIds = new Set(s.cards.map(c => c.id));
  const historyCardIds = new Set((history?.cards || []).map(c => c.id));
  const decisionIds = new Set(s.decisions.map(d => d.id));
  for (const c of s.cards) {
    for (const id of c.blockedBy || []) {
      if (liveCardIds.has(id) || historyCardIds.has(id) || decisionIds.has(id)) continue;
      findings.push({ rule: 'orphan-blockers', ref: `#${c.num}`,
        msg: `#${c.num} "${c.title}" blockedBy dangling ref ${id}` });
    }
  }
  return findings;
}

// ---- rule: blocker-unpopulated (D-TWR-OPS2=A) --------------------------------
// Plan-phase exit: an epoch-track card that already has a plan must also
// record its real prerequisites (or explicitly claim none). Sidequests and
// cards past planning are grandfathered — this catches the blank-graph drift
// at the moment a planner would otherwise leave blockedBy empty forever.
export function ruleBlockerUnpopulated(s) {
  const findings = [];
  for (const c of s.cards) {
    if (c.track !== 'epoch') continue;
    if (c.phase !== 'planning') continue;
    if (!c.plan || !String(c.plan).trim()) continue;
    if ((c.blockedBy || []).length) continue;
    // Explicit none-marker in the plan body: "blockedBy: none" / "no blockers"
    if (/\b(?:blockedBy\s*:\s*none|no blockers?)\b/i.test(c.plan)) continue;
    findings.push({ rule: 'blocker-unpopulated', ref: `#${c.num}`,
      msg: `#${c.num} "${c.title}" has a plan but empty blockedBy — record prerequisites (or write "blockedBy: none" in the plan)` });
  }
  return findings;
}

// A met or verified row must not carry evidence that says the work failed or
// never ran. Reopen the row when the evidence changes the result.
export function ruleCriteriaEvidenceConflicts(s) {
  const findings = [];
  for (const c of s?.cards || []) {
    for (const item of c.criteria || []) {
      if (!['met', 'verified'].includes(item.status)) continue;
      const evidence = String(item.evidence || '').trim();
      if (!DISPUTED_EVIDENCE_RE.test(evidence)) continue;
      findings.push({ rule: 'criteria-evidence-conflict', ref: '#' + c.num,
        msg: '#' + c.num + ' "' + c.title + '" criterion #' + item.n + ' is ' + item.status + ' but its evidence disputes it: ' + evidence });
    }
  }
  return findings;
}

// Criteria advance the work to verification. A card moved back to an earlier
// phase must not keep rows that claim the work already passed verification.
export function ruleCriteriaPhaseDrift(s) {
  const findings = [];
  for (const c of s?.cards || []) {
    const items = c.criteria || [];
    const phase = PHASE_SEQ[c.phase];
    if (!items.length || phase == null || phase >= PHASE_SEQ.verify) continue;
    const verified = items.filter(item => item.status === 'verified').map(item => '#' + item.n);
    const allSettled = items.every(item => item.status !== 'open');
    if (!verified.length && !allSettled) continue;
    const rows = verified.length ? 'verified rows ' + verified.join(', ') : 'all criteria met or verified';
    findings.push({ rule: 'criteria-phase-drift', ref: '#' + c.num,
      msg: '#' + c.num + ' "' + c.title + '" is in ' + c.phase + ' but holds ' + rows });
  }
  return findings;
}

// ---- rule: duplicate-suspect -----------------------------------------------
// Two or more open cards naming the same test, fixture, example, or spec
// reference usually describe one work slice under different symptoms.
export function ruleDuplicateSuspects(s) {
  const byReference = new Map();
  for (const c of s?.cards || []) {
    if (!isOpenCard(c)) continue;
    for (const token of referenceTokens(c.body)) {
      const group = byReference.get(token) || [];
      group.push(c);
      byReference.set(token, group);
    }
  }

  const groups = new Map();
  for (const [token, cards] of byReference) {
    if (cards.length < 2) continue;
    const key = cards.map(c => c.id ?? `#${c.num}`).join('|');
    const group = groups.get(key) || { cards, references: [] };
    group.references.push(referenceLabel(token));
    groups.set(key, group);
  }

  return [...groups.values()].map(({ cards, references }) => ({
    rule: 'duplicate-suspect',
    ref: cards.map(c => `#${c.num}`).join(','),
    msg: `open cards share ${references.join(', ')}: ${cards.map(c => `#${c.num} "${c.title ?? '(untitled)'}"`).join('; ')}`,
  }));
}

const CORE_RULES = [ruleDoneWithoutEvidence, ruleClaimedIdle, ruleMissingAttribution, ruleBallotGaps, ruleStaleDraft, ruleBlockerUnpopulated, ruleUnhomedCard, ruleCriteriaEvidenceConflicts, ruleCriteriaPhaseDrift, ruleDuplicateSuspects];

// ---- --docs mode: ratified decision id still listed in an open-ballot doc --
// Precise on purpose: only docs/ballots/*.md (not docs/plans/**), since plans
// may legitimately reference a ratified id long after the fact.
const DECISION_ID_RE = /\bD-[A-Z0-9-]+\b/g;

export function ruleBallotDocGaps(s, history, { docsRoot } = {}) {
  const dir = join(docsRoot, 'ballots');
  if (!existsSync(dir)) return [];
  const ratified = new Set([
    ...s.decisions.filter(d => d.status === 'ratified').map(d => d.id),
    ...(history?.decisions || []).map(d => d.id), // decisions only retire once ratified
  ]);
  const findings = [];
  for (const f of readdirSync(dir)) {
    if (!f.endsWith('.md')) continue;
    const text = readFileSync(join(dir, f), 'utf8');
    // A doc that declares itself decided history isn't an open queue.
    const head = text.split('\n').slice(0, 10).join('\n');
    if (/status:\s*(ratified|historical|archived)/i.test(head)) continue;
    const seen = new Set();
    for (const m of text.matchAll(DECISION_ID_RE)) {
      const id = m[0];
      if (seen.has(id) || !ratified.has(id)) continue;
      seen.add(id);
      findings.push({ rule: 'ratified-in-open-ballot-doc', ref: id,
        msg: `${id} is ratified but still listed in docs/ballots/${f}` });
    }
  }
  return findings;
}

// ---- aggregate ---------------------------------------------------------------
// Owner ruling 2026-08-05: every card lives in an epoch, is a sidequest, or is
// frozen. The store rejects new violations; this rule catches any that predate
// the guard or arrive through repair/restore paths.
export function ruleUnhomedCard(s) {
  const findings = [];
  for (const c of s.cards.filter(c => c.track === 'epoch' && c.epoch == null && c.phase !== 'frozen')) {
    findings.push({ rule: 'unhomed-card', ref: `#${c.num}`,
      detail: `epoch-track card with no epoch — assign an epoch, make it a sidequest, or freeze it` });
  }
  return findings;
}

export function lint(s, history, { docs = false, docsRoot } = {}) {
  let findings = CORE_RULES.flatMap(fn => fn(s));
  findings = findings.concat(ruleOrphanBlockers(s, history));
  if (docs) findings = findings.concat(ruleBallotDocGaps(s, history, { docsRoot }));
  return findings;
}
