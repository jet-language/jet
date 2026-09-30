# `jet check` scaling on large single units (2026-09-30)

Findings for card #3661. Plans and state live on the card; this note records
what was measured and why each change was made.

## Setup

- Binary: `~/.cache/jet-test-scratch/jet-debug-snapshot23` (debug build,
  built 2026-09-30 04:54, before the changes below), run through
  `~/.cache/jet-luna/safe-jet.sh` (6 GB cap) with `JET_RECEIPT_BYPASS=1`.
- Units: slices of `Compiler/Bootstrap/sources.list` built with
  `~/.cache/jet-luna/JetCompilerRunPath/slice.mjs`. Prefixes were cut at
  source-file boundaries. The Jetpack NixEval area was assembled with
  `Jetpack/Bootstrap/assemble.mjs check`.
- Profiling: `perf` is not installed. Profiles came from a gdb sampler
  (`~/.cache/jet-luna/CheckerSpeed/pmp.py`), which interrupts the process
  every 0.5–1 s and records every thread's stack. With full DWARF, gdb itself
  needs about 3.6 GB. With `-readnever` it needs 0.7 GB but loses inlined
  frames.

## Growth curve before the changes

| Unit | Lines | Wall | Peak RSS | Problems |
|---|---:|---:|---:|---:|
| Jetpack NixEval (17 sources) | 5,631 | 25.8 s | 0.40 GB | 67 |
| front prefix 12.5% | 5,636 | 33.2 s | 0.92 GB | 22 |
| parser slice | 14,909 | 83.9 s | 1.67 GB | 23 |
| front prefix 25% | 21,363 | 47.1 s | 1.10 GB | — |
| front prefix 50% | 30,980 | 143.0 s | 1.90 GB | 45 |
| front prefix 75% | 44,100 | 471.3 s | 2.77 GB | 604 |
| front slice (72 sources) | 57,965 | 705.9 s | 4.50 GB | — |

Between 31k and 58k lines, the cost per line rises from 4.6 ms to 12.2 ms.
Memory grows about 2.4x for 1.9x the lines. The front slice now finishes; on
2026-09-28 it ran out of memory at 8 GB after 39 minutes. The 115k-line sema
slice was not rerun, because at this slope it would exceed the 6 GB cap.
NixEval (195 s on 2026-09-26) is no longer the bottleneck.

## Hot spots

Sample counts come from the parser slice (219 samples on the busy thread) and
from the first 292 samples of the 75% prefix taken with `-readnever`.

1. **Every non-path `if` subject re-lexes the whole unit.** In the parser
   slice, 38 of 43 lexer samples came from
   `dispatch_subject_key <- dispatch_subject <- dispatch_condition_values`
   (19% of busy time). The L0514 adjacent-dispatch lint called
   `Lexer::lex(source)` on the full 0.6–6 MB unit for each non-path subject,
   and it evaluated each side of `==` twice. Its companion
   `statement_source_end` (3 samples) also lexed the whole unit. Cost:
   guards × unit size.
2. **Comptime call-closure walks rebuild a graph of the whole program.**
   `Purity::reachable_func_names` built a reverse edge map over every
   function body and then ran `project_reachability` on it.
   `comptime_stage_roots` called it once for every call or ident expression
   inside comptime-using functions (20 samples, 9%). Cost: sites × program.
3. **Two call-graph walks per immutable binding.**
   `implicit_fold_reaches_memoized_function` and
   `implicit_fold_reaches_contracted_function` ran eagerly for every
   non-mutable binding (11 samples). Their results were only used once an
   optional fold was otherwise eligible. Cost: bindings × reachable code.
4. **`NameLedger::body_snapshot` cloned the whole ledger for each checked
   body.** That includes every declaration, alias, and reference so far
   (7 samples, and deallocation in `check_module_bodies`). Cost: bodies ×
   unit.
5. **`type_contains_observable_clock` walked the type graph again for every
   `==`, interpolation, and copy** (8 samples). It had no memo across
   queries, unlike the other structural questions in `NominalWalk`.
6. **View-summary fixed point.** The loop always ran `view_jobs.len() + 1`
   full rounds, even after it converged. Cost: V² body checks. The compiler
   slices have almost no view-returning functions, so this did not show in
   the profiles.
