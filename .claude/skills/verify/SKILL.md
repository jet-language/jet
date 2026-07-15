---
name: verify
description: Verify a Jet compiler/stdlib change end-to-end in THIS repo — the project-specific checklist (targeted vs full suite, fresh-binary smoke test, snapshot/golden/formatter checks, /tmp trap). Use before claiming any card or change done, or when asked to verify.
---

# Verify a change in the Jet repo

## Environment sanity (before trusting ANY failure)

- `df -h /tmp` — if near full, `rm -rf /tmp/nix-shell.*` and re-run; a full
  tmpfs causes phantom ENOSPC failures unrelated to your change.
- `nix develop -c` prints a dev-shell banner; filter it before grepping
  captured output.

## Test strategy

- **Iterating:** targeted only — `nix develop -c cargo test --test <name>`.
- **Claiming done:** the FULL suite once —
  `nix develop -c scripts/agent/verify-full.sh`. It runs `cargo test` with a
  repo-local `TMPDIR` and normal test parallelism. Run it yourself; never accept
  a sub-agent's "green".
- Do not use global `-- --test-threads=1` for completion proof. Use it only for
  a targeted race reproduction after a parallel failure.

## Adversarial review gate

Before claiming a meaningful change done, use a reviewer other than its
implementer. Meaningful: compiler semantics, safety/ownership/FFI, runtime
behavior, public contract, generated output, or more than one coherent
implementation file. The reviewer starts fresh and receives only the diff,
acceptance criteria, relevant invariants, and test evidence. Instruct it to
assume the patch is wrong and seek concrete bugs, missed paths, false-green
tests, invariant breaks, and scope drift; it must not implement.

Implementer fixes every material finding. Reviewer re-checks material fixes.
Parent inspects the review and runs final verification; reviewer green is never
completion evidence by itself. Record reviewer identity, reviewed commit/diff,
findings, and resolution in the card/PR handoff. Exempt only a one-file exact
mechanical transformation with local proof; record the exemption rationale.

## Blessing snapshots and generated docs

Blessing accepts a reviewed behavior change; it is never a way to make red
tests disappear.

1. Run the focused test without an update variable and read the complete diff.
   Confirm every changed byte follows the ratified behavior and diagnostic voice.
2. Build a fresh binary before using devtools: `nix develop -c cargo build`.
3. Preview supported snapshot targets with
   `nix develop -c jet devtools bless <target> --dry-run`. Then update only the
   named target with `nix develop -c jet devtools bless <target>`.
   For generated error pages, use
   `nix develop -c env UPDATE_DOCS=1 cargo test --test gen_errors gen_error_pages -- --nocapture`.
4. Inspect `git diff` immediately. Revert unrelated churn; never bulk-accept a
   diagnostic code, wording, path, span, or generated grammar you cannot explain.
5. Re-run the same focused test with no update variable. Snapshot output and
   generated files must now be clean and stable.

### Run and update one fixture

Filters are repository-relative substring matches and fail when they match
nothing. Keep the test name in the command so an error-fixture selector does not
also invoke the lint-fixture test, or vice versa.

```sh
nix develop -c env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
nix develop -c env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  UPDATE_EXPECT=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
```

`UPDATE_EXPECT=<name>` must match exactly one selected fixture.
`UPDATE_EXPECT=1` is the explicit bless-all mode; use it only after reviewing
every printed diff. Lint snapshots use the same workflow with a
`tests/ui_lint/...` filter and the `lint_snapshots` test.

Golden examples use their own filter and update switch:

```sh
nix develop -c env JET_GOLDEN_FILTER=examples/features/basics/hello.jet \
  cargo test --test golden examples_compile_and_run -- --nocapture
nix develop -c env JET_GOLDEN_FILTER=examples/features/basics/hello.jet \
  JET_UPDATE_GOLDEN=1 \
  cargo test --test golden examples_compile_and_run -- --nocapture
```

Golden update mode requires a filter. It classifies the process exit first,
then updates only the matching output channel that already exists: `.out` for
successful stdout, `.stderr.out` for successful stderr, or `.err.out` for an
expected runtime failure. It never creates a new expectation channel.

## Adding syntax end to end

1. Confirm the exact spelling and semantics are ratified in
   `docs/spec/syntax-decisions.md`; otherwise ballot first and stop the gated work.
2. Add every user-typeable spelling to
   `crates/jet-foundation/src/Syntax.rs` with its decision ID. Do not scatter a
   literal into lexer/parser code.
3. Add a failing parser/UI fixture first, then implement parser → sema → TIR/codegen
   in that order. New rejection paths need a registered diagnostic and snapshot;
   user-visible behavior needs an example and golden output.
