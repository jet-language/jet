# Language-shape cross-surface conformance

E3 shipped the cross-surface match for syntax registry, parser, sema, formatter,
diagnostics, examples, and editor grammars. The package-graph depth that depends
on Epoch 4 metaprogramming is closed on Tower card `#560` in epoch `e4`, with
production workspace-driver and focused build-entry proof.

## Classification authority

| Concern | Authority |
|---------|-----------|
| User-typeable keywords / sigils | `crates/jet-foundation/src/Syntax.rs` + decision IDs |
| Decision log | `docs/spec/syntax-decisions.md` |
| Ecosystem shape proposals | `docs/proposals/ecosystem-shape.md`, archived research under `docs/archive/` |

## Surface matrix (prediction → proof)

| Surface | Proof path |
|---------|------------|
| Parser / AST | grammar regen via `jet devtools grammars`; fuzz corpus under `tests/fuzz/` |
| Sema | UI snapshots `tests/ui/*.stderr`; checker modules under `crates/jet-sema/` |
| Formatter | fmt stability suites; STABILITY fixtures |
| Diagnostics | registered codes + UI snapshots (I4) |
| Examples / goldens | `examples/features/**` + `*.expected_out` / `expected/` (I5) |
| CI change gate | `.github/workflows/ci.yml` checks the exact candidate with locked `cargo test --test grammar` and `cargo doc --workspace --no-deps` (D-CI1) |
| Package / Config | E4 card `#560`; workspace-driver and split-file package graph tests |
| Inspect / LSP / Canvas | existing focused suites; Canvas remains E8 for unfinished UI |

## CI nightly projections

D-CI1 adds CI work. It adds no user-typeable Jet form. It therefore adds no
`Syntax.rs` entry, example, diagnostic code, UI snapshot, or language golden.
The existing projections remain the source of truth:

| Surface | Source and proof |
|---|---|
| Examples and goldens | `examples/features/**` and `tests/golden.rs`; coverage uses `tests/fixtures/coverage.jet` with `coverage.text.golden` and `coverage.json.golden`. |
| Diagnostics | `docs/spec/diagnostics.md`, `tests/ui/*.stderr`, and `tests/diagnostics_coverage.rs`; the CI gate has no user diagnostic. |
| Grammar | `tests/grammar.rs`; the change gate calls the target directly. |
| Fuzzing | `tests/fuzz_sema.rs`; `FUZZ_SEED` reproduces a run and `FUZZ_VARIANTS` sets its bounded size. |
| Performance | `tools/perf/corpus.tsv`, `tools/perf/baseline.json`, `tools/perf/dashboard.sh`, and `tools/perf/ci-perf-check.sh`. |
| Audit evidence | `tools/ci/ci-evidence.sh` writes `jet.ci-evidence.v1` `candidate.txt`, `receipt.txt`, `toolchain.txt`, `command.stdout`, and `command.stderr`. `scripts/agent/verify-full.sh` keeps the Tower hygiene report beside that evidence. |

The CI evidence is a report, not a second test or language surface. A failed
command keeps its receipt and captured output. A missing report is not a pass.

## 2026-07-15 rulings

Recorded in `syntax-decisions.md`. Superseded spellings must not remain as a
second authority (I8). Migration behavior for retired role-files follows the
ratified Package law on the owning child cards.

## E4 closeout

Card `#560` closes the package-graph remainder through the production workspace
driver. Members run in dependency order with separate authority contexts; root
CLI grants do not authorize members, and explicit package/workspace grants still
apply through the same policy resolver. Focused proof lives in
`tests/build_entry.rs`, `tests/build_entry_epoch4.rs`, and `tests/build_graph.rs`.
