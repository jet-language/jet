# Handoff: ratify answered Tower decisions + improve examples for the rest

You are picking up a Jet-language project-management task mid-flight. Read this whole
file, then **read the `tower-sweep` skill** (`.claude/skills/tower-sweep/SKILL.md`) — it
is the durable workflow contract. Also obey `CLAUDE.md` (invariants I1–I8) and the Nix
command environment (`nix develop -c cargo test`, etc.). **Use the `advisor` tool** before
substantive edits and before declaring done.

## The two jobs

1. **Ratify every answered decision** in `tools/Tower/docs/ballots/ballot-results.md`
   (the owner submits answers there live — RE-READ it fresh; he pushed more while this
   prompt was being written, and may push more).
2. **Improve the worked examples** on the decisions still *open* in
   `tools/Tower/docs/ballots/decision-ballots.md` (everything not yet in ballot-results),
   to the owner's bar: terse plain language, a vivid real user-story example per option
   (what's typed, seen, errored), cross-language note where it helps. Edit the cards in
   place; you are the single writer of that file.

## Current state (already done this session — do NOT redo)

- Board reconciled to reality; 14 plans written **and independently vetted** by a second
  agent; they live in `tools/Tower/docs/sidequests/`. Each carded item links to its plan.
- 19 decisions were queued into the ballot; the owner is deciding them live.
- PM docs were reorganized under `tools/Tower/docs/` (`proposals/ sidequests/ plans/
  ballots/`); durable spec stays in `docs/spec/`. Tower webapp got an in-app markdown
  viewer/editor.
- The c20/c25 range-pattern ownership conflict was reconciled (D-PATR owns range-pattern
  semantics + exhaustiveness at all positions; c25 owns only the arm sugar).

## Answered decisions to ratify (RE-READ ballot-results.md for the live list)

As of the last read, answered: S83=D, D-TOOL-SPLIT=A, D-PATW=D, D-PATR=A, D-PATO=B,
D-RANGE1=A, D-RANGE2=A, D-ERR-CONV=A, D-DIST1=C, D-DIST2=B, D-WHEN1=A, D-WHEN2=A,
D-NARG-DIAG=A, D-CLI1=A, D-L0201=A, D-DBG1=A, D-EVAL1=A.

**Two carry owner comments you MUST honor — do not ratify them as a plain option letter:**

- **D-ERR-CONV=A, comment:** *"We decided that for impl/derive/fn scopes defined outside a
  type we use the `~~` operator (S83=D). Is that the appropriate one here? If NO, ratify
  original A. If YES, ratify with `~~`."* → Reason it through: S83's `~~` is the **Type-name
  connector** that attaches an external definition to a type (`impl Point~~Drawable` =
  "impl Drawable for Point"). D-ERR-CONV option A is `impl IoError -> ConfigError { … }`,
  where `->` means **"this error converts to that error"** — a different construct (a
  conversion declaration, not attaching a member to a type). They don't collide, so `->`
  is almost certainly correct and `~~` does NOT belong here. Verify against
  `tools/Tower/docs/sidequests/typed-error-families.md` and `docs/spec/syntax-decisions.md`,
  then ratify A with `->`. If genuinely ambiguous, ratify A with `->` and leave a one-line
  note on the card/log explaining why (the comment says "if NO, proceed with A").

- **D-DIST2=B, comment:** *"I want units to be part of an extension of the stdlib, not the
  core lang."* → Ratify as: **units of measure are in scope (not deferred), but delivered as
  a stdlib extension layer, NOT core-language syntax.** Distinct types (D-DIST1=C,
  `UserId :: distinct Int`) ship in core; units ride on top via stdlib. Phrase the log
  entry to capture this, and update `tools/Tower/docs/sidequests/distinct-types-units.md`
  accordingly (it currently recommended deferring units — flip it to "stdlib extension").

## Still OPEN — improve their examples (job 2)

D-DIST3, D-CT-L2NAME, D-DEFER1, D-PRELUDE1 (plus anything in decision-ballots.md not yet
in ballot-results.md — verify live). Upgrade each option's worked example in place.

## How to ratify (mechanics already worked out — saves you the derivation)

Enforcement: `tests/decisions.rs` checks that any ID in `Source/Syntax.rs` is in the
`## Ratified` section of `docs/spec/syntax-decisions.md` and NOT in the open registry.
None of these decision IDs are in that test's hardcoded surface list, and their **features
are not built yet**, so:

1. **Do NOT add `Source/Syntax.rs` entries** (would half-wire unbuilt tokens and risk the
   test). Record them as **"Ratified, not yet implemented"** — exactly how D-ALLOC1 /
   D-JSON3 / S60 already sit. Token registration happens later, at implementation.
2. **Add a `## Decision log` row** (bottom table of `docs/spec/syntax-decisions.md`) for
   each decision: `| 2026-06-19 | <ID> | <chosen option, terse rationale>. **Ratified, not
   yet implemented** | owner |`. Match the existing row style.
3. **S83 specifically:** it currently sits in the `## Open decisions` "Registered for
   M3–M14" registry table (~line 1761) with a long explanatory note block — **remove S83
   from that registry + delete its open note block** (it's decided: connector = `~~`,
   double tilde, used Type-first for `fn Point~~name`, `impl Point~~Trait`,
   `derive Point~~Derive`). Also update the S56 decision-log line that says "syntax pending
   S83" → resolved to `~~`.
4. **Strip every ratified card** from `tools/Tower/docs/ballots/decision-ballots.md`
   (ratified decisions leave the queue — owner gets decision fatigue from decided clutter).
5. **Clear the ratified entries from `tools/Tower/docs/ballots/ballot-results.md`** — but
   RE-READ it first and preserve any entries you did NOT just ratify (the owner may have
   added more). If it ends empty, reset it to the header stub.
6. **Update board cards** (`tools/Tower/board.json` — LIVE, owner-edited; surgical edits or
   a targeted node load-mutate-save only, NEVER rewrite wholesale): cards whose decisions
   are now fully ratified move from `decisions` → `planned` (ready to implement), with a
   note naming the ratified option(s). Fully-decided carded items: c20 (D-PATW/R/O), c25
   (D-RANGE1/2), c22 (D-ERR-CONV), c23 (D-DIST1/2/3 — only once D-DIST3 is also answered),
   c24 (D-WHEN1/2), c60 (D-NARG-DIAG), c11 (D-CLI1), c12 (D-L0201), c52 (D-DBG1), c54
   (D-EVAL1), c61 (D-CT-L2NAME — once answered). Leave a card in `decisions` if any of its
   decisions is still open (e.g. c23 until D-DIST3 lands; c13 until D-PRELUDE1; c21 until
   D-DEFER1).

## Verify before declaring done

- `nix develop -c cargo test --test decisions` (ratification enforcement) MUST pass.
- `node tools/Tower/Tower.mjs status` — ratified cards should be gone from the ballot;
  board stages should reflect the moves; JSON still valid.
- `node --check tools/Tower/Tower.mjs`.

## Hard constraints

- **board.json is owner-owned & live-edited** — never clobber; surgical/targeted writes
  only; expect Edit-staleness and re-read.
- **decision-ballots.md: single writer.** If you parallelize example-improvement across
  subagents, give each a DISJOINT set of cards and run them SEQUENTIALLY (same file), or
  have them return text and merge yourself.
- **No git worktrees.** Don't touch `tools/Tower/docs/proposals/` (a separate agent owns
  it). Don't ratify/implement beyond what the owner answered.
- These are owner-facing syntax ratifications — measure twice. When in doubt on a nuance
  (esp. the D-ERR-CONV `~~` question), ratify the safe reading and leave a one-line note
  rather than guessing big.
