# ErrorsAndEffects

Failures and how they travel, effects and authority, and contracts.

Run a lesson from the repository root, for example `jet run Examples/ErrorsAndEffects/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Contracts

- [InlineContracts](Contracts/InlineContracts.jet) — Checked `#Inline` contracts.
- [PostBreach](Contracts/PostBreach.jet) — A broken `#Post` stops in its function, the same on every tier.
- [PreBreach](Contracts/PreBreach.jet) — A broken `#Pre` stops at the call, the same on every tier.
- [PrePost](Contracts/PrePost.jet) — `#Pre`/`#Post` function contracts.

## Effects

- [AuditedGateLadder](Effects/AuditedGateLadder.jet) — All audited escapes use the same policy ladder.
- [AuthorityAllowFlag](Effects/AuthorityAllowFlag.jet) — Application authority, command-line acknowledgement: this file has no package block, so a plain run names the environment and clock reads it makes.
- [AuthorityDenied](Effects/AuthorityDenied.jet) — Application authority, a written deny: a deny wins over every grant, including `-A`.
- [AuthorityDenyFlag](Effects/AuthorityDenyFlag.jet) — Application authority, command-line denial: its golden runs it as `jet run -A --deny=Env authority_deny_flag.jet` (expected/effects/authority_deny_flag.case).
- [AuthorityPackageGrant](Effects/AuthorityPackageGrant.jet) — Application authority, written grant: a leading package block adds rights under `authority`.
- [AuthorityPartial](Effects/AuthorityPartial.jet) — Application authority, partial declaration: the written row lists Env, but the program also reads the clock.
- [AuthorityUndeclared](Effects/AuthorityUndeclared.jet) — Application authority, no declaration: a file with no package block may use every effect nothing denies.
- [CapabilitySigils](Effects/CapabilitySigils.jet) — Access sigils: the access marker rides the type — `&T` write, `&&T` move/take — and the call site mirrors it.
- [DependencyUpgrade](Effects/DependencyUpgrade/) — A dependency upgrade that does more: the package writes no effect rows, `.jet/lock` records each dependency's row, and `jet build --locked` refuses an upgrade that reads files, naming the call path.
- [Determinism](Effects/Determinism.jet)
- [EffectAuthority](Effects/EffectAuthority.jet) — `#FX(…) { … }` restricts a block to a fixed authority set: only those listed may be used inside, directly or through any call it reaches.
- [EffectDeclarations](Effects/EffectDeclarations.jet)
- [EffectGrant](Effects/EffectGrant.jet)
- [EffectHigherOrder](Effects/EffectHigherOrder.jet) — Effects flow through higher-order calls.
- [EffectLevers](Effects/EffectLevers.jet) — Expert effect levers.
- [EffectProhibition](Effects/EffectProhibition.jet) — effect prohibition -[!E]>.
- [EffectRowLayers](Effects/EffectRowLayers.jet) — Every layer that bounds effects, in one program.
- [Effects](Effects/Effects.jet) — Effect system: every function carries an inferred effect set — the ambient powers its body reaches.
- [EffectTraitBound](Effects/EffectTraitBound.jet) — A trait method may declare an effect upper bound.
- [EffectTree](Effects/EffectTree.jet) — The ten effect names (`Net`, `FS`, `DB`, …) are tree ROOTS.
- [FileScope](Effects/FileScope.jet)
- [GenericRow](Effects/GenericRow.jet) — A generic type's row belongs to each instance: what `Traced` does through its type parameter is charged to the caller that writes `Traced<Parser>`.
- [LibraryEffect](Effects/LibraryEffect.jet) — A library effect written as a type: `SQL` implements `Effect`, and the `#FX(…)` above it names the kernel effects a row that names `SQL` may do.
- [MethodReceivers](Effects/MethodReceivers.jet) — Method receivers — &self (write), &&self (consume), self (read).
- [ModuleCap](Effects/ModuleCap.jet) — A module cap: `#FX(…)` on a module bounds every function inside it, the function-row rule applied to a whole module; a deterministic offline module denies `Net`, `FS`, `Exec`, `Time` and `Rand`.
- [Pure](Effects/Pure.jet)
- [ScopedEffects](Effects/ScopedEffects.jet) — An effect argument names a scope: `-[FS.Read("ScopedEffects.jet")]>` reads that one file, a folder scope covers the files below it, and a scoped denial refuses only its folder.
- [SelfMutation](Effects/SelfMutation.jet) — Self-mutation in `&self` methods.
- [SingleUse](Effects/SingleUse.jet) — Single-use (must-consume) values.
- [SmartContext](Effects/SmartContext.jet) — Smart Context — `#Context(field: value) { … }`.
- [Taint](Effects/Taint.jet) — External values carry origin facts until a typed decode or an explicit #Scrub gate clears them.
- [TraitCap](Effects/TraitCap.jet) — A trait cap and a method bound together: `#FX(…)` above a trait bounds every implementation of its methods, and a row on one method narrows it further.
- [TypeCap](Effects/TypeCap.jet) — A type cap: `#FX(…)` on a struct or enum bounds every method of the type, in its body or in an `impl` block apart from it.

