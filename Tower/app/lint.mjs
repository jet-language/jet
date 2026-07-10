// Card #457 — `tower lint`: rule-based durability sweeper over the live
// board (+ optional docs scan). Each rule is its own function returning
// findings [{rule, ref, msg}]; `lint()` aggregates them. Read-only — never
// mutates the store.
import { readdirSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ballotGaps } from './store.mjs';

// `updated`/`ratifiedAt` are 'YYYY-MM-DD' (today()); `created` on decisions
// and `at` on events are full ISO timestamps (now()) — handle both.
function daysSince(dateStr) {
  if (!dateStr) return Infinity;
  const t = Date.parse(String(dateStr).length <= 10 ? `${dateStr}T00:00:00Z` : dateStr);
  return Number.isNaN(t) ? Infinity : (Date.now() - t) / 86_400_000;
}

const EVIDENCE_RE = /verif|green|tests?|evidence/i;

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

const CORE_RULES = [ruleDoneWithoutEvidence, ruleClaimedIdle, ruleMissingAttribution, ruleBallotGaps, ruleStaleDraft];

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
export function lint(s, history, { docs = false, docsRoot } = {}) {
  let findings = CORE_RULES.flatMap(fn => fn(s));
  findings = findings.concat(ruleOrphanBlockers(s, history));
  if (docs) findings = findings.concat(ruleBallotDocGaps(s, history, { docsRoot }));
  return findings;
}