7. **Comptime fragment lowering is whole-program per evaluation.** In the 75%
   prefix, 81 of 118 busy samples (69%) sat in `lower_mir_fragment_program ->
   optimize_mir_program -> verify_mir_legality` (`dominator_map`,
   `validate_function`). For every evaluation, `lower_mir_fragment`
   (`crates/jet-codegen/src/Codegen/TIR/mod.rs`) clones and lowers every
   struct in the module and every checked nominal, then optimizes and
   legality-checks the resulting MIR program, with a whole-program validation
   after each pass. Cost: evaluations × declared types. A full-symbol profile
   of the 50% prefix (242 samples, 121 busy) names the caller: 17 of the 22
   fragment stacks come from `eval_comptime_items ->
   evaluate_closed_value_with_imports_opts_collecting_structs_and_facts`,
   which evaluates each module-level constant. The other 5 come from
   `resolve_static_rule_products` (struct field defaults and marker
   arguments). Nearly all module constants are plain literals: 225 of 225 in
   the parser slice, 243 of 255 in the front slice and 252 of 266 in the sema
   slice. Each of them still paid for a whole-program fragment.
8. **Unit-prelude source scans** (`inject_units_prelude ->
   source_mentions_unit_member`, 17 samples in the 75% prefix, about 14%
   early in the run). This is linear, but it scans the unit once per unit
   member, and single-letter members such as `s` or `m` match almost every
   byte. It was not changed.

Other costs seen: FlowFacts clones and joins in `check_switch` grow with arms
× locals inside one function. They do not grow with unit size. Comptime
functions are checked twice, in the staged pass and again in the real pass
(42 staged against 83 real body-check samples in the parser slice). That is a
linear 1.5x factor.

## Changes (source only; not built or measured here)

| Root | Change | Files |
|---|---|---|
| 1 | `dispatch_subject_key` lexes only the subject's own text. Each side of `==` is classified once. `statement_source_end` lexes only the body's lines and falls back to the full tail only when the terminator it finds sits on the window's last newline. | `crates/jet-sema/src/Sema/CheckerCore/blocks.rs` |
| 2 | `reachable_func_names` walks forward from the roots and reads only the bodies it reaches. New `reachable_func_roots` and `reachable_func_closure` functions let `comptime_stage_roots` collect every site's roots and take one closure over their union. That union equals the old per-site union. | `crates/jet-comptime/src/Comptime/Purity.rs`, `crates/jet-comptime/src/Comptime/mod.rs`, `crates/jet-sema/src/Sema/Bundle/Validation.rs` |
| 3 | One combined walk (memoized or contracted), run only after `optional_comptime_fold_is_eligible` passes. Eligibility reads the globals in place, and they are copied only for an eligible fold. | `crates/jet-sema/src/Sema/CheckerCore/bindings.rs` |
| 4 | The lookup tables of `NameLedger` sit behind a copy-on-write `Arc<NameTables>`, so `body_snapshot` is O(1). | `crates/jet-foundation/src/Names.rs` |
| 5 | A new `NominalQuery::ObservableClock` memo row. A query that answers no records the checked module's nominals it visited. `NominalWalk` shares the new `nominal_memo_holds` and `record_neutral_nominals` helpers. | `crates/jet-sema/src/Sema/NominalWalk.rs`, `crates/jet-sema/src/Sema/CheckerItems.rs` |
| 6 | The view-summary loop stops once a round's results equal the previous round's. The published cells then hold the same values, so the next round would repeat. | `crates/jet-sema/src/Sema/Bundle/Validation.rs` |
| 7 | A module constant whose checked initializer is a plain literal (an unsuffixed `Int`, optionally negated, a `Bool`, a `Char`, or a string without interpolation) takes its value directly. Only the remaining constants go through the fragment evaluator. The checker step still runs first, so diagnostics are unchanged. | `crates/jet-sema/src/Sema/Registration/Items.rs` |

Not done for root 7:

- Sharing lowered declarations across evaluations would not give identical
  results as the code stands. `fragment_declaration_items` prunes each type's
  methods to the expression's reachable set, and the item list is seeded from
  the enum and struct literals in the expression. The hot frames are also
  per-function MIR checks, not declaration lowering.
- The roughly 14 table constants per unit (for example the diagnostic and
  core-call registries), struct field defaults, and optional folds in
  `check_binding` still build one whole-module fragment each.
- Comptime functions are still checked twice. The staged pass runs in
  deferred mode with an empty checked-function table, so its results cannot
  stand in for the real pass.

## Commands to measure after a build

Run each check alone. The slice units live under `~/.cache/jet-luna/CheckerSpeed/`.

