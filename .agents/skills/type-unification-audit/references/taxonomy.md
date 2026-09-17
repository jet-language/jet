# Type-unification census and taxonomy

## What belongs in this audit

| This audit | Not this |
| --- | --- |
| Phantom types: names in errors or signatures users cannot write | Shape or uniformity cosmetics (`surface-audit`) |
| One behavior with many spellings: facts, labels, or qualifiers | Concept mapping alone (`isomorphic-ontology-audit`) |
| Closed compiler tables that block user domains | Domain workload friction (`pragmatism-audit`) |
| Keyword constructs versus their typed artifacts | Spec text versus code (`spec-compliance-audit`) |

The target closure is set at activation. Do not widen it into a census of all Jet types.

## Census

Inventory every relevant declaration mechanism that mints a type-like artifact. For each row record what it mints, whether it is nameable in type position, whether `TypeInfo` can reflect it, whether it is user-open, its owning decision ID, and its code path. Start from `crates/jet-foundation/src/Syntax*.rs`, `Policy.rs` (`APPLIED_RULES`), `AST/{types,items}.rs`, and the casing table.

Census names that appear in rule signatures, diagnostics, or docs but resolve nowhere: rule argument types, sema-only handles, closed tables, and undeclared leaf names. For every headline claim, write a minimal `.jet` repro and run it with `scripts/agent/jet-env jet run …`. A claim without a live probe or `file:line` cite does not enter the report.

Classify each form with exactly one kind below. Lead each finding with **Fix**, then give evidence, gains, honest scope, and vehicle. Prefer a ratified mechanism such as enums, distincts, or existing markers over a new kind. A new “unifying” kind is usually the N+1th spelling and violates I8.

## Finding taxonomy

| Kind | Meaning |
| --- | --- |
| `phantom-type` | The compiler or diagnostics name a type users cannot write. |
| `missed-unification` | One erased-fact behavior has several declaration spellings. |
| `inert-magic` | The surface looks checked but checks nothing. |
| `closed-table` | Fixed compiler data blocks user instances of an open concept. |
| `unaddressable` | Checked names have no namespace, completion, or reflection. |
| `false-rhyme` | One name hides unrelated mechanisms, or one mechanism has clashing names. |
| `keep` | A thing is rightly not a type, such as declaration modifiers, tooling metadata, or control transfer. |

## Required report shape

```markdown
# Type-unification audit — YYYY-MM-DD

## Thesis
Where the shadow type system lives and the one-paragraph fix direction.

## The target shape
The cohesive end state the fixes serve: runtime types, fact types, handle types, and one meta surface.

## Fix plan at a glance
| # | Fix | Vehicle (card / ballot / note) |

## The kind zoo
| Kind | Decision | Mints | Nameable | Reflectable | Open | Path |

## The phantom-type census
| Phantom | Where it lives | Who sees it |

## Scorecard
Owner lenses per family: clarity, functionality, magic, explicit control, forward-compat.

## Findings
Ranked by gain-if-typed × difficulty-of-doing-it-right-later. Each leads with Fix, then evidence (probe + file:line), gains, honest scope (which decisions it amends), and vehicle.

## Forward-compatibility ledger
| Closed today | Open-later path | Retrofit class (representational / habit / additive) |

## Celebrated
Already type-shaped mechanisms to preserve and copy.

## Review passes
Peer, adversarial, pay-up-front: material findings and resolutions.

## Next actions
Card numbers and ballot IDs once created; otherwise titles only.

## Finding dispositions
<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
<!-- /audit-dispositions -->
```

Report-only runs list next actions but do not create them. Report completion never implements the proposed type or fix.