4. Extend formatter coverage with a **STABILITY** assertion that formatting once
   preserves every new token and formatting twice is byte-identical. Idempotence
   alone can bless a formatter that dropped the syntax on its first pass.
5. Run `nix develop -c cargo build`, then
   `nix develop -c jet devtools grammars`; inspect every generated editor section.
6. Update `docs/spec/spec.md` and the implementation/log entry in
   `syntax-decisions.md`. Run focused parser, formatter, UI, golden, and grammar
   tests before final project verification.

## ICE triage

1. Check `/tmp`, build a fresh compiler, and reproduce through
   `./target/debug/jet`. Record source, command, exit 101, generated Rust path,
   and complete banner. A missing rustc/linker/library is a tool or user
   diagnostic; only generated-code rejection or an impossible compiler state is
   an ICE.
2. Minimize while preserving the same oracle:
   `nix develop -c jet devtools reduce <file.jet>` (or `--code EXXXX` for a
   diagnostic regression). Re-run the minimized file to confirm it still fails.
3. Bundle durable evidence with
   `nix develop -c jet devtools ice-report <minimized.jet>`. Keep the original
   source when minimization removes context needed to understand the bug.
4. Find the first broken invariant: front-end acceptance (sema), TIR lowering,
   Rust emission, or build/ICE classification. Fix the owning layer; never teach
   codegen to use rustc as a semantic checker and never expose raw rustc output as
   a user diagnostic.
5. Add the smallest regression fixture proving the former ICE is now either
   accepted end to end or rejected by a Jet diagnostic. Run its focused test plus
   the relevant rustc-agreement/golden target, then final verification once.

## Runtime smoke test (always, for compiler changes)

1. `nix develop -c cargo build` — the dev-shell `jet` execs
   `target/debug/jet`, so a stale build silently tests old code.
2. Run a real program: `nix develop -c jet run examples/features/basics/hello.jet`
   plus an example exercising the changed feature. Check actual output, not
   just exit code.

## Feature completeness (I-invariant checklist)

- New diagnostic → code in docs/spec/diagnostics.md + tests/ui snapshot (I4).
- New feature → runnable example with golden-tested output (I5).
- New syntax → entry in crates/jet-foundation/src/Syntax.rs with decision ID
  (I7), AND formatter round-trip: fmt emits it + a fmt STABILITY test
  (idempotence alone misses dropped tokens — fmt has silently corrupted
  syntax before).
- Prelude (CoreLib.rs) edits → rebuild `jet` first (include_str-embedded);
  prelude bugs surface as ICEs on generated programs, dead prelude code
  never warns.
- Generated Rust must not contain the bare word "unsafe" outside gate
  regions — golden.rs greps the substring, including comments.
- Docs match behavior: spec.md + syntax-decisions.md status.

## Maintainer devtools (`jet devtools`, hidden namespace, D-DEVTOOLS1=A)

- `jet devtools grammars` — regenerate editor grammar GENERATED sections from `Syntax.rs`.
- `jet devtools reduce <file.jet> [--code EXXXX]` — delta-debugging minimizer; default oracle is an I2 repro (front end accepts, rustc rejects); writes `<file>.reduced.<ext>`.
- `jet devtools ice-report <file.jet>` — bundles source + generated Rust + rustc stderr + jet/rustc versions under `.jet/ice-report/<stem>-<ts>/` for a bug report.
- `jet devtools new-example <topic>/<name>` — scaffolds a passing `examples/features/<topic>/<name>.jet` + `expected/<topic>/<name>.out` pair (I5 golden layout).
- `jet devtools new-ui <name>` — scaffolds a self-consistent `tests/ui/<name>.jet` + `<name>.stderr` pair (I4 snapshot layout), pre-blessed against a real (generic) diagnostic.
- `jet devtools check-fixture-paths` — greps `tests/**/*.rs` for hardcoded example/doc/fixture path literals and reports any that don't exist on disk.
- `jet devtools bless [target...] [--dry-run]` — wraps `UPDATE_EXPECT=1 cargo test --test <target>` for every `UPDATE_EXPECT`-blessable test file (cli, cross, diagnostic_snapshots, diagnostics_coverage, release_gates); `--dry-run` previews without running.

## Traps

- Moving/renaming examples breaks path-embedding fixtures (panic-span
  .err.out, parallel_scan counts, hardcoded stem lists).
- "New diagnostics" reminders after a build agent finishes are stale
  mid-build snapshots — confirm with a real `cargo build` +
  `cargo test --no-run`.
- Unused-code warnings may be in-progress features — verify before removing.
