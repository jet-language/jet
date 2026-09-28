# What I built

I built four small embedded-language packages: a symbolic expression builder/simplifier/differentiator, an ordered fact/rule engine, a jq-shaped JSON projection, and a formula graph with incremental dirty propagation. Each has a tiny client entry point; the implementations use only Jet enums, structs, lists, maps, pattern matches, functions, and `core.encoding.json`/`core.math`. Files: `pkg/symbolic.jet`, `pkg/rules.jet`, `pkg/filter.jet`, `pkg/formula.jet`, their four `prim_dsl_*_demo.jet` clients, `package.jet`, and defect fixtures.

# What worked

- Symbolic expression construction, simplification, and product-rule differentiation work in the default tier. `jet run .../prim_dsl_symbolic_demo.jet` printed `expr=((2.0 * x) + 0.0)`, `simplified=(2.0 * x)`, and `derivative=((0.0 * x) + (2.0 * 1.0))`.
- Finite enum/struct pattern matching, typed records, ordered rule declaration, and visible write access work. Native `jet build .../prim_dsl_rules_demo.jet` succeeded; the executable printed `[bonus:score=10, cap:skip, award:score=11]`.
- Dynamic JSON values and safe optional access work. Native `jet build .../prim_dsl_filter_demo.jet` succeeded; the executable printed `[3, 8]`.
- A library author can implement dependency tracking and incremental recomputation with maps/lists. Native `jet build .../prim_dsl_formula_demo.jet` succeeded; the executable printed `first total=5 touched=[a, b, total]` and `second total=11 touched=[a, b, total]`.
- Operator overloading works through an existing trait: `jet run examples/features/operators/user_defined.jet` printed `4,6 4,6 true true false`.
- Comptime evaluation works for pure closed code: `jet run examples/features/comptime/comptime_core.jet` printed `2.0`, `256.0`, `3.0`, `3.0`, `20.0`, `-7 + 24i`, `5.0`.
- Compile-time reflection/derive generation works: `jet run examples/features/reflection/derive_loop.jet` printed `x`, `y`, `x`, `y`, `generated`.

# Gaps

1. **defect + impossible (G1, blocks): recursive algebraic data types.** The shipped recursive enum example cannot run: `jet run examples/features/types/recursive_enum.jet` reports E0112 (`_jet_derive_equal_Expr` wants `Expr` but got `Expr<>`) and E0401 for fallible `Bool`/`Ordering`. Symbolic code therefore uses a flat postfix `[Node]` instead of a natural recursive AST. Shared by data, backend, and science libraries.
2. **defect + impossible (G2, hurts): named-payload enum pattern lowering.** `jet build .../pkg/symbolic2_demo.jet` reports an internal compiler error: typed IR does not cover statement `if` at `symbolic2.jet:9:5` (I2/R7), `crates/jet-codegen/src/Codegen/Items.rs:3409:5`. A one-struct payload wrapper works around it.
3. **call-site + boilerplate (G3, hurts): value-producing dynamic-tree filter pipelines.** The jq-shaped projection takes a 28-line `filter.jet` walker with explicit `DataTree.Object`/`Array` matches, lookups, option handling, and loops; `jet run .../prim_dsl_filter_demo.jet` returns `[3, 8]`. A jq user writes a short `.items[] | .price` pipeline. This is shared by data and backend code.
4. **boilerplate + call-site (G4, hurts): compiler-owned formula dependency graphs and dirty recomputation.** The 58-line `formula.jet` implementation repeats dependency lists, a fixed evaluation order, dirty propagation, and touched-cell tracking. It works, but every spreadsheet-like library must implement this machinery itself. Shared by data, backend, and science code.
5. **impossible (G5, hurts): user-defined syntax extension for macros/custom literals.** `jet check .../pkg/macro_attempt.jet` reports E0003 at `macro twice(x) {` (“line computes a value but doesn't do anything with it”); no macro declaration surface is accepted. Constructors/parsers, `@` comptime functions, and reflection/derive generation are the replacement.
6. **defect + impossible (G6, hurts): default evaluator coverage for map field assignment.** `jet run .../prim_dsl_formula_demo.jet` reports E0956, “`index field assign on map` isn't supported by the current evaluator yet”; the same source native-builds and runs. This blocks default-tier/live-loop execution of a natural incremental implementation.
7. **defect + impossible (G7, hurts): native compilation of the flat symbolic implementation.** `jet build .../pkg/prim_dsl_symbolic_demo.jet` reports “internal compiler error: the generated Rust did not compile,” while default `jet run` succeeds. Jet emitted no rustc diagnostic, so the exact backend construct remains unidentified.

# Friction

The four clients are 12, 6, 6, and 6 lines, but the library implementations are 157, 57, 28, and 58 lines. The symbolic 157 lines are mostly AST construction, stack evaluation, simplification, and differentiation that a symbolic DSL normally gets from an expression node/visitor runtime. The filter author repeats one match/access/loop shape per query. Rule users must construct `Rule`, `RuleCondition`, and `RuleAction` records rather than declare `when`/`then` clauses. Formula users must supply both `deps` and a scheduling order and must explicitly mark mutation with `&`.

# Defects

- Recursive `Expr` (`examples/features/types/recursive_enum.jet`) fails in both `jet run` and `jet build` with E0112/E0401.
- Named-payload enum pattern (`pkg/symbolic2.jet:9`) fails native code generation with I2/R7; wrapping the payload in a struct avoids the ICE.
- `pkg/prim_dsl_formula_demo.jet` fails default evaluation with E0956 but succeeds in the native tier.
- `pkg/prim_dsl_symbolic_demo.jet` default-runs but native build reports generated Rust compilation failure without a rustc diagnostic.
- `examples/features/comptime/comptime_parse.jet` was not usable as a comptime proof because the shipped example has stale E2417 (`String` is not an error type); `comptime_core.jet` was used instead.

# Battery notes

Not applicable: this is a primitive probe, not a critical-area probe. The four executable examples above are the battery inputs.

# Verdict

Jet is **buildable today** for small typed embedded DSLs when the author accepts ordinary constructors and explicit walkers. Finite matching, operators, comptime, reflection, JSON access, maps, and incremental state are sufficient for real prototypes. Natural recursive ASTs and named enum payload patterns are currently blocked by compiler defects. Filter/formula/rule syntax is replaceable with library code but costs substantial author boilerplate and call-site ceremony. Fix G1/G2/G6/G7 first; then consider a shared filter/formula primitive only if this ceremony remains common across areas.
