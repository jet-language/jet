# DAP step-through debugger + adoption docs

**Status:** Step 1 SHIPPED (2026-06-25) — interpreter-backed source-level
debugger. **Step 2 SHIPPED (2026-07-01)** — native lldb backend + DAP server;
card c144, phase `done`. The `## Open Owner-Q` below is now resolved — owner
added lldb to the flake devShell.
**Card:** c144 (split from c52)

**Verification (2026-07-01, step 2):** Built AND verified against a REAL live
lldb (fetched ad hoc via `nix shell nixpkgs#lldb` for this session — not yet a
flake.nix devShell input, see Open Owner-Q). Live verification caught three
real bugs that a docs-only implementation would have shipped broken:

1. **lldb never re-prints a bare `(lldb) ` prompt over a pipe.** Driven
   non-interactively it only ECHOES the command it just read
   (`(lldb) <cmd>\n<output>`); waiting for the prompt to reappear hangs
   forever. Fixed with a bogus sentinel command after every real one — its
   deterministic rejection is the only reliable "output is flushed" signal.
2. **A resume command's own stop banner is asynchronous** and can lose a race
   to the NEXT command's synchronous reply — parsing `run`/`continue`/a step's
   own returned text for frame info is unreliable (confirmed: the banner
   sometimes arrived attached to the FOLLOWING command's output instead). Fixed
   by always sending a synchronous `bt` immediately after the resume command
   and deriving position from THAT reply, never the resume's own text. The
   same raciness hit the async EXIT notification too (arrived after even the
   sentinel) — a short grace-window drain after the sentinel catches it.
3. **The debuggee inherits lldb's own stdout by default**, so a Jet `print()`
   can land byte-interleaved into the middle of the sentinel's own echo
   (confirmed: a `total is 6` print tore the sentinel in half, hanging the
   session). Fixed by redirecting the debuggee's stdout/stderr to their own
   temp files (`settings set target.output-path`/`error-path`, before the
   first `run`) and draining them into the transcript / DAP `output` events.

Also loads rustc's own Rust pretty-printer setup (`lldb_lookup.py` +
`lldb_commands` — the same two files `rust-lldb` loads) so `String`/`&str`
locals render as `"text"` instead of the raw allocator/pointer/capacity
struct dump; `Inferior::parse_locals` also strips the synthetic per-byte
children a `String` summary still appends on the same line.

New: `Source/Debug/{LineMap,Inferior,Native,Dap}.rs`, `TStmt::LineMarker` +
`emit_bundle_dbg`/`compile_for_debug` (codegen line table, gated by
`cx.debug_linemap` — off by default, zero effect on normal builds/golden
tests/the JIT tier), `jet::Debug::needs_native` (auto-dispatch off the SAME
E2203 boundary scan step 1 already had — one command, one meaning, I8),
`--raw-frames` (D-DBG2) and `--dap` CLI flags. 12 new unit tests + 5 in
`tests/debug_native.rs` (gated on lldb presence like `tests/observe.rs` gates
on `rustc` — skips clean without it). Example
`examples/features/189_debug_native.jet` (native-only via `use core.fs`) +
golden `.out`. Full regression pass green (build/debug/debug_native/golden/
observe/cli/tir/decisions/dev+JIT).

