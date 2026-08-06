---
name: first-principles-rethink
description: >-
  Completely rethink one Jet area from first principles: sweep the full corpus
  and its silhouette, find the one underlying idea that unifies the fragments,
  and deliver an exsum-led proposal plus a ballot slate that simplifies,
  streamlines, and powers up with no sacrifices. Use when the owner says
  "rethink X from first principles", "first principles proposal/audit for X",
  or "find the unifying idea behind X". Not a cosmetic audit — this produces a
  re-founding proposal and owner decisions.
---

# First-principles rethink

Rethink one area of Jet from the ground up. The bar is three reader reactions,
in this order: "duh, that makes perfect sense", "how did I not make that
connection", "ohhh — those are the same underlying thing". If the proposal
does not produce those, the synthesis is not done.

## Mission and non-negotiables

- **Unify, simplify, AND power up. Never trade one for another.** The output
  preserves every existing capability, expands where the model naturally
  allows, and deletes mechanisms rather than adding them. Planning and
  implementation cost are expendable; the outcome is not (philosophy: effort
  is never a deterrent).
- **The named topics are examples, never the boundary.** The owner's list
  seeds the search. Sweep every part of Jet that could conceivably relate,
  and the silhouette: what is hinted at but missing, what pattern implies an
  absent piece, what was declined, deferred, parked, or promised without code.
- **Support anything anyone would need in the domain** — from critical
  simulation to trivial one-liners. Test the model against the extremes, not
  the middle.
- **Beginner magic, expert control** — run both passes on every element:
  invisible correct defaults with zero ceremony; full nameable, reflectable,
  overridable control behind explicit opt-in.
- **Never silently contradict ratified law.** Every ratified decision the
  proposal touches is either respected or named as an explicit amendment in a
  ballot. Frozen walls (no top type, no HKT, no macros, comptime never
  creates types, I-invariants) stay unless a ballot explicitly reopens one.
  This is the most common blocking review finding — check it before review
  does.

## Phase 1 — research fan-out (parallel, read-only)

Launch parallel read-only research passes; each returns compact evidence with
file:line references, real syntax, and decision IDs — raw data over polish.
Cover at least:

1. **Current mechanisms** — spec, sema/compiler code, prelude, examples,
   tests for the area. How each mechanism is declared, checked, represented,
   erased or reified.
2. **Adjacent planes** — every feature that could conceivably relate, however
   distant. Look for the same job done twice under different names.
3. **Decision record** — ratified decisions (especially ratified-but-unbuilt:
   they are the landing zone and the "why now"), declined, deferred, frozen,
   superseded. Read the DECISIONS state before designing: ratified → respect
   or amend explicitly; open → this proposal's ballots decide.
4. **Silhouette** — prior audits in `docs/audits/`, open and frozen Tower
   cards, unpromoted ideas, phantom types (named in errors but unwritable),
   closed compiler tables, string-smuggled data, reserved-unimplemented
   names, spec promises with no code, drift between spec and code.
5. **Prior art** where it genuinely informs — what peer languages unified or
   failed to unify here (`lessons-learned`, `surface-research` style, scoped
   to the area).

Do not design during research. Do not skip the silhouette pass — the negative
space is where the unification usually hides.

## Phase 2 — synthesis (the actual first-principles work)

- **Name the one idea in one sentence.** If it takes a paragraph, keep
  digging.
- **Build the evidence table of shadow systems**: every mechanism found doing
  the same underlying job, with its home and its defect. This table is the
  proof the rethink is needed; it also becomes the migration checklist.
- **Apply the unification test**: every existing feature must fall out as an
  instance of the one idea; every known missing feature must become a new
  instance of the same mechanism, not a new mechanism. A feature that resists
  is either evidence the idea is wrong or a documented, justified wall.
- **Find the axes.** Fragmented designs usually collapse into a small grid of
  orthogonal axes (the number tower: value world × knowledge grade). Recur
  ring shapes deserve names (point/delta, exact/approximate/measured).
- **State the law.** The best unifications compress into one conservation-
  style law that all existing ratified rules turn out to be instances of.
  Ratified precedent expressed as theorems of the new model is the strongest
  argument the model is right.
- Keep the honest kill-check: if the unification hollows beginner defaults,
  needs an invariant carve-out, or duplicates a mechanism it claims to
  delete, kill or narrow that slice before it reaches the owner.

## Phase 3 — the proposal (`docs/proposals/<area>-<slug>.md`)

Owner doc style throughout: `simple`-skill prose, plain words, no repetition,
no theming. Structure, in order:

1. **Executive summary first.** A legible exsum that ties the whole thing
   together — the finding, the one idea, why now, the concrete payoffs, what
   the ballots ask, what does not change. The owner reads the body and the
   ballots with the full picture already in mind. This is mandatory; the
   owner has asked for it explicitly.
2. **Glossary** — define every term of art before first use.
3. **The one idea** — one sentence, then one paragraph, with the beginner
   story and the expert story.
4. **Evidence** — the shadow-systems table with file:line proof.
5. **The model** — axes, planes, laws, with the "ohhh" connections spelled
   out explicitly as their own list.
6. **Worked examples** — full real programs, not toys; drive the design from
   a complete example the way the owner reasons.
7. **What this unlocks** — domain by domain, extremes included.
8. **What does not change** — spellings kept, walls kept, zero-cost kept.
9. **Decisions for the owner** — a compact direction-level table mapping to
   the ballot slate; each ballot stands alone so any subset can be adopted.
10. **Implementation shape** — phased: (A) internal re-founding with no
    surface change and all tests green; (B) land ratified-but-unbuilt work on
    the new substrate so it is built once; (C) balloted surface unifications,
    each a coherent greenfield migration that deletes the replaced form.

## Phase 4 — Tower (ballots are part of the deliverable)

- One homed card (correct epoch) carrying the proposal ref and exit
  criteria: ballots ready; no ratified decision touched without an amendment
  note; ratified outcomes recorded in spec; implementation cards minted per
  outcome.
- Mint the full ballot slate per the `tower-ballot` skill — full profile,
  worked code per option, genuine alternatives, review passes. Ballot first;
  never ask permission to mint. Direction-level ballots (adopt-the-model,
  per-unification choices), not implementation minutiae.
- Ratified decisions the design amends are named inside the relevant ballot
  text, not only in the proposal.

## Phase 5 — review, fix, close

- Fresh-context review that assumes the work is wrong. It must specifically
  hunt: silent contradictions of ratified decisions, fabricated or misused
  decision IDs, wrong code-level claims, exsum/body/ballot inconsistencies,
  invariant violations, strawman ballot options, worked code that breaks
  ratified syntax.
- Fix every finding, re-verify materially fixed items, then verify criteria
  with builder ≠ verifier.
- Commit only owned paths (proposal, skill/docs touched, Tower store); never
  another task's files; commit messages without card refs (the githook
  rewrites `.tower` on `#N` mentions).
- Save a memory pointer so the next session knows the slate exists and what
  awaits the owner.

## Anti-patterns (each one has burned a run)

- Designing before the silhouette pass — the unification hides in what's
  missing, not only in what exists.
- Treating the owner's example list as the scope.
- A proposal body with no exsum, or an exsum written last as a summary
  instead of first as the frame.
- Silently overriding a ratified decision the researcher didn't surface
  (check overflow/conversion/identity semantics especially).
- Padding the ballot menu with derivative spellings of one idea instead of
  genuine alternatives.
- Claiming a diagnostic or mechanism "dies" without checking how load-bearing
  it is.
- New mechanisms. The score is mechanisms deleted, capabilities kept, and
  capabilities gained — in that order.
