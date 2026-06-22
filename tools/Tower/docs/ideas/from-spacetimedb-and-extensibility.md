# Idea cards — mined from two research notes

Source files mined (read-only):
- `docs/jet-borrow-from-spacetimedb.md` ("Borrowing from SpacetimeDB → Jet")
- `docs/jet-library-extensibility.md` ("How Far Can a Library Bend a Language?")

Each card: what it is · where it came from · dedup status against ratified
decisions (`syntax-decisions.md`), the open ballot (`decision-ballots.md`), board
cards (`board.json`), proposals, and the idea-cards screening doc
(`tools/Tower/docs/idea-cards.md`) · one CEO note.

Status legend: `NEW` · `ALREADY A CARD/PLAN` · `ALREADY IN BALLOT` · `ALREADY
RATIFIED` · `ALREADY IMPLEMENTED` · `PARTIAL` (gap called out).

**Headline.** The SpacetimeDB note yields **two real decisions** and one
philosophy-validation (no card). The extensibility note is a landscape that
collapses to **one strategic decision**. Net new ballots queued: **3** — D-DET1
(checked determinism), D-TXN2 (irreversible-effect guard inside `#transact`),
D-EXT1 (library extensibility ceiling). Everything else dedups into existing work.

---

# A. From `jet-borrow-from-spacetimedb.md`

The note's own verdict table puts most of SpacetimeDB out of scope (it's a
database). Three language-level borrows survive; one is already-handled, two need
a call.

### A1. Checked determinism — `pure fn` guarantees *reproducibility*
**What it is.** Strengthen `pure` from "no side effects" to "same inputs ⇒ same
output": inside `pure`, reject wall-clock, OS randomness, fs, net, and calls to
non-`pure` fns. Make it *usable* by injecting deterministic `Clock`/`Rng`
capabilities (seeded RNG, fixed invocation timestamp), with an explicit
`assume_deterministic { … }` escape for the few edges the checker can't prove.
**Source.** §2 "Primary borrow — checked determinism."
**Status.** PARTIAL / NEW as a guarantee. `pure fn` is **ratified** (S60) but as
a *purity/effect* tag, and D-EFF1 (c66) generalizes it to an inferred effect set —
neither pins *determinism* as the contract (a fn can be effect-free yet read a
global clock through an injected param). The injected-`Clock`/`Rng` half is fork
**2.5** in `idea-cards.md` (Keep/Discard, un-carded), noted to ride D-SCAP1 +
D-EFF1. The `assume_deterministic { }` escape hatch is NEW. → **D-DET1**.
**CEO note.** The genuinely new question: does `pure` mean *no effects* (D-EFF1's
framing) or the stricter *reproducible*? SpacetimeDB's evidence is that
reproducible is what unlocks caching / parallelism / replay. Subsumes fork 2.5.

### A2. Atomic blocks (STM-lite) — all-or-nothing mutation
**What it is.** A block whose mutations fully revert if any step fails.
**Source.** §3 "Secondary borrow — atomic blocks."
**Status.** ALREADY IN BALLOT. This is **D-TXN1** (board card **c72**):
`#transact { }` auto-rollback over types implementing `Rollback`. Near-exact dup;
the note's "heavier lift, sequence after determinism" matches the existing card's
sequencing. No new card for the block itself.
**CEO note.** Covered by c72. Don't re-open the surface — `#transact` is ratified
(S82). Decide it on the D-TXN1 ballot.

### A3. Irreversible-effect guard inside `atomic`
**What it is.** Reject irreversible side effects (`send_email`, a network POST)
*inside* an atomic/transactional block — "you can't un-send an email on rollback"
— and tell the user to fire it after commit. (SpacetimeDB forbids I/O in reducers
for exactly this reason.)
**Source.** §3 "The subtle rule worth stealing" (diagnostic JET-ATOMIC-002).
**Status.** NEW. D-TXN1 covers *reversible* mutations via the `Rollback` trait but
says nothing about effects that *can't* be rolled back. This guard rides D-EFF1
(it needs to know which calls are irreversible effects). → **D-TXN2**.
**CEO note.** Small, high-value safety rule that closes the obvious footgun in
auto-rollback. Pairs with D-TXN1; gated on D-EFF1 for the effect classification.

