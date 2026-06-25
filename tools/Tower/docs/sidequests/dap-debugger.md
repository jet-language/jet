# DAP step-through debugger + adoption docs

**Status:** Step 1 SHIPPED (2026-06-25) — interpreter-backed source-level
debugger; step 2 (native DAP/lldb backend) remains.
**Card:** c52

## Step 1 — SHIPPED (2026-06-25): interpreter-backed source-level debugger

`jet debug <file>` (D-DBG1=A) ships now as a **source-level step debugger over
the existing tree-walking interpreter** (`Source/Comptime/Interpreter.rs`) — the
same engine `jet dev`/`jet repl` use, not lldb. This delivers the full D-DBG3
in-session surface (the ratified, owner-facing part) end to end:

- A `DebugHook` (`Source/Comptime/Interpreter.rs`) is called before every
  statement, threading the user-function call `depth` and current function name.
- The driver (`Source/Debug.rs`) runs the `(jet)` prompt: `step`/`next`/
  `continue`/`finish` (+ `s`/`n`/`c`/`f`), `break N`, `print X`, `locals`,
  `backtrace`, `list`, `help`, `quit`. Each stop prints
  `breakpoint hit file:line in fn()`, a source window with the `<- here` caret,
  and the one-line `locals:` dump.
- Every value is rendered via `CtValue::jet_show()` — Jet terms, never generated
  Rust (I2). Std-only, no DAP/JSON crate (I6).
- It declines the same features `jet dev` can't interpret (FFI/tasks/`#Unsafe`/
  native std) with **E2203**, pointing at `jet build`/`jet run`; a mid-run
  `quit` surfaces **E2204**.
- Example `examples/features/118_debug.jet`; tests `tests/debug.rs`.

**Step 2 (still open):** the native **DAP/lldb backend** below — step-through of
the *full* native feature set (the cases E2203 declines), the D-DBG2
`--raw-frames` expert view, and editor DAP wiring. Its command surface is
already ratified (D-DBG3) and unchanged; only the native backend remains. No new
owner decision is required to start it.

## What already shipped (pre-existing observability foundation)

- **Source maps.** Codegen emits `// jet:source-map source=<file>` markers at the
  top of every generated Rust file (`Source/Codegen/mod.rs:98,271`). Panic reports
  carry Jet file + line directly via `jet_panic` / `jet_panic_rich`, so runtime
  output is already in Jet terms (I2), not generated-Rust terms.
- **Rich panics (E3001).** `panic` / `require` / bounds failures print the Jet
  file, line, function name, a source-line context box, and (debug builds only)
  safe local values (D-OBS1/D-OBS2). Covered by `tests/observe.rs`.
- **`?` error-return traces (E3002).** Each `?` that re-raises appends a
  Zig-style frame (`error propagated from: {fn} ({file}:{line}) via ?`).

E3001/E3002 are runtime reports, not compile-time diagnostics
(`docs/spec/diagnostics.md:492`).

## The remaining delta

Full source-level **step** debugging over the Debug Adapter Protocol: set
breakpoints on Jet lines, step in/over/out, inspect locals and the call stack —
all in Jet terms. Plus an adoption-story doc that shows a real debugging session
end to end. Today there is no way to pause execution; the only runtime
observability is post-mortem (panic) or propagation (`?`).

## Proposed approach (worked example)

Debug the native build directly with the platform debugger (lldb/gdb) wrapped by
a thin Jet DAP adapter. Codegen already emits a one-marker source map; extend it
to a **line table** (Jet line → generated Rust line) so the adapter can translate
breakpoints and stack frames both ways. The adapter is a separate `jet debug`
binary path that speaks DAP on stdio to the editor and drives lldb/gdb under the
hood — the compiler stays a verifier (I2/I3); no debugger logic leaks into sema
or codegen beyond the line table.

```
$ jet debug examples/features/05_loops.jet
# editor sets a breakpoint on loops.jet:7
breakpoint hit  loops.jet:7  in main()
   6 |   var total = 0
   7 |   loop i in 1..n {        <- here
   8 |     total += i
locals:  n = 5   total = 0   i = 1
(jet-dbg) step
   8 |     total += i
locals:  n = 5   total = 0   i = 1
```

The editor (Zed/VS Code) sees Jet files and Jet line numbers throughout; lldb and
the Rust intermediate are never surfaced.

## Implementation sketch — file-level touchpoints

- `Source/Codegen/mod.rs` — replace the single `jet:source-map` marker with a
  structured line table: for each emitted Rust statement, record `(jet_line,
  rust_line)`. Emit as trailing `// jet:line <rust>=<jet>` comments or a sidecar
  `<file>.jetmap` JSON. Keep codegen dumb (I3): it only records spans it already
  has, no debug-specific transforms.
