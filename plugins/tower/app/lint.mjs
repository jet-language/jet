// Card #457 — `tower lint`: rule-based durability sweeper over the live
// board (+ optional docs scan). Each rule is its own function returning
// findings [{rule, ref, msg}]; `lint()` aggregates them. Read-only — never
// mutates the store.
import { readdirSync, existsSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { ballotGaps, findCard } from './store.mjs';
import { isOpenCard, referenceLabel, referenceTokens } from './card-matching.mjs';

// `updated`/`ratifiedAt` are 'YYYY-MM-DD' (today()); `created` on decisions
// and `at` on events are full ISO timestamps (now()) — handle both.
function daysSince(dateStr) {
  if (!dateStr) return Infinity;
  const t = Date.parse(String(dateStr).length <= 10 ? `${dateStr}T00:00:00Z` : dateStr);
  return Number.isNaN(t) ? Infinity : (Date.now() - t) / 86_400_000;
}

const EVIDENCE_RE = /verif|green|tests?|evidence/i;
const EXPLICIT_DISPUTE_RE = /\b(?:not\s+satisfied|unproven|(?:criterion|proof|check|test)\s+(?:is|remains)\s+(?:not\s+met|incomplete))\b/i;
const WEAK_DISPUTE_RE = /\b(?:could|can|did|does)\s+not\s+(?:be\s+)?(?:run|finish|pass|prove|cover|exercise|satisfy|meet|complete)|\bnot\s+(?:run|verified|met|complete|implemented)\b/i;
const HISTORICAL_EVIDENCE_RE = /\b(?:before|prior(?:\s+to)?|previously|formerly|earlier|at\s+that\s+head|historical(?:ly)?|was\s+fixed|were\s+fixed|has\s+been\s+fixed|have\s+been\s+fixed|now\s+(?:fits|passes|green|works|fixed|resolved)|removed|resolved|predated|old\s+state|past)\b/i;
const IRRELEVANT_EVIDENCE_RE = /\b(?:unrelated|independent|contains|quoted|source\s+string|outside\s+(?:this|the)\s+(?:card|criterion|scope)|not\s+(?:this|the)\s+(?:card|criterion|change))\b/i;
const QUALIFIED_EVIDENCE_RE = /\b(?:evidence|criterion|audit|review|report|log)\b[^.!?]{0,80}\b(?:said|reported|recorded|found|read|showed)\b[^.!?]{0,40}\b(?:not\s+satisfied|unproven)\b|\bnot\s+satisfied\s+by\b|\bnot\s+implemented\s+as\s+written\b/i;
const POSITIVE_EVIDENCE_RE = /\b(?:proof|pass(?:ed|es|ing)?|green|verified|confirmed|clean|success(?:ful)?|works?|assert(?:s|ed)?|golden|parity|measured|re-verified|production)\b/i;

// ---- rule: done-without-evidence -------------------------------------------
// A card phase==='done' whose log never mentions evidence AND whose criteria
// are empty or not all met/verified — done with nothing to show for it.
export function ruleDoneWithoutEvidence(s) {
  const findings = [];
  for (const c of s.cards) {
    if (c.phase !== 'done') continue;
    const items = c.criteria || [];
    const criteriaSettled = items.length > 0 && items.every(i => ['met', 'verified'].includes(i.status));
    if (criteriaSettled) continue;
    const hasEvidence = (c.log || []).some(l => EVIDENCE_RE.test(l.text || ''));
    if (hasEvidence) continue;
    findings.push({ rule: 'done-without-evidence', ref: `#${c.num}`,
      msg: `#${c.num} "${c.title}" is done with no verif/tests/green/evidence mention in its log and no fully-met criteria` });
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
  const decisionIds = new Set(s.decisions.map(d => d.id));
  const archived = { cards: history?.cards || [] };
  for (const c of s.cards) {
    for (const id of c.blockedBy || []) {
      if (findCard(s, id) || findCard(archived, id) || decisionIds.has(id)) continue;
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
      if (!evidenceConflicts(evidence)) continue;
      findings.push({ rule: 'criteria-evidence-conflict', ref: '#' + c.num,
        msg: '#' + c.num + ' "' + c.title + '" criterion #' + item.n + ' is ' + item.status + ' but its evidence disputes it: ' + evidence });
    }
  }
  return findings;
}

function evidenceConflicts(evidence) {
  // Inline code often quotes a diagnostic, fixture, or source string. It is
  // not a claim about the criterion's current state.
  const prose = evidence.replace(/`[^`]*`/g, ' ');
  const sentences = prose.split(/\n+|(?<=[.!?])\s+/).map(s => s.trim()).filter(Boolean);
  const hasPositiveProof = POSITIVE_EVIDENCE_RE.test(prose);
  return sentences.some(sentence => {
    const explicit = EXPLICIT_DISPUTE_RE.test(sentence);
    const weak = WEAK_DISPUTE_RE.test(sentence);
    if (!explicit && !weak) return false;
    if (HISTORICAL_EVIDENCE_RE.test(sentence) || IRRELEVANT_EVIDENCE_RE.test(sentence) || QUALIFIED_EVIDENCE_RE.test(sentence)) return false;
    // Strong present-tense admissions are findings even when the same
    // evidence also records earlier or partial proof. Weaker phrases only
    // dispute a settled row when no positive proof appears in the evidence.
    return explicit || !hasPositiveProof;
  });
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

const CORE_RULES = [ruleDoneWithoutEvidence, ruleClaimedIdle, ruleMissingAttribution, ruleBallotGaps, ruleStaleDraft, ruleBlockerUnpopulated, ruleUnhomedCard, ruleCriteriaEvidenceConflicts, ruleDuplicateSuspects];

const DECISION_ID_RE = /\bD-[A-Z0-9]+(?:-[A-Z0-9]+)*\b/g;
const CARD_REF_RE = /#(\d+)\b/g;
const DECISION_CONTEXT_RE = /\bD-[A-Z0-9]+(?:-[A-Z0-9]+)*\b/;

function specFiles(root) {
  if (!existsSync(root)) return [];
  const files = [];
  const walk = dir => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (entry.name.startsWith('.')) continue;
      const abs = join(dir, entry.name);
      if (entry.isDirectory()) walk(abs);
      else if (/\.(?:md|json|txt)$/i.test(entry.name)) files.push(abs);
    }
  };
  walk(root);
  return files;
}

function addLocation(map, id, path, line) {
  const locations = map.get(id) || [];
  locations.push(`${path}:${line}`);
  map.set(id, locations);
}

function cardRefsInLine(line) {
  // Only treat references as cards when prose names cards/issues, or when a
  // decision citation carries its owning card list. This avoids `[U8#4096]`
  // and similar language examples.
  const hasCardContext = /\b(?:card|cards|issue|issues)\b/i.test(line) || DECISION_CONTEXT_RE.test(line);
  if (!hasCardContext) return [];
  return [...line.matchAll(CARD_REF_RE)]
    .filter(m => (line.slice(0, m.index).match(/`/g) || []).length % 2 === 0)
    .map(m => `#${m[1]}`);
}

function isPlaceholderDecision(id) {
  return /^D-(?:X+|Y+|EXAMPLE|TODO)$/i.test(id);
}

export function ruleSpecReferenceGaps(s, history, { docsRoot = join(process.cwd(), 'docs') } = {}) {
  const specRoot = join(docsRoot, 'spec');
  const knownCards = new Set([
    ...(s?.cards || []).map(c => c.num),
    ...(history?.cards || []).map(c => c.num),
  ]);
  const knownDecisions = new Set([
    ...(s?.decisions || []).map(d => d.id),
    ...(history?.decisions || []).map(d => d.id),
  ]);
  const missingCards = new Map();
  const missingDecisions = new Map();

  for (const file of specFiles(specRoot)) {
    const path = `docs/${relative(docsRoot, file).replaceAll('\\', '/')}`;
    const lines = readFileSync(file, 'utf8').split('\n');
    lines.forEach((line, index) => {
      for (const ref of cardRefsInLine(line)) {
        if (!knownCards.has(Number(ref.slice(1)))) addLocation(missingCards, ref, path, index + 1);
      }
      for (const match of line.matchAll(DECISION_ID_RE)) {
        if (!isPlaceholderDecision(match[0]) && !knownDecisions.has(match[0]))
          addLocation(missingDecisions, match[0], path, index + 1);
      }
    });
  }

  const findings = [];
  for (const [ref, locations] of [...missingCards].sort(([a], [b]) => a.localeCompare(b, undefined, { numeric: true })))
    findings.push({ rule: 'spec-card-ref-missing', ref,
      msg: `${locations.join(', ')} cites ${ref}, but Tower has no card record` });
  for (const [ref, locations] of [...missingDecisions].sort(([a], [b]) => a.localeCompare(b)))
    findings.push({ rule: 'spec-decision-ref-missing', ref,
      msg: `${locations.join(', ')} cites ${ref}, but Tower has no decision record` });
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
  if (docs) findings = findings.concat(ruleSpecReferenceGaps(s, history, { docsRoot }));
  return findings;
}
