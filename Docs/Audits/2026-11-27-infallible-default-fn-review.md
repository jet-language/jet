# Infallible-by-default `fn` (E31/E51) — design review before build

Date: 2026-11-27. Scope: card #7054 criteria 1 and 2 (ordinary `fn` without a
written `!` rail is infallible; `run: fn()` and `#Test` keep the `Err!`
default; greenfield cutover). Status: review only. No compiler behavior changes
in this commit.

## Live mechanism (read this session)

- `Compiler/JetFoundation/Source/AST/Comptime.jet`:
  `failure_contract_from_return_type` maps an unwritten rail (`None`, or a
  success type with no `!`) to `FailureContract.Default{success, Err}`.
  `failure_contract_of_function` turns a span-less `T Never!` into
  `InferredNever` and a span-less typed error into `Inferred`.
- `Compiler/JetSema/Source/Sema/FailureSolve.jet`: `sema_failure_solve` runs a
  greatest fixpoint after every body is checked. It proves *candidates*
  infallible. A candidate (`sema_failure_inference_candidate`) has a default
  contract, is not `Stream`-returning, `#Memo` helper, comptime, job, `every`,
  inline-foreign, `gc_return`, `pre`/`post`, web-marked or external-typed, and
  is not used as a value (`address_taken`). Trait-impl methods are never
  candidates (`sema_failure_row_candidate`).
- `Compiler/JetSema/Source/Sema/FailureUnion.jet`: `sema_failure_union_infer`
  joins the typed failures of unannotated top-level functions (not methods,
  not the entry `run`), mints a `#Error enum` named after the function for
  `Err(.Case(…))` sites (`sema_failure_union_mint`), and projects the union
  onto the declaration. `sema_failure_project_infallible` projects
  `T Never!` (span-less) onto every solved function.
- `Compiler/JetSema/Source/Sema/Finalize.jet` (`sema_finalize_program`) runs
  solve → union → projection → recheck.
- An explicit `-> T Never!` caller that calls a fallible callee already reports
  `E2404` `Jet.ErrorNotPropagatable` per call site
  (`Compiler/JetSema/Source/Sema/Expressions/Control.jet`, fix text
  "The caller is declared `Never!`…"). Verified on the seed compiler with
  `s.to_int()?` and `Int.parse(s)` inside `-> Int Never!`.
- `Docs/Spec/spec.md` "Failure contracts and propagation" states the current
  rule: "`T` — a non-optional success with the implicit `Err` failure route",
  and function-typed parameters `fn(Int) -> Int` take the implicit `Err`.

So today an unwritten rail is **inferred**, not `Err`: callers see `T Never!`
when the solve proves it, an inferred union when typed failures reach it, and
the `Err` carrier otherwise. E31's complaint ("silently `T Err!`") is true only
for non-candidates and for function *types*; for most top-level functions the
compiler already infers. The friction the card targets is the written
`Never!` boilerplate the compiler's own sources carry to pin contracts at
module boundaries and for methods/values that the solve cannot see.

## Peers

- **Swift**: functions are non-throwing unless marked `throws`; `rethrows` for
  callback-generic failure. Closest to the requested rule. No inference.
- **Rust**: `fn f() -> T` is infallible; failure is a written `Result<T, E>`;
  `?` in a non-`Result` fn is error E0277 with a "change the return type"
  suggestion. Closest diagnostic model.
- **Zig**: `fn f() !T` opts in to failure with an *inferred* error set; no `!`
  means infallible. This is the strongest fit for Jet: keep the union
  inference, but make the opt-in marker mandatory.
- **Go**: failure is an explicit trailing `error` result; infallible default.

## Attacks on the proposal

1. **Union inference and minted error enums lose their trigger.** Today an
   unannotated function that writes `Err(.NotFound)` gets a minted
   `fooError` enum. Under "no rail = infallible" that body is an error. Either
   the mint moves behind a written marker (Zig-style bare `!`, which is new
   syntax and needs a ballot) or authors must write the minted name, which
   defeats the mint. **Owner gate.**
2. **Function types.** `fn(Int) -> Int` as a *type* still means implicit
   `Err` per spec. If declarations default to infallible but types default to
   fallible, a declaration `f: fn(x: Int) -> Int` and the type it is used at
   disagree in spelling — an I8 pairwise-orthogonality special case. Must be
   decided together (types infallible-by-default too, with callback failure
   via the existing `E!` generic-union law).
