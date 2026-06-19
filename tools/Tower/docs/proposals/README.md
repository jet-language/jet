# Proposals

Idea-stage feature proposals distilled from the owner's scratchpad. Each is a
**digestible report, not a plan** — it lays out the idea, worked examples,
tradeoffs, an implementation sketch, and the open decisions the owner would
need to make. Each report ends at the decision seam: its "open decisions" are
the future ballot rows. Approve a direction and a report converts cleanly into
a sidequest plan.

| # | Proposal | What it covers | One-line read |
|---|---|---|---|
| **P1** | [Qualifier system](P1-qualifier-system.md) | trait vs attribute vs **tag** boundary; maturity tags; capabilities & prohibitions; uncertainty & cost dimensions | Ratify the taxonomy first — it's cheap and unblocks every "is this a trait or an attribute?" question. |
| **P2** | [Content-addressed definitions](P2-content-addressed-definitions.md) | Unison-style identity = content hash; free renames; no merge conflicts; dependency-hell relief | Adopt as an invisible build cache now; the visible name/version model is a gated, larger design. |
| **P3** | [Reactive / dataflow](P3-reactive-dataflow.md) | spreadsheet-style auto-recalculation | Take the derived dataflow *graph* as tooling; reject reactivity as the evaluation model. |
| **P4** | [Sigils & fan-out](P4-sigils-and-fanout.md) | reference sigil `@` vs `&`; namespace fan-out `s.{…}` | Two sugar notes with real sigil-budget / one-operator-two-axes conflicts to weigh. |

**The linchpin is P1 §1** — the trait/attribute/tag decision rule. P1's other
sections and the value dimensions all classify themselves against it, so it is
the one piece worth ratifying on its own.

Already-decided scratchpad items, for the record (not re-proposed):

- `@`→`#` **attribute** sigil — ratified (D-ATTR1, board card c01). Distinct
  from P4's *reference* sigil note.
