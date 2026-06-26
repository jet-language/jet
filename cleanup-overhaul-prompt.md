# Implementing-agent prompt — Jet docs/files/syntax/tests cleanup & overhaul

You are working in the Jet repo (`/home/nate/Projects/Github/jet`). This is a
greenfield, pre-release compiled language. Your job is a structured cleanup &
consolidation pass. Read this whole prompt before acting.

## Ground rules (non-negotiable)

1. **Read first, in this order:** `CLAUDE.md`, then `docs/spec/philosophy.md`,
   `docs/spec/syntax-decisions.md`, `docs/spec/architecture.md`,
   `docs/spec/diagnostics.md`, `docs/spec/roadmap.md`. Obey invariants I1–I8.
2. **Command environment:** run every build/test/run through the Nix dev shell,
   one at a time, never in parallel:
   `nix develop -c cargo build`, `nix develop -c cargo test`,
   `nix develop -c jet run <file>`. The dev shell prints a banner to stdout —
   filter it when grepping/capturing. `nix develop -c` leaves ~197M
   `/tmp/nix-shell.*` dirs; `rm -rf /tmp/nix-shell.*` if `/tmp` fills, and check
   `df -h /tmp` before trusting any ENOSPC failure.
3. **Two action modes — respect them exactly:**
   - **EXECUTE** workstreams: make the change, keep the full suite green.
   - **PROPOSE / PLAN** workstreams: write a reviewed plan doc only. Do NOT move,
     rename, delete, or rewrite any file. The owner approves before a later pass
     touches anything.
   - When unsure which mode applies, it's PROPOSE.
4. **Do not touch:** `tools/Tower/board.json` (owner-owned, live-edited), any
   `Tower-v2/` files (separate rebuild), git branches/worktrees (work on current
   branch only), and never `git restore`/revert files you didn't create.
5. **Single source of truth.** The point of this whole task is to collapse
   scattered/duplicate/stale info into one canonical place per topic. Never
   create a second doc that overlaps an existing one; migrate unique content,
   then flag the old one for removal (don't delete in PROPOSE mode).
6. **Greenfield reality.** Pre-release, no users. Delete/flag anything written
   for an audience that doesn't exist ("alerting users about a new API",
   migration notes, deprecation hand-holding, changelog ceremony). Keep only
   what helps build the language now.
7. **Output style:** terse, plain, no LLM bloat. No "comprehensive/robust/
   seamless", no restated headings, no summary paragraphs that add nothing.
8. **Delegate.** This is large. Spawn sub-agents (haiku for grep/inventory/
   read-only; sonnet for writing; opus for the syntax-reconcile judgment calls).
   One layer deep, no nesting. Give each enough context to act standalone.
9. **Verify your own green.** Never trust a sub-agent's "tests pass" — re-run the
   relevant suite yourself. Use targeted `--test <name>` while iterating; full
   `nix develop -c cargo test` once at the end.

## Execution model — cheap, efficient, phased (non-negotiable)

Run this as **ONE delegating agent. Do NOT use a Workflow / ultracode / fan-out
review** — this is mostly sequential edits, so parallel fleets just multiply token
cost for no gain. Optimize for low burn:
- **Delegate by tier:** `haiku` for all read-only inventory/grep/map work; `sonnet`
  for writing docs/showcase/Ideas merge; `opus` ONLY for the 6 v2 decision cards
  (the judgment work). Never spawn an agent for a single shell command — run it.
- **Inventory once, reuse it.** The Phase 0 survey produces a file map; later phases
  read from that, not the whole tree again. Don't re-read large files you've seen.
- **Tests:** targeted `--test <name>` while iterating; the full
  `nix develop -c cargo test` exactly ONCE, at the end of Phase 1 and again before
  done. Test runs are cheap in tokens (just the pass/fail tail) but slow — don't
  loop them.
- Keep sub-agent prompts tight; pass only the paths + goal they need.

**Phases — stop and report after each; the owner says go before the next.** This is
a checkpointed handoff, not one unattended blast.

- **Phase 0 — Checkpoint + survey** (haiku). Confirm a clean git checkpoint exists.
  Inventory docs / examples / tests / v1 plans+ballots+Ideas / v2 `tower.json`. Output
  a short file map. No edits. → report.
