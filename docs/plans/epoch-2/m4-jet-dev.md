# E2-M4 — `jet dev`

**Status:** ✅ implemented (D-DEV1…D-DEV4 ratified). `jet dev <file>` is the
watch/interpret loop; the dev-shell-enter job is `jet env`.
**Depends on:** E2-M3 (CLI/`--json` patterns), the M9.5 comptime interpreter
(extended here to whole programs), and the M13 LSP foundation (overlays,
incremental front end, crash policy, latency harness). Unblocks E2-M11 and
E2-M18 (REPL shares this interpreter).
**Error codes:** E22xx block (claim in docs/spec/diagnostics.md).

## Goal

Instant-feeling development without changing release semantics. Elm reactor and
Flutter hot reload prove sub-second feedback transforms a language's DX
reputation (CLI-tooling survey, distilled in docs/spec/decision-ballots.md). Phase 1 is an interpreter
loop; release builds never touch it (I2/I3 intact).

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-DEV1 | Interpreter coverage boundary | **A** — common programs; native-only set explained | A | ✅ ratified 2026-06-16 — A: interpret common programs + opt-in "try anyway" flag (no guarantees) |
| D-DEV2 | JIT in Epoch 2 | **A** — design-only note, no impl (Cranelift later, owner approval) | A | ✅ ratified 2026-06-16 — defer JIT runtime type server to Epoch 3 (`docs/plans/epoch-3/jit-runtime-type-server.md`); Epoch 2 interpreter-only |
| D-DEV3 | Save-to-diagnostic latency budget | **A** — <200ms target with a test | <200ms | ✅ ratified 2026-06-16 — A: <200ms diagnostic budget |

## Scope

- **Watch server.** Long-running process over the import/package graph; on save,
  re-check and re-run the entry file, streaming output.
- **Reuse the LSP front end** (M13): source overlays, incremental sema, crash
  policy, latency harness — do not build a second pipeline (mirror I from M18).
- **Whole-program interpreter.** Extend the M9.5 comptime tree-walker to execute
  ordinary programs where possible.
- **Differential battery.** Interpreted output must match compiled output
  **bit-for-bit** for every supported program. Divergence is a P0 bug; extend
  the `tests/comptime_diff.rs` pattern.
- **Honest boundaries (D-DEV1).** FFI, tasks/channels, native-only std modules,
  and low-level/`unsafe` code may require a full build. When the interpreter
  can't run a program, it says so plainly and names `jet build`.
- **Latency budget (D-DEV3).** A measured save-to-diagnostic budget with a CI
  test, default <200ms for the example set.
- **JIT (D-DEV2).** Design note only this epoch — likely Cranelift — with no
  implementation and explicit owner approval required before any code.

## Boundary diagnostic (example)

```
$ jet dev service.jet
watching service.jet … (Ctrl-C to stop)
note: this program spawns a task, which `jet dev` can't interpret yet.
      Showing checks live; run `jet build && ./service` to execute it.
```

## Diagnostics to register

- **E2201** program uses a feature the dev interpreter can't execute; names the
  feature and `jet build` (what/why/fix).
- **E2202** interpreter step/fuel limit hit (if D-DEV adds a fuel cap; see M18
  open question 1) — points at the likely infinite loop.

## Examples & tests

- `jet dev examples/features/31_cli.jet` watches, rechecks, reruns, streams.
- `tests/dev/diff_battery/` — N supported programs whose interpreted stdout must
  equal compiled stdout byte-for-byte.
- `tests/dev/unsupported.txt` — a task/FFI program shows the E2201 boundary note.
- A latency test asserting the budget on the example set.

## Out of scope

- JIT implementation (design note only).
- Executing tasks/FFI/`unsafe` in the interpreter.
- Any release artifact from the interpreter path (I2/I3 — hard line).
- The REPL session model (E2-M18 builds on this interpreter separately).

## Exit criteria

- [x] Save-to-diagnostic latency has a budget and a passing test
  (`tests/dev.rs::check_latency_under_budget`, <200ms; measured <1ms on
  wordcount — check-only, the diagnostic work a save does).
- [x] `jet dev` watches/rechecks/reruns/streams a real example
  (`src/main.rs::run_dev`, std-only mtime poll — I6; per-iteration work is
  `jet::interp::dev_iteration`, golden-tested).
- [x] Unsupported programs fail with a plain explanation and a `jet build`
  suggestion (E2201; `tests/dev.rs::task_program_hits_e2201_boundary`,
  `tests/dev/unsupported.txt`). Opt-in `--try-anyway` runs past the boundary
  (D-DEV1).
- [x] The differential battery proves interpreted == compiled for supported
  programs (`tests/dev.rs::interpreter_matches_compiled_binary`, 15 examples,
  byte-for-byte; mirrors `tests/comptime_diff.rs`).
- [x] No release build ever uses the interpreter/JIT path (I2/I3: the
  interpreter lives behind `jet dev` only; `jet build`/`jet run` are
  unchanged and never call `jet::interp`/`comptime::run_main`).
- [x] `nix develop -c cargo test` green.

## Implementation notes

- **One evaluator, not two.** The dev interpreter reuses the M9.5 comptime
  tree-walker (`src/comptime.rs`): the `Interp` struct gained a `sink:
  Option<&mut DevSink>` so `print`/`eprint` buffer their output in
  whole-program "dev" mode, while pure comptime mode (`sink: None`) is
  unchanged. `comptime::run_main` runs `main`'s body; `src/interp.rs` is the
  thin driver (boundary scan → run-or-explain) that the CLI and tests share.
- **JIT (D-DEV2).** Out of scope this epoch by ratification — interpreter
  only. The Epoch-3 design lives in
  `docs/plans/epoch-3/jit-runtime-type-server.md`; no code here.
- **Coverage boundary.** The interpreter runs the deterministic, pure-enough
  subset (control flow, math, strings/lists/maps, structs/enums, fan-out,
  `print`). Constructs it doesn't evaluate yet (e.g. `??`/`or`, `?`
  propagation, lambdas, `when`) surface as E0956; runtime-only features
  (tasks/FFI/`@unsafe`/native std modules) surface as E2201 before running.
