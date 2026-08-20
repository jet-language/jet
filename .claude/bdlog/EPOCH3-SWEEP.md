# Epoch 3 closeout sweep

Owner process, set 2026-08-20: cards close on integrated implementation. Targeted
tests run once at a milestone boundary; the full suite runs once at epoch end.
Anything that would otherwise have blocked one card is collected here and resolved
in a single comprehensive pass after the cards are closed.

Nothing in this file is a reason to keep a card open. It is the epoch's one
verification and fix backlog.

## A. Deferred verification (expected to pass; prove once)

| # | What to run | Cards it clears |
| --- | --- | --- |
| A1 | `cargo test --test golden` over the whole corpus after the surface migration | #2080 c4, #2081 c4 |
| A2 | `cargo test --test diagnostic_snapshots` green, every fixture re-blessed | #2080 c8, #2081 c8, #2105 c8 |
| A3 | `cargo test --test fmt` green, including the two lossless rewrite declarations | #2080 c3, #2081 c3 |
| A4 | Default `jet run`, `--interpret` and `--release` agree on a construction-heavy example | #2080 c4, #2081 c4 |
| A5 | `cargo test --test dev_tier_parity` green | #2007 c4 |
| A6 | `cargo test --test dev_corpus_gate` reaches its assertion once sharded | #2008 c4, #2018 c2 and c6-c9 |

## B. Defects found this epoch, fixed in the sweep

| # | Defect | Evidence | Owner |
| --- | --- | --- | --- |
| B1 | Default `jet run` returns a WRONG answer for `memory/shared_transact` — from=500/to=500 instead of 200/800 — then repeats E3012 stack overflow. Interpreter and golden agree; the default tier is the odd one out. | probe at HEAD | card #2123, lane `sharedjit` |
| B2 | `jet fmt` DELETES a statement. `print("{NoDebug.{ value: 2 }:Debug}")` at `tests/ui/auto_derive_opt_out_use.jet:12` and the interpolated struct-literal comparison at `examples/features/operators/user_defined.jet:53` vanish on format. Silent source loss in a save-time tool. | fmtlaw lane, confirmed twice | needs card |
| B3 | `jet fmt` reports six files clean that `jet check` rejects with E0320/E0066: qualified variant heads, generic-module templates, inline modules, foreign-body files. The formatter certifies files its own compiler refuses. | `fmt_sweep` rewrote 0 of 6 | lane `fmtgap` |
| B4 | `(a > b) :>` parses as lambda parameters, so `fmt_if_expression_preserves_condition_parens` cannot pass. | fmtlaw lane | needs card |
| B5 | `CompilerWorkload.Edit{...}`, the ratified spelling, is rejected with E2903 by the perf role validator before formatting runs. | `tests/fixtures/compile_latency/src/run.jet` | needs card |
| B6 | Manifest role modules — `package.jet`, `kernel x { }`, `out: [Package] = []` — do not parse as ordinary Jet; about 27 corpus files fail `jet check`. Pre-existing, unrelated to the migration. | corpus sweep | needs card |
| B7 | `memory/arena_regions` times out at 30 s inside the tier battery but runs in under a second standalone on both tiers. Harness contention, not a program defect. | probe at HEAD | #2007 log |
| B8 | 217 `D-*` ids cited in comments resolve to nothing; 27 more leak into user-facing diagnostic text. | scanner | #2109, lane `citecopy` |
| B9 | `dev_corpus_gate` runs about 923 s against a 900 s budget guard, so `tests/jit_corpus_gate.txt` cannot regenerate. | measured | #2103, lane `shardgate` |

## C. Owner gates still open

| # | Question | Card |
| --- | --- | --- |
| C1 | Rename `TaskGroup` to `Group`? Evidence logged on the card: Jet types are bare in parameter position (`Path`, `DataTree`), and a domain prefix is the norm exactly where the bare word would be ambiguous (`TaskFailure`, `EncodingError`, `IOError`, `CompilerWorkload`). `task.group` is a verb — a different axis from the type name. Recommendation: keep `TaskGroup`. | #2092 |
| C2 | May an INFERRED typed head be raw? `Regex{"\d+"}` works because the head names the grammar that owns the backslashes. `re.is_match({"\d+"}, text)` has no head, so the lexer cannot know, and six sites in the regex example now name the head instead. | needs ballot |

## D. Session safety

`scripts/agent/lane-guardian.sh` runs for the duration: a working-tree snapshot
every 3 minutes into `~/.cache/jet-luna/snapshots`, and a 10 GB available-memory
floor that sheds the newest worker first. All lanes share one working tree, so
there is no multi-worktree merge ahead — the risk is two lanes writing one file,
which the snapshot interval bounds.
