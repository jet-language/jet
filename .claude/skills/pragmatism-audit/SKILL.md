---
name: pragmatism-audit
description: >-
  Audit Jet for getting real work done across domains and workloads. Find places
  where the language forces ceremony instead of shipping the obvious default,
  while still preserving expert reject/override. Draft skill — use for
  pragmatism audits, “does this help me finish the job?”, and domain-workload
  friction reviews.
---

# Pragmatism Audit (draft)

Score Jet on **shipping useful work**, not elegance alone. Cover many domains
and workloads. Every finding asks: would a competent person trying to finish a
real job hit friction that Jet could have absorbed by default?

This is a **draft** skill. Prefer clear method and concrete findings over
framework polish. Improve the skill when a run exposes a missing lens.

## What this is (and is not)

| This audit | Not this |
| --- | --- |
| “Can I finish the job without fighting the language?” | Mission/philosophy scorecard alone (`mission-audit`) |
| Domain workloads: games, UI, science, CLI, net, embed, data, tooling | Surface uniformity cosmetics (`surface-audit`) |
| Beginner magic **with** expert reject + override | Peer leave/stay competition (`field-audit`) |
| Defaults that match the most likely use case | Spec text vs code only (`spec-compliance-audit`) |

Authority order: owner instruction → ratified Tower verdicts →
`docs/spec/philosophy.md` (two-facet design, jack-of-all-trades) →
`AGENTS.md` invariants (esp. I1, I7, I8) → this skill.

Code shows implementation state, not design law.

## Dual-facet pragmatism bar

Both required on every finding:

1. **Beginner / default path.** The most common useful behavior happens without
   ceremony. Footguns stay opt-in. Printing, derives, conversions, units,
   formats, and “obvious” APIs Just Work for the common case.
2. **Expert / control path.** The same mechanism lets experts **reject** the
   default, **override** it, or take a fully manual path. No second parallel
   mechanism (I8). No hidden rustc. No safety carve-out without `#Unsafe`.

Conflict rule: **getting the job done beats theoretical purity**. Ceremony
wins only when it buys safety, clarity, or expert control that cannot live
behind opt-in.

## Method

Search live specs, examples, stdlib, CLI, and package surfaces. Prefer
`scripts/agent/jet-env` and `rg` over memory. Invent fresh domain personas when
useful; do not recycle the same three toy apps every run.

1. **Pick domains.** Cover at least six distinct workloads in one run unless
   the owner names a narrower slice. Suggested pool: CLI tools, web/UI, games,
   scientific/numeric, networking, embedded/systems, data/serde, packaging/
   build, scripting/automation, text/parsing.
2. **Name a concrete job** per domain (one paragraph): what the person builds,
   what “done” looks like, which Jet surfaces they touch.
3. **Walk the happy path** with real examples or a minimal repro under
   `scripts/agent/jet-env`. Note every place they must write ceremony the
   compiler or stdlib already knows.
4. **Classify each friction** (see taxonomy below).
5. **Propose the smallest complete fix**: default magic → optional reject →
   optional override. Kill slices that break invariants, duplicate mechanisms,
   or hide expert control.
6. **Owner gates** (new syntax, new stdlib external dep, invariant carve-out,
   taste): ballot titles only unless asked to raise cards.

### Friction taxonomy

Use exactly these kinds:

| Kind | Meaning |
| --- | --- |
| `missing-default` | Compiler/stdlib already knows the answer; user must still opt in |
| `dead-end-magic` | Feature exists but fails at the last mile (e.g. units check, bare print) |
| `no-reject` | Default cannot be turned off for a type/package/project |
| `no-override` | Default cannot be replaced with a hand-written path |
| `wrong-default` | Default matches a rare case; common case pays tax |
| `domain-blind` | Surface ignores a whole workload’s obvious needs |
| `keep` | Default + reject + override already lined up — celebrate |

### Calibration examples (seed; find more each run)

These are known pragmatism pressures. Re-verify against the tree; do not treat
as settled law until ratified.

1. **Auto derives (S55 family).** Built-in traits often auto-derive when
   fields qualify, and hand impls can override. Ask: is the default “everything
   useful derives”? Can a user **reject** auto-derive for a type or package?
   Can they **override** selectively? Prefer default-on + reject + override
   over opt-in ceremony for the common case.
2. **Dimensional / unit printing** (`examples/features/types/dimensional_quantities.jet`).
   Algebra and dimension checks exist; `print(recovered)` still feels bare if
   units do not appear (`12 meter`, `4 meter/second`, `766 px`). Ask: does
   the last mile of the feature finish the job a scientist/UI author expects?

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never
hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id pragmatism-audit-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/pragmatism-audit-YYYY-MM-DD.md --file -` for the
same day only when the owner asks to revise that run. Never overwrite a
different day's note. Do not write reports under `docs/plans/`.

### Required report sections

```markdown
# Pragmatism audit — YYYY-MM-DD

## Thesis
One short paragraph: where Jet helps finish jobs vs where it stops short.

## Domain scorecard
| Domain / workload | Job | Grade (ships / friction / blocked) | Top friction kind | Evidence |

## Findings
Ranked. Each: kind, domain, evidence (file/example/command), beginner impact,
expert reject/override status, smallest fix, owner-gate? (yes/no + ballot title).

## Defaults map
Table of “most likely use case → today’s default → reject path → override path”.
Mark holes.

## Celebrated pragmatism
Defaults that already ship the job — preserve these.

## Next actions
Ballot titles or card ids only — do not create cards unless asked.
```

## Anti-goals

- Not “add every Rust derive marker”
- Not peer popularity or trust (`field-audit`)
- Not ontology unity for its own sake (`isomorphic-ontology-audit`)
- Not inventing parallel mechanisms for one semantic job (I8)
- Not lowering safety to look pragmatic (I1)

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
