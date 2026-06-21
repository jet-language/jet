# REPL v2 polish

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c55

## What already shipped

`jet repl` (E2-M18, `Source/REPL.rs`, ~1047 lines), interpreter-backed via the
comptime tree-walker (D-REPL4=A, I2); 16 transcript tests in `tests/repl.rs`.
Working today: accumulating module (D-REPL7), `...` continuation on unbalanced
brackets (D-REPL9), `x : T = v` echo with `;` suppression (D-REPL16), meta-commands
`:quit` / `:reset` / `:load` / `:type` / `:help` (D-REPL15), fuel cap + E1801
(D-REPL-FUEL), `NO_COLOR`/`CLICOLOR` (D-REPL-COLOR), `std.io` preload note
(D-REPL-PRELOAD), and byte-identical diagnostics with the batch compiler
(D-REPL17, I4).

The header lists D-REPL8 (moves), `:run`, and `--project` as ratified, but all
three are **stubbed or incomplete** in this build.

## The remaining delta

1. **Move semantics across REPL inputs (D-REPL8=A).** Each input is sema-checked
   in a *fresh synthetic program* (`type_check_stmts` /
   `binding_stubs_src` / `accumulated_src`, `REPL.rs:467-495`) where prior
   bindings are reconstructed as fresh stubs. So a value moved in input N is
   reborn intact in input N+1 — a use-after-move that crosses inputs is **not**
   caught. Within a single input, ordinary sema already catches it.
2. **`:run` (D-REPL-FUEL=A).** Currently a stub that prints a "not wired in this
   build" note (`REPL.rs:839-849`). Should compile the accumulated session to a
   temp `.jet` file and run it natively, bypassing the fuel cap.
3. **`--project` manifest mode (D-REPL10).** The flag is parsed and threaded as
   `project_dir: Option<&str>` into `run()`/`run_transcript()`, and the
   `CommandSpec`/help text exist, but the dir is not used to load `pack.jet`
   imports — so REPL inputs can't `use` a project's modules.

## Proposed approach (worked examples)

**Moves across inputs.** Track per-binding moved/consumed state in the `Session`
so it persists between inputs. When generating the synthetic program for input
N+1, mark bindings that were moved in earlier inputs as already-consumed (e.g.
emit them so sema sees them moved, or carry a moved-set the REPL consults before
re-declaring a stub). The diagnostic is the same use-after-move sema already
emits — just made visible across the input boundary.

```
jet> val s = "hi"
s : Str = "hi"
jet> val t = s          # moves s
t : Str = "hi"
jet> print(s)
error[Exxx]: use of moved value `s`
  `s` was moved into `t` on a previous line
 fix: clone `s` before moving, or use a `view`
```

**`:run`.** Materialize the session — accumulated items + a synthetic `main` that
replays the input statements — to a temp `.jet`, then invoke the normal
`run`/`run_compile_cmd` path. Native run has no fuel cap, so loops/recursion that
trip E1801 in the interpreter execute fully.

```
jet> var n = 0
jet> loop i in 1..100_000_000 { n += i }
error[E1801]: snippet ran too long (fuel cap)  use :run to run natively
jet> :run
compiling session… running…
4999999950000000
```

**`--project`.** Load the project's `pack.jet`, resolve its module map, and make
those imports available to REPL inputs (`use app.models`, etc.), so the REPL can
explore real project code.

```
$ jet repl --project ./myapp
jet> use app.models
jet> User { name: "ana" }
User { name: "ana" }
```

## Implementation sketch — file-level touchpoints

- `Source/REPL.rs`:
  - **Moves:** add a `moved: HashSet<String>` (or per-binding state) to `Session`;
    update it after each successful input by inspecting what the input consumed
    (the interpreter already moves values — reflect that into the set). In
    `binding_stubs_src` / `type_check_stmts`, render moved bindings as consumed so
    cross-input use-after-move surfaces. Reset clears the set.
  - **`:run`:** replace the stub at `handle_meta` `"run"` — serialize
    `session.accumulated_src()` + a `main` wrapping the replayed statements to a
    temp file; call the existing compile+run entry. Mirror in `run_transcript`'s
    meta handling so it's testable.
  - **`--project`:** in `run()`/`run_transcript()`, when `project_dir` is set,
    load the manifest via `Loader::find_manifest_root` + the module loader, and
    inject the resolved import context into the synthetic programs and the
    interpreter scope.