- **Phase 1 — Syntax ground truth** (Workstream B). Build the compiling canon `.jet`
  + the aspirational reference; wire the golden test; run the full suite once. This
  establishes what syntax actually works, which C and E depend on. → report what
  compiled and what existing examples are stale.
- **Phase 2 — Content rescue** (Workstreams A + G). Merge v1 plans+ideas into Ideas.md;
  author the gate-cleared decisions into v2 via `store.mjs`. → report what landed in
  Ideas.md and which decisions hit the v2 queue.
- **Phase 3 — Proposals** (Workstreams C, D, E, F). All read-only/markdown; can run
  their sub-agents in parallel since they don't touch the same files. → report the
  four proposal docs + the exact move/delete list.
- **Phase 4 — Close.** Full `nix develop -c cargo test` green (re-run yourself), final
  summary: executed vs awaiting-approval, and the verbatim list of files/dirs proposed
  for move/delete.

## Context you need

- Forward planning lives in `tools/Tower/docs/plans/{epoch-3,epoch-4,epoch-5,
  jetpack-jetos}/`. Current epoch = Epoch 3 (`docs/spec/roadmap.md`).
- The master idea list is `tools/Tower/docs/proposals/Ideas.md` (sorted by
  implementation ease). It is the consolidation target.
- Ratified syntax record: `docs/spec/syntax-decisions.md` (~2970 lines, owner says
  bloated/outdated in places). Language-behavior-today: `docs/spec/spec.md`.
- Examples: `examples/` (216 `.jet` files; themed showcase set already in
  `examples/showcase/`). Tests: `tests/` (57 `.rs` files, 1000+ tests).
- Owner is CEO/CTO and works measure-twice-cut-once. He decides syntax; you never
  change owner-facing syntax or ratified records without approval.

## Tower v1 → v2 reality (READ before Workstreams A, D, G — non-negotiable)

**Tower v2 (`tools/Tower-v2/`) is now the canonical PM tool.** The owner already
ran a one-shot v1→v2 conversion. **That conversion was LOSSY** — `migrate.mjs` only
imported the "Open" ballot cards + ratified results + epochs. It did NOT carry:
- the 11 **deferred ballots** (D-PROP1, D-ROLE1, D-PROTO1, D-QUAL4, D-SERDE-ACCESS,
  D-REPLAY1, D-IFC1, D-REFINE1, D-BUDGET1, D-VERIFY1, D-REVERSE1) — they live in a
  separate "Deferred" section migrate skips,
- any `tools/Tower/docs/plans/*` forward-plan detail,
- `tools/Tower/docs/proposals/Ideas.md`.
All of that still exists ONLY in `tools/Tower/` (v1). **Do not delete `tools/Tower/`**
— it is the sole copy of the corpus this task consolidates. Treat v1 as a read-only
archive: consolidate FROM it, write the results to their new homes, never assume it's
already in v2.

Store facts:
- v2 = single `tools/Tower-v2/tower.json`, mutated via `tools/Tower-v2/app/store.mjs`
  functions: `load()` → `addCard` / `addDecision` / `addQuestion` / `promote` /
  `updateCard` → `save()`. There's also an `Add as card` path from the Ideas view.
- Re-running `migrate.mjs` is NOT a fix: it doesn't read the deferred/plans/ideas
  content, and it **replaces** cards/decisions — re-running would clobber anything
  authored in v2 since the conversion. Do not re-run it.

Owner's resolved routing for this task:
- **Ideas corpus → v1-style `Ideas.md` markdown stays the sorting surface.** The owner
  triages there and promotes winners into v2 himself later (Workstream A unchanged in
  destination). Because `tools/Tower/` is slated to retire once consolidated, flag
  `Ideas.md` in the Workstream D proposal as a PRESERVE/relocate item (move to a
  neutral durable path outside `tools/Tower/`).
- **Surfaced decisions → authored DIRECTLY into v2** (Workstream G), via `store.mjs`
  `addDecision`/`addCard` or the `Add as card` UI. NOT into v1 `decision-ballots.md`.

