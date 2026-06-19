# Pure-eval transitive impurity traces + rich `--pure` rendering

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c54

## What already shipped

- **E3401 — direct impure call in a `pure fn`.** `Source/Sema/Purity.rs`
  (`e3401`, `check_pure_fn`, `check_pure_expr`) walks a `pure fn`'s body and flags
  the first impure callee. The `e3401` constructor already takes a `path: &[String]`
  for a call-chain trace, but every call site passes `&[]` — so today the message
  only names the **direct** impure callee, one level deep.
- **`jet eval --pure`.** `Source/CmdDevTools.rs:run_eval` evaluates a pure program
  to stable JSON via `eval_pure_program` → `CtValue::serialize`
  (`Source/Comptime/Value.rs:144`). Output is compact one-line JSON.
- **Sandbox/determinism codes** E3402 (ambient I/O in sandboxed build) and E3403
  (non-deterministic call) ship alongside.

## The remaining delta

Three unbuilt seams:

1. **Transitive impurity chains.** When the impurity is reached through a chain of
   intermediate functions, the trace should show the whole path:
   `main → load → fetch → print`, not just `print`. The `path` parameter exists
   precisely for this and is never populated.
2. **The `--pure` enforcement in `run_eval` is ad-hoc and bypasses the snapshot
   diagnostic.** It checks only that each *top-level* fn is `is_pure` and prints a
   **hand-rolled** `error [E3401]: ...` string (`CmdDevTools.rs:344-352`) — not the
   real `e3401` Diagnostic, no span, no source box, no trace. A program where
   `main` is declared `pure` but transitively reaches `print` is **not caught here**
   the way the spec implies.
3. **Rich rendering.** `jet eval --pure` prints compact JSON. Structs and lists
   render as flat machine JSON, which is hard to read for non-trivial results.

### Why the transitive case bites only here

A `pure fn` calling another `pure fn` never needs a transitive trace: the callee's
own definition self-errors if *it* reaches something impure (`check_pure_fn`
trusts `sig.is_pure` and does not recurse). The transitive path only matters in
the **`--pure` eval context**, where the whole program is required pure but the
intermediate functions carry **no `pure` annotation**, so none of them
self-errors — there is no per-fn checkpoint to stop at, and the user needs the
full `main → … → print` chain to find the leak.

## Proposed approach (worked example)

Make `run_eval --pure` enforce purity through a **from-root walker** that reuses
`Purity.rs`, threading the `path` so `e3401` renders the chain. Replace the
hand-rolled error string with the real `e3401` Diagnostic (span + source box).

```shell
$ jet eval --pure recipe.jet
error[E3401]: `main` calls the impure function `print`
  --> recipe.jet:12:5
   |
12 |     print(summary)
   |     ^^^^^
   = main → load → render → print calls `print`, which is impure —
     the whole call chain must be pure inside `main`
 fix: mark `print` as `pure fn`, or remove the call from `main`
```

The `→`-joined trace and "the whole call chain must be pure inside" wording come
straight from the existing `e3401` `path`-branch (`Source/Sema/Purity.rs:21-27`);
the only new work is populating `path`, so the blessed snapshot should match this.

Rich rendering — pretty, indented, Jet-typed (a struct shows its name and fields,
not anonymous JSON):

```shell
$ jet eval --pure totals.jet
Report {
  total: 42,
  items: [
    Item { name: "a", qty: 3 },
    Item { name: "b", qty: 1 },
  ],
}
```

`--pure --json` keeps the existing compact stable JSON for machine consumers; the
pretty form is the default human view.

## Implementation sketch — file-level touchpoints

- `Source/Sema/Purity.rs`:
  - Add a from-root walker `check_pure_program_root(entry, funcs)` that, starting
    at `main` (or the eval entry), follows calls into `funcs` bodies, accumulating
    the call chain in `path`, and calls `e3401(root, impure_callee, &path, span)`
    on the first leak. Guard against cycles with a visited set.
  - Populate `path` at the `check_pure_expr` call sites that currently pass `&[]`
    when the walk descends into a callee body.
