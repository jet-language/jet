# Metaprogramming

Compile-time code, reflection, generics, derives, attributes and build facts.

Run a lesson from the repository root, for example `jet run Examples/Metaprogramming/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Attributes

- [DeclaredTextBlock](Attributes/DeclaredTextBlock.jet) — The library rule owns checking; Jet owns the braces.
- [DSLBlocks](Attributes/DSLBlocks.jet) — SQL and HTML blocks are fixed stdlib syntax islands.
- [MaturityTags](Attributes/MaturityTags.jet) — Maturity is doc-only `#Meta` data.
- [MetaAttribute](Attributes/MetaAttribute.jet)
- [StatementAttributes](Attributes/StatementAttributes.jet)

## BuildTime

- [BuildContribution](BuildTime/BuildContribution/)
- [BuildEntryDiscovery](BuildTime/BuildEntryDiscovery/) — The package build entry lives in tools/build_entry.jet, not beside this run entry.
- [BuildWriteGenerated](BuildTime/BuildWriteGenerated/) — `write_generated` (stock off) writes the sources `build: fn` generates to `.jet/generated/<package>/` for inspection: jet build…
- [CodegenControls](BuildTime/CodegenControls.jet) — Each marker expresses one independent intent.
- [CompilerAPI](BuildTime/CompilerAPI/) — The read-only compiler value API runs in the build entry and feeds the normal generated-source path.
- [CompilerAPIPackage](BuildTime/CompilerAPIPackage/)
- [ProgrammableBuild](BuildTime/ProgrammableBuild/)

## Comptime

- [BuildFactPrecedence](Comptime/BuildFactPrecedence.jet) — Every build fact follows one source-scope and layer law.
- [BuildStamp](Comptime/BuildStamp.jet) — Provenance is a locked build input, not a wall-clock read.
- [ComptimeIf](Comptime/ComptimeIf.jet) — `comptime if` selects a branch at compile time.
- [ComptimeParse](Comptime/ComptimeParse.jet) — Comptime Result/Option evaluation.
- [ComptimeTable](Comptime/ComptimeTable.jet)
- [ComptimeTiers](Comptime/ComptimeTiers.jet) — Comptime effect tiers.
- [ComptimeUse](Comptime/ComptimeUse/) — `comptime use` imports a module for compile-time code only: its names work in a `comptime { … }` block, a `comptime fn` body, a comptime argument and a…
- [ComputedConstants](Comptime/ComputedConstants.jet) — Declaration constants use the ordinary comptime evaluator.
- [ConstantUseSites](Comptime/ConstantUseSites/) — An immutable module constant with a known value is copied into each place that reads it: a number, text, or a small record becomes a literal at the use…
- [Embed](Comptime/Embed.jet) — `embed_file` bakes a file's contents into the binary at compile time — the file is never opened at runtime.
- [Find](Comptime/Find.jet) — `find(glob)` expands a build-time glob, sorts the relative file paths, and records each matched file hash for reproducible builds.
- [FoldConstants](Comptime/FoldConstants.jet) — `:` asks for build-time evaluation as an optimization; `comptime` forces it.
- [FrozenMap](Comptime/FrozenMap.jet) — A Map constant is frozen at build time like a List constant: its entries sit in static data in key order, and every read borrows them instead of building…
- [Phase](Comptime/Phase.jet) — `$phase` reads the evaluation-site `Phase`: `Preparation` inside `comptime` code and `Runtime` in the program, on every tier.
- [PrepBlock](Comptime/PrepBlock.jet) — `comptime { … }` is the explicit shared-preparation block.
- [PrepFn](Comptime/PrepFn.jet) — `comptime fn` marks a literal hook, a method that runs while building to turn a written number into a value.
- [PrepLoop](Comptime/PrepLoop.jet) — `comptime loop` repeats declarations while building.
- [RungSpellings](Comptime/RungSpellings.jet)
- [StartupConstants](Comptime/StartupConstants.jet)
- [StartupTrap](Comptime/StartupTrap.jet) — A constant whose build-time try stops (here a `panic`) is not a build error: the program computes it at startup instead, and the same stop then ends the…
- [StaticConstant](Comptime/StaticConstant.jet) — `#Static` gives a compile-time constant a stable address.
- [TableNominalPayloads](Comptime/TableNominalPayloads.jet) — A literal `@` table may hold enum payloads that are themselves user enums, structs, lists or optionals.

## Derives

- [AutoDerivePolicy](Derives/AutoDerivePolicy/) — Auto-derive policy: Package policy sets the default.
- [FieldTypes](Derives/FieldTypes.jet) — A reflected record names its type `type`: a derive reads `field.type`, and so does `T.reflect()`.
- [MarkerDerives](Derives/MarkerDerives.jet)
- [SerdeDerive](Derives/SerdeDerive.jet)
- [UserDerive](Derives/UserDerive.jet) — user-authored derive, plus `T.reflect()` Demonstrates: derive bodies are checked Jet items with typed holes.

## Generics

- [GenericConstructorInference](Generics/GenericConstructorInference.jet) — Constructor inputs or an expected type may hide generic receiver arguments only when they determine one concrete type.
- [GenericTypes](Generics/GenericTypes.jet)
- [ResultParameterInference](Generics/ResultParameterInference.jet) — A generic function's result may use a type parameter that only the callback fixes: `map_list<T, U>` learns `U` from what the lambda returns.
- [ScalarGenerics](Generics/ScalarGenerics.jet)
- [TypedAST](Generics/TypedAST.jet) — Typed expression AST: the evaluator's result type follows the node type.
- [TypeTables](Generics/TypeTables.jet) — A generic function branches on its type parameter with an ordinary `if` table.

## Reflection

- [CallerLocation](Reflection/CallerLocation.jet) — Code-site facts (D-JAI-SITE1): a parameter whose default is `$caller` receives the place of each call that leaves it out, and `$here` is the place where it is written.
- [FactPlane](Reflection/FactPlane.jet) — One folded program reads the registered build fact plane.
- [FactReads](Reflection/FactReads.jet) — Every compiler-held plane uses one typed `@` read.
- [FactSigil](Reflection/FactSigil.jet) — The fact sigil: compiler facts carry `$`.
- [GeneratingMarker](Reflection/GeneratingMarker.jet) — A marker that writes code types its template `-[Meta.Generate]>`, the row of `Marker.expand`.
- [LayoutFacts](Reflection/LayoutFacts.jet) — Focused layout facts and full reflection share one typed model; a struct without `#Layout` reports the bytes the compiler chose for it.
- [MarkerViews](Reflection/MarkerViews.jet) — `T.reflect()` in comptime is the declared type's TypeInfo.
- [ReadOnlyLint](Reflection/ReadOnlyLint.jet) — A read-only lint is a marker whose `expand` row is `-[]>`: it reads the marked type and may warn, but writes no code.
- [Reflect](Reflection/Reflect.jet) — Aggregate reflection and the typed `@` reader consume the same registered fact carriers.
- [ReflectNested](Reflection/ReflectNested.jet) — `reflect.of(x)` reflects a nested value all the way down: a struct field's `Value` lists that struct's own fields.
- [ReflectValue](Reflection/ReflectValue.jet) — `reflect.of(x)` — the runtime reflection floor.
- [UserRuleBody](Reflection/UserRuleBody.jet) — A source rule with a body adds an ordinary method.
