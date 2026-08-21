# Epoch 3 closeout sweep

Everything deferred while cards were closing. Worked after the cards close, not before.

The rule this session ran on: close on integrated implementation evidence, batch
the proof. Targeted suites at a milestone boundary, the full suite once at epoch
end, then one comprehensive fix pass. This file is the batch.

## A. Deferred verification

| # | What | Why it was deferred | How to settle it |
|---|---|---|---|
| A1 | Full test suite | Thirty lanes share one tree; a suite run mid-wave measures a half-written tree, not the product | One run after the last lane drains |
| A2 | Golden corpus | Same | `cargo test --test golden` after A1 |
| A3 | UI snapshot suite | Several diagnostics were reworded by lanes; snapshots follow the last edit, not the first | `cargo test --test diagnostic_snapshots`, then bless what genuinely moved |
| A4 | AOT tier proofs | `rustc` is absent from the plain devshell, so every AOT row skipped rather than failed. `tests/gate_ledger.rs:611-623` skips AOT when rustc is missing and `:676-689` skips web | Re-run the tier matrices under `scripts/agent/jet-env full` |
| A5 | `jet fmt --check` over the corpus | The formatter was still being repaired while the corpus was migrating | After the fmt gaps in B3 close |
| A6 | Editor grammar drift | Regenerated once this session; several lanes have touched syntax since | `jet self devtools grammars`, then `tests/grammar.rs` |

## B. Defects found by finishing, not by testing

| # | What | Evidence | State |
|---|---|---|---|
| B1 | `jet fmt` silently deleted statements | `print("{NoDebug.{ value: 2 }:Debug}")` at `tests/ui/auto_derive_opt_out_use.jet:12` vanished on format | Needs a card |
| B2 | `jet fmt` certified files the parser rejects | Thirteen files formatted clean and failed `jet check` | Lane `corpfmt` |
| B3 | `(a > b) :>` parses as lambda params | `fmt_if_expression_preserves_condition_parens` cannot pass | Needs a card |
| B4 | `CompilerWorkload.Edit{...}` rejected E2903 | Perf role validator | Needs a card |
| B5 | Manifest role modules do not parse as ordinary Jet | ~27 files: `package.jet`, `kernel x { }`, `out: [Package] = []` | Needs a card |
| B6 | 220 decision ids cited in code exist in neither Tower nor the spec | Measured by `scripts/agent/decision-index.mjs` | Card #2109, closed |
| B7 | `E0927` shipped a structured fix with no safety grade | The diagnostic registry ICE'd on **every** compile, not just on `#Bench` | Fixed, `2b50339e2` |
| B8 | Memory folded into the positive effect row | Nine examples reported `E0740` for `Mem.Alloc` against their own ceiling. D-AUTHORITY-MEM1=B makes memory deny-only | Fixed, `fb777dee9` |
| B9 | `sqrt(1/3)` accepted, `sqrt(0.5)` rejected | Both exact. The rejected spelling is what a beginner writes | Fixed, `2b50339e2` |
| B10 | A trailing arm table in a fallible function forced value context | Four shipped examples failed `E0116`; the fix text told you to do something impossible inside an arm | Card #2128, closed |
| B11 | `#1969` composite-key comparator sits outside the codegen Prelude | `crates/jet-foundation/src/Prelude.rs:12-74`; AOT keeps a separate derived `Ord` | Open, I9 |
| B12 | `jet_keep` bypassed on JIT and ambient | `lower_ctx.rs:14184-14190`, `ambient_interp.rs:2803-2807` return the argument, so the optimizer guarantee is AOT-only | Open, I9 |
| B13 | Binary patterns run three different matchers | AOT inline scan at `control_flow.rs:1516-1584`, shared `MatchScan.rs`, interpreter refuses outright | Card #2100 |
| B14 | Stale typed-decode caller | `Prelude/CoreLib/Top/DataFlow.rs:493-495` destructures decode as `(v, _)` while the canonical signature returns `Result<Self, Vec<FieldError>>` | Open, #1161 c7 |
| B15 | `wasip2` websockets | `TargetSurface.rs:6-27` passes the target; `WsClient.rs:547-560` refuses at runtime | Open, #1914 c3/c7 |
| B16 | `tests/taskgroup_parameter_tiers.rs` never runs | Absent from `tests/suites.txt` | Open, #1564 c5 |

## C. Owner gates

| # | What | State |
|---|---|---|
| C1 | `D-ERRSIGIL1` — `?` for absence, `!` for error | Ballot live on card #2127, four options, awaiting ruling |
| C2 | `TaskGroup` → `Group` | **Resolved without the owner.** D-CONC-GROUP1=A already ratified `Group` on 2026-08-06; ballot #2092 was re-asking a settled question and is closed |
| C3 | Service runtime | **Not a gate.** D-SERVICE1=D already ratifies typed builders; a prior lane wrongly stopped for an I7 ruling and blocked five cards. Corrected and dispatched |

## D. Session safety

- `lane-guardian.sh` snapshots the tree every three minutes to `~/.cache/jet-luna/snapshots` and sheds the newest worker under a 10 GB floor. Zero memory events across the session; steady state 13 GB of 61.
- `lane-keeper.sh` holds the lane cap, recycles open cards least-recently-worked-first, and now refuses to implement a card whose ballot is still open — it had started writing code for #2127.

## E. What the epoch taught

The dominant bug class was **one fact written twice, then drifted**. Every real
fix this session deleted a copy rather than adding a branch:

- memory joined `Panic` in the existing deny-only filter instead of getting its own rule;
- `Decimal` joined `Fraction` at the existing crossing instead of getting its own path;
- the decision index is generated from Tower instead of hand-maintained beside it.

The second lesson is about proof. Three separate changes this session type-checked
and would have shipped broken: the `Decimal` crossing was checker-only until a
verification pass caught that AOT emitted a symbol nobody had written. **A change
that sema accepts is not a change the tiers can run**, and I9 is the invariant that
keeps catching it.