### A4. Reducer vs. procedure split (guardrailed default + opt-in escape)
**What it is.** SpacetimeDB's reducers (deterministic, transactional) vs.
procedures (may do I/O if you manage the transaction). The note frames this as
*validation* of Jet's "magic default + explicit escape hatch," not a feature.
**Source.** §0 "philosophy validation (free souvenir)."
**Status.** NEW but **not a feature** — it's external evidence the dual-facet
model works at scale (philosophy.md). No card.
**CEO note.** Cite it in philosophy/marketing; nothing to build.

---

# B. From `jet-library-extensibility.md`

A landscape, not a feature list: how deep into the pipeline a library may reach.
The note's own closing line names the one actionable ballot.

### B1. The extensibility tier model + the "global footgun" line
**What it is.** Five tiers of library power — 0 vocabulary, 1 blessed protocols, 2
marked DSL blocks, 3 compile-time codegen, 4 sigils/keywords/grammar — and a rule
for where Jet draws the line: allow **local** footguns (scope = your program),
reject **global** ones (scope = the shared language + every tool). Two banked
principles: *mark library-introduced syntax* (visually distinct from core) and
*diagnostics are the real ceiling* (Jet may only expose depth at which it can
still emit a clean, attributed error).
**Source.** §2, §5 (tier table, local-vs-global footgun, two principles).
**Status.** NEW as an explicit policy. The invariants already imply the ceiling
(human-ratifies-syntax, front-end-owns-diagnostics, simplicity ratchet) but no
card *states* the tier model or the third-party-vs-stdlib rope split. → **D-EXT1**.
**CEO note.** Worth ratifying as a standing policy so every future "can a library
do X?" has a one-table answer instead of re-litigation. Tier 4 = never; the live
question is how much rope Tiers 2–3 get and whether it differs for stdlib.

### B2. Tier 1 — blessed protocols (the workhorse)
**What it is.** Core defines a *fixed* piece of syntax + a hook; a library fills
it without inventing grammar: `for x in coll` via an iterator trait, `coll[i]` via
an index trait, `5km` via a literal-suffix trait. The note calls this "the
highest-value, lowest-risk extensibility Jet can ship" and the obvious first
ballot: *which surface forms get a hook, and is the hook-set open to third parties
or stdlib-only?*
**Source.** §5 (Tier 1 = workhorse), §7 (the obvious first ballot).
**Status.** PARTIAL. Literal suffixes exist for units (`9.99.usd`, D-UNIT1);
iteration and composable Iterator are shipped (E2-M7); user-defined derives attach
via `~~` (S56/S83). But these landed piecemeal — there is no *ratified hook-set*
nor a rule on third-party openness. Folded into **D-EXT1** (the openness question
is the same call).
**CEO note.** Mostly already real in pieces; D-EXT1 just makes the set and the
openness policy explicit so third parties know what they may implement.

### B3. Tiers 2–4 concretely (DSL blocks, proc macros, reader macros)
**What it is.** Tier 2 marked DSL blocks (`sql!{ … }`); Tier 3 AST/proc macros
(`#derive`-style codegen); Tier 4 reader macros / mutable grammar (Lisp/Raku).
**Source.** §2 table, §6 illustrative Jet.
**Status.** PARTIAL / flagged. Tier 4 conflicts with a ratified invariant
(human-ratifies-all-syntax) → **reject even for experts** (the global-footgun
rule). Tier 3 overlaps comptime (S26 law: value-only, *no macros*) and user
derives (S56/S83) — Jet deliberately has no proc macros. Tier 2 has no card.
These are the *options* inside D-EXT1's ceiling decision, not separate cards.
**CEO note.** S26's "no macros" law already pushes the ceiling below Tier 3's
general form. D-EXT1 ratifies that line explicitly and decides Tier 2's fate.

---

## Summary

**Ideas extracted: 7** (A1–A4, B1–B3).

**Already covered:** A2 (D-TXN1/c72), A4 (philosophy validation — no build).

**New ballots queued: 3**
- **D-DET1** — checked determinism: does `pure` guarantee reproducibility, with
  injected `Clock`/`Rng` + `assume_deterministic` escape (subsumes fork 2.5)?
- **D-TXN2** — reject irreversible effects inside `#transact { }` (rides D-EFF1).
- **D-EXT1** — library extensibility ceiling (tier model + local/global footgun
  rule) and whether Tier-1 hooks are open to third parties or stdlib-only.

**Coverage:** every substantive language-level idea in both files is captured above
as a card, a dedup, or a flagged-conflict. The two source files are safe to delete
once D-DET1 / D-TXN2 / D-EXT1 are screened.