The v2-builder agent is **done** — v2 is stable and you are the only writer; no
coordination race to manage. Rules:
1. **Writing v2 data is allowed and expected** — but ONLY through `store.mjs`
   functions (`load`→mutate→`save`) or the UI. Never hand-edit `tower.json` raw, never
   edit v2 app code (`*.mjs`, `app/ui`). `store.mjs` uses `Date` (`today`/`now`); since
   you run as a single agent shelling out to node, that's fine — just never wrap these
   calls in a Workflow JS sandbox (Date throws there).
2. **Reconcile against v2's current state before surfacing any decision.** v1's
   deferred section is itself stale — e.g. **D-PUBLISH1 is already ratified in v2**
   (2026-06-25). Grep `tower.json` for each id first; skip anything already decided or
   already present there. Only surface what's genuinely still open.
3. **Workstream D must NOT propose restructuring `tools/Tower/` or `tools/Tower-v2/`.**
   v1 is a retiring archive (flag it for removal AFTER this task rescues its content);
   v2 is the live tool. List "retire `tools/Tower/` once consolidated" as one
   coordination item, not a move map.

---

## Workstream A — Merge forward plans into Ideas.md  [EXECUTE]

Goal: make `Ideas.md` the single source for designing epochs 3+.

- Read every `.md` under `tools/Tower/docs/plans/epoch-3/`, `epoch-4/`,
  `epoch-5/`, and `jetpack-jetos/`.
- For each, pull any unique detail/important info NOT already captured in
  `Ideas.md` and ADD it (additive only — do not replace, reword, or remove
  existing Ideas.md content).
- **No duplication:** before adding an item, check it isn't already represented
  (the list is already deduped; many plan topics already appear). If a plan adds
  depth to an existing Ideas.md bullet, append the detail to that bullet rather
  than creating a new one.
- Preserve traceability: note which plan file each added detail came from, so the
  owner can later confirm nothing was lost.
- The plan files themselves stay in place for now; in Workstream F, flag them as
  "superseded by Ideas.md — candidate for removal after owner confirms the merge."
- **`Ideas.md` is the owner's sorting surface and MUST survive v1 retirement.** It
  currently lives at `tools/Tower/docs/proposals/Ideas.md` inside the retiring v1
  tree. Keep editing it in place for this task, but flag it in the Workstream D
  proposal as a PRESERVE/relocate item (target: a neutral durable path outside
  `tools/Tower/`, e.g. `docs/planning/Ideas.md`). Do not let it be swept away with v1.

## Workstream B — Two syntax showcase files  [EXECUTE]

Goal: one honest place agents can read to see real syntax instead of guessing.

1. **`examples/showcase/_canon.jet`** (name it whatever fits the showcase
   convention) — the **compiling, golden-tested** showcase. Every line is
   ratified AND implemented syntax that actually compiles and runs through
   `jet`. Organize by feature area with brief comments. Wire it into the golden
   test harness (`tests/golden.rs` / `tests/showcase.rs` pattern) with pinned
   expected output. This file is the canonical "what works today" truth.
   - Use only the canonical syntax spellings (e.g. `T.{ }` / `.{ }` struct
     literals per D-DOTCTOR1/2; current `|`-vs-`||` switch rules; etc.). When
     existing examples disagree with the ratified record, the ratified record
     wins and you note the stale example in Workstream E.
   - Watch the golden "unsafe" substring trap: `golden.rs` fails any example
     whose generated Rust contains the bare word "unsafe". Reword comments.
   - If new syntax appears here that the formatter must round-trip, confirm
     `jet fmt` preserves it (formatter round-trip is required for syntax).
2. **`docs/reference/syntax-surface.jet`** (or a `.md` with `.jet` blocks if a
   non-compiling `.jet` would break tooling — your call, justify it) — the
   **aspirational reference**: the full ratified syntax surface INCLUDING
   ratified-but-not-yet-implemented features, each clearly marked
   `# RATIFIED, NOT YET IMPLEMENTED (<decision id>)`. This is for the owner to
   eyeball the whole surface and spot changes. It does NOT need to compile and is
   NOT golden-tested; mark it clearly as non-executable.