## Errors

- [AmbientErr](Errors/AmbientErr.jet)
- [CallbackFailurePassthrough](Errors/CallbackFailurePassthrough.jet) — A function passes a callback's failure through to its caller with a failure-type parameter.
- [ConvertedErrMessage](Errors/ConvertedErrMessage.jet) — After a declared `impl T -> Err`, the `??` fallback's ambient `err` is the converted default Err.
- [CoreErrorConversion](Errors/CoreErrorConversion.jet) — The standard library declares the conversion for its own error family.
- [DefaultErrEdge](Errors/DefaultErrEdge.jet) — An unhandled default Err is rendered at the program edge through the shared Prelude report.
- [DefaultErrorConversion](Errors/DefaultErrorConversion.jet) — One declared conversion rail can target the default Err.
- [DefaultErrValue](Errors/DefaultErrValue.jet) — Err is both the default error type and its constructor.
- [ErrorContext](Errors/ErrorContext.jet) — Automatic `?`-propagation trail (dev builds) + lazy `?` notes.
- [Errors](Errors/Errors.jet)
- [FallbackChain](Errors/FallbackChain.jet) — `a ??
- [FallibleArmTables](Errors/FallibleArmTables.jet) — A decision table is the value of a fallible function.
- [FallibleRun](Errors/FallibleRun.jet) — Optional success and typed failure use prefix roles.
- [FatalExit](Errors/FatalExit.jet) — A `-> Never` helper is a complete non-returning contract.
- [InferredFailureUnion](Errors/InferredFailureUnion.jet) — No function here writes a failure contract.
- [MustUse](Errors/MustUse.jet) — `#MustUse` on types, functions, and methods.
- [OrErr](Errors/OrErr.jet) — `?T` and `T !E` are two views of one outcome carrier.
- [OutcomeThreeState](Errors/OutcomeThreeState.jet) — A `User?
- [Panic](Errors/Panic.jet)
- [PartialAndNotes](Errors/PartialAndNotes.jet) — The middle states of the one outcome carrier.
- [PropagationJourney](Errors/PropagationJourney.jet) — A failure that propagates automatically reports where it started (`failed here`) and every call it passed through on the way out, origin first.
- [QqControl](Errors/QqControl.jet) — `??
- [RadixParse](Errors/RadixParse.jet) — `Int.from_radix(text, base)` is the explicit-base sibling of `Int.parse`: both return `Int ParseError!`, so bad digits or a base outside 2..=36 reach the…
- [ResultHandler](Errors/ResultHandler.jet) — One fixed exhaustive Result handler handles both payloads.
- [RollbackTrait](Errors/RollbackTrait.jet)
- [Transact](Errors/Transact.jet)
- [TypedErrorFamilies](Errors/TypedErrorFamilies.jet)
- [UnionErrorContract](Errors/UnionErrorContract.jet) — A whole-union error contract `T (E1 | E2)!` lets one callable fail with any listed error domain.
- [UnitFailureContract](Errors/UnitFailureContract.jet)
