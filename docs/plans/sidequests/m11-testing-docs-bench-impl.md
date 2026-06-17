# Sidequest: E2-M11 — Testing, docs, and benchmarking implementation

**Plan:** `docs/plans/epoch-2/m11-testing-docs-bench.md`  
**Status:** all decisions ratified; ready to implement after M3 ✅ + M4 ✅  
**Depends on:** E2-M3 ✅ (CLI/--json), E2-M4 ✅ (interpreter for fast test/bench)

## Ratified decisions with key deltas from original recommendations

| Decision | Ratified pick | Delta from original rec |
|---|---|---|
| D-TEST1 | Ship property testing IF shrinking design is small | Was "in, if small" — same; agent must assess shrinking design first |
| D-TEST2 = D-TOOL2 | **Ship `todo` typed-hole now** | **CHANGED**: original rec was "defer unless small"; owner chose A: ship now |
| D-TEST4 = D-TOOL1 | Doctests run under `jet test` | Same as rec |
| D-TOOL3 | `jet emit --rust` expert window | Same |
| D-TOOL4 | Snapshot testing with `-u` / `--update-snapshots` | Same; flags are `-u` and `--update-snapshots` (NOT "bless") |
| D-TOOL5 | **Human-readable capability summary by default; `--capabilities-json` for tooling** | **CHANGED**: original rec was unspecified; owner chose C: human by default |
| D-TEST3 | Docs-led learning first; `jet tour` later | Same |

## Critical: D-TOOL2 ships now

`todo` typed-hole compiles and type-checks; panics at runtime with file, line, AND **expected type**.

```jet
fn compute(x: Int) -> String = todo;  // compiles; jet build succeeds
// at runtime: panic: todo at src/main.jet:1 — expected String
```

## Critical: D-TOOL5 capability summary

`jet build` prints a human-readable capability summary by default (what the binary can do: network, file I/O, unsafe tier, FFI, etc.). `--capabilities-json` emits machine-readable JSON for tooling. This goes on the normal build output, not behind a flag.

## Snapshot test flags

The flags are exactly `-u` and `--update-snapshots`. The word "bless" is intentionally avoided in user-facing text (use "update" or "accept").

## Diagnostics to register (E29xx)

E2901 (doctest output mismatch), E2902 (`todo` hole reached at runtime), L2901 (test with no assertions).

## Exit criteria

See `m11-testing-docs-bench.md`. Key: doctests run; snapshot bless works; `jet bench` is statistically honest; `todo` compiles and panics with type info; capability summary prints on build. `nix develop -c cargo test` green.
