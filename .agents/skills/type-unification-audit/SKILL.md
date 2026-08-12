---
name: type-unification-audit
description: >-
  Audit Jet's traits, tags, markers, and keyword constructs for things that are
  secretly types. Find phantom types, fact-mechanism fragmentation, unnameable
  handles, and closed compiler tables — then propose the honest type. Draft
  skill — use for "should X be a type?", marker/type disjointedness reviews,
  and meta-type forward-compatibility checks.
---

# Type-Unification Audit (draft)

Find every place the compiler reasons with a type it refuses to admit exists,
and every place one classification idea wears several spellings. The output is
a fix plan: each finding leads with WHAT to fix and HOW, then evidence. The
report supports fixes; it is not a conviction list.

This is a **draft** skill. Improve it when a run exposes a missing lens.

## What this is (and is not)

| This audit | Not this |
| --- | --- |
| Phantom types: names in errors/signatures users cannot write | Shape/uniformity cosmetics (`surface-audit`) |
| One behavior, many spellings (facts, labels, qualifiers) | Concept mapping alone (`isomorphic-ontology-audit`) |
| Closed compiler tables that block user domains | Domain workload friction (`pragmatism-audit`) |
| Keyword constructs vs their typed artifacts | Spec text vs code (`spec-compliance-audit`) |

Authority order: owner instruction → ratified Tower verdicts →
`docs/spec/philosophy.md` → `AGENTS.md` invariants (esp. I7, I8, S26,
D-EXT1) → this skill. Check ratification **dates** — a verdict ratified the
same day is still law.

## The boundary law (calibrates every "should X be a type?")

**A control construct is an expression wherever it produces a value, and its
runtime artifacts are types; the construct itself never is.** A reified
construct is a second lambda (I8). The type-shaped thing near a keyword is
its artifact: the handle, the yielded collection, the range, the stream.
Audit the artifacts, not the keywords.

## The standing lens (partial)

Apply the **probe the running binary** and **honesty rules** sections of
`.agents/skills/_shared/standing-lens.md`. Skip the four questions and the micro
sweep: this skill measures whether Jet's own facts are honestly typed, and a
competitive frame would distort it.

The probe rule sharpens the phantom-type census. A closed compiler table, a
marker with no reachable behaviour, and an unnameable handle all look identical
in source; only running the surface separates a fact the compiler enforces from
one it merely records. The review-pass traps below already say re-verify rather
than assume — that is this rule, learned the hard way once.

Where a phantom type costs an agent rather than a human, say so. An unnameable
handle is a repair-determinism problem as much as a modelling one: a fact that
cannot be named cannot be suggested in a fix.

## Method

Search live registries and law; probe the real compiler. Prefer
`scripts/agent/jet-env` and `rg` over memory.

1. **Inventory the kind zoo.** Every declaration mechanism that mints a
   type-like thing. For each: what it mints, nameable in type position?,
   reflectable via `TypeInfo`?, user-open?, owning decision ID, code path.
   Start from `crates/jet-foundation/src/Syntax*.rs`, `Policy.rs`
   (`APPLIED_RULES`), `AST/{types,items}.rs`, and the casing table.
2. **Census the phantom types.** Names that appear in rule signatures,
   diagnostics, or docs but resolve nowhere: rule argument types, sema-only
   handles, closed tables, undeclared leaf names.
3. **Probe live.** Write minimal `.jet` repros for every headline claim and
   run them (`scripts/agent/jet-env jet run …`). A claim without a probe or a
   file:line cite does not go in the report.
4. **Classify** each form with the taxonomy below.
5. **Propose the honest type.** Fix first, evidence second. Prefer reusing a
   ratified mechanism (enums, distincts, existing markers) over minting a new
   kind — a "unifying" new kind is usually the N+1th spelling (I8 trap).
6. **Review passes, all three, fresh context:** peer (assume every cite and
   quote is wrong; re-run probes), adversarial (attack against invariants,
   S26, D-EXT1, beginner bar, soundness), pay-up-front (where is the
   expensive path actually right, and where is it speculation?). Record the
   passes and resolutions in the report.
