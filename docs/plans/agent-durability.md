# Agent durability — make the project frontier-model-independent

Audit date: 2026-07-10. Premise: the strongest model is leaving; mid-tier
models and the owner must be able to run this project without losing quality.
Five audit sweeps (CI/tests, docs, compiler code, Tower process, dev tooling)
found the same root cause everywhere: **quality currently depends on agent
judgment and memorized traps, not on machine enforcement.** A frontier model
papers over that; a weaker one won't.

One principle drives every item below: **every rule an agent must remember
becomes either a machine check that fails loudly or a one-command tool.**

## W1 — CI becomes the arbiter of "done"

Today "CI green" and "suite green" mean different things, so a weak model can
merge red work with green CI.

- CI runs only 6 of ~90 test targets (`.github/workflows/ci.yml:53-65`);
  `fmt`, `decisions`, `diagnostics_coverage`, `grammar`,
  `syntax_reconciliation`, `fuzz_sema`, `ownership`, `repl`, `lsp` and ~70
  more never run in CI. Replace the cherry-pick with
  `nix develop -c scripts/agent/verify-full.sh`.
- Golden/fuzz tests self-skip when rustc/cargo/gtk are missing
  (tests/golden.rs:83,197,263) — a sandboxed pass can be vacuous. Add a
  `JET_REQUIRE_RUSTC=1` CI env var that turns skips into failures.
- Stale CI step `node tools/Tower/Tower.mjs status` (ci.yml:44) reads the
  frozen legacy board. Point at `Tower/tower.mjs` or delete.
- `tools/perf/ci-perf-check.sh` self-describes as a CI check but is unwired.
  Wire it. `docs/reference/versioning.md` references a `release.yml` that
  does not exist; write it.
- Nightly workflow: `fuzz_sema` with rotating seed (today fixed seed 42,
  N=50 — effectively one-shot, tests/fuzz_sema.rs:36-41), N≥1000, plus the
  ungated-`unsafe` grep applied to fuzz-generated programs (I1 is currently
  checked on the examples corpus only, tests/golden.rs:228-252).
- Versioned git hooks: `scripts/githooks/` + `core.hooksPath` set in the
  flake shellHook. Pre-push runs the capability-claim ledger and fast doc-sync
  tier (`truthfulness`, `decisions`, `diagnostics_coverage`) through the Nix
  dev shell; any failed gate stops the push.

## W2 — close the invariant enforcement holes

| Invariant | Hole | Machine check to add |
|---|---|---|
| I3 codegen is dumb | no check at all (currently true by luck) | truthfulness check: zero `Diagnostic::` calls in jet-codegen |
| I6 zero external crates | hardcoded 9-crate allowlist (tests/truthfulness.rs:333-367); a new crate is invisible | enumerate `crates/` dir; every exemption carries a decision ID cross-checked against the ratified record |
| I7 keyword ↔ decision ID | parser skips constants with no decision comment (tests/decisions.rs:264 `continue`) | fail on any `pub const KW_*`/sigil without an adjacent ratified-decision comment |
| I4 snapshots | `UPDATE_EXPECT=1` blesses all 516 fixtures blind | scoped bless (`UPDATE_EXPECT=<name>`) or bless-to-`.new` + diff review |
| Formatter safety | idempotence test misses dropped tokens — the exact bug that silently corrupted code for months (tests/fmt.rs:1435) | lossless corpus check: `parse(fmt(x)) == parse(x)` token equality over all examples + ui fixtures; closes the class permanently, no per-feature test needed |
| Registry quality | coverage test checks row presence, not the what/why/fix body; `ACKNOWLEDGED_GAPS` lists grow silently | per-entry format validator + exclusion-count ratchets |

## W3 — mechanical dev tooling (fast, safe loops for weak models)

- **Single-fixture runs.** One `#[test]` loops all 516 ui fixtures
  (tests/diagnostic_snapshots.rs:22) and one loops all 339 golden examples
  (tests/golden.rs:85); first mismatch hides the rest, and every tweak costs
  a full run. Add `JET_UI_FILTER`/`JET_GOLDEN_FILTER` env filters,
  collect-all-failures, and unified diffs instead of assert blob dumps.
- **Golden bless path.** `expected/*.out` has no update mechanism at all —
  agents hand-copy from panic text. Add `JET_UPDATE_GOLDEN=1`.
