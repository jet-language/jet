---
name: elegance-audit
description: >-
  Audit Jet's user-facing surface for elegance: shrink its area and find the
  laws and rhymes that make it click. Use for elegance, beauty, grouping,
  numbering, or cross-layer consistency reviews of syntax, concepts,
  diagnostics, Core API, or CLI.
---

# Elegance audit

Make Jet smaller and more beautiful at the same time. Every layer a user touches should hold as few parts and exceptions as its features allow, and should obey a law simple enough that a newcomer, once told, can predict what they have not yet seen. The target reaction is "oh, of course — and that means X must work like this too."

This is a frontend and structural audit of what users type, read, and recognize. Backend code quality, repository layout, and dead machinery belong to other methods; record such an opportunity in one line and move on.

Before running, read [`_shared/audit-dispositions.md`](../_shared/audit-dispositions.md) and [`_shared/standing-lens.md`](../_shared/standing-lens.md). They own permissions, publication, finding dispositions, and evidence rules. This method owns the census, the owner's dive choice, the dive, and the report.

## Leading words

- **Area** — everything a user must hold to use a layer: its **parts** (each distinct keyword, sigil, operator, concept, code shape, code group, command, flag, module, or type family) plus its **exceptions** (each irregular spelling, special case, one-off shape, gap in a numbering law, member that breaks its group's law, second spelling of one meaning, or "except when…" rule). Count from the layer's executable home, never from prose.
- **Click** — a law stated in one sentence that makes many forms predictable. Python's "everything is an object", Unix's "everything is a file", HTTP's "the first digit is the class" (1xx info, 2xx success, 3xx redirect, 4xx your fault, 5xx our fault), Go's "capitalized means exported", and the periodic table's "the row and column predict the properties" are calibration clicks. The ontology's [classic isomorphisms](../isomorphic-ontology-audit/ontology.md) calibrate semantic clicks. A click must explain real forms; a pleasing pattern that predicts nothing is decoration.
- **Rhyme** — a parallel structure within one layer or across layers, such as diagnostic groups that mirror the checking stages, or CLI verbs that mirror language keywords. A **near rhyme** is almost parallel and names what breaks it. A **false rhyme** looks parallel but means something different, and it costs more than no rhyme.

## Verdicts

Every finding carries two separate verdicts. Keep them apart and let the owner weigh them.

| Verdict | Evidence |
| --- | --- |
| Area | Before and after counts of parts and exceptions from the ledger, and the net change. |
| Click | The one-sentence law, its prediction-test score, and a before/after showcase with the sentence the reader should think. |

Elegance is a lens that serves Jet's priorities. It never overrides them. Kill or narrow any slice that weakens memory or type safety, beginner experience, or runtime performance; drops a capability; adds a parallel mechanism (I8); or breaks an invariant. State what survives the kill-check.

Stable identities are part of the surface. Diagnostic codes, command names, and ratified spellings may be renumbered, regrouped, or renamed as one greenfield cutover. The proposal names every rule it amends, such as the no-renumbering rule in `Docs/spec/diagnostics.md`, and lists the migration surface instead of doing it.

## Layers

| Layer | Executable home |
| --- | --- |
| Syntax and lexical space | `crates/jet-foundation/src/Syntax.rs` and `Syntax/` |
| Concepts and semantics | Sema, the embedded Prelude, `Examples/`, and ratified decisions |
| Diagnostics | `crates/jet-codegen/src/Prelude/Diagnostics.jet` |
| Core API | `Core/` |
| CLI and tooling surface | `crates/jet-cli/src/CLI.rs` (`COMMANDS`), flags, output formats, and manifest keys |

Other methods are rough references, not chained workflows. Use [`surface-audit`](../surface-audit/SKILL.md) outlier kinds, the [`isomorphic-ontology-audit`](../isomorphic-ontology-audit/SKILL.md) ontology and false-rhyme discipline, and [`structure-cleanup`](../structure-cleanup/SKILL.md) grouping and cohesion sense wherever they sharpen a count or a law. A passive read of them is not a second audit.

## Route

| Phase | Read |
| --- | --- |
| 1. Census every layer, then ask the owner to choose the dive | [`references/census.md`](references/census.md) |
| 2. Dive into the chosen target | [`references/dive.md`](references/dive.md) |
| 3. Write, review, and publish the report | [`references/report.md`](references/report.md) |

Load only the current phase. Independent read-only helpers may count or search a layer and return evidence to this audit. They cannot start another audit or widen the scope.

## Completion and permissions

The run is complete when all five layers have a ledger row with counts and laws or an honest `unknown`; the rhyme map covers every layer pair; the owner chose a dive target or declined one; the dive, if any, has both verdicts, a prediction-test score, a kill-check, and named amendments; the fresh review is fixed; and the report is published with its disposition marker and read back.

This method is report-only. It cites Tower read-only, lists proposed ballot titles, and creates no cards, decisions, ballots, or implementation edits unless the owner explicitly changes that boundary. Report completion is not implementation.