- `Source/main.rs` — `--project` is already parsed and passed (`main.rs:441-447`);
  no dispatch change needed.
- `Source/Loader.rs` — reuse manifest + module resolution; no new loader.

## Test plan — snapshots / transcripts / examples

- `tests/repl.rs` — add transcripts:
  - cross-input use-after-move emits the move diagnostic (and the in-input case
    still does);
  - a fuel-capped loop hits E1801, then `:run` produces the full native result;
  - `:run` on an empty session is a clean no-op.
- A `--project` transcript: point `run_transcript` at a fixture project dir,
  `use` one of its modules, construct one of its types. (Add a tiny fixture
  project under `tests/` if none is reusable.)
- `examples/` — a short REPL session example if the adoption docs reference one
  (I5); transcripts are the primary contract here.

## Risks & invariant check

- **I2/I4 (interpreter == batch diagnostics):** the move diagnostic must be
  byte-identical to what the batch compiler emits (D-REPL17). The risk is the
  *synthetic-program reconstruction* producing a different span than a real
  program would; the moved-binding rendering has to land the error on the
  offending input line.
- **`:run` is a real compile (I2):** if rustc rejects the materialized session,
  that's an ICE/P0, not a user error. The temp file must be well-formed Jet built
  only from already-checked inputs.
- **Fuel boundary:** `:run` deliberately bypasses E1801. Keep the interpreter the
  default so accidental infinite loops still get capped; only `:run` is unbounded.
- **`--project` scope creep:** load imports for resolution, not the project's full
  build graph. Keep it read-only manifest+module resolution; don't pull in
  realize()/build machinery.

## Open decisions

## Status (2026-06-21)

The three documented deltas (moves across inputs D-REPL8, `:run` D-REPL-FUEL,
`--project` D-REPL10) shipped and are green. The remaining gap was the one the
card flags: a `use …` line typed in the REPL was rejected (`E0003: expected a
statement, found the keyword \`use\``) and never carried, so an alias couldn't
resolve in later inputs.

**Done in this pass (`Source/REPL.rs` + `tests/repl.rs`):**

- `use core.X as a` / `use a.{Item}` lines now classify as `InputKind::Import`,
  accumulate in `Session.import_srcs`, and prepend to every synthetic program
  (sema check, item check, `:run` materialization). `:reset` clears them.
- Cross-input resolution: an import on line N makes its alias resolve in any
  later input. The single-file checker can't register core-module aliases, so
  import-bearing checks route through the Loader bundle path
  (`run_sema_bundle`, `Check` mode, temp file) — the same path `jet check` uses.
  Import-free checks keep the cheaper `check_with_mode` (spans unchanged).
- Bad imports are rejected and not retained: unknown core module → `E1001`;
  REPL-incompatible module (`core.fs`/`core.tasks`/…) → existing `E1802`
  hard-reject (fires before import classification). 6 new transcript tests.
- No new diagnostic code introduced (reuses E1001/E1802/E0107), so no
  diagnostics.md / ui-snapshot change needed (I4 satisfied).

**Deferred fork — interpreter CoreLib runtime.** Sema now *resolves*
`math.sqrt(x)` across inputs, but the comptime tree-walker can't *execute*
core-module calls, so evaluating one inline still errors `E0956` (\`math\` can't
run at compile time yet). Running it works via `:run` (native codegen has
CoreLib — verified `print(math.sqrt(16.0))` → `4.0`). Making core calls
*interpret* inline means teaching `Source/Comptime/` all of CoreLib's runtime —
a large change touching the interpreter, not the REPL, affecting every core
module. Out of scope for c55; track as its own card if inline core execution in
the REPL is wanted.

## Original notes

**No owner decision — finishing ratified work.** All three deltas implement
already-ratified decisions: D-REPL8 (move semantics across inputs), D-REPL-FUEL
(`:run`), and D-REPL10 (`--project`). Each already has its decision ID,
`CommandSpec`/flag, and (for `:run`/`--project`) a help-text entry; the verbs and
flags are committed surface, just stubbed. Nothing here touches `Source/Syntax.rs`
or `syntax-decisions.md`. **No decision card.**
