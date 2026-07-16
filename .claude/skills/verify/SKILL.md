---
name: verify
description: Verify a Jet compiler/stdlib change in THIS repo — the project-specific checklist (scoped card proof, major-push closeout, fresh-binary smoke test, snapshot/golden/formatter checks, /tmp trap). Use before claiming any card or change done, or when asked to verify.
---

# Verify a change in the Jet repo

## Environment sanity (before trusting ANY failure)

- `df -h /tmp` — if near full, `rm -rf /tmp/nix-shell.*` and re-run; a full
  tmpfs causes phantom ENOSPC failures unrelated to your change.
- Use `scripts/agent/jet-env`; it uses nix-direnv's cached environment when
  available. `full` selects browser/FFI/VM tooling.

## Test strategy

- **Per card:** scoped targeted tests only —
  `scripts/agent/jet-env cargo test --test <name>`. Fresh Sol then Terra
  reviewers inspect the diff and re-run the relevant proof before close.
- **Major-push closeout:** only the orchestrator runs
  `scripts/agent/jet-env full scripts/agent/verify-full.sh`, once on the push's
  closeout or blocking card. It uses a repo-local `TMPDIR` and normal test
  parallelism. CI also runs the full suite.
- Do not use global `-- --test-threads=1` for completion proof. Use it only for
  a targeted race reproduction after a parallel failure.

## Adversarial review gate

Every completed change has one implementer and two reviewers. Both reviewers
start fresh and receive only the diff, acceptance criteria, relevant authority
and invariants, and test evidence. They assume the patch is wrong and seek
concrete bugs, missed paths, false-green tests, invariant breaks, stale
decisions, scope drift, duplicate mechanisms, and orphaned work. They never
implement.

Run a Sol reviewer first. The implementer fixes every material finding and Sol
rechecks those fixes. Then run an independent Terra reviewer on the resulting
patch. The implementer fixes its material findings and Terra rechecks them.
Record both identities, model/effort, reviewed commit or diff, findings,
resolutions, and rerun evidence in Tower/PR handoff. Reviewer approval alone is
not completion evidence; the orchestrator checks the integrated result.

## Blessing snapshots and generated docs

Blessing accepts a reviewed behavior change; it is never a way to make red
tests disappear.

1. Run the focused test without an update variable and read the complete diff.
   Confirm every changed byte follows the ratified behavior and diagnostic voice.
2. Build a fresh binary before using devtools: `scripts/agent/jet-env cargo build`.
3. Preview supported snapshot targets with
   `scripts/agent/jet-env jet self devtools bless <target> --dry-run`. Then update only the
   named target with `scripts/agent/jet-env jet self devtools bless <target>`.
   For generated error pages, use
   `scripts/agent/jet-env env UPDATE_DOCS=1 cargo test --test gen_errors gen_error_pages -- --nocapture`.
4. Inspect `git diff` immediately. Revert unrelated churn; never bulk-accept a
   diagnostic code, wording, path, span, or generated grammar you cannot explain.
5. Re-run the same focused test with no update variable. Snapshot output and
   generated files must now be clean and stable.

### Run and update one fixture

Filters are repository-relative substring matches and fail when they match
nothing. Keep the test name in the command so an error-fixture selector does not
also invoke the lint-fixture test, or vice versa.

```sh
scripts/agent/jet-env env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
scripts/agent/jet-env env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  UPDATE_EXPECT=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
```

`UPDATE_EXPECT=<name>` must match exactly one selected fixture.
`UPDATE_EXPECT=1` is the explicit bless-all mode; use it only after reviewing
every printed diff. Lint snapshots use the same workflow with a
`tests/ui_lint/...` filter and the `lint_snapshots` test.

Golden examples use their own filter and update switch:

```sh
scripts/agent/jet-env env JET_GOLDEN_FILTER=examples/features/basics/hello.jet \
  cargo test --test golden examples_compile_and_run -- --nocapture
scripts/agent/jet-env env JET_GOLDEN_FILTER=examples/features/basics/hello.jet \
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
5. Run `scripts/agent/jet-env cargo build`, then
   `scripts/agent/jet-env jet self devtools grammars`; inspect every generated editor section.
6. Update `docs/spec/spec.md` and the implementation/log entry in
   `syntax-decisions.md`. Run focused parser, formatter, UI, golden, and grammar
   tests before card review.

## ICE triage

1. Check `/tmp`, build a fresh compiler, and reproduce through
   `scripts/agent/jet-env ./target/debug/jet`. Record source, command, exit 101, generated Rust path,
   and complete banner. A missing rustc/linker/library is a tool or user
   diagnostic; only generated-code rejection or an impossible compiler state is
   an ICE.
2. Minimize while preserving the same oracle:
   `scripts/agent/jet-env jet self devtools reduce <file.jet>` (or `--code EXXXX` for a
   diagnostic regression). Re-run the minimized file to confirm it still fails.
3. Bundle durable evidence with
   `scripts/agent/jet-env jet self devtools ice-report <minimized.jet>`. Keep the original
   source when minimization removes context needed to understand the bug.
4. Find the first broken invariant: front-end acceptance (sema), TIR lowering,
   Rust emission, or build/ICE classification. Fix the owning layer; never teach
   codegen to use rustc as a semantic checker and never expose raw rustc output as
   a user diagnostic.
5. Add the smallest regression fixture proving the former ICE is now either
   accepted end to end or rejected by a Jet diagnostic. Run its focused test plus
   the relevant rustc-agreement/golden target, then Sol and Terra card reviews.

## Runtime smoke test (always, for compiler changes)

1. `scripts/agent/jet-env cargo build` — the dev-shell `jet` execs
   `target/debug/jet`, so a stale build silently tests old code.
2. Run a real program: `scripts/agent/jet-env jet run examples/features/basics/hello.jet`
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

## Maintainer devtools (`jet self devtools`, hidden namespace, D-DEVTOOLS1=A)

- `scripts/agent/jet-env jet self devtools grammars` — regenerate editor grammar GENERATED sections from `Syntax.rs`.
- `scripts/agent/jet-env jet self devtools reduce <file.jet> [--code EXXXX]` — minimize an I2 or named-diagnostic reproduction.
- `scripts/agent/jet-env jet self devtools ice-report <file.jet>` — bundle source, generated Rust, stderr, and versions under `.jet/ice-report/`.
- `scripts/agent/jet-env jet self devtools new-example <topic>/<name>` — scaffold a golden example pair.
- `scripts/agent/jet-env jet self devtools new-ui <name>` — scaffold a UI fixture and snapshot pair.
- `scripts/agent/jet-env jet self devtools check-fixture-paths` — reject missing embedded fixture paths.
- `scripts/agent/jet-env jet self devtools bless [target...] [--dry-run]` — update a named supported snapshot target. `tests/devtools.rs` pins this command surface.

## Traps

- Moving/renaming examples breaks path-embedding fixtures (panic-span
  .err.out, parallel_scan counts, hardcoded stem lists).
- "New diagnostics" reminders after a build agent finishes are stale
  mid-build snapshots — confirm with
  `scripts/agent/jet-env cargo build` and
  `scripts/agent/jet-env cargo test --no-run`.
- Unused-code warnings may be in-progress features — verify before removing.
