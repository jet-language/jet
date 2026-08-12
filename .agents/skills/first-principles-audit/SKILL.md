---
name: first-principles-audit
description: >-
  Completely rethink one Jet area from first principles: sweep the full corpus
  and its silhouette, find the one underlying idea that unifies the fragments,
  and deliver an exsum-led proposal plus a ballot slate that simplifies,
  streamlines, and powers up with no sacrifices. Use when the owner says
  "first principles audit of X", "rethink X from first principles",
  "first principles proposal for X", or "find the unifying idea behind X".
  Not a cosmetic pass — this produces a re-founding proposal, a surface
  redesign, and owner decisions.
---

# First-principles audit

Rethink one area of Jet from the ground up. The bar is three reader reactions,
in this order: "duh, that makes perfect sense", "how did I not make that
connection", "ohhh — those are the same underlying thing". If the proposal
does not produce those, the synthesis is not done.

## Mission and non-negotiables

Apply `.agents/skills/_shared/standing-lens.md` in full alongside everything
below: the four questions, the five agent-optimality quantities, the micro
sweep, probe the running binary, and the honesty rules. The owner never has to
ask for any of it. Where the lens and this skill overlap, this skill is the
more demanding of the two and wins.

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
- **Beginner magic, expert control is the governing philosophy, and the
  proposal must show it, not claim it.** This is the owner's stated test —
  "beginner magic out of the box and full expert control" — and it is the most
  common thing a rethink under-serves. Run both passes on every element:
  invisible correct defaults with zero ceremony; full nameable, reflectable,
  overridable control behind explicit opt-in. Then prove it with a **mandatory
  ladder section** (Phase 3 item 7): every rung from "the user types nothing"
  to "the user controls the authority", each rung opt-in, each shown as real
  code, and an explicit statement that no upper rung changes what the lowest
  rung does. Two failure modes to check by name:
  - **Ceremony creep** — a default that now needs a word it did not need
    before. If the common case gained a marker, the rethink went backwards.
  - **Magic without an exit** — a default with no way to refuse it, no way to
    see what it did, and no project-level switch. Every default owes the user
    all three.
  Default to opt-out for anything the compiler can do correctly on its own.
  Reserve opt-in for what the compiler genuinely cannot decide.
- **The surface is the product.** A rethink that only re-founds internals is
  a failed rethink — the owner has rejected one for exactly this. The main
  deliverable is greatly improved syntax, APIs, and structure: simpler to
  use, easier to read, easier to write, easier to reason about. Every
  unification must show up on the page as a before/after pair where the
  after is visibly better. If the user-facing code looks the same as today,
  the rethink is not done.
- **Greenfield: breaking changes are welcome.** Shipped spellings and APIs
  earn their place or they are replaced — never keep one because it is
  shipped. Judge only the best final design; migration churn is not a
  product tradeoff. Ratified decisions still need a named amendment in a
  ballot, but the ballot should propose the break whenever the break is
  better.
- **Write everything with the `simple` skill — and simple never means stuffy.**
  Short sentences, common words, one idea per sentence, no dense jargon walls.
  The owner has rejected LLM-coded dense prose AND stodgy report-speak — both
  read as "absolute shit" to him. Kill phrases like "it should be noted",
  "serves to", "is responsible for", "in order to", "the mechanism by which".
  Write like a sharp engineer talking: direct, concrete, alive. If a sentence
  would sound pompous said out loud, rewrite it. Load `simple` before writing
  any owner-facing text and follow it in every paragraph.
- **Show, don't describe. The owner decides from visuals.** His words: "the
  visuals ARE ALWAYS the most useful for me. I need to be able to visualize."
  Prose exists only to prep and clarify what he is about to see; the payload
  is what the code looks like and how the thing is structured, shown on the
  page. Every claim gets a code block, a before/after pair, a tree, or a
  table. A section that is all prose and no picture is a defect. Tables that
  line up the divergent forms side by side (the "five coats of one law" kind)
  are explicitly what he wants more of.
- **Format for the Jet doc viewer: never hard-wrap prose.** The owner reads
  proposals in a viewer that renders every newline, so hard-wrapped lines
  produce broken, ragged paragraphs — a recurring complaint. One paragraph =
  one long line; blank line between paragraphs; wrap nothing at a column
  count. Line breaks belong only in code fences, tables, and lists. This
  applies to every owner-facing markdown doc the audit produces.
- **A syntax-area audit rethinks the syntax space, not just the inventory.**
  Greenfield means the unclaimed lexical space is part of the sweep: prefix
  and suffix conventions (`_name`, `__internal`, dunder/sunder shapes),
  reserved namespaces for compiler internals, sigils, casing — what each
  could mean, what peer languages did with them, and either a proposed use
  or a stated reservation. Consolidating existing spellings alone is an
  incomplete run; the owner has rejected one for exactly this.