```
cd ~/.cache/jet-luna/CheckerSpeed
for d in parser fp-0.5 fp-0.75 front sema; do
  (cd $d && JET=/run/current-system/sw/bin/time JET_RECEIPT_BYPASS=1 SAFE_JET_TIMEOUT=2400 \
    ~/.cache/jet-luna/safe-jet.sh -f "WALL %e s  USER %U  MAXRSS %M KB" \
    /path/to/new/jet check unit.jet 2>&1 | tail -n 3)
done
(cd nixeval/check/project && JET=/run/current-system/sw/bin/time JET_RECEIPT_BYPASS=1 \
  ~/.cache/jet-luna/safe-jet.sh -f "WALL %e s  MAXRSS %M KB" /path/to/new/jet check src/jetpack.jet 2>&1 | tail -n 3)
```

Profile (0.7 GB of gdb overhead):

```
cd ~/.cache/jet-luna/CheckerSpeed/fp-0.75 && PMP_OUT=$PWD/../after.samples PMP_INTERVAL=1 \
  JET=/nix/store/q0blg6512mzczxmjk40whhfc57p5p545-gdb-17.1/bin/gdb JET_RECEIPT_BYPASS=1 SAFE_JET_TIMEOUT=3000 \
  ~/.cache/jet-luna/safe-jet.sh -readnever -q -batch -x ../pmp.py --args /path/to/new/jet check unit.jet
node ../agg.mjs ../after.samples 60; node ../callers.mjs ../after.samples 'lower_mir_fragment_program$' 8
```

The problem counts should stay the same: 23 for the parser slice, 45 for the
50% prefix, and 604 for the 75% prefix.

## Memory and second-pass roots (snapshot26)

`sp-0.625` (74,671 lines) on `jet-debug-snapshot26`: 658 s, peak RSS 5.05 GB.
RSS over time came from `pmp.py` plus a VmRSS column; heap growth was
attributed with a gdb script that breaks on glibc `sysmalloc` and records the
stack at each new VmData high-water step (`GLIBC_TUNABLES=glibc.malloc.arena_max=1`,
full DWARF, so only units up to about 2 GB fit beside gdb in the 6 GB cap).

| Time | RSS | Cause |
|---|---:|---|
| 0–38 s | 0.9 GB | parse, then 26 s of unit-prelude scans |
| 40–44 s | 2.5 GB | comptime phase: `comptime_states` item copy, `eval_items` copy, fragments for the table constants |
| 271 s | 3.8 GB | `check_module_bodies`: `comptime_stage_jobs` and `raw_eval_funcs` each cloned every function, and both stayed alive for the whole body pass |
| 377 s | 5.0 GB | `view_jobs` cloned every function before keeping only the view-returning ones |

Changes (source only, not yet measured):

| Root | Change | Files |
|---|---|---|
| Body-pass copies | Stage jobs, the reachability table and view-summary jobs borrow functions. Only the bodies that are actually checked get cloned. | `crates/jet-sema/src/Sema/Bundle/Validation.rs`, `crates/jet-comptime/src/Comptime/mod.rs` |
| Checked-text literals | Borrow the module function table instead of cloning it for each literal. | `crates/jet-sema/src/Sema/CheckerInfer/expr.rs` |
| Unit prelude | One pass records identifiers, unqualified identifiers, and sorted runs after digits or `from_`. Each catalog member is then a lookup. | `crates/jet-sema/src/Sema/Bundle/Units.rs` |
| Guard extents | `statement_source_end` lexes growing line windows. The previous window check nearly always fell back to a full-tail lex, because a synthetic terminator sits at the last token's end. A window is accepted once the terminator's deciding token and the two tokens after it end before the window's last newline. A `.`, `(` or `|` decider falls back to the full tail. | `crates/jet-sema/src/Sema/CheckerCore/blocks.rs` |
| `build_cx_items` | Secret-containing types come from one reachability pass, not one walk per type. The existing trait impls come from one set, not an item scan per type and trait. | `crates/jet-codegen/src/Codegen/Context.rs`, `crates/jet-codegen/src/Codegen/Imports.rs` |

Still open: `ct_funcs` (an unchecked copy of every function) and
`checked_ct_funcs` (a checked copy that grows during the body pass) stay alive
for the whole body pass. Every eligible optional fold, field default and table
constant still lowers a whole-module fragment, and
`checked_comptime_nominals_for_context` calls `canonical_paths` once per
module for each fold.