- `Source/CmdDevTools.rs:run_eval` — delete the ad-hoc `impure_fns` loop and the
  hand-rolled `error [E3401]` print; call the new root walker and render its
  Diagnostics through `render_all_colored` (real span + source box, I4).
- `Source/Comptime/Value.rs` — add `CtValue::render_pretty()` (indented,
  Jet-typed struct/enum/list/map output) next to the existing `serialize()`.
  Keep `serialize()` for `--json`.
- `Source/main.rs` eval dispatch — select pretty vs JSON by a `--json` check
  (already parsed globally); pass through to `run_eval`.

## Test plan — snapshots / transcripts / examples

- `tests/pure.rs` — add a transitive fixture (`main → a → b → print`); assert the
  E3401 message shows the full `→` chain and the correct innermost span. Add a
  no-leak fixture to confirm pretty output.
- `tests/ui/` — **re-bless the E3401 snapshot**: the message text and the presence
  of the trace line change (product copy, I4). Verify against the
  `docs/spec/diagnostics.md:538` format, then `UPDATE_EXPECT=1`.
- `examples/` — a pure recipe that evaluates to a struct-with-list result, with
  expected pretty output (I5); a sibling `--json` expected output.
- `docs/spec/diagnostics.md` — update the E3401 row to note the call-chain trace
  is shown for transitive leaks.

## Risks & invariant check

- **I4 (snapshot-pinned diagnostics):** E3401's text changes; the ui snapshot must
  be re-blessed and the diagnostics.md row updated in the same change. The `path`
  branch in `e3401` already produces the chain wording — confirm it matches the
  blessed format.
- **I2/I3:** purity stays a sema concern; codegen untouched. The from-root walker
  is checking, not "try and see."
- **Cycles / fan-out:** the walker needs a visited set to avoid infinite recursion
  on mutually recursive pure functions, and should report the **shortest** chain to
  the leak so the trace stays readable.
- **Rendering ambiguity:** pretty output is a *display* format, not the stable
  contract. Keep `serialize()`/`--json` byte-stable; only the human view changes.

## Open decisions

No new user-facing **syntax**. The transitive trace reuses the existing E3401
code and its already-written `path` wording; rich rendering is an output format,
not language surface. Nothing here touches `Source/Syntax.rs` or
`syntax-decisions.md`.

One borderline CLI/output choice — whether pretty rendering is the **default**
human view (with `--json` for the compact stable form) or stays opt-in behind a
`--pretty` flag — is carded below. It is output shape, not language syntax. Note
`--json` is already a global flag (`Source/CLI.rs:73`, parsed in `main.rs:262`);
`--pretty` would be a new flag.

## Proposed decision card(s)

### D-EVAL1 — Default output shape for `jet eval --pure` (rec A)

`jet eval --pure` produces a value. Is the default what a person reads, or what a
machine parses?

- **Option A — pretty by default, `--json` for stable machine output.** Humans get
  indented, Jet-typed output; pipelines opt into the existing compact stable JSON.
  Reuses the global `--json` flag — no new surface.

    ```shell
    $ jet eval --pure totals.jet
    Report { total: 42, items: [Item { name: "a", qty: 3 }] }

    $ jet eval --pure totals.jet --json
    {"total":42,"items":[{"name":"a","qty":3}]}
    ```

- **Option B — JSON by default, `--pretty` to opt in.** Preserves today's exact
  behavior; machine consumers need no flag, but the common interactive case is the
  less readable one, and it adds a new `--pretty` flag.

    ```shell
    $ jet eval --pure totals.jet
    {"total":42,"items":[{"name":"a","qty":3}]}

    $ jet eval --pure totals.jet --pretty
    Report { total: 42, items: [Item { name: "a", qty: 3 }] }
    ```

**Recommendation:** A — the interactive case is the common one, pretty is the
friendlier default, and it reuses the existing global `--json` flag instead of
adding `--pretty`. Byte-stable JSON stays one flag away for pipelines.
