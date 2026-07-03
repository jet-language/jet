# Ask-your-codebase query engine (#115 / c27iyznj)

Vet, not a fresh design. Card asks "where can balance go negative?" — a
structural/semantic query engine over the codebase.

## Ratified already — no re-ballot

`D-ASKCODE1=B` is ratified (2026-06-28): **defer the query-DSL design ballot
to Epoch 5.** Per I8/I7 rules this repo runs on ("don't re-ballot ratified
law"), that decision stands as-is; this plan does not reopen it.

The card's own log (2026-07-02) already reconciles the timing: `D-SEMINDEX1`
(the semantic-index API this rides on) shipped early, in Epoch 3 — but the
ratified text is specifically about *when the query-DSL surface ballot
opens*, not just "wait for the index." B says that's Epoch 5. Confirmed:
prerequisite (index) ships early; the ballot itself is still gated to e5.

## Card/epoch tag mismatch (flag, not fixed — tower.json is out of scope here)

Card #115's `epoch` field is `e4`. That doesn't match its own ratified
decision (`D-ASKCODE1=B`, Epoch 5) or the epoch definitions in
`tools/Tower/tower.json` (`e4` = jetpack/jetos package-and-OS layer; `e5` =
metaprogramming/build-as-Jet — the query engine has never been e4 scope
under either epoch taxonomy). Recommend retagging the card to `e5` next time
someone with tower.json write access touches it. Not done here (write scope
for this task is plan files only).

## What's already in place for the eventual Epoch 5 start

Confirmed shipped, read-only checks against the tree:

- `D-SEMINDEX1` — `crates/jet-semindex/` (symbols/refs/types/call-graph/
  effects), `jet semindex --json`. Card `c1oixt2m`, phase `done`.
- `D-IMPACT1` — `crates/jet-impact/` (`ImpactReport::analyze`: references,
  call sites, transitive upstream callers / downstream callees over
  `SemIndex`), CLI-wired as `jet impact <symbol> <query> [--depth] [--json]`
  via the root `jet` binary (`Cargo.toml` `[[bin]] name = "jet"` depends on
  `jet-impact`), covered by `tests/impact.rs`.
- `D-CODEMOD1` referenced as a sibling dependency of D-SEMINDEX1 in
  syntax-decisions.md — not independently verified here (out of this card's
  scope; check when e5 work actually starts).

This means when Epoch 5 opens the query-DSL ballot, there's no prerequisite
engineering left to wait on — only the surface-syntax decision itself
(typed Jet closures over `core.index.SemanticIndex` vs. extending `jet
impact` with a `--where` predicate vs. a string DSL — the three options
already drafted and adversarially passed in `D-ASKCODE1`'s ballot record,
recommendation **A**: extend `jet impact --where <jet-expr>`, no new CLI
command, no DSL, no I8 violation).

## Ballot check

None to raise now. `D-ASKCODE1` is ratified; its options/recommendation
already cover the eventual e5 surface question and don't need rework — they
need only to be re-surfaced to the owner when Epoch 5 actually opens (a
scheduling action, not a new decision).

## Phase recommendation

**Not e4 work — recommend retag to e5, freeze here.** Zero implementable
e4 slice exists: the one open question (query surface syntax) is explicitly
gated to Epoch 5 by ratified decision, and the prerequisite engineering
(D-SEMINDEX1, D-IMPACT1) is already done. Nothing to plan or ballot in e4
scope; re-surface `D-ASKCODE1`'s recommendation (A) to the owner at Epoch 5
kickoff, not before.