- **ICE tooling.** `jet self devtools reduce <file.jet>` (delta-debug minimizer;
  oracle = front-end accepts, rustc rejects, or diagnostic-code match) and
  `jet self devtools ice-report` (bundle source + generated Rust + rustc stderr +
  versions). Turns a P0 ICE from frontier-level hand-reduction into a
  mechanical step. `--emit-rust` already exists as the raw ingredient.
- **Scaffolders.** `jet self devtools new-example <topic>/<name>` and `new-ui
  <name>` that create fixture + expected output + remind the registry row.
- **Mass migration.** `jet inspect codemod` rejects everything but single renames
  (Source/CmdCodemod.rs:76-77), yet memory-model v5 S1–S9 migrations are
  ahead. Add batch pattern-rewrite over examples/ + tests/ui/, plus
  `check-fixture-paths` validating path-embedding fixtures (the known
  moving-examples trap).
- **Dev shell.** Add `jq`, `gh`, `fd`; move the `/tmp/nix-shell.*` cleanup
  and `df /tmp` check into the flake shellHook and verify-full preamble so
  every harness gets them, not just Claude Code's SessionStart hook.

## W4 — make the code safe for a weaker maintainer

- **`include!` splicing, ~180 files** (e.g. jet-codegen TIR subset.rs:14-23):
  spliced fragments share one invisible scope; rust-analyzer degrades; an
  agent edits one fragment blind to siblings. Convert mechanically to real
  `mod` + `pub(crate)`. Behavior-neutral, highest structural win.
- **Assert layer.** 11 `debug_assert!` in the whole workspace, zero in sema.
  A wrong sema edit becomes a codegen ICE at best, a silent miscompile at
  worst (ice_regressions b5–b7 are exactly this class). Add debug_asserts at
  the sema→TIR handoff (types resolved, no Unknown defaults) and an
  `ice!(span, "…")` macro in jet-foundation that stamps the I2 banner —
  plus a grep test banning bare `panic!` in compiler crates.
- **Canvas/js.rs: 5298 lines of JavaScript inside one Rust raw string** —
  unchecked by anything, zero comments. Extract to real `.js` assets via
  `include_str!` + smoke tests (playwright harness already exists).
- **jet-jit lower_ctx.rs: 22 comment lines / 1866** — the hairiest lowering
  logic in the repo with no map. Doc-comment pass tying each region to its
  TIR variant and the R12 parity contract.
- **TIR quadruple coupling** (subset gate ↔ emit ↔ lower ↔ interpreter) is
  memory-dependent; a missed arm = runtime ICE. Restructure to exhaustive
  `match` on the TIR enum in each consumer so a new variant breaks the build
  everywhere at once.
- **jetpack: 443 unwraps** in a user-facing tool — sweep to diagnostics.
- Encode the invisible traps at their point of use: header comments in
  Prelude/Core.rs (include_str embedding; golden greps the bare word
  "unsafe" including comments) and golden.rs.

## W5 — Tower enforces the process it currently only describes

The board already has the right bones (rev checks, derived lanes, claims,
auto-advance). What is missing is the state machine around the rules agents
keep in their heads:

- **`tower brief '#N' --json`** — one blob: card + checklist + LIVE linked
  decision text + blockers + milestone/epoch criteria + a new `refs[]` field
  (spec pointers) + the agent's unread messages + open questions. Kills both
  context reconstruction and the paraphrase-a-stale-ballot failure.
- **Card checklists with a done-gate.** Cards have no checklist field; any
  phase jump is legal today (store.mjs:224-243), triage→done included. Add
  `card.checklist[]`; refuse `done` while items are unchecked. On ratify of
  a syntax decision, auto-append the post-ratification chores (Syntax.rs,
  syntax-decisions.md log, `jet self devtools grammars`, re-bless) to the card's
  checklist.
- **BALLOT FIRST becomes structural.** Add `card.gates` (none | decision
  ids), required before a card may leave planning.
- **Ballot validation on `decision add`.** Today only title is validated
  (store.mjs:292-303); gist/story/inWild/options can be empty, `rec` and
  `group` unchecked. Enforce the ballot-ready standard at write time, with a
  `--draft` escape.
