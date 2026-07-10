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
