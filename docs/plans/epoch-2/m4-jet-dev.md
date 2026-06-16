# E2-M4 — `jet dev`

**Status:** draft — **blocked on D-DEV1…D-DEV3** (Group M4).
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
| D-DEV2 | JIT in Epoch 2 | **A** — design-only note, no impl (Cranelift later, owner approval) | A | OPEN — needs owner |
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

- Save-to-diagnostic latency has a budget and a passing test.
- `jet dev` watches/rechecks/reruns/streams a real example.
- Unsupported programs fail with a plain explanation and a `jet build` suggestion.
- The differential battery proves interpreted == compiled for supported programs.
- No release build ever uses the interpreter/JIT path.
- `nix develop -c cargo test` green.