- **Every magic default owes the expert an audit trail and an explicit form.**
  Whenever the proposal makes something automatic (imports, discovery,
  wiring), it must answer in-line: how does an expert SEE what the magic
  resolved (a real command, real output), how do they write it explicitly
  when they need control (real syntax, e.g. a relative-path form), and how
  do they refuse the magic project-wide. Magic with no ledger and no manual
  spelling is a design hole, not a convenience.
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
- **Run the agent pass.** Beginner and expert are two of three readers; the
  third drives the surface unattended. Test the one idea against the five
  agent-optimality quantities in `.agents/skills/_shared/standing-lens.md`:
  does the checker catch a misuse rather than production, does a verdict
  arrive fast enough to sit in a loop, can the report be acted on without
  guessing, what does the surface cost in tokens, and does a mistake admit one
  obvious repair or several. A unification that collapses several mechanisms
  into one usually improves repair determinism outright — say so, because that
  is a real argument for the model and it is routinely left unstated.
- Keep the honest kill-check: if the unification hollows beginner defaults,
  needs an invariant carve-out, duplicates a mechanism it claims to delete, or
  makes a mistake harder for a machine to repair, kill or narrow that slice
  before it reaches the owner.

## Phase 3 — the proposal (`docs/proposals/<area>-<slug>.md`)

Owner doc style throughout: `simple`-skill prose that is direct and alive,
plain words, no repetition, no theming, no hard-wrapped lines (one paragraph
per line — the doc viewer renders every newline). The shape below is the
owner's stated preference verbatim: exsum, then a brief look at the issues
carried by a side-by-side table, then the proposal itself inline with worked
examples climbing the rungs, closing on the full final vision shown
visually. Prose preps; visuals decide. Structure, in order:

1. **Executive summary first.** A legible, cleanly formatted exsum that ties
   the whole thing together — the finding, the one idea, why now, the
   concrete payoffs, what the ballots ask, what does not change. The owner
   reads everything after it with the full picture in mind. Mandatory.
2. **The problem, briefly.** A short prose setup and then the side-by-side
   evidence table — every divergent form of the same underlying thing, its
   home (file:line), and its defect. The table carries this section; keep
   the prose around it tight. Fold any needed glossary into a few lines
   here; define terms at first use.
3. **The proposal, inline and example-led.** The heart of the doc. Work
   through the redesign element by element, and for each one show it:
   before/after pairs from real programs, each pair introduced by one or two
   plain sentences saying what to look at. Climb the rungs inside each
   element — beginner (types nothing), intermediate, expert (full control) —
   as real code, with the rule that no upper rung changes what the lowest
   rung does. For every magic default, show the expert's three exits
   in-line: the command that reveals what it did, the explicit spelling
   that replaces it, the project switch that refuses it. Mark every item
   ratified, amended, or new. Axes, planes, and the law live here too,
   stated where the examples make them obvious — the "ohhh" connections
   spelled out right where the reader can see them on the page.
4. **The final vision.** Mandatory closing spread, maximally visual:
   complete example programs in real syntax spanning beginner default to
   expert extreme, today's code next to the proposed code for the same job,
   and the structure of the end state shown as a tree or layout diagram
   (module tree, file layout, registry shape — whatever the area's
   structure is). This is the section the owner decides from; if it were
   the only section he read, the proposal should still land. Mark every
   not-yet-ratified line "proposed" — the review pass checks this.
5. **What this unlocks** — domain by domain, extremes included; brief.
6. **What stays** — only things that earn their place: walls kept on
   purpose, zero-cost kept, spellings kept because they win on merit.
   Never "kept because shipped".
7. **Decisions for the owner** — a compact direction-level table mapping to
   the ballot slate; each ballot stands alone so any subset can be adopted.
8. **Implementation shape** — phased: (A) internal re-founding with no
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
  hunt: a missing or hand-waved magic-and-control ladder, ceremony added to a
  common case, a new default with no refusal / no way to see it / no project
  switch; silent contradictions of ratified decisions, fabricated or misused
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
- Shipping an internals-only rethink. The owner rejected one whole run
  because the surface, syntax, and APIs did not improve. Machinery
  unification is the means; better code on the page is the end.
- Inventing a parallel concept where the type system already has one.
  Check the live type-system proposals (and results/optionals, effects,
  units) before adding any new enum, handle, or wrapper — a task failure
  should ride the same rails as every other failure.
- Dense prose. If a paragraph needs two reads, rewrite it with `simple`.
- Stuffy report-speak. `simple` compliance does not excuse writing that
  sounds like a committee. The owner called one run's prose "terrible &
  stodgy & stuffy" — direct and alive, or rewrite.
- Hard-wrapped prose lines. They render as broken ragged paragraphs in the
  owner's doc viewer. One paragraph per line, always.
- A prose-heavy section with no code block, table, or diagram. The owner
  decides from visuals; describing a design without showing it fails him.
- A syntax audit that only consolidates existing spellings and never
  explores the open lexical space (underscore conventions, reserved
  namespaces, sigils). The owner rejected a run as incomplete for this.
- Magic with no expert exit shown in-line: no command to see what it
  resolved, no explicit spelling to replace it, no switch to refuse it.
  "Beginners get magic" without "experts get the ledger" is half a design.