- Cross-reference: the canon file links to the reference and vice-versa, and both
  point at `syntax-decisions.md` as the decision/rationale record.

## Workstream C — Reconcile the syntax single-source  [PROPOSE]

`syntax-decisions.md` is bloated/outdated. Do NOT rewrite it. Instead produce
`tools/Tower/docs/proposals/syntax-decisions-reconcile.md` containing:
- A line-referenced list of entries that are stale, contradicted by the compiling
  canon file (Workstream B), already ratified-and-shipped (candidates to compress
  to a one-line record per the "ratified decisions leave the queue" norm), or
  duplicated elsewhere.
- A proposed slimmer structure: decisions + rationale only, with the canon `.jet`
  as the worked-example surface (so the doc stops re-explaining syntax that the
  file already shows).
- For each proposed cut/move: what it is, why, and where its unique content
  survives. Nothing user-facing changes until owner approves.

## Workstream D — Propose the project folder structure  [PROPOSE]

Write `tools/Tower/docs/proposals/folder-structure.md`:
- Inventory the current top-level + notable nested layout (`Source/` workspace
  crates, `docs/`, `examples/`, `tests/`, `tools/`, `stdlibs/`, `editors/`,
  `jet-jit/`, `jet-net/`, stray scratch files, `examples/workspace/.jet/`, etc.).
- **Out of scope:** do NOT propose any restructuring inside `tools/Tower/` or
  `tools/Tower-v2/` (v2 rebuild is in flight — see the coordination section). List
  "Tower v1/v2 consolidation" as one open coordination item for the owner instead.
- Propose a simpler, consistent target layout. Simple but not oversimplified —
  respect that this is a compiler + stdlib + package manager + editor tooling +
  PM tool monorepo.
- Give a current→target move map, rationale per move, and a risk note for each
  (build paths, `Cargo.toml`/workspace members, `include_str!` embeds, golden
  test paths, doc links). Call out anything that would need code/path edits.
- Flag clearly-stray/scratch files (e.g. `examples/workspace/.jet/` if it's a
  build artifact) for deletion or gitignore.
- EXECUTE nothing. This is a map for owner approval.

## Workstream E — Test/example trim plan  [PLAN]

Write `tools/Tower/docs/proposals/test-example-trim.md`. The owner wants to keep
the safety of the suite but cut fat that makes every change slow/wasteful to
rewrite. Do NOT delete anything.
- Inventory: which test files / example dirs cover which features. Identify
  redundant, superseded, or overlapping examples+tests (especially examples that
  get rewritten on every syntax change, and golden tests that duplicate coverage).
- Produce a categorized cut/consolidate list. For EACH proposed removal, prove
  coverage is preserved: name the test/example that still covers the feature
  after the cut. Anything whose coverage isn't preserved elsewhere stays.
