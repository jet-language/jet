# #3386 — association law of the existing reduction paths

Date: 2026-09-29. Binary: jet-debug-snapshot14 via `~/.cache/jet-luna/safe-jet.sh`.

## Question

Do `fold`/`reduce` stay left folds, which paths reassociate (Float `sum`,
Float add-`fold`, `para_fold`), and is ordinary `reduce` ever reassociated for
speed? No tree-fold API is proposed.

## Sources read

- `Compiler/JetFoundation/Source/Collections.jet:237` (CollectionLaw.Fold:
  "once per item, in source order"), `:442-443` (`reduce`/`fold` both take a
  seed and an accumulator; there is no seedless reduce and no fallible fold —
  Fold failure column is "none").
- `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:678-776`:
  `FloatAddFold` is chosen only for `fold` (never `reduce`) on a Float list
  when the step lambda is literally `(acc, x) -> acc + x` with no captures or
  effects; `FloatAddParaFold` needs pure seed/step/merge adds.
- `crates/jet-codegen/src/Prelude/Core/SimdLanes.rs:683-795`: eight lanes,
  item i in lane i mod 8, seed in lane 0, finish
  `((l0+l1)+(l2+l3))+((l4+l5)+(l6+l7))`.
- `crates/jet-codegen/src/Prelude/Core.rs:466-491` and
  `Prelude/Core/ParallelKernel.rs:4-18`: `para_fold` uses fixed 64-item chunks,
  a fresh seed per chunk, source-order steps, and
  `jet_list_para_merge_tree` (adjacent pairs). Docs/spec/spec.md:2097-2104
  states the same contract.
- Tower D-FRED1 (ratified A, 2026-09-03): "Apply the same rule to `sum`,
  `fold` whose step is `+` ...".

## Evidence

Example: `Examples/features/collections/fold_association.jet`, golden
`Examples/features/expected/collections/fold_association.out`.

AOT (`safe-jet.sh build` then `.jet/build/fold_association`) printed:

```
fold: (((ab)c)d)
reduce: (((ab)c)d)
fold minus: 94
reduce minus: 94
empty: seed
singleton: (seedx)
  combine a + b
  combine (ab) + c
checked: failed: cannot combine c
sum: 0.0
fold acc + x: 0.0
reduce acc + x: 1.0
para_fold: [[0|64]|[128|192]]
```

- (a)/(b) left grouping for both `fold` and `reduce`.
- (c) empty returns the seed; singleton calls step once.
- (d) `fold`'s step cannot fail (Fold law); the failing reducer is an ordinary
  loop with automatic propagation; b and c combined in order, d never read.
- (e) `[1e16, 1, -1e16, 1]`: lane tree `(1e16+1)+(-1e16+1)` = 0.0 for `sum`
  and `fold(acc + x)`; `reduce` with the same `+` step stays left = 1.0.
- (f) 200 items: chunks start 0/64/128/192, merge `[[0|64]|[128|192]]`.

Tier status on snapshot14:

- `safe-jet.sh run` (JIT): ICE `resolved MIR Prelude symbol
  jet_list_para_fold is not registered`. Without cell (f)
  (`~/.cache/jet-test-scratch/Closer08/fa_nopara.jet`) the JIT output equals
  the AOT lines 1-12.
- `safe-jet.sh run --interpret`: E0956 "`core.list.fold()` isn't supported by
  the current evaluator yet" (even `[1,2,3].fold(100, (acc: Int, n: Int) ->
  acc - n)`). With every `.fold(` line removed
  (`fa_nofold.jet`), the interpreter prints the remaining lines identically,
  including `para_fold: [[0|64]|[128|192]]` and `reduce acc + x: 1.0`.

## Verdict

The association laws hold as documented on AOT, and every cell each tier can
run agrees with AOT. `reduce` is never reassociated; the only reassociating
paths are the ratified D-FRED1 Float add-fold/sum and `para_fold`'s contracted
merge tree. The criterion-4 three-tier match is not met: two tier defects.

## Findings to route

1. JIT: `para_fold` Prelude symbol not registered (any `para_fold` call).
2. Interpreter: List `fold` unsupported (E0956).
3. Spec ambiguity: D-FRED1 says "`fold` whose step is `+`", but the recognizer
   matches only `acc + x`; `(acc, x) -> x + acc` stays a left fold (observed on
   AOT: 1.0 vs 0.0 for the same data). Either the ballot wording or the
   recognizer should be made exact. The golden does not pin the `x + acc` line.
4. Collection-callback defect shared with #3378: an AOT `fold` whose step calls
   a user fn (`(acc, s) -> wrap(acc, s)`) fails rustc E0271 (closure returns
   `Result<String, JetErr>`); the example uses inline lambdas.

No tree-fold API or opt-in association policy is proposed; either would need
its own ballot.