**Not verified / explicitly out of scope this pass:** the DAP JSON-over-stdio
server (`Dap.rs`) is implemented to the documented wire spec but UNTESTED
against a real editor (no VS Code/Zed available in this environment) — verify
live before wiring into `editors/zed`/`editors/vscode` launch configs. gdb and
Windows are still follow-ups (phase 1 = lldb/Linux, as scoped below).

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
   6 |   total := 0
   7 |   loop i in 1..n {        <- here
   8 |     total += i
locals:  n = 5   total = 0   i = 1
(jet-dbg) step
   8 |     total += i
locals:  n = 5   total = 0   i = 1
```

The editor (Zed/VS Code) sees Jet files and Jet line numbers throughout; lldb and
the Rust intermediate are never surfaced.

## Implementation — actual file-level touchpoints (SHIPPED 2026-07-01)

Paths below reflect the post-workspace-split layout (`crates/jet-codegen/…`,
not `Source/Codegen/…` as the sketch above originally said).

- `crates/jet-codegen/src/Codegen/TIR/mod.rs` — `TStmt::LineMarker(usize)`, a
  new TIR node `lower_stmts` (`TIR/lower.rs`) interleaves ahead of every
  lowered statement, ONLY when `cx.debug_linemap` is set. `TIR/emit.rs` turns
  it into a `// jet:line N` comment. Gated (not the single always-on marker
  the sketch proposed) because the SAME `lower_stmts`/`TStmt` feed the JIT
  tier (`crates/jet-jit`) — an always-on marker would have silently disabled
  JIT coverage for every function. `Codegen/mod.rs::emit_bundle_dbg` /
  `Codegen/Context.rs::Cx.debug_linemap` / `jet::compile_for_debug` /
  `jet-driver::Driver::compile_bundle_path_opts_dbg` thread the flag through;
  default `false` everywhere else (byte-identical to today's output).
- `Source/Debug/` (std-only, I6; PascalCase filenames per this repo's module
  convention, not the sketch's lowercase names):
  - `LineMap.rs` — parses `// jet:line N` markers back into a rust-line ↔
    jet-line table; `main_entry_line` finds `fn main`'s first real statement.
  - `Inferior.rs` — spawns + drives `lldb` over piped stdin/stdout (module doc
    there covers three live-verified lldb quirks worked around: no bare
    prompt over a pipe, async stop/exit notifications that race a follow-up
    command, and stdout interleaving with the debuggee — see the Verification
    note up top). Also loads rustc's own Rust pretty-printer files.
  - `Native.rs` — the `(jet)` terminal session, reusing D-DBG3's vocabulary.
  - `Dap.rs` — the DAP JSON-over-stdio server (reuses `Source/LSP/JSON.rs`,
    now `pub(crate)`, for the hand-rolled codec — I6, one parser not two).
  - `mod.rs` — `needs_native` (the E2203 boundary scan, shared with the
    interpreter path) decides interpreter vs native; `run_native`/`run_dap`
    are the public entry points `Source/CmdCompile.rs` calls.
- `Source/main.rs` — `debug` dispatch gained `--raw-frames`/`--dap` parsing
  and the `needs_native` auto-routing; `Source/CLISpec.rs` registers both
  flags (`FLAG_HELP` + the `debug` `CommandSpec`).
- `Source/CmdCompile.rs::run_debug_native` — compiles via `compile_for_debug`,
  builds with `BuildProfile::Debug` (full `-C debuginfo=2`) through the
  existing `build()` bridge (FFI/clinks handled identically to a normal
  build), then calls `Debug::run_native`/`run_dap`.
- `editors/zed`/`editors/vscode` DAP registration — NOT done this pass (no
  editor available to verify against); `Dap.rs` is ready but its wire
  behavior is unverified live. Named follow-up, not silently skipped.
- `docs/guides/debugging.md` adoption doc — NOT written this pass; the
  example (`examples/features/189_debug_native.jet`) and this doc's
  Verification section cover the same ground informally. Follow-up if the
  owner wants a dedicated guide.

## Test plan — actual (SHIPPED)

- `Source/Debug/LineMap.rs` / `Inferior.rs` `#[cfg(test)]` — 12 unit tests,
  no lldb needed (line-table math, lldb-text parsing against captured live
  fixtures, name-mangling round trips).
- `tests/debug_native.rs` (new, 5 tests) — codegen-only checks
  (`debug_linemap` markers present/absent) run unconditionally; the full
  native session test gates on BOTH `rustc` and `lldb` presence, same
  posture `tests/observe.rs` takes for `rustc` alone (skip, don't fail, when
  either is absent).
- `examples/features/189_debug_native.jet` + golden `.out` — a native-only
  (`use core.fs`) program, golden-tested via the normal `jet run` path (I5);
  `jet debug` on it is a manual/live check, not snapshot-pinned (an
  interactive session isn't a stdout diff).
- Full regression pass (2026-07-01): `cargo build`, `cargo test` for
  `debug`/`debug_native`/`golden`/`observe`/`cli`/`tir`/`decisions`/`dev`
  (JIT differential battery) — all green.

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

## Open Owner-Q

No **language** decision blocks step 2 — the command vocabulary, `jet debug` entry
point, and `--raw-frames` are all ratified (D-DBG1/2/3); there is no new syntax and
nothing touches `Source/Syntax.rs`. The DAP transport is hand-rolled std JSON over
stdio (no crate → I6 holds; lldb is driven via process pipes, not a linked crate).

The one borderline product call worth an owner confirm (not a code-blocking gate):

- **Native-backend tool dependency + platform matrix.** The native backend requires
  an external system debugger (lldb on Linux/macOS for phase 1; gdb + Windows are
  follow-ups). This is a runtime *tool* dependency on the user's machine — the same
  posture the owner already accepted for native/system deps via nixpkgs "if the user
  has it." Confirm: (a) lldb-required-for-native-debug is the accepted stance (with a
  clear message + the interpreter-backed step-1 debugger as the no-lldb fallback), and
  (b) phase-1 = lldb/Linux+macOS only is the right initial scope. Tests gate on lldb
  presence (skip when absent), like `tests/observe.rs` gates on `rustc`.
  **Resolved (2026-07-01):** built and verified against a real lldb (21.1.8) — the
  stance and no-lldb fallback both work as designed. Owner approved adding `lldb` to
  `flake.nix`'s `devShells.default.packages`; done — `nix develop -c which lldb`
  resolves, `tests/debug_native.rs`'s live-gated case reruns clean with no manual
  fetch/PATH workaround.

## Open decisions (history)

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