- Recommend consolidation moves (e.g. fold N tiny per-feature examples into the
  canon showcase from Workstream B where that doesn't lose an assertion).
- Note expected suite-size / iteration-speed impact. Owner approves before cuts.

## Workstream F — Stale/scattered-doc cleanup proposal  [PROPOSE]

Write `tools/Tower/docs/proposals/doc-cleanup.md`:
- Catalogue docs/files that are outdated, duplicative, audience-for-nobody
  (pre-release "user alert"/migration/changelog cruft), or contradict the
  ratified record / the canon `.jet`. Search broadly (docs/, tools/, README,
  examples READMEs, stray `.md`).
- For each: recommend keep / merge-into-X / delete, with one-line reason and
  where unique content goes. Include the plan files superseded by the Ideas.md
  merge (Workstream A).
- EXECUTE nothing destructive. (You MAY fix obviously-broken internal doc links
  you encounter, since that's safe and additive — note any you change.)

## Workstream G — Surface buried deferred decisions INTO Tower v2  [EXECUTE the surfacing; owner still ratifies]

The v1→v2 conversion dropped 11 deferred ballots (see the "Tower v1 → v2 reality"
section). They live only in v1 `tools/Tower/docs/ballots/decision-ballots.md`
"Deferred ballots" section. Several are gated on decisions now ratified → decidable
today. Surface them as v2 decision cards so they hit the owner's Decisions queue. He
ratifies; you only surface, never decide.

This is a tower-sweep-style job — follow the v1 ballot **house rule** for card content
(it's the format v2's focus mode renders: Gist / Story / In the wild / Other
languages / options / recommendation), but the DESTINATION is v2, not v1 markdown.

1. **Read the 11 stubs from v1** (`decision-ballots.md`) — source only; never write
   back to it.
2. **Reconcile each against v2 first.** Grep `tools/Tower-v2/tower.json` for the id
   AND grep `docs/spec/syntax-decisions.md` for its gate. Skip any that are already
   decided/present in v2 (e.g. **D-PUBLISH1 is already ratified in v2** — do not
   re-surface). Confirmed-ratified gates as of 2026-06-26: D-EFF1, D-STATE1, D-LIN1,
   D-QUAL1, D-QUAL2, D-TAINT1 (A).
3. **Author the gate-cleared / no-gate items as v2 decisions** via
   `tools/Tower-v2/app/store.mjs` (`load()` → `addDecision`/`addCard` → `save()`),
   each with full house-rule content (sub-agent reviewed before it lands). Use the
   store API ONLY — never hand-edit `tower.json`, never edit v2 app code. Known-ready
   set (re-verify each before authoring; if a gate turns out unmet, leave deferred and
   say so):
   - **D-PROP1** — `#(no_net)` effect prohibitions (gate D-EFF1 ✓)
   - **D-ROLE1** — time-varying roles, typestate + time (gate D-STATE1 ✓)
   - **D-PROTO1** — typed client/server protocol + session-type gen (gates D-LIN1 ✓
     + D-STATE1 ✓)
   - **D-QUAL4** — plain marker spelling prefix `#Tag T` vs postfix `T #Tag` (no
     gate; rec: prefix)
   - **D-SERDE-ACCESS** — fluent accessor for dynamic `Json`/`DataTree` (no gate;
     rec: pattern-match floor + minimal fluent accessors)
   - **D-REPLAY1** — record/replay determinism (gate D-EFF1 ✓; runtime harness is a
     build dependency, but the decision surface is decidable)
4. **The genuinely-not-ready items** (D-IFC1 — owner-deferred post-Epoch-3; D-REFINE1
   / D-BUDGET1 / D-VERIFY1 / D-REVERSE1 — need an SMT/proof/cost layer, post-v1):
   capture them so they aren't lost when v1 retires, but do NOT author them as live
   decisions. Either add them as Frozen/deferred v2 cards with their gate noted, or
   list them in the doc-cleanup proposal (Workstream F) as "preserve when retiring
   v1." Your call; state which you did.
5. **Decide nothing.** You surface; the owner ratifies. Do not edit
   `syntax-decisions.md` to add a decision. Do not touch v1 `board.json`.

---

## Acceptance / done means

- `Ideas.md` contains all unique forward-plan detail, additive, de-duped, with
  source traceability; full suite still green.
- Two showcase files exist; the canon one compiles, runs, and is golden-tested
  green; the reference one covers the full ratified surface with pending items
  marked.
- Four proposal docs exist (syntax reconcile, folder structure, test/example
  trim, doc cleanup), each concrete and owner-approvable, executing nothing
  destructive.
- The gate-cleared deferred decisions are authored as v2 decisions in `tower.json`
  via `store.mjs` (house-rule content, sub-agent reviewed), reconciled against v2 so
  nothing already-decided is re-surfaced; the genuinely-not-ready ones are preserved
  (frozen v2 card or noted in doc-cleanup) so they survive v1 retirement; no decision
  was made for the owner; v1 `decision-ballots.md` was not written to.
- `nix develop -c cargo build` and `nix develop -c cargo test` pass; you re-ran
  them yourself (not via sub-agent report).
- A short final summary: what was executed, what awaits owner approval, and the
  exact list of files/dirs proposed for move/delete (so the owner can act fast).

## Open the door, don't decide for him

Where you hit a real fork (a syntax spelling that looks wrong, a structure choice
with genuine tradeoffs, a test cut that's borderline), surface it as an explicit
question in the relevant proposal doc rather than guessing. The owner is the only
allowed bottleneck — make his decisions cheap and well-framed, never pre-empt
them.