3. **Methods and trait methods.** The solve and union only cover top-level
   functions and inherent methods; trait declarations and trait-impl methods
   keep their written route. The new default must apply to trait method
   declarations too, or a trait method without `!` stays fallible while a
   free function without `!` is infallible (special case again).
4. **Address-taken and excluded candidates.** A function used as a value is
   excluded from the solve today, so its unwritten rail stays fallible to
   callers. A post-solve "report if fallible" lint would either false-positive
   on these (if it reuses the solve) or need its own fixpoint that ignores
   `address_taken`.
5. **`run` identity.** `run` is identified as `name == "run"` in the entry
   module (`FailureUnion.jet`). A library function named `run` must become
   infallible-by-default; only the entry `run` keeps `Err!`. `#Test` blocks
   are `Item.Test`, not `Func`, so they keep the default naturally; a helper
   `fn` inside a test module does **not** (correct per card text).
6. **Diagnostic shape.** Reusing `E2404` per call site gives N reports per
   function and a fix text that says "declared `Never!`" about a function that
   wrote nothing. Criterion 1 needs one crisp report per function (or the
   first site) that names the signature edit: add `E!` (the inferred union,
   which the union pass already computes) or `Err!`. That is a new registered
   code (I4) with a UI snapshot; never a reworded `E2404`.
7. **Bootstrap.** The rule is enforced by the stage built from the patch, so
   `Compiler/` and `Core/` must be migrated in the *same* patch or S2 fails.
   Migration to a bare `Err!` is not always valid: an inferred typed failure
   without an `impl Err.From(…)` route reports `E2402`. The automated
   migration must write the inferred union the diagnostic prints, not `Err!`.

## Scope measurement (this session)

Heuristic count of single-line `name: fn(…) … {` declarations with no `!` on
the line (ripgrep over `*.jet`; a lower bound on *unannotated* declarations,
not on *fallible* ones — the fallible subset needs the built rule to count):

|Tree|Declarations|No `!` on line|
|---|---|---|
|`Compiler/`|38746|15889|
|`Core/`|11228|1258|
|`Examples/`|2325|1979|
|`Tests/`|18222|16652|
|`Tools/`|6349|2376|

## Recommended mechanism (single source of truth)

1. In `failure_contract_from_return_type`, an unwritten rail on a declaration
   is `DeclaredNever`-like (`T Never!`), except the entry `run`; function
   types follow the same law (attack 2), ballot first.
2. Delete the infallibility solve (`sema_failure_solve`,
   `sema_failure_project_infallible`, reconcile/recheck for `T Never!`) — it
   becomes dead: the 1-in-1-out collapse I8 asks for.
3. Keep union inference only behind an explicit opt-in marker (attack 1,
   ballot: Zig-style bare `!` vs. mandatory named domain).
4. New code for "fallible body under no rail", one report per function at
   the first failure site, fix = `add `{union}!` or `Err!``, UI snapshot +
   golden on O0/O1/O2 (+web where the host supports it).

## Binding cutover list

- `Comptime.jet` contract mapping and its `#Test` rows; `failure_contract_source`
  text "implicit default Err!".
- `FailureSolve.jet`, `FailureUnion.jet` (projection paths), `Finalize.jet`
  solve/recheck block, `Contracts.jet` `unsettled` check, `Control.jet`
  `InferredNever` probe, `WebGraph.jet`, `SemIndex/Build.jet`,
  `Outputs.jet`, `OutputFixtures.jet`, `ReturnDiagnosticFixtures.jet`.
- Registry text: `DiagnosticRows.jet` / `Prelude/Diagnostics.jet` rows that say
  "failure is implicit" (the `!`-without-domain row) and `E2404`'s infallible
  fix text; parser `E0003` text "omit the contract for implicit `Err`"
  (`JetParser/Source/Parser/Items.jet`, `Types.jet`).
- `Docs/Spec/spec.md` "Failure contracts and propagation" and function-type
  parameter paragraph; LSP hover test `HoverSignatures.jet`.
- Sweep: every fallible unannotated declaration in `Compiler/`, `Core/`,
  `Examples/`, `Tests/`, `Tools/` gains its inferred rail; every redundant
  written `Never!` is removed in the same patch.

## Verdict

The direction is sound (Swift/Rust/Zig/Go all default to infallible), but the
card cannot be delivered as a parser/sema default flip alone. It collides with
three live mechanisms (union inference + minted error types, function-type
defaults, the infallibility solve) and needs owner ballots on attacks 1 and 2
before the build. The cutover is compiler-wide and must land in one patch with
the rule for bootstrap.
