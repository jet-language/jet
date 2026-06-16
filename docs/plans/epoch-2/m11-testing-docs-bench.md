# E2-M11 — Testing, docs, and benchmarking

**Status:** draft — **blocked on D-TEST1…D-TEST4** (Group M11) and the tooling
surfaces **D-TOOL1 (doctests), D-TOOL2 (typed holes), D-TOOL4 (snapshot bless)**
(Group 19).
**Depends on:** E2-M3 (CLI/`--json`), E2-M4 (interpreter for fast test/bench and
a possible playground). Feeds E2-M9 package docs and E2-M17 GA.
**Error codes:** E29xx block; lints L29xx (claim in docs/spec/diagnostics.md).

## Goal

Make quality workflows first-class. For a language whose *product is
diagnostics*, the learning path should use real compiler feedback, not a
separate tutorial language (see docs/spec/decision-ballots.md Group 19).

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-TEST1 | Property testing | **A** — in, if a small shrinking design exists | A | OPEN — needs owner |
| D-TEST4 = D-TOOL1 | Doctests run under `jet test` | **A** — yes (I5 for user code) | A | ✅ ratified 2026-06-16 — A: doctests run under `jet test` |
| D-TOOL4 | Snapshot testing w/ one-key bless | **A** — yes | A | OPEN — needs owner |
| D-TEST2 = D-TOOL2 | `todo` typed-hole expression | **B** — defer unless small | defer | OPEN — needs owner |
| D-TEST3 | Guided learning (`jet tour`/`jet learn`) | **B** — docs-first; A if cheap | docs-first | ✅ ratified 2026-06-16 — B: docs-led learning first, `jet tour` later |
| D-TOOL3 | `jet emit --rust` expert window | — | — | ✅ ratified 2026-06-16 — A: ship gated `jet emit --rust` expert window |
| D-TOOL5 | Capability summary | — | — | OPEN — needs owner |

## Scope

- **`jet doc`** from doc comments; **doctests** — examples in docs run under
  `jet test` (D-TOOL1; I5 generalized to user code — "docs cannot lie").
- **Snapshot testing (D-TOOL4)** with one-command bless, mirroring the internal
  `UPDATE_EXPECT` workflow shipped to users.
- **Coverage** output from `jet test` in CI-readable and human-readable modes.
- **Property testing (D-TEST1)** with shrinking, *only if* a small enough design
  exists; shrinking (minimizing the failing case) is the part users love.
- **`jet bench`** with warmups, repeated runs, variance, and comparison output —
  statistically honest out of the box (hyperfine-grade), never a naive timer.
- **`todo` typed hole (D-TOOL2)** — compiles, panics at runtime, reports its
  expected type. Recommended deferred unless the design is small.
- **`jet tour`/`jet learn` (D-TEST3)** — guided in-terminal exercises where the
  diagnostics are the teacher. Docs-first; promote to a command if cheap.
- **Playground** design *if* the E2-M4 interpreter is ready (shareable,
  sandboxed) — design only this milestone unless owner promotes.

## Surface (examples)

```jet
/// Adds two ints.
/// ```
/// > add(2, 3)
/// 5
/// ```
fn add(a: Int, b: Int) -> Int = a + b;   // doctest runs under `jet test`

test "wordcount splits on whitespace" {
    expect(wordcount("a b  c")).snapshot();   // one-key bless updates it
}
```

```
$ jet bench parse
parse   1.84 ms ±0.05  (200 runs, 20 warmup)   # honest stats, scriptable --json
```

## Diagnostics to register

- **E2901** doctest output mismatch (shows expected vs actual, like the internal
  expect-test).
- **E2902** `todo` hole reached at runtime; reports the expected type (if D-TOOL2
  ratified).
- **L2901** advisory: a `test` with no assertions.

## Examples & tests

- `examples/features/46_doctest.jet` — a function whose doc example is a test.
- `tests/bench/stats.txt` — `jet bench` output shape + `--json`.
- `tests/test/snapshot_bless.txt` — bless workflow transcript.
- Coverage golden in both human and CI formats.

## Out of scope

- Mutation testing (far horizon).
- A separate tutorial language or sandboxed multi-tenant playground (D-REPL19).
- Fuzzing harness UX (E2-M17 audit uses fuzzing internally).

## Exit criteria

- Published packages get docs and tested examples automatically.
- Coverage works in CI-readable and human-readable modes.
- `jet bench` output is statistically honest and scriptable.
- The beginner learning path uses real compiler feedback, not a separate
  language.
- `nix develop -c cargo test` green.