7. **Cards and ballots only when the owner asks.** Bugs → cards; any syntax,
   surface, API, or feature change → ballots (`tower-ballot` standard).

### Finding taxonomy

Use exactly these kinds:

| Kind | Meaning |
| --- | --- |
| `phantom-type` | Compiler/diagnostics name a type users cannot write |
| `missed-unification` | One erased-fact behavior, several declaration spellings |
| `inert-magic` | Surface looks checked but checks nothing |
| `closed-table` | Fixed compiler data blocks user instances of an open concept |
| `unaddressable` | Checked names with no namespace, completion, or reflection |
| `false-rhyme` | One name, unrelated mechanisms (or one mechanism, clashing names) |
| `keep` | Rightly not a type — declaration modifiers, tooling metadata, control transfer |

### Review-pass traps (learned 2026-07-28; re-verify, do not assume)

1. **Fabricated law.** Quote ratified decisions verbatim with line numbers;
   reference-file comments are non-normative. One misquote killed a flagship
   finding.
2. **S26's forever list.** "Comptime types" are rejected forever. Ordinary
   enums consumed at compile time need no new category — never spell a
   proposal "comptime-only types". Say "amends S26", never "clarifies", when
   you touch it.
3. **Same-day verdicts.** D-UNIFYLIT1=A was ratified hours before a draft
   proposed relitigating it. Check the ratification log's dates.
4. **Soundness pricing.** Jet v1 has no lifetimes. Any proposal that moves a
   handle across a frame boundary must state its capture/escape rule and be
   priced as a real mechanism (owned captures, second-class params).
5. **Compat grading.** Jet is greenfield: surface respells are near-free.
   Grade retrofit risk on *representation* (serialized data, snapshots) and
   *habit* (idioms accrete), not break-cost arithmetic.
6. **Duplicated authority.** Before typing a handle (e.g. `Capability<E>`),
   ask what it buys over the ambient mechanism (effect rows + grants). If the
   style it enables is foreclosed by v1's memory model, it is a post-v1 note.

## Output

One markdown report under `docs/audits/` via the Tower CLI:

```
node plugins/tower/tower.mjs docs add --section audits --id type-unification-audit-YYYY-MM-DD --title "…" --file -
```

Same-day revisions only via `docs update`. Never overwrite another day's
note. Write user-visible prose with the `simple` skill.

### Required report sections

```markdown
# Type-unification audit — YYYY-MM-DD

## Thesis
Where the shadow type system lives and the one-paragraph fix direction.

## The target shape
The cohesive end state the fixes serve (planes: runtime types, fact types,
handle types, one meta surface).

## Fix plan at a glance
| # | Fix | Vehicle (card / ballot / note) |

## The kind zoo
| Kind | Decision | Mints | Nameable | Reflectable | Open | Path |

## The phantom-type census
| Phantom | Where it lives | Who sees it |

## Scorecard
Owner lenses per family: clarity, functionality, magic, explicit control,
forward-compat.

## Findings
Ranked by gain-if-typed × difficulty-of-doing-it-right-later. Each leads with
**Fix**, then evidence (probe + file:line), gains, honest scope (which
decisions it amends), and vehicle.

## Forward-compatibility ledger
| Closed today | Open-later path | Retrofit class (representational / habit / additive) |

## Celebrated
Already type-shaped mechanisms to preserve and copy.

## Review passes
Peer, adversarial, pay-up-front: material findings and resolutions.

## Next actions
Card numbers and ballot IDs once created; otherwise titles only.
```

## Anti-goals

- Not reifying keywords (`loop`/`if`/`taskgroup` as values) — see the
  boundary law
- Not a new meta-type kind when a ratified mechanism can carry the facts (I8)
- Not value-dependent types (literal-only positions stay literal-only)
- Not relitigating ratified verdicts, including today's
- Not proposing checks that check nothing (a fact kind without a consumer is
  inert magic)

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
