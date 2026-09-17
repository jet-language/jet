---
name: type-unification-audit
description: >-
  Audit whether Jet's typed artifacts are honest and unified. Use when reviewing
  traits, tags, markers, handles, or keyword-adjacent type questions.
---

# Type-unification audit (draft)

Find where the compiler reasons with a type it does not admit, or where one classification wears several spellings. Lead each finding with the fix and evidence. This audit supports fixes; it is not a conviction list.

If a run exposes a missing lens, record the proposed method improvement in that run's report. Do not edit this skill during the audit; maintenance is a separate authorized task.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own shared permissions, scope depth, evidence rules, publication, and finding dispositions. This method owns the target census, boundary law, live probes, mandatory reviews, fix plan, and finite closeout.

## Scope and authority

At activation, freeze the target construct or concept and its relevant declaration, artifact, and dependency closure. Route by that target; require a complete census inside the closure, not a search for every type-like thing in Jet. Authority is owner instruction → ratified Tower verdicts and acceptance terms → relevant domain spec → `AGENTS.md` invariants and owner gates → this skill. Check ratification dates; a same-day verdict is still law.

## Boundary law

**A control construct is an expression wherever it produces a value, and its runtime artifacts are types; the construct itself never is.** A reified construct is a second lambda (I8). The type-shaped thing near a keyword is its artifact: the handle, yielded collection, range, or stream. Audit artifacts, not keywords.

The probe and honesty sections of the standing lens apply to the declared target. Skip unrelated four-question, five-quantity, micro-sweep, and competitive work. A phantom type, closed table, inert marker, or unnameable handle needs a live probe to separate an enforced fact from a recorded claim. State agent cost as well as human cost.

## Route and obligations

| Need | Read |
| --- | --- |
| Census, taxonomy, and report shape | [`references/taxonomy.md`](references/taxonomy.md) |
| Ratification, soundness, I8 traps, and reviews | [`references/reviews.md`](references/reviews.md) |

For the declared target: inventory relevant kind mechanisms and fields; census phantom names; run minimal `.jet` repros through `scripts/agent/jet-env`; classify with the exact taxonomy; and propose the smallest honest fix, preferring ratified enums, distincts, or markers over a new kind. A claim without a probe or `file:line` cite does not enter the report.

All three fresh-context reviews are mandatory on an authorized run: peer, adversarial, and pay-up-front. Record each material finding and resolution. Cards and ballots are created only when the owner explicitly asks; bugs map to cards, and syntax, surface, API, or feature changes map to `tower-ballot` decisions.

## Completion and permissions

Stop when the frozen target census is complete, every row has evidence or an honest `unknown`, ratification, soundness, and I8 checks are recorded, all three reviews have resolutions, and the fix-first report plus disposition marker is complete. Report completion does not implement a proposed type or fix.

This is report-only by default: write one report under `docs/audits/` through the project-approved non-serve CLI, cite Tower read-only, and create no cards, decisions, ballots, or implementation edits. An explicit owner request can change that boundary; follow the shared permission contract and keep implementation separate from the report.