- New `Source/Debug/` module (std-only, I6):
  - `adapter.rs` — DAP stdio loop: `initialize`, `launch`, `setBreakpoints`,
    `stackTrace`, `scopes`, `variables`, `continue`, `next`/`stepIn`/`stepOut`.
  - `linemap.rs` — load the line table; translate Jet↔Rust line both directions.
  - `inferior.rs` — spawn + drive lldb (`-batch`/MI-style) via pipes; map
    breakpoints, read frames and locals, format values in Jet terms.
- `Source/main.rs` / `Source/CLI.rs` — add `jet debug <file>` dispatch + a
  `CommandSpec`; build in debug mode (so D-OBS2 safe-locals are live) then attach.
- `Source/CmdCompile.rs` — a debug-build entry that emits the line table and
  keeps the temp Rust + binary around for the inferior to load.
- `editors/zed/` — register the DAP adapter so Zed's debugger UI drives `jet debug`.
- `docs/guides/debugging.md` (new adoption doc) — the end-to-end session above,
  plus how breakpoints, stepping, and locals map to Jet, and the E3001/E3002
  relationship (post-mortem vs live).

## Test plan — snapshots / transcripts / examples

- `tests/debug.rs` (new) — script the adapter over a pipe with a recorded DAP
  request sequence; assert the JSON responses (breakpoint resolved to the right
  Jet line, stack frame in Jet terms, locals match). Gate on lldb presence the
  same way `tests/observe.rs` gates on `rustc` (skip when absent) so CI without a
  debugger still passes.
- `tests/observe.rs` — add a line-table assertion: emitted map round-trips
  Jet↔Rust for a known fixture.
- `examples/` — a small stepping example + an expected transcript the adoption
  doc references (I5).
- Adoption doc is prose, not snapshot-pinned, but its example transcript must be
  golden-checked against real `jet debug` output.

## Risks & invariant check

- **I2 (rustc/lldb silent to users):** the adapter must translate every frame,
  breakpoint, and value to Jet terms. Any leaked Rust path/line in editor UI is a
  P0 bug. Hardest case: stepping into stdlib/generated glue with no Jet line —
  policy: step over it transparently (don't surface a frame without a Jet line).
- **I3 (codegen dumb):** the line table is recorded spans only; no debug-driven
  codegen branches. Debug builds differ only by `cfg!(debug_assertions)`, which
  already exists for safe-locals.
- **I6 (no external crates in `Source/`):** the adapter speaks DAP JSON and drives
  lldb via process pipes using std only. No `serde`, no DAP crate.
- **Platform spread:** lldb vs gdb vs Windows differ. Phase 1 targets lldb on
  Linux/macOS; gdb and Windows are follow-ups. Note this in the doc, don't
  pretend cross-platform on day one.

## Open decisions

No new user-facing **syntax** — DAP is a tooling/protocol surface, and the line
table is an internal codegen artifact. Nothing here touches `Source/Syntax.rs` or
needs a `syntax-decisions.md` row.

The one borderline user-facing choice is the **command name / surface** for the
debugger entry point (`jet debug <file>` vs a `jet run --debug` flag vs editor-only
launch). This is CLI shape, not language syntax, but the owner names commands, so
it is carded below.

## Decision — RATIFIED (no owner decision open)

### D-DBG1 — Debugger entry point (ratified 2026-06-19 = A; D-DBG2 raw-frame policy ratified 2026-06-22)

**Ratified: option A — `jet debug <file>`** (a dedicated verb parallel to `jet run`/`jet test`,
discoverable in `jet --help`; the editor launches the same command). The options below are kept
for design history. D-OBS1/D-OBS2 (safe-locals) and D-DBG2 (`--raw-frames` expert opt-in) are
also ratified — this plan is fully unblocked for implementation.

- **Option A — `jet debug <file>` (ratified).** A dedicated verb, parallel to `jet run` /
  `jet test`. Discoverable in `jet --help`; the editor launches the same command
  under the hood.

    ```shell
    $ jet debug examples/features/05_loops.jet
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
    ```

- **Option B — `jet run --debug <file>`.** No new verb; debugging is a mode of
  `run`. Fewer top-level commands, but couples a long-running interactive session
  to the "build and run to completion" verb, and the flag is easy to miss.

    ```shell
    $ jet run --debug examples/features/05_loops.jet
    breakpoint hit  loops.jet:7  in main()
    ```

- **Option C — editor-only, no terminal verb.** The DAP adapter is an internal
  binary the editor spawns; there is no documented terminal command. Smallest CLI
  surface, but no terminal-first debugging and harder to script/test.

    ```shell
    # nothing to type — press the editor's "Debug" button
    ```

**Recommendation:** A — a dedicated `jet debug` verb mirrors `jet run`/`jet test`,
shows up in `--help`, and is scriptable/testable; the editor drives the same path.
