---
name: isomorphic-ontology-audit
description: >-
  Audit Jet syntax against a language-agnostic foundational ontology. Map every
  surface form to what it fundamentally is; find missed isomorphisms, false
  rhymes, and clarity/ceremony failures. Use for isomorphic ontology audits,
  concept-unity reviews, or “ohhh” consistency checks — not surface-audit
  cosmetics.
---

# Isomorphic Ontology Audit

Map Jet’s user-facing surface onto fundamental programming concepts. Optimize
for **clarity first**, then **legitimate conceptual reuse**, then density.
Do **not** run a surface-audit (shape/outlier cosmetics). Do **not** chain
other audit skills unless the owner asks.

## Mission

Success looks like:

> Reader sees A and B share form → realizes A is subset / same / dual of B →
> “ohhh, of course.”

Failure modes to reject:

- Cosmetic rhyme with no shared ontology (**false rhyme**)
- Shared ontology with unrelated spelling (**missed isomorphism**)
- Ceremony that exists only because two concepts were split wrongly
- Golf that hurts scanability
- Consistency that obscures what a form *is*

## Dual-facet bar (both required)

1. **Exploratory / analysis (Python bar).** Typical Python scripts and
   data/exploration programs should be close to as short in Jet — or shorter —
   via simpler surface + stronger builtins/stdlib. Clarity still wins over
   cryptic density.
2. **Systems / safety (Zig–Rust–Odin–C–C++ bar).** Fully explicit, safe,
   low-level, and systems-grade programs must remain expressible without a
   second language or hidden second mechanism. Expert power is opt-in, not
   deleted.

Conflict rule: **clarity beats mere consistency**. Consistency wins only when
it teaches ontology. Compression wins only when it preserves or improves
clarity.

Authority: `docs/spec/philosophy.md`, `AGENTS.md` invariants (esp. I1, I7, I8),
ratified `docs/spec/syntax-decisions.md`. Code shows implementation state, not
design law.

## Foundation

Read and use as the closed category catalog:

- [`ontology.md`](ontology.md) — language-agnostic primitives, axes, calibration
  isomorphisms, extension protocol

Do not invent a parallel taxonomy. Extend `ontology.md` only when a concept
cannot land in an existing family (follow its extension protocol).

## Method

Search live specs, examples, stdlib, Syntax registry, and CLI surfaces. Prefer
`scripts/agent/jet-env` and `rg` over memory.

1. **Inventory Jet forms.** Keywords, sigils, declaration shapes, expression
   forms, patterns, type syntax, attributes, module/import forms, expert
   escapes. Cite files.
2. **Classify each form** into ontology ids from `ontology.md` (primary family
   + orthogonal X-axes). One sentence: *what is this?*
3. **Cluster by ontology**, not by glyph. Build concept families.
4. **Score each family** with the lenses in `ontology.md` §16:
   clarity, isomorphism, exploratory density, systems expressiveness,
   ceremony tax, tiering.
5. **Emit findings** only as:
   - **Missed unification** — same ontology, divergent spelling
   - **False rhyme** — similar spelling, different ontology
   - **Clarity failure** — form does not teach what it is
   - **Ceremony without teaching** — tokens that buy neither safety nor clarity
   - **Facet failure** — loses Python-density bar or systems-expressiveness bar
   - **Keep / celebrate** — isomorphism that already creates the “ohhh”
6. **Recommend** the smallest spelling/semantics move that creates the “ohhh”
   (or explicit “leave alone”). No stubs. No parallel mechanisms (I8). Owner
   gates (new syntax, etc.) → ballot titles only unless asked to raise them.

Calibration examples (do not merely restate; find more):

- Named function ≈ named binding of a function value
- Lambda ≈ same function, anonymous (X01)
- Method ≈ function + receiver (+ dispatch rule)

## Output artifact

One markdown report under `docs/audits/` via Tower CLI (never hand-edit board
JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id isomorphic-ontology-audit-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/isomorphic-ontology-audit-YYYY-MM-DD.md --file -`
for the same day only when the owner asks to revise that run. Never overwrite
a different day's note. Do not write under `docs/plans/`.

### Required report sections

```markdown
# Isomorphic ontology audit — YYYY-MM-DD

## Thesis
One paragraph: what Jet currently teaches about its own ontology.

## Dual-facet scorecard
| Lens | Grade (aligned/drift/unknown) | Evidence |
| Exploratory density vs Python | … | … |
| Systems expressiveness vs Zig/Rust/Odin/C/C++ | … | … |
| Clarity | … | … |
| Isomorphic consistency | … | … |

## Concept map (Jet → ontology)
Table: Jet surface form → ontology id(s) → X-axes → one-line “what it is” →
status (teaches well / partial / broken / false rhyme / absent).

Cover every inventoried form. Group rows by ontology family.

## Concept families
For each non-trivial family:
- Members
- Shared ontology (one sentence)
- Spellings today
- Isomorphism / clarity score
- Exploratory + systems impact
- Smallest “ohhh” move (or leave alone)

## Findings
Ranked. Each finding: kind, evidence, ontology ids, dual-facet impact,
recommendation, owner-gate? (yes/no + ballot title if yes).

## Celebrated isomorphisms
What already works — preserve these.

## Ontology gaps / extensions
Concepts in ontology.md with no Jet landing (absent vs deferred vs
deliberately out of scope). Any ontology.md extensions made this run.

## Next actions
Ballot titles or card ids only — do not create cards unless asked.
```

## Anti-goals

- Not surface-audit (uniformity/outlier cosmetics without ontology)
- Not field-audit (peer leave/stay competition)
- Not mission-audit (philosophy scorecard alone)
- Not “make everything look the same”
- Not proposing a second mechanism for one semantic job

Follow `AGENTS.md`. Pick this skill alone.
