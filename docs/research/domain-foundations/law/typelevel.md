# typelevel

## Ratified

- **S33 / D-GENERIC-CALL1=A** — generic types use `Type<Args>`; explicit `call<T>(…)` is allowed on every generic free, namespaced, and method call, with no Rust-style `::<T>`. — `docs/spec/syntax-decisions.md:807-815`
- **S45 / D-LIB2** — generic functions/types use bounds such as `fn largest<T: Comparable>(…)` and `struct Pair<T>`; multi-trait bounds are lists; “no `where`” and “no higher-kinded types.” — `docs/spec/syntax-decisions.md:817-820`
- **S76 / D-FIXARR1** — `[T#N]` is a compile-time-length refinement lowered to a “real stack array”; it widens one-way to `[T]` by copy, `.map` preserves `N`, and length-changing operations are rejected. — `docs/spec/syntax-decisions.md:825-828`
- **D-GENMOD1 / D-GENMOD2 / D-GENMOD-VALUE1=A** — generic modules accept type parameters in `<…>` and typed value parameters in `(…)`; values are immutable Tier-0 comptime `Bool`, `Int`, `Char`, `String`, or fieldless enum values. `[T#capacity]` is the narrowly approved layout slot. — `docs/spec/syntax-decisions.md:1656-1666`
- **D-GENMOD-VALUE1=A** — value expressions must finish in the Tier-0 pure comptime interpreter; no defaults, inference, packs, implicit conversions, runtime/effectful state, or “general const-generic/type-computation surface” is opened. — `docs/spec/syntax-decisions.md:1668-1686`
- **D-CAPBUNDLE1 / D-CAPPLANE1=A** — capability rules expose one concept on every type; distinct-only `#Numeric` exposes `+ - * /` and ordering “for the same type,” and `Usd + Eur` remains a type error. — `docs/spec/syntax-decisions.md:864-880`
- **D-OPDEF1** — existing symbols dispatch through ordinary hooks `Add.add`, `Sub.sub`, `Mul.mul`, `Div.div`, `Equatable.equal`, and `Comparable.compare -> Ordering`; implementations use `impl Type.Trait`, keep the same operand type, and cannot add symbols, precedence, overload sets, or side-effect meanings. — `docs/spec/syntax-decisions.md:6507-6514`
- **D-EXT1** — methods, traits, and operators on a package’s own types are open; Tier-2 DSL blocks are stdlib-only; “Tier 3 proc macros and Tier 4 grammar/sigil changes [are] rejected — even for experts.” — `docs/spec/syntax-decisions.md:1815-1817`
- **D-QUAL3 / D-UNITLIT1** — `#UnitFamily` mints one distinct type per member; `500ms` and `12.50usd` resolve against in-scope family members, unknown suffixes are E0134, and typed construction such as `px{100}` is valid. — `docs/spec/syntax-decisions.md:897-903`
- **D-SHAPE-QUANTITY1=A / D-DIMENSION-OPEN1=D / D-DERIVED-DIMENSION-CLAIM1=A** — physical dimensions use normalized exponent maps with no runtime cost; currency stays nominal; third-party dimensions are nominal to their declaring package; derived dimensions claim an existing structural dimension. — `docs/spec/syntax-decisions.md:905-926`
- **D-LITCARRIER1=D** — numeric literal carriers and component suffixes are distinct: context still selects a literal type, `3.4f` selects `Float` without context, and `i`, `j`, `k` build components rather than carrier selectors. — `docs/spec/syntax-decisions.md:1249-1260`
- **D-UNIFYLIT1=A** — one head names the language/domain; current domain text heads are `SQL`, `HTML`, `Sh`, and `[U8]`, while “User-defined prefixes remain deferred.” — `docs/spec/syntax-decisions.md:1294-1301`
- **S57 / D-META-STAGE1=B / D-META-CONST1=A / D-META-EFFECT1=A** — compile time has one `@` mark belonging to the name at every mention; computable compile-time values are legal wherever constants are legal; compile-time/runtime share one effect model, with Tier 2 requiring `#Impure` and `--gate impure=allow`. — `docs/spec/syntax-decisions.md:2671-2708,7263-7366`
- **D-META-ONE1=A / D-META-REG1=A / D-META-CODE1=A** — compiler rules are Jet declarations in one shared registration table, and generated code is real Jet code entering ordinary semantic checking. — `docs/spec/syntax-decisions.md:7267-7279,7324-7327`

## Shipped

- Fixed-size arrays and one-way widening: `examples/features/collections/fixed_arrays.jet`.
- Generic structs/functions and generic modules with `(capacity:Int)` and `[T#capacity]`: `examples/features/types/generic_types.jet`; `examples/features/modules/generic_modules.jet`.
- Unit and carrier literals: `examples/features/types/unit_literals.jet`.
- Compile-time `@` values/blocks and arithmetic: `examples/features/comptime/comptime_core.jet` and `examples/features/comptime/comptime_block.jet`.
- User-defined ordinary operator hooks: `examples/features/operators/user_defined.jet`.

## Undecided

- Whether type-level expressions or const parameters beyond the closed generic-module values and `[T#capacity]` layout slot should be admitted, and what termination/fuel and diagnostics contract they require.
- Whether heterogeneous operator operands/results are allowed. The exact E0907 (`Div` mismatch), E0360, E0905 (`Int` not `Numeric`), and E0109 behavior is not settled by the current hook law; nor is a broader `Numeric` contract.
- Whether phantom type parameters need a first-class declaration/bound/variance/coherence rule, including which traits are required or forbidden for a phantom parameter.
- How orphan/coherence boundaries work across packages (E0902), and whether an extension mechanism can add traits/operators to a foreign type without opening a forbidden grammar tier.
- Whether library-defined literal/suffix hooks should be added beyond the closed unit-family and typed-head surfaces, and what that means for E0134 and `D-UNIFYLIT1`.
- Whether macro-like facilities beyond the current Jet declaration/`@` system are needed; no current ruling opens proc-macro or grammar-extension syntax.

## Conflicts

- `[T#capacity]` is a closed, narrowly approved generic-module layout slot. D-GENMOD-VALUE1 expressly says no general const-generic/type-computation surface is opened; do not ballot an unrestricted const-generic language as if it were missing implementation.
- D-OPDEF1 requires ordinary trait hooks, same operand type, and no new symbols, precedence, overload sets, or side-effect meanings. A heterogeneous operator proposal is an amendment, not a restatement of current operator law.
- D-CAPBUNDLE1/CAPPLANE1 keeps `#Numeric` operations same-type and rejects `Usd + Eur`; widening or cross-unit behavior cannot be smuggled in as a generic Numeric implementation.
- D-EXT1 rejects Tier-3 proc macros and Tier-4 grammar/sigil changes even for experts. A macro/procedure syntax ballot would conflict with that ceiling.
- D-QUAL3 and D-SHAPE-QUANTITY1 keep unit families distinct and reject inexact/noncommensurable mixing; `D-UNIFYLIT1` supersedes prefix-first syntax and explicitly defers user-defined prefixes.
- `@` is the one compile-time marker and `comptime` is retired; proposals for a second staging keyword or parallel compile-time mechanism conflict with S57/D-META-STAGE1.