- **`tower verdict`** — one command/button that records an owner
  acceptance/bounce as a ratified decision and flips the card, so a verdict
  can never be mis-filed as a log note.
- **`tower lint`** — hygiene sweep: done cards without checklist, claimed
  idle cards, missing `--by` attribution, ballot-field gaps, plus `--docs`
  mode failing when a ratified decision ID still appears in open-ballot docs.
- **Integrity guards:** ratify `outcome` must match an option key;
  ratify/activate require owner attribution (or explicit, quoted
  on-behalf-of); non-assignee writes to claimed cards warn; frozen/owner-lane
  cards refuse agent writes; card deletion must not cascade-delete ratified
  decisions (store.mjs:245-253 does today); `blockedBy` accepts decision ids;
  `release` of a building card requires `--handoff`.

## W6 — repair the knowledge layer

Contradictions and stale text a weak model will follow off a cliff:

- The designated verifier instructs the trap-prone command: jet-verify.md:12
  and tower SKILL.md:98 say `cargo test`; canonical is
  `scripts/agent/verify-full.sh`. Fix both.
- tower skill hardwires "Epoch 3 burndown" while the board is on e7 —
  parameterize scope (or add `tower next --burndown` and let the board own it).
- architecture.md R3/R5/R7 still name pre-workspace paths
  (`Source/Syntax.rs`, `codegen.rs`); AGENTS.md I6 says `Source/` where the
  compiler now lives in `crates/*`; AGENTS.md's rg hint omits `crates/`;
  AGENTS.md references a `pipeline()`/`parallel()` API that doesn't exist.
- docs/README.md launches the retired dashboard, says "R1–R7", and omits
  docs/design + docs/proposals. docs/plans/README.md (the "implementing-agent
  protocol") still mandates retired numbered examples and dead paths —
  rewrite or fold into AGENTS.md. docs/reference/errors/README.md names a
  nonexistent script; real command is `UPDATE_DOCS=1 cargo test gen_error_pages`.
- Roadmap header still says "Current epoch: Epoch 3"; completed E2 content
  sits under an "Active" heading.
- **Runbooks** (in canonical docs, not new standalone files): bless procedure
  (verify skill), add-a-diagnostic (diagnostics.md head), add-syntax
  end-to-end incl. fmt STABILITY test + grammars regen (verify skill), ICE
  triage (verify skill), FFI bridge pattern (architecture.md).
- **Migrate agent-memory knowledge into the repo** so non-Claude harnesses
  see it. tower-ballot skill already carries most of the owner profile; still
  missing: design kill criteria (no hollowed defaults, no dictated file
  structure, no invariant carve-outs), design options vary UX not palette,
  no metaphor theming in UI, frontend acceptance = full mock matrix in the
  owner's real terminal, verdicts are ballots.

## Immediate security item

Resolved in Tower: `.tower/config.json` remains tracked as public project
configuration, while auth, VAPID private material, and push subscriptions live
only in ignored `.tower/secrets.json`. Tower rejects legacy secret fields in
the public file and never migrates exposed credentials forward. Owner follow-up
remains open: rotate previously committed credentials, renew device
subscriptions, and decide whether repository-history remediation is required.

## Do-first order

1. CI runs the full suite + stale Tower path + `JET_REQUIRE_RUSTC` (W1) —
   one small change, retires the biggest failure class.
2. Doc contradiction fixes (W6 first four bullets) — hours, prevents active harm.
3. Single-fixture filters + golden bless + unified diffs (W3).
4. Formatter lossless corpus test (W2) — known past silent corruption.
5. `tower brief` + card checklist done-gate + ballot validation (W5).
6. `ice!` macro + sema assert layer + `devtools reduce` (W4/W3).
7. Invariant pins I3/I6/I7 + ratchets (W2).
8. `include!` → real modules; extract Canvas JS (W4).
9. Runbooks + memory→repo migration (W6).
10. Nightly fuzz + release.yml + versioned hooks (W1).

## Owner gates to ballot before building

- New `jet self devtools` subcommands (bless, reduce, ice-report, new-example,
  new-ui, check-fixture-paths) — CLI surface additions.
- Tower state-machine strictness (done-gate, ballot validation, owner-only
  guards) — changes the owner's own workflow; he should pick the strictness.
- CI cost: full suite per push vs. tiered (fast PR lane + full merge lane).
